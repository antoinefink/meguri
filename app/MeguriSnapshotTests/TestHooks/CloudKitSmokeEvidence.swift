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
        var publicationEvent: SelectedCloudPublicationEvent?
        var committedAt: UInt64?
    }
    private struct Callback {
        let alias: String
        let kind: String
        let batch: UInt64?
        var phase = 0
        var returned: UInt64?
        var saved: Set<String> = []
        var savedRecords: [String: Data] = [:]
        var stateDigest: Data?
        var received: [String: Data] = [:]
        var zone: String?
        var group: String?
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
        var joined = false
        var selectedPublication = false
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
        var adoptingFrom: UInt64?
        var retiredGenerations: Set<UInt64> = []
        var sdkAdmissions: [String: UInt64] = [:]
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
    func setGeneration(_ generation: UInt64, selectedPublication: Bool = false) throws {
        try state.withLock {
            guard $0.generation == nil else { throw AgentFailure.execution }
            $0.generation = generation
            $0.selectedPublication = selectedPublication
        }
    }
    func fail() { state.withLock { $0.failed = true } }
    func check() throws { try state.withLock { if $0.failed { throw AgentFailure.execution } } }
    func drain() -> [AgentFact] { state.withLock { let facts = $0.facts; $0.facts.removeAll(); return facts } }

    func beginAdoption() throws {
        try state.withLock { s in
            guard !s.failed, let generation = s.generation, s.adoptingFrom == nil,
                  s.origin == nil, s.callbacks.isEmpty, s.batches.isEmpty,
                  s.own.values.allSatisfy({ $0.phase == 4 }) else { throw AgentFailure.execution }
            s.adoptingFrom = generation
        }
    }
    // Called at the actual factory boundary after current-Host control/scan and
    // independently retained original-role validation. No reset or caller-selected generation.
    func admitSDK(origin: String, control: SelectedCloudControlObservation, replacement: CoreEvent?) throws {
        try state.withLock { s in
            guard !s.failed, control.requiredScanComplete, s.sdkAdmissions[origin] == nil else { throw AgentFailure.execution }
            if let old = s.adoptingFrom {
                guard s.generation == old, control.generation > old,
                      control.status == .proven, control.provenance == .retainedAdoption,
                      control.bindingMatchesEpoch == .proven, control.activation == .proven, control.ownedBackup == .proven,
                      let replacement, replacement.generation == control.generation,
                      case .collectionReplaced = replacement.kind,
                      s.origin == nil, s.callbacks.isEmpty, s.batches.isEmpty,
                      s.own.values.allSatisfy({ $0.phase == 4 }) else { throw AgentFailure.execution }
                s.retiredGenerations.insert(old)
                s.generation = control.generation
                s.adoptingFrom = nil
                s.append(.applied, "normal-adoption-generation-joined")
            }
            guard s.generation == control.generation, !s.retiredGenerations.contains(control.generation) else { throw AgentFailure.execution }
            s.sdkAdmissions[origin] = control.generation
            s.append(.applied, "required-scan-before-native-sdk")
        }
    }
    // A new opening starts an empty ledger. Only its actual SDK admission is
    // added here; original journal callbacks/commits/ticks are never imported.
    func admitRestoredSDK(origin: String, control: RestoredCloudControlObservation) throws {
        try state.withLock { s in
            guard !s.failed, s.generation == control.generation, control.requiredScanComplete,
                  s.sdkAdmissions[origin] == nil, s.adoptingFrom == nil else { throw AgentFailure.execution }
            s.sdkAdmissions[origin] = control.generation
            s.append(.applied, "required-scan-before-native-sdk")
        }
    }
    func requireSelectedAdmission(current: SmokeCloudObservation, control: SelectedCloudControlObservation) throws {
        try state.withLock { s in
            guard !s.failed, s.adoptingFrom == nil, control.requiredScanComplete,
                  current.generation == control.generation, current.generation == s.generation,
                  let origin = current.engine, s.sdkAdmissions[origin] == current.generation else { throw AgentFailure.execution }
        }
    }

    func own(_ observation: CloudOwnObservation) {
        state.withLock { s in
            do {
                guard !s.failed, !s.joined, s.generation != nil else { throw AgentFailure.execution }
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
                    case ("saveCompleteEpoch", .conflict), ("saveActive", .conflict):
                        guard s.selectedPublication, let event = observation.publicationEvent,
                              event.origin == .conflict, event.attempt == own.attempt,
                              event.correlation.count == 32,
                              event.kind == (kind == "saveCompleteEpoch" ? .completeEpoch : .doneActive) else {
                            throw AgentFailure.execution
                        }
                    default: throw AgentFailure.execution
                    }
                    // Keep the actual typed reply, including the server's opaque
                    // media root. Only the original Core call validates it.
                    own.result = result
                    own.publicationEvent = observation.publicationEvent
                    expected = 2; suffix = "replied"
                case .committed:
                    guard own.result != nil, own.publicationEvent == observation.publicationEvent else { throw AgentFailure.execution }
                    expected = 3; suffix = "committed"
                }
                guard own.phase == expected else { throw AgentFailure.execution }
                own.phase += 1; s.own[observation.identity] = own
                s.append(expected == 3 ? .stateSaved : .applied, own.alias + "-" + suffix)
                if expected == 3 { own.committedAt = s.tick; s.own[observation.identity] = own }
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
    func requireSelectedPublication(_ control: SelectedCloudControlObservation) throws {
        try state.withLock { s in
            guard !s.failed, s.selectedPublication, control.generation == s.generation, control.status == .proven,
                  control.provenance == .runtimeAcceptedPublication,
                  let complete = control.publicationComplete, complete.kind == .completeEpoch,
                  let done = control.publicationDoneActive, done.kind == .doneActive,
                  complete.correlation.count == 32, done.correlation.count == 32 else { throw AgentFailure.execution }
            var committed: [UInt64] = []
            for event in [complete, done] {
                let joined = s.own.values.filter { $0.phase == 4 && $0.publicationEvent == event && $0.attempt == event.attempt }
                guard joined.count == 1, let own = joined.first, let at = own.committedAt,
                      own.kind == (event.kind == .completeEpoch ? "saveCompleteEpoch" : "saveActive") else { throw AgentFailure.execution }
                committed.append(at)
            }
            guard committed[0] < committed[1] else { throw AgentFailure.execution }
        }
    }

    func validate(origin: String, current: SmokeCloudObservation) {
        state.withLock { s in
            do {
                guard !s.joined, current.engine == origin else { throw AgentFailure.execution }
                try s.bind(origin, generation: current.generation)
            } catch { s.failed = true }
        }
    }
    func sdk(_ observation: CloudSDKObservation, current: SmokeCloudObservation? = nil) {
        state.withLock { s in
            do {
                guard !s.failed, !s.joined, let generation = s.generation else { throw AgentFailure.execution }
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
                var received: [String: Data] = [:]
                var stateDigest: Data?
                var zone: String?
                var group: String?
                switch observation.event {
                case .stateUpdate(let data): kind = "state"; batch = nil; saved = []; stateDigest = Self.hash(data)
                case .sent(let id, let records, let failures, let deleted, let failedDeletes):
                    guard failures.isEmpty, deleted.isEmpty, failedDeletes.isEmpty else { throw AgentFailure.execution }
                    kind = "sent"; batch = id; saved = records
                case .deliveryFailed: throw AgentFailure.execution
                case .willFetch(let id, _) where s.selectedPublication:
                    kind = "fetch-start"; batch = nil; saved = []; group = id
                case .willFetchZone(let id) where s.selectedPublication:
                    kind = "fetch-zone-start"; batch = nil; saved = []; zone = id
                case .fetched(let id, let records, let deleted) where s.selectedPublication:
                    guard deleted.isEmpty, Set(records.map(\.name)).count == records.count else { throw AgentFailure.execution }
                    kind = "fetched"; batch = nil; saved = []; zone = id
                    received = Dictionary(uniqueKeysWithValues: records.map { ($0.name, Self.hash($0.envelope)) })
                case .didFetchZone(let id, let error) where s.selectedPublication:
                    guard error == nil else { throw AgentFailure.execution }
                    kind = "fetch-zone-end"; batch = nil; saved = []; zone = id
                case .didFetch(let id, _) where s.selectedPublication:
                    kind = "fetch-end"; batch = nil; saved = []; group = id
                default: return
                }
                guard Set(saved.map(\.name)).count == saved.count else { throw AgentFailure.execution }
                let savedRecords = Dictionary(uniqueKeysWithValues: saved.map { ($0.name, Self.hash($0.envelope)) })
                if case .delivered = observation.phase {
                    guard s.callbacks.count < 1024, observation.ordinal > s.lastOrdinal,
                          s.callbacks.values.allSatisfy({ $0.phase == 3 }) else { throw AgentFailure.execution }
                    s.lastOrdinal = observation.ordinal
                    var cb = Callback(alias: "sdk\(s.callbacks.count + 1)", kind: kind, batch: batch)
                    cb.savedRecords = savedRecords; cb.stateDigest = stateDigest
                    cb.received = received; cb.zone = zone; cb.group = group
                    s.callbacks[observation.ordinal] = cb
                }
                guard var cb = s.callbacks[observation.ordinal], cb.kind == kind, cb.batch == batch,
                      cb.savedRecords == savedRecords, cb.stateDigest == stateDigest,
                      cb.received == received, cb.zone == zone, cb.group == group else { throw AgentFailure.execution }
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
                guard !s.failed, !s.joined, s.batches.count < 256, s.batches[batch.id] == nil,
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
    func acknowledgementBoundary() throws -> UInt64 {
        try state.withLock { s in
            guard !s.failed else { throw AgentFailure.execution }
            return s.tick
        }
    }
    func requireSelectedAcknowledgement(_ entity: SmokeCloudEntity, observations: [CloudWorkflowObservation],
                                        attemptBoundary: UInt64) throws -> [AgentFact] {
        let joined = try requireAcknowledgements([entity], observations: observations)
        guard let returned = joined.first?.count else { throw AgentFailure.execution }
        // This identifies the original ACK chain. The attempt sample precedes
        // prepare/request, while SDK events can arrive on another actor. A later
        // tick establishes no ordering against the actual manual request.
        return joined + [.init(name: .stateSaved, count: entity.revision,
            digest: CloudKitSmokeGeneratedInput.hex(entity.digest),
            operation: returned <= attemptBoundary ? "n1-earlier-original-ack" : "n1-original-ack-after-attempt-boundary-timing-unresolved")]
    }
    func requireSelectedFetch(_ entity: SmokeCloudEntity) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed, s.selectedPublication else { throw AgentFailure.execution }
            let callbacks = s.callbacks.values.filter { $0.phase == 3 }.sorted { ($0.returned ?? 0) < ($1.returned ?? 0) }
            for page in callbacks where page.kind == "fetched" && page.received[entity.recordName] == entity.ackedEnvelopeDigest {
                guard let pageAt = page.returned, let zone = page.zone,
                      let start = callbacks.last(where: { $0.kind == "fetch-start" && ($0.returned ?? .max) < pageAt }),
                      let startAt = start.returned,
                      let zoneStart = callbacks.last(where: { $0.kind == "fetch-zone-start" && $0.zone == zone && ($0.returned ?? .max) < pageAt }),
                      let zoneStartAt = zoneStart.returned, zoneStartAt > startAt,
                      let zoneEnd = callbacks.first(where: { $0.kind == "fetch-zone-end" && $0.zone == zone && ($0.returned ?? 0) > pageAt }),
                      let zoneEndAt = zoneEnd.returned,
                      callbacks.contains(where: { $0.kind == "fetch-end" && $0.group == start.group && ($0.returned ?? 0) > zoneEndAt }) else { continue }
                return [.init(name: .engineCallback, count: pageAt, engine: "engine1", operation: "n1-original-fetched-returned")]
            }
            throw AgentFailure.implementationMissing
        }
    }
    // Common original-attempt/callback join. Selected programs have no held
    // review; their earlier source ACK or peer apply proof is retained by runtime.
    func joinedOriginalWork() throws -> [AgentFact] {
        try state.withLock { s in
            let facts = try Self.originalWorkJoin(s)
            s.joined = true
            return facts
        }
    }
    private static func originalWorkJoin(_ s: State) throws -> [AgentFact] {
        guard !s.failed, !s.joined, s.generation != nil, s.origin != nil, s.adoptingFrom == nil,
              s.own.values.allSatisfy({ $0.phase == 4 }),
              !s.callbacks.isEmpty, s.callbacks.values.allSatisfy({ cb in
                  cb.phase == 3 && (cb.saved.isEmpty || s.stateCommits.contains { $0 > (cb.returned ?? UInt64.max) })
              }) else { throw AgentFailure.execution }
        return [.init(name: .ownedWorkJoined, count: UInt64(s.own.count), operation: "own-joined"),
                .init(name: .ownedWorkJoined, count: UInt64(s.callbacks.count), engine: "engine1", operation: "sdk-joined")]
    }
    func joined() throws -> [AgentFact] {
        try state.withLock { s in
            let originalWork = try Self.originalWorkJoin(s)
            guard let held = s.held else { throw AgentFailure.execution }
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
            s.joined = true
            return facts + [.init(name: .applied, count: UInt64(batches.count + mediaBatches.count), engine: "engine1", operation: "held-batches"),
                    .init(name: .applied, count: UInt64(mediaBatches.count), engine: "engine1", operation: "held-media-batches"),
                    .init(name: .applied, count: UInt64(batches.count), engine: "engine1", operation: "held-collection-batches"),
                    .init(name: .applied, count: UInt64(batches.flatMap(\.images).filter { $0.name == held.card }.count), engine: "engine1", operation: "c1-held-handed"),
                    .init(name: .applied, count: 0, engine: "engine1", operation: "r1-held-handed")] + originalWork
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
