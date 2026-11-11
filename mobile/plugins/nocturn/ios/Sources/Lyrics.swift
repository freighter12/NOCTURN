import SwiftUI

// Lyrics in Now Playing — the desktop app's lyrics, natively.
//
// The web side builds the timing (same code as the desktop app,
// src/music/lyricsTiming.ts): lines, "gap" items for intros and instrumental
// breaks (three dots that fill up), and per-word timings — real ones from
// enhanced LRC, or estimated from syllables. Here we only draw them:
// the clock is read from the player every frame, shifted by the output
// latency (AirPods/Bluetooth lag) and the user's per-song nudge.

struct LyricWord: Decodable, Equatable, Sendable {
  let t: Double
  let w: String
}

struct LyricItem: Decodable, Equatable, Sendable {
  let kind: String  // "line" | "gap"
  let start: Double
  let end: Double
  let text: String
  let words: [LyricWord]
  let estimated: Bool?
  let sungEnd: Double?
}

enum LyricsTiming {
  /// Index of the item playing at `t` (last one that has started), or -1.
  static func activeIndex(_ items: [LyricItem], _ t: Double) -> Int {
    var lo = 0, hi = items.count - 1, ans = -1
    while lo <= hi {
      let mid = (lo + hi) / 2
      if items[mid].start <= t {
        ans = mid
        lo = mid + 1
      } else {
        hi = mid - 1
      }
    }
    return ans
  }

  static func progress(_ it: LyricItem, _ t: Double) -> Double {
    min(1, max(0, (t - it.start) / max(1, it.end - it.start)))
  }

  /// 0–1 for each word (a word fills until the next one starts).
  static func wordProgress(_ it: LyricItem, _ t: Double) -> [Double] {
    it.words.enumerated().map { j, w in
      let next = j + 1 < it.words.count ? it.words[j + 1].t : (it.sungEnd ?? min(it.end, w.t + 900))
      return min(1, max(0, (t - w.t) / max(1, next - w.t)))
    }
  }
}

struct LyricsView: View {
  let engine: PlayerEngine
  @AppStorage("lyrics.wordSync") private var wordSync = true
  @State private var lastUserScroll = Date.distantPast

  private var mine: Bool { engine.lyricsTrackId == engine.current?.videoId }
  private var items: [LyricItem] { mine ? engine.lyricItems : [] }

  var body: some View {
    GeometryReader { g in
      // Big and readable, scaled to the screen: ~27 pt on small iPhones, up to 32 on Pro Max.
      let size = min(32, max(24, g.size.width * 0.072))
      Group {
        if !mine {
          artworkFallback(text: "Looking for lyrics…", in: g.size)
        } else if items.isEmpty {
          if engine.lyricsPlain.isEmpty {
            artworkFallback(text: "No lyrics for this song", in: g.size)
          } else {
            plain(size: size * 0.8)
          }
        } else {
          synced(size: size)
        }
      }
      .frame(width: g.size.width, height: g.size.height)
    }
  }

  // MARK: synced lines

