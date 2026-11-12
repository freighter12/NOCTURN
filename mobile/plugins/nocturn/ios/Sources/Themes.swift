import Observation
import SwiftUI
import UIKit

/// The background theme, chosen in Settings › Appearance.
///   artwork   – the song's colours slowly drifting (default)
///   lunar     – night sky, stars and tonight's real moon phase
///   aurora    – northern lights rippling over a starry sky
///   nebula    – deep-space clouds of colour with stars
///   synthwave – a striped sunset sun over a neon grid rushing toward you
///   ocean     – night waves under the moon
///   snow      – gentle snowfall
///   fireflies – glowing fireflies drifting in the dark
///   seasons   – changes with the season (blossom, golden hour, leaves, Christmas)
///   rain, meteors, galaxy, lava, waveform, city, bokeh, blossom, summer, leaves, christmas
///   black     – pure black
///   photo     – a picture of your own, blurred and darkened
@MainActor
@Observable
final class ThemeStore {
  static let shared = ThemeStore()
  static let names = [
    "artwork", "seasons", "lunar", "aurora", "nebula", "synthwave", "ocean", "snow", "fireflies",
    "rain", "meteors", "galaxy", "lava", "waveform", "city", "bokeh", "blossom", "summer", "leaves", "christmas",
    "black", "photo",
  ]

  private(set) var theme: String
  private(set) var photo: UIImage?
  /// How fast the background moves: 0.5 slow, 1 normal, 1.8 fast.
  private(set) var speed: Double
  /// How much the background is darkened behind text: 0 lighter, 1 normal, 2 darker.
  private(set) var dim: Int
  /// Freeze the movement (saves a little battery).
  private(set) var still: Bool
  /// How smoothly backgrounds move: "auto" (each theme's own pace),
  /// "120" (every frame on ProMotion screens) or "saver" (half the frames).
  private(set) var refresh: String
  /// The screen's top refresh rate (120 on ProMotion iPhones, else 60).
  static var maxFps: Int { UIScreen.main.maximumFramesPerSecond }

  private let photoURL: URL = {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    return base.appendingPathComponent("theme-photo.jpg")
  }()

  private init() {
    let d = UserDefaults.standard
    let saved = d.string(forKey: "theme") ?? "artwork"
    theme = ThemeStore.names.contains(saved) ? saved : "artwork"
    speed = d.object(forKey: "theme.speed") as? Double ?? 1
    dim = d.object(forKey: "theme.dim") as? Int ?? 1
    still = d.bool(forKey: "theme.still")
    refresh = d.string(forKey: "theme.refresh") ?? "auto"
    photo = UIImage(contentsOfFile: photoURL.path)
  }

  /// `photo` is a base64 JPEG (already resized on the web side).
  func set(_ name: String, photo base64: String?) {
    if let b = base64, let data = Data(base64Encoded: b), let img = UIImage(data: data) {
      try? data.write(to: photoURL, options: .atomic)
      photo = img
    }
    var next = ThemeStore.names.contains(name) ? name : "artwork"
    if next == "photo" && photo == nil { next = "artwork" }
    theme = next
    UserDefaults.standard.set(next, forKey: "theme")
  }

  func setLook(speed newSpeed: Double?, dim newDim: Int?, still newStill: Bool?, refresh newRefresh: String? = nil) {
    let d = UserDefaults.standard
    if let v = newRefresh, ["auto", "120", "saver"].contains(v) {
      refresh = v
      d.set(v, forKey: "theme.refresh")
    }
    if let v = newSpeed {
      speed = min(3, max(0.2, v))
      d.set(speed, forKey: "theme.speed")
    }
    if let v = newDim {
      dim = min(2, max(0, v))
      d.set(dim, forKey: "theme.dim")
    }
    if let v = newStill {
      still = v
      d.set(v, forKey: "theme.still")
    }
  }

  func photoBase64() -> String? {
    guard photo != nil else { return nil }
    return (try? Data(contentsOf: photoURL))?.base64EncodedString()
  }
}

// MARK: - the background view

/// Drawn behind every tab (the web view is transparent) and behind Now Playing.
/// `strong` darkens it more, so text on the pages stays easy to read.
struct ThemeBackground: View {
  let engine: PlayerEngine
  var strong = false

  var body: some View {
    let store = ThemeStore.shared
    let k: Double = store.dim == 0 ? 0.55 : (store.dim == 2 ? 1.4 : 1)
    // tab pages: even all the way down, so nothing looks like a dark bar behind the tab bar
    let base: [Double] = strong ? [0.48, 0.52, 0.5] : [0.18, 0.38, 0.66]
    let shade = base.map { Color.black.opacity(min(0.92, $0 * k)) }
    GeometryReader { g in
      ZStack {
        switch store.theme {
        case "lunar":
          LunarSky()
        case "aurora":
          AuroraSky()
        case "nebula":
          NebulaSky()
        case "synthwave":
          SynthwaveSky()
        case "ocean":
          NightOcean()
        case "snow":
          Snowfall()
        case "fireflies":
          Fireflies()
        case "seasons":
          Seasons()
        case "rain":
          RainyNight()
        case "meteors":
          MeteorShower()
        case "galaxy":
          SpiralGalaxy()
        case "lava":
          LavaLamp()
        case "waveform":
          Waveform()
        case "city":
          CityLights()
        case "bokeh":
          Bokeh()
        case "blossom":
          Blossom()
        case "summer":
          GoldenHour()
        case "leaves":
          AutumnLeaves()
        case "christmas":
          ChristmasLights()
        case "black":
          Color.black
        case "photo":
          PhotoBackdrop(image: store.photo, size: g.size)
        default:
          MovingArtwork(colors: engine.palette.map { Color(uiColor: $0) })
        }
        if store.theme != "black" {
          LinearGradient(colors: shade, startPoint: .top, endPoint: .bottom)
        }
      }
      .frame(width: g.size.width, height: g.size.height)
      .clipped()
    }
    .ignoresSafeArea()
    .allowsHitTesting(false)
    .animation(.easeInOut(duration: 0.5), value: store.theme)
  }
}

// MARK: - shared helpers

enum ThemeClockStart {
  static let date = Date()
}

/// The clock every moving background runs on. It follows the speed setting
/// and stops when "Still Background" is on.
struct ThemeClock<Content: View>: View {
  var fps: Double = 24
  @ViewBuilder var content: (Double) -> Content

  var body: some View {
    let store = ThemeStore.shared
    let speed = store.speed
    let rate: Double = switch store.refresh {
    case "120": Double(ThemeStore.maxFps)
    case "saver": max(10, fps / 2)
    default: fps
    }
    TimelineView(.animation(minimumInterval: 1.0 / rate, paused: store.still)) { ctx in
      content(ctx.date.timeIntervalSince(ThemeClockStart.date) * speed + 40)
    }
  }
}

/// Repeatable random numbers, so the stars are in the same place every launch.
struct SeededRandom {
  var seed: UInt64
  mutating func next() -> Double {
    seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
    return Double(seed >> 11) / Double(UInt64(1) << 53)
  }
}

enum StarField {
  struct Star {
    let x: CGFloat, y: CGFloat, r: CGFloat, speed: Double, base: Double
  }

