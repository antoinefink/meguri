import MeguriCoreBindings
import Testing

@testable import CoreBridge

@Suite struct ErrorTests {
    static let cases: [(CoreError, BridgeError)] = [
        (.CloudPolicy(failure: .staleCommand), .cloudPolicy(.staleCommand)),
        (.CloudSdkCallbackRejected, .cloudSDKCallbackRejected),
        // A generic failure with the old fence's text is still a real error.
        (.Unavailable(reason: "cloud runtime incarnation is stale"), .unavailable(reason: "cloud runtime incarnation is stale")),
        (.Stale(table: .notetypes, key: "37"), .stale(table: .notetypes, key: "37")),
        (.StaleGeneration(planned: 37, current: 41), .staleGeneration(planned: 37, current: 41)),
        (.UndoUnavailable(reason: .alreadyUploaded), .undoUnavailable(reason: .alreadyUploaded)),
        (.NotetypeRefused(refusal: .template(kind: .unknownField, template: 3, side: .back, name: "Missing", earlier: nil)),
         .notetypeRefused(.template(kind: .unknownField, template: 3, side: .back, name: "Missing", earlier: nil))),
        (.NotetypeRefused(refusal: .invalid(reason: .emptyFieldName(ord: 7))), .notetypeRefused(.invalid(reason: .emptyFieldName(ord: 7)))),
        (.DeckRefused(refusal: .underDescendant), .deckRefused(.underDescendant)),
        (.NotetypeStructureRefused(refusal: .noteFieldCountMismatch(note: -9)),
         .notetypeStructureRefused(.noteFieldCountMismatch(note: -9))),
        (.NotetypeStructureRefused(refusal: .removalLossesChanged(notes: 2, cards: 5)),
         .notetypeStructureRefused(.removalLossesChanged(notes: 2, cards: 5))),
        (.NotetypeStructureRefused(refusal: .schemaChanged), .notetypeStructureRefused(.schemaChanged)),
        (.NoteNotFound(id: -37), .noteNotFound(id: -37)),
        (.NotFound(table: .decks, key: "-41"), .notFound(table: .decks, key: "-41")),
        (.InsufficientSpace(required: 100, available: 99), .insufficientSpace(required: 100, available: 99)),
        (.Open(failure: .upgradeFailed(version: 11, stage: .step(step: 16), detail: "failed")), .open(.upgradeFailed(version: 11, stage: .step(16), detail: "failed"))),
        (.SuspendDeadline(phase: "readers"), .suspendDeadline(phase: "readers")),
        (.SessionActive, .sessionActive),
        (.InvalidTypeAnswerReplacement, .invalidTypeAnswerReplacement),
        (.ExportSourceChanged, .exportSourceChanged),
        (.ExportSourceInvalid, .exportSourceInvalid),
        (.ExportFormatLimit, .exportFormatLimit),
        (.Cancelled, .cancelled),
        (.Busy(runningOp: "Check Database"), .busy(runningOperation: "Check Database")),
        (.Unavailable(reason: "exclusive phase"), .unavailable(reason: "exclusive phase")),
        (.Unsupported(feature: "servers"), .unsupported(feature: "servers")),
        (.InvalidArgument(message: "deck id 0"), .invalidArgument("deck id 0")),
        (.Internal(message: "invariant"), .internal("invariant")),
        (.MediaCommitPending(seq: 17, message: "recover attachments"), .mediaCommitPending(sequence: 17, message: "recover attachments")),
        (.CommitCompleted(seq: 18, message: "refresh state"), .commitCompleted(sequence: 18, message: "refresh state")),
    ]

    @Test(arguments: cases)
    func aRustErrorArrivesAsItsTypedBridgeError(core: CoreError, expected: BridgeError) async {
        await #expect(throws: expected) {
            try await MaintenanceLane.shared.call("test.returnError") { try testReturnError(error: core) }
        }
    }

    @Test func aRustPanicArrivesAsAnInternalErrorAndTheProcessSurvives() async throws {
        let error = await #expect(throws: BridgeError.self) {
            try await MaintenanceLane.shared.call("test.panic") { try testPanic(message: "deliberate test panic") }
        }
        guard case .internal(let message) = error else {
            Issue.record("expected .internal, got \(String(describing: error))")
            return
        }
        #expect(message.contains("deliberate test panic"))
        // The core is still usable after the caught panic.
        #expect(try await Core.version() == (try VersionTests.workspaceVersion()))
    }
}
