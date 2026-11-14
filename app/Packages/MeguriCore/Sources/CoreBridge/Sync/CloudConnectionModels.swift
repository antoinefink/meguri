import Foundation
internal import MeguriCoreBindings

public enum CloudConnectionFailure: Sendable, Hashable {
    case closed, wrongOwner, consumed, authorityChanged, accountUnavailable, accountMismatch, busy
    case keyUnavailable, epochIncomplete, unreadableControl, notOffered
    case incomingStagingUnavailable, mediaApplyUnavailable, resetRecoveryUnavailable
    case tracking, incomplete, fileIdentityUnavailable
    init(_ value: MeguriCoreBindings.CloudConnectionFailure) {
        self = switch value {
        case .closed: .closed; case .wrongOwner: .wrongOwner; case .consumed: .consumed
        case .accountMismatch: .accountMismatch
        case .authorityChanged: .authorityChanged; case .accountUnavailable: .accountUnavailable; case .busy: .busy
        case .keyUnavailable: .keyUnavailable; case .epochIncomplete: .epochIncomplete
        case .unreadableControl: .unreadableControl; case .notOffered: .notOffered
        case .incomingStagingUnavailable: .incomingStagingUnavailable; case .mediaApplyUnavailable: .mediaApplyUnavailable
        case .resetRecoveryUnavailable: .resetRecoveryUnavailable; case .tracking: .tracking
        case .incomplete: .incomplete; case .fileIdentityUnavailable: .fileIdentityUnavailable
        }
    }
}
public struct CloudControlTarget: Sendable, Equatable {
    public let zone: String
    public let name: String
    public let recordType: String
    public init(zone: String, name: String, recordType: String) { self.zone = zone; self.name = name; self.recordType = recordType }
    init(_ v: MeguriCoreBindings.CloudControlTarget) { self.init(zone: v.zone, name: v.name, recordType: v.recordType) }
}
/// Opaque encrypted transport bytes. Neither this nor system fields is a UI
/// model or confirmation authority. No Codable or diagnostic description.
public struct CloudControlRecord: Sendable, Equatable {
    public let encrypted: Data?
    public let systemFields: Data
    public init(encrypted: Data?, systemFields: Data) { self.encrypted = encrypted; self.systemFields = systemFields }
    var raw: MeguriCoreBindings.CloudControlRecord { .init(encrypted: encrypted, systemFields: systemFields) }
}
public enum CloudControlRead: Sendable, Equatable {
    case found(CloudControlRecord), missing, zoneMissing, failed
    var raw: MeguriCoreBindings.CloudControlRead {
        switch self { case .found(let r): .found(record: r.raw); case .missing: .missing; case .zoneMissing: .zoneMissing; case .failed: .failed }
    }
}
public enum CloudConnectionDecision: Sendable, Equatable { case noCollection, confirmUpload, confirmAdoption, confirmResume, chooseCollection, cloudUnreadable }
public enum CloudCountKind: Sendable, Hashable { case note, card, revlog, deck, preset, notetype, tag, config, mediaName }
public struct CloudConnectionSummary: Sendable {
    public let localCollectionPresent: Bool
    public let initialAdoptionRequired: Bool
    public let localCounts: [CloudCountKind: UInt64]
    public let localEmpty: Bool
    /// Counts from a manifest, labelled with publishedAtMilliseconds. Review
    /// and modification instants are unknown; these are never exact row counts.
    public let manifestCounts: [CloudCountKind: UInt64]?
    public let publishedAtMilliseconds: Int64?
    public let decision: CloudConnectionDecision
    public let unpublishedCounts: [CloudCountKind: UInt64]
    /// Informational only. Confirmation always passes the original opaque preview.
    public let resumeBindingId: Data?
    public let unsentCounts: [CloudCountKind: UInt64]
    /// When false, unsentCounts and unsentChangeCount are only lower bounds.
    public let unsentCountsExact: Bool
    public let fetchedUnverifiedCount: UInt64
    public let heldCount: UInt64
    public let isolatedUnsentCount: UInt64
    public let conflictCopyCount: UInt64
    public var unsentChangeCount: UInt64 {
        unsentCounts.values.reduce(0) { total, count in
            let sum = total.addingReportingOverflow(count)
            return sum.overflow ? .max : sum.partialValue
        }
    }
    init(_ v: MeguriCoreBindings.CloudConnectionSummary) {
        localCollectionPresent = v.localCollectionPresent
        initialAdoptionRequired = v.initialAdoptionRequired
        localCounts = cloudKindCounts(v.localCounts); localEmpty = v.localEmpty
        manifestCounts = v.manifestCounts.map(cloudKindCounts); publishedAtMilliseconds = v.publishedAtMs
        unpublishedCounts = cloudKindCounts(v.unpublishedCounts)
        resumeBindingId = v.resumeBindingId
        unsentCounts = cloudKindCounts(v.unsentCounts)
        unsentCountsExact = v.unsentCountsExact
        fetchedUnverifiedCount = v.fetchedUnverifiedCount
        heldCount = v.heldCount
        isolatedUnsentCount = v.isolatedUnsentCount
        conflictCopyCount = v.conflictCopyCount
        decision = switch v.decision { case .noCollection: .noCollection; case .confirmUpload: .confirmUpload; case .confirmAdoption: .confirmAdoption; case .confirmResume: .confirmResume; case .chooseCollection: .chooseCollection; case .cloudUnreadable: .cloudUnreadable }
    }
}
public struct CloudConnectionPreview: Sendable { let raw: MeguriCoreBindings.CloudConnectionPreview }
public enum CloudConnectionPreviewWork: Sendable {
    case readActive(CloudConnectionPreview, CloudControlTarget)
    case readEpoch(CloudConnectionPreview, CloudControlTarget)
    case ready(CloudConnectionPreview, CloudConnectionSummary)
    init(_ v: MeguriCoreBindings.CloudConnectionPreviewWork) {
        self = switch v {
        case .readActive(let p, let t): .readActive(.init(raw: p), .init(t))
        case .readEpoch(let p, let t): .readEpoch(.init(raw: p), .init(t))
        case .ready(let p, let s): .ready(.init(raw: p), .init(s))
        }
    }
}
public struct CloudRecoveryOperation: Sendable { let raw: MeguriCoreBindings.CloudRecoveryOperation }
public struct CloudPublicationLocal: Sendable { let raw: MeguriCoreBindings.CloudPublicationLocal }
public struct CloudPublicationRequest: Sendable { let raw: MeguriCoreBindings.CloudPublicationRequest }
public struct CloudAdoptionWork: Sendable { let raw: MeguriCoreBindings.CloudAdoptionWork }
public enum CloudPublicationLocalKind: Sendable { case bind, writeDecision, cancel }
public enum CloudOwnKind: Sendable { case createZones, saveMediaRoot, savePublishingEpoch, saveCompleteEpoch, readOwnEpoch, saveActive, readResetMediaRoot }
public struct CloudOwnMetadata: Sendable {
    public let operationID: Data
    public let attempt: UInt64
    public let kind: CloudOwnKind
    init(operationID: Data, attempt: UInt64, kind: CloudOwnKind) {
        self.operationID = operationID; self.attempt = attempt; self.kind = kind
    }
    init(_ v: MeguriCoreBindings.CloudOwnMetadata) {
        operationID = v.operationId; attempt = v.attempt
        kind = switch v.kind { case .createZones: .createZones; case .saveMediaRoot: .saveMediaRoot; case .savePublishingEpoch: .savePublishingEpoch; case .saveCompleteEpoch: .saveCompleteEpoch; case .readOwnEpoch: .readOwnEpoch; case .saveActive: .saveActive; case .readResetMediaRoot: .readResetMediaRoot }
    }
}
public enum CloudOwnOperation: Sendable {
    case createZones([String])
    case read(CloudControlTarget)
    case save(CloudControlTarget, encrypted: Data, systemFields: Data?)
    init(_ v: MeguriCoreBindings.CloudOwnOperation) {
        self = switch v {
        case .createZones(let z): .createZones(z)
        case .read(let t): .read(.init(t))
        case .save(let t, let e, let s): .save(.init(t), encrypted: e, systemFields: s)
        }
    }
}
public enum CloudPublicationResult: Sendable {
    case zoneLosses([CloudZoneLoss])
    case zonesCreated, saved(CloudControlRecord), conflict(CloudControlRecord)
    case read(CloudControlRead), failed(retryAfterMilliseconds: Int64?), notStarted
    /// Server duration only. Rust validates the reply and persists its deadline.
    case retryAfter(durationMilliseconds: UInt64)
    var raw: MeguriCoreBindings.CloudPublicationResult {
        switch self {
        case .zoneLosses(let losses): .zoneLosses(losses: losses.map(\.raw))
        case .zonesCreated: .zonesCreated
        case .saved(let r): .saved(record: r.raw)
        case .conflict(let r): .conflict(record: r.raw)
        case .read(let r): .read(read: r.raw)
        case .failed(let t): .failed(retryAfterMs: t)
        case .retryAfter(let duration): .retryAfter(durationMs: duration)
        case .notStarted: .notStarted
        }
    }
}
public enum CloudAdoptionAction: Sendable {
    case readActive(CloudControlTarget, expected: CloudControlRecord)
    case read(CloudControlTarget), fetch(zone: String, cursor: Data?, expectedEpoch: CloudControlRecord), fetchPayload(zone: String, name: String, expected: CloudControlRecord), validate, activate
    init(_ v: MeguriCoreBindings.CloudAdoptionAction) {
        self = switch v { case .readActive(let t, let e): .readActive(.init(t), expected: .init(encrypted: e.encrypted, systemFields: e.systemFields)); case .read(let t): .read(.init(t)); case .fetch(let z, let c, let e): .fetch(zone: z, cursor: c, expectedEpoch: .init(encrypted: e.encrypted, systemFields: e.systemFields)); case .fetchPayload(let z, let n, let e): .fetchPayload(zone: z, name: n, expected: .init(encrypted: e.encrypted, systemFields: e.systemFields)); case .validate: .validate; case .activate: .activate }
    }
}
public struct CloudStagingRecord: Sendable {
    public let name: String
    public let recordType: String
    public let record: CloudControlRecord
    public let payloadFile: URL?
    // The URL is only a transport projection. This lease owns the bytes until
    // the awaited Rust intake returns, including its queued maintenance call.
    private let payloadLease: CloudMediaStagedAsset?
    public init(name: String, recordType: String, record: CloudControlRecord, payloadFile: URL? = nil) {
        self.name = name; self.recordType = recordType; self.record = record
        self.payloadFile = payloadFile; payloadLease = nil
    }
    init(name: String, recordType: String, record: CloudControlRecord, payloadLease: CloudMediaStagedAsset?) {
        self.name = name; self.recordType = recordType; self.record = record
        self.payloadLease = payloadLease; payloadFile = payloadLease?.url
    }
    var raw: MeguriCoreBindings.CloudStagingRecord { .init(name: name, recordType: recordType, record: record.raw, payloadFile: payloadFile?.path) }
}
public enum CloudAdoptionResult: Sendable {
    case failed
    case transportFailed(code: Int64, retryAfterDurationMilliseconds: UInt64?)
    case payload(CloudStagingRecord)
    case read(CloudControlRead)
    case page(records: [CloudStagingRecord], deleted: [String], next: Data?)
    var raw: MeguriCoreBindings.CloudAdoptionResult {
        switch self { case .failed: .failed; case .transportFailed(let code, let delay): .transportFailed(code: code, retryAfterDurationMs: delay); case .payload(let r): .payload(record: r.raw); case .read(let r): .read(read: r.raw); case .page(let r, let d, let n): .page(records: r.map(\.raw), deleted: d, next: n) }
    }
    /// Keep the concrete staging projection alive across the awaited lane.
    /// Capturing raw paths alone would allow ARC to release native file leases.
    func withRaw<T: Sendable>(_ body: @Sendable (MeguriCoreBindings.CloudAdoptionResult) async throws -> T) async rethrows -> T {
        defer { withExtendedLifetime(self) {} }
        return try await body(raw)
    }
}
public enum CloudPublicationWork: Sendable {
    case local(CloudPublicationLocal, CloudPublicationLocalKind, CloudRecoveryOperation)
    case own(CloudPublicationRequest, CloudOwnOperation, CloudRecoveryOperation, CloudOwnMetadata)
    case adoption(CloudAdoptionWork, CloudAdoptionAction, CloudRecoveryOperation)
    case scan(CloudPublicationLocal), sendEntities, idle
    init(_ v: MeguriCoreBindings.CloudPublicationWork) {
        switch v {
        case .local(let h, let k, let o):
            let kind: CloudPublicationLocalKind = switch k { case .bind: .bind; case .writeDecision: .writeDecision; case .cancel: .cancel }
            self = .local(.init(raw: h), kind, .init(raw: o))
        case .own(let r, let c, let o, let m): self = .own(.init(raw: r), .init(c), .init(raw: o), .init(m))
        case .adoption(let h, let a, let o): self = .adoption(.init(raw: h), .init(a), .init(raw: o))
        case .scan(let h): self = .scan(.init(raw: h)); case .sendEntities: self = .sendEntities; case .idle: self = .idle
        }
    }
}
public enum CloudRecoveryStep: Sendable, Equatable { case confirmed, bound, zones, sent, complete, cancelling, staging, staged, ready }
public struct CloudRecoveryStatus: Sendable {
    public let operation: CloudRecoveryOperation
    public let step: CloudRecoveryStep
    public let switchSent: Bool
    public let switchConflict: Bool
    public let retryAfterMilliseconds: Int64?
    public let adoptionTransportWait: Bool
    init(_ v: MeguriCoreBindings.CloudRecoveryStatus) {
        adoptionTransportWait = v.adoptionTransportWait
        operation = .init(raw: v.operation); switchSent = v.switchSent; switchConflict = v.switchConflict; retryAfterMilliseconds = v.retryAfterMs
        step = switch v.step { case .confirmed: .confirmed; case .bound: .bound; case .zones: .zones; case .sent: .sent; case .complete: .complete; case .cancelling: .cancelling; case .staging: .staging; case .staged: .staged; case .ready: .ready }
    }
}

