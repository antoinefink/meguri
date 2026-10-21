internal import MeguriCoreBindings

public enum CollectionUpgradeStage: Sendable, Hashable {
    case copy
    case step(UInt32)
    init(_ raw: MeguriCoreBindings.CollectionUpgradeStage) {
        switch raw { case .copy: self = .copy; case .step(let step): self = .step(step) }
    }
}
public enum CollectionOpenFailure: Sendable, Hashable {
    case openInAnotherProgram
    case tooOld(version: Int64)
    case tooNew(version: Int64)
    case v1Scheduler
    case notACollection(missingTables: [String])
    case upgradeFailed(version: Int64, stage: CollectionUpgradeStage, detail: String)
    case corrupt(detail: String)
    case connectionInit(detail: String)
    case notFound
    case io(detail: String)
    init(_ raw: MeguriCoreBindings.CollectionOpenFailure) {
        switch raw {
        case .openInAnotherProgram: self = .openInAnotherProgram
        case .tooOld(let version): self = .tooOld(version: version)
        case .tooNew(let version): self = .tooNew(version: version)
        case .v1Scheduler: self = .v1Scheduler
        case .notACollection(let tables): self = .notACollection(missingTables: tables)
        case .upgradeFailed(let version, let stage, let detail):
            self = .upgradeFailed(version: version, stage: CollectionUpgradeStage(stage), detail: detail)
        case .corrupt(let detail): self = .corrupt(detail: detail)
        case .connectionInit(let detail): self = .connectionInit(detail: detail)
        case .notFound: self = .notFound
        case .io(let detail): self = .io(detail: detail)
        }
    }
}
public enum CollectionFailedReason: Sendable, Hashable {
    case open(CollectionOpenFailure)
    case openStep(step: String, detail: String)
    case replace(detail: String)
    case corrupt(detail: String)
    init(_ raw: MeguriCoreBindings.CollectionFailedReason) {
        switch raw {
        case .open(let error): self = .open(CollectionOpenFailure(error))
        case .openStep(let step, let detail): self = .openStep(step: step, detail: detail)
        case .replace(let detail): self = .replace(detail: detail)
        case .corrupt(let detail): self = .corrupt(detail: detail)
        }
    }
}
public enum CollectionLongOp: Sendable, Hashable {
    case oneWaySync, restore, colpkgImport, apkgApply, checkDatabase
    init(_ raw: MeguriCoreBindings.CollectionLongOp) {
        switch raw {
        case .oneWaySync: self = .oneWaySync
        case .restore: self = .restore
        case .colpkgImport: self = .colpkgImport
        case .apkgApply: self = .apkgApply
        case .checkDatabase: self = .checkDatabase
        }
    }
}
public enum CollectionExclusivePhase: Sendable, Hashable {
    case draining, working, replacing
    init(_ raw: MeguriCoreBindings.CollectionExclusivePhase) {
        switch raw {
        case .draining: self = .draining
        case .working: self = .working
        case .replacing: self = .replacing
        }
    }
}
public enum CollectionLifecycle: Sendable, Hashable {
    case noCollection
    case opening
    case upgrading(from: Int64, step: UInt32)
    case ready(generation: UInt64)
    case exclusiveBusy(op: CollectionLongOp, phase: CollectionExclusivePhase, cancellable: Bool)
    case failed(CollectionFailedReason)
    init(_ raw: MeguriCoreBindings.CollectionLifecycle) {
        switch raw {
        case .noCollection: self = .noCollection
        case .opening: self = .opening
        case .upgrading(let from, let step): self = .upgrading(from: from, step: step)
        case .ready(let generation): self = .ready(generation: generation)
        case .exclusiveBusy(let op, let phase, let cancellable):
            self = .exclusiveBusy(op: CollectionLongOp(op), phase: CollectionExclusivePhase(phase), cancellable: cancellable)
        case .failed(let reason): self = .failed(CollectionFailedReason(reason))
        }
    }
}
