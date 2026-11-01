#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

// Invoke the exact assigned closure: SDK property getters normalize partial errors.
private final class ZoneInventoryCallbacks: Sendable {
    private let perZoneHandler = Mutex<(@Sendable (CKRecordZone.ID, Result<CKRecordZone, any Error>) -> Void)?>(nil)
    private let handler = Mutex<(@Sendable (Result<Void, any Error>) -> Void)?>(nil)
    func installed(_ callback: @escaping @Sendable (Result<Void, any Error>) -> Void) {
        handler.withLock { $0 = callback }
    }
    func installedPerZone(_ callback: @escaping @Sendable (CKRecordZone.ID, Result<CKRecordZone, any Error>) -> Void) {
        perZoneHandler.withLock { $0 = callback }
    }
    var isInstalled: Bool { handler.withLock { $0 != nil } && perZoneHandler.withLock { $0 != nil } }
    func zone(_ id: CKRecordZone.ID, _ result: Result<CKRecordZone, any Error>) throws {
        let callback = try #require(perZoneHandler.withLock { $0 })
        callback(id, result)
    }
    func receive(_ names: [String]) throws {
        for name in names {
            let id = NativeCloudOwnRecords.zone(name)
            try zone(id, .success(CKRecordZone(zoneID: id)))
        }
    }
    func complete(_ result: Result<Void, any Error>) throws {
        let callback = try #require(handler.withLock { $0 })
        callback(result)
    }
}

