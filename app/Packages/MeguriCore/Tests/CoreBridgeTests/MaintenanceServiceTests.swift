import Dispatch
import Foundation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

// This adapter observes routing while forwarding every call to the real Rust
// surface. It supplies no semantic results and never replaces the raw listener.
private final class RecordingMaintenanceNative: MaintenanceNative {
    let calls = Mutex<[String]>([])
    private func record(_ name: String) {
        #expect(!Thread.isMainThread)
        #expect(String(cString: __dispatch_queue_get_label(nil)) == Lane.maintenance.queueLabel)
        calls.withLock { $0.append(name) }
    }
    func startCreateBackup() throws -> MeguriCoreBindings.CreateBackupJob {
        record("create"); return try MeguriCoreBindings.startCreateBackup()
    }
    func listBackups() throws -> [MeguriCoreBindings.BackupEntry] {
        record("list"); return try MeguriCoreBindings.listBackups()
    }
    func backupLimits() throws -> MeguriCoreBindings.BackupLimits {
        record("limits"); return try MeguriCoreBindings.backupLimits()
    }
    func setBackupLimits(_ limits: MeguriCoreBindings.BackupLimits) throws -> Bool {
        record("setLimits"); return try MeguriCoreBindings.setBackupLimits(limits: limits)
    }
    func startRestoreBackup(_ name: String) throws -> MeguriCoreBindings.RestoreBackupJob {
        record("restore"); return try MeguriCoreBindings.startRestoreBackup(name: name)
    }
    func maintenanceStatus() throws -> MeguriCoreBindings.MaintenanceStatus {
        record("status"); return try MeguriCoreBindings.maintenanceStatus()
    }
    func startRunDueMaintenance(_ trigger: MeguriCoreBindings.MaintenanceTrigger) throws -> MeguriCoreBindings.DueMaintenanceJob {
        record("due"); return try MeguriCoreBindings.startRunDueMaintenance(trigger: trigger)
    }
}
private final class MaintenanceClock: CollectionClock {
    let mainThreadCalls = Mutex<[Bool]>([])
    func nowMilliseconds() -> Int64 { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return 0 }
    func zoneIdentifier() -> String { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return "UTC" }
}
private struct MaintenanceSpace: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 {
        #expect(!Thread.isMainThread)
        return UInt64.max
    }
}

@Suite struct MaintenanceMappingTests {
    @Test func publicMaintenanceValuesAreSendable() {
        func requireSendable<T: Sendable>(_: T.Type) {}
        requireSendable(CoreBridge.MaintenanceError.self)
        requireSendable(CoreBridge.MaintenanceFailure.self)
        requireSendable(CoreBridge.BackupEntry.self)
        requireSendable(CoreBridge.BackupLimits.self)
        requireSendable(CoreBridge.RestoredBackup.self)
        requireSendable(CoreBridge.MaintenanceStatus.self)
        requireSendable(CoreBridge.MaintenanceTrigger.self)
        requireSendable(MaintenanceOperation.self)
    }

