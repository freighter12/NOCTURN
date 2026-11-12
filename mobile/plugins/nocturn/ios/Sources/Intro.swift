import SwiftUI
import UIKit

/// The start-up animation: a crescent moon is revealed as its shadow slides
/// away, a star glints, and the NOCTURN wordmark settles into place. Then
/// it fades out over the app. Settings › Appearance › Start-up Animation
/// picks full / short / off. Tap to skip.
@MainActor
enum Intro {
  private static var host: UIHostingController<IntroView>?
  /// true while the animation is on screen (the web side waits for it)
  private(set) static var playing = false
  static var onDone: (() -> Void)?

  static var mode: String {
    let m = UserDefaults.standard.string(forKey: "intro") ?? "full"
    return ["full", "short", "off"].contains(m) ? m : "full"
  }

  static func setMode(_ m: String) {
    UserDefaults.standard.set(["full", "short", "off"].contains(m) ? m : "full", forKey: "intro")
  }

  static func show(over parent: UIViewController) {
    let m = mode
    guard m != "off", host == nil else { return }
    let short = m == "short"
    playing = true
    let h = UIHostingController(rootView: IntroView(short: short, start: Date()))
    h.view.backgroundColor = .clear
    h.view.frame = parent.view.bounds
    h.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    h.view.addGestureRecognizer(UITapGestureRecognizer(target: IntroSkip.shared, action: #selector(IntroSkip.skip)))
    parent.addChild(h)
    parent.view.addSubview(h.view)
    h.didMove(toParent: parent)
    host = h

    let total = short ? 1.05 : 2.35
    // a soft tap when the crescent is complete
    DispatchQueue.main.asyncAfter(deadline: .now() + (short ? 0.6 : 1.15)) {
      MainActor.assumeIsolated {
        if playing { UIImpactFeedbackGenerator(style: .soft).impactOccurred(intensity: 0.7) }
      }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + total) {
      MainActor.assumeIsolated { finish() }
    }
  }

  static func finish() {
    guard let h = host else { return }
    host = nil
    playing = false
    onDone?()
    UIView.animate(
      withDuration: 0.5, delay: 0, options: [.curveEaseInOut, .allowUserInteraction],
      animations: {
        h.view.alpha = 0
        h.view.transform = CGAffineTransform(scaleX: 1.12, y: 1.12)
      },
      completion: { _ in
        h.willMove(toParent: nil)
        h.view.removeFromSuperview()
        h.removeFromParent()
      })
  }
}

@MainActor
final class IntroSkip: NSObject {
  static let shared = IntroSkip()
  @objc func skip() { Intro.finish() }
}

private func clamp01(_ x: Double) -> Double { min(1, max(0, x)) }

/// ease in-out (cubic)
private func ease(_ x: Double) -> Double {
  let v = clamp01(x)
  return v < 0.5 ? 4 * v * v * v : 1 - pow(-2 * v + 2, 3) / 2
}

private func circle(_ c: CGPoint, _ r: CGFloat) -> Path {
  Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2))
}

struct IntroView: View {
  let short: Bool
  let start: Date

  private static let stars = StarField.make(90, seed: 0x1D70_BEEF_0000_0042)

  var body: some View {
    TimelineView(.animation) { ctx in
      let t: Double = ctx.date.timeIntervalSince(start)
      // the short version plays everything twice as fast
      let p: Double = short ? t * 2 : t
      Canvas { c, size in
        c.fill(Path(CGRect(origin: .zero, size: size)), with: .color(rgb(0.03, 0.03, 0.045)))

        let starsIn: Double = ease((p - 0.05) / 0.9)
        StarField.draw(c, IntroView.stars, size: size, t: t * 2.5, alpha: starsIn)

        let r: CGFloat = min(size.width, size.height) * 0.17
        let center = CGPoint(x: size.width / 2, y: size.height * 0.42)

        // soft halo
        let halo: Double = ease((p - 0.3) / 0.9)
        let hr: CGFloat = r * 2.7
        c.fill(
          circle(center, hr),
          with: .radialGradient(
            Gradient(colors: [rgb(0.8, 0.85, 1).opacity(0.22 * halo), .clear]),
            center: center, startRadius: r * 0.6, endRadius: hr))

        // faint full disc (earthshine)
        c.fill(circle(center, r), with: .color(.white.opacity(0.045 * halo)))

        // the crescent: the shadow circle slides off the moon
        let m: Double = ease((p - 0.15) / 1.0)
        let mf = CGFloat(m)
        let cutR: CGFloat = r * (1.06 - 0.19 * mf)
        let cut = CGPoint(x: center.x + r * 0.58 * mf, y: center.y - r * 0.38 * mf)
        let shine = Gradient(colors: [rgb(0.96, 0.96, 0.97), rgb(0.79, 0.8, 0.83), rgb(0.5, 0.51, 0.56)])
        c.drawLayer { l in
          l.fill(
            circle(center, r),
            with: .linearGradient(
              shine,
              startPoint: CGPoint(x: center.x - r * 0.6, y: center.y - r * 0.8),
              endPoint: CGPoint(x: center.x + r * 0.6, y: center.y + r * 0.9)))
          l.blendMode = .destinationOut
          l.fill(circle(cut, cutR), with: .color(.black))
        }

        // the star, with a glint
        let s: Double = ease((p - 0.9) / 0.4)
        if s > 0 {
          let sf = CGFloat(s)
          let sp = CGPoint(x: center.x + r * 0.98, y: center.y - r * 1.02)
          c.fill(circle(sp, r * 0.055 * sf), with: .color(.white))
          let fade: Double = 1 - 0.5 * clamp01((p - 1.3) / 0.6)
          let g: CGFloat = r * 0.32 * sf * CGFloat(fade)
          var glint = Path()
          glint.move(to: CGPoint(x: sp.x - g, y: sp.y))
          glint.addLine(to: CGPoint(x: sp.x + g, y: sp.y))
          glint.move(to: CGPoint(x: sp.x, y: sp.y - g))
          glint.addLine(to: CGPoint(x: sp.x, y: sp.y + g))
          c.stroke(glint, with: .color(.white.opacity(0.7 * s * fade)), lineWidth: 1)
        }

        // wordmark: letters start spread out and settle
        let w: Double = ease((p - 0.65) / 0.85)
        if w > 0 {
          var tc = c
          tc.opacity = w
          let word = Text("NOCTURN")
            .font(.system(size: 30, weight: .semibold))
            .tracking(CGFloat(20 - 11 * w))
            .foregroundColor(rgb(0.93, 0.94, 0.96))
          tc.draw(word, at: CGPoint(x: size.width / 2, y: center.y + r * 1.75 + CGFloat(10 * (1 - w))), anchor: .center)
        }
        let sub: Double = ease((p - 1.0) / 0.6)
        if sub > 0 {
          var tc = c
          tc.opacity = sub * 0.6
          let label = Text("MUSIC")
            .font(.system(size: 12, weight: .medium))
            .tracking(7)
            .foregroundColor(.white)
          tc.draw(label, at: CGPoint(x: size.width / 2, y: center.y + r * 1.75 + 34), anchor: .center)
        }
      }
    }
    .ignoresSafeArea()
  }
}