/// Synthetic callbacks at the original SDK operation boundary only.
struct NativeCloudZoneInventoryTransportTests {
    @Test func actualAllZonesOperationIsPreparedBeforeOneEnqueueAndReturnsOnlyActualNames() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneInventoryCallbacks()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneInventory)
        #expect(terminal.isInstalled)
        #expect(queue.operations.withLock { $0.isEmpty })
        #expect(prepared.enqueue())
        #expect(!prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZonesOperation })
        #expect(operation.recordZoneIDs == nil)
        #expect(operation.fetchRecordZonesResultBlock != nil)
        // Include unrelated and orphan names; no Active/current/previous seed.
        let names = ["z-unrelated", "c-orphan-7", "arbitrary zone", "meguri-media"]
        #expect(operation.perRecordZoneResultBlock != nil)
        for name in names {
            let id = NativeCloudOwnRecords.zone(name)
            operation.perRecordZoneResultBlock?(id, .success(CKRecordZone(zoneID: id)))
        }
        operation.fetchRecordZonesResultBlock?(.success(()))
        let reply = try await prepared.reply()
        guard case .zoneInventory(let inventory) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(inventory == names.sorted())
        #expect(queue.operations.withLock { $0.count } == 1)
    }

    @Test(arguments: [false, true])
    func successfulEmptyInventoryAndFirstTerminalCannotBeReplaced(empty: Bool) async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneInventoryCallbacks()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneInventory)
        #expect(prepared.enqueue())
        let names = empty ? [] : ["c-only-actual-zone"]
        try terminal.receive(names)
        try terminal.complete(.success(()))
        try terminal.receive(["replacement"])
        try terminal.zone(NativeCloudOwnRecords.zone("late-error"), .failure(CKError(.networkFailure)))
        try terminal.complete(.success(()))
        try terminal.complete(.failure(CKError(.networkFailure)))
        let reply = try await prepared.reply()
        guard case .zoneInventory(let inventory) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(inventory == names)
    }

    @Test(arguments: [CKError.Code.networkFailure, .zoneNotFound, .operationCancelled, .partialFailure], [false, true])
    func originalFailuresRetainExactCodesResetAndNestedRetryMetadata(code: CKError.Code, perZone: Bool) async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneInventoryCallbacks()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneInventory)
        #expect(prepared.enqueue())
        let resetID = NativeCloudOwnRecords.zone("c-reset")
        let retryID = NativeCloudOwnRecords.zone("c-retry")
        let reset = NSError(domain: CKErrorDomain, code: CKError.Code.zoneNotFound.rawValue,
            userInfo: [CKErrorUserDidResetEncryptedDataKey: true])
        let retry = NSError(domain: CKErrorDomain, code: CKError.Code.requestRateLimited.rawValue,
            userInfo: [CKErrorRetryAfterKey: 120.0001])
        let partial = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue,
            userInfo: [CKErrorRetryAfterKey: 10.0, CKPartialErrorsByItemIDKey: [resetID: reset, retryID: retry]])
        let original = NSError(domain: CKErrorDomain, code: code.rawValue,
            userInfo: [CKErrorRetryAfterKey: 5.0, CKErrorUserDidResetEncryptedDataKey: true,
                CKPartialErrorsByItemIDKey: [resetID: partial]])
        try terminal.receive(["c-valid-subset"])
        if perZone {
            try terminal.zone(resetID, .failure(original))
            try terminal.zone(retryID, .failure(CKError(.networkFailure)))
            try terminal.complete(.success(()))
        } else {
            try terminal.zone(resetID, .failure(reset))
            try terminal.complete(.failure(original))
        }
        try terminal.complete(.success(()))
        try terminal.receive(["c-fixture-late-9"])
        try terminal.complete(.success(()))
        let result: Result<CloudOwnTransportReply, any Error>
        do { result = .success(try await prepared.reply()) } catch { result = .failure(error) }
        guard case .failure(let error) = result else { Issue.record("failed inventory became success"); return }
        let actual = error as NSError
        #expect(actual === original)
        #expect(actual.domain == CKErrorDomain && actual.code == code.rawValue)
        #expect((actual.userInfo[CKErrorUserDidResetEncryptedDataKey] as? NSNumber)?.boolValue == true)
        let delay = NativeCloudOwnRecords.retryDuration(error)
        #expect(delay == 120_001)
        let partials = try #require(actual.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any])
        let nested = try #require(partials[resetID] as? NSError)
        #expect(nested === partial && nested.code == CKError.Code.partialFailure.rawValue)
        let leaves = try #require(nested.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any])
        let actualReset = try #require(leaves[resetID] as? NSError)
        let actualRetry = try #require(leaves[retryID] as? NSError)
        #expect(actualReset === reset && actualReset.code == CKError.Code.zoneNotFound.rawValue)
        #expect((actualReset.userInfo[CKErrorUserDidResetEncryptedDataKey] as? NSNumber)?.boolValue == true)
        #expect(actualRetry === retry && actualRetry.code == CKError.Code.requestRateLimited.rawValue)
        #expect((actualRetry.userInfo[CKErrorRetryAfterKey] as? NSNumber)?.doubleValue == 120.0001)
    }

    @Test(arguments: [0, 1, 2, 3])
    func foreignOwnersAndMismatchedPerZoneIdentityRefuseEntireInventory(shape: Int) async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneInventoryCallbacks()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneInventory)
        #expect(prepared.enqueue())
        let own = NativeCloudOwnRecords.zone("c-synthetic")
        let foreign = CKRecordZone.ID(zoneName: own.zoneName, ownerName: "synthetic-foreign-owner")
        let key = shape == 0 || shape == 2 ? foreign : own
        let value = shape == 0 || shape == 1 ? foreign : (shape == 3 ? NativeCloudOwnRecords.zone("c-other") : own)
        try terminal.receive(["c-valid-but-incomplete"])
        try terminal.zone(key, .success(CKRecordZone(zoneID: value)))
        try terminal.complete(.success(()))
        let result: Result<CloudOwnTransportReply, any Error>
        do { result = .success(try await prepared.reply()) } catch { result = .failure(error) }
        guard case .failure(let error) = result else { Issue.record("invalid inventory became success"); return }
        guard let failure = error as? NativeCloudKitFailure, case .unsupportedRecord = failure else {
            Issue.record("unexpected inventory validation failure"); return
        }
    }

    @Test func cancellationBeforeEnqueueRefusesWithoutQueueOrCallback() throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.zoneInventory)
        prepared.cancel()
        #expect(!prepared.enqueue())
        #expect(!prepared.enqueue())
        #expect(queue.operations.withLock { $0.isEmpty })
    }

    @Test(arguments: [false, true])
    func acceptedCallerCancellationWaitsForOriginalTerminalEvenIfSDKReportsSuccess(terminalSucceeds: Bool) async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneInventoryCallbacks()
        let (waiting, admitted) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) },
            zoneInventoryOriginalReplyWaiting: { admitted.yield(()); admitted.finish() }).prepare(.zoneInventory)
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZonesOperation })
        let finished = Mutex(false)
        let terminalReleased = Mutex(false)
        let completedAfterTerminal = Mutex(false)
        let waiter = Task { () -> Result<CloudOwnTransportReply, any Error> in
            defer {
                completedAfterTerminal.withLock { $0 = terminalReleased.withLock { $0 } }
                finished.withLock { $0 = true }
            }
            return await withTaskCancellationHandler {
                do { return .success(try await prepared.reply()) } catch { return .failure(error) }
            } onCancel: { prepared.cancel() }
        }
        var admission = waiting.makeAsyncIterator()
        let observedAdmission: Void? = await admission.next()
        #expect(observedAdmission != nil)
        #expect(!finished.withLock { $0 })
        waiter.cancel()
        #expect(operation.isCancelled)
        #expect(!prepared.enqueue())
        #expect(!finished.withLock { $0 })
        let original = NSError(domain: CKErrorDomain, code: CKError.Code.operationCancelled.rawValue,
            userInfo: [CKErrorRetryAfterKey: 7.0])
        terminalReleased.withLock { $0 = true }
        if terminalSucceeds { try terminal.complete(.success(())) }
        else { try terminal.complete(.failure(original)) }
        let result = await waiter.value
        guard case .failure(let error) = result else { Issue.record("cancelled inventory became success"); return }
        if terminalSucceeds { #expect(error is CancellationError) }
        else { #expect((error as NSError) === original) }
        #expect(finished.withLock { $0 })
        #expect(completedAfterTerminal.withLock { $0 })
        #expect(queue.operations.withLock { $0.count } == 1)
    }

    @Test func originalCallbackDuringSynchronousEnqueueIsBuffered() async throws {
        struct Immediate: NativeCloudDatabaseQueue {
            let terminal: ZoneInventoryCallbacks
            let capturing = NativeOwnCapturingQueue()
            func add(_ operation: CKDatabaseOperation) -> Bool {
                guard operation is CKFetchRecordZonesOperation, terminal.isInstalled else { return false }
                guard capturing.add(operation) else { return false }
                do {
                    let zone = CKRecordZone(zoneID: NativeCloudOwnRecords.zone("c-during-enqueue"))
                    try terminal.zone(zone.zoneID, .success(zone))
                    try terminal.complete(.success(()))
                } catch { Issue.record(error); return false }
                return true
            }
        }
        let terminal = ZoneInventoryCallbacks()
        let queue = Immediate(terminal: terminal)
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneInventoryPerZoneHandlerInstalled: { terminal.installedPerZone($0) },
            zoneInventoryTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneInventory)
        #expect(prepared.enqueue())
        #expect(!prepared.enqueue())
        let reply = try await prepared.reply()
        guard case .zoneInventory(let inventory) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(inventory == ["c-during-enqueue"])
        #expect(queue.capturing.operations.withLock { $0.count } == 1)
    }
}
#endif
