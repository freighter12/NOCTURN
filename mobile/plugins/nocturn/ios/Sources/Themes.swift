import Observation
import SwiftUI
import UIKit

/// The background theme, chosen in Library › Settings.
///   artwork – the song's colours slowly drifting (default)
///   lunar   – night sky, stars and tonight's real moon phase
///   black   – pure black
///   photo   – a picture of your own, blurred and darkened
@MainActor
@Observable
final class ThemeStore {
  static let shared = ThemeStore()
  static let names = ["artwork", "lunar", "black", "photo"]

  private(set) var theme: String
  private(set) var photo: UIImage?

  private let photoURL: URL = {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    return base.appendingPathComponent("theme-photo.jpg")
  }()

  private init() {
    let saved = UserDefaults.standard.string(forKey: "theme") ?? "artwork"
    theme = ThemeStore.names.contains(saved) ? saved : "artwork"
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
    GeometryReader { g in
      ZStack {
        switch store.theme {
        case "lunar":
          LunarSky()
        case "black":
          Color.black
        case "photo":
          PhotoBackdrop(image: store.photo, size: g.size)
        default:
          MovingArtwork(colors: engine.palette.map { Color(uiColor: $0) })
        }
        if store.theme != "black" {
          LinearGradient(
            colors: strong
              ? [.black.opacity(0.45), .black.opacity(0.6), .black.opacity(0.75)]
              : [.black.opacity(0.18), .black.opacity(0.38), .black.opacity(0.66)],
            startPoint: .top, endPoint: .bottom)
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

/// The song's colours, flowing slowly like a lava lamp.
struct MovingArtwork: View {
  let colors: [Color]

  private static let fallback: [Color] = [
    Color(red: 0.20, green: 0.16, blue: 0.42), Color(red: 0.10, green: 0.20, blue: 0.40),
    Color(red: 0.34, green: 0.20, blue: 0.46), Color(red: 0.08, green: 0.12, blue: 0.26),
  ]

  var body: some View {
    let p = colors.isEmpty ? MovingArtwork.fallback : colors
    let c = { (i: Int) in p[i % p.count] }
    let grid: [Color] = [
      c(0), c(1), c(2),
      c(3), c(1).opacity(0.9), c(0),
      c(2), c(3), c(1),
    ]
    TimelineView(.animation(minimumInterval: 1.0 / 24)) { ctx in
      let t = ctx.date.timeIntervalSinceReferenceDate
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

/// Night sky with twinkling stars and tonight's moon.
struct LunarSky: View {
  private struct Star {
    let x: CGFloat, y: CGFloat, r: CGFloat, speed: Double, base: Double
  }

  private static let stars: [Star] = {
    var seed: UInt64 = 0x9E37_79B9_7F4A_7C15
    func rnd() -> Double {
      seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
      return Double(seed >> 11) / Double(1 << 53)
    }
    return (0..<150).map { _ in
      Star(x: rnd(), y: rnd(), r: 0.35 + rnd() * 1.15, speed: 0.4 + rnd() * 1.6, base: 0.2 + rnd() * 0.6)
    }
  }()

  var body: some View {
    TimelineView(.animation(minimumInterval: 1.0 / 15)) { ctx in
      let t = ctx.date.timeIntervalSinceReferenceDate
      Canvas { c, size in
        c.fill(
          Path(CGRect(origin: .zero, size: size)),
          with: .linearGradient(
            Gradient(colors: [Color(red: 0.05, green: 0.06, blue: 0.13), Color(red: 0.02, green: 0.025, blue: 0.06), .black]),
            startPoint: .zero, endPoint: CGPoint(x: 0, y: size.height)))

        for (i, s) in LunarSky.stars.enumerated() {
          let tw = 0.5 + 0.5 * sin(t * s.speed + Double(i))
          let x = s.x * size.width, y = s.y * size.height
          c.fill(
            Path(ellipseIn: CGRect(x: x - s.r, y: y - s.r, width: s.r * 2, height: s.r * 2)),
            with: .color(.white.opacity(s.base * (0.45 + 0.55 * tw))))
        }

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
        c.fill(LunarSky.litPath(center: center, r: r, phase: LunarSky.moonPhase(ctx.date)), with: .color(Color(red: 0.93, green: 0.94, blue: 0.97)))
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
