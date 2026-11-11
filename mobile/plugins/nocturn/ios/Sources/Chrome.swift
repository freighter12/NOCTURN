import AVKit
import MediaPlayer
import SwiftUI
import UIKit
import WebKit

/// One tab as the web side describes it.
struct TabSpec: Decodable, Equatable {
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
  /// The tab bar owns the window, so the web view can be moved into tabs.
  private var ready = false
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
    // Hand the window to the tab bar FIRST: iOS refuses to make Tauri's view
    // controller a child of a tab while it's still the window's root (that
    // crashed build 0.1.0.3/4 on launch).
    window.rootViewController = tabBar
    window.makeKeyAndVisible()
    ready = true
    rebuildTabs()
    if let id = pendingSelect { select(id) }
    if let host = tabBar.selectedViewController as? HostViewController { embed(into: host) }
    playerChanged()
  }

  // MARK: tabs

  func setTabs(_ tabs: [TabSpec], selected: String?) {
    let changed = tabs != specs
    specs = tabs
    pendingSelect = selected
    guard installed, ready else { return }
    // Same tabs as now (the usual case at startup): nothing to rebuild.
    if changed { rebuildTabs() }
    if let selected { select(selected) }
  }

  func select(_ id: String) {
    pendingSelect = id
    guard installed, ready else { return }
    let isSearch = specs.first(where: { $0.id == id })?.search == true
    guard let tab = tabBar.tabs.first(where: { isSearch ? $0 is UISearchTab : $0.identifier == id }) else { return }
    if tabBar.selectedTab !== tab { tabBar.selectedTab = tab }
    if let host = hosts[id] { embed(into: host) }
  }

  private func rebuildTabs() {
    // A tab's screen can belong to only one tab (UIKit asserts otherwise), so
    // new tabs get new screens; the web view moves into whichever is shown.
    hosts.removeAll()
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
    guard ready, let content = webVC else { return }
    // never adopt a view controller that is still some window's root
    if let w = content.view.window, w.rootViewController === content { return }
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
    let view = NowPlayingView(
      engine: PlayerEngine.shared,
      navigate: { [weak self] kind, arg in
        self?.nowPlaying?.dismiss(animated: true)
        self?.onAction?(kind, arg)
      },
      signal: { [weak self] kind, arg in self?.onAction?(kind, arg) })
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
