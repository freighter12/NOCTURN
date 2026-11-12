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

class LyricsLookArgs: Decodable {
  let color: String?
  let style: String?
  let size: Double?
  let align: String?
  let blur: Bool?
  let words: Bool?
  let translate: Bool?
}

struct LyricsLookResult: Encodable {
  let color: String
  let style: String
  let size: Double
  let align: String
  let blur: Bool
  let words: Bool
  let translate: Bool
}

struct ScanResult: Encodable {
  let code: String
}

class CrossfadeArgs: Decodable {
  let seconds: Double
}

class AudioArgs: Decodable {
  let mix: Bool?
  let resume: Bool?
  let level: Bool?
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

class DownloadArgs: Decodable {
  let tracks: [TrackDTO]
}

class RemoveDownloadsArgs: Decodable {
  let ids: [String]?
  let all: Bool?
}

class ThemeArgs: Decodable {
  let theme: String
  /// base64 JPEG, only when a new photo was picked
  let photo: String?
}

class LookArgs: Decodable {
  let intro: String?
  let speed: Double?
  let dim: Int?
  let still: Bool?
  let refresh: String?
  let pulse: Bool?
}

struct SettingsResult: Encodable {
  let theme: String
  let photo: String?
  let liveLyrics: Bool
  let speed: Double
  let dim: Int
  let still: Bool
  let autoplay: Bool
  let aiShuffle: Bool
  let intro: String
  let introPlaying: Bool
  let refresh: String
  let maxFps: Int
  let pulse: Bool
  let mixWithOthers: Bool
  let autoResume: Bool
  let levelling: Bool
  let crossfade: Double
  let lyrics: LyricsLookResult
  /// This build's numbers (CFBundleShortVersionString / CFBundleVersion).
  let version: String
  let build: String
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
        Downloads.shared.onChange = { [weak self] state in
          try? self?.trigger("downloads", data: state)
        }
        LiveLyrics.shared.cleanUp()
        ChromeController.shared.install(webview: webview)
        // Simulator trailer recording: SIMCTL_CHILD_NOCTURN_DEMO=trailer
        if Trailer.on { Trailer.run() }
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

  static func lyricsLook() -> LyricsLookResult {
    let d = UserDefaults.standard
    return LyricsLookResult(
      color: d.string(forKey: "lyrics.color") ?? "white", style: d.string(forKey: "lyrics.style") ?? "fill",
      size: d.object(forKey: "lyrics.size") as? Double ?? 1, align: d.string(forKey: "lyrics.align") ?? "left",
      blur: d.object(forKey: "lyrics.blur") as? Bool ?? true, words: d.object(forKey: "lyrics.wordSync") as? Bool ?? true,
      translate: d.object(forKey: "lyrics.translate") as? Bool ?? true)
  }

  @objc public func setLyricsLook(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(LyricsLookArgs.self)
    onMain(invoke) {
      let d = UserDefaults.standard
      if let v = a.color, LyricsLook.colors.contains(v) { d.set(v, forKey: "lyrics.color") }
      if let v = a.style, LyricsLook.styles.contains(v) { d.set(v, forKey: "lyrics.style") }
      if let v = a.size { d.set(min(1.4, max(0.7, v)), forKey: "lyrics.size") }
      if let v = a.align, ["left", "center"].contains(v) { d.set(v, forKey: "lyrics.align") }
      if let v = a.blur { d.set(v, forKey: "lyrics.blur") }
      if let v = a.words { d.set(v, forKey: "lyrics.wordSync") }
      if let v = a.translate { d.set(v, forKey: "lyrics.translate") }
    }
  }

  // MARK: PC sync: scan the QR code on the PC

  @objc public func scanQr(_ invoke: Invoke) {
    DispatchQueue.main.async {
      MainActor.assumeIsolated {
        guard let top = ChromeController.shared.topViewController() else {
          invoke.reject("The camera can't open right now.")
          return
        }
        QRScanner.scan(from: top) { code, problem in
          if let code { invoke.resolve(ScanResult(code: code)) } else { invoke.reject(problem ?? "cancelled") }
        }
      }
    }
  }

  // MARK: listening stats (Nocturn Wrapped)

  @objc public func getStats(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let data = MainActor.assumeIsolated { () -> StatsFile in
        ListeningStats.shared.save()
        return ListeningStats.shared.data
      }
      invoke.resolve(data)
    }
  }

  @objc public func clearStats(_ invoke: Invoke) {
    onMain(invoke) { ListeningStats.shared.clear() }
  }

  @objc public func setCrossfade(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(CrossfadeArgs.self)
    onMain(invoke) { PlayerEngine.shared.setCrossfade(a.seconds) }
  }

  @objc public func setAudio(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(AudioArgs.self)
    onMain(invoke) {
      PlayerEngine.shared.setAudio(mix: a.mix, resume: a.resume)
      if let l = a.level { PlayerEngine.shared.setLevelling(l) }
    }
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

  // MARK: downloads

  @objc public func download(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(DownloadArgs.self)
    onMain(invoke) { Downloads.shared.add(a.tracks) }
  }

  @objc public func removeDownloads(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(RemoveDownloadsArgs.self)
    onMain(invoke) {
      if a.all == true {
        Downloads.shared.removeAll()
      } else {
        Downloads.shared.remove(a.ids ?? [])
      }
    }
  }

  @objc public func getDownloads(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let s = MainActor.assumeIsolated { Downloads.shared.state() }
      invoke.resolve(s)
    }
  }

  // MARK: settings (theme, live lyrics)

  @objc public func getSettings(_ invoke: Invoke) {
    DispatchQueue.main.async {
      let s = MainActor.assumeIsolated {
        SettingsResult(
          theme: ThemeStore.shared.theme, photo: ThemeStore.shared.photoBase64(), liveLyrics: LiveLyrics.shared.enabled,
          speed: ThemeStore.shared.speed, dim: ThemeStore.shared.dim, still: ThemeStore.shared.still,
          autoplay: PlayerEngine.shared.autoplay, aiShuffle: PlayerEngine.shared.aiShuffle,
          intro: Intro.mode, introPlaying: Intro.playing,
          refresh: ThemeStore.shared.refresh, maxFps: ThemeStore.maxFps, pulse: ThemeStore.shared.pulse,
          mixWithOthers: PlayerEngine.shared.mixWithOthers, autoResume: PlayerEngine.shared.autoResume,
          levelling: PlayerEngine.shared.levelling,
          crossfade: PlayerEngine.shared.crossfade,
          lyrics: NocturnPlugin.lyricsLook(),
          version: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "",
          build: Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "")
      }
      invoke.resolve(s)
    }
  }

  @objc public func setTheme(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(ThemeArgs.self)
    onMain(invoke) { ThemeStore.shared.set(a.theme, photo: a.photo) }
  }

  @objc public func setLook(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(LookArgs.self)
    onMain(invoke) {
      ThemeStore.shared.setLook(speed: a.speed, dim: a.dim, still: a.still, refresh: a.refresh, pulse: a.pulse)
      if let i = a.intro { Intro.setMode(i) }
    }
  }

  @objc public func setLiveLyrics(_ invoke: Invoke) throws {
    let a = try invoke.parseArgs(AutoplayArgs.self)
    onMain(invoke) { LiveLyrics.shared.setEnabled(a.on) }
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