  static func make(_ count: Int, seed: UInt64) -> [Star] {
    var rng = SeededRandom(seed: seed)
    return (0..<count).map { _ in
      Star(
        x: CGFloat(rng.next()), y: CGFloat(rng.next()), r: CGFloat(0.35 + rng.next() * 1.15),
        speed: 0.4 + rng.next() * 1.6, base: 0.2 + rng.next() * 0.6)
    }
  }

  static func draw(_ c: GraphicsContext, _ stars: [Star], size: CGSize, t: Double, alpha: Double = 1) {
    for (i, s) in stars.enumerated() {
      let tw = 0.5 + 0.5 * sin(t * s.speed + Double(i))
      let x = s.x * size.width, y = s.y * size.height
      c.fill(
        Path(ellipseIn: CGRect(x: x - s.r, y: y - s.r, width: s.r * 2, height: s.r * 2)),
        with: .color(.white.opacity(alpha * s.base * (0.45 + 0.55 * tw))))
    }
  }
}

func verticalGradient(_ colors: [Color], from y0: CGFloat, to y1: CGFloat) -> GraphicsContext.Shading {
  .linearGradient(Gradient(colors: colors), startPoint: CGPoint(x: 0, y: y0), endPoint: CGPoint(x: 0, y: y1))
}

func rgb(_ r: Double, _ g: Double, _ b: Double) -> Color {
  Color(red: r, green: g, blue: b)
}

// MARK: - artwork

/// The song's colours, flowing slowly like a lava lamp.
struct MovingArtwork: View {
  let colors: [Color]

  private static let fallback: [Color] = [
    rgb(0.20, 0.16, 0.42), rgb(0.10, 0.20, 0.40), rgb(0.34, 0.20, 0.46), rgb(0.08, 0.12, 0.26),
  ]

  var body: some View {
    let p = colors.isEmpty ? MovingArtwork.fallback : colors
    let c = { (i: Int) in p[i % p.count] }
    let grid: [Color] = [
      c(0), c(1), c(2),
      c(3), c(1).opacity(0.9), c(0),
      c(2), c(3), c(1),
    ]
    ThemeClock { t in
      MeshGradient(width: 3, height: 3, points: MovingArtwork.points(t), colors: grid, background: .black, smoothsColors: true)
    }
    .id(p.description)
    .transition(.opacity)
  }

  static func points(_ t: Double) -> [SIMD2<Float>] {
    func s(_ phase: Double, _ speed: Double, _ amount: Double = 0.2) -> Float {
      Float(0.5 + amount * sin(t * speed + phase))
    }
    typealias P = SIMD2<Float>
    let top = P(s(0, 0.23), 0)
    let left = P(0, s(1, 0.19))
    let mid = P(s(2, 0.17, 0.24), s(3, 0.21, 0.24))
    let right = P(1, s(4, 0.15))
    let bottom = P(s(5, 0.2), 1)
    return [P(0, 0), top, P(1, 0), left, mid, right, P(0, 1), bottom, P(1, 1)]
  }
}

// MARK: - lunar

/// Night sky with twinkling stars and tonight's moon.
struct LunarSky: View {
  private static let stars = StarField.make(150, seed: 0x9E37_79B9_7F4A_7C15)

  var body: some View {
    ThemeClock(fps: 15) { t in
      Canvas { c, size in
        c.fill(
          Path(CGRect(origin: .zero, size: size)),
          with: verticalGradient([rgb(0.05, 0.06, 0.13), rgb(0.02, 0.025, 0.06), .black], from: 0, to: size.height))
        StarField.draw(c, LunarSky.stars, size: size, t: t)

        let r = min(size.width, size.height) * 0.105
        let center = CGPoint(x: size.width * 0.76, y: max(r * 2.2, size.height * 0.13))
        let glow = r * 3.2
        c.fill(
          Path(ellipseIn: CGRect(x: center.x - glow, y: center.y - glow, width: glow * 2, height: glow * 2)),
          with: .radialGradient(
            Gradient(colors: [Color(red: 0.85, green: 0.88, blue: 1).opacity(0.16), .clear]),
            center: center, startRadius: r * 0.8, endRadius: glow))
        // the dark part of the moon, faintly visible (earthshine)
        c.fill(Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2)), with: .color(.white.opacity(0.07)))
        c.fill(LunarSky.litPath(center: center, r: r, phase: LunarSky.moonPhase(Date())), with: .color(rgb(0.93, 0.94, 0.97)))
      }
    }
  }

  /// 0 = new moon, 0.5 = full moon.
  static func moonPhase(_ d: Date) -> Double {
    let knownNewMoon = Date(timeIntervalSince1970: 947_182_440)  // 6 Jan 2000, 18:14 UTC
    let synodic = 29.530588853 * 86400
    var p = d.timeIntervalSince(knownNewMoon).truncatingRemainder(dividingBy: synodic) / synodic
    if p < 0 { p += 1 }
    return p
  }

  /// The lit part: half the rim on the sunny side, closed by the terminator (an ellipse).
  static func litPath(center: CGPoint, r: CGFloat, phase p: Double) -> Path {
    let k = CGFloat(cos(2 * Double.pi * p))
    let side: CGFloat = p < 0.5 ? 1 : -1
    let n = 48
    var path = Path()
    for i in 0...n {
      let a = -Double.pi / 2 + Double.pi * Double(i) / Double(n)
      let pt = CGPoint(x: center.x + side * r * CGFloat(cos(a)), y: center.y + r * CGFloat(sin(a)))
      if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
    }
    for i in 0...n {
      let a = Double.pi / 2 - Double.pi * Double(i) / Double(n)
      path.addLine(to: CGPoint(x: center.x + side * k * r * CGFloat(cos(a)), y: center.y + r * CGFloat(sin(a))))
    }
    path.closeSubpath()
    return path
  }
}

// MARK: - aurora

/// Northern lights: soft ribbons of green, teal and violet rippling over stars.
struct AuroraSky: View {
  private struct Band {
    let color: Color, height: Double, speed: Double, phase: Double
  }

  private static let stars = StarField.make(110, seed: 0xA5A5_1234_DEAD_BEEF)
  private static let bands: [Band] = [
    Band(color: rgb(0.25, 1.0, 0.6), height: 0.27, speed: 0.35, phase: 0),
    Band(color: rgb(0.1, 0.8, 0.85), height: 0.36, speed: 0.27, phase: 2.1),
    Band(color: rgb(0.62, 0.35, 1.0), height: 0.2, speed: 0.22, phase: 4.2),
  ]

