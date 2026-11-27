import Foundation
import Synchronization

/// Content-free observations only. Completion and convergence are separate
/// stages; callers must report only the boundary they actually observed.
nonisolated public struct FieldDiagnosticObservation: Codable, Sendable, Equatable {
    public enum Component: String, Codable, Sendable {
        case startup, review, collectionImport, collectionExport, media, scheduling
        // Preserve the tag in installed local diagnostic archives.
        case sync = "cloud"
    }
    public enum Operation: String, Codable, Sendable { case open, start, answer, save, inspect, activate, recover, prepare, publish, cleanup, status, exchange, transfer }
    public enum Stage: String, Codable, Sendable { case requested, admission, coreAccepted, persistence, presentation, inspection, activation, preparation, publication, cleanup, statusObserved, exchangeCompleted, convergenceObserved }
    public enum Outcome: String, Codable, Sendable { case observed, succeeded, failed, waiting, cancelled, refused, stale, incomplete, unknown, cleanupPending }
    public enum Failure: String, Codable, Sendable {
        case other, storageUnavailable, unavailable, invalidState, queueRejected, transport, noSpace, permission, notAuthenticated, rateLimited, serviceUnavailable

        /// Deliberately does not bridge, describe, or inspect an arbitrary error.
        static func storageFailure(for _: any Error) -> Self { .storageUnavailable }
    }

    public enum RoundMilestone: String, Codable, Sendable {
        case started, collectionStarted, serverAcknowledged, applyStarted, applyCompleted
        case mediaCheckStarted, mediaTransferStarted, terminal
    }
    public enum RoundReason: String, Codable, Sendable {
        case completed, cancelled, refused, acceptanceUnknown, fullSyncRequired, authentication
        case network, server, mediaIncomplete, localFailure, unavailable
    }
    /// Random local correlation only, unrelated to any account, job or collection.
    public let roundID: UUID?
    public let roundMilestone: RoundMilestone?
    public let roundReason: RoundReason?
    public let component: Component
    public let operation: Operation
    public let stage: Stage
    public let outcome: Outcome
    public let failure: Failure?
    public let count: UInt64?
    public let durationMilliseconds: UInt64?

    public init(component: Component, operation: Operation, stage: Stage, outcome: Outcome,
                failure: Failure? = nil, count: UInt64? = nil, durationMilliseconds: UInt64? = nil,
                roundID: UUID? = nil, roundMilestone: RoundMilestone? = nil, roundReason: RoundReason? = nil) {
        self.roundID = roundID; self.roundMilestone = roundMilestone; self.roundReason = roundReason
        self.component = component; self.operation = operation; self.stage = stage; self.outcome = outcome
        self.failure = failure; self.count = count; self.durationMilliseconds = durationMilliseconds
    }
}

nonisolated public struct FieldDiagnosticEvent: Codable, Sendable, Equatable {
    public let observedAtUnixMilliseconds: Int64
    public let observation: FieldDiagnosticObservation
}

nonisolated public struct FieldDiagnosticTimeline: Codable, Sendable {
    public enum Storage: String, Codable, Sendable { case available, unavailable, corrupt, oversized, unsupportedSchema }
    public let schemaVersion: Int
    public internal(set) var events: [FieldDiagnosticEvent]
    public internal(set) var droppedCount: UInt64
    public internal(set) var expiredCount: UInt64
    public internal(set) var truncatedCount: UInt64
    public internal(set) var invalidCount: UInt64
    /// Unknown losses (e.g. an unreadable file) are never reported as zero
    /// known missing events. This flag survives subsequent successful writes.
    public internal(set) var priorHistoryUnavailable: Bool
    public internal(set) var priorHistoryUnavailableReason: Storage?
    public internal(set) var storage: Storage

    static var empty: Self {
        .init(schemaVersion: 1, events: [], droppedCount: 0, expiredCount: 0, truncatedCount: 0,
              invalidCount: 0, priorHistoryUnavailable: false, priorHistoryUnavailableReason: nil, storage: .available)
    }
}

/// One bounded inbox and at most one drain task per store, with no file work
/// or suspension on the producer. The lock protects only this small buffer.
nonisolated final class FieldDiagnosticInbox: Sendable {
    static let capacity = 64
    private struct State {
        var pending: [FieldDiagnosticEvent] = []
        var draining = false
        var dropped: UInt64 = 0
    }
    struct Batch {
        let events: [FieldDiagnosticEvent]
        let dropped: UInt64
        let finished: Bool
    }
    private let state = Mutex(State())

    func enqueue(_ event: FieldDiagnosticEvent?) -> Bool {
        state.withLock {
            if let event, $0.pending.count < Self.capacity {
                $0.pending.append(event)
            } else {
                $0.dropped = saturatedAdd($0.dropped, 1)
            }
            guard !$0.draining else { return false }
            $0.draining = true
            return true
        }
    }

    func take(finishingIfEmpty: Bool) -> Batch {
        state.withLock {
            let events = $0.pending
            $0.pending.removeAll(keepingCapacity: true)
            let finished = finishingIfEmpty && events.isEmpty
            if finished { $0.draining = false }
            return Batch(events: events, dropped: $0.dropped, finished: finished)
        }
    }
}

nonisolated func saturatedAdd(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
    let result = lhs.addingReportingOverflow(rhs)
    return result.overflow ? .max : result.partialValue
}

/// Finite representable instants only; no invented zero timestamp.
nonisolated func diagnosticMilliseconds(_ date: Date) -> Int64? {
    let value = date.timeIntervalSince1970 * 1_000
    guard value.isFinite, value >= -62_135_596_800_000, value <= 253_402_300_799_999 else { return nil }
    return Int64(value.rounded(.down))
}
