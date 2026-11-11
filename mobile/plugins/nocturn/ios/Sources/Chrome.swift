import AVKit
import MediaPlayer
import SwiftUI
import UIKit
import WebKit

/// One tab as the web side describes it.
struct TabSpec: Decodable {
  let id: String
  let title: String
  let symbol: String
  let search: Bool?
}

/// The system Liquid Glass chrome around the web content: the iOS 26 tab bar
/// (shrinks while scrolling), the mini player docked above it, and the
/// full-screen Now Playing view.
///
/// The web view (inside Tauri's own view controller) is moved into whichever
/// tab is showing, so there is one web app behind all tabs and the web side
/// decides what each tab shows.
@MainActor
final class ChromeController: NSObject, UITabBarControllerDelegate {
  static let shared = ChromeController()

  var onTab: ((String) -> Void)?
  var onAction: ((String, String?) -> Void)?

  private let tabBar = UITabBarController()
  private weak var webview: WKWebView?
  private var webVC: UIViewController?
  private var hosts: [String: HostViewController] = [:]
  private var specs: [TabSpec] = []
  private var pendingSelect: String?
  private var installed = false
  private var accessoryHost: UIHostingController<MiniPlayerView>?
  private var accessoryShown = false
  private weak var nowPlaying: UIViewController?

  // MARK: install

  func install(webview: WKWebView, attempt: Int = 0) {
    guard !installed else { return }
    guard let window = webview.window, let root = window.rootViewController else {
      // The web view isn't on screen yet; try again shortly (up to ~10 s).
      if attempt < 100 {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
          MainActor.assumeIsolated { self.install(webview: webview, attempt: attempt + 1) }
        }
      }
      return
    }
    installed = true
    self.webview = webview
    self.webVC = root
    window.overrideUserInterfaceStyle = .dark
    window.backgroundColor = UIColor(red: 0.03, green: 0.03, blue: 0.04, alpha: 1)

    webview.isOpaque = false
    webview.backgroundColor = .clear
    webview.scrollView.backgroundColor = .clear
    webview.scrollView.contentInsetAdjustmentBehavior = .never

