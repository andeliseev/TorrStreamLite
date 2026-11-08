// Renders demo.html in WebKit, like the Mac app, and saves a 2x PNG.
//   swiftc snap.swift -o snap -framework AppKit -framework WebKit
//   ./snap <demo.html> <light|dark> <out.png> [width height]
// Used by make.py.
import AppKit
import WebKit

let args = CommandLine.arguments
let page = URL(fileURLWithPath: args[1])
let theme = args[2]
let output = URL(fileURLWithPath: args[3])
let size = NSSize(
  width: args.count > 4 ? Double(args[4])! : 1040, height: args.count > 5 ? Double(args[5])! : 660)

final class Snapper: NSObject, WKNavigationDelegate {
  let web = WKWebView(frame: NSRect(origin: .zero, size: size))
  var window: NSWindow!

  func start() {
    window = NSWindow(
      contentRect: NSRect(x: -6000, y: -6000, width: size.width, height: size.height),
      styleMask: [.borderless], backing: .buffered, defer: false)
    web.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
    web.navigationDelegate = self
    window.contentView = web
    window.orderFrontRegardless()
    var parts = URLComponents(url: page.absoluteURL, resolvingAgainstBaseURL: true)!
    parts.fragment = theme
    web.loadFileURL(parts.url!, allowingReadAccessTo: page.deletingLastPathComponent())
    DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
      FileHandle.standardError.write(Data("Timed out\n".utf8))
      exit(1)
    }
  }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    // Let the page poll and render, then open episode 3 so it shows as playing.
    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
      webView.evaluateJavaScript("document.querySelectorAll('.tool.play')[2]?.click(); 1") { _, _ in
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { self.snap() }
      }
    }
  }

  func snap() {
    let config = WKSnapshotConfiguration()
    config.rect = web.bounds
    // In points: a Retina screen makes the image twice as many pixels wide.
    config.snapshotWidth = NSNumber(value: Double(size.width))
    web.takeSnapshot(with: config) { image, error in
      guard let image, let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff),
        let png = bitmap.representation(using: .png, properties: [:])
      else {
        FileHandle.standardError.write(Data("Snapshot failed: \(String(describing: error))\n".utf8))
        exit(1)
      }
      try! png.write(to: output)
      print("Wrote \(output.path) (\(bitmap.pixelsWide)×\(bitmap.pixelsHigh))")
      exit(0)
    }
  }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let snapper = Snapper()
snapper.start()
app.run()
