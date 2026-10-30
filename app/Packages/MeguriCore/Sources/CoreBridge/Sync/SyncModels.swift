import Foundation
internal import MeguriCoreBindings

public enum CloudPendingChange: Sendable, Equatable { case save, delete }
public enum CloudUpdateKind: Sendable, Equatable { case full, changes }
public struct CloudPendingUpdate: Sendable, Equatable {
    public let id: UInt64
    public let kind: CloudUpdateKind
    public var adds: [String: CloudPendingChange]
    public var removes: Set<String>
}
public struct CloudRecord: Sendable, Equatable {
    public let name: String
    /// K5's domain-owned encrypted `e` bytes, never an invented record field.
    public let envelope: Data
    public let systemFields: Data?
    public let recordType: String
    public let zone: String?
    /// Rust owns upload staging through the original SDK Saved/error/cancel.
    public let assetURL: URL?
    public init(name: String, envelope: Data, systemFields: Data?, recordType: String = "MGEntity", zone: String? = nil, assetURL: URL? = nil) {
        self.name = name; self.envelope = envelope; self.systemFields = systemFields
        self.recordType = recordType; self.zone = zone; self.assetURL = assetURL
    }
    init(_ raw: MeguriCoreBindings.CloudRecord) {
        self.init(name: raw.name, envelope: raw.envelope, systemFields: raw.systemFields, recordType: raw.recordType ?? "MGEntity", zone: raw.zone, assetURL: raw.assetPath.map { URL(fileURLWithPath: $0) })
    }
    var raw: MeguriCoreBindings.CloudRecord { .init(name: name, envelope: envelope, systemFields: systemFields, recordType: recordType, zone: zone, assetPath: assetURL?.path) }
}
public struct CloudEngineBatch: Sendable, Equatable {
    public let id: UInt64
    public let saves: [CloudRecord]
    public let deletes: [String]
    let mediaAdmission: CloudMediaBatchAdmission?
    init(id: UInt64, saves: [CloudRecord], deletes: [String], mediaAdmission: CloudMediaBatchAdmission? = nil) {
        self.id = id; self.saves = saves; self.deletes = deletes; self.mediaAdmission = mediaAdmission
    }
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id && lhs.saves == rhs.saves && lhs.deletes == rhs.deletes }
}
public enum CloudEngineErrorCode: Sendable, Equatable {
    case notAuthenticated, accountTemporarilyUnavailable, networkFailure, networkUnavailable
    case requestRateLimited, serviceUnavailable, zoneBusy, limitExceeded, batchRequestFailed, unknownItem, zoneNotFound, other
}
public struct CloudRecordFailure: Sendable, Equatable {
    public let name: String
    public let code: CloudEngineErrorCode
    public let zone: String?
    public let nativeCode: Int64?
    public let retryAfterMilliseconds: Int64?
    public init(name: String, code: CloudEngineErrorCode, zone: String? = nil, nativeCode: Int64? = nil, retryAfterMilliseconds: Int64? = nil) {
        self.name = name; self.code = code; self.zone = zone; self.nativeCode = nativeCode; self.retryAfterMilliseconds = retryAfterMilliseconds
    }
}
public enum CloudFetchReason: Sendable, Equatable { case manual, scheduled }
public enum CloudAccountChange: Sendable, Equatable { case signIn, signOut, switchAccounts }
public enum CloudAccountStatus: Sendable, Equatable { case available, noAccount, temporarilyUnavailable, restricted, couldNotDetermine, switchedAccounts, configuration }
public struct CloudFetchOptions: Sendable, Equatable {
    public var zones: [String]
    public var priorityZones: [String]
    public var groupID: String
    public init(zones: [String], priorityZones: [String], groupID: String) { self.zones = zones; self.priorityZones = priorityZones; self.groupID = groupID }
}
public struct CloudEngineConfiguration: Sendable, Equatable {
    public let instanceID: String; public let automaticallySync: Bool; public let serialization: Data?; public let collectionZone: String
}
public enum CloudEngineEvent: Sendable, Equatable {
    case deliveryFailed
    case databaseChanges
    case stateUpdate(Data)
    case accountChanged(change: CloudAccountChange, identity: Data?)
    case fetched(zone: String, records: [CloudRecord], deleted: [String])
    case sent(batchID: UInt64, saved: [CloudRecord], failed: [CloudRecordFailure], deleted: [String])
    case willFetch(groupID: String, reason: CloudFetchReason)
    case willFetchZone(zone: String)
    case didFetchZone(zone: String, error: CloudEngineErrorCode?)
    case didFetch(groupID: String, reason: CloudFetchReason)
    case willSend(zone: String), didSend(zone: String)
}
public protocol CloudKitEngine: Sendable {
    var instanceID: String { get }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange]
    func pendingDatabaseChanges() async -> Set<String>
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws
    func removePendingDatabaseChanges() async
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue
    func sendChanges(zones: [String]) async throws
    func fetchChanges(options: CloudFetchOptions) async throws
    func cancelOperations() async
    func release() async
}
public protocol CloudKitEngineDelegate: Sendable {
    func eventWillDeliver(engine: any CloudKitEngine) async
    func handleEvent(_ event: CloudEngineEvent, engine: any CloudKitEngine) async
    /// The SDK seam reports lifetime completion after the forwarding handler
    /// returns. The adapter never predicts this boundary by yielding or sleeping.
    func eventHandlerReturned(_ event: CloudEngineEvent, engine: any CloudKitEngine) async
    func nextRecordZoneChangeBatch(pending: [String: CloudPendingChange], zone: String, engine: any CloudKitEngine) async -> CloudEngineBatch?
    func nextFetchChangesOptions(_ options: CloudFetchOptions, engine: any CloudKitEngine) async -> CloudFetchOptions
}
public struct CloudResolvedAccount: Sendable {
    public let status: CloudAccountStatus
    public let identity: Data?
    public init(status: CloudAccountStatus, identity: Data?) { self.status = status; self.identity = identity }
}
public protocol CloudKitEngineFactory: Sendable {
    func accountChanges() async -> AsyncStream<Void>?
    func resolveAccount() async throws -> CloudResolvedAccount?
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine
}
extension CloudKitEngineFactory {
    public func accountChanges() async -> AsyncStream<Void>? { nil }
    public func resolveAccount() async throws -> CloudResolvedAccount? { nil }
}
public struct CloudGuard: Sendable, Equatable { public let generation: UInt64; public let binding: Data; public let token: UInt64 }
public struct CloudFlight: Sendable, Equatable { public let localRev: UInt64 }
public struct CloudDirtyMark: Sendable, Equatable { public let source: UInt8; public let changeSeq: UInt64; public let payloadDigest: Data }
public struct CloudEntityFacts: Sendable, Equatable {
    public let name: String; public let localRev: UInt64; public let ackedRev: UInt64
    public let mark: CloudDirtyMark?; public let inFlight: [CloudFlight]
    public let payloadDigest: Data; public let ackedPayloadDigest: Data; public let ackedSystemFields: Data
}
public struct CloudDurableSnapshot: Sendable, Equatable {
    public let binding: Data; public let serialization: Data?; public let entities: [CloudEntityFacts]
    public let trackingContent: Data
    public let inboxNames: Set<String>; public let rootEnvelope: Data?; public let rootSystemFields: Data?
    public let trackingStopped: Bool; public let absencePassCompleted: Bool
}
public struct CloudFetchDrain: Sendable, Equatable { public let zone: String; public let fetched: Bool }
public struct CloudWorkflowDiagnostics: Sendable, Equatable { public let missingDidFetchEvents: UInt64 }
public struct CloudWorkflowStatus: Sendable, Equatable { public let syncingCollection: Bool }
public enum CloudWaitingReason: Sendable, Equatable { case noAccount }
public enum CloudCollectionOutcome: Sendable, Equatable { case succeeded, waiting(CloudWaitingReason), incomplete, failed, cancelled }
public struct CloudExchangeResult: Sendable, Equatable {
    public let id: UInt64; public let collection: CloudCollectionOutcome
    public let selectedNames: Set<String>; public let ackedNames: Set<String>; public let outstandingNames: Set<String>
}
public enum CloudWorkflowObservation: Sendable, Equatable {
    case sessionEnded(session: UInt64), corePendingEmitted(kind: CloudUpdateKind, actionID: UInt64)
    case fullComparisonSkipped(actionID: UInt64), batchBuilt(names: Set<String>, batchID: UInt64)
    case pendingAdded(name: String, actionID: UInt64), handlerReturned(requestID: UInt64), savedPendingRemoved(name: String, requestID: UInt64)
    case reconcileKindCompleted(kind: UInt8, rowCount: UInt64), mediaFeedDrained(entryCount: UInt64, feedID: Data, cursor: UInt64)
}