  var body: some View {
    ThemeClock(fps: 20) { t in
      ZStack {
        Canvas { c, size in
          c.fill(
            Path(CGRect(origin: .zero, size: size)),
            with: verticalGradient([rgb(0.01, 0.04, 0.08), rgb(0.02, 0.07, 0.11), rgb(0, 0.02, 0.04)], from: 0, to: size.height))
          StarField.draw(c, AuroraSky.stars, size: size, t: t, alpha: 0.85)
        }
        Canvas { c, size in
          let w = Double(size.width), h = Double(size.height)
          for b in AuroraSky.bands {
            var top = Path()
            let steps = 40
            for i in 0...steps {
              let fx = Double(i) / Double(steps)
              let w1: Double = sin(fx * 5 + t * b.speed + b.phase)
              let w2: Double = sin(fx * 11 - t * b.speed * 1.7 + b.phase * 2)
              let y: Double = b.height + 0.07 * w1 + 0.035 * w2
              let pt = CGPoint(x: fx * w, y: y * h)
              if i == 0 { top.move(to: pt) } else { top.addLine(to: pt) }
            }
            let pulse = 0.6 + 0.3 * sin(t * 0.4 + b.phase)
            // the curtain hanging below the bright edge
            c.stroke(top.offsetBy(dx: 0, dy: h * 0.07), with: .color(b.color.opacity(0.22 * pulse)), lineWidth: h * 0.14)
            c.stroke(top, with: .color(b.color.opacity(0.35 * pulse)), lineWidth: h * 0.07)
            c.stroke(top, with: .color(b.color.opacity(0.7 * pulse)), lineWidth: h * 0.018)
          }
        }
        .blur(radius: 30)
        .blendMode(.screen)
      }
    }
  }
}

// MARK: - nebula

/// Deep-space clouds of magenta, indigo and teal, slowly swirling, with stars.
struct NebulaSky: View {
  private static let stars = StarField.make(140, seed: 0x1234_5678_9ABC_DEF1)
  private static let colors: [Color] = [
    rgb(0.02, 0.01, 0.06), rgb(0.35, 0.08, 0.45), rgb(0.03, 0.02, 0.10),
    rgb(0.10, 0.12, 0.40), rgb(0.80, 0.24, 0.52), rgb(0.05, 0.30, 0.45),
    rgb(0.02, 0.02, 0.08), rgb(0.22, 0.05, 0.36), rgb(0.01, 0.01, 0.04),
  ]

  var body: some View {
    ThemeClock(fps: 20) { t in
      ZStack {
        MeshGradient(width: 3, height: 3, points: MovingArtwork.points(t * 0.8 + 3), colors: NebulaSky.colors, background: .black, smoothsColors: true)
          .hueRotation(.degrees(22 * sin(t * 0.05)))
        Canvas { c, size in
          StarField.draw(c, NebulaSky.stars, size: size, t: t)
        }
      }
    }
  }
}

// MARK: - synthwave

/// A striped sunset sun over a neon grid that rushes toward you.
struct SynthwaveSky: View {
  private static let stars = StarField.make(70, seed: 0x5EED_0000_1111_2222)

  var body: some View {
    ThemeClock(fps: 24) { t in
      Canvas { c, size in
        let w = size.width, h = size.height
        let horizon = h * 0.56
        let haze = rgb(0.78, 0.22, 0.48)

        // sky
        c.fill(
          Path(CGRect(x: 0, y: 0, width: w, height: horizon)),
          with: verticalGradient([rgb(0.05, 0.02, 0.14), rgb(0.27, 0.05, 0.36), haze], from: 0, to: horizon))
        StarField.draw(c, SynthwaveSky.stars, size: CGSize(width: w, height: horizon * 0.6), t: t, alpha: 0.9)

        // sun, with stripes drifting down through it
        let r = min(w * 0.3, horizon * 0.42)
        let sc = CGPoint(x: w / 2, y: horizon - r * 0.4)
        let glowR = r * 1.9
        c.fill(
          Path(ellipseIn: CGRect(x: sc.x - glowR, y: sc.y - glowR, width: glowR * 2, height: glowR * 2)),
          with: .radialGradient(Gradient(colors: [rgb(1, 0.45, 0.5).opacity(0.35), .clear]), center: sc, startRadius: r * 0.7, endRadius: glowR))
        var sunLayer = c
        let sun = Path(ellipseIn: CGRect(x: sc.x - r, y: sc.y - r, width: r * 2, height: r * 2))
        sunLayer.clip(to: sun)
        var cut = Path(CGRect(x: sc.x - r, y: sc.y - r, width: r * 2, height: r * 2))
        let drift = CGFloat((t * 0.12).truncatingRemainder(dividingBy: 1))
        for i in 0..<7 {
          let pos = (CGFloat(i) + drift) / 7
          let y = sc.y + r * pos
          cut.addRect(CGRect(x: sc.x - r, y: y, width: r * 2, height: r * (0.015 + pos * 0.11)))
        }
        sunLayer.fill(
          cut,
          with: .linearGradient(
            Gradient(colors: [rgb(1, 0.88, 0.4), rgb(1, 0.45, 0.45), rgb(0.95, 0.25, 0.6)]),
            startPoint: CGPoint(x: sc.x, y: sc.y - r), endPoint: CGPoint(x: sc.x, y: sc.y + r)),
          style: FillStyle(eoFill: true))

        // floor
        let floorH = h - horizon
        c.fill(
          Path(CGRect(x: 0, y: horizon, width: w, height: floorH)),
          with: verticalGradient([rgb(0.13, 0.02, 0.2), rgb(0.03, 0, 0.07)], from: horizon, to: h))
        var lines = Path()
        let phase = (t * 0.55).truncatingRemainder(dividingBy: 1)
        for k in 0..<20 {
          let z = Double(k) + 1 - phase
          guard z > 0.2 else { continue }
          let y = horizon + floorH * CGFloat(1 / z)
          if y > h + 4 { continue }
          lines.move(to: CGPoint(x: 0, y: y))
          lines.addLine(to: CGPoint(x: w, y: y))
        }
        for i in -14...14 {
          let fi = CGFloat(i)
          lines.move(to: CGPoint(x: w / 2 + fi * w * 0.015, y: horizon))
          lines.addLine(to: CGPoint(x: w / 2 + fi * w * 0.2, y: h))
        }
        var floorLayer = c
        floorLayer.clip(to: Path(CGRect(x: 0, y: horizon, width: w, height: floorH)))
        let neon = rgb(1, 0.3, 0.78)
        floorLayer.stroke(lines, with: .color(neon.opacity(0.22)), lineWidth: 4)
        floorLayer.stroke(lines, with: .color(neon.opacity(0.85)), lineWidth: 1.1)
        floorLayer.fill(
          Path(CGRect(x: 0, y: horizon, width: w, height: floorH * 0.35)),
          with: verticalGradient([haze.opacity(0.7), .clear], from: horizon, to: horizon + floorH * 0.35))
        c.fill(Path(CGRect(x: 0, y: horizon - 1, width: w, height: 2)), with: .color(rgb(1, 0.6, 0.85).opacity(0.8)))
      }
    }
  }
}

// MARK: - ocean

/// Night waves rolling under a small moon, with moonlight glittering on the water.
struct NightOcean: View {
  private static let stars = StarField.make(80, seed: 0x0CEA_0CEA_7777_1111)

