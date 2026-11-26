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
    func updateBackupLimit(_ patch: MeguriCoreBindings.BackupLimitPatch) throws -> MeguriCoreBindings.BackupLimits {
        record("updateLimit"); return try MeguriCoreBindings.updateBackupLimit(patch: patch)
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
    let instant = Mutex<Int64>(1_894_701_600_000)
    func advance() { instant.withLock { $0 += 1000 } }
    let mainThreadCalls = Mutex<[Bool]>([])
    func nowMilliseconds() -> Int64 { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return instant.withLock { $0 } }
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
        requireSendable(CoreBridge.BackupLimitPatch.self)
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
        for (raw, expected) in cases {
            #expect(CoreBridge.MaintenanceError(raw) == expected)
            #expect(BridgeError(MeguriCoreBindings.CoreError.Maintenance(cause: raw)) == .maintenance(expected))
        }
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
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
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
            // Settings cached the old limits before another writer committed.
            let cached = try await service.backupLimits()
            let remote = CoreBridge.BackupLimits(daily: .max, weekly: 10, monthly: 23, minimumIntervalMinutes: .max)
            _ = try await service.setBackupLimits(remote)
            #expect(cached.monthly == 9)
            let confirmed = try await service.updateBackupLimit(.weekly(4))
            #expect(confirmed == .init(daily: .max, weekly: 4, monthly: 23, minimumIntervalMinutes: .max))
            #expect(try await service.backupLimits() == confirmed)
            #expect(try await service.updateBackupLimit(.daily(0)) == .init(daily: 0, weekly: 4, monthly: 23, minimumIntervalMinutes: .max))
            #expect(try await service.updateBackupLimit(.monthly(.max)) == .init(daily: 0, weekly: 4, monthly: .max, minimumIntervalMinutes: .max))
            #expect(try await service.updateBackupLimit(.minimumIntervalMinutes(30)) == .init(daily: 0, weekly: 4, monthly: .max, minimumIntervalMinutes: 30))
            _ = try await service.setBackupLimits(limits)
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
            clock.advance() // Forced safety backup has a distinct wall name.
            let restored = try await service.restoreBackup(name: backup.name, operation: MaintenanceOperation(), progress: { _ in })
            #expect(restored.generation == generation + 1 && restored.cloudPublicationPending)
            let safety = try #require(restored.safetyBackup)
            #expect(files.fileExists(atPath: location.backupsDirectory.appending(path: safety).path))
            #expect(try await collections.lifecycle() == .ready(generation: generation + 1))
            #expect(native.calls.withLock { Set($0) } == Set(["create", "list", "limits", "setLimits", "updateLimit", "restore", "status", "due"]))
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
            #expect(error == .invalidArgument("maintenance listener setup"))
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

extension MaintenanceMappingTests {
    @Test func fullStatusRetainsEveryValueEveryProblemCountAndIndependentFieldErrors() throws {
        let problems = MeguriCoreBindings.CheckDatabaseProblems(cardProperties: 1, newPositionsTooHigh: 2,
            cardLastReviewTime: 3, cardsWithoutNote: 4, decksMissing: 5, fieldCounts: 6, duplicateCardOrds: 7,
            cardsWithoutTemplate: 8, revlogProperties: 9, notesInvalidUtf8: 10, futureIds: 11, notetypesRecovered: UInt64.max)
        let status = CoreBridge.MaintenanceStatus(MeguriCoreBindings.MaintenanceStatus(
            lastBackup: .init(name: "backup.colpkg", instantMs: 1_894_701_600_123, snapshotModifiedMs: -7001),
            lastCheckDatabase: .init(instantMs: 1_894_701_600_456, result: .problems(problems: problems)),
            lastOptimizeMs: 1_894_701_600_789, lastQuickCheck: .init(instantMs: 1_894_701_600_987, passed: false),
            collectionBytes: 5_000_000_001, mediaBytes: 6_000_000_002, trashBytes: UInt64.max, remedies: [.restore], failures: []))
        #expect(status.lastBackup?.name == "backup.colpkg" && status.lastBackup?.instantMilliseconds == 1_894_701_600_123)
        #expect(status.lastBackup?.snapshotModifiedMilliseconds == -7001)
        #expect(status.lastCheckDatabase?.instantMilliseconds == 1_894_701_600_456)
        guard case .problems(let counts) = status.lastCheckDatabase?.result else { Issue.record("Expected typed problem counts"); return }
        #expect([counts.cardProperties, counts.newPositionsTooHigh, counts.cardLastReviewTime, counts.cardsWithoutNote,
                 counts.decksMissing, counts.fieldCounts, counts.duplicateCardOrds, counts.cardsWithoutTemplate,
                 counts.revlogProperties, counts.notesInvalidUTF8, counts.futureIDs, counts.notetypesRecovered]
                == [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, UInt64.max])
        #expect(status.lastOptimizeMilliseconds == 1_894_701_600_789)
        #expect(status.lastQuickCheck?.instantMilliseconds == 1_894_701_600_987 && status.lastQuickCheck?.passed == false)
        #expect(status.collectionBytes == 5_000_000_001 && status.mediaBytes == 6_000_000_002 && status.trashBytes == UInt64.max)
        #expect(status.remedies == [.restore] && status.failures.isEmpty)
        let rawFields: [MeguriCoreBindings.MaintenanceStatusField] = [.lastBackup, .lastCheckDatabase, .lastOptimize, .lastQuickCheck, .collectionBytes, .mediaBytes, .trashBytes, .remedies]
        let failed = CoreBridge.MaintenanceStatus(MeguriCoreBindings.MaintenanceStatus(lastBackup: nil,
            lastCheckDatabase: .init(instantMs: 99, result: .corrupt), lastOptimizeMs: nil, lastQuickCheck: nil,
            collectionBytes: nil, mediaBytes: nil, trashBytes: nil, remedies: [],
            failures: rawFields.map { .init(field: $0, cause: .io(detail: "record unreadable")) }))
        #expect(failed.failures.map(\.field) == [.lastBackup, .lastCheckDatabase, .lastOptimize, .lastQuickCheck, .collectionBytes, .mediaBytes, .trashBytes, .remedies])
        #expect(failed.failures.allSatisfy { $0.cause == .io("record unreadable") })
        #expect(failed.lastBackup == nil && failed.lastOptimizeMilliseconds == nil && failed.lastQuickCheck == nil)
        #expect(failed.collectionBytes == nil && failed.mediaBytes == nil && failed.trashBytes == nil)
        #expect(failed.lastCheckDatabase?.result == .corrupt && failed.lastCheckDatabase?.instantMilliseconds == 99)
    }
}

