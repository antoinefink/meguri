internal import MeguriCoreBindings

/// Owned safe scalars only. Generated records and legacy zone-bearing history
/// remain internal to CoreBridge; export never hydrates that history.
public struct CloudDiagnosticFailure: Sendable, Encodable, Equatable {
    public let code: Int64?
    public let retryAfterMilliseconds: Int64?
    public let encryptedDataReset: Bool
    public init(code: Int64? = nil, retryAfterMilliseconds: Int64? = nil, encryptedDataReset: Bool = false) {
        self.code = code; self.retryAfterMilliseconds = retryAfterMilliseconds; self.encryptedDataReset = encryptedDataReset
    }
    init(_ value: MeguriCoreBindings.CloudZoneDeletionFailure) {
        self.init(code: value.code, retryAfterMilliseconds: value.retryAfterMs, encryptedDataReset: value.encryptedDataReset)
    }
    init(_ value: CloudZoneDeletionFailure) {
        self.init(code: value.code, retryAfterMilliseconds: value.retryAfterMilliseconds, encryptedDataReset: value.encryptedDataReset)
    }
}
public struct CloudDiagnosticRootRead: Sendable, Encodable {
    public enum Outcome: String, Sendable, Encodable { case found, unknownItem, zoneNotFound, failed, other }
    public let active: Bool
    public let lostCollection: Bool
    public let outcome: Outcome
    public let a7Failure: Bool
    public let accepted: Bool
    public let failure: CloudDiagnosticFailure?
    public let terminal: CloudDiagnosticFailure?
    public init(active: Bool, lostCollection: Bool, outcome: Outcome, a7Failure: Bool, accepted: Bool,
                failure: CloudDiagnosticFailure? = nil, terminal: CloudDiagnosticFailure? = nil) {
        self.active = active; self.lostCollection = lostCollection; self.outcome = outcome
        self.a7Failure = a7Failure; self.accepted = accepted; self.failure = failure; self.terminal = terminal
    }
    init(_ value: MeguriCoreBindings.CloudDiagnosticRootRead) {
        let outcome: Outcome
        switch value.outcome {
        case .found: outcome = .found
        case .unknownItem: outcome = .unknownItem
        case .zoneNotFound: outcome = .zoneNotFound
        case .failed: outcome = .failed
        case .other: outcome = .other
        }
        self.init(active: value.active, lostCollection: value.lostCollection, outcome: outcome,
                  a7Failure: value.a7Failure, accepted: value.accepted,
                  failure: value.failure.map(CloudDiagnosticFailure.init), terminal: value.terminal.map(CloudDiagnosticFailure.init))
    }
}
public struct CloudDiagnosticResult: Sendable, Encodable {
    public let failure: CloudDiagnosticFailure?
    public init(failure: CloudDiagnosticFailure?) { self.failure = failure }
}
public struct CloudDiagnosticDeletion: Sendable, Encodable {
    public enum Kind: String, Sendable, Encodable { case read, deleted, localFailure }
    public let kind: Kind
    public let found: Bool?
    public let allowed: UInt64?
    public let failure: CloudDiagnosticFailure?
    public let terminal: CloudDiagnosticFailure?
    public let results: [CloudDiagnosticResult]?
    public let resultsOmittedCount: UInt64?
    public let category: CloudZoneDeletionLocalFailure?
    public init(kind: Kind, found: Bool? = nil, allowed: UInt64? = nil, failure: CloudDiagnosticFailure? = nil,
                terminal: CloudDiagnosticFailure? = nil, results: [CloudDiagnosticResult]? = nil,
                resultsOmittedCount: UInt64? = nil, category: CloudZoneDeletionLocalFailure? = nil) {
        self.kind = kind; self.found = found; self.allowed = allowed; self.failure = failure; self.terminal = terminal
        self.results = results.map { Array($0.prefix(64)) }
        self.resultsOmittedCount = results.map {
            let sum = (resultsOmittedCount ?? 0).addingReportingOverflow(UInt64(max(0, $0.count - 64)))
            return sum.overflow ? .max : sum.partialValue
        }
        self.category = category
    }
    init(_ value: MeguriCoreBindings.CloudDiagnosticDeletion) {
        switch value {
        case .read(let found, let allowed, let failure, let terminal):
            self.init(kind: .read, found: found, allowed: allowed, failure: failure.map(CloudDiagnosticFailure.init), terminal: terminal.map(CloudDiagnosticFailure.init))
        case .deleted(let results, let omitted, let terminal):
            self.init(kind: .deleted, terminal: terminal.map(CloudDiagnosticFailure.init),
                      results: results.map { .init(failure: $0.failure.map(CloudDiagnosticFailure.init)) }, resultsOmittedCount: omitted)
        }
    }
}
/// Observed Runtime flags only. No recomputed policy or inferred Unknown cause.
public struct CloudDiagnosticStatusEvidence: Sendable, Encodable {
    public let scanRequired: Bool
    public let scanRunning: Bool
    public let wholeScanComplete: Bool
    public let capturePresent: Bool
    public let noteScanPresent: Bool
    public let creationTimeKnown: Bool
    public let creationTimeMatches: Bool
    public let trackingStopped: Bool
    public let attentionEvidenceUnavailable = true
    public let incomingEvidenceUnavailable = true
    init(_ value: MeguriCoreBindings.CloudDiagnosticStatusEvidence) {
        scanRequired = value.scanRequired; scanRunning = value.scanRunning; wholeScanComplete = value.wholeScanComplete
        capturePresent = value.capturePresent; noteScanPresent = value.noteScanPresent
        creationTimeKnown = value.creationTimeKnown; creationTimeMatches = value.creationTimeMatches; trackingStopped = value.trackingStopped
    }
}
public struct CloudDiagnosticsExport: Sendable {
    public let available: Bool
    public let missingDidFetchEvents: UInt64?
    public let rootReads: [CloudDiagnosticRootRead]
    public let rootReadsOmittedCount: UInt64
    public let zoneDeletions: [CloudDiagnosticDeletion]
    public let zoneDeletionsOmittedCount: UInt64
    public let statusEvidence: CloudDiagnosticStatusEvidence?
    public static let unavailable = CloudDiagnosticsExport(available: false)

