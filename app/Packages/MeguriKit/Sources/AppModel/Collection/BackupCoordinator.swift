import CoreBridge
import Foundation
import Observation

struct BackupRestoreCompletion {
    let candidateActive: Bool?
    let ready: Bool
}

@MainActor
protocol BackupHost: AnyObject {
    var backupGeneration: UInt64 { get }
    var backupsWritable: Bool { get }
    var backupsAvailable: Bool { get }
    var backupCancellationSafe: Bool { get }
    func prepareBackups() async throws
    func beginBackupRestore() async throws
    func cancelBackupRestoreAdmission()
    func finishBackupRestore(candidateActive: Bool?, generation: UInt64?) async -> BackupRestoreCompletion
    func backupRestoreSucceeded()
}

/// Shared by every scene, including recovery before the first successful open.
@Observable @MainActor
public final class BackupCoordinator {
    public enum Work: Equatable { case create, restore }
    public enum LimitField: CaseIterable, Equatable, Sendable { case daily, weekly, monthly, interval }
    public private(set) var entries: [BackupEntry] = []
    public var selectedName: String?
    public private(set) var limits: BackupLimits?
    public private(set) var status: MaintenanceStatus?
    public private(set) var loading = false
    public private(set) var work: Work?
    public private(set) var progress: CoreProgress?
    public private(set) var created: ImportBackupReceipt?
    public private(set) var failure: MaintenanceError?
    public private(set) var failureWork: Work?
    public private(set) var listFailure: MaintenanceError?
    public private(set) var candidateActive = false
    public private(set) var activationUncertain = false
    public private(set) var safetyBackup: String?
    public private(set) var backupPublication: ImportBackupPublication = .unpublished
    public private(set) var restoreCompleted = false
    public private(set) var savingLimits = false
    public private(set) var rejectedNames: Set<String> = []
    @ObservationIgnored private weak var host: (any BackupHost)?
    @ObservationIgnored private let service: any MaintenanceService
    @ObservationIgnored private var operation: MaintenanceOperation?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var loadRevision: UInt64 = 0
    @ObservationIgnored private var limitsRevision: UInt64 = 0
    @ObservationIgnored private var ownership: UInt64 = 0
    @ObservationIgnored private var cancellationRequested = false

    init(host: any BackupHost, service: any MaintenanceService) { self.host = host; self.service = service }
    public var busy: Bool { work != nil }
    public var canCreate: Bool { host?.backupsWritable == true && !busy }
    public var canOpenRestore: Bool { host?.backupsAvailable == true && !busy && !activationUncertain }
    public var canEditLimits: Bool { canCreate && limits != nil && !savingLimits }
    public var canCancel: Bool { busy && !cancellationRequested && progress?.cancellable != false && host?.backupCancellationSafe != false }
    public nonisolated static func isRestorable(_ entry: BackupEntry, rejectedNames: Set<String>) -> Bool {
        entry.kind != .serverCopyUnverified && !rejectedNames.contains(entry.name)
    }
    public var selectedEntry: BackupEntry? {
        entries.first { $0.name == selectedName && Self.isRestorable($0, rejectedNames: rejectedNames) }
    }

    public func refresh() async {
        guard let host else { return }
        loadRevision &+= 1
        let read = loadRevision, expected = ownership
        loading = true
        defer { if read == loadRevision { loading = false } }
        do {
            try await host.prepareBackups()
            let generation = host.backupGeneration
            let values = try await service.listBackups()
            guard read == loadRevision, expected == ownership, generation == host.backupGeneration else { return }
            entries = values
            listFailure = nil
            selectEligible()
            // A damaged host can still list/restore even when config/status reads fail.
            let readStatus = try? await service.status()
            let limitsRead = limitsRevision, limitsWereSaving = savingLimits
            let readLimits = try? await service.backupLimits()
            guard read == loadRevision, expected == ownership, generation == host.backupGeneration else { return }
            status = readStatus
            if !limitsWereSaving, !savingLimits, limitsRead == limitsRevision { limits = readLimits }
        } catch {
            guard read == loadRevision, expected == ownership else { return }
            listFailure = Self.cause(error)
        }
    }

    private func selectEligible() {
        if selectedEntry == nil {
            selectedName = entries.first { Self.isRestorable($0, rejectedNames: rejectedNames) }?.name
        }
    }

    public func openedRestore() async {
        guard !busy else { return }
        failure = nil; failureWork = nil; candidateActive = false; restoreCompleted = false
        rejectedNames.removeAll(); selectedName = nil
        await refresh()
    }

    @discardableResult public func create() -> Task<Void, Never>? {
        guard canCreate, let host else { return nil }
        begin(.create)
        let expected = ownership, generation = host.backupGeneration
        let operation = MaintenanceOperation(); self.operation = operation
        let job = Task {
            defer { end() }
            do {
                let receipt = try await service.createBackup(operation: operation, progress: progressHandler(expected, operation: operation))
                guard expected == ownership, host.backupGeneration == generation else { return }
                created = receipt
            } catch {
                if expected == ownership, host.backupGeneration == generation { record(error) }
            }
            if expected == ownership { await refresh() }
        }
        task = job
        return job
    }

