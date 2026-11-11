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
  @State private var offset: Double = 0
  @State private var lastUserScroll = Date.distantPast
  @State private var shownActive = -1

  private var mine: Bool { engine.lyricsTrackId == engine.current?.videoId }
  private var items: [LyricItem] { mine ? engine.lyricItems : [] }
  private var trackId: String { engine.current?.videoId ?? "" }

  var body: some View {
    VStack(spacing: 0) {
      if !mine {
        placeholder(icon: "quote.bubble", text: "Looking for lyrics…")
      } else if items.isEmpty {
        if engine.lyricsPlain.isEmpty {
          placeholder(icon: "quote.bubble", text: "No lyrics found for this song")
        } else {
          plain
        }
      } else {
        synced
        controls
      }
    }
    .onAppear { offset = Self.savedOffset(trackId) }
    .onChange(of: trackId) { _, id in
      offset = Self.savedOffset(id)
      shownActive = -1
    }
  }

  // MARK: synced lines

  private var synced: some View {
    ScrollViewReader { proxy in
      // 10× a second is plenty to move between lines; the active line animates every frame.
      TimelineView(.periodic(from: .now, by: 0.1)) { _ in
        let t = engine.lyricClockMs() + offset
        let active = LyricsTiming.activeIndex(items, t)
        ScrollView(showsIndicators: false) {
          LazyVStack(alignment: .leading, spacing: 20) {
            Color.clear.frame(height: 90)
            ForEach(Array(items.enumerated()), id: \.offset) { i, item in
              row(item, index: i, active: active)
                .id(i)
                .contentShape(Rectangle())
                .onTapGesture {
                  engine.seek(ms: max(0, item.start + engine.outputLatencyMs - offset))
                  lastUserScroll = .distantPast
                }
            }
            Color.clear.frame(height: 260)
          }
          .padding(.horizontal, 28)
        }
        .simultaneousGesture(DragGesture(minimumDistance: 8).onChanged { _ in lastUserScroll = Date() })
        .onChange(of: active) { _, a in
          shownActive = a
          // Keep the line a little above the middle — unless you're scrolling yourself.
          guard a >= 0, Date().timeIntervalSince(lastUserScroll) > 3.5 else { return }
          withAnimation(.spring(response: 0.55, dampingFraction: 0.9)) {
            proxy.scrollTo(a, anchor: UnitPoint(x: 0.5, y: 0.32))
          }
        }
      }
    }
    .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.12), .init(color: .black, location: 0.82), .init(color: .clear, location: 1)], startPoint: .top, endPoint: .bottom))
  }

  @ViewBuilder
  private func row(_ item: LyricItem, index i: Int, active: Int) -> some View {
    let distance = active < 0 ? 99 : abs(i - active)
    let scrolling = Date().timeIntervalSince(lastUserScroll) < 3.5
    let blur: CGFloat = scrolling || i == active ? 0 : (distance == 1 ? 0.6 : distance == 2 ? 1.2 : 2)
    Group {
      if i == active {
        // every frame for the line being sung
        TimelineView(.animation) { _ in
          activeRow(item, t: engine.lyricClockMs() + offset)
        }
      } else if item.kind == "gap" {
        GapDots(progress: i < active ? 1 : 0)
      } else {
        Text(item.text)
          .font(.system(size: 28, weight: .bold))
          .foregroundStyle(.white.opacity(i < active ? 0.42 : 0.3))
      }
    }
    .blur(radius: blur)
    .scaleEffect(i == active ? 1 : 0.97, anchor: .leading)
    .animation(.easeOut(duration: 0.35), value: active)
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  @ViewBuilder
  private func activeRow(_ item: LyricItem, t: Double) -> some View {
    if item.kind == "gap" {
      GapDots(progress: LyricsTiming.progress(item, t))
    } else if !item.words.isEmpty && (wordSync || item.estimated != true) {
      // word by word: each word brightens as it's sung
      let wp = LyricsTiming.wordProgress(item, t)
      item.words.enumerated().reduce(Text("")) { acc, pair in
        let (j, w) = pair
        let p = j < wp.count ? wp[j] : 0
        return acc + Text(w.w).foregroundStyle(.white.opacity(0.32 + 0.68 * p))
      }
      .font(.system(size: 28, weight: .bold))
      .shadow(color: .white.opacity(0.25), radius: 10)
    } else {
      Text(item.text)
        .font(.system(size: 28, weight: .bold))
        .foregroundStyle(.white)
        .shadow(color: .white.opacity(0.25), radius: 10)
    }
  }

  // MARK: plain (not synced)

  private var plain: some View {
    ScrollView(showsIndicators: false) {
      VStack(alignment: .leading, spacing: 14) {
        Text(engine.lyricsPlain)
          .font(.system(size: 22, weight: .semibold))
          .foregroundStyle(.white.opacity(0.85))
          .lineSpacing(6)
          .textSelection(.enabled)
        Text("Lyrics from LRCLIB (not synced to this version)")
          .font(.footnote)
          .foregroundStyle(.white.opacity(0.45))
      }
      .padding(.horizontal, 28)
      .padding(.vertical, 40)
    }
  }

  private func placeholder(icon: String, text: String) -> some View {
    VStack(spacing: 10) {
      Image(systemName: icon).font(.largeTitle).foregroundStyle(.white.opacity(0.4))
      Text(text).foregroundStyle(.white.opacity(0.6))
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  // MARK: timing controls (same as the desktop: Words/Lines, Later / In time / Earlier)

  private var controls: some View {
    HStack(spacing: 8) {
      Text(engine.lyricsApprox ? "≈ Approx. synced" : "LRCLIB")
        .font(.caption)
        .foregroundStyle(.white.opacity(0.45))
        .lineLimit(1)
      Spacer(minLength: 4)
      Button(wordSync ? "Words" : "Lines") { wordSync.toggle() }
      Divider().frame(height: 14).overlay(.white.opacity(0.2))
      Button("Later") { nudge(-100) }
      Button(offset == 0 ? "In time" : String(format: "%@%.1f s", offset > 0 ? "+" : "−", abs(offset) / 1000)) { nudge(nil) }
        .disabled(offset == 0)
        .monospacedDigit()
      Button("Earlier") { nudge(100) }
    }
    .font(.footnote.weight(.semibold))
    .buttonStyle(.plain)
    .foregroundStyle(.white.opacity(0.75))
    .padding(.horizontal, 28)
    .padding(.top, 6)
  }

  private func nudge(_ delta: Double?) {
    offset = delta.map { max(-15000, min(15000, offset + $0)) } ?? 0
    UserDefaults.standard.set(offset, forKey: "lyrics.offset.\(trackId)")
  }

  static func savedOffset(_ id: String) -> Double {
    UserDefaults.standard.double(forKey: "lyrics.offset.\(id)")
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
