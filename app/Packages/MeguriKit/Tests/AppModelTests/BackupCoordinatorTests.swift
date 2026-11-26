@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

actor BackupServiceProbe: MaintenanceService {
    var entries: [BackupEntry] = []
    var limits = BackupLimits(daily: .max, weekly: 10, monthly: 9, minimumIntervalMinutes: .max)
    var written: [BackupLimits] = []
    var restoredNames: [String] = []
    var triggers: [MaintenanceTrigger] = []
    var dueOperation: MaintenanceOperation?
    var restoreOperation: MaintenanceOperation?
    var refusal: MaintenanceFailure?
    var listFailure: MaintenanceError?
    var limitsFailure: MaintenanceError?
    var limitEditFailure: MaintenanceError?
    var deadline: Int64?
    var held = false
    var started = false
    var startWaiters: [UUID: AsyncStream<Void>.Continuation] = [:]
    var releaseWaiters: [CheckedContinuation<Void, Never>] = []
    var beforeRestore: (@Sendable () async -> Void)?
    func configure(entries: [BackupEntry], refusal: MaintenanceFailure? = nil) {
        self.entries = entries; self.refusal = refusal
    }
    func configureReadFailures(list: MaintenanceError? = nil, limits: MaintenanceError? = nil, editLimit: MaintenanceError? = nil) {
        listFailure = list; limitsFailure = limits; limitEditFailure = editLimit
    }
    func configureDue(deadline: Int64?, held: Bool = false) {
        self.deadline = deadline; self.held = held; started = false
    }
    func beforeRestore(_ action: @escaping @Sendable () async -> Void) { beforeRestore = action }
    func waitForStart(timeout: Duration = .seconds(10)) async throws {
        try Task.checkCancellation()
        if started { return }
        let id = UUID(), signal = AsyncStream<Void>.makeStream()
        startWaiters[id] = signal.continuation
        defer { startWaiters.removeValue(forKey: id)?.finish() }
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask {
                for await _ in signal.stream { try Task.checkCancellation(); return }
                throw CancellationError()
            }
            group.addTask {
                try await Task.sleep(for: timeout)
                throw BridgeError.internal("Timed out waiting for backup service admission")
            }
            defer { group.cancelAll() }
            _ = try await group.next()
            try Task.checkCancellation()
        }
    }
    func release() {
        held = false
        let waiting = releaseWaiters; releaseWaiters.removeAll()
        waiting.forEach { $0.resume() }
    }
    private func reachGate() async {
        started = true
        let waiting = startWaiters; startWaiters.removeAll()
        waiting.values.forEach { $0.yield(()); $0.finish() }
        if held { await withCheckedContinuation { releaseWaiters.append($0) } }
    }
    func createBackup(operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ImportBackupReceipt {
        throw MaintenanceError.diskFull
    }
    func restoreBackup(name: String, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> RestoredBackup {
        restoredNames.append(name)
        restoreOperation = operation
        await reachGate()
        await beforeRestore?()
        if let refusal { throw refusal }
        return RestoredBackup(name: name, safetyBackup: "backup-predecessor.colpkg", generation: 2,
            cloudPublicationPending: true, deadlineMilliseconds: 300_000)
    }
    func runDueMaintenance(_ trigger: MaintenanceTrigger, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> Int64? {
        triggers.append(trigger)
        dueOperation = operation
        await reachGate()
        return deadline
    }
    func requireOperationNotCancelled() throws {
        try dueOperation?.check()
    }
    func waitForOperationCancellation() async throws {
        let limit = ContinuousClock.now.advanced(by: .seconds(4))
        while ContinuousClock.now < limit {
            if let dueOperation {
                do { try dueOperation.check() } catch BridgeError.cancelled { return }
            }
            try await Task.sleep(for: .milliseconds(1))
        }
        throw BridgeError.internal("Automatic maintenance operation was not cancelled")
    }
    func listBackups() async throws -> [BackupEntry] {
        if let listFailure { throw listFailure }
        return entries
    }
    func backupLimits() async throws -> BackupLimits {
        if let limitsFailure { throw limitsFailure }
        return limits
    }
    func setBackupLimits(_ limits: BackupLimits) async throws -> Bool { self.limits = limits; written.append(limits); return true }
    func updateBackupLimit(_ patch: BackupLimitPatch) async throws -> BackupLimits {
        if let limitEditFailure { throw limitEditFailure }
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

@MainActor @Suite struct BackupServiceProbeTests {
    @Test(.timeLimit(.minutes(1)))
    func oneReleaseJoinsHeldMaintenanceAndRestore() async throws {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: 4_000, held: true)
        let maintenanceOperation = MaintenanceOperation(), restoreOperation = MaintenanceOperation()
        let name = "backup-2030-01-02-09.00.00.colpkg"
        var maintenanceEnded = false, restoreEnded = false
        let maintenance = Task {
            defer { maintenanceEnded = true }
            return try await service.runDueMaintenance(.opened, operation: maintenanceOperation, progress: { _ in })
        }
        let restore = Task {
            defer { restoreEnded = true }
            return try await service.restoreBackup(name: name, operation: restoreOperation, progress: { _ in })
        }
        let admissionLimit = ContinuousClock.now.advanced(by: .seconds(4))
        while await service.releaseWaiters.count < 2, ContinuousClock.now < admissionLimit { await Task.yield() }
        // These exact calls must both be suspended before the single release.
        #expect(await service.releaseWaiters.count == 2)
        #expect(await service.held)
        #expect(await service.dueOperation === maintenanceOperation)
        #expect(await service.restoreOperation === restoreOperation)
        #expect(await service.triggers == [.opened])
        #expect(await service.restoredNames == [name])
        #expect(!maintenanceEnded && !restoreEnded)
        await service.release()
        let completionLimit = ContinuousClock.now.advanced(by: .seconds(4))
        while !(maintenanceEnded && restoreEnded), ContinuousClock.now < completionLimit { await Task.yield() }
        let completed = maintenanceEnded && restoreEnded
        if !completed {
            // Cancellation cannot recover a lost continuation. Join only work
            // observed to finish, so a broken release fails within the bound.
            maintenance.cancel(); restore.cancel()
            if maintenanceEnded { _ = await maintenance.result }
            if restoreEnded { _ = await restore.result }
        }
        try #require(completed, "One release must finish both held operations")
        let maintenanceResult = await maintenance.result
        let restoreResult = await restore.result
        #expect(try maintenanceResult.get() == 4_000)
        #expect(try restoreResult.get() == RestoredBackup(name: name, safetyBackup: "backup-predecessor.colpkg",
            generation: 2, cloudPublicationPending: true, deadlineMilliseconds: 300_000))
        #expect(await service.releaseWaiters.isEmpty)
    }

    @Test(.timeLimit(.minutes(1)))
    func missingStartTimesOutAndCleanupReleasesAndJoinsDelayedWork() async throws {
        let service = BackupServiceProbe(), admission = BackupRaceGate()
        await service.configureDue(deadline: nil, held: true)
        var ended = false
        let pending = Task {
            await admission.hold()
            _ = try? await service.runDueMaintenance(.opened, operation: MaintenanceOperation(), progress: { _ in })
            ended = true
        }
        do {
            try await admission.waitUntilHeld()
            do {
                try await service.waitForStart(timeout: .zero)
                Issue.record("Missing service admission must fail")
            } catch BridgeError.internal(let detail) {
                #expect(detail == "Timed out waiting for backup service admission")
            }
            #expect(await service.startWaiters.isEmpty)
            #expect(!ended)
        } catch {
            await service.release(); admission.release(); await pending.value
            throw error
        }
        // Release even a not-yet-admitted operation before joining its owner.
        await service.release(); admission.release(); await pending.value
        let triggers = await service.triggers
        #expect(ended && triggers == [.opened])
        try await service.waitForStart() // Already-started success stays immediate.
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func startWaitHonorsCancellationAndRetiresItsWaiter(alreadyStarted: Bool) async throws {
        let service = BackupServiceProbe()
        if alreadyStarted {
            _ = try await service.runDueMaintenance(.opened, operation: MaintenanceOperation(), progress: { _ in })
        }
        let waiting = Task { try await service.waitForStart() }
        do {
            if !alreadyStarted {
                let deadline = ContinuousClock.now.advanced(by: .seconds(4))
                while await service.startWaiters.isEmpty, ContinuousClock.now < deadline {
                    try await Task.sleep(for: .milliseconds(1))
                }
                try #require(await service.startWaiters.count == 1)
            }
            waiting.cancel()
            switch await waiting.result {
            case .success: Issue.record("Cancelled start wait must fail")
            case .failure(let error): #expect(error is CancellationError)
            }
            #expect(await service.startWaiters.isEmpty)
            // Cancelling one wait must not poison the next successful admission.
            if !alreadyStarted {
                let next = Task { try await service.waitForStart() }
                _ = try await service.runDueMaintenance(.opened, operation: MaintenanceOperation(), progress: { _ in })
                try await next.value
            }
        } catch {
            waiting.cancel(); _ = await waiting.result
            await service.release()
            throw error
        }
    }
}

@MainActor @Suite struct BackupCoordinatorTests {
    private func entry(_ day: UInt8, kind: BackupKind = .backup) -> BackupEntry {
        BackupEntry(name: "backup-2030-01-\(day)-09.00.00.colpkg", kind: kind,
            wallTime: .init(year: 2030, month: 1, day: day, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
    }
    @Test func listFailureIsDistinctFromSuccessfulEmptyReadsAndLimitErrors() async {
        let host = BackupHostProbe(), service = BackupServiceProbe()
        let model = BackupCoordinator(host: host, service: service)
        await service.configureReadFailures(list: .io("Synthetic list read failure"))
        await model.openedRestore()
        #expect(model.entries.isEmpty && model.listFailure == .io("Synthetic list read failure"))
        #expect(model.failure == nil && model.failureWork == nil)
        await service.configureReadFailures(limits: .collectionUnavailable)
        await model.refresh()
        // Status always fails in this probe. Neither auxiliary read makes
        // a successful empty list into a list-load or restore failure.
        #expect(model.entries.isEmpty && model.listFailure == nil && model.failure == nil)
        #expect(model.status == nil && model.limits == nil)
        await service.configureReadFailures(editLimit: .diskFull)
        await model.refresh()
        await model.editLimit(.weekly, value: 4)
        #expect(model.failure == .diskFull && model.failureWork == nil && model.listFailure == nil)
    }
    @Test func listRefreshFailurePreservesRestoreFailureAndRecoveryFacts() async throws {
        let host = BackupHostProbe(), service = BackupServiceProbe(), selected = entry(2)
        let model = BackupCoordinator(host: host, service: service)
        let restoreCause = MaintenanceError.io("Synthetic final-step failure")
        let listCause = MaintenanceError.io("Synthetic list read failure")
        await service.configure(entries: [selected], refusal: .init(cause: restoreCause,
            candidateActive: true, generation: 2, safetyBackup: "kept-predecessor.colpkg"))
        await service.beforeRestore { await service.configureReadFailures(list: listCause) }
        await model.openedRestore()
        let restore = try #require(model.restoreConfirmed(name: selected.name))
        await restore.value
        #expect(model.entries.map(\.name) == [selected.name])
        #expect(model.listFailure == listCause && model.failure == restoreCause && model.failureWork == .restore)
        #expect(model.candidateActive && model.safetyBackup == "kept-predecessor.colpkg" && !model.restoreCompleted)
        await service.configureReadFailures()
        await model.refresh()
        #expect(model.listFailure == nil && model.failure == restoreCause && model.failureWork == .restore)
        #expect(model.candidateActive && model.safetyBackup == "kept-predecessor.colpkg")
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
        do { try await service.waitForStart() }
        catch { await service.release(); await restore.value; throw error }
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
        do { try await service.waitForStart() }
        catch { trigger.own(nil); await service.release(); await trigger.drain(); throw error }
        await trigger.drain()
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
    @Test func eventWinsOverAnInFlightDeadlineResultAndReplacementDiscardsTerminalTimer() async throws {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: 4000, held: true)
        let trigger = BackupTrigger(service: service, schedule: { _, _ in {} })
        trigger.own(1)
        let running = Task { await trigger.run(.opened) }
        do { try await service.waitForStart() }
        catch { trigger.own(nil); await service.release(); await running.value; throw error }
        trigger.event(deadline: 9000)
        await service.release(); await running.value
        #expect(trigger.deadlineMilliseconds == 9000)
        await service.configureDue(deadline: 6000, held: true)
        let retired = Task { await trigger.run(.deadline) }
        do { try await service.waitForStart() }
        catch { trigger.own(nil); await service.release(); await retired.value; throw error }
        trigger.own(2)
        trigger.event(deadline: 12000)
        await service.release(); await retired.value
        #expect(trigger.deadlineMilliseconds == 12000)
        trigger.own(nil)
    }
    @Test func cancellationRetiresHeldAndQueuedWorkButJoinsBeforeSuccessor() async throws {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: 4000, held: true)
        let trigger = BackupTrigger(service: service, schedule: { _, _ in {} })
        trigger.own(1)
        var firstEnded = false, successorEnded = false
        let running = Task { await trigger.run(.opened); firstEnded = true }
        do { try await service.waitForStart() }
        catch { trigger.own(nil); await service.release(); await running.value; throw error }
        var queuedEntered = false
        let queued = Task { queuedEntered = true; await trigger.run(.background) }
        let admissionLimit = ContinuousClock.now.advanced(by: .seconds(2))
        while !queuedEntered, ContinuousClock.now < admissionLimit { await Task.yield() }
        do {
            try #require(queuedEntered)
            trigger.cancelPending()
            try await service.waitForOperationCancellation()
        } catch {
            trigger.own(nil); await service.release(); await running.value; await queued.value
            throw error
        }
        let successor = Task { await trigger.run(.opened); successorEnded = true }
        await Task.yield()
        #expect(!firstEnded && !successorEnded) // Cancellation never abandons the active operation.
        trigger.event(deadline: 9000)
        await service.release()
        await running.value; await queued.value; await successor.value
        #expect(await service.triggers == [.opened, .opened])
        #expect(firstEnded && successorEnded)
        trigger.own(nil)
    }
    @Test func quitBudgetCancelsTheHeldOperationAndStillWaitsForItsTerminalResult() async throws {
        let service = BackupServiceProbe()
        await service.configureDue(deadline: nil, held: true)
        let trigger = BackupTrigger(service: service, schedule: { _, _ in {} })
        trigger.own(1)
        var ended = false
        let began = ContinuousClock.now
        let quit = Task { await trigger.run(.quit); ended = true }
        do {
            try await service.waitForStart()
            try await service.waitForOperationCancellation()
        } catch { trigger.own(nil); await service.release(); await quit.value; throw error }
        #expect(began.duration(to: .now) < .seconds(3))
        #expect(!ended)
        await service.release(); await quit.value
        #expect(ended && trigger.deadlineMilliseconds == nil)
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
