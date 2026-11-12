import Foundation
import Observation
import UIKit

/// A downloaded song, kept in Application Support/Downloads.
struct DownloadEntry: Codable, Sendable {
  let track: TrackDTO
  var bytes: Int
  var added: Date
}

/// What the web side sees (Library › Downloads, the ↓ marks on songs).
struct DownloadState: Encodable, Sendable {
  var tracks: [TrackDTO]
  var pending: [String]
  var active: String?
  var progress: Double
  var bytes: Int
  var failed: [String]
}

/// Saves songs on the phone so they play with no internet.
///
/// One song at a time: get the audio link (same as playback), fetch it in
/// 2 MB pieces (YouTube slows down big single requests), then save the
/// artwork next to it so the lock screen has it offline too.
@MainActor
@Observable
final class Downloads {
  static let shared = Downloads()

  private(set) var entries: [String: DownloadEntry] = [:]
  private(set) var pending: [TrackDTO] = []
  private(set) var active: String?
  private(set) var progress: Double = 0
  private(set) var failed: [String] = []

  @ObservationIgnored var onChange: ((DownloadState) -> Void)?
  @ObservationIgnored private var working = false
  @ObservationIgnored private var cancelled: Set<String> = []
  @ObservationIgnored private var lastEmit = Date.distantPast

  let dir: URL = {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    var d = base.appendingPathComponent("Downloads", isDirectory: true)
    try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
    // big files the user can download again: keep them out of iCloud backups
    var rv = URLResourceValues()
    rv.isExcludedFromBackup = true
    try? d.setResourceValues(rv)
    return d
  }()

  private let session: URLSession = {
    let c = URLSessionConfiguration.default
    c.timeoutIntervalForRequest = 30
    return URLSession(configuration: c)
  }()

  private var indexURL: URL { dir.appendingPathComponent("index.json") }
  private func audioFile(_ id: String) -> URL { dir.appendingPathComponent("\(id).m4a") }
  private func artFile(_ id: String) -> URL { dir.appendingPathComponent("\(id).jpg") }

  private init() {
    if let data = try? Data(contentsOf: indexURL),
      let saved = try? JSONDecoder().decode([String: DownloadEntry].self, from: data)
    {
      entries = saved.filter { FileManager.default.fileExists(atPath: audioFile($0.key).path) }
    }
  }

  // MARK: reading

  func audioURL(_ id: String) -> URL? {
    guard entries[id] != nil else { return nil }
    let u = audioFile(id)
    return FileManager.default.fileExists(atPath: u.path) ? u : nil
  }

  func artwork(_ id: String) -> UIImage? {
    guard entries[id] != nil else { return nil }
    return UIImage(contentsOfFile: artFile(id).path)
  }

  func state() -> DownloadState {
    DownloadState(
      tracks: entries.values.sorted { $0.added > $1.added }.map(\.track),
      pending: pending.map(\.videoId),
      active: active,
      progress: progress,
      bytes: entries.values.reduce(0) { $0 + $1.bytes },
      failed: failed)
  }

  // MARK: changing

  func add(_ tracks: [TrackDTO]) {
    var seen = Set(entries.keys).union(pending.map(\.videoId))
    if let a = active { seen.insert(a) }
    for t in tracks where !seen.contains(t.videoId) {
      pending.append(t)
      seen.insert(t.videoId)
    }
    let asked = Set(tracks.map(\.videoId))
    failed.removeAll { asked.contains($0) }
    emit(force: true)
    work()
  }

  func remove(_ ids: [String]) {
    for id in ids {
      if active == id { cancelled.insert(id) }
      pending.removeAll { $0.videoId == id }
      failed.removeAll { $0 == id }
      entries.removeValue(forKey: id)
      try? FileManager.default.removeItem(at: audioFile(id))
      try? FileManager.default.removeItem(at: artFile(id))
    }
    save()
    emit(force: true)
  }

  func removeAll() {
    var ids = Array(entries.keys) + pending.map(\.videoId)
    if let a = active { ids.append(a) }
    remove(ids)
  }

  // MARK: the worker

