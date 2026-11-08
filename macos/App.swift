import Cocoa
import Darwin
import UniformTypeIdentifiers
import WebKit

/// Covers the transparent titlebar band above the page, so dragging there moves the
/// window and a double-click does what the user chose in System Settings.
final class TitlebarDragView: NSView {
  override var mouseDownCanMoveWindow: Bool { true }

  override func mouseDown(with event: NSEvent) {
    guard let window else { return }
    guard event.clickCount == 2 else {
      window.performDrag(with: event)
      return
    }
    // Desktop & Dock → "Double-click a window's title bar to".
    switch UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") {
    case "Minimize": window.performMiniaturize(nil)
    case "None": break
    default: window.performZoom(nil)
    }
  }
}

final class AppDelegate: NSObject, NSApplicationDelegate, WKNavigationDelegate, WKUIDelegate,
  WKScriptMessageHandler
{
  var window: NSWindow!
  var web: WKWebView!
  var server: Process?
  var quitting = false
  var startupText = ""
  var serverURL: URL?

  /// The page's main background, so nothing flashes while it loads or the window resizes.
  static let pageBackground = NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
      ? NSColor(srgbRed: 26 / 255, green: 28 / 255, blue: 32 / 255, alpha: 1) : .white
  }

  func applicationDidFinishLaunching(_ notification: Notification) {
    let defaults = UserDefaults.standard
    let theme =
      ["light", "dark"].contains(defaults.string(forKey: "appearance"))
      ? defaults.string(forKey: "appearance")! : "system"
    applyAppearance(theme)
    NSApp.mainMenu = makeMenu()

    // The page runs under a transparent titlebar: one surface, with the window
    // buttons over the sidebar. The page leaves that band free of controls.
    window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 900, height: 600),
      styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
      backing: .buffered, defer: false)
    window.title = "TorrStream Lite"
    window.titleVisibility = .hidden
    window.titlebarAppearsTransparent = true
    window.minSize = NSSize(width: 720, height: 520)
    window.backgroundColor = Self.pageBackground
    let content = window.contentView!
    let measured = content.bounds.height - window.contentLayoutRect.height
    let titlebar = measured > 0 ? measured : 28

    let tvEnabled = defaults.bool(forKey: "tvAccessEnabled")
    let storedPort = defaults.integer(forKey: "tvAccessPort")
    let tvPort = (1024...65535).contains(storedPort) ? storedPort : 8090
    let configuration = WKWebViewConfiguration()
    configuration.userContentController.addUserScript(
      WKUserScript(
        source: """
          window.torrstreamDesktop = true;
          window.torrstreamTV = {enabled: \(tvEnabled), port: \(tvPort)};
          window.torrstreamTheme = "\(theme)";
          document.documentElement.style.setProperty("--titlebar", "\(Int(titlebar))px");
          """,
        injectionTime: .atDocumentStart, forMainFrameOnly: true))
    configuration.userContentController.add(self, name: "desktop")
    web = WKWebView(frame: content.bounds, configuration: configuration)
    web.autoresizingMask = [.width, .height]
    web.navigationDelegate = self
    web.uiDelegate = self
    web.underPageBackgroundColor = Self.pageBackground
    content.addSubview(web)
    let drag = TitlebarDragView(
      frame: NSRect(
        x: 0, y: content.bounds.height - titlebar, width: content.bounds.width, height: titlebar))
    drag.autoresizingMask = [.width, .minYMargin]
    content.addSubview(drag)
    web.loadHTMLString(
      """
      <meta name="color-scheme" content="light dark"><style>
      body{margin:0;height:100vh;display:grid;place-items:center;font:13px -apple-system,sans-serif;background:#fff;color:#6b7280}
      @media (prefers-color-scheme:dark){body{background:#1a1c20;color:#9298a2}}
      </style><body>Starting TorrStream Lite…</body>
      """,
      baseURL: nil)

    window.center()
    window.setFrameAutosaveName("TorrStreamLite")
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    startServer()
  }

  func makeMenu() -> NSMenu {
    let menu = NSMenu()
    func add(_ title: String, _ items: [NSMenuItem]) -> NSMenu {
      let submenu = NSMenu(title: title)
      items.forEach(submenu.addItem)
      let root = NSMenuItem(title: title, action: nil, keyEquivalent: "")
      root.submenu = submenu
      menu.addItem(root)
      return submenu
    }
    func item(
      _ title: String, _ action: Selector, _ key: String = "",
      _ modifiers: NSEvent.ModifierFlags = .command
    ) -> NSMenuItem {
      let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
      item.keyEquivalentModifierMask = modifiers
      return item
    }
    _ = add(
      "TorrStream Lite",
      [
        item("About TorrStream Lite", #selector(showAbout)), .separator(),
        item("Settings…", #selector(openSettings), ","), .separator(),
        item("Hide TorrStream Lite", #selector(NSApplication.hide(_:)), "h"),
        item(
          "Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h",
          [.command, .option]),
        item("Show All", #selector(NSApplication.unhideAllApplications(_:))), .separator(),
        item("Quit TorrStream Lite", #selector(NSApplication.terminate(_:)), "q"),
      ])
    _ = add(
      "File",
      [
        item("Open Torrent File…", #selector(openTorrentFile), "o"), .separator(),
        item("Close Window", #selector(NSWindow.performClose(_:)), "w"),
      ])
    _ = add(
      "Edit",
      [
        item("Undo", Selector(("undo:")), "z"),
        item("Redo", Selector(("redo:")), "z", [.command, .shift]), .separator(),
        item("Cut", #selector(NSText.cut(_:)), "x"),
        item("Copy", #selector(NSText.copy(_:)), "c"),
        item("Paste", #selector(NSText.paste(_:)), "v"),
        item("Select All", #selector(NSText.selectAll(_:)), "a"),
      ])
    NSApp.windowsMenu = add(
      "Window",
      [
        item("Minimize", #selector(NSWindow.performMiniaturize(_:)), "m"),
        item("Zoom", #selector(NSWindow.performZoom(_:))), .separator(),
        item("Bring All to Front", #selector(NSApplication.arrangeInFront(_:))),
      ])
    return menu
  }

  func applyAppearance(_ theme: String) {
    NSApp.appearance =
      theme == "light"
      ? NSAppearance(named: .aqua) : theme == "dark" ? NSAppearance(named: .darkAqua) : nil
  }

  func startServer() {
    guard let resources = Bundle.main.resourceURL else { return }
    let process = Process()
    process.executableURL = resources.appendingPathComponent("torrstream-lite")
    process.arguments = ["--listen", "127.0.0.1:0", "--cache-mib", "96"]
    let pipe = Pipe()
    process.standardOutput = pipe
    process.standardError = pipe
    pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
      let data = handle.availableData
      guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
      DispatchQueue.main.async {
        guard let self = self else { return }
        self.startupText = String((self.startupText + text).suffix(8192))
        if self.serverURL == nil,
          let line = self.startupText.components(separatedBy: "\n").dropLast().first(where: {
            $0.hasPrefix("TORRSTREAM_LISTEN=")
          }),
          let url = URL(string: String(line.dropFirst("TORRSTREAM_LISTEN=".count)))
        {
          self.serverURL = url
          self.web.load(URLRequest(url: url))
        }
      }
    }
    process.terminationHandler = { [weak self] p in
      DispatchQueue.main.async {
        guard let self = self, !self.quitting else { return }
        self.showError(
          "The streaming server stopped (exit \(p.terminationStatus)).\n\n\(self.startupText)")
      }
    }
    do {
      try process.run()
      server = process
    } catch { showError("Cannot start the streaming server: \(error.localizedDescription)") }
  }

  func showError(_ message: String) {
    let alert = NSAlert()
    alert.messageText = "TorrStream Lite"
    alert.informativeText = message
    alert.runModal()
  }

  @objc func showAbout() {
    let credits = NSAttributedString(
      string: "Inspired by TorrServer. Uses librqbit under the Apache-2.0 license.",
      attributes: [
        .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
        .foregroundColor: NSColor.secondaryLabelColor,
      ])
    NSApp.orderFrontStandardAboutPanel(options: [.credits: credits])
  }

  @objc func openSettings() {
    web.evaluateJavaScript("window.torrstreamOpenSettings?.()", completionHandler: nil)
  }

  /// File → Open Torrent File… (⌘O). The page adds it, so progress and errors show there.
  @objc func openTorrentFile() {
    guard serverURL != nil else { return }
    let panel = NSOpenPanel()
    panel.allowsMultipleSelection = false
    panel.canChooseDirectories = false
    if let torrent = UTType(filenameExtension: "torrent") { panel.allowedContentTypes = [torrent] }
    panel.beginSheetModal(for: window) { response in
      guard response == .OK, let file = panel.url else { return }
      guard let data = try? Data(contentsOf: file), data.count <= 2 * 1024 * 1024 else {
        self.showError("Choose a .torrent file of at most 2 MiB.")
        return
      }
      self.web.evaluateJavaScript(
        "window.torrstreamAddTorrent?.(\"\(data.base64EncodedString())\")", completionHandler: nil)
    }
  }

  func openProjectLink(_ raw: String) {
    guard let url = URL(string: raw), url.scheme == "https", url.host == "github.com",
      url.user == nil, url.password == nil, url.port == nil,
      [
        "/andeliseev/TorrStreamLite", "/andeliseev/TorrStreamLite/releases",
        "/YouROK/TorrServer", "/ikatson/rqbit",
      ].contains(url.path)
    else { return }
    NSWorkspace.shared.open(url)
  }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
  func userContentController(
    _ userContentController: WKUserContentController, didReceive message: WKScriptMessage
  ) {
    // Only our local main frame may invoke native actions.
    guard message.frameInfo.isMainFrame,
      message.frameInfo.securityOrigin.host == "127.0.0.1",
      message.frameInfo.securityOrigin.port == serverURL?.port,
      let body = message.body as? [String: Any], let action = body["action"] as? String
    else { return }
    switch action {
    case "openExternal":
      if let url = body["url"] as? String { openProjectLink(url) }
    case "saveTVSettings":
      if let enabled = body["enabled"] as? Bool, let port = body["port"] as? Int,
        (1024...65535).contains(port)
      {
        UserDefaults.standard.set(enabled, forKey: "tvAccessEnabled")
        UserDefaults.standard.set(port, forKey: "tvAccessPort")
      }
    case "setTheme":
      if let theme = body["theme"] as? String, ["system", "light", "dark"].contains(theme) {
        UserDefaults.standard.set(theme, forKey: "appearance")
        applyAppearance(theme)
      }
    case "copy":
      if let text = body["text"] as? String, text.count <= 8192 {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
      }
    default: break
    }
  }
  func applicationWillTerminate(_ notification: Notification) {
    quitting = true
    if let server = server, server.isRunning {
      server.terminate()
      // A pending metadata lookup or a stalled client must not leave an
      // orphan server after the control window quits.
      for _ in 0..<20 {
        if !server.isRunning { break }
        Thread.sleep(forTimeInterval: 0.05)
      }
      if server.isRunning { kill(server.processIdentifier, SIGKILL) }
    }
  }
  func webView(
    _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
    decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
  ) {
    guard let url = navigationAction.request.url else {
      decisionHandler(.cancel)
      return
    }
    if url.scheme == "torrstream-player" || url.scheme == "vlc" {
      let raw =
        url.scheme == "torrstream-player"
        ? (URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: {
          $0.name == "url"
        })?.value ?? "")
        : String(url.absoluteString.dropFirst("vlc://".count))
      let player =
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: "org.videolan.vlc")
        ?? NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.colliderli.iina")
      if let streamURL = URL(string: raw), let player = player {
        NSWorkspace.shared.open(
          [streamURL], withApplicationAt: player, configuration: NSWorkspace.OpenConfiguration()
        ) { _, error in
          if let error = error {
            DispatchQueue.main.async { self.showError(error.localizedDescription) }
          }
        }
      } else {
        showError("Install VLC or IINA, or use Copy stream URL with your preferred player.")
      }
      decisionHandler(.cancel)
      return
    }
    if url.scheme == "about" || (url.host == "127.0.0.1" && url.port == serverURL?.port) {
      decisionHandler(.allow)
    } else {
      decisionHandler(.cancel)
    }
  }
  func webView(
    _ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters,
    initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping ([URL]?) -> Void
  ) {
    let panel = NSOpenPanel()
    panel.allowsMultipleSelection = false
    panel.canChooseDirectories = false
    if let torrent = UTType(filenameExtension: "torrent") { panel.allowedContentTypes = [torrent] }
    panel.beginSheetModal(for: window) { response in
      completionHandler(response == .OK ? panel.urls : nil)
    }
  }
}
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