  var body: some View {
    ThemeClock(fps: 24) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        c.fill(
          Path(CGRect(origin: .zero, size: size)),
          with: verticalGradient([rgb(0.02, 0.03, 0.09), rgb(0.04, 0.08, 0.18), rgb(0.05, 0.12, 0.22)], from: 0, to: size.height * 0.5))
        StarField.draw(c, NightOcean.stars, size: CGSize(width: w, height: h * 0.42), t: t)

        let mr = w * 0.055
        let mx = w * 0.27, my = h * 0.17
        let mc = CGPoint(x: mx, y: my)
        c.fill(
          Path(ellipseIn: CGRect(x: mx - mr * 4, y: my - mr * 4, width: mr * 8, height: mr * 8)),
          with: .radialGradient(Gradient(colors: [rgb(0.8, 0.88, 1).opacity(0.18), .clear]), center: mc, startRadius: mr, endRadius: mr * 4))
        c.fill(Path(ellipseIn: CGRect(x: mx - mr, y: my - mr, width: mr * 2, height: mr * 2)), with: .color(rgb(0.93, 0.95, 0.98)))

        let layers = 6
        for i in 0..<layers {
          let fi = Double(i)
          let base = 0.47 + fi * 0.085
          let amp: Double = 5 + fi * 4
          let waves: Double = 1.5 + fi * 0.35
          let speed: Double = 0.25 + fi * 0.12
          let k: Double = 2 * Double.pi * waves
          var crest = Path()
          for s in 0...48 {
            let x = Double(s) / 48
            let a1: Double = sin(x * k + t * speed + fi * 1.3)
            let a2: Double = sin(x * k * 2.3 - t * speed * 1.4 + fi)
            let y: Double = base * h + amp * a1 + amp * 0.4 * a2
            let pt = CGPoint(x: x * w, y: y)
            if s == 0 { crest.move(to: pt) } else { crest.addLine(to: pt) }
          }
          var water = crest
          water.addLine(to: CGPoint(x: w, y: h))
          water.addLine(to: CGPoint(x: 0, y: h))
          water.closeSubpath()
          let shade = fi / Double(layers - 1)
          let waterColor = rgb(0.04 + 0.03 * (1 - shade), 0.15 - 0.09 * shade, 0.3 - 0.19 * shade)
          c.fill(water, with: .color(waterColor))
          c.stroke(crest, with: .color(.white.opacity(0.05 + 0.06 * (1 - shade))), lineWidth: 1)
        }

        // moonlight glitter on the water, under the moon
        for j in 0..<16 {
          let fj = Double(j)
          let y = h * (0.5 + fj * 0.028)
          let flicker = 0.5 + 0.5 * sin(t * 2.3 + fj * 1.7)
          let grow: Double = 1 + fj * 0.18
          let half: Double = w * 0.025 * grow * (0.5 + 0.5 * flicker)
          let gx: Double = mx - half + 6 * sin(t + fj)
          c.fill(
            Path(roundedRect: CGRect(x: gx, y: y, width: half * 2, height: 2), cornerRadius: 1),
            with: .color(rgb(0.85, 0.9, 1).opacity(0.1 + 0.18 * flicker)))
        }
      }
    }
  }
}

// MARK: - snow

/// Gentle snowfall; closer flakes are bigger and fall faster.
struct Snowfall: View {
  private struct Flake {
    let x: Double, y: Double, r: Double, speed: Double, sway: Double, phase: Double
  }

  private static let flakes: [Flake] = {
    var rng = SeededRandom(seed: 0x5A0F_5A0F_1234_9876)
    return (0..<170).map { _ in
      let depth = rng.next()
      return Flake(
        x: rng.next(), y: rng.next(), r: 0.7 + depth * 2.3, speed: 0.02 + depth * 0.06,
        sway: 0.5 + rng.next(), phase: rng.next() * 6.28)
    }
  }()

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        c.fill(
          Path(CGRect(origin: .zero, size: size)),
          with: verticalGradient([rgb(0.03, 0.05, 0.11), rgb(0.07, 0.1, 0.18), rgb(0.12, 0.15, 0.22)], from: 0, to: size.height))
        for f in Snowfall.flakes {
          let fall = (f.y + t * f.speed).truncatingRemainder(dividingBy: 1)
          let y = fall * (h + 20) - 10
          let sway: Double = sin(t * 0.6 * f.sway + f.phase)
          let x: Double = f.x * w + sway * 7 * f.r
          c.fill(
            Path(ellipseIn: CGRect(x: x - f.r, y: y - f.r, width: f.r * 2, height: f.r * 2)),
            with: .color(Color.white.opacity(0.3 + 0.55 * (f.r / 3))))
        }
      }
    }
  }
}

// MARK: - fireflies

/// Warm fireflies drifting and glowing on and off in the dark.
struct Fireflies: View {
  private struct Fly {
    let x: Double, y: Double, ax: Double, ay: Double, fx: Double, fy: Double, phase: Double, pulse: Double
  }

  private static let flies: [Fly] = {
    var rng = SeededRandom(seed: 0xF1F1_F1F1_0000_ABCD)
    return (0..<42).map { _ in
      Fly(
        x: 0.1 + rng.next() * 0.8, y: 0.15 + rng.next() * 0.8, ax: 0.04 + rng.next() * 0.1, ay: 0.03 + rng.next() * 0.08,
        fx: 0.05 + rng.next() * 0.15, fy: 0.05 + rng.next() * 0.15, phase: rng.next() * 6.28, pulse: 0.4 + rng.next() * 0.9)
    }
  }()

  var body: some View {
    ThemeClock(fps: 24) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        c.fill(
          Path(CGRect(origin: .zero, size: size)),
          with: verticalGradient([rgb(0.01, 0.03, 0.05), rgb(0.02, 0.07, 0.07), rgb(0.03, 0.09, 0.05)], from: 0, to: size.height))
        for f in Fireflies.flies {
          let dx: Double = f.ax * sin(t * f.fx + f.phase)
          let dy: Double = f.ay * cos(t * f.fy + f.phase * 1.3)
          let x: Double = (f.x + dx) * w
          let y: Double = (f.y + dy) * h
          let on = max(0, sin(t * f.pulse + f.phase))
          let b = 0.12 + 0.88 * on * on
          let glow = 16.0
          let center = CGPoint(x: x, y: y)
          c.fill(
            Path(ellipseIn: CGRect(x: x - glow, y: y - glow, width: glow * 2, height: glow * 2)),
            with: .radialGradient(Gradient(colors: [rgb(1, 0.85, 0.35).opacity(0.5 * b), .clear]), center: center, startRadius: 0, endRadius: glow))
          c.fill(Path(ellipseIn: CGRect(x: x - 1.6, y: y - 1.6, width: 3.2, height: 3.2)), with: .color(rgb(1, 0.96, 0.7).opacity(b)))
        }
      }
    }
  }
}

// MARK: - shared bits for the newer themes

/// A repeatable 0–1 number for a pair of integers (window lights, meteor paths).
func hash01(_ a: Int, _ b: Int) -> Double {
  let x = sin(Double(a) * 127.1 + Double(b) * 311.7) * 43_758.5453
  return x - x.rounded(.down)
}

func fillCircle(_ c: GraphicsContext, _ x: Double, _ y: Double, _ r: Double, _ color: Color) {
  c.fill(Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2)), with: .color(color))
}

func glowCircle(_ c: GraphicsContext, _ x: Double, _ y: Double, _ r: Double, _ color: Color, _ alpha: Double) {
  let center = CGPoint(x: x, y: y)
  c.fill(
    Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2)),
    with: .radialGradient(Gradient(colors: [color.opacity(alpha), .clear]), center: center, startRadius: 0, endRadius: r))
}

func skyFill(_ c: GraphicsContext, _ size: CGSize, _ colors: [Color]) {
  c.fill(Path(CGRect(origin: .zero, size: size)), with: verticalGradient(colors, from: 0, to: size.height))
}