  private func synced(size: CGFloat) -> some View {
    ScrollViewReader { proxy in
      // 10× a second is plenty to move between lines; the active line animates every frame.
      TimelineView(.periodic(from: .now, by: 0.1)) { _ in
        let t = engine.lyricClockMs() + engine.lyricsOffset
        let active = LyricsTiming.activeIndex(items, t)
        ScrollView(.vertical, showsIndicators: false) {
          LazyVStack(alignment: .leading, spacing: size * 0.62) {
            Color.clear.frame(height: 28)
            ForEach(Array(items.enumerated()), id: \.offset) { i, item in
              row(item, index: i, active: active, size: size)
                .id(i)
                .contentShape(Rectangle())
                .onTapGesture {
                  engine.seek(ms: max(0, item.start + engine.outputLatencyMs - engine.lyricsOffset))
                  lastUserScroll = .distantPast
                }
            }
            Color.clear.frame(height: 160)
          }
          .padding(.horizontal, 24)
          .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollBounceBehavior(.basedOnSize)
        .simultaneousGesture(DragGesture(minimumDistance: 8).onChanged { _ in lastUserScroll = Date() })
        .onChange(of: active) { _, a in
          // Keep the sung line in the upper third — unless you're scrolling yourself.
          guard a >= 0, Date().timeIntervalSince(lastUserScroll) > 3.5 else { return }
          withAnimation(.spring(response: 0.6, dampingFraction: 0.9)) {
            proxy.scrollTo(a, anchor: UnitPoint(x: 0.5, y: 0.3))
          }
        }
        .onAppear {
          if active >= 0 { proxy.scrollTo(active, anchor: UnitPoint(x: 0.5, y: 0.3)) }
        }
      }
    }
    .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.08), .init(color: .black, location: 0.88), .init(color: .clear, location: 1)], startPoint: .top, endPoint: .bottom))
  }

  @ViewBuilder
  private func row(_ item: LyricItem, index i: Int, active: Int, size: CGFloat) -> some View {
    let distance = active < 0 ? 99 : abs(i - active)
    let scrolling = Date().timeIntervalSince(lastUserScroll) < 3.5
    let blur: CGFloat = scrolling || i == active ? 0 : (distance == 1 ? 0.5 : distance == 2 ? 1.1 : 1.8)
    Group {
      if i == active {
        // every frame for the line being sung
        TimelineView(.animation) { _ in
          activeRow(item, t: engine.lyricClockMs() + engine.lyricsOffset, size: size)
        }
      } else if item.kind == "gap" {
        GapDots(progress: i < active ? 1 : 0)
      } else {
        Text(item.text)
          .font(.system(size: size, weight: .bold))
          .foregroundStyle(.white.opacity(i < active ? 0.38 : (distance == 1 ? 0.5 : 0.32)))
      }
    }
    .fixedSize(horizontal: false, vertical: true)
    .multilineTextAlignment(.leading)
    .blur(radius: blur)
    .scaleEffect(i == active ? 1 : 0.96, anchor: .leading)
    .animation(.spring(response: 0.45, dampingFraction: 0.85), value: active)
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  @ViewBuilder
  private func activeRow(_ item: LyricItem, t: Double, size: CGFloat) -> some View {
    if item.kind == "gap" {
      GapDots(progress: LyricsTiming.progress(item, t))
    } else if !item.words.isEmpty && (wordSync || item.estimated != true) {
      // word by word: each word brightens as it's sung
      let wp = LyricsTiming.wordProgress(item, t)
      item.words.enumerated().reduce(Text("")) { acc, pair in
        let (j, w) = pair
        let p = j < wp.count ? wp[j] : 0
        return acc + Text(w.w).foregroundStyle(.white.opacity(0.35 + 0.65 * p))
      }
      .font(.system(size: size, weight: .bold))
      .shadow(color: .white.opacity(0.28), radius: 12)
    } else {
      Text(item.text)
        .font(.system(size: size, weight: .bold))
        .foregroundStyle(.white)
        .shadow(color: .white.opacity(0.28), radius: 12)
    }
  }

  // MARK: plain (not synced) and no lyrics

  private func plain(size: CGFloat) -> some View {
    ScrollView(.vertical, showsIndicators: false) {
      VStack(alignment: .leading, spacing: 14) {
        Text(engine.lyricsPlain)
          .font(.system(size: size, weight: .semibold))
          .foregroundStyle(.white.opacity(0.85))
          .lineSpacing(6)
          .fixedSize(horizontal: false, vertical: true)
          .textSelection(.enabled)
        Text("Lyrics from LRCLIB (not synced to this version)")
          .font(.footnote)
          .foregroundStyle(.white.opacity(0.45))
      }
      .padding(.horizontal, 24)
      .padding(.vertical, 24)
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }

  /// No lyrics: show the artwork big instead, so the screen never looks empty.
  private func artworkFallback(text: String, in size: CGSize) -> some View {
    VStack(spacing: 16) {
      ArtworkView(image: engine.artworkImage, corner: 16)
        .frame(width: max(120, min(size.width - 72, size.height - 70, 340)), height: max(120, min(size.width - 72, size.height - 70, 340)))
        .scaleEffect(engine.playing ? 1 : 0.92)
        .shadow(color: .black.opacity(0.45), radius: 28, y: 16)
        .animation(.spring(response: 0.45, dampingFraction: 0.75), value: engine.playing)
      Text(text)
        .font(.subheadline)
        .foregroundStyle(.white.opacity(0.55))
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }
}

/// Three dots that fill up through an instrumental break.
struct GapDots: View {
  let progress: Double

  var body: some View {
    HStack(spacing: 9) {
      ForEach(0..<3, id: \.self) { k in
        let p = min(1, max(0, progress * 3 - Double(k)))
        Circle()
          .fill(.white.opacity(0.25 + 0.75 * p))
          .frame(width: 11, height: 11)
          .scaleEffect(0.8 + 0.3 * p)
      }
    }
    .padding(.vertical, 8)
    .accessibilityLabel("Instrumental")
  }
}
