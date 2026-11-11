#if canImport(ActivityKit)
import ActivityKit
#endif
import Foundation
import UIKit

/// Keeps the Live Activity (Dynamic Island + lock screen) showing the line
/// being sung. The player calls `update` twice a second; the activity is only
/// touched when the line, the song or play/pause changes.
@MainActor
final class LiveLyrics {
  static let shared = LiveLyrics()

  private(set) var enabled = UserDefaults.standard.object(forKey: "liveLyrics") as? Bool ?? true
  private var lastKey = ""
  private var pausedSince: Date?
  #if canImport(ActivityKit)
  private var activity: Activity<NocturnLyricsAttributes>?
  #endif

  func setEnabled(_ on: Bool) {
    enabled = on
    UserDefaults.standard.set(on, forKey: "liveLyrics")
    lastKey = ""
    if on {
      update(PlayerEngine.shared)
    } else {
      endAll()
    }
  }

  /// Ends activities left over from the last time the app ran.
  func cleanUp() {
    #if canImport(ActivityKit)
    let old = Activity<NocturnLyricsAttributes>.activities
    guard !old.isEmpty else { return }
    Task {
      for a in old { await a.end(nil, dismissalPolicy: .immediate) }
    }
    #endif
  }

  func update(_ e: PlayerEngine) {
    #if canImport(ActivityKit)
    guard enabled, let t = e.current else {
      if activity != nil { endAll() }
      return
    }
    if e.playing {
      pausedSince = nil
    } else {
      if pausedSince == nil { pausedSince = Date() }
      // paused for 10 minutes: tidy the island away
      if let p = pausedSince, Date().timeIntervalSince(p) > 600 {
        if activity != nil { endAll() }
        return
      }
    }

    let items = e.lyricsTrackId == t.videoId ? e.lyricItems : []
    var line = ""
    var next = ""
    var index = -1
    if !items.isEmpty {
      // a quarter of a second early: the island takes a moment to redraw
      let clock = e.lyricClockMs() + e.lyricsOffset + 250
      index = LyricsTiming.activeIndex(items, clock)
      if index >= 0, items[index].kind == "line" {
        line = items[index].text
      } else {
        line = "♪"
      }
      next = items[(index + 1)...].first(where: { $0.kind == "line" })?.text ?? ""
    }

    let key = "\(t.videoId)|\(index)|\(e.playing)|\(items.count)"
    guard key != lastKey else { return }

    let pos = max(0, e.positionMs) / 1000
    let dur = max(e.durationMs / 1000, pos + 1)
    let start = Date().addingTimeInterval(-pos)
    let state = NocturnLyricsAttributes.ContentState(
      line: line, next: next, title: t.title, artist: t.artist, playing: e.playing,
      start: start, end: start.addingTimeInterval(dur), progress: min(1, pos / dur), hasLyrics: !items.isEmpty)
    let content = ActivityContent(state: state, staleDate: nil)

    if let a = activity, a.activityState == .active || a.activityState == .stale {
      lastKey = key
      Task { await a.update(content) }
      return
    }
    // A new activity can only be started while the app is on screen.
    guard e.playing, UIApplication.shared.applicationState == .active,
      ActivityAuthorizationInfo().areActivitiesEnabled
    else { return }
    do {
      activity = try Activity.request(attributes: NocturnLyricsAttributes(name: "Nocturn"), content: content, pushType: nil)
      lastKey = key
    } catch {
      // try again on a later tick
    }
    #endif
  }

  private func endAll() {
    lastKey = ""
    #if canImport(ActivityKit)
    activity = nil
    let all = Activity<NocturnLyricsAttributes>.activities
    Task {
      for a in all { await a.end(nil, dismissalPolicy: .immediate) }
    }
    #endif
  }
}