// MARK: - rainy night

/// Rain streaking past a window, city lights glowing out of focus below.
struct RainyNight: View {
  private struct Drop {
    let x: Double, y: Double, len: Double, speed: Double, alpha: Double
  }
  private struct Bead {
    let x: Double, y: Double, r: Double, slide: Double, phase: Double
  }
  private struct Light {
    let x: Double, y: Double, r: Double, color: Color
  }

  private static let drops: [Drop] = {
    var rng = SeededRandom(seed: 0x2A1E_D20F_0000_1111)
    return (0..<150).map { _ in
      let d = rng.next()
      return Drop(x: rng.next(), y: rng.next(), len: 12 + d * 28, speed: 0.45 + d * 0.7, alpha: 0.06 + d * 0.2)
    }
  }()
  private static let beads: [Bead] = {
    var rng = SeededRandom(seed: 0xBEAD_BEAD_2222_3333)
    return (0..<34).map { _ in
      Bead(x: rng.next(), y: rng.next(), r: 1.5 + rng.next() * 3.5, slide: rng.next() < 0.3 ? 0.02 + rng.next() * 0.03 : 0, phase: rng.next() * 6.28)
    }
  }()
  private static let lights: [Light] = {
    var rng = SeededRandom(seed: 0x11FE_C17E_4444_5555)
    let palette = [rgb(1, 0.62, 0.3), rgb(0.35, 0.8, 0.9), rgb(1, 0.4, 0.45), rgb(0.95, 0.85, 0.5)]
    return (0..<14).map { i in
      Light(x: rng.next(), y: 0.62 + rng.next() * 0.36, r: 30 + rng.next() * 60, color: palette[i % palette.count])
    }
  }()

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.03, 0.04, 0.07), rgb(0.05, 0.07, 0.11), rgb(0.08, 0.08, 0.12)])
        for (i, l) in RainyNight.lights.enumerated() {
          let breathe: Double = 0.7 + 0.3 * sin(t * 0.3 + Double(i))
          glowCircle(c, l.x * w, l.y * h, l.r, l.color, 0.22 * breathe)
        }
        var streaks = Path()
        for d in RainyNight.drops {
          let fall: Double = (d.y + t * d.speed).truncatingRemainder(dividingBy: 1)
          let y: Double = fall * (h + d.len) - d.len
          let x: Double = d.x * w + fall * 18
          streaks.move(to: CGPoint(x: x, y: y))
          streaks.addLine(to: CGPoint(x: x + d.len * 0.12, y: y + d.len))
        }
        c.stroke(streaks, with: .color(rgb(0.75, 0.82, 0.95).opacity(0.22)), lineWidth: 1)
        for b in RainyNight.beads {
          let slid: Double = b.slide > 0 ? (b.y + t * b.slide).truncatingRemainder(dividingBy: 1) : b.y
          let x = b.x * w, y = slid * h
          fillCircle(c, x, y, b.r, rgb(0.7, 0.78, 0.9).opacity(0.14))
          fillCircle(c, x - b.r * 0.3, y - b.r * 0.35, b.r * 0.35, Color.white.opacity(0.35))
        }
      }
    }
  }
}

// MARK: - meteor shower

/// A starry sky with shooting stars streaking across now and then.
struct MeteorShower: View {
  private static let stars = StarField.make(170, seed: 0x3E7E_0125_6789_ABCD)

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.02, 0.02, 0.07), rgb(0.03, 0.04, 0.11), rgb(0.05, 0.04, 0.12)])
        StarField.draw(c, MeteorShower.stars, size: size, t: t)
        for slot in 0..<5 {
          let period: Double = 5 + Double(slot) * 1.7
          let local: Double = t + Double(slot) * 2.3
          let cycle = Int((local / period).rounded(.down))
          let age: Double = local - Double(cycle) * period
          let life: Double = 1.3
          guard age < life else { continue }
          let p: Double = age / life
          let sx: Double = (0.2 + 0.9 * hash01(cycle, slot)) * w
          let sy: Double = (0.02 + 0.35 * hash01(slot, cycle + 7)) * h
          let travel: Double = (0.35 + 0.25 * hash01(cycle + 3, slot)) * w
          let hx: Double = sx - travel * p
          let hy: Double = sy + travel * 0.45 * p
          let tail: Double = 90 + 80 * hash01(slot + 9, cycle)
          let fade: Double = sin(p * .pi)
          let head = CGPoint(x: hx, y: hy)
          let end = CGPoint(x: hx + tail * 0.9, y: hy - tail * 0.4)
          var line = Path()
          line.move(to: head)
          line.addLine(to: end)
          c.stroke(line, with: .linearGradient(Gradient(colors: [Color.white.opacity(0.95 * fade), .clear]), startPoint: head, endPoint: end), lineWidth: 1.8)
          glowCircle(c, hx, hy, 7, rgb(0.8, 0.88, 1), 0.8 * fade)
        }
      }
    }
  }
}

// MARK: - galaxy

/// A spiral galaxy turning slowly; the middle turns faster than the edge.
struct SpiralGalaxy: View {
  private struct Dot {
    let r: Double, angle: Double, size: Double, color: Color, spread: Double
  }

  private static let stars = StarField.make(110, seed: 0x6A1A_C7E0_1111_2222)
  private static let dots: [Dot] = {
    var rng = SeededRandom(seed: 0x5917_A1A1_3333_4444)
    return (0..<620).map { i in
      let arm = Double(i % 3)
      let r: Double = pow(rng.next(), 1.5)
      let angle: Double = arm * 2.0944 + r * 4.4 + (rng.next() - 0.5) * 0.5
      let warm: Double = max(0, 1 - r * 2.2)
      let color = rgb(0.55 + 0.45 * warm, 0.55 + 0.35 * warm, 1 - 0.25 * warm)
      return Dot(r: r, angle: angle, size: 0.5 + rng.next() * 1.3, color: color, spread: (rng.next() - 0.5) * 0.06)
    }
  }()

  var body: some View {
    ThemeClock(fps: 24) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.01, 0.01, 0.04), rgb(0.03, 0.02, 0.08), rgb(0.01, 0.01, 0.03)])
        StarField.draw(c, SpiralGalaxy.stars, size: size, t: t, alpha: 0.7)
        let cx = w * 0.5, cy = h * 0.36
        let scale: Double = min(w, h) * 0.62
        glowCircle(c, cx, cy, scale * 0.55, rgb(0.5, 0.4, 0.9), 0.18)
        for d in SpiralGalaxy.dots {
          let spin: Double = t * 0.05 / (0.25 + d.r)
          let a: Double = d.angle + spin
          let rr: Double = (d.r + d.spread) * scale
          let x: Double = cx + cos(a) * rr
          let y: Double = cy + sin(a) * rr * 0.5
          let alpha: Double = 0.35 + 0.6 * (1 - d.r)
          fillCircle(c, x, y, d.size, d.color.opacity(alpha))
        }
        glowCircle(c, cx, cy, scale * 0.16, rgb(1, 0.92, 0.8), 0.75)
      }
    }
  }
}

// MARK: - lava lamp

/// Warm blobs rising, merging and splitting like a lava lamp.
struct LavaLamp: View {
  private struct Blob {
    let x: Double, r: Double, speed: Double, phase: Double, sway: Double
  }