extension EventTests {
    @Test @MainActor func maintenanceServiceSafetyFailureOutranksCancellation() async throws {
        try await maintenancePrecedenceFixture(afterActivation: false, failReevaluation: false, listenerFailure: false)
    }
    @Test @MainActor func maintenanceServiceCompletedRestoreOutranksLateCancellation() async throws {
        try await maintenancePrecedenceFixture(afterActivation: true, failReevaluation: false, listenerFailure: false)
    }
    @Test @MainActor func maintenanceServiceActivatedFailureOutranksCancellation() async throws {
        try await maintenancePrecedenceFixture(afterActivation: true, failReevaluation: true, listenerFailure: false)
    }
    @Test @MainActor func maintenanceServiceActivatedFailureOutranksListenerSetupFailure() async throws {
        try await maintenancePrecedenceFixture(afterActivation: true, failReevaluation: true, listenerFailure: true)
    }
    @Test @MainActor func maintenanceServiceSafetyFailureOutranksListenerSetupFailure() async throws {
        try await maintenancePrecedenceFixture(afterActivation: false, failReevaluation: false, listenerFailure: true)
    }
}

// All semantics come from real restore and KTD18 boundaries. The sole Swift
// injection throws during listener setup, after native work reached the gate.
@MainActor private func maintenancePrecedenceFixture(afterActivation: Bool, failReevaluation: Bool, listenerFailure: Bool) async throws {
    let files = FileManager.default
    let root = files.temporaryDirectory.appending(path: UUID().uuidString)
    let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
        manualTransferDirectory: root.appending(path: "ManualTransfers"))
    try files.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
    try files.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
    defer { try? files.removeItem(at: root) }
    try await MaintenanceLane.shared.bootstrapCollection(location)
    let clock = MaintenanceClock()
    let collections = CoreCollectionService(clock: clock, diskSpace: MaintenanceSpace())
    try await collections.open(location, randomSeed: 133)
    do {
        let generation = try await collections.waitAvailable(timeoutMilliseconds: 0)
        let backup = try await CoreMaintenanceService().createBackup(operation: MaintenanceOperation(), progress: { _ in })
        clock.advance() // Never relax the backend's exclusive-create wall-name rule.
        let gate = try await MaintenanceLane.shared.armMaintenanceBoundary(afterActivation: afterActivation, failReevaluation: failReevaluation)
        let setupRelease = DispatchSemaphore(value: 0)
        let (handles, handleReady) = AsyncStream<MeguriCoreBindings.JobHandle>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let (updates, updateReady) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let subject = CoreMaintenanceService(native: RecordingMaintenanceNative(), trackingSetup: { handle in
            if listenerFailure {
                handleReady.yield(handle)
                setupRelease.wait()
                throw BridgeError.invalidArgument("maintenance listener setup")
            }
            return try Jobs.track(handle)
        })
        let operation = MaintenanceOperation()
        let worker = Task {
            defer { handleReady.finish(); updateReady.finish() }
            return try await subject.restoreBackup(name: backup.name, operation: operation, progress: { _ in updateReady.yield(()) })
        }
        do {
            var setupHandle: MeguriCoreBindings.JobHandle?
            if listenerFailure {
                var next = handles.makeAsyncIterator()
                let requiredHandle = try #require(await next.next())
                setupHandle = requiredHandle
            } else {
                var next = updates.makeAsyncIterator()
                #expect(await next.next() != nil) // Service attached before cancellation.
            }
            try await QueryLane.shared.awaitMaintenanceBoundary(gate)
            if let setupHandle {
                try await CoreBridge.CancelToken(raw: setupHandle.cancel).cancel()
                setupRelease.signal()
            } else {
                await operation.cancel()
            }
            try await ControlLane.shared.releaseMaintenanceBoundary(gate)
            let result = await worker.result
            try await ControlLane.shared.resetMaintenanceBoundary(gate)
            if afterActivation && !failReevaluation {
                let restored = try result.get()
                #expect(restored.generation == generation + 1 && restored.cloudPublicationPending)
                #expect(restored.safetyBackup != nil)
            } else {
                guard case .failure(let error) = result else { Issue.record("Expected owned maintenance failure"); try await collections.close(); return }
                let failure = try #require(error as? CoreBridge.MaintenanceFailure)
                let safety = try #require(failure.safetyBackup)
                #expect(files.fileExists(atPath: location.backupsDirectory.appending(path: safety).path))
                #expect(failure.candidateActive == afterActivation)
                if afterActivation {
                    guard case .io = failure.cause else { Issue.record("Expected reevaluation I/O failure"); try await collections.close(); return }
                    #expect(failure.generation == generation + 1)
                    #expect(failure.restored?.generation == generation + 1 && failure.restored?.cloudPublicationPending == true)
                    #expect(failure.restored?.name == backup.name && failure.restored?.safetyBackup == safety)
                } else {
                    #expect(failure.cause == .cancelled && failure.generation == nil && failure.restored == nil)
                }
            }
            #expect(try await collections.lifecycle() == .ready(generation: afterActivation ? generation + 1 : generation))
        } catch {
            setupRelease.signal()
            await operation.cancel()
            try? await ControlLane.shared.releaseMaintenanceBoundary(gate)
            _ = await worker.result
            try? await ControlLane.shared.resetMaintenanceBoundary(gate)
            throw error
        }
        updateReady.finish(); handleReady.finish()
        try await collections.close()
    } catch {
        try? await collections.close()
        throw error
    }
}

