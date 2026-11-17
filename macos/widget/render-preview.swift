import AppKit
import SwiftUI

/// Renders the real widget view with explicitly synthetic data. It never
/// resolves an App Group, reads preferences, launches the CLI, or uses a key.
@main
struct UsageWidgetPreviewRenderer {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 2,
              CommandLine.arguments[1].hasPrefix("/") else {
            throw RenderError.usage
        }
        let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        NSApplication.shared.setActivationPolicy(.prohibited)

        let snapshot = UsageWidgetPreview.snapshot()
        for (width, height) in [(360, 170), (330, 155)] {
            for (name, scheme) in [("light", ColorScheme.light), ("dark", .dark)] {
                let file = output.appendingPathComponent("widget-preview-\(name)-\(width)x\(height).png")
                try render(snapshot: snapshot, now: UsageWidgetPreview.date,
                           width: width, height: height, scheme: scheme, to: file)
            }
        }

        // Exercise the same small layout with old readings and unavailable
        // accounts. Every output still carries the synthetic-preview label.
        let aged = UsageWidgetPreview.date.addingTimeInterval(900)
        try render(snapshot: snapshot, now: aged, width: 330, height: 155, scheme: .dark,
                   to: output.appendingPathComponent("widget-preview-stale-dark-330x155.png"))
        try render(snapshot: UsageWidgetSnapshot(generatedAt: UsageWidgetPreview.date),
                   now: UsageWidgetPreview.date, width: 330, height: 155, scheme: .light,
                   to: output.appendingPathComponent("widget-preview-empty-light-330x155.png"))
    }

    @MainActor
    private static func render(snapshot: UsageWidgetSnapshot, now: Date,
                               width: Int, height: Int, scheme: ColorScheme, to file: URL) throws {
        let content = UsageWidgetView(snapshot: snapshot, now: now, isPreview: true)
            .frame(width: CGFloat(width), height: CGFloat(height))
            .environment(\.colorScheme, scheme)
            .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        let renderer = ImageRenderer(content: content)
        renderer.scale = 2
        guard let image = renderer.cgImage,
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw RenderError.noImage
        }
        try png.write(to: file, options: .atomic)
        print(file.path)
    }

    enum RenderError: Error, CustomStringConvertible {
        case usage
        case noImage

        var description: String {
            switch self {
            case .usage: return "Usage: widget-preview-renderer /absolute/output/directory"
            case .noImage: return "SwiftUI did not produce a preview image"
            }
        }
    }
}
