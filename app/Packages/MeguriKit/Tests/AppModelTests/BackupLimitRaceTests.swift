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
    func refusedLimitEditReadsRemoteValuesAndFencesConcurrentSnapshot() async throws {
        let read = BackupRaceGate(), write = BackupRaceGate()
        let service = HeldBackupLimits(read: read, write: write), host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        await service.holdNextRead()
        let refresh = Task { await model.refresh() }
        var edit: Task<Void, Never>?
        do {
            try await read.waitUntilHeld()
            await service.holdNextWrite()
            await service.refuseNextWrite()
            let pending = Task { await model.editLimit(.weekly, value: 4) }
            edit = pending
            try await write.waitUntilHeld()
            await service.remoteWeekly(7)
            await service.remoteMonthly(23)
            write.release(); await pending.value
            let remote = BackupLimits(daily: .max, weekly: 7, monthly: 23, minimumIntervalMinutes: .max)
            #expect(model.limits == remote && model.failure == .diskFull && !model.savingLimits)
            #expect(await service.written.isEmpty)
            read.release(); await refresh.value
            #expect(model.limits == remote && model.failure == .diskFull)
            // Refusal does not close admission to a later genuine edit.
            await model.editLimit(.weekly, value: 4)
            #expect(model.limits?.weekly == 4 && model.limits?.monthly == 23)
        } catch {
            read.release(); write.release(); await refresh.value; await edit?.value
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true], [false, true])
    func lateLimitResultCannotPublishAfterOwnerChanges(refused: Bool, generationOnly: Bool) async throws {
        let read = BackupRaceGate(), write = BackupRaceGate()
        let service = HeldBackupLimits(read: read, write: write), host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        if refused { await service.refuseNextWrite(); await service.holdNextRead() }
        else { await service.holdNextWrite() }
        let edit = Task { await model.editLimit(.weekly, value: 4) }
        do {
            // Hold either the successful write or the refused write's fresh read.
            try await (refused ? read : write).waitUntilHeld()
            if generationOnly { host.backupGeneration += 1 }
            else { model.collectionChanged() }
            await service.remoteWeekly(8)
            await service.remoteMonthly(23)
            let before = model.limits
            read.release(); write.release(); await edit.value
            #expect(model.limits == before && !model.savingLimits)
            if refused, generationOnly { #expect(model.failure == .diskFull) }
            await model.refresh()
            #expect(model.limits?.monthly == 23)
        } catch {
            read.release(); write.release(); await edit.value
            throw error
        }
    }

    @Test func failedLimitReadPreservesWriteFailureAndLastKnownLimits() async {
        let service = HeldBackupLimits(read: BackupRaceGate(), write: BackupRaceGate()), host = LimitRaceHost()
        let model = BackupCoordinator(host: host, service: service)
        await model.refresh()
        let previous = model.limits
        await service.refuseNextWrite()
        await service.refuseNextRead()
        await model.editLimit(.weekly, value: 4)
        #expect(model.limits == previous && model.failure == .diskFull && !model.savingLimits)
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
        draft.text = " 1 "
        #expect(draft.commit(value: 1, minimum: 5) == nil && !draft.invalid)
        draft.text = " 1 "
        #expect(draft.commit(value: 8, minimum: 5) == nil && !draft.invalid && draft.text == "8")
        draft.reset(to: .max)
        draft.text = " \(UInt32.max) "
        #expect(draft.commit(value: 9, minimum: 5) == nil && !draft.invalid && draft.text == "9")
        draft.reset(to: 1)
        draft.text = "2"
        #expect(draft.commit(value: 1, minimum: 5) == nil && draft.invalid)
        #expect(draft.commit(value: 2, minimum: 5) == nil && draft.invalid) // Synced value is not the edit baseline.
        draft.text = "10000"
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
    private var refuseWrite = false, refuseRead = false
    init(read: BackupRaceGate, write: BackupRaceGate) { self.read = read; self.write = write }
    func holdNextRead() { holdRead = true }
    func holdNextWrite() { holdWrite = true }
    func refuseNextWrite() { refuseWrite = true }
    func refuseNextRead() { refuseRead = true }
    func remoteWeekly(_ value: UInt32) { limits.weekly = value }
    func remoteMonthly(_ value: UInt32) { limits.monthly = value }
    func backupLimits() async throws -> BackupLimits {
        if refuseRead { refuseRead = false; throw MaintenanceError.collectionUnavailable }
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
        if refuseWrite { refuseWrite = false; throw MaintenanceError.diskFull }
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
    var backupGeneration: UInt64 = 1
    let backupsWritable = true, backupsAvailable = true, backupCancellationSafe = true
    func prepareBackups() async throws {}
    func beginBackupRestore() async throws {}
    func cancelBackupRestoreAdmission() {}
    func finishBackupRestore(candidateActive: Bool?, generation: UInt64?) async -> BackupRestoreCompletion {
        .init(candidateActive: candidateActive, ready: true)
    }
    func backupRestoreSucceeded() {}
}
