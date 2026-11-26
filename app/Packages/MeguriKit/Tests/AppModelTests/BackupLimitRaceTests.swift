import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor @Suite struct BackupLimitRaceTests {
    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func heldLimitReadFinishingAfterSuccessfulEditCannotReplaceConfirmedValues(duringWrite: Bool) async throws {
        let read = BackupRaceGate(), write = BackupRaceGate()
        defer { read.release(); write.release() }
        let service = HeldBackupLimits(read: read, write: write), host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        await service.holdNextRead()
        var edit: Task<Void, Never>?
        if duringWrite {
            await service.holdNextWrite()
            edit = Task { await model.editLimit(.weekly, value: 4) }
            try await write.waitUntilHeld()
        }
        let refresh = Task { await model.refresh() }
        try await read.waitUntilHeld()
        if let edit { write.release(); await edit.value }
        else { await model.editLimit(.weekly, value: 4) }
        let confirmed = BackupLimits(daily: .max, weekly: 4, monthly: 9, minimumIntervalMinutes: .max)
        #expect(!model.savingLimits && model.limits == confirmed)
        read.release(); await refresh.value
        #expect(model.limits == confirmed)
        #expect(await service.written == [confirmed])
        // A later, uncontended refresh still accepts synced changes.
        await service.remoteWeekly(7)
        await model.refresh()
        #expect(model.limits?.weekly == 7)
    }

    @Test(.timeLimit(.minutes(1)))
    func cachedLimitsEditKeepsRemoteMembersAndDisplaysConfirmedStoredLimits() async throws {
        let write = BackupRaceGate()
        defer { write.release() }
        let service = HeldBackupLimits(read: BackupRaceGate(), write: write)
        let host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        #expect(model.limits?.monthly == 9)
        await service.holdNextWrite()
        let edit = Task { await model.editLimit(.weekly, value: 4) }
        try await write.waitUntilHeld()
        await service.remoteMonthly(23)
        write.release()
        await edit.value
        let confirmed = BackupLimits(daily: .max, weekly: 4, monthly: 23, minimumIntervalMinutes: .max)
        #expect(model.limits == confirmed)
        #expect(await service.written == [confirmed])
        #expect(try await service.backupLimits() == confirmed)
    }

    @Test func untouchedDraftAdoptsRemoteRefreshWithoutSavingOrValidatingLegacyValues() {
        var draft = BackupLimitDraft()
        draft.reset(to: 10)
        #expect(draft.commit(value: 20, minimum: 0) == nil)
        #expect(draft.text == "20" && !draft.invalid)
        draft.reset(to: .max)
        #expect(draft.commit(value: .max, minimum: 5) == nil)
        #expect(!draft.invalid && draft.text == String(UInt32.max))
        draft.reset(to: 1) // Legacy interval below the UI's minimum.
        #expect(draft.commit(value: 1, minimum: 5) == nil && !draft.invalid)
        draft.text = "2"
        #expect(draft.commit(value: 1, minimum: 5) == nil && draft.invalid)
        draft.text = "5"
        #expect(draft.commit(value: 1, minimum: 5) == 5 && !draft.invalid)
    }

    @Test func actualDraftEditSavesOnlyItsMemberAndSubmitThenDefocusDoesNotSaveTwice() async {
        let service = HeldBackupLimits(read: BackupRaceGate(), write: BackupRaceGate()), host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        var draft = BackupLimitDraft()
        draft.reset(to: 10)
        draft.text = "4"
        if let value = draft.commit(value: 10, minimum: 0) { await model.editLimit(.weekly, value: value) }
        #expect(draft.commit(value: 4, minimum: 0) == nil)
        #expect(await service.written == [.init(daily: .max, weekly: 4, monthly: 9, minimumIntervalMinutes: .max)])
        draft.reset(to: 10)
        draft.text = " 10 "
        #expect(draft.commit(value: 20, minimum: 0) == nil) // No numeric edit to the original.
        #expect(draft.text == "20")
        draft.text = "12"
        #expect(draft.commit(value: 30, minimum: 0) == 12) // An explicit edit survives a refresh.
    }
}

private actor HeldBackupLimits: MaintenanceService {
    let read: BackupRaceGate, write: BackupRaceGate
    var limits = BackupLimits(daily: .max, weekly: 10, monthly: 9, minimumIntervalMinutes: .max)
    var written: [BackupLimits] = []
    private var holdRead = false, holdWrite = false
    init(read: BackupRaceGate, write: BackupRaceGate) { self.read = read; self.write = write }
    func holdNextRead() { holdRead = true }
    func holdNextWrite() { holdWrite = true }
    func remoteWeekly(_ value: UInt32) { limits.weekly = value }
    func remoteMonthly(_ value: UInt32) { limits.monthly = value }
    func backupLimits() async throws -> BackupLimits {
        let snapshot = limits
        if holdRead { holdRead = false; await read.hold() }
        return snapshot
    }
    func setBackupLimits(_ value: BackupLimits) async throws -> Bool {
        if holdWrite { holdWrite = false; await write.hold() }
        limits = value; written.append(value); return true
    }
    func listBackups() async throws -> [BackupEntry] { [] }
    func updateBackupLimit(_ patch: BackupLimitPatch) async throws -> BackupLimits {
        if holdWrite { holdWrite = false; await write.hold() }
        switch patch {
        case .daily(let value): limits.daily = value
        case .weekly(let value): limits.weekly = value
        case .monthly(let value): limits.monthly = value
        case .minimumIntervalMinutes(let value): limits.minimumIntervalMinutes = value
        }
        written.append(limits)
        return limits
    }
    func status() async throws -> MaintenanceStatus { throw MaintenanceError.collectionUnavailable }
    func createBackup(operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ImportBackupReceipt { throw MaintenanceError.collectionUnavailable }
    func restoreBackup(name: String, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> RestoredBackup { throw MaintenanceError.collectionUnavailable }
    func runDueMaintenance(_ trigger: MaintenanceTrigger, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> Int64? { nil }
}

@MainActor private final class LimitRaceHost: BackupHost {
    let backupGeneration: UInt64 = 1
    let backupsWritable = true, backupsAvailable = true, backupCancellationSafe = true
    func prepareBackups() async throws {}
    func beginBackupRestore() async throws {}
    func cancelBackupRestoreAdmission() {}
    func finishBackupRestore(candidateActive: Bool?, generation: UInt64?) async -> BackupRestoreCompletion {
        .init(candidateActive: candidateActive, ready: true)
    }
    func backupRestoreSucceeded() {}
}