  private static let blobs: [Blob] = {
    var rng = SeededRandom(seed: 0x1A7A_1A3D_5555_6666)
    return (0..<8).map { _ in
      Blob(x: 0.15 + rng.next() * 0.7, r: 0.09 + rng.next() * 0.09, speed: 0.05 + rng.next() * 0.07, phase: rng.next() * 6.28, sway: 0.03 + rng.next() * 0.06)
    }
  }()

  var body: some View {
    ThemeClock(fps: 24) { t in
      ZStack {
        LinearGradient(colors: [rgb(0.1, 0.02, 0.08), rgb(0.05, 0.01, 0.06)], startPoint: .top, endPoint: .bottom)
        LinearGradient(colors: [rgb(1, 0.45, 0.25), rgb(0.95, 0.2, 0.45), rgb(0.6, 0.15, 0.7)], startPoint: .top, endPoint: .bottom)
          .hueRotation(.degrees(18 * sin(t * 0.07)))
          .mask(
            Canvas { c, size in
              let w = Double(size.width), h = Double(size.height)
              c.addFilter(.alphaThreshold(min: 0.5, color: .white))
              c.addFilter(.blur(radius: 26))
              c.drawLayer { layer in
                for b in LavaLamp.blobs {
                  let y: Double = 0.5 + 0.46 * sin(t * b.speed + b.phase)
                  let x: Double = b.x + b.sway * sin(t * b.speed * 1.7 + b.phase)
                  let r: Double = b.r * min(w, h)
                  fillCircle(layer, x * w, y * h, r, .white)
                }
              }
            }
          )
          .opacity(0.85)
      }
    }
  }
}

// MARK: - waveform

/// Glowing sound waves flowing across the screen.
struct Waveform: View {
  private static let lines: [(Color, Double, Double, Double)] = [
    (rgb(0.3, 0.85, 1), 1.0, 0.0, 1.3),
    (rgb(0.65, 0.45, 1), 0.8, 1.7, 1.0),
    (rgb(1, 0.4, 0.75), 0.65, 3.1, 1.6),
    (rgb(0.4, 1, 0.75), 0.5, 4.4, 0.8),
  ]

  var body: some View {
    ThemeClock(fps: 30) { t in
      ZStack {
        LinearGradient(colors: [rgb(0.02, 0.02, 0.06), rgb(0.03, 0.03, 0.09), rgb(0.01, 0.01, 0.04)], startPoint: .top, endPoint: .bottom)
        Canvas { c, size in
          let w = Double(size.width), h = Double(size.height)
          let mid: Double = h * 0.42
          for (color, amp, phase, freq) in Waveform.lines {
            var p = Path()
            for i in 0...80 {
              let x: Double = Double(i) / 80
              let envelope: Double = sin(x * .pi)
              let wave: Double = sin(x * 7 * freq + t * 0.9 + phase) + 0.4 * sin(x * 13 * freq - t * 1.3 + phase)
              let breathe: Double = 0.6 + 0.4 * sin(t * 0.35 + phase)
              let y: Double = mid + wave * envelope * amp * breathe * h * 0.09
              let pt = CGPoint(x: x * w, y: y)
              if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
            }
            c.stroke(p, with: .color(color.opacity(0.25)), lineWidth: 9)
            c.stroke(p, with: .color(color.opacity(0.9)), lineWidth: 1.6)
          }
        }
        .blur(radius: 1.5)
        .blendMode(.screen)
      }
    }
  }
}

// MARK: - city lights

/// A city skyline at night; windows light up and go dark one by one.
struct CityLights: View {
  private struct Building {
    let x: Double, width: Double, height: Double, near: Bool, cols: Int, rows: Int
  }

  private static let stars = StarField.make(60, seed: 0xC171_57A2_7777_8888)
  private static let buildings: [Building] = {
    var rng = SeededRandom(seed: 0xB01D_1265_9999_0000)
    var out: [Building] = []
    for near in [false, true] {
      var x = -0.02
      while x < 1.02 {
        let bw: Double = near ? 0.09 + rng.next() * 0.1 : 0.06 + rng.next() * 0.08
        let bh: Double = near ? 0.16 + rng.next() * 0.26 : 0.22 + rng.next() * 0.22
        out.append(Building(x: x, width: bw, height: bh, near: near, cols: max(2, Int(bw * 40)), rows: max(4, Int(bh * 55))))
        x += bw + (near ? 0.006 : 0.004)
      }
    }
    return out
  }()

  var body: some View {
    ThemeClock(fps: 12) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.03, 0.03, 0.1), rgb(0.1, 0.06, 0.2), rgb(0.28, 0.12, 0.25)])
        StarField.draw(c, CityLights.stars, size: CGSize(width: w, height: h * 0.4), t: t, alpha: 0.6)
        glowCircle(c, w * 0.5, h, h * 0.55, rgb(1, 0.5, 0.35), 0.18)
        for (bi, b) in CityLights.buildings.enumerated() {
          let left: Double = b.x * w, bw: Double = b.width * w, bh: Double = b.height * h
          let top: Double = h - bh
          let body = b.near ? rgb(0.04, 0.035, 0.07) : rgb(0.08, 0.06, 0.13)
          c.fill(Path(CGRect(x: left, y: top, width: bw, height: bh)), with: .color(body))
          let cw: Double = bw / Double(b.cols), rh: Double = min(9, bh / Double(b.rows))
          let tick = Int((t / (b.near ? 9 : 14)).rounded(.down))
          var lit = Path()
          for r in 1..<b.rows {
            let wy: Double = top + Double(r) * rh + 2
            if wy > h - 6 { break }
            for col in 0..<b.cols {
              let seed = bi * 1000 + r * 37 + col
              let change = Int(hash01(seed, 3) * 5)
              guard hash01(seed, tick + change) > 0.62 else { continue }
              lit.addRect(CGRect(x: left + Double(col) * cw + cw * 0.25, y: wy, width: cw * 0.5, height: rh * 0.5))
            }
          }
          c.fill(lit, with: .color(rgb(1, 0.82, 0.5).opacity(b.near ? 0.75 : 0.4)))
        }
        // a red light blinking on the tallest tower
        if let tall = CityLights.buildings.filter({ $0.near }).max(by: { $0.height < $1.height }) {
          let on: Double = sin(t * 2.4) > 0.6 ? 1 : 0.15
          glowCircle(c, (tall.x + tall.width / 2) * w, h - tall.height * h - 4, 8, rgb(1, 0.2, 0.2), 0.9 * on)
        }
      }
    }
  }
}

// MARK: - bokeh

/// Soft out-of-focus lights drifting and breathing.
struct Bokeh: View {
  private struct Orb {
    let x: Double, y: Double, r: Double, color: Color, drift: Double, phase: Double
  }

  private static let orbs: [Orb] = {
    var rng = SeededRandom(seed: 0xB0CE_0000_ABCD_1234)
    let palette = [rgb(1, 0.7, 0.4), rgb(0.5, 0.75, 1), rgb(1, 0.5, 0.7), rgb(0.7, 0.55, 1), rgb(0.55, 1, 0.85)]
    return (0..<28).map { i in
      Orb(x: rng.next(), y: rng.next(), r: 18 + rng.next() * 70, color: palette[i % palette.count], drift: 0.004 + rng.next() * 0.012, phase: rng.next() * 6.28)
    }
  }()