  private func work() {
    guard !working else { return }
    working = true
    Task {
      while !self.pending.isEmpty {
        let t = self.pending.removeFirst()
        self.active = t.videoId
        self.progress = 0
        self.emit(force: true)
        do {
          let bytes: Int
          do {
            bytes = try await self.fetch(t)
          } catch is CancellationError {
            throw CancellationError()
          } catch {
            // a stale link: get a fresh one and try once more
            await YouTube.shared.forget(t.videoId)
            bytes = try await self.fetch(t)
          }
          if self.cancelled.remove(t.videoId) != nil {
            try? FileManager.default.removeItem(at: self.audioFile(t.videoId))
            try? FileManager.default.removeItem(at: self.artFile(t.videoId))
          } else {
            self.entries[t.videoId] = DownloadEntry(track: t, bytes: bytes, added: Date())
            self.save()
          }
        } catch {
          if self.cancelled.remove(t.videoId) == nil { self.failed.append(t.videoId) }
          try? FileManager.default.removeItem(at: self.dir.appendingPathComponent("\(t.videoId).part"))
        }
        self.active = nil
        self.progress = 0
        self.emit(force: true)
      }
      self.working = false
    }
  }

  private func fetch(_ t: TrackDTO) async throws -> Int {
    let s = try await YouTube.shared.stream(t.videoId)
    // so the downloaded copy is levelled like the streamed one
    if let db = s.loudnessDb { PlayerEngine.shared.rememberLoudness(t.videoId, db) }
    let wait = s.availableAt - Date().timeIntervalSince1970
    if wait > 0 && wait < 15 {
      try await Task.sleep(nanoseconds: UInt64((wait + 0.25) * 1_000_000_000))
    }
    let part = dir.appendingPathComponent("\(t.videoId).part")
    try? FileManager.default.removeItem(at: part)
    FileManager.default.createFile(atPath: part.path, contents: nil)
    let fh = try FileHandle(forWritingTo: part)
    var offset = 0
    var total = 0
    let chunk = 2_000_000
    do {
      while true {
        if cancelled.contains(t.videoId) { throw CancellationError() }
        var req = URLRequest(url: s.url)
        req.setValue("bytes=\(offset)-\(offset + chunk - 1)", forHTTPHeaderField: "Range")
        req.setValue(YouTube.safariUA, forHTTPHeaderField: "User-Agent")
        let (data, resp) = try await session.data(for: req)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 206 || http.statusCode == 200 else {
          let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
          throw YouTubeError.message("download failed (\(code))")
        }
        if total == 0 {
          if let cr = http.value(forHTTPHeaderField: "Content-Range"), let slash = cr.lastIndex(of: "/"),
            let n = Int(cr[cr.index(after: slash)...])
          {
            total = n
          } else {
            total = offset + data.count
          }
        }
        try fh.write(contentsOf: data)
        offset += data.count
        progress = total > 0 ? min(1, Double(offset) / Double(total)) : 0
        emit(force: false)
        if data.isEmpty || http.statusCode == 200 || offset >= total { break }
      }
      try fh.close()
    } catch {
      try? fh.close()
      throw error
    }
    guard offset > 10_000 else { throw YouTubeError.message("download was empty") }
    let dest = audioFile(t.videoId)
    try? FileManager.default.removeItem(at: dest)
    try FileManager.default.moveItem(at: part, to: dest)

    // artwork for the lock screen and Now Playing when offline
    if let a = t.artwork, let url = URL(string: a),
      let (data, _) = try? await session.data(from: url), let img = UIImage(data: data),
      let jpg = PlayerEngine.squared(img).jpegData(compressionQuality: 0.85)
    {
      try? jpg.write(to: artFile(t.videoId), options: .atomic)
    }
    return offset
  }

  private func save() {
    guard let data = try? JSONEncoder().encode(entries) else { return }
    try? data.write(to: indexURL, options: .atomic)
  }

  private func emit(force: Bool) {
    guard force || Date().timeIntervalSince(lastEmit) >= 0.5 else { return }
    lastEmit = Date()
    onChange?(state())
  }
}
