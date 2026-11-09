#if DEBUG && MEGURI_TEST_HOOKS
@testable import AppModel
import CoreBridge
import CryptoKit
import Foundation

@MainActor final class CloudKitSmokeChanges: ChangeFanoutSink {
    private let stream = AgentExchangeEvents<CoreEvent>()
    private(set) var events: [CoreEvent] = []
    func receive(_ event: CoreEvent) {
        guard events.count < 4096 else { stream.fail(.execution); return }
        events.append(event); stream.yield(event)
    }
    func next(after seq: UInt64, matching predicate: @escaping (CoreEvent) -> Bool) async throws -> CoreEvent {
        try stream.checkFailure()
        if let event = events.first(where: { $0.seq > seq && predicate($0) }) { return event }
        return try await stream.next(where: { $0.seq > seq && predicate($0) })
    }
    func checkFailure() throws { try stream.checkFailure() }
    func finish() { ChangeFanout.shared.unsubscribe(self); stream.finish() }
}

nonisolated enum CloudKitSmokeProof {
    static func acknowledgements(current: [SmokeCloudEntity], retained: [String: SmokeCloudEntity],
                                 observations: [CloudWorkflowObservation]) throws -> [AgentFact] {
        let required: Set<String> = ["n1", "c1", "r1", "m1"]
        guard current.count == 4, Set(current.map(\.label)) == required,
              Set(retained.keys) == required else { throw AgentFailure.execution }
        return try current.map { entity in
            guard let original = retained[entity.label], entity.revision > 0,
                  entity.recordName == original.recordName, entity.revision == original.revision,
                  entity.digest == original.digest, entity.digest.count == 32,
                  entity.acknowledged, entity.ackedRevision == original.revision,
                  observations.contains(where: {
                      if case .savedPendingRemoved(let name, _) = $0 { return name == entity.recordName }; return false
                  }), entity.label != "m1" || (entity.blobPresent && entity.mediaBytes == 10_240 && entity.mediaSHA256 == original.mediaSHA256) else { throw AgentFailure.execution }
            // Earlier actual automatic ACKs may satisfy a current revision;
            // these facts deliberately carry no manual-exchange attribution.
            return AgentFact(name: .stateSaved, count: entity.ackedRevision,
                             digest: CloudKitSmokeGeneratedInput.hex(entity.digest), operation: entity.label)
        }
    }
}

nonisolated enum CloudKitSmokeGeneratedInput {
    static func media(run: String, device: String) -> Data {
        var bytes = Data()
        for i in 0..<320 {
            bytes.append(contentsOf: SHA256.hash(data: Data("MeguriCloudSmoke/\(run)/\(device)/m1/\(i)".utf8)))
        }
        return bytes
    }
    static func hex(_ data: Data) -> String { data.map { String(format: "%02x", $0) }.joined() }
}
#endif
