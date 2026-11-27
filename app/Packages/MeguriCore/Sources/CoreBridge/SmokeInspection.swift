#if DEBUG
import Instrumentation
internal import MeguriCoreBindings

/// Fixed local integrity observations from one owned Query snapshot.
public struct SmokeLocalIntegrity: Sendable, Hashable {
    public let generation: UInt64
    public let seq: UInt64
    public let integrityOK: Bool
    public let schemaOK: Bool
    public let referenceFailures: UInt64
}

public struct CoreSmokeInspection: Sendable {
    public init() {}

    public func integrity() async throws(BridgeError) -> SmokeLocalIntegrity {
        try await QueryLane.shared.call(.smokeIntegrity) {
            let observation = try MeguriCoreBindings.testSmokeIntegrity()
            return SmokeLocalIntegrity(
                generation: observation.generation, seq: observation.seq,
                integrityOK: observation.integrityOk, schemaOK: observation.schemaOk,
                referenceFailures: observation.referenceFailures)
        }
    }
}
#endif
