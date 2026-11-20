import MeguriCoreBindings
import Testing
@testable import AppModel
@testable import CoreBridge

@MainActor struct FieldDiagnosticCloudStatusTests {
    @Test func nilDelayedHistoryAndCollectionActivityNeverInventCompletion() {
        var projection = FieldDiagnosticCloudStatus()
        var events: [FieldDiagnosticObservation] = []
        let record: FieldDiagnosticRecorder = { events.append($0) }
        projection.observe(status(), generation: 1, revision: 1, record: record)
        #expect(!events.contains { $0.operation == .exchange })
        projection.observe(status(state: .syncingCollection), generation: 1, revision: 1, record: record)
        projection.observe(status(attempt: attempt(1)), generation: 1, revision: 1, record: record)
        #expect(events.filter { $0.operation == .exchange }.map(\.outcome) == [.unknown])
        #expect(!events.contains { $0.stage == .exchangeCompleted })
        let seeded = events
        projection.observe(status(attempt: attempt(1)), generation: 1, revision: 1, record: record)
        #expect(events == seeded)
    }

    @Test func threeHistoricalAttemptsAndReplayCannotBecomeLiveIncludingSuccessorOwner() {
        var projection = FieldDiagnosticCloudStatus()
        var events: [FieldDiagnosticObservation] = []
        let record: FieldDiagnosticRecorder = { events.append($0) }
        for id in [UInt64(1), 2, 3, 2] {
            projection.observe(status(attempt: attempt(id)), generation: 1, revision: 1, record: record)
        }
        #expect(events.filter { $0.operation == .exchange }.map(\.outcome) == Array(repeating: .unknown, count: 4))
        for (generation, revision) in [(UInt64(1), UInt64(2)), (2, 2)] {
            projection.observe(status(state: .syncingCollection), generation: generation, revision: revision, record: record)
            projection.observe(status(attempt: attempt(2)), generation: generation, revision: revision, record: record)
        }
        #expect(events.filter { $0.operation == .exchange }.count == 6)
        #expect(!events.contains { $0.stage == .exchangeCompleted || $0.stage == .convergenceObserved })
    }

    @Test func actualCompletionMappingIsSeparateFromQualifiedConvergence() {
        var projection = FieldDiagnosticCloudStatus()
        var events: [FieldDiagnosticObservation] = []
        let record: FieldDiagnosticRecorder = { events.append($0) }
        let succeeded = attempt(1, outcome: .succeeded(selected: 0, heldBack: 0, newerPending: 0,
            incomingWaiting: 0, media: media()))
        FieldDiagnosticCloudStatus.completed(.init(succeeded), record: record)
        #expect(events.map(\.stage) == [.exchangeCompleted])
        #expect(events.map(\.outcome) == [.succeeded])
        projection.observe(status(state: .upToDate(asOfMs: 100), attempt: succeeded),
            generation: 1, revision: 1, record: record)
        #expect(!events.contains { $0.stage == .convergenceObserved })
        projection.observe(status(state: .upToDate(asOfMs: 100), asOf: 100, attempt: succeeded),
            generation: 1, revision: 1, record: record)
        #expect(events.filter { $0.stage == .convergenceObserved }.map(\.outcome) == [.observed])
        #expect(events.filter { $0.stage == .exchangeCompleted }.count == 1)
        projection.observe(status(attempt: succeeded), generation: 1, revision: 1, record: record)
        #expect(events.filter { $0.stage == .convergenceObserved }.count == 1)
    }

    @Test func absentMediaMeasurementsStayAbsentAndTypedFactsKeepTheirMeaning() {
        var projection = FieldDiagnosticCloudStatus()
        var events: [FieldDiagnosticObservation] = []
        let record: FieldDiagnosticRecorder = { events.append($0) }
        projection.observe(status(), generation: 1, revision: 1, record: record)
        #expect(events.filter { $0.component == .media }.map(\.outcome) == [.observed])
        #expect(events.last?.count == nil && events.last?.failure == nil && events.last?.durationMilliseconds == nil)
        projection.observe(status(facts: media(missing: 3)), generation: 1, revision: 1, record: record)
        #expect(events.last?.component == .media && events.last?.outcome == .incomplete && events.last?.count == 3)
        projection.observe(status(facts: media(running: 1)), generation: 1, revision: 1, record: record)
        #expect(events.last?.outcome == .waiting && events.last?.count == nil)
        projection.observe(status(facts: media(noSpace: true)), generation: 1, revision: 1, record: record)
        #expect(events.last?.outcome == .failed && events.last?.failure == .noSpace)
        projection.observe(status(facts: media()), generation: 1, revision: 1, record: record)
        #expect(events.last?.outcome == .observed && events.last?.failure == nil && events.last?.count == nil)
    }

    private func media(missing: UInt64? = nil, running: UInt64? = nil, noSpace: Bool? = nil) -> MeguriCoreBindings.CloudPolicyMediaFacts {
        .init(uploadFiles: nil, uploadBytes: nil, downloadFiles: nil, downloadBytes: nil,
            unknownBlobs: nil, missingNames: missing, uploadsRunning: running, downloadsRunning: nil,
            remainingDownloadMs: nil, rootAcked: nil, zoneMissing: nil, noSpace: noSpace)
    }

    private func attempt(_ id: UInt64, outcome: MeguriCoreBindings.CloudPolicyExchangeOutcome = .cancelled) -> MeguriCoreBindings.CloudPolicyExchangeRecord {
        .init(exchange: id, endedMs: Int64(id), source: .command, outcome: outcome)
    }

    // Synthetic value projections only; no command, proof or capability is built.
    private func status(state: MeguriCoreBindings.CloudPolicyStatusState = .awaitingEvidence,
                        asOf: Int64? = nil, attempt: MeguriCoreBindings.CloudPolicyExchangeRecord? = nil,
                        facts: MeguriCoreBindings.CloudPolicyMediaFacts? = nil) -> CoreBridge.CloudPolicyStatus {
        .init(MeguriCoreBindings.CloudPolicyStatus(state: state, stateWithoutAttention: nil, pending: 0, heldBack: 0, inFlight: 0,
            incomingWaiting: 0, attention: [], media: facts ?? media(), mediaPaused: false, error: nil,
            accountStatus: .available, accountMatchesBinding: true, asOfMs: asOf,
            lastCompleted: nil, lastAttempt: attempt, syncNow: .available,
            controls: .init(enable: .pause, bindingUsable: true, bindingActive: true,
                bindingStopped: false, decisionRequired: false, userPaused: false,
                resumeBindingOffered: nil, automaticEligible: true)))
    }
}