  var body: some View {
    ThemeClock(fps: 20) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.03, 0.02, 0.06), rgb(0.05, 0.03, 0.09), rgb(0.02, 0.02, 0.04)])
        for o in Bokeh.orbs {
          let y: Double = (o.y - t * o.drift).truncatingRemainder(dividingBy: 1.2)
          let yy: Double = (y < 0 ? y + 1.2 : y) * h - 0.1 * h
          let x: Double = o.x * w + 24 * sin(t * 0.1 + o.phase)
          let breathe: Double = 0.55 + 0.45 * sin(t * 0.4 + o.phase)
          glowCircle(c, x, yy, o.r, o.color, 0.32 * breathe)
          c.stroke(Path(ellipseIn: CGRect(x: x - o.r * 0.62, y: yy - o.r * 0.62, width: o.r * 1.24, height: o.r * 1.24)), with: .color(o.color.opacity(0.1 * breathe)), lineWidth: 1.2)
        }
      }
      .blur(radius: 2)
    }
  }
}

// MARK: - falling things (blossom, leaves)

private struct Faller {
  let x: Double, y: Double, size: Double, speed: Double, sway: Double, spin: Double, phase: Double, tint: Int

  static func make(_ n: Int, seed: UInt64) -> [Faller] {
    var rng = SeededRandom(seed: seed)
    return (0..<n).map { _ in
      let depth = rng.next()
      return Faller(
        x: rng.next(), y: rng.next(), size: 3 + depth * 6, speed: 0.025 + depth * 0.05, sway: 10 + rng.next() * 30,
        spin: 0.4 + rng.next() * 1.4, phase: rng.next() * 6.28, tint: Int(rng.next() * 4))
    }
  }

  /// Where this one is at time t, its turn and its flip (for a tumbling look).
  func place(t: Double, w: Double, h: Double) -> (x: Double, y: Double, angle: Double, flip: Double) {
    let fall: Double = (y + t * speed).truncatingRemainder(dividingBy: 1)
    let yy: Double = fall * (h + 40) - 20
    let xx: Double = x * w + sway * sin(t * 0.5 + phase) + fall * 40
    return (xx, yy, t * spin + phase, cos(t * spin * 1.3 + phase))
  }
}

func drawFaller(_ c: GraphicsContext, _ f: (x: Double, y: Double, angle: Double, flip: Double), shape: Path, color: Color) {
  var g = c
  g.translateBy(x: f.x, y: f.y)
  g.rotate(by: .radians(f.angle))
  g.scaleBy(x: max(0.15, abs(f.flip)), y: 1)
  g.fill(shape, with: .color(color))
}

/// Spring: cherry blossom petals drifting through a pink dusk.
struct Blossom: View {
  private static let petals = Faller.make(70, seed: 0x5A4B_0BB0_1357_2468)
  private static let tints = [rgb(1, 0.78, 0.86), rgb(1, 0.68, 0.8), rgb(1, 0.88, 0.92), rgb(0.98, 0.6, 0.75)]

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.1, 0.07, 0.2), rgb(0.32, 0.16, 0.32), rgb(0.55, 0.3, 0.42)])
        glowCircle(c, w * 0.72, h * 0.16, w * 0.3, rgb(1, 0.85, 0.9), 0.25)
        fillCircle(c, w * 0.72, h * 0.16, w * 0.06, rgb(1, 0.95, 0.96).opacity(0.9))
        for p in Blossom.petals {
          let s = p.size
          let petal = Path(ellipseIn: CGRect(x: -s, y: -s * 0.55, width: s * 2, height: s * 1.1))
          drawFaller(c, p.place(t: t, w: w, h: h), shape: petal, color: Blossom.tints[p.tint].opacity(0.85))
        }
      }
    }
  }
}

/// Autumn: leaves tumbling down under a low harvest moon.
struct AutumnLeaves: View {
  private static let leaves = Faller.make(46, seed: 0xA070_1EAF_8642_9753)
  private static let tints = [rgb(0.95, 0.45, 0.15), rgb(0.85, 0.25, 0.12), rgb(0.98, 0.7, 0.2), rgb(0.6, 0.3, 0.12)]

  private static func leaf(_ s: Double) -> Path {
    var p = Path()
    p.move(to: CGPoint(x: -s * 1.4, y: 0))
    p.addQuadCurve(to: CGPoint(x: s * 1.4, y: 0), control: CGPoint(x: 0, y: -s * 1.3))
    p.addQuadCurve(to: CGPoint(x: -s * 1.4, y: 0), control: CGPoint(x: 0, y: s * 1.3))
    return p
  }

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.06, 0.04, 0.1), rgb(0.2, 0.08, 0.1), rgb(0.38, 0.16, 0.08)])
        glowCircle(c, w * 0.3, h * 0.24, w * 0.35, rgb(1, 0.6, 0.25), 0.28)
        fillCircle(c, w * 0.3, h * 0.24, w * 0.09, rgb(1, 0.8, 0.5).opacity(0.92))
        for l in AutumnLeaves.leaves {
          drawFaller(c, l.place(t: t * 0.9, w: w, h: h), shape: AutumnLeaves.leaf(l.size), color: AutumnLeaves.tints[l.tint].opacity(0.9))
        }
      }
    }
  }
}

// MARK: - golden hour

/// Summer: the sun setting into a glittering sea, warm specks of light in the air.
struct GoldenHour: View {
  private static let motes = Faller.make(30, seed: 0x6011_DE40_0000_FFFF)

  var body: some View {
    ThemeClock(fps: 24) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        let horizon: Double = h * 0.6
        c.fill(
          Path(CGRect(x: 0, y: 0, width: w, height: horizon)),
          with: verticalGradient([rgb(0.12, 0.1, 0.3), rgb(0.7, 0.3, 0.35), rgb(1, 0.62, 0.3)], from: 0, to: horizon))
        let r: Double = w * 0.17
        glowCircle(c, w * 0.5, horizon, r * 3.2, rgb(1, 0.7, 0.35), 0.45)
        var sky = c
        sky.clip(to: Path(CGRect(x: 0, y: 0, width: w, height: horizon)))
        fillCircle(sky, w * 0.5, horizon - r * 0.35, r, rgb(1, 0.86, 0.55))
        c.fill(
          Path(CGRect(x: 0, y: horizon, width: w, height: h - horizon)),
          with: verticalGradient([rgb(0.55, 0.25, 0.25), rgb(0.15, 0.08, 0.18), rgb(0.05, 0.03, 0.08)], from: horizon, to: h))
        // the sun's path on the water
        for j in 0..<22 {
          let fj = Double(j)
          let y: Double = horizon + 4 + fj * fj * 0.9
          if y > h { break }
          let flicker: Double = 0.5 + 0.5 * sin(t * 2 + fj * 1.9)
          let half: Double = r * (0.4 + fj * 0.07) * (0.6 + 0.4 * flicker)
          let x: Double = w * 0.5 - half + 5 * sin(t * 0.8 + fj)
          c.fill(Path(roundedRect: CGRect(x: x, y: y, width: half * 2, height: 2.5), cornerRadius: 1.2), with: .color(rgb(1, 0.8, 0.5).opacity(0.18 + 0.3 * flicker)))
        }
        for m in GoldenHour.motes {
          let p = m.place(t: -t * 0.6, w: w, h: horizon)
          let tw: Double = 0.4 + 0.6 * abs(sin(t * m.spin + m.phase))
          glowCircle(c, p.x, p.y, m.size * 1.6, rgb(1, 0.85, 0.55), 0.5 * tw)
        }
      }
    }
  }
}

