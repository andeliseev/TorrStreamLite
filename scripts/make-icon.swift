// Renders app-icon.svg into macos/AppIcon.icns. Run from the repository root:
//   xcrun swift scripts/make-icon.swift [preview.png]
// NSImage reads SVG on macOS 14 or later. The optional argument also writes the
// 1024 px rendering, for a quick look.
import AppKit

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
guard let image = NSImage(contentsOf: root.appendingPathComponent("app-icon.svg")) else {
  FileHandle.standardError.write(Data("Cannot read app-icon.svg (needs macOS 14 or later)\n".utf8))
  exit(1)
}

// Draw in sRGB, the SVG's own color space, so the tile color doesn't shift.
func png(pixels: Int) -> Data {
  let context = CGContext(
    data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
  NSGraphicsContext.current?.imageInterpolation = .high
  image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels))
  NSGraphicsContext.restoreGraphicsState()
  return NSBitmapImageRep(cgImage: context.makeImage()!).representation(using: .png, properties: [:])!
}

let files = FileManager.default
let iconset = files.temporaryDirectory.appendingPathComponent("TorrStreamLite-\(getpid()).iconset")
try? files.removeItem(at: iconset)
try files.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? files.removeItem(at: iconset) }
for points in [16, 32, 128, 256, 512] {
  try png(pixels: points).write(to: iconset.appendingPathComponent("icon_\(points)x\(points).png"))
  try png(pixels: points * 2).write(to: iconset.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
if CommandLine.arguments.count > 1 {
  try png(pixels: 1024).write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
}

let output = root.appendingPathComponent("macos/AppIcon.icns")
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", output.path]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else { exit(1) }
print("Wrote \(output.path)")