    tabBar.delegate = self
    tabBar.mode = .tabBar
    tabBar.tabBarMinimizeBehavior = .onScrollDown
    tabBar.view.backgroundColor = window.backgroundColor
    if specs.isEmpty {
      specs = [
        TabSpec(id: "home", title: "Home", symbol: "house.fill", search: nil),
        TabSpec(id: "library", title: "Library", symbol: "square.stack.fill", search: nil),
        TabSpec(id: "search", title: "Search", symbol: "magnifyingglass", search: true),
      ]
    }
    rebuildTabs()
    window.rootViewController = tabBar
    window.makeKeyAndVisible()
    if let id = pendingSelect { select(id) }
    playerChanged()
  }

  // MARK: tabs

  func setTabs(_ tabs: [TabSpec], selected: String?) {
    specs = tabs
    pendingSelect = selected
    guard installed else { return }
    rebuildTabs()
    if let selected { select(selected) }
  }

  func select(_ id: String) {
    pendingSelect = id
    guard installed, let tab = tabBar.tabs.first(where: { $0.identifier == id }) else { return }
    tabBar.selectedTab = tab
    if let host = hosts[id] { embed(into: host) }
  }

  private func rebuildTabs() {
    tabBar.tabs = specs.map { spec in
      let provider: (UITab) -> UIViewController = { [weak self] _ in
        MainActor.assumeIsolated { self?.host(for: spec.id) ?? UIViewController() }
      }
      if spec.search == true {
        let t = UISearchTab(viewControllerProvider: provider)
        t.title = spec.title
        return t
      }
      return UITab(title: spec.title, image: UIImage(systemName: spec.symbol), identifier: spec.id, viewControllerProvider: provider)
    }
  }

  private func host(for id: String) -> HostViewController {
    if let h = hosts[id] { return h }
    let h = HostViewController()
    h.tabId = id
    h.onAppear = { [weak self] host in self?.embed(into: host) }
    hosts[id] = h
    return h
  }

  /// Moves the one web view into the tab that's showing.
  private func embed(into host: HostViewController) {
    guard let content = webVC else { return }
    if content.parent !== host {
      if content.parent != nil {
        content.willMove(toParent: nil)
        content.view.removeFromSuperview()
        content.removeFromParent()
      }
      host.addChild(content)
      content.view.frame = host.view.bounds
      content.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
      host.view.addSubview(content.view)
      content.didMove(toParent: host)
    }
    if let sv = webview?.scrollView {
      host.setContentScrollView(sv, for: .bottom)
    }
  }

  func tabBarController(_ tabBarController: UITabBarController, didSelectTab selectedTab: UITab, previousTab: UITab?) {
    let id = selectedTab is UISearchTab ? (specs.first(where: { $0.search == true })?.id ?? "search") : selectedTab.identifier
    pendingSelect = id
    if let h = hosts[id] { embed(into: h) }
    onTab?(id)
  }

  // MARK: mini player + Now Playing

  /// Shows the mini player once something is loaded.
  func playerChanged() {
    guard installed else { return }
    let hasTrack = PlayerEngine.shared.current != nil
    if hasTrack && !accessoryShown {
      let host = accessoryHost ?? UIHostingController(rootView: MiniPlayerView(engine: PlayerEngine.shared, open: { [weak self] in self?.showNowPlaying() }))
      host.view.backgroundColor = .clear
      if accessoryHost == nil {
        tabBar.addChild(host)
        host.didMove(toParent: tabBar)
        accessoryHost = host
      }
      tabBar.bottomAccessory = UITabAccessory(contentView: host.view)
      accessoryShown = true
    } else if !hasTrack && accessoryShown {
      tabBar.bottomAccessory = nil
      accessoryShown = false
    }
  }

  func showNowPlaying() {
    guard installed, nowPlaying == nil, PlayerEngine.shared.current != nil else { return }
    let view = NowPlayingView(engine: PlayerEngine.shared) { [weak self] kind, arg in
      self?.nowPlaying?.dismiss(animated: true)
      self?.onAction?(kind, arg)
    }
    let vc = UIHostingController(rootView: view)
    vc.view.backgroundColor = UIColor(red: 0.03, green: 0.03, blue: 0.04, alpha: 1)
    vc.modalPresentationStyle = .fullScreen
    if let src = accessoryHost?.view {
      vc.preferredTransition = .zoom { _ in src }
    }
    nowPlaying = vc
    tabBar.present(vc, animated: true)
  }

  func haptic(_ style: String) {
    switch style {
    case "light": UIImpactFeedbackGenerator(style: .light).impactOccurred()
    case "heavy": UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
    case "select": UISelectionFeedbackGenerator().selectionChanged()
    case "success": UINotificationFeedbackGenerator().notificationOccurred(.success)
    default: UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }
  }
}

/// An empty tab page; the web view gets moved in when it appears.
final class HostViewController: UIViewController {
  var tabId = ""
  var onAppear: ((HostViewController) -> Void)?

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = UIColor(red: 0.03, green: 0.03, blue: 0.04, alpha: 1)
  }

  override func viewWillAppear(_ animated: Bool) {
    super.viewWillAppear(animated)
    onAppear?(self)
  }

  override var preferredStatusBarStyle: UIStatusBarStyle { .lightContent }
}

// MARK: - SwiftUI views

struct ArtworkView: View {
  let image: UIImage?
  var corner: CGFloat = 8

  var body: some View {
    ZStack {
      RoundedRectangle(cornerRadius: corner, style: .continuous).fill(Color.white.opacity(0.08))
      if let image {
        Image(uiImage: image).resizable().aspectRatio(contentMode: .fill)
      } else {
        Image(systemName: "music.note").font(.system(size: 18, weight: .medium)).foregroundStyle(.white.opacity(0.35))
      }
    }
    .clipShape(RoundedRectangle(cornerRadius: corner, style: .continuous))
  }
}

/// Docked above the tab bar (the system draws the glass around it).
struct MiniPlayerView: View {
  let engine: PlayerEngine
  let open: () -> Void