// MARK: - christmas

/// Winter: snow falling past strings of twinkling fairy lights, pine trees below.
struct ChristmasLights: View {
  private static let flakes = Faller.make(120, seed: 0xC4A1_5735_0000_1225)
  private static let bulbColors = [rgb(1, 0.3, 0.3), rgb(0.35, 1, 0.5), rgb(1, 0.82, 0.35), rgb(0.4, 0.65, 1)]

  private static func tree(_ c: GraphicsContext, x: Double, base: Double, height: Double) {
    let green = rgb(0.03, 0.12, 0.08)
    for k in 0..<3 {
      let fk = Double(k)
      let top: Double = base - height + fk * height * 0.22
      let half: Double = height * (0.22 + fk * 0.1)
      let bottom: Double = top + height * 0.42
      var p = Path()
      p.move(to: CGPoint(x: x, y: top))
      p.addLine(to: CGPoint(x: x + half, y: bottom))
      p.addLine(to: CGPoint(x: x - half, y: bottom))
      p.closeSubpath()
      c.fill(p, with: .color(green))
    }
  }

  var body: some View {
    ThemeClock(fps: 30) { t in
      Canvas { c, size in
        let w = Double(size.width), h = Double(size.height)
        skyFill(c, size, [rgb(0.02, 0.03, 0.09), rgb(0.04, 0.06, 0.15), rgb(0.08, 0.1, 0.2)])
        // fairy lights hanging in two loops
        for (row, y0) in [0.07, 0.17].enumerated() {
          let sag: Double = h * 0.05
          var wire = Path()
          let n = 16
          for i in 0...n {
            let x: Double = Double(i) / Double(n)
            let u: Double = 2 * x - 1
            let y: Double = y0 * h + sag * (1 - u * u)
            if i == 0 { wire.move(to: CGPoint(x: x * w, y: y)) } else { wire.addLine(to: CGPoint(x: x * w, y: y)) }
          }
          c.stroke(wire, with: .color(Color.white.opacity(0.12)), lineWidth: 1)
          for i in 1..<n {
            let x: Double = Double(i) / Double(n)
            let u: Double = 2 * x - 1
            let y: Double = y0 * h + sag * (1 - u * u) + 5
            let color = ChristmasLights.bulbColors[(i + row) % 4]
            let tw: Double = 0.45 + 0.55 * max(0, sin(t * 1.6 + Double(i * 3 + row * 5)))
            glowCircle(c, x * w, y, 14, color, 0.55 * tw)
            fillCircle(c, x * w, y, 2.6, color.opacity(0.5 + 0.5 * tw))
          }
        }
        // snowy ground and trees
        c.fill(Path(CGRect(x: 0, y: h * 0.86, width: w, height: h * 0.14)), with: verticalGradient([rgb(0.55, 0.6, 0.72), rgb(0.3, 0.34, 0.45)], from: h * 0.86, to: h))
        for (i, x) in [0.12, 0.3, 0.78, 0.92].enumerated() {
          ChristmasLights.tree(c, x: x * w, base: h * 0.88, height: h * (0.16 + 0.05 * Double(i % 2)))
        }
        for f in ChristmasLights.flakes {
          let p = f.place(t: t * 0.8, w: w, h: h)
          fillCircle(c, p.x, p.y, f.size * 0.35, Color.white.opacity(0.35 + 0.08 * f.size))
        }
      }
    }
  }
}

// MARK: - seasons

/// The special one: the scene changes with the season — blossom in spring,
/// golden hour in summer, falling leaves in autumn, Christmas lights in winter.
struct Seasons: View {
  /// "spring" | "summer" | "autumn" | "christmas" for a date (northern hemisphere).
  static func season(_ d: Date = Date()) -> String {
    switch Calendar.current.component(.month, from: d) {
    case 3...5: return "spring"
    case 6...8: return "summer"
    case 9...11: return "autumn"
    default: return "christmas"
    }
  }

  var body: some View {
    switch Seasons.season() {
    case "spring": Blossom()
    case "summer": GoldenHour()
    case "autumn": AutumnLeaves()
    default: ChristmasLights()
    }
  }
}

// MARK: - photo

/// Your own picture, blurred so text on top stays readable.
struct PhotoBackdrop: View {
  let image: UIImage?
  let size: CGSize

  var body: some View {
    if let image {
      Image(uiImage: image)
        .resizable()
        .scaledToFill()
        .frame(width: size.width, height: size.height)
        .blur(radius: 22)
        .clipped()
    } else {
      Color.black
    }
  }
}

// MARK: - colours from the artwork

enum Palette {
  /// Up to four strong, different colours from an image, softened for a background.
  static func colors(from img: UIImage) -> [UIColor] {
    guard let cg = img.cgImage else { return [] }
    let side = 10
    var px = [UInt8](repeating: 0, count: side * side * 4)
    let ok = px.withUnsafeMutableBytes { buf -> Bool in
      guard
        let ctx = CGContext(
          data: buf.baseAddress, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
          space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
      else { return false }
      ctx.interpolationQuality = .medium
      ctx.draw(cg, in: CGRect(x: 0, y: 0, width: side, height: side))
      return true
    }
    guard ok else { return [] }

    struct C { let r, g, b, score: CGFloat }
    var all: [C] = []
    for i in 0..<(side * side) {
      let r = CGFloat(px[i * 4]) / 255, g = CGFloat(px[i * 4 + 1]) / 255, b = CGFloat(px[i * 4 + 2]) / 255
      var h: CGFloat = 0, s: CGFloat = 0, v: CGFloat = 0, a: CGFloat = 0
      UIColor(red: r, green: g, blue: b, alpha: 1).getHue(&h, saturation: &s, brightness: &v, alpha: &a)
      let score = v < 0.12 ? 0 : s * 0.65 + v * 0.35
      all.append(C(r: r, g: g, b: b, score: score))
    }
    all.sort { $0.score > $1.score }
    var picked: [C] = []
    for c in all {
      let far = picked.allSatisfy { p in
        let d = (p.r - c.r) * (p.r - c.r) + (p.g - c.g) * (p.g - c.g) + (p.b - c.b) * (p.b - c.b)
        return d > 0.04
      }
      if far { picked.append(c) }
      if picked.count == 4 { break }
    }
    if picked.isEmpty, let first = all.first { picked = [first] }

    return picked.map { c in
      var h: CGFloat = 0, s: CGFloat = 0, v: CGFloat = 0, a: CGFloat = 0
      UIColor(red: c.r, green: c.g, blue: c.b, alpha: 1).getHue(&h, saturation: &s, brightness: &v, alpha: &a)
      return UIColor(hue: h, saturation: min(1, s * 1.15), brightness: min(0.78, max(0.32, v)), alpha: 1)
    }
  }
}
