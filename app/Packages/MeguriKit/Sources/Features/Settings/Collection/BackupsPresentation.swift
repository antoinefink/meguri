import AppModel
import CoreBridge
import Foundation

struct BackupsPresentation {
    struct Receipt {
        let name: String
        let sizeBytes: UInt64
    }
    let entries: [BackupEntry]
    let rejectedNames: Set<String>
    let limits: BackupLimits?
    let lastBackupMilliseconds: Int64?
    let lastBackupUnavailable: Bool
    let loading: Bool
    let work: BackupCoordinator.Work?
    let created: Receipt?
    let failure: MaintenanceError?
    let failureWork: BackupCoordinator.Work?
    let listFailure: MaintenanceError?
    let candidateActive: Bool
    let activationUncertain: Bool
    let safetyBackup: String?
    let backupPublication: ImportBackupPublication
    let canCreate: Bool
    let canOpenRestore: Bool
    let canEditLimits: Bool
    let canExport: Bool
    let canCancel: Bool
    let provider: SyncProviderSelection?
    let serverBackupsURL: URL?
    var busy: Bool { work != nil }

    init(
        entries: [BackupEntry] = [],
        rejectedNames: Set<String> = [],
        limits: BackupLimits? = nil,
        lastBackupMilliseconds: Int64? = nil,
        lastBackupUnavailable: Bool = false,
        loading: Bool = false,
        work: BackupCoordinator.Work? = nil,
        created: Receipt? = nil,
        failure: MaintenanceError? = nil,
        failureWork: BackupCoordinator.Work? = nil,
        listFailure: MaintenanceError? = nil,
        candidateActive: Bool = false,
        activationUncertain: Bool = false,
        safetyBackup: String? = nil,
        backupPublication: ImportBackupPublication = .unpublished,
        canCreate: Bool = true,
        canOpenRestore: Bool = true,
        canEditLimits: Bool = true,
        canExport: Bool = true,
        canCancel: Bool = false,
        provider: SyncProviderSelection? = .localOnly,
        serverBackupsURL: URL? = nil
    ) {
        self.entries = entries
        self.rejectedNames = rejectedNames
        self.limits = limits
        self.lastBackupMilliseconds = lastBackupMilliseconds
        self.lastBackupUnavailable = lastBackupUnavailable
        self.loading = loading
        self.work = work
        self.created = created
        self.failure = failure
        self.failureWork = failureWork
        self.listFailure = listFailure
        self.candidateActive = candidateActive
        self.activationUncertain = activationUncertain
        self.safetyBackup = safetyBackup
        self.backupPublication = backupPublication
        self.canCreate = canCreate
        self.canOpenRestore = canOpenRestore
        self.canEditLimits = canEditLimits
        self.canExport = canExport
        self.canCancel = canCancel
        self.provider = provider
        self.serverBackupsURL = serverBackupsURL
    }

    @MainActor init(_ collection: ReviewCollection) {
        let model = collection.backups
        self.init(entries: model.entries, rejectedNames: model.rejectedNames, limits: model.limits,
            lastBackupMilliseconds: model.status?.lastBackup?.instantMilliseconds,
            lastBackupUnavailable: model.status?.failures.contains(where: { $0.field == .lastBackup }) == true || (model.status == nil && !model.loading),
            loading: model.loading, work: model.work, created: model.created.map { Receipt(name: $0.name, sizeBytes: $0.sizeBytes) },
            failure: model.failure, failureWork: model.failureWork, listFailure: model.listFailure,
            candidateActive: model.candidateActive, activationUncertain: model.activationUncertain,
            safetyBackup: model.safetyBackup, backupPublication: model.backupPublication,
            canCreate: model.canCreate, canOpenRestore: model.canOpenRestore, canEditLimits: model.canEditLimits,
            canExport: collection.collectionExport.canPresent && !model.busy, canCancel: model.canCancel,
            provider: collection.sync.providerState?.selection,
            serverBackupsURL: collection.sync.providerState?.selection == .meguri && collection.sync.serviceConnected
                ? BackupCoordinator.accountBackupsURL(endpoint: collection.sync.meguriEndpoint) : nil)
    }
}

struct BackupsActions {
    var create: () -> Void = {}
    var restore: () -> Void = {}
    var revealFolder: () -> Void = {}
    var export: () -> Void = {}
    var editLimit: (BackupCoordinator.LimitField, UInt32) -> Void = { _, _ in }
    var cancel: () -> Void = {}
}
