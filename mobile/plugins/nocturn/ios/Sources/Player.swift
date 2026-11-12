import AVFoundation
import MediaPlayer
import Observation
import UIKit

/// What the web side and the native screens see.
struct PlayerState: Encodable, Sendable {
  var playing = false
  var loading = false
  var index = -1
  var positionMs: Double = 0
  var durationMs: Double = 0
  var queueLength = 0
  var track: TrackDTO?
  var repeatMode = "off"
  var autoplay = true
  var error: String?
  var aiShuffle = false
  /// Songs skipped within 30 s this session (AI Shuffle learns from them).
  var skipped: [String] = []
}

/// The music player. Lives entirely on the native side so playback, skipping
/// and moving to the next song keep working with the screen locked.
@MainActor
@Observable
final class PlayerEngine {
  static let shared = PlayerEngine()

  // observed by SwiftUI
  private(set) var queue: [TrackDTO] = []
  private(set) var index = -1
  private(set) var playing = false
  private(set) var loading = false
  private(set) var positionMs: Double = 0
  private(set) var durationMs: Double = 0
  private(set) var repeatMode = "off"
  private(set) var autoplay = UserDefaults.standard.object(forKey: "autoplay") as? Bool ?? true
  private(set) var lastError: String?
  private(set) var artworkImage: UIImage?
  /// Strong colours from the artwork, for the "moving artwork" background.
  private(set) var palette: [UIColor] = []
  var lyricItems: [LyricItem] = []
  var lyricsTrackId: String?
  var lyricsApprox = false
  var lyricsPlain = ""
  /// The user's timing nudge for the current song's lyrics (ms; positive = earlier).
  private(set) var lyricsOffset: Double = 0
  /// Nocturn's smart shuffle (planned on the web/Rust side, see ai.rs).
  private(set) var aiShuffle = UserDefaults.standard.bool(forKey: "aiShuffle")
  /// Waiting for an AI Shuffle plan.
  var aiBusy = false
  /// Last AI Shuffle explanation ("Planned 25 songs around …").
  var aiNote: String?
  private(set) var skipped: [String] = []
  /// Screenshot/demo mode (simulator tests): a fake clock instead of real audio.
  @ObservationIgnored private var demoStart: Date?

  var current: TrackDTO? { queue.indices.contains(index) ? queue[index] : nil }

  /// Called whenever something the web side cares about changes.
  @ObservationIgnored var onChange: ((PlayerState) -> Void)?

  private let player = AVQueuePlayer()
  @ObservationIgnored private var itemIndex: [ObjectIdentifier: Int] = [:]
  @ObservationIgnored private var preparedNext: AVPlayerItem?
  @ObservationIgnored private var loadToken = 0
  @ObservationIgnored private var timeObserver: Any?
  @ObservationIgnored private var currentItemObserver: NSKeyValueObservation?
  @ObservationIgnored private var statusObserver: NSKeyValueObservation?
  @ObservationIgnored private var lastEmit = Date.distantPast
  @ObservationIgnored private var artworkFor: String?
  @ObservationIgnored private var fetchingRadio = false
  @ObservationIgnored private var retried: Set<String> = []

  private init() {
    player.actionAtItemEnd = .advance
    player.automaticallyWaitsToMinimizeStalling = true
    configureSession()
    configureRemoteCommands()
    observe()
  }

  // MARK: public API

  func setQueue(_ tracks: [TrackDTO], start: Int, play: Bool, positionMs: Double?) {
    queue = tracks
    retried.removeAll()
    guard !tracks.isEmpty else {
      stopAll()
      return
    }
    load(index: max(0, min(start, tracks.count - 1)), autoplay: play, seekMs: positionMs)
  }

  /// Adds songs right after the current one ("Play next") or at the end.
  func insert(_ tracks: [TrackDTO], next: Bool) {
    guard !tracks.isEmpty else { return }
    if queue.isEmpty {
      setQueue(tracks, start: 0, play: true, positionMs: nil)
      return
    }
    let at = next ? min(index + 1, queue.count) : queue.count
    queue.insert(contentsOf: tracks, at: at)
    if at == index + 1 { dropPrepared() }
    prepareNext()
    emit(force: true)
  }

  func jump(to i: Int) {
    guard queue.indices.contains(i) else { return }
    load(index: i, autoplay: true, seekMs: nil)
  }

  func play() {
    if player.currentItem == nil, current != nil {
      load(index: index, autoplay: true, seekMs: positionMs)
      return
    }
    activateSession()
    player.play()
    playing = true
    updateNowPlaying()
    emit(force: true)
  }