    @Test func completeTypedErrorSetPreservesCategoriesAndPayloads() {
        let cases: [(MeguriCoreBindings.MaintenanceError, CoreBridge.MaintenanceError)] = [
            (.cancelled, .cancelled),
            (.insufficientSpace(needed: 8_000_000_000, available: 1_048_576), .insufficientSpace(needed: 8_000_000_000, available: 1_048_576)),
            (.corrupt, .corrupt), (.tooNew, .tooNew), (.schemaUnsupported, .schemaUnsupported),
            (.unsupportedFormat, .unsupportedFormat), (.invalidIds, .invalidIDs), (.safetyBackupFailed, .safetyBackupFailed),
            (.busy(runningOp: "one-way sync"), .busy(runningOperation: "one-way sync")),
            (.collectionUnavailable, .collectionUnavailable), (.collectionCorrupt, .collectionCorrupt),
            (.sessionActive, .sessionActive), (.invalidArgument(message: "invalid backup name"), .invalidArgument("invalid backup name")),
            (.diskFull, .diskFull), (.io(detail: "volume unavailable"), .io("volume unavailable")),
            (.destinationExists, .destinationExists),
        ]
        for (raw, expected) in cases { #expect(CoreBridge.MaintenanceError(raw) == expected) }
    }

    @Test func backupLabelsWallTimeLimitsAndAllTriggersRoundTrip() {
        let kinds: [(MeguriCoreBindings.BackupKind, CoreBridge.BackupKind)] = [
            (.backup, .backup), (.serverCopy, .serverCopy), (.serverCopyUnverified, .serverCopyUnverified),
        ]
        for (kind, expected) in kinds {
            let entry = CoreBridge.BackupEntry(MeguriCoreBindings.BackupEntry(name: "synthetic.colpkg", kind: kind,
                wallTime: .init(year: 2030, month: 1, day: 15, hour: 10, minute: 2, second: 3), sizeBytes: 5_000_000_000))
            #expect(entry.kind == expected && entry.sizeBytes == 5_000_000_000)
            #expect(entry.wallTime.year == 2030 && entry.wallTime.month == 1 && entry.wallTime.day == 15)
            #expect(entry.wallTime.hour == 10 && entry.wallTime.minute == 2 && entry.wallTime.second == 3)
        }
        let limits = CoreBridge.BackupLimits(daily: 21, weekly: 13, monthly: 7, minimumIntervalMinutes: 45)
        #expect(CoreBridge.BackupLimits(limits.raw) == limits)
        let triggers: [(CoreBridge.MaintenanceTrigger, MeguriCoreBindings.MaintenanceTrigger)] = [
            (.opened, .opened), (.background, .background), (.quit, .quit),
            (.backgroundProcessing, .backgroundProcessing), (.deadline, .deadline),
        ]
        for (value, raw) in triggers { #expect(value.raw == raw) }
    }

    @Test func activationFailureKeepsSafetyAndPendingPublicationWhenTransportReportsFailure() {
        let restored = MeguriCoreBindings.RestoredBackup(name: "backup-2030-01-15-10.00.00.colpkg",
            safetyBackup: "backup-2030-01-15-10.00.01.colpkg", generation: 19,
            cloudPublicationPending: true, deadlineMs: nil)
        let raw = MeguriCoreBindings.MaintenanceFailure(cause: .io(detail: "reevaluation failed"),
            candidateActive: true, generation: 19, safetyBackup: restored.safetyBackup,
            restored: restored, backupPublication: .unpublished)
        let failure = CoreBridge.MaintenanceFailure(raw)
        #expect(failure.cause == .io("reevaluation failed") && failure.candidateActive)
        #expect(failure.generation == 19 && failure.safetyBackup == restored.safetyBackup)
        #expect(failure.restored?.cloudPublicationPending == true)
        #expect(failure.restored?.generation == 19 && failure.restored?.deadlineMilliseconds == nil)
        let safetyOnly = CoreBridge.MaintenanceFailure(MeguriCoreBindings.MaintenanceFailure(cause: .cancelled,
            candidateActive: false, generation: nil, safetyBackup: restored.safetyBackup, restored: nil,
            backupPublication: .unpublished))
        #expect(safetyOnly.cause == .cancelled && !safetyOnly.candidateActive)
        #expect(safetyOnly.safetyBackup == restored.safetyBackup && safetyOnly.restored == nil)
        let durable = MeguriCoreBindings.ImportBackupReceipt(name: "safety.colpkg", sizeBytes: 123,
            instantMs: 101, snapshotModifiedMs: 99)
        let published = CoreBridge.MaintenanceFailure(MeguriCoreBindings.MaintenanceFailure(cause: .io(detail: "metadata"),
            candidateActive: false, generation: nil, safetyBackup: nil, restored: nil,
            backupPublication: .durable(receipt: durable)))
        #expect(published.backupPublication == .durable(CoreBridge.ImportBackupReceipt(durable)))
    }

    @Test func sharedProgressRelayBuffersNewestAndTerminatesWithTheJob() async {
        let (stream, continuation) = AsyncStream<CoreProgress>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let relay = ProgressRelay(continuation)
        for done in UInt64(0)...3 {
            relay.onProgress(update: .init(jobId: 133, phase: .planning, etaMillis: nil,
                completed: done, total: 3, collectionMediaPrepared: nil))
        }
        let completion = JobCompletion(progress: continuation)
        completion.resolve(.succeeded)
        var values: [CoreProgress] = []
        for await value in stream { values.append(value) }
        #expect(values.count == 1 && values.first?.completed == 3 && values.first?.total == 3)
        #expect(await completion.wait() == .succeeded)
    }
}

// Open collections and Rust jobs share the existing serialized process suite.
// No raw listener is installed here; EventTests' established listener rules apply.
extension EventTests {
    @Test @MainActor func maintenanceServiceRealBackupRestoreAndEveryCallUsesMaintenanceLane() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"))
        try files.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
        try files.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let clock = MaintenanceClock()
        let collections = CoreCollectionService(clock: clock, diskSpace: MaintenanceSpace())
        let native = RecordingMaintenanceNative()
        let service: any MaintenanceService = CoreMaintenanceService(native: native)
        try await collections.open(location, randomSeed: 133)
        do {
            let generation = try await collections.waitAvailable(timeoutMilliseconds: 0)
            let limits = CoreBridge.BackupLimits(daily: 12, weekly: 10, monthly: 9, minimumIntervalMinutes: 30)
            _ = try await service.setBackupLimits(limits)
            #expect(try await service.backupLimits() == limits)
            let updates = Mutex<[CoreProgress]>([])
            let backup = try await service.createBackup(operation: MaintenanceOperation(), progress: { value in
                updates.withLock { $0.append(value) }
            })
            #expect(backup.name == "backup-2030-01-15-10.00.00.colpkg")
            let attributes = try files.attributesOfItem(atPath: location.backupsDirectory.appending(path: backup.name).path)
            #expect((attributes[.size] as? NSNumber)?.uint64Value == backup.sizeBytes)
            #expect(backup.sizeBytes > 0)
            let listing = try await service.listBackups()
            #expect(listing.first?.name == backup.name && listing.first?.kind == .backup)
            _ = try await service.status()
            #expect(try await service.runDueMaintenance(.deadline, operation: MaintenanceOperation(), progress: { _ in }) == nil)
            let restored = try await service.restoreBackup(name: backup.name, operation: MaintenanceOperation(), progress: { _ in })
            #expect(restored.generation == generation + 1 && restored.cloudPublicationPending)
            let safety = try #require(restored.safetyBackup)
            #expect(files.fileExists(atPath: location.backupsDirectory.appending(path: safety).path))
            #expect(try await collections.lifecycle() == .ready(generation: generation + 1))
            #expect(native.calls.withLock { Set($0) } == Set(["create", "list", "limits", "setLimits", "restore", "status", "due"]))
            #expect(clock.mainThreadCalls.withLock { !$0.isEmpty && $0.allSatisfy { !$0 } })
            for update in updates.withLock({ $0 }) {
                if let total = update.total { #expect(update.completed <= total) }
            }
            try await collections.close()
            for await event in CoreEventStream.shared.events {
                if event.generation == generation + 1 && event.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await collections.close()
            throw error
        }
    }

    @Test func maintenanceExplicitCancelBeforeAttachCancelsAndDrainsTheRealWorker() async throws {
        let operation = MaintenanceOperation()
        await operation.cancel()
        let job = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        await operation.attach(job)
        #expect(await job.outcome() == .cancelled)
        #expect(try await job.cancelToken.isCancelled())
        var count = 0
        for await _ in job.progress { count += 1 }
        #expect(count <= 1, "terminal closes the existing newest-only stream")
    }

    @Test func maintenanceTrackingSetupFailureCancelsAndDrainsTheRealWorker() async throws {
        try await CoreEventStream.shared.start()
        let handle = try await MaintenanceLane.shared.call("test.startJob") {
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        do {
            _ = try await Jobs.trackDraining(handle, setup: { _ in throw BridgeError.invalidArgument("maintenance listener setup") })
            Issue.record("Expected setup failure")
        } catch {
            #expect(error as? BridgeError == .invalidArgument("maintenance listener setup"))
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
        }
    }

    @Test @MainActor func maintenanceOperationCancelsOnControlWhileMaintenanceIsOccupied() async throws {
        let operation = MaintenanceOperation()
        let job = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        await operation.attach(job)
        var progress = job.progress.makeAsyncIterator()
        #expect(await progress.next() != nil)
        let gate = MaintenanceOccupancy()
        let occupying = Task { try await MaintenanceLane.shared.occupy(gate) }
        defer { gate.release.signal() }
        for await _ in gate.entered { break }
        await operation.cancel()
        #expect(try await job.cancelToken.isCancelled())
        #expect(!gate.watchdogFired.withLock { $0 }, "cancellation waited for maintenance")
        gate.release.signal()
        try await occupying.value
        #expect(await job.outcome() == .cancelled)
    }
}