extension CloudPendingChange {
    init(_ r: MeguriCoreBindings.CloudPendingChange) { self = r == .save ? .save : .delete }
    var raw: MeguriCoreBindings.CloudPendingChange { self == .save ? .save : .delete }
}
extension CloudUpdateKind {
    init(_ r: MeguriCoreBindings.CloudUpdateKind) { self = r == .full ? .full : .changes }
    var raw: MeguriCoreBindings.CloudUpdateKind { self == .full ? .full : .changes }
}
extension CloudPendingUpdate {
    init(_ r: MeguriCoreBindings.CloudPendingUpdate) { id = r.id; kind = .init(r.kind); adds = r.adds.mapValues(CloudPendingChange.init); removes = Set(r.removes) }
}
extension CloudEntityFacts {
    init(_ r: MeguriCoreBindings.CloudEntityFacts) {
        name = r.name; localRev = r.localRev; ackedRev = r.ackedRev
        mark = r.mark.map { .init(source: $0.source, changeSeq: $0.changeSeq, payloadDigest: $0.payloadDigest) }
        inFlight = r.inFlight.map { .init(localRev: $0.localRev) }
        payloadDigest = r.payloadDigest; ackedPayloadDigest = r.ackedPayloadDigest; ackedSystemFields = r.ackedSystemFields
    }
}
extension CloudExchangeResult {
    init(_ r: MeguriCoreBindings.CloudExchangeResult) {
        id = r.id; selectedNames = Set(r.selectedNames); ackedNames = Set(r.ackedNames); outstandingNames = Set(r.outstandingNames)
        switch r.collection { case .succeeded: collection = .succeeded; case .waitingNoAccount: collection = .waiting(.noAccount); case .incomplete: collection = .incomplete; case .failed: collection = .failed; case .cancelled: collection = .cancelled }
    }
}
extension CloudEngineErrorCode {
    var raw: MeguriCoreBindings.CloudEngineErrorCode {
        switch self {
        case .notAuthenticated: .notAuthenticated
        case .accountTemporarilyUnavailable: .accountTemporarilyUnavailable
        case .networkFailure: .networkFailure
        case .networkUnavailable: .networkUnavailable
        case .requestRateLimited: .requestRateLimited
        case .serviceUnavailable: .serviceUnavailable
        case .zoneBusy: .zoneBusy
        case .limitExceeded: .limitExceeded
        case .batchRequestFailed: .batchRequestFailed
        case .unknownItem: .unknownItem
        case .zoneNotFound: .zoneNotFound
        case .other: .other
        }
    }
}
extension CloudAccountStatus {
    var raw: MeguriCoreBindings.CloudAccountStatus {
        switch self { case .available: .available; case .noAccount: .noAccount; case .temporarilyUnavailable: .temporarilyUnavailable; case .restricted: .restricted; case .couldNotDetermine: .couldNotDetermine; case .switchedAccounts: .switchedAccounts; case .configuration: .configuration }
    }
}
extension CloudWorkflowObservation {
    init(_ r: MeguriCoreBindings.CloudWorkflowObservation) {
        switch r {
        case .sessionEnded(let session): self = .sessionEnded(session: session)
        case .corePendingEmitted(let kind, let id): self = .corePendingEmitted(kind: .init(kind), actionID: id)
        case .fullComparisonSkipped(let id): self = .fullComparisonSkipped(actionID: id)
        case .batchBuilt(let names, let id): self = .batchBuilt(names: Set(names), batchID: id)
        case .pendingAdded(let name, let id): self = .pendingAdded(name: name, actionID: id)
        case .handlerReturned(let id): self = .handlerReturned(requestID: id)
        case .savedPendingRemoved(let name, let id): self = .savedPendingRemoved(name: name, requestID: id)
        case .reconcileKindCompleted(let kind, let count): self = .reconcileKindCompleted(kind: kind, rowCount: count)
        case .mediaFeedDrained(let count, let feed, let cursor): self = .mediaFeedDrained(entryCount: count, feedID: feed, cursor: cursor)
        }
    }
}
