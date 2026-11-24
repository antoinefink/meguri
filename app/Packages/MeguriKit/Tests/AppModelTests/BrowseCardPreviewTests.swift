import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#endif
@testable import AppModel

/// Exercise the real page pair, including side changes queued during render.
@MainActor @Suite(.serialized) struct BrowseCardPreviewTests {
    @Test func directSelectionFlipSaveAndNavigationKeepTheRenderedSideInSync() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
        let preview = BrowseCardPreview()
        defer { preview.close(); Task { await resolver.close(); try? FileManager.default.removeItem(at: root) } }
        preview.resolver = { resolver }
        preview.render = { id in
            CardPayload(cardID: id, question: "Front \(id)", answer: "Back \(id)",
                        audioPolicy: .init(autoplay: false, interrupt: true, replayQuestion: false))
        }
        preview.show(card: 7)
        // Set Back before the first question finishes preparing.
        preview.selectSide(.answer)
        #if os(macOS)
        try await until { preview.pair != nil }
        let pair = try #require(preview.pair)
        let window = PreviewTestWindow(contentRect: CGRect(x: -30_000, y: -30_000, width: 480, height: 320),
                                       styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        pair.container.frame = CGRect(x: 0, y: 0, width: 480, height: 320)
        window.contentView = pair.container
        defer { window.contentView = nil; window.close() }
        #endif
        try await shown(preview, card: 7, side: .answer)
        preview.selectSide(.answer) // direct selection is idempotent
        preview.flip()
        try await shown(preview, card: 7, side: .question)
        preview.selectSide(.answer)
        try await shown(preview, card: 7, side: .answer)
        preview.show(card: 7) // a field save rerenders this card
        try await shown(preview, card: 7, side: .answer)
        preview.selectSide(.question)
        preview.selectSide(.answer)
        preview.selectSide(.question)
        try await shown(preview, card: 7, side: .question)
        preview.show(card: 8) // another card starts on Front
        try await shown(preview, card: 8, side: .question)

        // An actual reveal error restores Front; selecting Back again retries
        // from a fresh question page rather than the failed answer identity.
        let page = try #require(preview.pair?.current)
        _ = try await page.webView.callAsyncJavaScript("window._showAnswer = () => { throw new Error('synthetic reveal failure'); };",
                                                       arguments: [:], in: nil, contentWorld: .page)
        preview.selectSide(.answer)
        try await until { preview.failed }
        #expect(preview.side == .question)
        preview.selectSide(.answer)
        try await shown(preview, card: 8, side: .answer)

        // Losing native completion cannot strand all subsequent presentations.
        let committed = preview.pair?.onCommitted
        preview.pair?.onCommitted = nil
        preview.selectSide(.question)
        try await until { preview.failed }
        #expect(preview.side == .question)
        preview.pair?.onCommitted = committed
        preview.selectSide(.question)
        try await shown(preview, card: 8, side: .question)

        // Supersede a question at the exact callback/await boundary. Its old
        // task must not publish card 9 after clear has invalidated the request.
        preview.pair?.onCommitted = { update, receipt in
            committed?(update, receipt)
            preview.clear()
        }
        preview.show(card: 9)
        try await until { preview.showing == 0 }
        #expect(preview.card == nil && !preview.hasAudio)
        preview.clear()
        #expect(preview.card == nil && preview.side == .question && !preview.hasAudio)
    }

    private func shown(_ preview: BrowseCardPreview, card: Int64, side: CardRenderSide) async throws {
        try await until {
            guard preview.showing == 0, preview.card == card, preview.side == side,
                  let page = preview.pair?.current, page.update?.side == side, page.presented, !preview.failed else { return false }
            let rendered = try await page.webView.callAsyncJavaScript("return document.getElementById('qa')?.textContent.trim();",
                                                                    arguments: [:], in: nil, contentWorld: .page) as? String
            return rendered == "\(side == .question ? "Front" : "Back") \(card)"
        }
        #expect(!preview.failed && preview.showing == 0 && preview.card == card && preview.side == side)
        #expect(preview.pair?.current.update?.card == card && preview.pair?.current.update?.side == side)
    }

    private func until(_ condition: () async throws -> Bool) async throws {
        for _ in 0..<1000 {
            if try await condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        try #require(try await condition(), "preview completed within its bounded wait")
    }
}

#if os(macOS)
/// The CardRendering test harness uses this same never-ordered window pattern
/// to let real offscreen WebKit pages run their frame callbacks.
@MainActor private final class PreviewTestWindow: NSWindow {
    override var isVisible: Bool { true }
    override var occlusionState: NSWindow.OcclusionState { .visible }
}
#endif