  func pause() {
    player.pause()
    playing = false
    updateNowPlaying()
    emit(force: true)
  }

  func toggle() { playing ? pause() : play() }

  func next() {
    guard !queue.isEmpty else { return }
    // a skip in the first 30 s tells AI Shuffle you didn't want this one
    if let c = current, positionMs < 30_000 {
      skipped.removeAll { $0 == c.videoId }
      skipped.append(c.videoId)
      if skipped.count > 100 { skipped.removeFirst() }
    }
    if index + 1 < queue.count {
      load(index: index + 1, autoplay: true, seekMs: nil)
    } else if repeatMode == "all" {
      load(index: 0, autoplay: true, seekMs: nil)
    } else {
      continueWithRadio()
    }
  }

  func previous() {
    if positionMs > 3000 || index <= 0 {
      seek(ms: 0)
    } else {
      load(index: index - 1, autoplay: true, seekMs: nil)
    }
  }

  func seek(ms: Double) {
    let t = CMTime(seconds: max(0, ms) / 1000, preferredTimescale: 600)
    positionMs = max(0, ms)
    player.seek(to: t, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
      Task { @MainActor in
        self?.updateNowPlaying()
        self?.emit(force: true)
      }
    }
  }

  func setRepeat(_ mode: String) {
    repeatMode = ["off", "all", "one"].contains(mode) ? mode : "off"
    player.actionAtItemEnd = repeatMode == "one" ? .pause : .advance
    dropPrepared()
    prepareNext()
    emit(force: true)
  }

  func setAutoplay(_ on: Bool) {
    autoplay = on
    UserDefaults.standard.set(on, forKey: "autoplay")
    emit(force: true)
  }

  func setAIShuffle(_ on: Bool) {
    aiShuffle = on
    UserDefaults.standard.set(on, forKey: "aiShuffle")
    aiBusy = on
    if !on { aiNote = nil }
    emit(force: true)
    if on {
      // don't spin forever if the plan never arrives
      Task {
        try? await Task.sleep(nanoseconds: 12_000_000_000)
        self.aiBusy = false
      }
    }
  }

  /// Replaces everything after the current song (an AI Shuffle plan).
  func replaceUpcoming(_ tracks: [TrackDTO], note: String?) {
    aiBusy = false
    aiNote = note
    guard index >= 0, index < queue.count else {
      if !tracks.isEmpty { setQueue(tracks, start: 0, play: true, positionMs: nil) }
      return
    }
    queue = Array(queue[...index]) + tracks.filter { $0.videoId != queue[index].videoId }
    dropPrepared()
    prepareNext()
    emit(force: true)
  }

  func setLyricsOffset(_ ms: Double?) {
    lyricsOffset = ms.map { max(-15000, min(15000, $0)) } ?? 0
    if let id = current?.videoId {
      UserDefaults.standard.set(lyricsOffset, forKey: "lyrics.offset.\(id)")
    }
  }

  private func loadLyricsOffset() {
    lyricsOffset = current.map { UserDefaults.standard.double(forKey: "lyrics.offset.\($0.videoId)") } ?? 0
  }

  /// Output lag of the current speakers (AirPods/Bluetooth are often 150–250 ms).
  var outputLatencyMs: Double { AVAudioSession.sharedInstance().outputLatency * 1000 }

  /// Where the song is as you *hear* it — read straight from the player, for lyrics.
  func lyricClockMs() -> Double {
    if let d = demoStart { return Date().timeIntervalSince(d) * 1000 }
    let s = player.currentTime().seconds
    return (s.isFinite ? s * 1000 : positionMs) - outputLatencyMs
  }

  func state() -> PlayerState {
    PlayerState(
      playing: playing, loading: loading, index: index, positionMs: positionMs, durationMs: durationMs,
      queueLength: queue.count, track: current, repeatMode: repeatMode, autoplay: autoplay, error: lastError,
      aiShuffle: aiShuffle, skipped: skipped)
  }

