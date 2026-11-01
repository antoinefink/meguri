#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization

extension NativeCloudOwnTransport {
    func prepareZoneInventory() -> NativePreparedOwnOperation {
        let operation = CKFetchRecordZonesOperation.fetchAllRecordZonesOperation()
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue,
            originalReplyWaiting: zoneInventoryOriginalReplyWaiting)
        struct Inventory {
            var names: Set<String> = []
            var failure: (any Error)?
            var invalid = false
            var finished = false
        }
        let state = Mutex(Inventory())
        let perZoneHandler: @Sendable (CKRecordZone.ID, Result<CKRecordZone, any Error>) -> Void = { id, result in
            state.withLock { value in
                guard !value.finished else { return }
                switch result {
                case .failure(let error):
                    if value.failure == nil { value.failure = error }
                case .success(let zone):
                    do {
                        guard id == zone.zoneID else { throw NativeCloudKitFailure.unsupportedRecord }
                        try NativeCloudKitIdentityValidation.requireCurrentOwner(zone.zoneID)
                        value.names.insert(try NativeCloudKitIdentityValidation.zoneName(id))
                    } catch { value.invalid = true }
                }
            }
        }
        let terminalHandler: @Sendable (Result<Void, any Error>) -> Void = { [weak prepared] result in
            let actual = state.withLock { value -> Inventory? in
                guard !value.finished else { return nil }
                value.finished = true
                return value
            }
            guard let actual else { return }
            guard let prepared else { return }
            do {
                try result.get()
                if let error = actual.failure { throw error }
                guard !actual.invalid else { throw NativeCloudKitFailure.unsupportedRecord }
                guard !prepared.cancelled else { throw CancellationError() }
                prepared.finish(.success(.zoneInventory(zones: actual.names.sorted())))
            } catch { prepared.finish(.failure(error)) }
        }
        operation.perRecordZoneResultBlock = perZoneHandler
        operation.fetchRecordZonesResultBlock = terminalHandler
        zoneInventoryPerZoneHandlerInstalled?(perZoneHandler)
        // Keep the exact assigned closure for raw NSError tests; SDK property
        // getters may normalize partial errors before invoking the handler.
        zoneInventoryTerminalHandlerInstalled?(terminalHandler)
        return prepared
    }
}
#endif
