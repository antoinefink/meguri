import Testing
@testable import CoreBridge
import Instrumentation

@Suite struct RenderingTests {
    @Test func renderIsCallableAndReportsClosedCollectionOnItsLane() async {
        // U37 supplies process-wide collection opening. The real stored-card
        // success path is covered in engine/T0 tests until that provider lands.
        for mode in [CardRenderMode.review, .preview, .browser] {
            do {
                _ = try await Core.renderCard(id: 1, mode: mode, platform: .mac)
                Issue.record("No collection is open in this test process")
            } catch {
                guard case .unavailable = error else {
                    Issue.record("Expected unavailable, got \(error)"); continue
                }
            }
        }
        #expect(BridgeCall.renderCard.name == "render_card")
    }
}
