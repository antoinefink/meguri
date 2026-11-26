import CoreBridge
import Foundation
import Testing
@testable import AppModel

private actor BackupServiceProbe: MaintenanceService {
    var entries: [BackupEntry] = []
    var limits = BackupLimits(daily: .max, weekly: 10, monthly: 9, minimumIntervalMinutes: .max)
    var written: [BackupLimits] = []
    var restoredNames: [String] = []
    var triggers: [MaintenanceTrigger] = []
    var refusal: MaintenanceFailure?
    var deadline: Int64?
    var held = false
    var started = false
    var startWaiters: [CheckedContinuation<Void, Never>] = []
    var releaseWaiter: CheckedContinuation<Void, Never>?
    var beforeRestore: (@Sendable () async -> Void)?
    func configure(entries: [BackupEntry], refusal: MaintenanceFailure? = nil) {
        self.entries = entries; self.refusal = refusal
    }
    func configureDue(deadline: Int64?, held: Bool = false) {
        self.deadline = deadline; self.held = held; started = false
    }
    func beforeRestore(_ action: @escaping @Sendable () async -> Void) { beforeRestore = action }
    func waitForStart() async {
        if started { return }
        await withCheckedContinuation { startWaiters.append($0) }
    }
    func release() { held = false; releaseWaiter?.resume(); releaseWaiter = nil }
    private func reachGate() async {
        started = true
        let waiting = startWaiters; startWaiters.removeAll()
        waiting.forEach { $0.resume() }
        if held { await withCheckedContinuation { releaseWaiter = $0 } }
    }
    func createBackup(operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ImportBackupReceipt {
        throw MaintenanceError.diskFull
    }
    func restoreBackup(name: String, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> RestoredBackup {
        restoredNames.append(name)
        await reachGate()
        await beforeRestore?()
        if let refusal { throw refusal }
        return RestoredBackup(name: name, safetyBackup: "backup-predecessor.colpkg", generation: 2,
            cloudPublicationPending: true, deadlineMilliseconds: 300_000)
    }
    func runDueMaintenance(_ trigger: MaintenanceTrigger, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> Int64? {
        triggers.append(trigger)
        await reachGate()
        return deadline
    }
    func listBackups() async throws -> [BackupEntry] { entries }
    func backupLimits() async throws -> BackupLimits { limits }
    func setBackupLimits(_ limits: BackupLimits) async throws -> Bool { self.limits = limits; written.append(limits); return true }
    func status() async throws -> MaintenanceStatus { throw MaintenanceError.collectionUnavailable }
}

@MainActor private final class BackupHostProbe: BackupHost {
    var backupGeneration: UInt64 = 1
    var backupsWritable = true
    var backupsAvailable = true
    var backupCancellationSafe = true
    var prepared = 0
    var admissionFailure: MaintenanceError?
    var finishes: [(active: Bool, generation: UInt64?)] = []
    var succeeded = false
    func prepareBackups() async throws { prepared += 1 }
    func beginBackupRestore() async throws { if let admissionFailure { throw admissionFailure } }
    func cancelBackupRestoreAdmission() {}
    func finishBackupRestore(candidateActive: Bool?, generation: UInt64?) async -> BackupRestoreCompletion {
        finishes.append((candidateActive == true, generation))
        if candidateActive == true, let generation { backupGeneration = generation }
        return BackupRestoreCompletion(candidateActive: candidateActive, ready: true)
    }
    func backupRestoreSucceeded() { succeeded = true }
}

@MainActor @Suite struct BackupCoordinatorTests {
    private func entry(_ day: UInt8, kind: BackupKind = .backup) -> BackupEntry {
        BackupEntry(name: "backup-2030-01-\(day)-09.00.00.colpkg", kind: kind,
            wallTime: .init(year: 2030, month: 1, day: day, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
    }
    @Test func offlineListSkipsUnverifiedAndRestoresTheSelectedOlderName() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe()
        let newest = entry(3), older = entry(2), unverified = entry(4, kind: .serverCopyUnverified)
        await service.configure(entries: [unverified, newest, older])
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore()
        #expect(host.prepared == 1 && model.selectedName == newest.name)
        model.selectedName = unverified.name
        #expect(model.selectedEntry == nil)
        model.selectedName = older.name
        let restore = try #require(model.restoreConfirmed(name: older.name))
        await restore.value
        #expect(await service.restoredNames == [older.name])
        #expect(host.finishes.count == 1 && host.finishes.first?.active == true)
        #expect(model.safetyBackup == "backup-predecessor.colpkg")
        #expect(model.restoreCompleted && host.succeeded)
    }
    @Test func rejectedCandidateKeepsRecoveryAndSelectsAnotherButSafetyFailureKeepsChoice() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe()
        host.backupsWritable = false
        let newest = entry(3), older = entry(2)
        await service.configure(entries: [newest, older], refusal: .init(cause: .corrupt))
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore()
        #expect(!model.canCreate && model.canOpenRestore)
        let first = try #require(model.restoreConfirmed(name: newest.name))
        await first.value
        #expect(!model.restoreCompleted && !host.succeeded)
        #expect(model.selectedName == older.name && model.failure == .corrupt)
        #expect(host.finishes.first?.active == false)
        await service.configure(entries: [newest, older], refusal: .init(cause: .safetyBackupFailed))
        let second = try #require(model.restoreConfirmed(name: older.name))
        await second.value
        #expect(model.failure == .safetyBackupFailed && model.selectedName == older.name)
        #expect(!model.rejectedNames.contains(older.name))
        await service.configure(entries: [newest, older])
        let third = try #require(model.restoreConfirmed(name: older.name))
        await third.value
        #expect(host.succeeded)
    }
    @Test func refreshRemovesStaleConsentAndSessionRefusalDoesNotCallRestore() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe(), older = entry(2)
        await service.configure(entries: [entry(3), older])
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore(); model.selectedName = older.name
        await service.configure(entries: [entry(3)])
        await model.refresh()
        #expect(model.restoreConfirmed(name: older.name) == nil)
        host.admissionFailure = .sessionActive
        let restore = try #require(model.restoreConfirmed(name: entry(3).name))
        await restore.value
        #expect(model.failure == .sessionActive && host.finishes.isEmpty)
        #expect(await service.restoredNames.isEmpty)
    }
    @Test func activatedTypedFailureSurvivesReplacementEventAndDoesNotClaimSuccess() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe(), selected = entry(2)
        let model = BackupCoordinator(host: host, service: service)
        await service.configure(entries: [selected], refusal: .init(cause: .io("Synthetic final-step failure"),
            candidateActive: true, generation: 2, safetyBackup: "kept-predecessor.colpkg"))
        await service.beforeRestore { @MainActor in
            host.backupGeneration = 2
            model.collectionChanged()
        }
        await model.openedRestore()
        let restore = try #require(model.restoreConfirmed(name: selected.name))
        await restore.value
        #expect(model.candidateActive && !model.restoreCompleted)
        #expect(model.safetyBackup == "kept-predecessor.colpkg" && !host.succeeded)
        #expect(host.finishes.first?.active == true && host.finishes.first?.generation == 2)
    }
    @Test func limitsEditPreservesEveryUntouchedOutOfRangeIntegerAndEmptyListIsUsable() async {
        let host = BackupHostProbe(), service = BackupServiceProbe()
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore()
        #expect(model.entries.isEmpty && model.selectedEntry == nil && model.canOpenRestore)
        await model.editLimit(.weekly, value: 4)
        #expect(await service.written == [.init(daily: .max, weekly: 4, monthly: 9, minimumIntervalMinutes: .max)])
        await model.editLimit(.weekly, value: 4)
        #expect(await service.written.count == 1)
        let create = model.create()
        await create?.value
        #expect(model.failure == .diskFull && !model.busy)
    }
    @Test func browserDestinationHasNoCredentialsQueryOrFragment() {
        #expect(BackupCoordinator.accountBackupsURL(endpoint: "https://sync.example.test/native?secret=removed#token")?.absoluteString
            == "https://sync.example.test/collection")
        #expect(BackupCoordinator.accountBackupsURL(endpoint: "https://user:password@sync.example.test") == nil)
        #expect(BackupCoordinator.accountBackupsURL(endpoint: "file:///tmp/collection") == nil)
    }
    @Test func cancellationCannotHideAnActivatedFailure() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe(), selected = entry(2)
        await service.configure(entries: [selected], refusal: .init(cause: .cancelled,
            candidateActive: true, generation: 2, safetyBackup: "kept-safety.colpkg"))
        await service.configureDue(deadline: nil, held: true)
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore()
        let restore = try #require(model.restoreConfirmed(name: selected.name))
        await service.waitForStart()
        host.backupCancellationSafe = false
        #expect(!model.canCancel)
        restore.cancel()
        await service.release(); await restore.value
        #expect(model.candidateActive && model.safetyBackup == "kept-safety.colpkg")
        #expect(host.finishes.first?.active == true && !model.restoreCompleted)
    }
}

