import CoreBridge

/// Projects only accepted owner status. Cursors and source stamps are private
/// deduplication state; they are never exported as observations or receipts.
struct FieldDiagnosticCloudStatus {
    private enum Kind: Equatable {
        case localOnly, attention, offline, collection, media, pending, current, awaiting
        case unavailable(CloudPolicyUnavailableReason), paused(CloudPolicyPausedReason)
    }
    private struct Status: Equatable {
        let kind: Kind
        let pending: UInt64
        let held: UInt64
        let inFlight: UInt64
        let incoming: UInt64
        let attention: [CloudPolicyAttention: UInt64]
        let error: CloudPolicyOperationError?
        let account: CloudPolicyAccountStatus
        let matches: Bool
        let converged: Bool
    }
    private struct Media: Equatable {
        let facts: CloudPolicyMediaFacts
        let paused: Bool
    }
    private struct Attempt: Equatable {
        let exchange: UInt64
        let ended: Int64
    }
    private var owner: (generation: UInt64, revision: UInt64)?
    private var status: Status?
    private var media: Media?
    private var attempt: Attempt?

    mutating func observe(_ value: CloudPolicyStatus, generation: UInt64, revision: UInt64,
                          record: FieldDiagnosticRecorder) {
        if owner?.generation != generation || owner?.revision != revision {
            owner = (generation, revision)
            status = nil; media = nil; attempt = nil
        }
        let kind: Kind
        let outcome: FieldDiagnosticObservation.Outcome
        switch value.state {
        case .localOnly: kind = .localOnly; outcome = .observed
        case .iCloudUnavailable(let reason): kind = .unavailable(reason); outcome = .waiting
        case .paused(let reason): kind = .paused(reason); outcome = .waiting
        case .needsAttention: kind = .attention; outcome = .incomplete
        case .offline: kind = .offline; outcome = .waiting
        case .syncingCollection: kind = .collection; outcome = .waiting
        case .syncingMedia: kind = .media; outcome = .waiting
        case .changesPending: kind = .pending; outcome = .waiting
        case .upToDate: kind = .current; outcome = .observed
        case .awaitingEvidence: kind = .awaiting; outcome = .unknown
        }
        // P14's qualified status supplies convergence. Exchange time, a success
        // result, or a display string cannot supply it.
        let converged = kind == .current && value.asOfMilliseconds != nil
        let next = Status(kind: kind, pending: value.pending, held: value.heldBack, inFlight: value.inFlight,
            incoming: value.incomingWaiting, attention: value.attention, error: value.error?.error,
            account: value.accountStatus, matches: value.accountMatchesBinding, converged: converged)
        if status != next {
            record(.init(component: .cloud, operation: .status, stage: .statusObserved, outcome: outcome,
                failure: value.error.map { Self.failure($0.error) }, count: value.pending))
            if converged, status?.converged != true {
                record(.init(component: .cloud, operation: .status, stage: .convergenceObserved, outcome: .observed))
            }
            status = next
        }
        let nextMedia = Media(facts: value.media, paused: value.mediaPaused)
        if media != nextMedia {
            let facts = value.media
            let failure: FieldDiagnosticObservation.Failure? = facts.noSpace == true ? .noSpace
                : facts.zoneMissing == true ? .unavailable : nil
            let incomplete = (facts.missingNames ?? 0) > 0 || (facts.unknownBlobs ?? 0) > 0
            let running = (facts.uploadsRunning ?? 0) > 0 || (facts.downloadsRunning ?? 0) > 0
            record(.init(component: .media, operation: .transfer, stage: .statusObserved,
                outcome: failure != nil ? .failed : incomplete ? .incomplete : running || value.mediaPaused ? .waiting : .observed,
                failure: failure, count: facts.missingNames))
            media = nextMedia
        }
        if let last = value.lastAttempt {
            let cursor = Attempt(exchange: last.exchange, ended: last.endedMilliseconds)
            if cursor != attempt {
                attempt = cursor
                // History may load late or replay. Even collection activity
                // cannot establish that this field just completed an exchange.
                record(.init(component: .cloud, operation: .exchange, stage: .statusObserved, outcome: .unknown))
            }
        }
    }

    /// P12.7 emits RecordExchange once per ExchangeEnded. The caller observes
    /// its existing successful native admission, never a status-history delta.
    static func completed(_ exchange: CloudPolicyExchangeRecord, record: FieldDiagnosticRecorder) {
        let outcome: FieldDiagnosticObservation.Outcome
        let failure: FieldDiagnosticObservation.Failure?
        switch exchange.outcome {
        case .succeeded: outcome = .succeeded; failure = nil
        case .incomplete: outcome = .incomplete; failure = nil
        case .waiting(let error, _): outcome = .waiting; failure = error.map(Self.failure)
        case .failed(let error): outcome = .failed; failure = Self.failure(error)
        case .cancelled: outcome = .cancelled; failure = nil
        }
        record(.init(component: .cloud, operation: .exchange, stage: .exchangeCompleted,
            outcome: outcome, failure: failure))
    }

    private static func failure(_ error: CloudPolicyOperationError) -> FieldDiagnosticObservation.Failure {
        switch error {
        case .noAccount: .notAuthenticated
        case .configuration: .unavailable
        case .offline: .transport
        case .throttled: .rateLimited
        case .quotaFull, .localStorageFull: .noSpace
        case .trackingError, .conflictRecovery, .unsupportedSchema, .other: .other
        }
    }
}
