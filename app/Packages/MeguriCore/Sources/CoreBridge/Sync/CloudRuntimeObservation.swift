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