    public init(available: Bool, missingDidFetchEvents: UInt64? = nil, rootReads: [CloudDiagnosticRootRead] = [],
                rootReadsOmittedCount: UInt64 = 0, zoneDeletions: [CloudDiagnosticDeletion] = [],
                zoneDeletionsOmittedCount: UInt64 = 0, statusEvidence: CloudDiagnosticStatusEvidence? = nil) {
        self.available = available; self.missingDidFetchEvents = missingDidFetchEvents
        self.rootReads = Array(rootReads.suffix(64)); self.zoneDeletions = Array(zoneDeletions.suffix(128))
        let reads = rootReadsOmittedCount.addingReportingOverflow(UInt64(max(0, rootReads.count - 64)))
        let deletions = zoneDeletionsOmittedCount.addingReportingOverflow(UInt64(max(0, zoneDeletions.count - 128)))
        self.rootReadsOmittedCount = reads.overflow ? .max : reads.partialValue
        self.zoneDeletionsOmittedCount = deletions.overflow ? .max : deletions.partialValue
        self.statusEvidence = statusEvidence
    }
    init(_ value: MeguriCoreBindings.CloudDiagnosticSnapshot?, localFailures: [CloudZoneDeletionDiagnostic]) {
        let local: [CloudDiagnosticDeletion] = localFailures.compactMap {
            guard case .localFailure(let category, let failure) = $0 else { return nil }
            return .init(kind: .localFailure, failure: .init(failure), category: category)
        }
        // The Rust snapshot is already bounded before FFI and this mapping.
        self.init(available: value != nil, missingDidFetchEvents: value?.missingDidFetchEvents,
                  rootReads: value?.rootReads.map(CloudDiagnosticRootRead.init) ?? [], rootReadsOmittedCount: value?.rootReadsOmittedCount ?? 0,
                  zoneDeletions: (value?.zoneDeletions.map(CloudDiagnosticDeletion.init) ?? []) + local,
                  zoneDeletionsOmittedCount: value?.zoneDeletionsOmittedCount ?? 0,
                  statusEvidence: value?.statusEvidence.map(CloudDiagnosticStatusEvidence.init))
    }
}
