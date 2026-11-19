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

    // Synthetic DTO examples show mapping/layout only. Media input remains guarded.
    private static func presentation(_ state: PreviewState) -> CollectionEntryPresentation {
        switch state {
        case .empty:
            .init()
        case .typical:
            .init(facts: .init(notes: 1_204, cards: 3_410, reviews: 18_250, replacingExisting: true, mediaCount: 212, mediaDecodedBytes: 48_300_000),
                  fileName: "Japanese.colpkg")
        case .overflow:
            // Large counts and the actual, longer replacement/safety explanation.
            // There are no source titles, paths or other unrendered source fields.
            .init(facts: .init(notes: 12_345_678, cards: 98_765_432, reviews: 1_234_567_890, replacingExisting: true,
                               mediaCount: 12_345, mediaDecodedBytes: 9_876_543_210),
                  fileName: "A collection exported from another device with a very long descriptive name 2026-11-19.colpkg")
        case .error:
            .init(message: CollectionEntryCoordinator.explanation(
                CollectionEntryFailure(issue: .replacement(.mediaRecoveryRequired(recoveryReference: "r6-preview"))), fallback: ""
            ), canChoose: false, requiresRecovery: true,
                  preparedMedia: .init(incoming: 5, decodedBytes: 2048, reused: 2, added: 3, trashed: 1),
                  mediaReference: "r6-preview")
        case .loading:
            .init(progress: .init(title: "Importing…", completed: 42, total: 100, canCancel: true), canChoose: false,
                  preparedMedia: .init(incoming: 5, decodedBytes: 2048, reused: 2, added: 3, trashed: 1))
        }
    }
}

#Preview("Empty") { CollectionEntryPreview.preview(.empty) }
#Preview("Typical") { CollectionEntryPreview.preview(.typical) }
#Preview("Overflow") { CollectionEntryPreview.preview(.overflow) }
#Preview("Error") { CollectionEntryPreview.preview(.error) }
#Preview("Loading") { CollectionEntryPreview.preview(.loading) }

// Synthetic completed result; this preview does not enable guarded media input.
#Preview("Completed import") {
    CollectionEntryContent(presentation: .init(activated: true,
        finalMedia: .init(incoming: 5, decodedBytes: 2048, reused: 2, added: 3, trashed: 1),
        generation: 19, mediaReference: "r6-preview-retained"),
        actions: .init(confirm: {}, chooseFile: {}, reinspect: {}, continueRecovery: {}, dismiss: {}))
}
