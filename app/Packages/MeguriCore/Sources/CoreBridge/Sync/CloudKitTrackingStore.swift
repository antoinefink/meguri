import Foundation
import Instrumentation
internal import MeguriCoreBindings

struct CloudRegistration: Sendable {
    let instanceID: String
    let serialization: Data?
    let collectionZone: String
    let needsRootFetch: Bool
}

/// A retained handle to the host-owned Rust tracking store. There is no Swift
/// database and no reconstruction of domain outcomes from SDK return values.
public actor CoreCloudWorkflowService {
    private var creation: Task<MeguriCoreBindings.CloudWorkflow, any Error>?
    public init() {}
    private func owner() async throws -> MeguriCoreBindings.CloudWorkflow {
        if let creation { return try await creation.value }
        let task = Task { try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() } }
        creation = task
        return try await task.value
    }
    public func policy() async throws -> CoreCloudPolicyService {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { [raw] in CoreCloudPolicyService(try raw.policy()) }
    }
    public func connection() async throws -> CoreCloudConnectionService {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowConnection) { CoreCloudConnectionService(try raw.connection()) }
    }
    public func media() async throws -> CoreCloudMediaService {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowMedia) { CoreCloudMediaService(try raw.media()) }
    }
    /// Begin only after the original SDK handler has returned.
    public func beginFirstAssetReadPass(sdk: String) async throws -> CoreCloudAssetReadPass {
        let raw = try await owner()
        let pass: MeguriCoreBindings.CloudAssetReadPass = try await CoreCloudPolicyService.runOrdered(
            .cloudWorkflowStartBeginFirstAssetReadPass, result: .cloudPolicyAssetReadPassResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw, sdk] in
            let started = try raw.startBeginFirstAssetReadPass(sdk: sdk)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CoreCloudAssetReadPass(pass)
    }
    func submitMediaBatch(sdk: String, batch: UInt64, queue: any CloudMediaQueue) async throws -> Bool {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowSubmitMediaBatch) { try raw.submitMediaBatch(sdk: sdk, batch: batch, queue: ForeignMediaQueue(queue)) }
    }
    public func instance() async throws -> String {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowInstance) { try raw.instance() }
    }
    public func currentGuard() async throws -> CloudGuard {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowCurrentGuard) { let r = try raw.currentGuard(); return .init(generation: r.generation, binding: r.binding, token: r.token) }
    }
    public func entityFacts(name: String) async throws -> CloudEntityFacts? {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowEntityFacts) { try raw.entityFacts(name: name).map(CloudEntityFacts.init) }
    }
    public func awaitTracking(generation: UInt64, seq: UInt64) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowAwaitTracking) { try raw.awaitTracking(generation: generation, seq: seq) }
    }
    public func trackingSendables() async throws -> [String: CloudPendingChange] {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowTrackingSendables) { try raw.trackingSendables().mapValues(CloudPendingChange.init) }
    }
    public func pendingTransition(instance: String, kind: CloudUpdateKind, snapshot: [String: CloudPendingChange]) async throws -> CloudPendingUpdate {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowPendingTransition) { CloudPendingUpdate(try raw.pendingTransition(instance: instance, kind: kind.raw, snapshot: snapshot.mapValues(\.raw))) }
    }
    public func emittedUpdateIDs() async throws -> [UInt64] {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowEmittedUpdateIds) { try raw.emittedUpdateIds() }
    }
    public func observations() async throws -> [CloudWorkflowObservation] {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowObservations) { try raw.observations().map(CloudWorkflowObservation.init) }
    }
    public func pollReview() async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowPollReview) { try raw.pollReview() }
    }
    public func setPaused(_ paused: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowSetPaused) { try raw.setPaused(paused: paused) }
    }
    public func stopSyncing() async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowStopSyncing) { try raw.stopSyncing() }
    }
    public func durableSnapshot() async throws -> CloudDurableSnapshot {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowDurableSnapshot) { let r = try raw.durableSnapshot(); return .init(binding: r.binding, serialization: r.serialization, entities: r.entities.map(CloudEntityFacts.init), trackingContent: r.trackingContent, inboxNames: Set(r.inboxNames), rootEnvelope: r.rootEnvelope, rootSystemFields: r.rootSystemFields, trackingStopped: r.trackingStopped, absencePassCompleted: r.absencePassCompleted) }
    }
    public func fetchDrains() async throws -> [CloudFetchDrain] {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFetchDrains) { try raw.fetchDrains().map { .init(zone: $0.zone, fetched: $0.fetched) } }
    }
    public func diagnostics() async throws -> CloudWorkflowDiagnostics {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowDiagnostics) {
            let value = try raw.diagnostics()
            return CloudWorkflowDiagnostics(missingDidFetchEvents: value.missingDidFetchEvents, rootReads: value.rootReads.map(CloudRootReadDiagnostic.init), zoneDeletions: value.zoneDeletions.map(CloudZoneDeletionDiagnostic.init))
        }
    }
    public func status() async throws -> CloudWorkflowStatus {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowStatus) { CloudWorkflowStatus(syncingCollection: try raw.status().syncingCollection) }
    }
    public func exchangeResult(id: UInt64) async throws -> CloudExchangeResult? {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowExchangeResult) { try raw.exchangeResult(id: id).map(CloudExchangeResult.init) }
    }
    public func beginIdentityRead(transition: Bool = false, originSDK: String? = nil) async throws -> UInt64 {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowBeginIdentityRead) { try raw.beginIdentityRead(transition: transition, originSdk: originSDK) }
    }
    public func completeIdentityRead(id: UInt64, status: CloudAccountStatus, identity: Data?) async throws -> Bool {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowCompleteIdentityRead) { try raw.completeIdentityRead(id: id, status: status.raw, value: identity) }
    }
     func prepareRuntime() async throws {
        let raw = try await owner()
        try await MaintenanceLane.shared.call(.cloudWorkflowPrepareRuntime) { try raw.prepareRuntime() }
    }
    func registerSDK(automatic: Bool) async throws -> CloudRegistration {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowRegisterSdk) { let r = try raw.registerSdk(automatic: automatic); return .init(instanceID: r.instanceId, serialization: r.serialization, collectionZone: r.collectionZone, needsRootFetch: r.needsRootFetch) }
    }
     func validateSDK(_ sdk: String) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowValidateSdk) { try raw.validateSdk(sdk: sdk) }
    }
    func finalSDKAuthorization(_ sdk: String) async throws -> @Sendable () throws -> Void {
        let raw = try await owner()
        return { try cloudFinalCoreCall(.cloudWorkflowValidateSdk) { try raw.validateSdk(sdk: sdk) } }
    }
     func retireSDK(_ sdk: String, policyStop: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowRetireSdk) { try raw.retireSdk(sdk: sdk, policyStop: policyStop) }
    }
     func releaseSDK(_ sdk: String) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowReleaseSdk) { try raw.releaseSdk(sdk: sdk) }
    }
     func accountChanged(sdk: String?, status: CloudAccountStatus, identity: Data?) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowAccountChanged) { try raw.accountChanged(sdk: sdk, status: status.raw, value: identity) }
    }
     func takePendingUpdates(sdk: String) async throws -> [CloudPendingUpdate] {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowTakePendingUpdates) { try raw.takePendingUpdates(sdk: sdk).map(CloudPendingUpdate.init) }
    }
     func updateApplied(sdk: String, id: UInt64, added: [String]) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowUpdateApplied) { try raw.updateApplied(sdk: sdk, id: id, added: added) }
    }
     func buildBatch(sdk: String, zone: String, pending: [String: CloudPendingChange]) async throws -> CloudEngineBatch? {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowBuildBatch) {
            try raw.buildBatch(sdk: sdk, zone: zone, pending: pending.mapValues(\.raw)).map { batch in
                let admission: CloudMediaBatchAdmission? = zone == "meguri-media" ? CloudMediaBatchAdmission { queue in
                    try await MaintenanceLane.shared.call(.cloudWorkflowSubmitMediaBatch) {
                        try raw.submitMediaBatch(sdk: sdk, batch: batch.id, queue: ForeignMediaQueue(queue))
                    }
                } : nil
                return .init(id: batch.id, saves: batch.saves.map(CloudRecord.init), deletes: batch.deletes, mediaAdmission: admission)
            }
        }
    }
    @discardableResult
    func sent(sdk: String, batchID: UInt64, saved: [CloudRecord], failed: [CloudRecordFailure], deleted: [String], failedDeletes: [CloudRecordFailure], batchCount: UInt32) async throws -> UInt64 {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowSent) { try raw.sent(sdk: sdk, batchId: batchID, saved: saved.map(\.raw), failed: failed.map(\.raw), deleted: deleted, failedDelete: failedDeletes.map(\.raw), batchCount: batchCount) }
    }
     func handlerReturned(sdk: String) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowHandlerReturned) { try raw.handlerReturned(sdk: sdk) }
    }
     func trackingFailure(sdk: String) async throws {
        let raw = try await owner()
        try await MaintenanceLane.shared.call(.cloudWorkflowTrackingFailure) { try raw.trackingFailure(sdk: sdk) }
    }
    func stateUpdate(sdk: String, state: Data) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowStateUpdate) { try raw.stateUpdate(sdk: sdk, state: state) }
    }
     func fetched(sdk: String, zone: String, records: [CloudRecord], deleted: [String]) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFetched) { try raw.fetched(sdk: sdk, zone: zone, records: records.map(\.raw), deleted: deleted) }
    }
     func beginFetch(sdk: String, zones: [String]) async throws -> UInt64 {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowBeginFetch) { try raw.beginFetch(sdk: sdk, zones: zones) }
    }
     func willFetch(sdk: String, group: String, manual: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowWillFetch) { try raw.willFetch(sdk: sdk, group: group, manual: manual) }
    }
     func fetchZone(sdk: String, zone: String, will: Bool, failed: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFetchZone) { try raw.fetchZone(sdk: sdk, zone: zone, will: will, failed: failed) }
    }
     func didFetch(sdk: String, group: String, manual: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowDidFetch) { try raw.didFetch(sdk: sdk, group: group, manual: manual) }
    }
     func finishFetch(sdk: String, id: UInt64) async throws -> Bool {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFinishFetch) { try raw.finishFetch(sdk: sdk, id: id) }
    }
     func beginExchange(sdk: String) async throws -> UInt64 {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowBeginExchange) { try raw.beginExchange(sdk: sdk) }
    }
     func exchangeFetched(sdk: String, complete: Bool) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowExchangeFetched) { try raw.exchangeFetched(sdk: sdk, complete: complete) }
    }
     func completeExchange(sdk: String, id: UInt64, fetched: Bool, cancelled: Bool = false, failed: Bool = false) async throws -> CloudExchangeResult {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowCompleteExchange) { CloudExchangeResult(try raw.completeExchange(sdk: sdk, id: id, fetched: fetched, cancelled: cancelled, failed: failed)) }
    }
     func sendDue(sdk: String) async throws -> Bool {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowSendDue) { try raw.sendDue(sdk: sdk) }
    }
     func beginSend(sdk: String) async throws -> UInt64? {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowBeginSend) { try raw.beginSend(sdk: sdk) }
    }
     func finishSend(sdk: String, id: UInt64, pending: [String: CloudPendingChange]) async throws -> Void {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFinishSend) { try raw.finishSend(sdk: sdk, id: id, pending: pending.mapValues(\.raw)) }
    }
}

public enum CloudPlatformEnvironment: Sendable { case development, production }
extension CoreCloudWorkflowService {
    /// Call before collection open, after resolving the account asynchronously.
    public static func configurePlatform(container: String, environment: CloudPlatformEnvironment, status: CloudAccountStatus, identity: Data?) async throws {
        try await MaintenanceLane.shared.call(.configureCloudPlatform) {
            try MeguriCoreBindings.configureCloudPlatform(container: container, environment: environment == .development ? .development : .production, status: status.raw, value: identity, keyBytes: SystemCloudKeyByteProvider())
        }
    }
}

extension CoreCloudWorkflowService {
    func fetchReadPlan(sdk: String, id: UInt64) async throws -> CloudFetchReadPlan {
        let raw = try await owner()
        return try await MaintenanceLane.shared.call(.cloudWorkflowFetchReadPlan) {
            let plan = try raw.fetchReadPlan(sdk: sdk, id: id)
            return CloudFetchReadPlan(qualified: plan.qualified, reads: plan.reads.map(CoreCloudFetchRead.init))
        }
    }
}
