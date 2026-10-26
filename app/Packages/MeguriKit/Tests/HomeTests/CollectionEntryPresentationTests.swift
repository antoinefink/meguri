import Testing
@testable import CoreBridge
@testable import Home

@MainActor @Suite struct CollectionEntryPresentationTests {
    private let plan = CollectionImportMedia(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)

    @Test(arguments: [false, true])
    func rolledBackAndPartialRecoveryCountsArePlans(partialRecovery: Bool) {
        let issue: ImportIssue = partialRecovery
            ? .replacement(.mediaRecoveryRequired(recoveryReference: "r6-partial"))
            : .replacement(.staging(.stagingCorrupt))
        let failure = CollectionEntryFailure(issue: issue, candidateActive: partialRecovery,
            mediaRecoveryReference: partialRecovery ? "r6-partial" : nil, media: plan)
        // Even candidate-active and partially published media cannot make a
        // prepared DTO into completed live operations.
        let presentation = CollectionEntryPresentation(failure: failure)
        #expect(presentation.preparedMedia == plan && presentation.finalMedia == nil)
        #expect(presentation.mediaDetailRows.map(\.count) == [5, 123, 2, 3, 1])
        #expect(presentation.mediaDetailRows.map(\.label) == ["Incoming media files", "Decoded media bytes",
            "Planned reused files", "Planned added files", "Planned files to move to trash"])
        #expect(presentation.mediaCompletionNote?.contains("Completion") == true)
        #expect(presentation.mediaCompletionNote?.contains("verification") == true)
    }

    @Test func unknownOutcomeRetainsPlanWithoutCompletedOperations() {
        let failure = CollectionEntryFailure(issue: .nativeActivationUncertain(.outcomePending), preparedMedia: plan)
        let presentation = CollectionEntryPresentation(failure: failure)
        #expect(presentation.preparedMedia == plan && presentation.finalMedia == nil)
        #expect(presentation.mediaDetailRows.map(\.label).contains("Planned added files"))
        #expect(presentation.mediaCompletionNote != nil)
    }

    @Test func completedSuccessUsesCompletedOperationLabels() {
        let presentation = CollectionEntryPresentation(finalMedia: plan)
        #expect(presentation.finalMedia == plan && presentation.mediaCompletionNote == nil)
        #expect(presentation.mediaDetailRows.map(\.count) == [5, 123, 2, 3, 1])
        #expect(presentation.mediaDetailRows.map(\.label) == ["Incoming media files", "Decoded media bytes",
            "Reused files", "Added files", "Files moved to trash"])
    }

    @Test func nativeFailureAfterSuccessfulReceiptKeepsCompletedTotals() {
        let activation = CollectionActivation(.init(generation: 19, backup: nil,
            media: .init(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1),
            mediaRetentionReference: "r6-retained"))
        let failure = CollectionEntryFailure(after: activation, issue: .core(.internal("tracking failure")))
        let presentation = CollectionEntryPresentation(failure: failure)
        #expect(presentation.finalMedia == plan && presentation.mediaCompletionNote == nil)
        #expect(presentation.mediaDetailRows.map(\.label).contains("Added files"))
        #expect(!presentation.mediaDetailRows.map(\.label).contains("Planned added files"))
    }
}
