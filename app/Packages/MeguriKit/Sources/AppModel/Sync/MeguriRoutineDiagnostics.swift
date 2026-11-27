import CoreBridge
import Foundation

/// One routine attempt, independent of the mutable current provider snapshot.
/// A missing terminal in persisted history means this client stopped before
/// observing one. Neither a collection commit nor a replay invents success.
@MainActor final class MeguriRoutineDiagnostics {
    private let id = UUID()
    private let started = ContinuousClock.now
    private var seen: Set<MeguriRoundMilestone> = []
    private let record: FieldDiagnosticRecorder
    private var finished = false

    init(record: @escaping FieldDiagnosticRecorder) {
        self.record = record
        emit(.started)
    }
    func observe(_ milestones: [MeguriRoundMilestone]) -> MeguriRoutinePhase? {
        for milestone in milestones where seen.insert(milestone).inserted {
            switch milestone {
            case .collectionStarted: emit(.collectionStarted)
            case .serverAcknowledged: emit(.serverAcknowledged)
            case .applyStarted: emit(.applyStarted)
            case .applyCompleted: emit(.applyCompleted)
            case .mediaCheckStarted: emit(.mediaCheckStarted)
            case .mediaTransferStarted: emit(.mediaTransferStarted)
            }
        }
        if seen.contains(.mediaTransferStarted) { return .syncingMedia }
        if seen.contains(.mediaCheckStarted) { return .checkingMedia }
        if seen.contains(.applyStarted) { return .applying }
        if seen.contains(.collectionStarted) { return .collection }
        return nil
    }
    func finish(_ report: MeguriRoundReport) {
        if report.complete { finish(.completed, outcome: .succeeded); return }
        let reason: FieldDiagnosticObservation.RoundReason
        switch report.collection.outcome {
        case .refused: reason = .refused
        case .pending: reason = .unavailable
        case .collection(let value):
            if value.acceptanceUnknown { reason = .acceptanceUnknown }
            else if value.refusal != nil { reason = .refused }
            else if value.state == .fullSyncRequired { reason = .fullSyncRequired }
            else if let failure = value.failure { reason = Self.reason(failure) }
            else if value.recovery != nil { reason = .localFailure }
            else if let media = report.media {
                switch media.outcome {
                case .refused: reason = .refused
                case .media(let value):
                    if let failure = value.failure ?? value.accountingFailure { reason = Self.reason(failure) }
                    else if value.refusal != nil { reason = .refused }
                    else if value.recovery != nil { reason = .localFailure }
                    else { reason = .mediaIncomplete }
                default: reason = .unavailable
                }
            } else { reason = .cancelled }
        case .media: reason = .unavailable
        }
        let outcome: FieldDiagnosticObservation.Outcome = switch reason {
        case .cancelled: .cancelled
        case .refused: .refused
        default: .incomplete
        }
        finish(reason, outcome: outcome)
    }
    func finish(error: any Error) {
        let reason: FieldDiagnosticObservation.RoundReason
        switch error {
        case is CancellationError, BridgeError.cancelled: reason = .cancelled
        case MeguriRoundAdmissionFailure.credentialsMissing: reason = .authentication
        case MeguriProviderCredentialFailure.access(_),
             MeguriProviderCredentialFailure.malformedPayload,
             MeguriProviderCredentialFailure.wrongConnection: reason = .localFailure
        default: reason = .unavailable
        }
        finish(reason, outcome: reason == .cancelled ? .cancelled : .failed)
    }
    private static func reason(_ failure: MeguriRoundFailure) -> FieldDiagnosticObservation.RoundReason {
        switch failure.kind {
        case "cancelled", "interrupted": .cancelled
        case "auth_failed": .authentication
        case "offline", "timeout", "network": .network
        case "server_error", "unexpected_status", "server_message": .server
        default: .localFailure
        }
    }
    private func finish(_ reason: FieldDiagnosticObservation.RoundReason, outcome: FieldDiagnosticObservation.Outcome) {
        guard !finished else { return }
        finished = true
        emit(.terminal, outcome: outcome, reason: reason)
    }
    private func emit(_ milestone: FieldDiagnosticObservation.RoundMilestone,
                      outcome: FieldDiagnosticObservation.Outcome = .observed,
                      reason: FieldDiagnosticObservation.RoundReason? = nil) {
        let elapsed = started.duration(to: .now).components
        let milliseconds = UInt64(max(0, elapsed.seconds)) * 1_000
            + UInt64(max(0, elapsed.attoseconds) / 1_000_000_000_000_000)
        record(.init(component: .cloud, operation: .exchange, stage: .statusObserved, outcome: outcome,
            durationMilliseconds: milliseconds, roundID: id, roundMilestone: milestone, roundReason: reason))
    }
}