  /// Fills the player with a sample song and lyrics, with a fake clock, so
  /// Now Playing can be screenshotted in the simulator without audio.
  func startDemo() {
    let t = TrackDTO(videoId: "demo0000000", title: "A Very Long Song Title That Should Truncate Gracefully (Extended Mix)", artist: "Nocturn Test Artist & Friends", album: "Night Sky", artwork: nil, durationMs: 214_000)
    queue = [t]
    index = 0
    durationMs = 214_000
    positionMs = 41_000
    playing = true
    demoStart = Date().addingTimeInterval(-41)
    artworkFor = t.videoId
    artworkImage = PlayerEngine.demoArtwork()
    palette = Palette.colors(from: artworkImage!)
    let lines = ["(Instrumental intro)", "I been running through the night", "Moonlight on the window glass", "Every city sleeps but I", "Hear the echo of the past", "Hold on, hold on", "Don't let the morning come too soon", "We're the only ones awake", "Dancing underneath the moon", "Hold on, hold on", "The stars are falling into place"]
    var items: [LyricItem] = [LyricItem(kind: "gap", start: 0, end: 12_000, text: "", words: [], estimated: nil, sungEnd: nil)]
    var start = 12_000.0
    for line in lines.dropFirst() {
      let words = line.split(separator: " ").enumerated().map { i, w in LyricWord(t: start + Double(i) * 420, w: String(w) + " ") }
      items.append(LyricItem(kind: "line", start: start, end: start + 4_800, text: line, words: words, estimated: true, sungEnd: start + Double(words.count) * 420))
      start += 4_800
    }
    lyricItems = items
    lyricsTrackId = t.videoId
    lyricsPlain = lines.joined(separator: "\n")
    aiShuffle = true
    aiNote = "Planned 25 songs around this one — 9 new finds"
  }

