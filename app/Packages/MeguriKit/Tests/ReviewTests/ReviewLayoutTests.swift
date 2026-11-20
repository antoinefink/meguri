#if os(macOS)
import AppKit
import AppModel
import DesignSystem
import SwiftUI
import Testing
@testable import Review

/// The Mac review's geometry (native-interface-contract N16.1, N16.4): the
/// card fills the window, states keep the readable column, and the progress
/// bar is the window's last row, edge to edge. Rendered in a borderless
/// window that is never ordered in, so nothing reaches this Mac's screen.
@MainActor @Suite struct ReviewLayoutTests {
    @Observable final class Width { var value: CGFloat = -1 }

    private func host<V: View>(_ view: V, size: CGSize) -> (NSWindow, NSHostingView<V>) {
        let window = NSWindow(contentRect: CGRect(origin: CGPoint(x: -30_000, y: -30_000), size: size),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: .aqua)
        let hosting = NSHostingView(rootView: view)
        hosting.frame = CGRect(origin: .zero, size: size)
        window.contentView = hosting
        hosting.layoutSubtreeIfNeeded()
        return (window, hosting)
    }

    private func settle(_ hosting: NSView, until done: () -> Bool) async throws {
        for _ in 0..<100 where !done() {
            hosting.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    private func layout<Card: View>(phase: ReviewCoordinator.Phase, @ViewBuilder card: @escaping () -> Card) -> ReviewLayout<Card> {
        ReviewLayout(progress: ReviewSessionProgress(answered: 1, remaining: 3), notice: nil,
            undoDescription: "Undo", canUndo: false, canRedo: false, phase: phase, states: nil, enabled: true,
            close: {}, undo: {}, redo: {}, reveal: {}, rate: { _ in }) { card() }
    }

    /// N16.4 (changed 2026-11-20): no reading column for the card. With the
    /// earlier 640 pt column the card's page was narrower than the window its
    /// styling was written for.
    @Test(arguments: [800.0, 1000.0, 1600.0]) func cardFillsTheWindow(width: Double) async throws {
        let measured = Width()
        let (window, hosting) = host(layout(phase: .answer) {
            Color.clear.onGeometryChange(for: CGFloat.self) { $0.size.width } action: { measured.value = $0 }
        }, size: CGSize(width: width, height: 700))
        defer { window.close() }
        try await settle(hosting) { measured.value >= 0 }
        #expect(measured.value == CGFloat(width))
    }

    /// N16.4, N16.6: a state's text keeps the centred readable column.
    @Test(arguments: [800.0, 1600.0]) func statesKeepTheReadableColumn(width: Double) async throws {
        let measured = Width()
        let (window, hosting) = host(layout(phase: .waiting) {
            ReviewStatusLayout {
                Color.clear.frame(maxWidth: .infinity, minHeight: 10)
                    .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { measured.value = $0 }
            }
        }, size: CGSize(width: width, height: 700))
        defer { window.close() }
        try await settle(hosting) { measured.value >= 0 }
        #expect(measured.value == min(ContentWidth.readable, CGFloat(width)))
    }

    /// N16.1: the bar's track reaches the window's bottom edge and both sides;
    /// just above it is the card page. The system indicator left a gap below
    /// its inset track and at its rounded ends.
    @Test(arguments: [800.0, 1000.0]) func progressBarIsTheBottomEdge(width: Double) async throws {
        let size = CGSize(width: width, height: 600)
        let (window, hosting) = host(layout(phase: .question) { Color.clear }, size: size)
        defer { window.close() }
        try await settle(hosting) { false }
        let bitmap = try #require(hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds))
        hosting.cacheDisplay(in: hosting.bounds, to: bitmap)
        let scale = CGFloat(bitmap.pixelsWide) / size.width
        func pixel(_ x: CGFloat, _ yFromBottom: CGFloat) -> NSColor? {
            bitmap.colorAt(x: Int(x * scale), y: bitmap.pixelsHigh - 1 - Int(yFromBottom * scale))?.usingColorSpace(.sRGB)
        }
        let page = try #require(pixel(size.width / 2, size.height / 2))
        func isPage(_ color: NSColor?) -> Bool {
            guard let color else { return false }
            return abs(color.redComponent - page.redComponent) < 0.02 && abs(color.greenComponent - page.greenComponent) < 0.02
                && abs(color.blueComponent - page.blueComponent) < 0.02
        }
        // Bottom pixel row: the bar at the leading edge (its value), the middle and the trailing edge (its track).
        #expect(!isPage(pixel(0, 0)))
        #expect(!isPage(pixel(size.width / 2, 0)))
        #expect(!isPage(pixel(size.width - 1 / scale, 0)))
        // Above the bar's thickness, beside the answer row: the card page.
        #expect(isPage(pixel(0, 6)))
        #expect(isPage(pixel(size.width - 1 / scale, 6)))
    }
}
#endif
