import Foundation
import UIKit
import WebKit

// Trailer mode, for recording the app in the simulator (never on a phone):
//   SIMCTL_CHILD_NOCTURN_DEMO=trailer xcrun simctl launch …
// A timeline walks through the app by itself with made-up songs, so the
// recording needs no touches, no account and no network.

@MainActor
enum Trailer {
  static var on: Bool { ProcessInfo.processInfo.environment["NOCTURN_DEMO"] == "trailer" }

  private static func web(_ step: String) {
    ChromeController.shared.onAction?("trailer", step)
  }

  private static func look(theme: String? = nil, style: String? = nil, color: String? = nil) {
    if let t = theme { ThemeStore.shared.set(t, photo: nil) }
    if let s = style { UserDefaults.standard.set(s, forKey: "lyrics.style") }
    if let c = color { UserDefaults.standard.set(c, forKey: "lyrics.color") }
  }

  /// Waits until the web side has its trailer listener (it sets
  /// window.__nocturnTrailerReady), then starts the timeline. On a slow
  /// simulator the page can take many seconds to load, and steps sent before
  /// that are lost.
  static func run(webview: WKWebView) {
    webview.evaluateJavaScript("window.__nocturnTrailerReady === true") { result, _ in
      MainActor.assumeIsolated {
        if (result as? Bool) == true {
          start()
        } else {
          DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            MainActor.assumeIsolated { run(webview: webview) }
          }
        }
      }
    }
  }

  private static func start() {
    BeatMeter.synthetic = true
    look(theme: "artwork", style: "fill", color: "white")
    UserDefaults.standard.set(true, forKey: "lyrics.wordSync")
    let steps: [(Double, () -> Void)] = [
      (0.0, { web("setup") }),
      (3.6, { ChromeController.shared.select("home"); web("home") }),
      (5.6, { web("scroll:300") }),
      (7.4, { web("scroll:0") }),
      (8.2, { web("handoff") }),
      (10.6, {
        web("hidehandoff")
        PlayerEngine.shared.startDemo(trailer: true)
        ChromeController.shared.showNowPlaying()
      }),
      (14.0, { look(theme: "aurora", style: "glow", color: "moon") }),
      (17.0, { look(theme: "galaxy") }),
      (20.0, { look(theme: "synthwave", style: "bounce", color: "rose") }),
      (23.0, { look(theme: "rain") }),
      (26.0, { look(theme: "city", style: "rise", color: "gold") }),
      (29.0, { look(theme: "seasons") }),
      (31.5, { look(theme: "christmas") }),
      (34.0, { look(theme: "meteors", style: "glow", color: "artwork") }),
      (36.5, { NotificationCenter.default.post(name: .trailerCard, object: nil) }),
      (41.0, { NotificationCenter.default.post(name: .trailerCloseCard, object: nil) }),
      (42.5, {
        // a calm, dark background behind Wrapped and the theme picker (bright ones wash out the text)
        look(theme: "aurora")
        ChromeController.shared.hideNowPlaying()
        ChromeController.shared.select("library")
        web("wrapped")
      }),
      (45.5, { web("scroll:520") }),
      (48.5, { web("scroll:1150") }),
      (51.5, { web("themes") }),
      (53.5, { web("scroll:700") }),
      (56.5, { web("scroll:1500") }),
      (59.5, { look(theme: "artwork") }),
    ]
    for (at, step) in steps {
      DispatchQueue.main.asyncAfter(deadline: .now() + at) { MainActor.assumeIsolated { step() } }
    }
  }
}

extension Notification.Name {
  static let trailerCard = Notification.Name("nocturn.trailer.card")
  static let trailerCloseCard = Notification.Name("nocturn.trailer.closecard")
}
