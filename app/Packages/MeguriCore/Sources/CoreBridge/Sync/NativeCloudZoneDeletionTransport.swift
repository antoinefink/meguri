#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization

extension CloudZoneDeletionFailure {
    static func native(_ error: any Error) -> Self {
        let error = error as NSError
        return .init(code: error.domain == CKErrorDomain ? Int64(error.code) : nil,
            retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(error).map { Int64(clamping: $0) },
            encryptedDataReset: (error.userInfo[CKErrorUserDidResetEncryptedDataKey] as? NSNumber)?.boolValue == true)
    }
    static func terminal<T>(_ result: Result<T, any Error>) -> Self? {
        if case .failure(let error) = result { return .native(error) }
        return nil
    }
}
extension NativeCloudOwnTransport {
    func prepareZoneDeletionRead(_ target: CloudControlTarget) throws -> NativePreparedOwnOperation {
        guard target == CloudControlTarget(zone: "meguri-root", name: "active", recordType: "MGActive") else { throw NativeCloudKitFailure.unsupportedRecord }
        let id = NativeCloudOwnRecords.id(target)
        let operation = CKFetchRecordsOperation(recordIDs: [id])
        operation.desiredKeys = nil
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        struct State { var result: CloudControlRead?; var failure: CloudZoneDeletionFailure?; var finished = false }
        let state = Mutex(State())
        operation.perRecordResultBlock = { received, result in
            state.withLock { value in
                guard !value.finished else { return }
                guard received == id, value.result == nil else {
                    CloudZoneDeletionDiagnostics.record(.invalidReply, error: CloudOwnTransportFailure.unexpectedReply)
                    value.result = .failed; value.failure = .init(code: nil); return
                }
                do {
                    value.result = try NativeCloudOwnRecords.readResult(result, target: target)
                    if case .failure(let error) = result { value.failure = .native(error) }
                } catch {
                    CloudZoneDeletionDiagnostics.record(.invalidReply, error: error)
                    value.result = .failed; value.failure = .native(error)
                }
            }
        }
        let terminalHandler: @Sendable (Result<Void, any Error>) -> Void = { [weak prepared] result in
            let read = state.withLock { value in let read = value; value.finished = true; return read }
            guard let prepared else { return }
            var failure = read.failure
            var original = read.result
            if original == nil, case .failure(let error) = result {
                // Preserve a single-record partial error's actual cause as well
                // as its enclosing terminal failure.
                let native = error as NSError
                let partials = native.userInfo[CKPartialErrorsByItemIDKey] as? [CKRecord.ID: any Error]
                let cause = partials?[id] ?? error
                original = NativeCloudOwnRecords.readError(cause); failure = .native(cause)
            }
            if original == nil {
                CloudZoneDeletionDiagnostics.record(.invalidReply, error: CloudOwnTransportFailure.missingReply)
                original = .failed; failure = .init(code: nil)
            }
            prepared.finish(.success(.zoneDeletionActiveRead(.init(read: original ?? .failed,
                failure: failure, terminal: .terminal(result)))))
        }
        zoneDeletionTerminalHandlerInstalled?(terminalHandler)
        operation.fetchRecordsResultBlock = terminalHandler
        return prepared
    }
    func prepareZoneDeletion(_ batch: CloudZoneDeletionBatch) throws -> NativePreparedOwnOperation {
        let names = batch.zones
        guard !names.isEmpty, Set(names).count == names.count, names.allSatisfy({ $0.hasPrefix("c-") }) else { throw NativeCloudKitFailure.unsupportedRecord }
        let ids = Set(names.map(NativeCloudOwnRecords.zone))
        let operation = CKModifyRecordZonesOperation(recordZonesToSave: nil, recordZoneIDsToDelete: names.map(NativeCloudOwnRecords.zone))
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        struct State { var results: [CKRecordZone.ID: CloudZoneDeletionResult] = [:]; var invalid = false; var finished = false }
        let state = Mutex(State())
        operation.perRecordZoneDeleteBlock = { id, result in
            state.withLock { value in
                guard !value.finished else { return }
                guard ids.contains(id), id.ownerName == CKCurrentUserDefaultName, value.results[id] == nil else { value.invalid = true; return }
                value.results[id] = .init(zone: id.zoneName, failure: .terminal(result))
            }
        }
        let terminalHandler: @Sendable (Result<Void, any Error>) -> Void = { [weak prepared] result in
            let actual = state.withLock { value in let actual = value; value.finished = true; return actual }
            guard let prepared else { return }
            let terminal = CloudZoneDeletionFailure.terminal(result)
            let partials: [CKRecordZone.ID: any Error]?
            if case .failure(let error) = result { partials = (error as NSError).userInfo[CKPartialErrorsByItemIDKey] as? [CKRecordZone.ID: any Error] }
            else { partials = nil }
            let results = names.map { name -> CloudZoneDeletionResult in
                let id = NativeCloudOwnRecords.zone(name)
                // Actual per-item errors survive a terminal partial wrapper.
                // Missing callbacks are never inferred successful.
                if let per = actual.results[id], per.failure != nil { return per }
                if let error = partials?[id] { return .init(zone: name, failure: .native(error)) }
                if let per = actual.results[id], !actual.invalid { return per }
                CloudZoneDeletionDiagnostics.record(.invalidReply, error: CloudOwnTransportFailure.missingReply)
                return .init(zone: name, failure: .init(code: nil))
            }
            prepared.finish(.success(.deletedZones(.init(results: results, terminal: terminal))))
        }
        zoneDeletionTerminalHandlerInstalled?(terminalHandler)
        operation.modifyRecordZonesResultBlock = terminalHandler
        return prepared
    }
}
#endif
