import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Copy Image and Copy Values (S9, N25.5): the chart's context menu on its box
/// and on its page.
@MainActor
enum StatsChartExport {
    /// Puts the image of `content`, drawn at `scale` in `colorScheme`, on the pasteboard.
    static func copyImage(of content: some View, width: CGFloat, colorScheme: ColorScheme, scale: CGFloat) {
        let renderer = ImageRenderer(content: content
            .frame(width: width)
            .padding(Spacing.medium)
            .background(Color.clear)
            .environment(\.colorScheme, colorScheme))
        renderer.scale = scale
        #if os(macOS)
        guard let image = renderer.nsImage else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.writeObjects([image])
        #else
        guard let image = renderer.uiImage else { return }
        UIPasteboard.general.image = image
        #endif
    }

    /// Puts the tab-separated table on the pasteboard as text.
    static func copyValues(_ text: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #else
        UIPasteboard.general.string = text
        #endif
    }
}
