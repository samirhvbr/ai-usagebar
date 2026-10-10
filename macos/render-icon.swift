// Reuse the Windows SVG mark for Finder's multi-resolution application icon.
// Run on macOS: swift macos/render-icon.swift
import AppKit

let script = URL(fileURLWithPath: #filePath)
let macos = script.deletingLastPathComponent()
let source = macos.deletingLastPathComponent().appendingPathComponent("windows/tray-icon.svg")
let svg = try String(contentsOf: source, encoding: .utf8)
    .replacingOccurrences(of: "currentColor", with: "#171717")
    .replacingOccurrences(of: "1em", with: "1024")
guard let mark = NSImage(data: Data(svg.utf8)) else {
    fatalError("Could not decode Windows tray artwork")
}
let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
let iconset = temporary.appendingPathComponent("AIUsage.iconset")
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: temporary) }

for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        let side = CGFloat(pixels)
        guard let bitmap = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
            fatalError("Could not create icon bitmap")
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        // A light tile keeps the black tray mark legible in light/dark Finder.
        NSColor(calibratedWhite: 0.97, alpha: 1).setFill()
        NSBezierPath(
            roundedRect: NSRect(x: side * 0.05, y: side * 0.05, width: side * 0.9, height: side * 0.9),
            xRadius: side * 0.2, yRadius: side * 0.2
        ).fill()
        mark.draw(in: NSRect(x: side * 0.11, y: side * 0.11, width: side * 0.78, height: side * 0.78))
        NSGraphicsContext.restoreGraphicsState()
        let suffix = scale == 2 ? "@2x" : ""
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            fatalError("Could not encode icon bitmap")
        }
        try png.write(to: iconset.appendingPathComponent("icon_\(points)x\(points)\(suffix).png"))
    }
}
let converter = Process()
converter.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
converter.arguments = ["-c", "icns", iconset.path, "-o", macos.appendingPathComponent("AIUsage.icns").path]
try converter.run()
converter.waitUntilExit()
guard converter.terminationStatus == 0 else { fatalError("iconutil failed") }
