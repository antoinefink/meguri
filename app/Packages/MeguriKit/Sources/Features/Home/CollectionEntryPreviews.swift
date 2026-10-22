import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

public enum CollectionEntryPreview: PreviewStates {
    public static let previewID = "onboarding.collectionImport"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in
            PreviewFixture(state) {
                CollectionEntryContent(presentation: presentation(state), actions: .init(
                    confirm: {}, chooseFile: {}, reinspect: {}, continueRecovery: {}, dismiss: {}
                ))
            }
        }
    }

    private static func presentation(_ state: PreviewState) -> CollectionEntryPresentation {
        switch state {
        case .empty:
            .init()
        case .typical:
            .init(facts: .init(notes: 6, cards: 11, reviews: 6, replacingExisting: false))
        case .overflow:
            // Large counts and the actual, longer replacement/safety explanation.
            // There are no source titles, paths or other unrendered source fields.
            .init(facts: .init(notes: 12_345_678, cards: 98_765_432, reviews: 1_234_567_890, replacingExisting: true))
        case .error:
            .init(message: CollectionEntryCoordinator.explanation(
                CollectionEntryFailure(issue: .staleConfirmation), fallback: ""
            ), needsReinspection: true)
        case .loading:
            .init(progress: .init(title: "Copying collection…", completed: 42, total: 100, canCancel: true), canChoose: false)
        }
    }
}

#Preview("Empty") { CollectionEntryPreview.preview(.empty) }
#Preview("Typical") { CollectionEntryPreview.preview(.typical) }
#Preview("Overflow") { CollectionEntryPreview.preview(.overflow) }
#Preview("Error") { CollectionEntryPreview.preview(.error) }
#Preview("Loading") { CollectionEntryPreview.preview(.loading) }
