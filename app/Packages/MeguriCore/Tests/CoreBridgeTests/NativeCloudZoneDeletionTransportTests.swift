#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

// Capture the assigned original before CloudKit normalizes terminal partial errors.
private final class ZoneDeletionTerminalCapture: Sendable {
    private let handler = Mutex<(@Sendable (Result<Void, any Error>) -> Void)?>(nil)
    func installed(_ callback: @escaping @Sendable (Result<Void, any Error>) -> Void) {
        handler.withLock { $0 = callback }
    }
    func complete(_ result: Result<Void, any Error>) throws {
        let callback = try #require(handler.withLock { $0 })
        callback(result)
    }
}

private final class ZoneDeletionHandlers: Sendable {
    private let perZone: Mutex<(CKRecordZone.ID, Result<Void, any Error>) -> Void>
    init(_ operation: CKModifyRecordZonesOperation) throws {
        perZone = Mutex(try #require(operation.perRecordZoneDeleteBlock))
    }
    func deleted(_ zone: CKRecordZone.ID, _ result: Result<Void, any Error>) { perZone.withLock { $0(zone, result) } }
}
struct NativeCloudZoneDeletionTransportTests {
    @Test func terminalForeignOwnerCannotHideBehindSuccessfulDeletionCallback() async throws {
        let queue = NativeOwnCapturingQueue(), terminal = ZoneDeletionTerminalCapture()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.deleteZones(.init(zones: ["c-first"])))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKModifyRecordZonesOperation })
        let handlers = try ZoneDeletionHandlers(operation)
        handlers.deleted(NativeCloudOwnRecords.zone("c-first"), .success(()))
        let foreign = CKRecordZone.ID(zoneName: "c-first", ownerName: "synthetic-foreign-owner")
        try terminal.complete(.failure(CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [foreign: CKError(.zoneNotFound)]])))
        do { _ = try await prepared.reply(); Issue.record("Foreign terminal ID admitted a deletion result") }
        catch { #expect(error is NativeCloudKitFailure) }
    }

    @Test func singletonTerminalAbsenceRetainsExactIssuedRowCause() async throws {
        for reset in [false, true] {
            let queue = NativeOwnCapturingQueue(), terminal = ZoneDeletionTerminalCapture()
            let prepared = try NativeCloudOwnTransport(queue: queue,
                zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) })
                .prepare(.deleteZones(.init(zones: ["c-first"])))
            #expect(prepared.enqueue())
            try terminal.complete(.failure(CKError(.zoneNotFound,
                userInfo: [CKErrorUserDidResetEncryptedDataKey: NSNumber(value: reset)])))
            guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
            #expect(reply.results == [.init(zone: "c-first", failure: .init(code: 26, encryptedDataReset: reset))])
            #expect(reply.terminal?.code == 26 && reply.terminal?.encryptedDataReset == reset)
        }
    }
    @Test(arguments: [CKError.Code.unknownItem, .networkFailure, .userDeletedZone])
    func singletonTerminalFailureRetainsOriginalNumericCause(code: CKError.Code) async throws {
        let terminal = ZoneDeletionTerminalCapture()
        let prepared = try NativeCloudOwnTransport(queue: NativeOwnCapturingQueue(),
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.deleteZones(.init(zones: ["c-first"])))
        #expect(prepared.enqueue())
        try terminal.complete(.failure(CKError(code)))
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results == [.init(zone: "c-first", failure: .init(code: Int64(code.rawValue)))])
        #expect(reply.terminal?.code == Int64(code.rawValue))
    }

    @Test func ambiguousTerminalAbsenceDoesNotGuessMissingRows() async throws {
        let terminal = ZoneDeletionTerminalCapture()
        let prepared = try NativeCloudOwnTransport(queue: NativeOwnCapturingQueue(),
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) })
            .prepare(.deleteZones(.init(zones: ["c-first", "c-second"])))
        #expect(prepared.enqueue())
        try terminal.complete(.failure(CKError(.zoneNotFound)))
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results.allSatisfy { $0.failure != nil && $0.failure?.code == nil })
        #expect(reply.terminal?.code == 26)
    }
    @Test func accountWideTerminalResetRetainsAllExactIssuedAbsenceRows() async throws {
        let terminal = ZoneDeletionTerminalCapture()
        let names = ["c-first", "c-second"]
        let prepared = try NativeCloudOwnTransport(queue: NativeOwnCapturingQueue(),
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.deleteZones(.init(zones: names)))
        #expect(prepared.enqueue())
        try terminal.complete(.failure(CKError(.zoneNotFound,
            userInfo: [CKErrorUserDidResetEncryptedDataKey: NSNumber(value: true)])))
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results == names.map { .init(zone: $0, failure: .init(code: 26, encryptedDataReset: true)) })
    }

    @Test func oneNativeBatchPreservesOriginalPerZoneTerminalAndEncryptedResetErrors() async throws {
        let zones = ["c-first", "c-second", "c-third"]
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneDeletionTerminalCapture()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.deleteZones(.init(zones: zones)))
        #expect(prepared.enqueue())
        #expect(!prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKModifyRecordZonesOperation })
        #expect(operation.recordZonesToSave == nil)
        #expect(operation.recordZoneIDsToDelete == zones.map(NativeCloudOwnRecords.zone))
        let handlers = try ZoneDeletionHandlers(operation)
        let reset = CKError(.zoneNotFound, userInfo: [CKErrorUserDidResetEncryptedDataKey: true])
        let retry = CKError(.serviceUnavailable, userInfo: [CKErrorRetryAfterKey: 123.0])
        let terminalError = CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [NativeCloudOwnRecords.zone(zones[2]): retry]])
        handlers.deleted(NativeCloudOwnRecords.zone(zones[0]), .success(()))
        handlers.deleted(NativeCloudOwnRecords.zone(zones[1]), .failure(reset))
        // No per-zone callback for third: use its actual terminal partial cause.
        try terminal.complete(.failure(terminalError))
        try terminal.complete(.success(())) // A duplicate callback cannot replace it.
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results == [.init(zone: zones[0], failure: nil),
            .init(zone: zones[1], failure: .init(code: 26, encryptedDataReset: true)),
            .init(zone: zones[2], failure: .init(code: 6, retryAfterMilliseconds: 123_000))])
        #expect(reply.terminal?.code == 2 && reply.terminal?.retryAfterMilliseconds == 123_000)
        #expect(queue.operations.withLock { $0.count } == 1)
    }
    @Test func missingCallbackIsNeverTreatedAsADeletionAndCancelStillJoinsOriginalReply() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneDeletionTerminalCapture()
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.deleteZones(.init(zones: ["c-first", "c-second"])))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKModifyRecordZonesOperation })
        let handlers = try ZoneDeletionHandlers(operation)
        prepared.cancel()
        #expect(operation.isCancelled)
        handlers.deleted(NativeCloudOwnRecords.zone("c-first"), .failure(CKError(.operationCancelled)))
        try terminal.complete(.success(()))
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results[0].failure?.code == 20)
        #expect(reply.results[1].failure != nil && reply.results[1].failure?.code == nil)
    }
    @Test func activeReadRetainsPerRecordAndOriginalTerminalPartialFailure() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = ZoneDeletionTerminalCapture()
        let target = CloudControlTarget(zone: "meguri-root", name: "active", recordType: "MGActive")
        let prepared = try NativeCloudOwnTransport(queue: queue,
            zoneDeletionTerminalHandlerInstalled: { terminal.installed($0) }).prepare(.zoneDeletionActiveRead(target))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        #expect(operation.recordIDs == [NativeCloudOwnRecords.id(target)] && operation.desiredKeys == nil)
        let receive = try #require(operation.perRecordResultBlock)
        let failure = CKError(.zoneNotFound, userInfo: [CKErrorUserDidResetEncryptedDataKey: true])
        receive(NativeCloudOwnRecords.id(target), .failure(failure))
        try terminal.complete(.failure(CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [NativeCloudOwnRecords.id(target): failure]])))
        guard case .zoneDeletionActiveRead(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.read == .zoneMissing)
        #expect(reply.failure?.code == 26 && reply.failure?.encryptedDataReset == true)
        #expect(reply.terminal?.code == 2)
    }
    @Test func bufferedOriginalCallbackMayArriveDuringSynchronousEnqueue() async throws {
        struct Immediate: NativeCloudDatabaseQueue {
            func add(_ operation: CKDatabaseOperation) -> Bool {
                guard let operation = operation as? CKModifyRecordZonesOperation else { return false }
                for zone in operation.recordZoneIDsToDelete ?? [] { operation.perRecordZoneDeleteBlock?(zone, .success(())) }
                operation.modifyRecordZonesResultBlock?(.success(()))
                return true
            }
        }
        let prepared = try NativeCloudOwnTransport(queue: Immediate()).prepare(.deleteZones(.init(zones: ["c-first"])))
        #expect(prepared.enqueue())
        guard case .deletedZones(let reply) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(reply.results == [.init(zone: "c-first", failure: nil)])
    }
}
#endif
