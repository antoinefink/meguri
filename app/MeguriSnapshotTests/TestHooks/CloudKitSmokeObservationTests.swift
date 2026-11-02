#if DEBUG && MEGURI_TEST_HOOKS
@testable import CoreBridge
import Foundation
import Testing

// Generated observations exercise refusal logic only. They are never actual
// CloudKit success records, and this suite belongs outside the smoke selection.
@MainActor @Suite(.serialized) struct CloudKitSmokeObservationTests {
    private func entity(_ label: String, revision: UInt64 = 7, acked: UInt64 = 7,
                        digest: UInt8 = 11, acknowledged: Bool = true,
                        blob: Bool = true, bytes: UInt64 = 10_240) -> SmokeCloudEntity {
        .init(label: label, recordName: "generated-record-" + label, revision: revision,
              ackedRevision: acked, digest: Data(repeating: digest, count: 32), acknowledged: acknowledged,
              mediaBytes: label == "m1" ? bytes : nil,
              mediaSHA256: label == "m1" ? Data(repeating: 13, count: 32) : nil,
              blobPresent: blob, mediaChangeCounter: label == "m1" ? 5 : nil)
    }
    @Test func acknowledgementNeedsEachExactCurrentRevisionActualRecordEventAndBlobProof() throws {
        let labels = ["n1", "c1", "r1", "m1"]
        let current = labels.map { entity($0) }
        let retained = Dictionary(uniqueKeysWithValues: current.map { ($0.label, $0) })
        let events = current.map { CloudWorkflowObservation.savedPendingRemoved(name: $0.recordName, requestID: 17) }
        let valid = try CloudKitSmokeProof.acknowledgements(current: current, retained: retained, observations: events)
        #expect(Set(valid.compactMap(\.operation)) == Set(labels))
        #expect(valid.allSatisfy { $0.name == .stateSaved && $0.count == 7 })
        // A count or matching content cannot ACK an older/newer revision. Also
        // reject a note-only save and a collection success with pending media.
        for invalid in [
            [entity("n1", acked: 6)] + Array(current.dropFirst()),
            [entity("n1", revision: 8, acked: 8)] + Array(current.dropFirst()),
            [entity("n1", digest: 12)] + Array(current.dropFirst()),
            [entity("n1", acknowledged: false)] + Array(current.dropFirst()),
            Array(current.dropLast()) + [entity("m1", blob: false)],
            Array(current.dropLast()) + [entity("m1", bytes: 10_239)],
            Array(current.dropLast()) + [entity("n1")]
        ] {
            #expect(throws: AgentFailure.execution) {
                try CloudKitSmokeProof.acknowledgements(current: invalid, retained: retained, observations: events)
            }
        }
        for incomplete in [Array(events.prefix(1)), Array(events.dropLast()), []] {
            #expect(throws: AgentFailure.execution) {
                try CloudKitSmokeProof.acknowledgements(current: current, retained: retained, observations: incomplete)
            }
        }
    }
    @Test func generatedMediaUsesAll320RunBoundSHA256Blocks() {
        let bytes = CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "generated-mac")
        #expect(bytes.count == 10_240)
        #expect(bytes == CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "generated-mac"))
        #expect(bytes != CloudKitSmokeGeneratedInput.media(run: "other-run", device: "generated-mac"))
        #expect(bytes != CloudKitSmokeGeneratedInput.media(run: "generated-run", device: "other-mac"))
        #expect(Set(stride(from: 0, to: bytes.count, by: 32).map { bytes.subdata(in: $0..<($0 + 32)) }).count == 320)
    }
}
#endif
