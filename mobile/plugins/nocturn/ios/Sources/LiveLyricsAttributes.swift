#if canImport(ActivityKit)
import ActivityKit
import Foundation

/// The live lyrics shown in the Dynamic Island and on the lock screen.
///
/// This one file is compiled into both the app (this plugin) and the
/// Live Activity widget (mobile/widget), so the two always agree on the shape.
struct NocturnLyricsAttributes: ActivityAttributes {
  struct ContentState: Codable, Hashable {
    /// The line being sung ("♪" during an instrumental part).
    var line: String
    /// The line after it (dimmed).
    var next: String
    var title: String
    var artist: String
    var playing: Bool
    /// The song's start and end as clock times, so the progress bar moves by itself.
    var start: Date
    var end: Date
    /// 0–1, for when the song is paused.
    var progress: Double
    var hasLyrics: Bool
  }

  var name: String
}
#endif