  var body: some View {
    HStack(spacing: 12) {
      ArtworkView(image: engine.artworkImage, corner: 7)
        .frame(width: 34, height: 34)
      VStack(alignment: .leading, spacing: 1) {
        Text(engine.current?.title ?? "Not playing")
          .font(.subheadline.weight(.semibold))
          .lineLimit(1)
        Text(engine.current?.artist ?? "")
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }
      Spacer(minLength: 6)
      Button {
        engine.toggle()
      } label: {
        Group {
          if engine.loading && engine.playing {
            ProgressView().controlSize(.small)
          } else {
            Image(systemName: engine.playing ? "pause.fill" : "play.fill")
              .contentTransition(.symbolEffect(.replace))
          }
        }
        .font(.title3)
        .frame(width: 36, height: 36)
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel(engine.playing ? "Pause" : "Play")
      Button {
        engine.next()
      } label: {
        Image(systemName: "forward.fill")
          .font(.title3)
          .frame(width: 36, height: 36)
          .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel("Next")
    }
    .padding(.leading, 10)
    .padding(.trailing, 8)
    .frame(maxHeight: .infinity)
    .contentShape(Rectangle())
    .onTapGesture(perform: open)
  }
}

/// The full-screen player, with Liquid Glass controls over the blurred cover.
struct NowPlayingView: View {
  let engine: PlayerEngine
  let action: (String, String?) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var scrub: Double?
  @State private var showLyrics = false

  init(engine: PlayerEngine, action: @escaping (String, String?) -> Void) {
    self.engine = engine
    self.action = action
  }

  private var duration: Double { max(engine.durationMs, 1) }
  private var position: Double { min(scrub ?? engine.positionMs, duration) }

  var body: some View {
    ZStack {
      backdrop
      VStack(spacing: 0) {
        Capsule()
          .fill(.white.opacity(0.35))
          .frame(width: 38, height: 5)
          .padding(.top, 8)
          .onTapGesture { dismiss() }

        Group {
          if showLyrics {
            LyricsView(engine: engine)
              .transition(.opacity)
          } else {
            ArtworkView(image: engine.artworkImage, corner: 14)
              .aspectRatio(1, contentMode: .fit)
              .scaleEffect(engine.playing ? 1 : 0.86)
              .shadow(color: .black.opacity(0.45), radius: 30, y: 18)
              .animation(.spring(response: 0.45, dampingFraction: 0.75), value: engine.playing)
              .padding(.horizontal, 28)
              .transition(.opacity)
          }
        }
        .frame(maxHeight: .infinity)

        titleRow.padding(.horizontal, 28)
        scrubber.padding(.horizontal, 28).padding(.top, 18)
        transport.padding(.top, 14)
        VolumeSlider().frame(height: 34).padding(.horizontal, 32).padding(.top, 14)
        bottomRow.padding(.horizontal, 44).padding(.top, 12).padding(.bottom, 10)
      }
    }
    .preferredColorScheme(.dark)
    .gesture(
      DragGesture(minimumDistance: 30).onEnded { g in
        if g.translation.height > 120 && abs(g.translation.width) < 100 { dismiss() }
      }
    )
  }

  private var backdrop: some View {
    ZStack {
      Color(red: 0.03, green: 0.03, blue: 0.04)
      if let img = engine.artworkImage {
        Image(uiImage: img)
          .resizable()
          .aspectRatio(contentMode: .fill)
          .blur(radius: 70)
          .saturation(1.4)
          .opacity(0.75)
          .scaleEffect(1.4)
      }
      LinearGradient(colors: [.black.opacity(0.15), .black.opacity(0.65)], startPoint: .top, endPoint: .bottom)
    }
    .ignoresSafeArea()
  }

  private var titleRow: some View {
    HStack(alignment: .center, spacing: 12) {
      VStack(alignment: .leading, spacing: 3) {
        Text(engine.current?.title ?? "")
          .font(.title3.weight(.bold))
          .lineLimit(1)
        Text(engine.current?.artist ?? "")
          .font(.title3)
          .foregroundStyle(.white.opacity(0.65))
          .lineLimit(1)
      }
      Spacer()
      Menu {
        Button("Go to Artist", systemImage: "music.microphone") { action("artist", engine.current?.artist) }
        if let album = engine.current?.album, !album.isEmpty {
          Button("Go to Album", systemImage: "square.stack") { action("album", album) }
        }
        Button("Start Radio", systemImage: "dot.radiowaves.left.and.right") { action("radio", engine.current?.videoId) }
        Button("Add to Library", systemImage: "plus") { action("save", engine.current?.videoId) }
      } label: {
        Image(systemName: "ellipsis")
          .font(.body.weight(.semibold))
          .frame(width: 34, height: 34)
      }
      .buttonStyle(.glass)
      .buttonBorderShape(.circle)
    }
  }

  private var scrubber: some View {
    VStack(spacing: 4) {
      Slider(
        value: Binding(get: { position }, set: { scrub = $0 }),
        in: 0...duration,
        onEditingChanged: { editing in
          if !editing, let s = scrub {
            engine.seek(ms: s)
            scrub = nil
          }
        }
      )
      .tint(.white)
      HStack {
        Text(Self.time(position))
        Spacer()
        Text("-" + Self.time(max(0, duration - position)))
      }
      .font(.caption.monospacedDigit())
      .foregroundStyle(.white.opacity(0.6))
    }
  }

  private var transport: some View {
    GlassEffectContainer(spacing: 24) {
      HStack(spacing: 22) {
        Button {
          engine.previous()
        } label: {
          Image(systemName: "backward.fill").font(.title2).frame(width: 62, height: 62)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)

        Button {
          engine.toggle()
        } label: {
          Image(systemName: engine.playing ? "pause.fill" : "play.fill")
            .font(.system(size: 34, weight: .semibold))
            .contentTransition(.symbolEffect(.replace))
            .frame(width: 84, height: 84)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)

        Button {
          engine.next()
        } label: {
          Image(systemName: "forward.fill").font(.title2).frame(width: 62, height: 62)
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.circle)
      }
    }
    .foregroundStyle(.white)
  }

  private var bottomRow: some View {
    HStack {
      Button {
        withAnimation(.easeInOut(duration: 0.25)) { showLyrics.toggle() }
      } label: {
        Image(systemName: showLyrics ? "quote.bubble.fill" : "quote.bubble").font(.title3)
      }
      .accessibilityLabel("Lyrics")
      Spacer()
      Button {
        let next = ["off": "all", "all": "one", "one": "off"][engine.repeatMode] ?? "off"
        engine.setRepeat(next)
      } label: {
        Image(systemName: engine.repeatMode == "one" ? "repeat.1" : "repeat")
          .font(.title3)
          .foregroundStyle(engine.repeatMode == "off" ? .white.opacity(0.55) : .white)
      }
      .accessibilityLabel("Repeat")
      Spacer()
      AirPlayButton().frame(width: 30, height: 30)
      Spacer()
      Button {
        action("queue", nil)
      } label: {
        Image(systemName: "list.bullet").font(.title3)
      }
      .accessibilityLabel("Up Next")
    }
    .buttonStyle(.plain)
    .foregroundStyle(.white.opacity(0.85))
  }

  static func time(_ ms: Double) -> String {
    let s = Int((ms / 1000).rounded(.down))
    return String(format: "%d:%02d", s / 60, s % 60)
  }
}

struct VolumeSlider: UIViewRepresentable {
  func makeUIView(context: Context) -> MPVolumeView {
    let v = MPVolumeView(frame: .zero)
    v.tintColor = .white
    return v
  }
  func updateUIView(_ uiView: MPVolumeView, context: Context) {}
}

struct AirPlayButton: UIViewRepresentable {
  func makeUIView(context: Context) -> AVRoutePickerView {
    let v = AVRoutePickerView()
    v.tintColor = UIColor.white.withAlphaComponent(0.85)
    v.activeTintColor = .white
    v.prioritizesVideoDevices = false
    return v
  }
  func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}
