import SwiftRs
import Tauri
import UIKit
import WebKit

// MARK: arguments from the web side

class SetQueueArgs: Decodable {
  let tracks: [TrackDTO]
  let index: Int?
  let play: Bool?
  let positionMs: Double?
}

class InsertArgs: Decodable {
  let tracks: [TrackDTO]
  let next: Bool?
}

class SeekArgs: Decodable {
  let ms: Double
}

class RepeatArgs: Decodable {
  let mode: String
}

class AutoplayArgs: Decodable {
  let on: Bool
}

class JumpArgs: Decodable {
  let index: Int
}

class TabsArgs: Decodable {
  let tabs: [TabSpec]
  let selected: String?
}

class SelectTabArgs: Decodable {
  let id: String
}

class LyricsArgs: Decodable {
  let videoId: String
  let items: [LyricItem]
  let approx: Bool?
  let plain: String?
}

class ReplaceArgs: Decodable {
  let tracks: [TrackDTO]
  let note: String?
}

class HapticArgs: Decodable {
  let style: String?
}

// MARK: events to the web side

struct TabEvent: Encodable {
  let id: String
}

struct ActionEvent: Encodable {
  let kind: String
  let value: String?
}

/// Entry point for `plugin:nocturn|…` calls from the web app.
/// Calls arrive on Tauri's IPC queue; everything touching the player or UI
/// hops to the main actor first.
class NocturnPlugin: Plugin {
  @objc public override func load(webview: WKWebView) {
    DispatchQueue.main.async {
      MainActor.assumeIsolated {
        PlayerEngine.shared.onChange = { [weak self] state in
          try? self?.trigger("state", data: state)
          ChromeController.shared.playerChanged()
        }
        ChromeController.shared.onTab = { [weak self] id in
          try? self?.trigger("tab", data: TabEvent(id: id))
        }
        ChromeController.shared.onAction = { [weak self] kind, value in
          try? self?.trigger("action", data: ActionEvent(kind: kind, value: value))
        }
        ChromeController.shared.install(webview: webview)
        // Simulator screenshot tests: SIMCTL_CHILD_NOCTURN_DEMO=nowplaying
        if ProcessInfo.processInfo.environment["NOCTURN_DEMO"] == "nowplaying" {
          DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) {
            MainActor.assumeIsolated {
              PlayerEngine.shared.startDemo()
              ChromeController.shared.playerChanged()
              ChromeController.shared.showNowPlaying()
            }
          }
        }
      }
    }
  }

  private func onMain(_ invoke: Invoke, _ body: @escaping @MainActor () -> Void) {
    DispatchQueue.main.async {
      MainActor.assumeIsolated { body() }
      invoke.resolve()
    }
  }

  // MARK: player

  @objc public func setQueue(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(SetQueueArgs.self)
    onMain(invoke) { PlayerEngine.shared.setQueue(a.tracks, start: a.index ?? 0, play: a.play ?? true, positionMs: a.positionMs) }
  }

  @objc public func insert(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(InsertArgs.self)
    onMain(invoke) { PlayerEngine.shared.insert(a.tracks, next: a.next ?? false) }
  }

  @objc public func play(_ invoke: Invoke) {
    onMain(invoke) { PlayerEngine.shared.play() }
  }

  @objc public func pause(_ invoke: Invoke) {
    onMain(invoke) { PlayerEngine.shared.pause() }
  }

  @objc public func toggle(_ invoke: Invoke) {
    onMain(invoke) { PlayerEngine.shared.toggle() }
  }

  @objc public func next(_ invoke: Invoke) {
    onMain(invoke) { PlayerEngine.shared.next() }
  }

  @objc public func previous(_ invoke: Invoke) {
    onMain(invoke) { PlayerEngine.shared.previous() }
  }

  @objc public func seek(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(SeekArgs.self)
    onMain(invoke) { PlayerEngine.shared.seek(ms: a.ms) }
  }

  @objc public func setRepeat(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(RepeatArgs.self)
    onMain(invoke) { PlayerEngine.shared.setRepeat(a.mode) }
  }

  @objc public func setAutoplay(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(AutoplayArgs.self)
    onMain(invoke) { PlayerEngine.shared.setAutoplay(a.on) }
  }

  @objc public func jump(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(JumpArgs.self)
    onMain(invoke) { PlayerEngine.shared.jump(to: a.index) }
  }

  @objc public func replaceUpcoming(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(ReplaceArgs.self)
    onMain(invoke) { PlayerEngine.shared.replaceUpcoming(a.tracks, note: a.note) }
  }

  @objc public func setAiShuffle(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(AutoplayArgs.self)
    onMain(invoke) { PlayerEngine.shared.setAIShuffle(a.on) }
  }

  @objc public func getState(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let s = MainActor.assumeIsolated { PlayerEngine.shared.state() }
      invoke.resolve(s)
    }
  }

  @objc public func setLyrics(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(LyricsArgs.self)
    onMain(invoke) {
      PlayerEngine.shared.lyricItems = a.items
      PlayerEngine.shared.lyricsApprox = a.approx ?? false
      PlayerEngine.shared.lyricsPlain = a.plain ?? ""
      PlayerEngine.shared.lyricsTrackId = a.videoId
    }
  }

  // MARK: chrome

  @objc public func setTabs(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(TabsArgs.self)
    onMain(invoke) { ChromeController.shared.setTabs(a.tabs, selected: a.selected) }
  }

  @objc public func selectTab(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(SelectTabArgs.self)
    onMain(invoke) { ChromeController.shared.select(a.id) }
  }

  @objc public func showNowPlaying(_ invoke: Invoke) {
    onMain(invoke) { ChromeController.shared.showNowPlaying() }
  }

  @objc public func haptic(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(HapticArgs.self)
    onMain(invoke) { ChromeController.shared.haptic(a.style ?? "medium") }
  }
}

@_cdecl("init_plugin_nocturn")
public func initPlugin() -> Plugin {
  return NocturnPlugin()
}