@MainActor @Suite struct BackupTriggerTests {
    @Test func anImmediateDeadlineFiresOnceAndUsesTheCoreReschedule() async throws {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: 5000)
        var fire: (@MainActor () -> Void)?
        var scheduled: [Date] = []
        let trigger = BackupTrigger(service: service, schedule: { date, action in
            scheduled.append(date); fire = action; return {}
        })
        trigger.own(1); trigger.event(deadline: 0)
        let immediate = try #require(fire)
        immediate()
        await service.waitForStart(); await trigger.drain()
        #expect(await service.triggers == [.deadline])
        #expect(scheduled == [Date(timeIntervalSince1970: 0), Date(timeIntervalSince1970: 5)])
        trigger.own(nil)
    }
    @Test func deadlineReplacementNilAndCloseCancelExactlyOneTimer() {
        var dates: [Date] = [], cancelled = 0
        let trigger = BackupTrigger(service: BackupServiceProbe(), schedule: { date, _ in
            dates.append(date); return { cancelled += 1 }
        })
        trigger.own(1)
        trigger.event(deadline: 0)
        trigger.event(deadline: 2000)
        #expect(dates == [Date(timeIntervalSince1970: 0), Date(timeIntervalSince1970: 2)] && cancelled == 1)
        trigger.event(deadline: nil)
        #expect(cancelled == 2 && trigger.deadlineMilliseconds == nil)
        trigger.event(deadline: 3000)
        trigger.own(nil)
        trigger.event(deadline: 4000)
        #expect(cancelled == 3 && dates.count == 3 && trigger.deadlineMilliseconds == nil)
    }
    @Test func eventWinsOverAnInFlightDeadlineResultAndReplacementDiscardsTerminalTimer() async {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: 4000, held: true)
        let trigger = BackupTrigger(service: service, schedule: { _, _ in {} })
        trigger.own(1)
        let running = Task { await trigger.run(.opened) }
        await service.waitForStart()
        trigger.event(deadline: 9000)
        await service.release(); await running.value
        #expect(trigger.deadlineMilliseconds == 9000)
        await service.configureDue(deadline: 6000, held: true)
        let retired = Task { await trigger.run(.deadline) }
        await service.waitForStart()
        trigger.own(2)
        trigger.event(deadline: 12000)
        await service.release(); await retired.value
        #expect(trigger.deadlineMilliseconds == 12000)
        trigger.own(nil)
    }
    @Test func coreNilLeavesNoTimerAndLifecycleTriggersAreForwarded() async {
        let service = BackupServiceProbe()
        var scheduled = 0
        let trigger = BackupTrigger(service: service, schedule: { _, _ in scheduled += 1; return {} })
        trigger.own(1)
        await trigger.run(.opened)
        await trigger.run(.background)
        await trigger.run(.quit)
        #expect(await service.triggers == [.opened, .background, .quit])
        #expect(scheduled == 0 && trigger.deadlineMilliseconds == nil)
        trigger.own(nil)
    }
}