#if DEBUG
/// Frozen content-free failure from the exact drained adoption result.
struct CloudAdoptionFailureDiagnostic: Sendable, Equatable {
    enum Task: String, Sendable {
        case readActive
        case readEpoch
        case readMediaRoot
        case fetch
        case fetchPayload
        case validate
        case recheckActive
        case activate
        init(_ value: MeguriCoreBindings.CloudAdoptionDiagnosticTask) {
            self = switch value {
            case .readActive: .readActive
            case .readEpoch: .readEpoch
            case .readMediaRoot: .readMediaRoot
            case .fetch: .fetch
            case .fetchPayload: .fetchPayload
            case .validate: .validate
            case .recheckActive: .recheckActive
            case .activate: .activate
            }
        }
    }
    enum Stage: String, Sendable {
        case operation
        case owner
        case claim
        case authority
        case activeValidation
        case epochShape
        case epochValidation
        case epochWrite
        case mediaRootShape
        case mediaRootValidation
        case mediaRootWrite
        case mediaRootEpoch
        case mediaRootStageCreate
        case mediaRootSpace
        case pageValidation
        case pageRecordValidation
        case pageEpochShape
        case pageEpochData
        case pageEpochSystemFields
        case pageSpace
        case payloadValidation
        case payloadRead
        case payloadSpace
        case payloadWrite
        case receivedValidation
        case recordEncode
        case recordSpace
        case recordWrite
        case rowStage
        case finalSpace
        case pageAdvance
        case pageFinish
        case nextAuthority
        case transport
        init(_ value: MeguriCoreBindings.CloudAdoptionDiagnosticStage) {
            self = switch value {
            case .operation: .operation
            case .owner: .owner
            case .claim: .claim
            case .authority: .authority
            case .activeValidation: .activeValidation
            case .epochShape: .epochShape
            case .epochValidation: .epochValidation
            case .epochWrite: .epochWrite
            case .mediaRootShape: .mediaRootShape
            case .mediaRootValidation: .mediaRootValidation
            case .mediaRootWrite: .mediaRootWrite
            case .mediaRootEpoch: .mediaRootEpoch
            case .mediaRootStageCreate: .mediaRootStageCreate
            case .mediaRootSpace: .mediaRootSpace
            case .pageValidation: .pageValidation
            case .pageRecordValidation: .pageRecordValidation
            case .pageEpochShape: .pageEpochShape
            case .pageEpochData: .pageEpochData
            case .pageEpochSystemFields: .pageEpochSystemFields
            case .pageSpace: .pageSpace
            case .payloadValidation: .payloadValidation
            case .payloadRead: .payloadRead
            case .payloadSpace: .payloadSpace
            case .payloadWrite: .payloadWrite
            case .receivedValidation: .receivedValidation
            case .recordEncode: .recordEncode
            case .recordSpace: .recordSpace
            case .recordWrite: .recordWrite
            case .rowStage: .rowStage
            case .finalSpace: .finalSpace
            case .pageAdvance: .pageAdvance
            case .pageFinish: .pageFinish
            case .nextAuthority: .nextAuthority
            case .transport: .transport
            }
        }
    }
    enum Category: String, Sendable {
        case validation
        case tracking
        case engine
        case cancelled
        case authorityChanged
        case accountUnavailable
        case publication
        case incomplete
        case fileIdentityUnavailable
        case transport
        init(_ value: MeguriCoreBindings.CloudAdoptionFailureCategory) {
            self = switch value {
            case .validation: .validation
            case .tracking: .tracking
            case .engine: .engine
            case .cancelled: .cancelled
            case .authorityChanged: .authorityChanged
            case .accountUnavailable: .accountUnavailable
            case .publication: .publication
            case .incomplete: .incomplete
            case .fileIdentityUnavailable: .fileIdentityUnavailable
            case .transport: .transport
            }
        }
    }
    let task: Task
    let lastEnteredStage: Stage
    let primaryCategory: Category?
    let cleanupCategory: Category?

    init(task: Task, lastEnteredStage: Stage, primaryCategory: Category?, cleanupCategory: Category?) {
        self.task = task; self.lastEnteredStage = lastEnteredStage
        self.primaryCategory = primaryCategory; self.cleanupCategory = cleanupCategory
    }
    init(_ value: MeguriCoreBindings.CloudAdoptionFailureDiagnostic) {
        self.init(task: Task(value.task), lastEnteredStage: Stage(value.lastEnteredStage),
                  primaryCategory: value.primaryCategory.map(Category.init),
                  cleanupCategory: value.cleanupCategory.map(Category.init))
    }
}
#endif
