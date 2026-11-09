#if DEBUG
import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum SelectedCloudMissing: Sendable, Equatable {
    case unbound, currentCollectionChanged, accountUnavailable, bindingUnusable
    case operationInProgress, activeUnavailable, controlsMismatch, missingAfterRestart
    case publicationNotCommitted, adoptionNotActivated, activationUnavailable, ownedBackupUnavailable
    init(_ r: MeguriCoreBindings.CloudSelectedMissing) {
        self = switch r {
        case .unbound: .unbound
        case .currentCollectionChanged: .currentCollectionChanged
        case .accountUnavailable: .accountUnavailable
        case .bindingUnusable: .bindingUnusable
        case .operationInProgress: .operationInProgress
        case .activeUnavailable: .activeUnavailable
        case .controlsMismatch: .controlsMismatch
        case .missingAfterRestart: .missingAfterRestart
        case .publicationNotCommitted: .publicationNotCommitted
        case .adoptionNotActivated: .adoptionNotActivated
        case .activationUnavailable: .activationUnavailable
        case .ownedBackupUnavailable: .ownedBackupUnavailable
        }
    }
}
public enum SelectedCloudProof: Sendable, Equatable {
    case proven, missing(SelectedCloudMissing)
    init(_ r: MeguriCoreBindings.CloudSelectedProof) {
        self = switch r { case .proven: .proven; case .missing(let reason): .missing(.init(reason)) }
    }
}
public enum SelectedCloudProvenance: Sendable, Equatable {
    case currentAcceptedActive, runtimeAcceptedPublication, retainedAdoption
    init(_ r: MeguriCoreBindings.CloudSelectedProvenance) {
        self = switch r {
        case .currentAcceptedActive: .currentAcceptedActive
        case .runtimeAcceptedPublication: .runtimeAcceptedPublication
        case .retainedAdoption: .retainedAdoption
        }
    }
}
extension SelectedCloudPublicationEvent {
    init(_ r: MeguriCoreBindings.CloudSelectedPublicationEvent) {
        let origin: Origin = switch r.origin { case .saved: .saved; case .conflict: .conflict }
        let kind: Kind = switch r.kind { case .completeEpoch: .completeEpoch; case .doneActive: .doneActive }
        self.init(origin: origin, attempt: r.attempt, kind: kind, correlation: r.correlation)
    }
}
public struct SelectedCloudControlObservation: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public let generation: UInt64
    public let status: SelectedCloudProof
    public let provenance: SelectedCloudProvenance?
    public let publicationComplete: SelectedCloudPublicationEvent?
    public let publicationDoneActive: SelectedCloudPublicationEvent?
    public let activeSHA256: Data?
    public let epochSHA256: Data?
    public let bindingMatchesEpoch: SelectedCloudProof
    public let activation: SelectedCloudProof
    public let ownedBackup: SelectedCloudProof
    public let requiredScanComplete: Bool
    public var description: String { "SelectedCloudControlObservation" }
    public var debugDescription: String { description }
    init(_ r: MeguriCoreBindings.CloudSelectedControlObservation) {
        generation = r.generation; status = .init(r.status); provenance = r.provenance.map(SelectedCloudProvenance.init)
        publicationComplete = r.publicationComplete.map(SelectedCloudPublicationEvent.init)
        publicationDoneActive = r.publicationDoneActive.map(SelectedCloudPublicationEvent.init)
        activeSHA256 = r.activeSha256; epochSHA256 = r.epochSha256
        bindingMatchesEpoch = .init(r.bindingMatchesEpoch); activation = .init(r.activation); ownedBackup = .init(r.ownedBackup)
        requiredScanComplete = r.requiredScanComplete
    }
}
public actor SelectedCloudControlInspection: CustomStringConvertible, CustomDebugStringConvertible {
    public init() {}
    public nonisolated var description: String { "SelectedCloudControlInspection" }
    public nonisolated var debugDescription: String { description }
    public func current() async throws -> SelectedCloudControlObservation {
        try await QueryLane.shared.call(.testCloudSelectedControl) {
            try SelectedCloudControlObservation(MeguriCoreBindings.testCloudSelectedControl())
        }
    }
    static func publicationEvent(_ request: CloudPublicationRequest, reply: CloudPublicationResult) async throws -> SelectedCloudPublicationEvent? {
        try await QueryLane.shared.call(.testCloudSelectedPublicationEvent) {
            try MeguriCoreBindings.testCloudSelectedPublicationEvent(request: request.raw, reply: reply.raw).map(SelectedCloudPublicationEvent.init)
        }
    }
}
#endif