    /// Only the confirmation button calls this. Capture the exact listed name;
    /// refreshing or a selection change must never redirect destructive consent.
    @discardableResult public func restoreConfirmed(name: String) -> Task<Void, Never>? {
        guard canOpenRestore, selectedEntry?.name == name, let host else { return nil }
        begin(.restore)
        let expected = ownership
        let operation = MaintenanceOperation(); self.operation = operation
        let job = Task {
            defer { end() }
            var admitted = false
            do {
                try Task.checkCancellation()
                try await host.beginBackupRestore(); admitted = true
                try Task.checkCancellation()
                let restored = try await service.restoreBackup(name: name, operation: operation, progress: progressHandler(expected, operation: operation))
                // Activation is owned terminal evidence, even when its event has
                // already advanced the collection generation while this awaited.
                candidateActive = true; safetyBackup = restored.safetyBackup
                let completion = await host.finishBackupRestore(candidateActive: true, generation: restored.generation)
                guard completion.ready else { failure = .collectionUnavailable; failureWork = .restore; return }
                await refresh()
                restoreCompleted = true
                host.backupRestoreSucceeded()
            } catch {
                record(error)
                let typed = error as? MaintenanceFailure
                if admitted {
                    let completion = await host.finishBackupRestore(candidateActive: typed?.candidateActive,
                        generation: typed?.generation ?? typed?.restored?.generation)
                    candidateActive = completion.candidateActive == true
                    activationUncertain = completion.candidateActive == nil
                }
                if !candidateActive, !activationUncertain, Self.rejectsCandidate(Self.cause(error)) {
                    rejectedNames.insert(name); selectedName = nil
                }
                // Keep the sheet, including after activation with a failed final
                // step. Never describe that candidate as the untouched predecessor.
                await refresh()
            }
        }
        task = job
        return job
    }

    public func cancel() {
        guard canCancel, let operation else { return }
        cancellationRequested = true
        if work == .restore { host?.cancelBackupRestoreAdmission() }
        task?.cancel()
        Task { await operation.cancel() }
    }
    func cancelAndDrain() async {
        let original = task
        cancellationRequested = true
        if work == .restore { host?.cancelBackupRestoreAdmission() }
        original?.cancel()
        if let operation { await operation.cancel() }
        await original?.value
    }

    func collectionChanged() {
        ownership &+= 1; loadRevision &+= 1; loading = false
        entries = []; selectedName = nil; limits = nil; status = nil; created = nil
        listFailure = nil
        rejectedNames.removeAll()
        if work != .restore { failure = nil; failureWork = nil }
    }

    public func editLimit(_ field: LimitField, value: UInt32) async {
        guard canEditLimits, let current = limits, let host else { return }
        let patch: BackupLimitPatch
        let previous: UInt32
        let bounded = field == .interval ? min(max(value, 5), 9999) : min(value, 9999)
        switch field {
        case .daily: patch = .daily(bounded); previous = current.daily
        case .weekly: patch = .weekly(bounded); previous = current.weekly
        case .monthly: patch = .monthly(bounded); previous = current.monthly
        case .interval: patch = .minimumIntervalMinutes(bounded); previous = current.minimumIntervalMinutes
        }
        guard bounded != previous else { return }
        let expected = ownership, generation = host.backupGeneration
        limitsRevision &+= 1
        let revision = limitsRevision
        func currentOwner() -> Bool {
            expected == ownership && generation == host.backupGeneration && revision == limitsRevision
        }
        savingLimits = true
        defer { savingLimits = false }
        do {
            let confirmed = try await service.updateBackupLimit(patch)
            if currentOwner() { limits = confirmed }
        } catch {
            guard currentOwner() else { return }
            failure = Self.cause(error)
            failureWork = nil
            // The refused write fenced concurrent snapshots. Read again under
            // that same owner while keeping the write failure visible.
            if let fresh = try? await service.backupLimits(), currentOwner() { limits = fresh }
        }
    }

    private func begin(_ work: Work) {
        self.work = work; progress = nil; failure = nil; failureWork = nil; candidateActive = false; activationUncertain = false
        safetyBackup = nil; backupPublication = .unpublished; created = nil
        cancellationRequested = false; restoreCompleted = false
    }
    private func end() { work = nil; progress = nil; operation = nil; task = nil }
    private func progressHandler(_ expected: UInt64, operation: MaintenanceOperation) -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in Task { @MainActor in
            guard let self, self.operation === operation, self.work != nil, self.ownership == expected else { return }
            self.progress = value
        } }
    }
    private func record(_ error: any Error) {
        failure = Self.cause(error)
        failureWork = work
        if let typed = error as? MaintenanceFailure {
            candidateActive = typed.candidateActive; safetyBackup = typed.safetyBackup ?? typed.restored?.safetyBackup
            backupPublication = typed.backupPublication
        }
    }
    static func rejectsCandidate(_ cause: MaintenanceError) -> Bool {
        switch cause {
        case .corrupt, .tooNew, .schemaUnsupported, .unsupportedFormat, .invalidIDs, .invalidArgument: true
        default: false
        }
    }
    static func cause(_ error: any Error) -> MaintenanceError {
        if let failure = error as? MaintenanceFailure { return failure.cause }
        if let failure = error as? MaintenanceError { return failure }
        if error is CancellationError { return .cancelled }
        if let bridge = error as? BridgeError {
            switch bridge {
            case .cancelled: return .cancelled
            case .sessionActive: return .sessionActive
            case .busy(let name): return .busy(runningOperation: name)
            case .insufficientSpace(let needed, let available): return .insufficientSpace(needed: needed, available: available)
            default: break
            }
        }
        return .collectionUnavailable
    }

    /// Browser session sign-in remains separate from native credentials.
    public static func accountBackupsURL(endpoint: String) -> URL? {
        guard var url = URLComponents(string: endpoint), let scheme = url.scheme?.lowercased(), ["https", "http"].contains(scheme),
              url.host != nil, url.user == nil, url.password == nil else { return nil }
        url.path = "/collection"; url.query = nil; url.fragment = nil
        return url.url
    }
}
