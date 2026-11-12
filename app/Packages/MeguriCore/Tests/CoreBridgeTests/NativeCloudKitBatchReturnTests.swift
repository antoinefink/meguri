#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

// This permission fake tests native ordering only. It supplies no epoch body,
// producer evidence or signed authority; Rust's Service tests cover those facts.
private final class NativeBatchReturnPermission: Sendable {
    private struct State {
        var allowed = true
        var prepared = false
        var validationStages: [Bool] = []
    }
    private let state = Mutex(State())
    func validate() throws {
        try state.withLock {
            $0.validationStages.append($0.prepared)
            guard $0.allowed else { throw NativeCloudKitFailure.unsupportedRecord }
        }
    }
    func revoke() { state.withLock { $0.allowed = false } }
    func prepared() { state.withLock { $0.prepared = true } }
    var validationStages: [Bool] { state.withLock { $0.validationStages } }
}

private final class NativeBatchReturnMedia: @unchecked Sendable {
    let value: CKSyncEngine.RecordZoneChangeBatch
    init(_ value: CKSyncEngine.RecordZoneChangeBatch) { self.value = value }
}

private actor DelayedNativeBatchReturn {
    let permission: NativeBatchReturnPermission
    private let lifetime = CloudKitNetworkLifetime()
    private var savedRemoval = NativeCloudSavedRemoval()
    private var pendingBatch: UInt64?
    private var batches: [String: UInt64] = [:]

    init(permission: NativeBatchReturnPermission) { self.permission = permission }

    func returnBatch(after gate: CloudCallbackGate, revoked: Bool) async throws {
        let permission = self.permission
        let generation = lifetime.authority()
        let zone = "c-aaaaaaaaaaaaaaaaaaaaaaaaaa"
        let save = CloudRecord(name: "synthetic-save", envelope: Data([1]), systemFields: nil)
        let batch = CloudEngineBatch(id: 42, saves: [save], deletes: ["synthetic-delete"],
            finalAdmission: { try permission.validate() })
        // The original request was admitted before native preparation paused.
        try permission.validate()
        let issue = savedRemoval.capture(pending: [save.name: .save, "synthetic-delete": .delete], generation: 7)
        let prepared = CKSyncEngine.RecordZoneChangeBatch(
            recordsToSave: [try NativeCloudRecordConversion.encode(save, zone: zone)],
            recordIDsToDelete: [.init(recordName: "synthetic-delete", zoneID: NativeCloudOwnRecords.zone(zone))],
            atomicByZone: false)
        await gate.enter()
        permission.prepared()
        // The same synchronous return boundary used by the native engine.
        let returned = NativeCloudKitBatchReturn.finish(prepared, batch: batch,
            lifetime: lifetime, generation: generation, issue: issue,
            savedRemoval: &savedRemoval, pendingBatch: &pendingBatch, batches: &batches)
        #expect((returned != nil) == !revoked)
        #expect(pendingBatch == (revoked ? nil : 42))
        #expect(batches == (revoked ? [:] : [save.name: 42, "synthetic-delete": 42]))
        #expect(permission.validationStages == [false, true])

        // A denied offer must leave no issue that a later Sent can confirm.
        savedRemoval.sent(42, names: [save.name, "synthetic-delete"])
        savedRemoval.observe(instance: "synthetic-native", pending: [:], generation: 7)
        let receipts = savedRemoval.take(generation: 7)
        #expect(receipts == (revoked ? [] : [.init(instanceID: "synthetic-native", requestID: 42,
            names: [save.name, "synthetic-delete"])]))
    }

    func returnMediaBatch(after gate: CloudCallbackGate, lifetime: CloudKitNetworkLifetime, cancelled: Bool) async throws {
        let permission = self.permission
        let generation = lifetime.authority()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("original-stage")
        try Data([7, 5, 3, 1]).write(to: file)
        let name = "baaaaaaaaaaaaaaaaaaaaaaaaaa"
        let record = CloudRecord(name: name, envelope: Data([1]), systemFields: nil,
            recordType: "MGBlob", zone: "meguri-media", assetURL: file)
        let admission = CloudMediaBatchAdmission { delivery in delivery.enqueue() }
        let batch = CloudEngineBatch(id: 42, saves: [record], deletes: [], mediaAdmission: admission,
            finalAdmission: { try permission.validate() })
        try permission.validate()
        let issue = savedRemoval.capture(pending: [name: .save], generation: generation)
        try NativeCloudMediaBatch.validate(batch, zone: "meguri-media")
        let prepared = CKSyncEngine.RecordZoneChangeBatch(
            recordsToSave: [try NativeCloudRecordConversion.encode(record, zone: "meguri-media")],
            recordIDsToDelete: [], atomicByZone: false)
        let delivery = try await admission.deliver(NativeBatchReturnMedia(prepared), commit: { deliver in
            var committed = false
            try lifetime.enqueue(generation: generation, validate: {}) { committed = deliver() }
            return committed
        })
        let delivered = try #require(delivery)
        // Real delivery has resumed its continuation, but the actor's final
        // return is held while native cancellation may advance the generation.
        await gate.enter()
        permission.prepared()
        let returned = NativeCloudKitBatchReturn.finish(delivered.value, batch: batch,
            lifetime: lifetime, generation: generation, issue: issue,
            savedRemoval: &savedRemoval, pendingBatch: &pendingBatch, batches: &batches)
        #expect((returned != nil) == !cancelled)
        #expect(pendingBatch == (cancelled ? nil : 42))
        #expect(batches == (cancelled ? [:] : [name: 42]))
        // Core permission remains valid in both cases, including final validation.
        #expect(permission.validationStages == [false, true])
        savedRemoval.sent(42, names: [name])
        savedRemoval.observe(instance: "synthetic-native", pending: [:], generation: generation)
        #expect(savedRemoval.take(generation: generation) == (cancelled ? [] : [
            .init(instanceID: "synthetic-native", requestID: 42, names: [name])]))
    }
}

