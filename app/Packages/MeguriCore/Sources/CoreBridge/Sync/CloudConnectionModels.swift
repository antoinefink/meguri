import Foundation
internal import MeguriCoreBindings

public enum CloudConnectionFailure: Sendable, Hashable {
    case closed, wrongOwner, consumed, authorityChanged, accountUnavailable, busy
    case keyUnavailable, epochIncomplete, unreadableControl, notOffered
    case incomingStagingUnavailable, mediaApplyUnavailable, resetRecoveryUnavailable
    case tracking, incomplete, fileIdentityUnavailable
    init(_ value: MeguriCoreBindings.CloudConnectionFailure) {
        self = switch value {
        case .closed: .closed; case .wrongOwner: .wrongOwner; case .consumed: .consumed
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
public enum CloudConnectionDecision: Sendable, Equatable { case noCollection, confirmUpload, confirmAdoption, chooseCollection, cloudUnreadable }
public enum CloudCountKind: Sendable, Hashable { case note, card, revlog, deck, preset, notetype, tag, config, mediaName }
public struct CloudConnectionSummary: Sendable {
    public let localCollectionPresent: Bool
    public let localCounts: [CloudCountKind: UInt64]
    public let localEmpty: Bool
    /// Counts from a manifest, labelled with publishedAtMilliseconds. Review
    /// and modification instants are unknown; these are never exact row counts.
    public let manifestCounts: [CloudCountKind: UInt64]?
    public let publishedAtMilliseconds: Int64?
    public let decision: CloudConnectionDecision
    public let unpublishedCounts: [CloudCountKind: UInt64]
    init(_ v: MeguriCoreBindings.CloudConnectionSummary) {
        func counts(_ rows: [MeguriCoreBindings.CloudKindCount]) -> [CloudCountKind: UInt64] {
            Dictionary(uniqueKeysWithValues: rows.map { row in
                let kind: CloudCountKind = switch row.kind {
                case .note: .note; case .card: .card; case .revlog: .revlog; case .deck: .deck; case .preset: .preset
                case .notetype: .notetype; case .tag: .tag; case .config: .config; case .mediaName: .mediaName
                }
                return (kind, row.count)
            })
        }
        localCollectionPresent = v.localCollectionPresent
        localCounts = counts(v.localCounts); localEmpty = v.localEmpty
        manifestCounts = v.manifestCounts.map(counts); publishedAtMilliseconds = v.publishedAtMs
        unpublishedCounts = counts(v.unpublishedCounts)
        decision = switch v.decision { case .noCollection: .noCollection; case .confirmUpload: .confirmUpload; case .confirmAdoption: .confirmAdoption; case .chooseCollection: .chooseCollection; case .cloudUnreadable: .cloudUnreadable }
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
    case zonesCreated, saved(CloudControlRecord), conflict(CloudControlRecord)
    case read(CloudControlRead), failed(retryAfterMilliseconds: Int64?), notStarted
    /// Server duration only. Rust validates the reply and persists its deadline.
    case retryAfter(durationMilliseconds: UInt64)
    var raw: MeguriCoreBindings.CloudPublicationResult {
        switch self {
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
    case read(CloudControlTarget), fetch(zone: String, cursor: Data?), validate, activate
    init(_ v: MeguriCoreBindings.CloudAdoptionAction) {
        self = switch v { case .read(let t): .read(.init(t)); case .fetch(let z, let c): .fetch(zone: z, cursor: c); case .validate: .validate; case .activate: .activate }
    }
}
public struct CloudStagingRecord: Sendable {
    public let name: String
    public let recordType: String
    public let record: CloudControlRecord
    public init(name: String, recordType: String, record: CloudControlRecord) { self.name = name; self.recordType = recordType; self.record = record }
    var raw: MeguriCoreBindings.CloudStagingRecord { .init(name: name, recordType: recordType, record: record.raw) }
}
public enum CloudAdoptionResult: Sendable {
    case read(CloudControlRead)
    case page(records: [CloudStagingRecord], deleted: [String], next: Data?)
    var raw: MeguriCoreBindings.CloudAdoptionResult {
        switch self { case .read(let r): .read(read: r.raw); case .page(let r, let d, let n): .page(records: r.map(\.raw), deleted: d, next: n) }
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
    init(_ v: MeguriCoreBindings.CloudRecoveryStatus) {
        operation = .init(raw: v.operation); switchSent = v.switchSent; switchConflict = v.switchConflict; retryAfterMilliseconds = v.retryAfterMs
        step = switch v.step { case .confirmed: .confirmed; case .bound: .bound; case .zones: .zones; case .sent: .sent; case .complete: .complete; case .cancelling: .cancelling; case .staging: .staging; case .staged: .staged; case .ready: .ready }
    }
}