  static func demoArtwork() -> UIImage {
    let size = CGSize(width: 600, height: 600)
    return UIGraphicsImageRenderer(size: size).image { ctx in
      let colors = [UIColor(red: 0.35, green: 0.22, blue: 0.62, alpha: 1).cgColor, UIColor(red: 0.95, green: 0.45, blue: 0.35, alpha: 1).cgColor] as CFArray
      let g = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])!
      ctx.cgContext.drawLinearGradient(g, start: .zero, end: CGPoint(x: size.width, y: size.height), options: [])
      UIColor.white.withAlphaComponent(0.85).setFill()
      UIBezierPath(ovalIn: CGRect(x: 170, y: 170, width: 260, height: 260)).fill()
    }
  }

  // MARK: loading songs

  private func load(index i: Int, autoplay play: Bool, seekMs: Double?) {
    loadToken += 1
    let token = loadToken
    index = i
    positionMs = seekMs ?? 0
    durationMs = queue[i].durationMs ?? 0
    loading = true
    lastError = nil
    player.removeAllItems()
    itemIndex.removeAll()
    preparedNext = nil
    playing = play
    updateNowPlaying()
    loadArtwork()
    loadLyricsOffset()
    emit(force: true)
    let track = queue[i]
    Task {
      do {
        let item = try await self.makeItem(for: track)
        guard token == self.loadToken else { return }
        self.itemIndex[ObjectIdentifier(item)] = i
        self.player.insert(item, after: nil)
        if let ms = seekMs, ms > 0 {
          _ = await item.seek(to: CMTime(seconds: ms / 1000, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero)
        }
        guard token == self.loadToken else { return }
        if play {
          self.activateSession()
          self.player.play()
        }
        self.prepareNext()
      } catch {
        guard token == self.loadToken else { return }
        self.fail(track: track, error: error)
      }
    }
  }

  private func makeItem(for track: TrackDTO) async throws -> AVPlayerItem {
    // downloaded songs play from the phone, no internet needed
    if let local = Downloads.shared.audioURL(track.videoId) {
      let item = AVPlayerItem(asset: AVURLAsset(url: local))
      item.audioTimePitchAlgorithm = .spectral
      return item
    }
    let s = try await YouTube.shared.stream(track.videoId)
    let wait = s.availableAt - Date().timeIntervalSince1970
    if wait > 0 && wait < 15 {
      try await Task.sleep(nanoseconds: UInt64((wait + 0.25) * 1_000_000_000))
    }
    let asset = AVURLAsset(url: s.url)
    let item = AVPlayerItem(asset: asset)
    item.preferredForwardBufferDuration = 30
    item.audioTimePitchAlgorithm = .spectral
    return item
  }

  /// Gets the next song ready so it starts without a gap.
  private func prepareNext() {
    guard repeatMode != "one", preparedNext == nil, player.currentItem != nil else { return }
    var n = index + 1
    if n >= queue.count {
      guard repeatMode == "all", !queue.isEmpty else { return }
      n = 0
    }
    let track = queue[n]
    let token = loadToken
    Task {
      guard let item = try? await self.makeItem(for: track), token == self.loadToken, self.preparedNext == nil,
        let cur = self.player.currentItem
      else { return }
      // the queue may have changed while we waited
      let expected = self.index + 1 < self.queue.count ? self.index + 1 : (self.repeatMode == "all" ? 0 : -1)
      guard expected >= 0, self.queue[expected].videoId == track.videoId else { return }
      self.itemIndex[ObjectIdentifier(item)] = expected
      self.preparedNext = item
      self.player.insert(item, after: cur)
    }
  }

  private func dropPrepared() {
    if let p = preparedNext {
      player.remove(p)
      itemIndex.removeValue(forKey: ObjectIdentifier(p))
      preparedNext = nil
    }
  }

  private func fail(track: TrackDTO, error: Error) {
    // A link can go stale; try a fresh one once before skipping.
    if !retried.contains(track.videoId) {
      retried.insert(track.videoId)
      Task {
        await YouTube.shared.forget(track.videoId)
        self.load(index: self.index, autoplay: self.playing, seekMs: self.positionMs)
      }
      return
    }
    loading = false
    lastError = "\(track.title): \(error.localizedDescription)"
    emit(force: true)
    if playing, index + 1 < queue.count {
      Task {
        try? await Task.sleep(nanoseconds: 1_200_000_000)
        self.next()
      }
    } else {
      playing = false
      updateNowPlaying()
    }
  }

  private func stopAll() {
    loadToken += 1
    player.removeAllItems()
    itemIndex.removeAll()
    preparedNext = nil
    queue = []
    index = -1
    playing = false
    loading = false
    positionMs = 0
    durationMs = 0
    MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    emit(force: true)
  }

  /// End of the queue: keep going with YouTube Music's radio for the last song.
  private func continueWithRadio() {
    guard autoplay, let last = queue.last, !fetchingRadio else {
      pause()
      seek(ms: 0)
      return
    }
    fetchingRadio = true
    Task {
      defer { self.fetchingRadio = false }
      let more = (try? await YouTube.shared.radio(after: last.videoId)) ?? []
      let known = Set(self.queue.map(\.videoId))
      let fresh = more.filter { !known.contains($0.videoId) }.prefix(25)
      guard !fresh.isEmpty else {
        self.pause()
        return
      }
      let start = self.queue.count
      self.queue.append(contentsOf: fresh)
      self.load(index: start, autoplay: true, seekMs: nil)
    }
  }

  // MARK: observing AVPlayer

  private func observe() {
    timeObserver = player.addPeriodicTimeObserver(forInterval: CMTime(seconds: 0.5, preferredTimescale: 600), queue: .main) { [weak self] t in
      MainActor.assumeIsolated {
        guard let self else { return }
        if t.seconds.isFinite { self.positionMs = t.seconds * 1000 }
        if let d = self.player.currentItem?.duration.seconds, d.isFinite, d > 0 { self.durationMs = d * 1000 }
        let isPlaying = self.player.timeControlStatus == .playing
        let waiting = self.player.timeControlStatus == .waitingToPlayAtSpecifiedRate
        if self.loading && isPlaying { self.loading = false }
        self.loading = self.loading || (waiting && self.playing)
        if isPlaying { self.loading = false }
        self.emit(force: false)
        LiveLyrics.shared.update(self)
      }
    }
    currentItemObserver = player.observe(\.currentItem, options: [.new]) { [weak self] _, _ in
      Task { @MainActor in
        guard let self else { return }
        let item = self.player.currentItem
        self.observeStatus(item)
        // nil = ran out of songs; the end-of-song handler below decides what's next
        guard let item else { return }
        if let i = self.itemIndex[ObjectIdentifier(item)], i != self.index {
          self.index = i
          if item === self.preparedNext { self.preparedNext = nil }
          self.positionMs = 0
          self.durationMs = self.queue[i].durationMs ?? 0
          self.loadArtwork()
          self.loadLyricsOffset()
          self.updateNowPlaying()
          self.emit(force: true)
          self.prepareNext()
        }
      }
    }
    NotificationCenter.default.addObserver(forName: AVPlayerItem.didPlayToEndTimeNotification, object: nil, queue: .main) { [weak self] note in
      MainActor.assumeIsolated {
        guard let self, let item = note.object as? AVPlayerItem, self.itemIndex[ObjectIdentifier(item)] == self.index else { return }
        if self.repeatMode == "one" {
          self.seek(ms: 0)
          self.player.play()
        } else if self.preparedNext == nil {
          // nothing queued up behind this song
          self.next()
        }
      }
    }
  }

  private func observeStatus(_ item: AVPlayerItem?) {
    statusObserver = item?.observe(\.status, options: [.new]) { [weak self] it, _ in
      guard it.status == .failed else { return }
      let err = it.error
      Task { @MainActor in
        guard let self, let track = self.current else { return }
        self.fail(track: track, error: err ?? YouTubeError.message("playback failed"))
      }
    }
  }

  private func emit(force: Bool) {
    guard force || Date().timeIntervalSince(lastEmit) >= 1 else { return }
    lastEmit = Date()
    onChange?(state())
    LiveLyrics.shared.update(self)
  }

  // MARK: audio session, lock screen, remote controls

  private func configureSession() {
    let s = AVAudioSession.sharedInstance()
    try? s.setCategory(.playback, mode: .default, policy: .longFormAudio)
    NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: s, queue: .main) { [weak self] note in
      let typeRaw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
      let optsRaw = note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
      MainActor.assumeIsolated {
        guard let self, let typeRaw, let type = AVAudioSession.InterruptionType(rawValue: typeRaw) else { return }
        if type == .began {
          self.playing = false
          self.emit(force: true)
        } else if AVAudioSession.InterruptionOptions(rawValue: optsRaw).contains(.shouldResume) {
          self.play()
        }
      }
    }
    NotificationCenter.default.addObserver(forName: AVAudioSession.routeChangeNotification, object: s, queue: .main) { [weak self] note in
      let reasonRaw = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
      MainActor.assumeIsolated {
        // headphones unplugged / AirPods taken out → pause, like Apple Music
        if let reasonRaw, AVAudioSession.RouteChangeReason(rawValue: reasonRaw) == .oldDeviceUnavailable {
          self?.pause()
        }
      }
    }
  }

  private func activateSession() {
    try? AVAudioSession.sharedInstance().setActive(true)
  }

  private func configureRemoteCommands() {
    // Handlers may arrive on any thread; hop to the main actor explicitly.
    let c = MPRemoteCommandCenter.shared()
    c.playCommand.addTarget { _ in
      Task { @MainActor in PlayerEngine.shared.play() }
      return .success
    }
    c.pauseCommand.addTarget { _ in
      Task { @MainActor in PlayerEngine.shared.pause() }
      return .success
    }
    c.togglePlayPauseCommand.addTarget { _ in
      Task { @MainActor in PlayerEngine.shared.toggle() }
      return .success
    }
    c.nextTrackCommand.addTarget { _ in
      Task { @MainActor in PlayerEngine.shared.next() }
      return .success
    }
    c.previousTrackCommand.addTarget { _ in
      Task { @MainActor in PlayerEngine.shared.previous() }
      return .success
    }
    c.changePlaybackPositionCommand.addTarget { e in
      guard let e = e as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
      let ms = e.positionTime * 1000
      Task { @MainActor in PlayerEngine.shared.seek(ms: ms) }
      return .success
    }
  }

  private func updateNowPlaying() {
    guard let t = current else {
      MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
      return
    }
    var info: [String: Any] = [
      MPMediaItemPropertyTitle: t.title,
      MPMediaItemPropertyArtist: t.artist,
      MPNowPlayingInfoPropertyElapsedPlaybackTime: positionMs / 1000,
      MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
      MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
    ]
    if let a = t.album { info[MPMediaItemPropertyAlbumTitle] = a }
    if durationMs > 0 { info[MPMediaItemPropertyPlaybackDuration] = durationMs / 1000 }
    if let img = artworkImage, artworkFor == t.videoId {
      info[MPMediaItemPropertyArtwork] = MPMediaItemArtwork(boundsSize: img.size) { _ in img }
    }
    MPNowPlayingInfoCenter.default().nowPlayingInfo = info
  }

  private func loadArtwork() {
    guard let t = current else { return }
    if artworkFor == t.videoId { return }
    artworkFor = t.videoId
    if let saved = Downloads.shared.artwork(t.videoId) {
      setArtwork(saved)
      return
    }
    artworkImage = nil
    guard let s = t.artwork, let url = URL(string: s) else { return }
    let id = t.videoId
    Task {
      guard let (data, _) = try? await URLSession.shared.data(from: url), let img = UIImage(data: data) else { return }
      guard self.current?.videoId == id else { return }
      self.setArtwork(PlayerEngine.squared(img))
    }
  }

  private func setArtwork(_ img: UIImage) {
    artworkImage = img
    let colors = Palette.colors(from: img)
    if !colors.isEmpty { palette = colors }
    updateNowPlaying()
  }

  /// Video thumbnails are 16:9 with bars; crop to the square middle like album art.
  static func squared(_ img: UIImage) -> UIImage {
    let w = img.size.width, h = img.size.height
    guard w > h * 1.1 else { return img }
    let side = h
    let rect = CGRect(x: (w - side) / 2 * img.scale, y: 0, width: side * img.scale, height: side * img.scale)
    guard let cg = img.cgImage?.cropping(to: rect) else { return img }
    return UIImage(cgImage: cg, scale: img.scale, orientation: img.imageOrientation)
  }
}


