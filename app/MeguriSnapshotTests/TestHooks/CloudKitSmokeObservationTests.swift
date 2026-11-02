#if DEBUG && MEGURI_TEST_HOOKS
@testable import CoreBridge
import Foundation
import CryptoKit
import Testing

// Generated observations exercise refusal logic only. They are never actual
// CloudKit success records, and this suite belongs outside the smoke selection.
@MainActor @Suite(.serialized) struct CloudKitSmokeObservationTests {
    private func entity(_ label: String, revision: UInt64 = 7, acked: UInt64 = 7,
                        digest: UInt8 = 11, acknowledged: Bool = true,
                        blob: Bool = true, bytes: UInt64 = 10_240, ackEnvelopeByte: UInt8 = 1) -> SmokeCloudEntity {
        .init(label: label, recordName: "generated-record-" + label, revision: revision,
              ackedRevision: acked, digest: Data(repeating: digest, count: 32),
              ackedEnvelopeDigest: Data(SHA256.hash(data: Data([ackEnvelopeByte]))), acknowledged: acknowledged,
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

    private func current(engine: String = "generated-engine", generation: UInt64 = 11) -> SmokeCloudObservation {
        .init(generation: generation, activeSession: nil, seq: 20, publicationID: nil, publicationReason: nil,
              publicationCompleting: false, publicationBindingID: nil, bindingID: nil,
              publicationInitial: false, publicationComplete: true, bindingUsable: true,
              engine: engine, entities: [], cardNoteID: nil, lastReviewID: nil)
    }
    private func ledger() throws -> CloudKitSmokeEvidence {
        let evidence = CloudKitSmokeEvidence()
        try evidence.setGeneration(11)
        return evidence
    }
    private func stateCallback(_ evidence: CloudKitSmokeEvidence, ordinal: UInt64 = 1) {
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: ordinal,
                               event: .stateUpdate(Data([9])), phase: phase), current: current())
        }
    }
    @Test func originalOwnAttemptRequiresPrepareEnqueueReplyThenCommit() throws {
        let identity = UUID()
        let metadata = CloudOwnMetadata(operationID: Data([1]), attempt: 4, kind: .createZones)
        let phases: [CloudOwnObservationPhase] = [.prepared, .enqueued(true), .replied(.zonesCreated), .committed]
        let valid = try ledger()
        for phase in phases { valid.own(.init(metadata: metadata, identity: identity, phase: phase)) }
        try valid.check()
        let facts = valid.drain()
        #expect(facts.map(\.count) == [1, 2, 3, 4])
        #expect(facts.map(\.name) == [.applied, .applied, .applied, .stateSaved])
        #expect(facts.map(\.operation) == ["own1-createZones-prepared", "own1-createZones-enqueued", "own1-createZones-replied", "own1-createZones-committed"])
        let invalidPhases: [[CloudOwnObservationPhase]] = [
            [.prepared, .committed], [.prepared, .enqueued(false)],
            [.prepared, .prepared], [.enqueued(true)],
            [.prepared, .enqueued(true), .replied(.notStarted)]
        ]
        for invalid in invalidPhases {
            let evidence = try ledger()
            for phase in invalid { evidence.own(.init(metadata: metadata, identity: identity, phase: phase)) }
            #expect(throws: AgentFailure.execution) { try evidence.check() }
        }
        let wrongOwner = try ledger()
        wrongOwner.own(.init(metadata: metadata, identity: identity, phase: .prepared))
        wrongOwner.own(.init(metadata: metadata, identity: UUID(), phase: .enqueued(true)))
        #expect(throws: AgentFailure.execution) { try wrongOwner.check() }
        let wrongAttempt = try ledger()
        wrongAttempt.own(.init(metadata: metadata, identity: identity, phase: .prepared))
        wrongAttempt.own(.init(metadata: .init(operationID: Data([1]), attempt: 5, kind: .createZones), identity: identity, phase: .enqueued(true)))
        #expect(throws: AgentFailure.execution) { try wrongAttempt.check() }
    }
    @Test func sdkStateSaveRequiresOriginalDeliveryCommitAndReturnWithoutOpaqueFacts() throws {
        let valid = try ledger()
        stateCallback(valid)
        try valid.check()
        let facts = valid.drain()
        #expect(facts.map(\.count) == [1, 2, 3])
        #expect(facts.map(\.name) == [.engineCallback, .stateSaved, .engineCallback])
        #expect(facts.allSatisfy { $0.engine == "engine1" && $0.digest == nil })
        let invalidPhases: [[CloudSDKObservationPhase]] = [
            [.committed], [.delivered, .returned], [.delivered, .delivered],
            [.delivered, .committed, .committed]
        ]
        for phases in invalidPhases {
            let invalid = try ledger()
            for phase in phases {
                invalid.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([9])), phase: phase), current: current())
            }
            #expect(throws: AgentFailure.execution) { try invalid.check() }
        }
        for wrong in [current(engine: "replacement"), current(generation: 12)] {
            let invalid = try ledger()
            invalid.sdk(.init(origin: "generated-engine", ordinal: 1, event: .stateUpdate(Data([9])), phase: .delivered), current: wrong)
            #expect(throws: AgentFailure.execution) { try invalid.check() }
        }
    }
    private func heldProjection(cardDigest: UInt8 = 11, reviewDigest: Data? = nil,
                                session: UInt64 = 99, generation: UInt64 = 11) -> SmokeBatchProof {
        .init(generation: generation, seq: 20, session: session, saves: 0, images: [
            .init(name: "generated-record-c1", envelopeDigest: nil, digest: Data(repeating: cardDigest, count: 32), imageSeq: 19, held: true),
            .init(name: "generated-record-r1", envelopeDigest: nil, digest: reviewDigest, imageSeq: 19, held: true)
        ])
    }
    private func retainHeld(_ evidence: CloudKitSmokeEvidence, _ projection: SmokeBatchProof) throws -> [AgentFact] {
        try evidence.held(projection, session: 99, answerSeq: 20,
                          card: entity("c1", revision: 8, digest: 12), review: entity("r1"), baseline: entity("c1"))
    }
    @Test func zeroActualBatchesStillNeedsSameHeldProjectionAndCannotClaimAnUpload() throws {
        let valid = try ledger()
        let facts = try retainHeld(valid, heldProjection())
        #expect(facts.map(\.operation) == ["c1-held-projection", "r1-held-absent"])
        stateCallback(valid)
        let joined = try valid.joined()
        #expect(joined.first { $0.operation == "held-collection-batches" }?.count == 0)
        #expect(joined.first { $0.operation == "c1-held-handed" }?.count == 0)
        for invalid in [heldProjection(cardDigest: 12), heldProjection(reviewDigest: Data(repeating: 11, count: 32)),
                        heldProjection(session: 100), heldProjection(generation: 12)] {
            #expect(throws: AgentFailure.execution) { try retainHeld(ledger(), invalid) }
        }
        #expect(throws: AgentFailure.execution) { try retainHeld(valid, heldProjection()) }
    }
    @Test func heldHandoffRejectsPrematureAnswerImageAndDuplicateOrAlteredBatch() throws {
        let record = CloudRecord(name: "generated-record-c1", envelope: Data([1]), systemFields: nil)
        let envelopeDigest = Data(SHA256.hash(data: record.envelope))
        for digest in [UInt8(11), UInt8(12)] {
            let evidence = try ledger()
            _ = try retainHeld(evidence, heldProjection())
            let proof = SmokeBatchProof(generation: 11, seq: 20, session: 99, saves: 1, images: [
                .init(name: record.name, envelopeDigest: envelopeDigest, digest: Data(repeating: digest, count: 32), imageSeq: 19, held: true)
            ])
            evidence.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
            stateCallback(evidence)
            if digest == 11 {
                #expect(try evidence.joined().first { $0.operation == "c1-held-handed" }?.count == 1)
                evidence.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
                #expect(throws: AgentFailure.execution) { try evidence.check() }
            } else {
                #expect(throws: AgentFailure.execution) { try evidence.joined() }
            }
        }
        let invalidEnvelope = try ledger()
        let proof = SmokeBatchProof(generation: 11, seq: 20, session: 99, saves: 1, images: [
            .init(name: record.name, envelopeDigest: Data(repeating: 0, count: 32), digest: Data(repeating: 11, count: 32), imageSeq: 19, held: true)
        ])
        invalidEnvelope.batch(origin: "generated-engine", batch: .init(id: 5, saves: [record], deletes: []), proof: proof, current: current())
        #expect(throws: AgentFailure.execution) { try invalidEnvelope.check() }
    }
    @Test func acknowledgementJoinsOriginalBatchSentReturnRemovalAndLaterStateCommit() throws {
        let record = CloudRecord(name: "generated-record-n1", envelope: Data([1]), systemFields: Data([2]))
        let evidence = try ledger()
        evidence.batch(origin: "generated-engine", batch: .init(id: 9, saves: [record], deletes: []), proof: nil, current: current())
        let sent = CloudEngineEvent.sent(batchID: 9, saved: [record], failed: [], deleted: [])
        for phase in [CloudSDKObservationPhase.delivered, .committed, .returned] {
            evidence.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: phase), current: current())
        }
        let removed = [CloudWorkflowObservation.savedPendingRemoved(name: record.name, requestID: 9)]
        #expect(throws: AgentFailure.execution) {
            try evidence.requireAcknowledgements([entity("n1")], observations: removed)
        }
        stateCallback(evidence, ordinal: 2)
        let facts = try evidence.requireAcknowledgements([entity("n1")], observations: removed)
        #expect(facts.first?.operation == "n1-sdk1-batch1-ack")
        #expect(throws: AgentFailure.execution) {
            try evidence.requireAcknowledgements([entity("n1", ackEnvelopeByte: 2)], observations: removed)
        }
        for wrong in [[], [.savedPendingRemoved(name: record.name, requestID: 8)]] as [[CloudWorkflowObservation]] {
            #expect(throws: AgentFailure.execution) {
                try evidence.requireAcknowledgements([entity("n1")], observations: wrong)
            }
        }
        let unowned = try ledger()
        unowned.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .delivered), current: current())
        unowned.sdk(.init(origin: "generated-engine", ordinal: 1, event: sent, phase: .committed), current: current())
        #expect(throws: AgentFailure.execution) { try unowned.check() }
    }
}
#endif