struct NativeCloudKitBatchReturnTests {
    @Test(arguments: [false, true])
    func originalNativeResultDrainsBeforeSameNameReoffer(retry: Bool) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("original-stage")
        try Data([7, 5, 3, 1]).write(to: file)
        let name = "baaaaaaaaaaaaaaaaaaaaaaaaaa"
        let record = CloudRecord(name: name, envelope: Data([1]), systemFields: nil,
            recordType: "MGBlob", zone: "meguri-media", assetURL: file)
        let admissions = Mutex(0)
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        func batch(_ id: UInt64) -> CloudEngineBatch {
            CloudEngineBatch(id: id, saves: [record], deletes: [],
                finalAdmission: { admissions.withLock { $0 += 1 } })
        }
        let original = batch(42)
        let fresh = batch(43)
        try NativeCloudMediaBatch.validate(original, zone: "meguri-media")
        let encoded = try NativeCloudRecordConversion.encode(record, zone: "meguri-media")
        #expect((encoded["a"] as? CKAsset)?.fileURL == file)
        #expect(encoded.recordID.zoneID == NativeCloudOwnRecords.zone("meguri-media"))
        let prepared = CKSyncEngine.RecordZoneChangeBatch(recordsToSave: [encoded],
            recordIDsToDelete: [], atomicByZone: false)
        var savedRemoval = NativeCloudSavedRemoval()
        var pendingBatch: UInt64?
        var batches: [String: UInt64] = [:]
        let issue = savedRemoval.capture(pending: [name: .save], generation: 7)
        #expect(NativeCloudKitBatchReturn.finish(prepared, batch: original,
            lifetime: lifetime, generation: generation, issue: issue,
            savedRemoval: &savedRemoval, pendingBatch: &pendingBatch, batches: &batches) != nil)

        // Both a delayed original result and its unfinished bookkeeping own
        // the name. Check the early preparation fence and the final boundary.
        for callbackDelivered in [false, true] {
            if callbackDelivered {
                let routed = try NativeCloudKitRecordRouting.sentEngineBatchID(
                    saved: retry ? [] : [encoded.recordID], failed: retry ? [encoded.recordID] : [],
                    deleted: [], collectionZoneID: NativeCloudOwnRecords.zone("c-synthetic"),
                    batches: batches, pendingBatch: pendingBatch)
                #expect(routed == original.id)
                savedRemoval.sent(routed, names: retry ? [] : [name])
            }
            for offer in [original, fresh] {
                #expect(!NativeCloudKitBatchReturn.canPrepare(offer, batches: batches))
                #expect(NativeCloudKitBatchReturn.finish(prepared, batch: offer,
                    lifetime: lifetime, generation: generation, issue: issue,
                    savedRemoval: &savedRemoval, pendingBatch: &pendingBatch, batches: &batches) == nil)
                #expect(batches == [name: original.id])
                #expect(pendingBatch == original.id)
                #expect(admissions.withLock { $0 } == 1)
            }
        }
        savedRemoval.observe(instance: "synthetic-native", pending: retry ? [name: .save] : [:], generation: 7)
        #expect(savedRemoval.take(generation: 7) == (retry ? [] : [
            .init(instanceID: "synthetic-native", requestID: original.id, names: [name])]))

        // Production calls this only after the original callback and both
        // delegate bookkeeping awaits complete. Retry may reoffer the same id.
        NativeCloudKitBatchReturn.drained(original.id, pendingBatch: &pendingBatch, batches: &batches)
        #expect(batches.isEmpty && pendingBatch == nil)
        let reoffer = retry ? original : fresh
        #expect(NativeCloudKitBatchReturn.canPrepare(reoffer, batches: batches))
        let nextIssue = savedRemoval.capture(pending: [name: .save], generation: 7)
        #expect(NativeCloudKitBatchReturn.finish(prepared, batch: reoffer,
            lifetime: lifetime, generation: generation, issue: nextIssue,
            savedRemoval: &savedRemoval, pendingBatch: &pendingBatch, batches: &batches) != nil)
        #expect(batches == [name: reoffer.id] && pendingBatch == reoffer.id)
        #expect(admissions.withLock { $0 } == 2)
        // The old Saved/absence cannot confirm removal for this new offer.
        savedRemoval.observe(instance: "synthetic-native", pending: [:], generation: 7)
        #expect(savedRemoval.take(generation: 7).isEmpty)
    }

    @Test(arguments: [false, true])
    func delayedNativePreparationRevalidatesAtBatchReturn(revoked: Bool) async throws {
        let gate = CloudCallbackGate()
        let permission = NativeBatchReturnPermission()
        let boundary = DelayedNativeBatchReturn(permission: permission)
        try await withCloudTask(gates: [gate], operation: {
            try await boundary.returnBatch(after: gate, revoked: revoked)
        }, whileRunning: {
            try await gate.waitForArrival()
            #expect(permission.validationStages == [false])
            if revoked { permission.revoke() }
        })
        #expect(await gate.pendingWaiterCount() == 0)
    }

    @Test(arguments: [false, true])
    func deliveredMediaRevalidatesOriginalNativeLifetimeAtBatchReturn(cancelled: Bool) async throws {
        let gate = CloudCallbackGate()
        let permission = NativeBatchReturnPermission()
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let boundary = DelayedNativeBatchReturn(permission: permission)
        try await withCloudTask(gates: [gate], operation: {
            try await boundary.returnMediaBatch(after: gate, lifetime: lifetime, cancelled: cancelled)
        }, whileRunning: {
            try await gate.waitForArrival()
            #expect(permission.validationStages == [false])
            if cancelled {
                #expect(lifetime.beginCancellation().isEmpty)
                lifetime.endCancellation()
                #expect(lifetime.authority() != generation)
            }
        })
        #expect(await gate.pendingWaiterCount() == 0)
    }
}
#endif
