import SwiftUI
import Testing

import DesignSystem

/// The state registration protocol (R1970) that feeds previews and
/// snapshots alike.
@Suite struct PreviewStatesTests {
    struct Sample: PreviewStates {
        static let previewID = "test.sample"
        static var previewFixtures: [PreviewFixture] {
            [
                PreviewFixture(.loading) { Text(verbatim: "Loading") },
                PreviewFixture(.typical) { Text(verbatim: "Typical") },
                PreviewFixture(.empty) { Text(verbatim: "Empty") },
            ]
        }
    }

    @Test func theFiveStatesAreTheVocabulary() {
        #expect(PreviewState.allCases.map(\.rawValue) == ["empty", "typical", "overflow", "error", "loading"])
    }

    @Test func statesComeInVocabularyOrderWhateverTheFixtureOrder() {
        #expect(Sample.previewStates == [.empty, .typical, .loading])
    }

    @Test func aFixtureIsFoundByItsState() {
        #expect(Sample.fixture(.typical)?.state == .typical)
        #expect(Sample.fixture(.error) == nil)
    }

    @Test func chromeIsLiquidGlassUnlessAFlatRenderAsksOtherwise() {
        #expect(EnvironmentValues().chromeGlass == .liquid)
    }
}