// Build boundary closures on their lane, as bootstrapCollection does. MainActor
// fixtures send only the generated Sendable gate and value parameters.
private extension MaintenanceLane {
    func armMaintenanceBoundary(afterActivation: Bool, failReevaluation: Bool) throws -> MaintenanceTestGate {
        try call("test_arm_maintenance_boundary") {
            try testArmMaintenanceBoundary(boundary: afterActivation ? .afterActivation : .beforeRename, failReevaluation: failReevaluation)
        }
    }
    func notesPageOffset() throws -> UInt64 {
        try call("test_maintenance_notes_page_offset") { try testMaintenanceNotesPageOffset() }
    }
}
private extension QueryLane {
    func awaitMaintenanceBoundary(_ gate: MaintenanceTestGate) throws {
        try call("MaintenanceTestGate.wait_until_reached") { try gate.waitUntilReached() }
    }
}
private extension ControlLane {
    func releaseMaintenanceBoundary(_ gate: MaintenanceTestGate) throws {
        try call("MaintenanceTestGate.release") { try gate.release() }
    }
    func resetMaintenanceBoundary(_ gate: MaintenanceTestGate) throws {
        try call("MaintenanceTestGate.reset") { try gate.reset() }
    }
}

extension EventTests {
    @Test @MainActor func maintenanceServiceUncleanPageDamageAndPublicRestoreRecoversOnEveryPlatform() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try files.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
        try files.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let clock = MaintenanceClock()
        let collections = CoreCollectionService(clock: clock, diskSpace: MaintenanceSpace())
        let service = CoreMaintenanceService()
        try await collections.open(location, randomSeed: 133)
        do {
            let backup = try await service.createBackup(operation: MaintenanceOperation(), progress: { _ in })
            let backupFile = location.backupsDirectory.appending(path: backup.name)
            let originalBackup = try Data(contentsOf: backupFile)
            let offset = try await MaintenanceLane.shared.notesPageOffset()
            try await collections.close()
            let sidecar = location.collectionDirectory.appending(path: "meguri-sidecar.json")
            var document = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: sidecar)) as? [String: Any])
            document["clean_shutdown"] = false
            try JSONSerialization.data(withJSONObject: document).write(to: sidecar, options: .atomic)
            let database = location.collectionDirectory.appending(path: "collection.anki2")
            let file = try FileHandle(forWritingTo: database)
            do {
                try file.seek(toOffset: offset)
                try file.write(contentsOf: Data([0]))
                try file.synchronize()
                try file.close()
            } catch { try? file.close(); throw error }
            try await collections.open(location, randomSeed: 133)
            let generation = try await collections.waitAvailable(timeoutMilliseconds: 0)
            _ = try await service.runDueMaintenance(.opened, operation: MaintenanceOperation(), progress: { _ in })
            guard case .failed(.corrupt) = try await collections.lifecycle() else {
                Issue.record("Real damaged notes page must retain FailedCorrupt")
                try await collections.close()
                return
            }
            let status = try await service.status()
            #expect(status.lastQuickCheck?.passed == false)
            #expect(status.remedies == [.restore])
            let limits = CoreBridge.BackupLimits(daily: 12, weekly: 10, monthly: 9, minimumIntervalMinutes: 30)
            do {
                _ = try await service.setBackupLimits(limits)
                Issue.record("FailedCorrupt must refuse a real settings write")
            } catch {
                #expect(error as? BridgeError == .maintenance(.collectionCorrupt))
            }
            // FailedCorrupt refuses writes: these are the raw inputs to T7's copy.
            let damagedMain = try Data(contentsOf: database)
            #expect(damagedMain[Int(offset)] == 0)
            let walFile = location.collectionDirectory.appending(path: "collection.anki2-wal")
            let damagedWAL = files.fileExists(atPath: walFile.path) ? try Data(contentsOf: walFile) : nil
            clock.advance() // Damaged safety bytes must have a distinct wall name.
            let restored = try await service.restoreBackup(name: backup.name, operation: MaintenanceOperation(), progress: { _ in })
            #expect(restored.generation == generation + 1 && restored.cloudPublicationPending)
            #expect(try await collections.lifecycle() == .ready(generation: generation + 1))
            let safety = try #require(restored.safetyBackup)
            #expect(restored.name == backup.name)
            #expect(safety == "corrupt-2030-01-15-10.00.01.anki2")
            let safetyFile = location.backupsDirectory.appending(path: safety)
            #expect(files.fileExists(atPath: safetyFile.path))
            #expect(try Data(contentsOf: safetyFile) == damagedMain)
            let safetyWAL = location.backupsDirectory.appending(path: safety + "-wal")
            if let damagedWAL, !damagedWAL.isEmpty {
                #expect(try Data(contentsOf: safetyWAL) == damagedWAL)
            } else {
                #expect(!files.fileExists(atPath: safetyWAL.path))
            }
            let listing = try await service.listBackups()
            #expect(!listing.contains { $0.name == safety || $0.name == safety + "-wal" })
            #expect(listing.contains { $0.name == backup.name && $0.kind == .backup })
            #expect(try Data(contentsOf: backupFile) == originalBackup)
            #expect(try await service.backupLimits() == limits)
            try await collections.close()
            for await event in CoreEventStream.shared.events {
                if event.generation == generation + 1 && event.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await collections.close()
            throw error
        }
    }

    @Test @MainActor func maintenanceServiceRealListingIOFailureRetainsTypedDomain() async throws {
        let filesystem = FileManager.default
        let sandbox = filesystem.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? filesystem.removeItem(at: sandbox) }
        let paths = CoreBridge.CollectionLocation(
            collectionDirectory: sandbox.appending(path: "collection"),
            backupsDirectory: sandbox.appending(path: "backups"),
            appGroupSnapshot: sandbox.appending(path: "widget.snapshot"),
            manualTransferDirectory: sandbox.appending(path: "ManualTransfers"))
        for directory in [paths.collectionDirectory, paths.backupsDirectory] {
            try filesystem.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        try await MaintenanceLane.shared.bootstrapCollection(paths)
        let owner = CoreCollectionService(clock: MaintenanceClock(), diskSpace: MaintenanceSpace())
        let maintenance = CoreMaintenanceService()
        try await owner.open(paths, randomSeed: 133)
        do {
            let openedGeneration = try await owner.waitAvailable(timeoutMilliseconds: 0)
            let heldDirectory = sandbox.appending(path: "backup-directory-held")
            try filesystem.moveItem(at: paths.backupsDirectory, to: heldDirectory)
            var refusal: (any Error)?
            do {
                try Data([1]).write(to: paths.backupsDirectory)
                do {
                    _ = try await maintenance.listBackups()
                    Issue.record("Expected backup listing I/O failure")
                } catch {
                    refusal = error
                }
                try filesystem.removeItem(at: paths.backupsDirectory)
                try filesystem.moveItem(at: heldDirectory, to: paths.backupsDirectory)
            } catch {
                // Return ownership of the original directory before closing.
                try? filesystem.removeItem(at: paths.backupsDirectory)
                try filesystem.moveItem(at: heldDirectory, to: paths.backupsDirectory)
                throw error
            }
            let failure = try #require(refusal)
            guard let bridgeFailure = failure as? BridgeError,
                  case .maintenance(.io(let message)) = bridgeFailure else {
                Issue.record("Directory enumeration lost its maintenance I/O category: \(failure)")
                throw failure
            }
            #expect(!message.isEmpty)
            #expect(try await maintenance.listBackups().isEmpty)
            #expect(try await owner.lifecycle() == .ready(generation: openedGeneration))
            try await owner.close()
            for await notification in CoreEventStream.shared.events {
                if notification.generation == openedGeneration && notification.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await owner.close()
            throw error
        }
    }
}
