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

@Suite struct TypeAnswerTests {
    @Test func pureComparisonUsesTheInteractiveLaneWithoutAnOpenCollection() async throws {
        let d = TypeAnswerDescriptor(expected: "ka")
        #expect(try await Core.compareTypedAnswer(d, typed: "ka") == "<code id=typeans><span class=typeGood>ka</span></code>")
        #expect(try await Core.compareTypedAnswer(d, typed: "kq") == "<code id=typeans><span class=typeGood>k</span><span class=typeBad>q</span><br><span id=typearrow>&darr;</span><br><span class=typeGood>k</span><span class=typeMissed>a</span></code>")
        #expect(try await Core.compareTypedAnswer(d, typed: "") == "<code id=typeans>ka</code>")
        let nc = TypeAnswerDescriptor(expected: "Qëvax", mode: .nc, caseInsensitive: true, font: "Qav Serif", size: 28)
        #expect(try await Core.compareTypedAnswer(nc, typed: "QEVAX") == "<code id=typeans><span class=typeGood>Qëvax</span></code>")
        #expect(BridgeCall.compareTypedAnswer.name == "compare_typed_answer")
        #expect(TypeAnswerDescriptor.placeholderHTML == "<span data-meguri-type-answer></span>")
    }
}
