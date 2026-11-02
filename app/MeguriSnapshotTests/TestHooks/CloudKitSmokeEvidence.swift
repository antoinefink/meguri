#if DEBUG && MEGURI_TEST_HOOKS
import CoreBridge
import CryptoKit
import Foundation
import Synchronization

// One bounded run-local ledger. Synchronous enqueue observations cannot hop to
// a Task: that would destroy their ordering relative to the original reply.
nonisolated final class CloudKitSmokeEvidence: Sendable {
    private struct Own {
        let alias: String
        let operation: Data
        let attempt: UInt64
        let kind: String
        var phase = 0
        var result: CloudPublicationResult?
    }
    private struct Callback {
        let alias: String
        let kind: String
        let batch: UInt64?
        var phase = 0
        var returned: UInt64?
        var saved: Set<String> = []
    }
    private struct Batch {
        let alias: String
        let ordinal: Int
        let records: [String: Data]
        let proof: SmokeBatchProof?
        let session: UInt64?
        let seq: UInt64
    }
    private struct Held {
        let session: UInt64
        let seq: UInt64
        let card: String
        let review: String
        let baseline: Data
        let answer: Data
    }
    private struct State {
        var failed = false
        var generation: UInt64?
        var origin: String?
        var tick: UInt64 = 0
        var facts: [AgentFact] = []
        var own: [UUID: Own] = [:]
        var callbacks: [UInt64: Callback] = [:]
        var batches: [UInt64: Batch] = [:]
        var lastOrdinal: UInt64 = 0
        var held: Held?
        var stateCommits: [UInt64] = []
        mutating func append(_ name: AgentFact.Name, _ label: String, engine: Bool = false) {
            guard tick < 4096 else { failed = true; return }
            tick += 1
            facts.append(.init(name: name, count: tick, engine: engine ? "engine1" : nil, operation: label))
        }
        mutating func bind(_ origin: String, generation: UInt64) throws {
            guard self.generation == generation, self.origin == nil || self.origin == origin else { throw AgentFailure.execution }
            self.origin = origin
        }
    }
    private let state = Mutex(State())
    func setGeneration(_ generation: UInt64) throws {
        try state.withLock {
            guard $0.generation == nil else { throw AgentFailure.execution }
            $0.generation = generation
        }
    }
    func fail() { state.withLock { $0.failed = true } }
    func check() throws { try state.withLock { if $0.failed { throw AgentFailure.execution } } }
    func drain() -> [AgentFact] { state.withLock { let facts = $0.facts; $0.facts.removeAll(); return facts } }

    func own(_ observation: CloudOwnObservation) {
        state.withLock { s in
            do {
                guard !s.failed, s.generation != nil else { throw AgentFailure.execution }
                let kind = Self.kind(observation.metadata.kind)
                if case .prepared = observation.phase {
                    guard s.own.count < 256, s.own[observation.identity] == nil,
                          !s.own.values.contains(where: { $0.operation == observation.metadata.operationID && $0.attempt == observation.metadata.attempt && $0.kind == kind }) else { throw AgentFailure.execution }
                    s.own[observation.identity] = Own(alias: "own\(s.own.count + 1)-\(kind)",
                        operation: observation.metadata.operationID, attempt: observation.metadata.attempt, kind: kind)
                }
                guard var own = s.own[observation.identity], own.operation == observation.metadata.operationID,
                      own.attempt == observation.metadata.attempt, own.kind == kind else { throw AgentFailure.execution }
                let expected: Int
                let suffix: String
                switch observation.phase {
                case .prepared: expected = 0; suffix = "prepared"
                case .enqueued(let accepted):
                    guard accepted else { throw AgentFailure.execution }
                    expected = 1; suffix = "enqueued"
                case .replied(let result):
                    switch (kind, result) {
                    case ("createZones", .zonesCreated), ("saveMediaRoot", .saved), ("saveMediaRoot", .conflict),
                         ("savePublishingEpoch", .saved), ("saveCompleteEpoch", .saved),
                         ("saveActive", .saved), ("readOwnEpoch", .read), ("readResetMediaRoot", .read): break
                    default: throw AgentFailure.execution
                    }
                    // Keep the actual typed reply, including the server's opaque
                    // media root. Only the original Core call validates it.
                    own.result = result
                    expected = 2; suffix = "replied"
                case .committed:
                    guard own.result != nil else { throw AgentFailure.execution }
                    expected = 3; suffix = "committed"
                }
                guard own.phase == expected else { throw AgentFailure.execution }
                own.phase += 1; s.own[observation.identity] = own
                s.append(expected == 3 ? .stateSaved : .applied, own.alias + "-" + suffix)
            } catch { s.failed = true }
        }
    }
    func requirePublication(_ id: Data, completed: SmokeCloudObservation) throws {
        try state.withLock { s in
            guard !s.failed, !s.own.isEmpty,
                  completed.generation == s.generation, completed.publicationID == nil,
                  completed.publicationComplete, completed.bindingUsable, completed.engine != nil,
                  s.own.values.allSatisfy({ $0.operation == id && $0.phase == 4 }),
                  Set(s.own.values.map(\.kind)).isSuperset(of: ["createZones", "saveMediaRoot", "savePublishingEpoch", "saveCompleteEpoch", "saveActive"]) else { throw AgentFailure.execution }
        }
    }

    func validate(origin: String, current: SmokeCloudObservation) {
        state.withLock { s in
            do {
                guard current.engine == origin else { throw AgentFailure.execution }
                try s.bind(origin, generation: current.generation)
            } catch { s.failed = true }
        }
    }
    func sdk(_ observation: CloudSDKObservation, current: SmokeCloudObservation? = nil) {
        state.withLock { s in
            do {
                guard !s.failed, let generation = s.generation else { throw AgentFailure.execution }
                if let current {
                    guard current.engine == observation.origin else { throw AgentFailure.execution }
                    try s.bind(observation.origin, generation: current.generation)
                } else {
                    // Capture the boundary before the observer's first await.
                    // Fresh Core ownership is checked before returning to SDK.
                    try s.bind(observation.origin, generation: generation)
                }
                let kind: String
                let batch: UInt64?
                let saved: [CloudRecord]
                switch observation.event {
                case .stateUpdate: kind = "state"; batch = nil; saved = []
                case .sent(let id, let records, let failures, let deleted, let failedDeletes):
                    guard failures.isEmpty, deleted.isEmpty, failedDeletes.isEmpty else { throw AgentFailure.execution }
                    kind = "sent"; batch = id; saved = records
                case .deliveryFailed: throw AgentFailure.execution
                default: return
                }
                if case .delivered = observation.phase {
                    guard s.callbacks.count < 1024, observation.ordinal > s.lastOrdinal,
                          s.callbacks.values.allSatisfy({ $0.phase == 3 }) else { throw AgentFailure.execution }
                    s.lastOrdinal = observation.ordinal
                    s.callbacks[observation.ordinal] = Callback(alias: "sdk\(s.callbacks.count + 1)", kind: kind, batch: batch)
                }
                guard var cb = s.callbacks[observation.ordinal], cb.kind == kind, cb.batch == batch else { throw AgentFailure.execution }
                let expected: Int
                let suffix: String
                switch observation.phase {
                case .delivered: expected = 0; suffix = "delivered"
                case .committed:
                    expected = 1; suffix = "committed"
                    if let batch {
                        guard let handed = s.batches[batch], Set(saved.map(\.name)).count == saved.count,
                              saved.allSatisfy({ handed.records[$0.name] == Self.hash($0.envelope) }) else { throw AgentFailure.execution }
                        cb.saved = Set(saved.map(\.name))
                    }
                case .returned: expected = 2; suffix = "returned"
                }
                guard cb.phase == expected else { throw AgentFailure.execution }
                cb.phase += 1
                let batchLabel = batch.flatMap { s.batches[$0]?.alias }.map { "-" + $0 } ?? ""
                s.append(kind == "state" && expected == 1 ? .stateSaved : .engineCallback,
                         cb.alias + "-" + kind + batchLabel + "-" + suffix, engine: true)
                if expected == 2 { cb.returned = s.tick }
                if kind == "state" && expected == 1 { s.stateCommits.append(s.tick) }
                s.callbacks[observation.ordinal] = cb
            } catch { s.failed = true }
        }
    }
    func batch(origin: String, batch: CloudEngineBatch, proof: SmokeBatchProof?, current: SmokeCloudObservation) {
        state.withLock { s in
            do {
                guard !s.failed, s.batches.count < 256, s.batches[batch.id] == nil,
                      current.engine == origin, batch.deletes.isEmpty,
                      Set(batch.saves.map(\.name)).count == batch.saves.count else { throw AgentFailure.execution }
                try s.bind(origin, generation: current.generation)
                if let proof {
                    guard proof.generation == current.generation, proof.saves == UInt64(batch.saves.count),
                          Set(proof.images.map(\.name)).count == proof.images.count,
                          proof.images.allSatisfy({ image in
                              image.digest?.count == 32 && image.imageSeq <= proof.seq &&
                              batch.saves.contains { $0.name == image.name && Self.hash($0.envelope) == image.envelopeDigest }
                          }) else { throw AgentFailure.execution }
                }
                let alias = "batch\(s.batches.count + 1)"
                s.batches[batch.id] = Batch(alias: alias, ordinal: s.batches.count + 1,
                    records: Dictionary(uniqueKeysWithValues: batch.saves.map { ($0.name, Self.hash($0.envelope)) }),
                    proof: proof, session: proof?.session ?? current.activeSession, seq: proof?.seq ?? current.seq)
                s.append(.applied, alias + (proof == nil ? "-media-handoff" : "-collection-handoff"), engine: true)
                s.facts.append(.init(name: .applied, count: UInt64(batch.saves.count), engine: "engine1", operation: alias + "-save-count"))
            } catch { s.failed = true }
        }
    }

    func held(_ proof: SmokeBatchProof, session: UInt64, answerSeq: UInt64,
              card: SmokeCloudEntity, review: SmokeCloudEntity, baseline: SmokeCloudEntity) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed, s.held == nil, proof.generation == s.generation,
                  proof.session == session, proof.seq >= answerSeq,
                  card.recordName == baseline.recordName, card.digest != baseline.digest,
                  proof.images.count == 2,
                  let c = proof.images.first(where: { $0.name == card.recordName }),
                  let r = proof.images.first(where: { $0.name == review.recordName }),
                  c.held, c.digest == baseline.digest, c.imageSeq < answerSeq,
                  r.held, r.digest == nil, r.imageSeq < answerSeq else { throw AgentFailure.execution }
            s.held = Held(session: session, seq: answerSeq, card: card.recordName, review: review.recordName,
                          baseline: baseline.digest, answer: card.digest)
            return [.init(name: .applied, count: c.imageSeq, digest: CloudKitSmokeGeneratedInput.hex(baseline.digest), engine: "engine1", operation: "c1-held-projection"),
                    .init(name: .applied, count: 0, engine: "engine1", operation: "r1-held-absent")]
        }
    }
    func requireAcknowledgements(_ entities: [SmokeCloudEntity], observations: [CloudWorkflowObservation]) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed else { throw AgentFailure.execution }
            return try entities.map { entity in
                let candidates = s.callbacks.values.filter { cb in
                    guard cb.kind == "sent", cb.phase == 3, cb.saved.contains(entity.recordName),
                          let batch = cb.batch, let returned = cb.returned,
                          s.batches[batch]?.records[entity.recordName] == entity.ackedEnvelopeDigest,
                          s.stateCommits.contains(where: { $0 > returned }) else { return false }
                    return observations.contains {
                        if case .savedPendingRemoved(let name, let request) = $0 { return name == entity.recordName && request == batch }; return false
                    }
                }
                guard let cb = candidates.sorted(by: { ($0.returned ?? 0) < ($1.returned ?? 0) }).last,
                      let batch = cb.batch, let alias = s.batches[batch]?.alias else { throw AgentFailure.execution }
                return .init(name: .applied, count: cb.returned, engine: "engine1", operation: entity.label + "-" + cb.alias + "-" + alias + "-ack")
            }
        }
    }
    func joined() throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed, s.own.values.allSatisfy({ $0.phase == 4 }),
                  !s.callbacks.isEmpty, s.callbacks.values.allSatisfy({ cb in
                      cb.phase == 3 && (cb.saved.isEmpty || s.stateCommits.contains { $0 > (cb.returned ?? UInt64.max) })
                  }),
                  let held = s.held else { throw AgentFailure.execution }
            let batches = s.batches.values.compactMap(\.proof).filter { $0.session == held.session && $0.seq >= held.seq }
            for batch in batches {
                for image in batch.images where image.name == held.card || image.name == held.review {
                    guard image.name != held.review, image.held, image.digest == held.baseline,
                          image.digest != held.answer, image.imageSeq < held.seq else { throw AgentFailure.execution }
                }
            }
            let mediaBatches = s.batches.values.filter { $0.proof == nil && $0.session == held.session && $0.seq >= held.seq }
            var facts: [AgentFact] = mediaBatches.map {
                .init(name: .applied, count: $0.seq, engine: "engine1", operation: $0.alias + "-s1-media-snapshot")
            }
            for batch in s.batches.values.sorted(by: { $0.ordinal < $1.ordinal }) {
                guard let proof = batch.proof, proof.session == held.session, proof.seq >= held.seq else { continue }
                facts.append(.init(name: .applied, count: proof.seq, engine: "engine1", operation: batch.alias + "-s1-snapshot"))
                if let card = proof.images.first(where: { $0.name == held.card }), let digest = card.digest {
                    facts.append(.init(name: .applied, count: card.imageSeq, digest: CloudKitSmokeGeneratedInput.hex(digest), engine: "engine1", operation: batch.alias + "-c1-held"))
                }
                facts.append(.init(name: .applied, count: 0, engine: "engine1", operation: batch.alias + "-r1-absent"))
            }
            return facts + [.init(name: .applied, count: UInt64(batches.count + mediaBatches.count), engine: "engine1", operation: "held-batches"),
                    .init(name: .applied, count: UInt64(mediaBatches.count), engine: "engine1", operation: "held-media-batches"),
                    .init(name: .applied, count: UInt64(batches.count), engine: "engine1", operation: "held-collection-batches"),
                    .init(name: .applied, count: UInt64(batches.flatMap(\.images).filter { $0.name == held.card }.count), engine: "engine1", operation: "c1-held-handed"),
                    .init(name: .applied, count: 0, engine: "engine1", operation: "r1-held-handed"),
                    .init(name: .ownedWorkJoined, count: UInt64(s.own.count), operation: "own-joined"),
                    .init(name: .ownedWorkJoined, count: UInt64(s.callbacks.count), engine: "engine1", operation: "sdk-joined")]
        }
    }
    private static func hash(_ data: Data) -> Data { Data(SHA256.hash(data: data)) }
    private static func kind(_ kind: CloudOwnKind) -> String {
        switch kind {
        case .createZones: "createZones"
        case .saveMediaRoot: "saveMediaRoot"
        case .savePublishingEpoch: "savePublishingEpoch"
        case .saveCompleteEpoch: "saveCompleteEpoch"
        case .readOwnEpoch: "readOwnEpoch"
        case .saveActive: "saveActive"
        case .readResetMediaRoot: "readResetMediaRoot"
        }
    }
}
#endif
