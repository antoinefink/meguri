internal import MeguriCoreBindings

public protocol MaintenanceService: Sendable {
    func createBackup(operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ImportBackupReceipt
    func restoreBackup(name: String, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> RestoredBackup
    func runDueMaintenance(_ trigger: MaintenanceTrigger, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> Int64?
    func listBackups() async throws -> [BackupEntry]
    func backupLimits() async throws -> BackupLimits
    func updateBackupLimit(_ patch: BackupLimitPatch) async throws -> BackupLimits
    func setBackupLimits(_ limits: BackupLimits) async throws -> Bool
    func status() async throws -> MaintenanceStatus
}
/// Test observers forward these calls to the real exports; result objects stay native.
protocol MaintenanceNative: Sendable {
    func startCreateBackup() throws -> MeguriCoreBindings.CreateBackupJob
    func startRestoreBackup(_ name: String) throws -> MeguriCoreBindings.RestoreBackupJob
    func startRunDueMaintenance(_ trigger: MeguriCoreBindings.MaintenanceTrigger) throws -> MeguriCoreBindings.DueMaintenanceJob
    func listBackups() throws -> [MeguriCoreBindings.BackupEntry]
    func backupLimits() throws -> MeguriCoreBindings.BackupLimits
    func updateBackupLimit(_ patch: MeguriCoreBindings.BackupLimitPatch) throws -> MeguriCoreBindings.BackupLimits
    func setBackupLimits(_ limits: MeguriCoreBindings.BackupLimits) throws -> Bool
    func maintenanceStatus() throws -> MeguriCoreBindings.MaintenanceStatus
}
private struct RealMaintenanceNative: MaintenanceNative {
    func startCreateBackup() throws -> MeguriCoreBindings.CreateBackupJob { try MeguriCoreBindings.startCreateBackup() }
    func startRestoreBackup(_ name: String) throws -> MeguriCoreBindings.RestoreBackupJob { try MeguriCoreBindings.startRestoreBackup(name: name) }
    func startRunDueMaintenance(_ trigger: MeguriCoreBindings.MaintenanceTrigger) throws -> MeguriCoreBindings.DueMaintenanceJob { try MeguriCoreBindings.startRunDueMaintenance(trigger: trigger) }
    func listBackups() throws -> [MeguriCoreBindings.BackupEntry] { try MeguriCoreBindings.listBackups() }
    func backupLimits() throws -> MeguriCoreBindings.BackupLimits { try MeguriCoreBindings.backupLimits() }
    func setBackupLimits(_ limits: MeguriCoreBindings.BackupLimits) throws -> Bool { try MeguriCoreBindings.setBackupLimits(limits: limits) }
    func updateBackupLimit(_ patch: MeguriCoreBindings.BackupLimitPatch) throws -> MeguriCoreBindings.BackupLimits { try MeguriCoreBindings.updateBackupLimit(patch: patch) }
    func maintenanceStatus() throws -> MeguriCoreBindings.MaintenanceStatus { try MeguriCoreBindings.maintenanceStatus() }
}
public final class CoreMaintenanceService: MaintenanceService, Sendable {
    private let native: any MaintenanceNative
    private let trackingSetup: @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob
    public init() { native = RealMaintenanceNative(); trackingSetup = { try Jobs.track($0) } }
    init(native: any MaintenanceNative, trackingSetup: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob = { try Jobs.track($0) }) {
        self.native = native; self.trackingSetup = trackingSetup
    }
    // Even failed listener setup cancels and drains through the existing registry.
    // The owned typed outcome inspected afterwards outranks cancellation/setup errors.
    private func drain(_ handle: MeguriCoreBindings.JobHandle, operation: MaintenanceOperation,
                       progress: @escaping @Sendable (CoreProgress) -> Void) async {
        do {
            let job = try await Jobs.trackDraining(handle, setup: trackingSetup)
            await operation.attach(job)
            let updates = Task { for await value in job.progress { progress(value) } }
            _ = await job.outcome()
            await updates.value
        } catch { /* trackDraining has already drained the native worker. */ }
    }
    public func createBackup(operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ImportBackupReceipt {
        try await CoreEventStream.shared.start()
        try operation.check()
        return try await withTaskCancellationHandler {
            let started = try await MaintenanceLane.shared.call(.startCreateBackup) { try self.native.startCreateBackup() }
            await drain(started.job, operation: operation, progress: progress)
            switch try await MaintenanceLane.shared.call(.createBackupResultOutcome, { try started.result.outcome() }) {
            case .created(let backup): return ImportBackupReceipt(backup)
            case .failed(let failure): throw MaintenanceFailure(failure)
            case .pending: throw BridgeError.internal("Backup pending after terminal delivery")
            }
        } onCancel: { Task { await operation.cancel() } }
    }
    public func restoreBackup(name: String, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> RestoredBackup {
        try await CoreEventStream.shared.start()
        try operation.check()
        return try await withTaskCancellationHandler {
            let started = try await MaintenanceLane.shared.call(.startRestoreBackup) { try self.native.startRestoreBackup(name) }
            await drain(started.job, operation: operation, progress: progress)
            switch try await MaintenanceLane.shared.call(.restoreBackupResultOutcome, { try started.result.outcome() }) {
            case .restored(let restored): return RestoredBackup(restored)
            case .failed(let failure): throw MaintenanceFailure(failure)
            case .pending: throw BridgeError.internal("Restore pending after terminal delivery")
            }
        } onCancel: { Task { await operation.cancel() } }
    }
    public func runDueMaintenance(_ trigger: MaintenanceTrigger, operation: MaintenanceOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> Int64? {
        try await CoreEventStream.shared.start()
        try operation.check()
        return try await withTaskCancellationHandler {
            let started = try await MaintenanceLane.shared.call(.startRunDueMaintenance) { try self.native.startRunDueMaintenance(trigger.raw) }
            await drain(started.job, operation: operation, progress: progress)
            switch try await MaintenanceLane.shared.call(.dueMaintenanceResultOutcome, { try started.result.outcome() }) {
            case .completed(let deadline): return deadline
            case .failed(let failure): throw MaintenanceFailure(failure)
            case .pending: throw BridgeError.internal("Maintenance pending after terminal delivery")
            }
        } onCancel: { Task { await operation.cancel() } }
    }
    public func listBackups() async throws -> [BackupEntry] {
        try await MaintenanceLane.shared.call(.listBackups) { try self.native.listBackups().map(BackupEntry.init) }
    }
    public func backupLimits() async throws -> BackupLimits {
        try await MaintenanceLane.shared.call(.backupLimits) { try BackupLimits(self.native.backupLimits()) }
    }
    public func setBackupLimits(_ limits: BackupLimits) async throws -> Bool {
        try await MaintenanceLane.shared.call(.setBackupLimits) { try self.native.setBackupLimits(limits.raw) }
    }
    public func updateBackupLimit(_ patch: BackupLimitPatch) async throws -> BackupLimits {
        try await MaintenanceLane.shared.call(.updateBackupLimit) { try BackupLimits(self.native.updateBackupLimit(patch.raw)) }
    }
    public func status() async throws -> MaintenanceStatus {
        try await MaintenanceLane.shared.call(.maintenanceStatus) { try MaintenanceStatus(self.native.maintenanceStatus()) }
    }
}
