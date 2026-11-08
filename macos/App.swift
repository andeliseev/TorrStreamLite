import Cocoa
import Darwin
import UniformTypeIdentifiers
import WebKit

final class AppDelegate: NSObject, NSApplicationDelegate, WKNavigationDelegate, WKUIDelegate,
  WKScriptMessageHandler
{
  var window: NSWindow!
  var web: WKWebView!
  var server: Process?
  var quitting = false
  var startupText = ""
  var serverURL: URL?

  func applicationDidFinishLaunching(_ notification: Notification) {
    let menu = NSMenu()
    let root = NSMenuItem()
    menu.addItem(root)
    let appMenu = NSMenu()
    appMenu.addItem(
      withTitle: "About TorrStream Lite", action: #selector(openSettings), keyEquivalent: "")
    appMenu.addItem(withTitle: "Settings…", action: #selector(openSettings), keyEquivalent: ",")
    appMenu.addItem(NSMenuItem.separator())
    appMenu.addItem(
      withTitle: "Quit TorrStream Lite", action: #selector(NSApplication.terminate(_:)),
      keyEquivalent: "q")
    root.submenu = appMenu
    let editRoot = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
    let edit = NSMenu(title: "Edit")
    edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
    edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
    edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
    edit.addItem(
      withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
    editRoot.submenu = edit
    menu.addItem(editRoot)
    NSApp.mainMenu = menu
    window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 860, height: 560),
      styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false
    )
    window.title = "TorrStream Lite"
    window.minSize = NSSize(width: 740, height: 560)
    let configuration = WKWebViewConfiguration()
    let tvEnabled = UserDefaults.standard.bool(forKey: "tvAccessEnabled")
    let storedPort = UserDefaults.standard.integer(forKey: "tvAccessPort")
    let tvPort = (1024...65535).contains(storedPort) ? storedPort : 8091
    configuration.userContentController.addUserScript(
      WKUserScript(
        source:
          "window.torrstreamDesktop = true; window.torrstreamTV = {enabled: \(tvEnabled), port: \(tvPort)};",
        injectionTime: .atDocumentStart, forMainFrameOnly: true))
    configuration.userContentController.add(self, name: "desktop")
    web = WKWebView(frame: window.contentView!.bounds, configuration: configuration)
    web.autoresizingMask = [.width, .height]
    web.navigationDelegate = self
    web.uiDelegate = self
    window.contentView!.addSubview(web)
    web.loadHTMLString(
      "<body style='background:#0e131b;color:#8ee1c1;font:20px -apple-system;padding:60px'>Starting TorrStream Lite…</body>",
      baseURL: nil)
    window.center()
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    startServer()
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
  @objc func openSettings() {
    web.evaluateJavaScript("window.torrstreamOpenSettings?.()", completionHandler: nil)
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
