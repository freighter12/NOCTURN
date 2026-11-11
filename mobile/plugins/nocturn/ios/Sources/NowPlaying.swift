import AVKit
import SwiftUI

/// Now Playing — lyrics first.
///
/// One vertical stack, nothing positioned absolutely:
///   1. compact track info (handle, artwork, title, artist, menu)
///   2. synced lyrics — takes all the space in between
///   3. playback controls (progress, previous / play / next, repeat · AI Shuffle · AirPlay)
/// The blurred artwork sits in `.background`, so it can never widen the layout
/// (a scaled `.fill` image in a ZStack is what made the old screen clip).
struct NowPlayingView: View {
  let engine: PlayerEngine
  /// Closes Now Playing, then acts (go to artist / album …).
  let navigate: (String, String?) -> Void
  /// Acts while staying open (AI Shuffle on/off …).
  let signal: (String, String?) -> Void

  @Environment(\.dismiss) private var dismiss
  @AppStorage("lyrics.wordSync") private var wordSync = true
  @State private var dragDown: CGFloat = 0

  init(engine: PlayerEngine, navigate: @escaping (String, String?) -> Void, signal: @escaping (String, String?) -> Void) {
    self.engine = engine
    self.navigate = navigate
    self.signal = signal
  }

  var body: some View {
    VStack(spacing: 0) {
      header
      LyricsView(engine: engine)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
      PlaybackControls(engine: engine, signal: signal)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background { ThemeBackground(engine: engine) }
    .offset(y: max(0, dragDown))
    .preferredColorScheme(.dark)
  }

  // MARK: 1 · track info

  private var header: some View {
    VStack(spacing: 10) {
      Capsule()
        .fill(.white.opacity(0.32))
        .frame(width: 38, height: 5)
        .padding(.top, 6)
        .accessibilityLabel("Close")
        .accessibilityAddTraits(.isButton)
        .onTapGesture { dismiss() }

      HStack(spacing: 12) {
        ArtworkView(image: engine.artworkImage, corner: 9)
          .frame(width: 54, height: 54)
          .shadow(color: .black.opacity(0.35), radius: 10, y: 5)
        VStack(alignment: .leading, spacing: 2) {
          Text(engine.current?.title ?? "")
            .font(.system(.headline, design: .default).weight(.semibold))
            .foregroundStyle(.white)
            .lineLimit(2)
            .truncationMode(.tail)
            .fixedSize(horizontal: false, vertical: true)
          Text(engine.current?.artist ?? "")
            .font(.subheadline)
            .foregroundStyle(.white.opacity(0.62))
            .lineLimit(1)
            .truncationMode(.tail)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .layoutPriority(1)
        moreMenu
      }
      .padding(.horizontal, 20)
      .padding(.bottom, 6)
    }
    .contentShape(Rectangle())
    .gesture(
      DragGesture(minimumDistance: 12)
        .onChanged { g in dragDown = g.translation.height }
        .onEnded { g in
          if g.translation.height > 110 || g.predictedEndTranslation.height > 260 {
            dismiss()
          } else {
            withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { dragDown = 0 }
          }
        }
    )
  }

  private var moreMenu: some View {
    Menu {
      Section {
        Button("Go to Artist", systemImage: "music.microphone") { navigate("artist", engine.current?.artist) }
        if let album = engine.current?.album, !album.isEmpty {
          Button("Go to Album", systemImage: "square.stack") { navigate("album", album) }
        }
        Button("Start Radio", systemImage: "dot.radiowaves.left.and.right") { navigate("radio", engine.current?.videoId) }
        Button("Add to Liked Songs", systemImage: "heart") { signal("save", engine.current?.videoId) }
        Button("Up Next", systemImage: "list.bullet") { navigate("queue", nil) }
      }
      Section("Lyrics timing") {
        Button("Show Lyrics Earlier", systemImage: "arrow.up.to.line") { engine.setLyricsOffset(engine.lyricsOffset + 100) }
        Button("Show Lyrics Later", systemImage: "arrow.down.to.line") { engine.setLyricsOffset(engine.lyricsOffset - 100) }
        if engine.lyricsOffset != 0 {
          Button(String(format: "Reset Timing (%@%.1f s)", engine.lyricsOffset > 0 ? "+" : "−", abs(engine.lyricsOffset) / 1000), systemImage: "arrow.counterclockwise") {
            engine.setLyricsOffset(nil)
          }
        }
        Toggle("Word-by-Word", systemImage: "textformat.characters", isOn: $wordSync)
      }
    } label: {
      Image(systemName: "ellipsis")
        .font(.body.weight(.semibold))
        .frame(width: 36, height: 36)
        .contentShape(Circle())
    }
    .buttonStyle(.glass)
    .buttonBorderShape(.circle)
    .accessibilityLabel("More")
  }
}

// MARK: - 3 · playback controls

struct PlaybackControls: View {
  let engine: PlayerEngine
  let signal: (String, String?) -> Void

  var body: some View {
    VStack(spacing: 14) {
      Scrubber(engine: engine)
      transport
      secondary
    }
    .padding(.horizontal, 24)
    .padding(.top, 10)
    .padding(.bottom, 8)
    .background(alignment: .top) {
      // a soft fade so lyrics scroll away beneath the controls
      LinearGradient(colors: [.clear, .black.opacity(0.28)], startPoint: .top, endPoint: .bottom)
        .padding(.top, -30)
        .allowsHitTesting(false)
    }
  }

  private var transport: some View {
    GlassEffectContainer(spacing: 20) {
      HStack(spacing: 26) {
        Button {
          engine.previous()
        } label: {
          Image(systemName: "backward.fill").font(.title2).frame(width: 58, height: 58)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)
        .accessibilityLabel("Previous")

        Button {
          engine.toggle()
        } label: {
          Group {
            if engine.loading && engine.playing {
              ProgressView().controlSize(.large).tint(.white)
            } else {
              Image(systemName: engine.playing ? "pause.fill" : "play.fill")
                .font(.system(size: 32, weight: .semibold))
                .contentTransition(.symbolEffect(.replace))
            }
          }
          .frame(width: 80, height: 80)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)
        .accessibilityLabel(engine.playing ? "Pause" : "Play")

        Button {
          engine.next()
        } label: {
          Image(systemName: "forward.fill").font(.title2).frame(width: 58, height: 58)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)
        .accessibilityLabel("Next")
      }
    }
    .foregroundStyle(.white)
    .frame(maxWidth: .infinity)
  }

  private var secondary: some View {
    HStack(spacing: 0) {
      Button {
        let next = ["off": "all", "all": "one", "one": "off"][engine.repeatMode] ?? "off"
        engine.setRepeat(next)
      } label: {
        Image(systemName: engine.repeatMode == "one" ? "repeat.1" : "repeat")
          .font(.title3)
          .foregroundStyle(engine.repeatMode == "off" ? .white.opacity(0.5) : .white)
          .frame(width: 48, height: 40)
      }
      .accessibilityLabel("Repeat: \(engine.repeatMode)")
      Spacer(minLength: 8)
      AIShuffleButton(engine: engine, signal: signal)
      Spacer(minLength: 8)
      AirPlayButton().frame(width: 48, height: 40)
        .accessibilityLabel("AirPlay")
    }
    .buttonStyle(.plain)
    .padding(.horizontal, 8)
  }
}

/// Nocturn's smart shuffle: plans the next songs from the mood of what's
/// playing and how you listen (see src-tauri/src/ai.rs).
struct AIShuffleButton: View {
  let engine: PlayerEngine
  let signal: (String, String?) -> Void
  @State private var glow = false

  var body: some View {
    let on = engine.aiShuffle
    Button {
      let next = !on
      engine.setAIShuffle(next)
      signal("aiShuffle", next ? "on" : "off")
      UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    } label: {
      HStack(spacing: 7) {
        if engine.aiBusy {
          ProgressView().controlSize(.small).tint(.white)
        } else {
          Image(systemName: on ? "sparkles" : "shuffle")
            .font(.subheadline.weight(.semibold))
            .symbolEffect(.bounce, value: on)
        }
        Text("AI Shuffle")
          .font(.subheadline.weight(.semibold))
          .lineLimit(1)
          .fixedSize()
      }
      .padding(.horizontal, 14)
      .frame(height: 36)
      .foregroundStyle(on ? Color.white : Color.white.opacity(0.7))
      .background {
        if on {
          Capsule()
            .fill(LinearGradient(colors: [Color(red: 0.55, green: 0.48, blue: 0.95), Color(red: 0.85, green: 0.62, blue: 0.95), Color(red: 0.62, green: 0.78, blue: 1.0)], startPoint: .leading, endPoint: .trailing))
            .opacity(0.55)
            .shadow(color: Color(red: 0.7, green: 0.6, blue: 1).opacity(glow ? 0.6 : 0.25), radius: glow ? 14 : 6)
        }
      }
      .glassEffect(.regular, in: Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("AI Shuffle")
    .accessibilityValue(on ? "On" : "Off")
    .accessibilityHint(engine.aiNote ?? "Plans the next songs from the mood of this one and how you listen")
    .onAppear {
      withAnimation(.easeInOut(duration: 2.2).repeatForever(autoreverses: true)) { glow = true }
    }
  }
}

/// Thin progress bar that thickens while you drag it.
struct Scrubber: View {
  let engine: PlayerEngine
  @State private var dragging: Double?

  private var duration: Double { max(engine.durationMs, 1) }
  private var position: Double { min(dragging ?? engine.positionMs, duration) }

  var body: some View {
    VStack(spacing: 6) {
      GeometryReader { g in
        let frac = CGFloat(position / duration)
        ZStack(alignment: .leading) {
          Capsule().fill(.white.opacity(0.18))
          Capsule().fill(.white.opacity(dragging == nil ? 0.9 : 1)).frame(width: max(0, g.size.width * frac))
        }
        .frame(height: dragging == nil ? 5 : 10)
        .frame(maxHeight: .infinity)
        .animation(.spring(response: 0.25, dampingFraction: 0.8), value: dragging == nil)
        .contentShape(Rectangle())
        .gesture(
          DragGesture(minimumDistance: 0)
            .onChanged { v in dragging = Double(min(max(0, v.location.x / max(1, g.size.width)), 1)) * duration }
            .onEnded { _ in
              if let d = dragging { engine.seek(ms: d) }
              dragging = nil
            }
        )
      }
      .frame(height: 22)
      HStack {
        Text(Self.time(position))
        Spacer()
        Text("-" + Self.time(max(0, duration - position)))
      }
      .font(.caption.monospacedDigit().weight(.medium))
      .foregroundStyle(.white.opacity(0.55))
    }
    .accessibilityElement()
    .accessibilityLabel("Song position")
    .accessibilityValue("\(Self.time(position)) of \(Self.time(duration))")
    .accessibilityAdjustableAction { dir in
      engine.seek(ms: max(0, min(duration, engine.positionMs + (dir == .increment ? 10_000 : -10_000))))
    }
  }

  static func time(_ ms: Double) -> String {
    let s = Int((ms / 1000).rounded(.down))
    return String(format: "%d:%02d", s / 60, s % 60)
  }
}

/// Blurred colours from the artwork behind everything. Lives in `.background`
/// and is clipped to its own frame, so it never affects layout.
struct Backdrop: View {
  let image: UIImage?

  var body: some View {
    GeometryReader { g in
      ZStack {
        Color(red: 0.03, green: 0.03, blue: 0.04)
        if let image {
          Image(uiImage: image)
            .resizable()
            .scaledToFill()
            .frame(width: g.size.width, height: g.size.height)
            .blur(radius: 90)
            .saturation(1.5)
            .opacity(0.75)
            .clipped()
        }
        LinearGradient(colors: [.black.opacity(0.25), .black.opacity(0.45), .black.opacity(0.7)], startPoint: .top, endPoint: .bottom)
      }
      .frame(width: g.size.width, height: g.size.height)
      .clipped()
    }
    .ignoresSafeArea()
    .animation(.easeInOut(duration: 0.6), value: image)
  }
}

struct AirPlayButton: UIViewRepresentable {
  func makeUIView(context: Context) -> AVRoutePickerView {
    let v = AVRoutePickerView()
    v.tintColor = UIColor.white.withAlphaComponent(0.75)
    v.activeTintColor = .white
    v.prioritizesVideoDevices = false
    return v
  }
  func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}
