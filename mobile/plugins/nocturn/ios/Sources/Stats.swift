import Foundation
import UIKit

// Listening time for Nocturn Wrapped (Library › Your Stats).
//
// Counted here, in the native player, so it keeps counting with the screen
// locked. Kept on the phone only, in Application Support/stats.json:
// per day, the time listened, the time per hour of the day, and the time and
// plays per song. A "play" is 30 seconds of a song (or half of a short one).

struct StatsSong: Codable, Sendable {
  var ms: Double = 0
  var plays: Int = 0
}

struct StatsDay: Codable, Sendable {
  var ms: Double = 0
  /// Time listened in each hour of the day (0–23).
  var hours: [Double] = Array(repeating: 0, count: 24)
  var songs: [String: StatsSong] = [:]
}

struct StatsMeta: Codable, Sendable {
  var title: String
  var artist: String
  var album: String?
  var artwork: String?
}

struct StatsFile: Codable, Sendable {
  /// "2026-11-12" → that day.
  var days: [String: StatsDay] = [:]
  /// videoId → what the song is (for top-song lists).
  var tracks: [String: StatsMeta] = [:]
  var since: String?
}

@MainActor
final class ListeningStats {
  static let shared = ListeningStats()

  private(set) var data = StatsFile()
  private var lastTick: Date?
  private var lastSave = Date()
  private var dirty = false
  /// The song being listened to now, and how long it's been heard this time.
  private var currentId: String?
  private var heardMs: Double = 0
  private var counted = false

  private let url: URL = {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    return base.appendingPathComponent("stats.json")
  }()

  private static let dayFormat: DateFormatter = {
    let f = DateFormatter()
    f.calendar = Calendar(identifier: .gregorian)
    f.locale = Locale(identifier: "en_US_POSIX")
    f.dateFormat = "yyyy-MM-dd"
    return f
  }()

  private init() {
    if let d = try? Data(contentsOf: url), let f = try? JSONDecoder().decode(StatsFile.self, from: d) {
      data = f
    }
    if data.since == nil { data.since = ListeningStats.dayFormat.string(from: Date()) }
    NotificationCenter.default.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: .main) { _ in
      MainActor.assumeIsolated { ListeningStats.shared.save() }
    }
  }

  /// Called a few times a second by the player. `playing` false just resets the clock.
  func tick(_ track: TrackDTO?, playing: Bool) {
    let now = Date()
    defer { lastTick = playing ? now : nil }
    guard playing, let track, let last = lastTick else { return }
    let delta = min(1000, now.timeIntervalSince(last) * 1000)
    guard delta > 0 else { return }

    if currentId != track.videoId {
      currentId = track.videoId
      heardMs = 0
      counted = false
    }
    heardMs += delta
    let key = ListeningStats.dayFormat.string(from: now)
    let hour = Calendar.current.component(.hour, from: now)
    var day = data.days[key] ?? StatsDay()
    day.ms += delta
    if day.hours.count == 24 { day.hours[hour] += delta }
    var song = day.songs[track.videoId] ?? StatsSong()
    song.ms += delta
    let needed = min(30_000, max(5_000, (track.durationMs ?? 60_000) / 2))
    if !counted && heardMs >= needed {
      counted = true
      song.plays += 1
    }
    day.songs[track.videoId] = song
    data.days[key] = day
    if data.tracks[track.videoId] == nil {
      data.tracks[track.videoId] = StatsMeta(title: track.title, artist: track.artist, album: track.album, artwork: track.artwork)
    }
    dirty = true
    if now.timeIntervalSince(lastSave) > 30 { save() }
  }

  /// The same song starting again (repeat, or played twice) counts as a new play.
  func songRestarted() {
    currentId = nil
  }

  func save() {
    guard dirty else { return }
    dirty = false
    lastSave = Date()
    if let d = try? JSONEncoder().encode(data) {
      try? d.write(to: url, options: .atomic)
    }
  }

  func clear() {
    data = StatsFile(since: ListeningStats.dayFormat.string(from: Date()))
    dirty = true
    save()
  }
}
