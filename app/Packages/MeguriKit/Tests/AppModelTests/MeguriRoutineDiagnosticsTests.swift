import MeguriCoreBindings
import Testing

@testable import AppModel
@testable import CoreBridge

@MainActor @Suite struct MeguriRoutineDiagnosticsTests {
    private func report(kind: String? = nil, complete: Bool = false) -> MeguriRoundReport {
        let collection = MeguriCollectionRound(.init(state: .noChanges, failure: nil, refusal: nil,
            acceptanceUnknown: false, skipped: 0, recovery: nil))
        let failure: MeguriCoreBindings.ProviderRoundFailure? = kind.map {
            .init(kind: $0, method: nil, status: nil, clockOffsetSeconds: nil, databaseCheckIsLocal: nil)
        }
        let media = MeguriMediaRound(.init(complete: complete, failure: failure, refusal: nil, recovery: nil,
            uploadedFiles: 0, uploadedDeletions: 0, downloadedFiles: 0, downloadedDeletions: 0,
            bytesUploaded: 0, bytesDownloaded: 0, checked: 0, resynced: false, excluded: 0,
            pending: 0, awaiting: 0, affected: [], accountingFailure: nil))
        return .init(collection: .init(outcome: .collection(collection), finalization: nil, trackingFailure: nil),
            media: .init(outcome: .media(media), finalization: nil, trackingFailure: nil))
    }

    @Test func noChangeCollectionKeepsMediaFailureReasonAndNeverInventsApply() {
        let cases: [(String, FieldDiagnosticObservation.RoundReason)] = [
            ("network", .network), ("auth_failed", .authentication), ("server_error", .server), ("cancelled", .cancelled)
        ]
        for (kind, reason) in cases {
            var events: [FieldDiagnosticObservation] = []
            let diagnostics = MeguriRoutineDiagnostics { events.append($0) }
            _ = diagnostics.observe([.collectionStarted])
            _ = diagnostics.observe([.mediaCheckStarted])
            let value = report(kind: kind)
            #expect(!value.complete)
            diagnostics.finish(value)
            #expect(events.last?.roundReason == reason)
            #expect(events.last?.outcome == (reason == .cancelled ? .cancelled : .incomplete))
            #expect(events.compactMap(\.roundMilestone) == [.started, .collectionStarted, .mediaCheckStarted, .terminal])
        }
    }

    @Test func nativeInterruptedAtEitherTerminalBoundaryIsCancellation() {
        let failure = MeguriCoreBindings.ProviderRoundFailure(kind: "interrupted", method: nil,
            status: nil, clockOffsetSeconds: nil, databaseCheckIsLocal: nil)
        let collection = MeguriCollectionRound(.init(state: .failed, failure: failure, refusal: nil,
            acceptanceUnknown: false, skipped: 0, recovery: nil))
        let collectionStop = MeguriRoundReport(collection: .init(outcome: .collection(collection),
            finalization: nil, trackingFailure: nil), media: nil)
        for value in [collectionStop, report(kind: failure.kind)] {
            var events: [FieldDiagnosticObservation] = []
            let diagnostics = MeguriRoutineDiagnostics { events.append($0) }
            diagnostics.finish(value)
            #expect(events.compactMap(\.roundMilestone) == [.started, .terminal])
            #expect(events.last?.roundReason == .cancelled && events.last?.outcome == .cancelled)
        }
    }

    @Test func credentialFailuresBeforeNetworkKeepTypedReasonsWithoutPhaseMetrics() {
        let failures: [(any Error, FieldDiagnosticObservation.RoundReason)] = [
            (MeguriRoundAdmissionFailure.credentialsMissing, .authentication),
            (MeguriProviderCredentialFailure.access(-1), .localFailure),
            (MeguriProviderCredentialFailure.malformedPayload, .localFailure),
            (MeguriProviderCredentialFailure.wrongConnection, .localFailure)
        ]
        for (error, reason) in failures {
            var events: [FieldDiagnosticObservation] = []
            let diagnostics = MeguriRoutineDiagnostics { events.append($0) }
            diagnostics.finish(error: error)
            #expect(events.map(\.roundMilestone) == [.started, .terminal])
            #expect(events.last?.roundReason == reason && events.last?.outcome == .failed)
        }
    }

    @Test func terminalSuccessRefusalAndCancellationKeepTheirActualOutcomesExactlyOnce() {
        var events: [FieldDiagnosticObservation] = []
        let success = MeguriRoutineDiagnostics { events.append($0) }
        success.finish(report(complete: true))
        success.finish(error: BridgeError.cancelled)
        #expect(events.compactMap(\.roundMilestone) == [.started, .terminal])
        #expect(events.last?.outcome == .succeeded && events.last?.roundReason == .completed)
        events = []
        let refused = MeguriRoutineDiagnostics { events.append($0) }
        refused.finish(.init(collection: .init(outcome: .refused(.paused), finalization: nil, trackingFailure: nil), media: nil))
        #expect(events.last?.outcome == .refused && events.last?.roundReason == .refused)
        events = []
        let cancelled = MeguriRoutineDiagnostics { events.append($0) }
        cancelled.finish(error: BridgeError.cancelled)
        #expect(events.last?.outcome == .cancelled && events.last?.roundReason == .cancelled)
    }
}
