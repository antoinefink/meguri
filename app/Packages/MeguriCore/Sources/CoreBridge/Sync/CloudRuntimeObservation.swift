import Foundation

/// Passive in-process observations. Native identities and event values must
/// never be serialized by a client of this seam.
public enum CloudSDKObservationPhase: Sendable { case delivered, committed, returned }
public struct CloudSDKObservation: Sendable {
    public let origin: String
    public let ordinal: UInt64
    public let event: CloudEngineEvent
    public let phase: CloudSDKObservationPhase
}
public enum CloudOwnObservationPhase: Sendable {
    case prepared, enqueued(Bool), replied(CloudPublicationResult), committed
}
public struct CloudOwnObservation: Sendable {
    public let metadata: CloudOwnMetadata
    public let identity: UUID
    public let phase: CloudOwnObservationPhase
    public let publicationEvent: SelectedCloudPublicationEvent?
    init(metadata: CloudOwnMetadata, identity: UUID, phase: CloudOwnObservationPhase,
         publicationEvent: SelectedCloudPublicationEvent? = nil) {
        self.metadata = metadata; self.identity = identity; self.phase = phase
        self.publicationEvent = publicationEvent
    }
}

/// Content-free projection of an original reply. Present only when an observer
/// requested capture in a test-hooks build; never used for ordinary decisions.
public struct SelectedCloudPublicationEvent: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    public enum Origin: Sendable, Equatable { case saved, conflict }
    public enum Kind: Sendable, Equatable { case completeEpoch, doneActive }
    public let origin: Origin
    public let attempt: UInt64
    public let kind: Kind
    public let correlation: Data
    public var description: String { "SelectedCloudPublicationEvent" }
    public var debugDescription: String { description }
}

struct ObservedCloudPublicationQueue: CloudPublicationQueue {
    let original: any CloudPreparedOwnOperation
    let observe: @Sendable (Bool) -> Void
    func enqueue() -> Bool {
        let accepted = original.enqueue()
        observe(accepted)
        return accepted
    }
}
