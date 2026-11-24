import CardRendering
import CoreBridge
import Foundation
import Testing
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
        preview.clear()
        #expect(preview.card == nil && preview.side == .question && !preview.hasAudio)
    }

    private func shown(_ preview: BrowseCardPreview, card: Int64, side: CardRenderSide) async throws {
        for _ in 0..<1000 {
            if preview.showing == 0, preview.card == card, preview.side == side,
               preview.pair?.current.update?.side == side, preview.pair?.current.presented == true { break }
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(!preview.failed && preview.showing == 0 && preview.card == card && preview.side == side)
        #expect(preview.pair?.current.update?.card == card && preview.pair?.current.update?.side == side)
    }
}
