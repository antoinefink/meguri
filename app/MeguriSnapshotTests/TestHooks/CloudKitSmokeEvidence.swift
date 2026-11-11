#if DEBUG && MEGURI_TEST_HOOKS
import CloudKit
import CoreBridge
import CryptoKit
import Foundation
import Synchronization

// Only the signed test runtime installs this writer. SDK callbacks await it
// before returning. The mutex ledger releases its lock before any append.
@MainActor final class CloudKitSmokeJournal {
    private let evidence: CloudKitSmokeEvidence
    private var latest: AgentCallbackJournal?
    private var callbacks: [String: (ordinal: UInt64, writer: AgentObservationWriter)] = [:]

    init(evidence: CloudKitSmokeEvidence) { self.evidence = evidence }

    func install(_ journal: @escaping AgentCallbackJournal) { latest = journal }
    func writer() throws -> AgentObservationWriter {
        guard let latest else { throw AgentFailure.journal }
        return try latest()
    }

    func record(_ event: CloudSDKObservation, historical: [SmokeHistoricalAck] = []) {
        evidence.sdk(event, historical: historical)
        flush(event: event)
    }

    func recordBatch(origin: String, batch: CloudEngineBatch, proof: SmokeBatchProof?, current: SmokeCloudObservation,
                     operation: CloudNativeSendOperation? = nil) {
        evidence.batch(origin: origin, batch: batch, proof: proof, current: current, operation: operation)
        flush()
    }
    func recordSendReturn(origin: String, operation: CloudNativeSendOperation) {
        evidence.sendReturned(origin: origin, operation: operation)
        flush()
    }
    func recordPendingUpdate(origin: String, adds: Set<String>, removes: [String: Bool]) {
        evidence.pendingUpdated(origin: origin, adds: adds, removes: removes)
        flush()
    }

    private func flush(event: CloudSDKObservation? = nil) {
        let facts = evidence.drain()
        guard !facts.isEmpty else { return }
        do {
            let current = try writer()
            guard let event else {
                try current(facts)
                return
            }
            var callbackWriter = current
            func isCallback(_ fact: AgentFact) -> Bool {
                fact.engine != nil && (fact.name == .engineCallback || fact.name == .stateSaved)
            }
            if facts.contains(where: isCallback) {
                // No causal command is available here. Bind at delivery and
                // keep that command through commit, return and a later step.
                switch event.phase {
                case .delivered:
                    callbacks[event.origin] = (event.ordinal, current)
                case .committed, .returned:
                    guard let callback = callbacks[event.origin], callback.ordinal == event.ordinal else {
                        throw AgentFailure.journal
                    }
                    callbackWriter = callback.writer
                }
                if case .returned = event.phase { callbacks.removeValue(forKey: event.origin) }
            }
            var start = facts.startIndex
            while start < facts.endIndex {
                let callback = isCallback(facts[start])
                let end = facts[start...].firstIndex { isCallback($0) != callback } ?? facts.endIndex
                try (callback ? callbackWriter : current)(Array(facts[start..<end]))
                start = end
            }
        } catch { evidence.fail() }
    }
}

// One bounded run-local ledger. Synchronous enqueue observations cannot hop to
// a Task: that would destroy their ordering relative to the original reply.
nonisolated final class CloudKitSmokeEvidence: Sendable {
    private struct EngineKey: Hashable {
        let origin: String
        let id: UInt64
    }
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
    private enum RecordResult: Equatable {
        case saved, deleted, saveError(Int64), deleteError(Int64)

        var label: String {
            switch self {
            case .saved: "saved"
            case .deleted: "deleted"
            case .saveError(let code): "save-error-\(code)"
            case .deleteError(let code): "delete-error-\(code)"
            }
        }
        var isDelete: Bool {
            switch self {
            case .deleted, .deleteError: true
            case .saved, .saveError: false
            }
        }
        var keepsPending: Bool {
            let code: Int64
            switch self {
            case .saveError(let value), .deleteError(let value): code = value
            case .saved, .deleted: return false
            }
            switch CKError.Code(rawValue: Int(code)) {
            case .notAuthenticated, .accountTemporarilyUnavailable, .networkFailure, .networkUnavailable,
                 .requestRateLimited, .serviceUnavailable, .zoneBusy: return true
            default: return false
            }
        }
    }
    private struct Callback {
        let alias: String
        let engine: String
        let kind: String
        let batch: EngineKey?
        var phase = 0
        var returned: UInt64?
        var saved: Set<String> = []
        var savedRecords: [String: Data] = [:]
        var results: [String: RecordResult] = [:]
        var conflicts: [String: Data] = [:]
        var stateDigest: Data?
        var received: [String: Data] = [:]
        var zone: String?
        var group: String?
    }
    private struct Batch {
        let alias: String
        let engine: String
        let ordinal: Int
        let records: [String: Data]
        let deletes: Set<String>
        let proof: SmokeBatchProof?
        let session: UInt64?
        let seq: UInt64
        var send: String?
        var open: Set<String> = []
        var reported: Set<String> = []
        var returned = false
        var readded: Set<String> = []
        var firstStateSave: BatchStateSave?
        var removedWhileHeld: Set<String> = []
        var unreportedStateUnsupported = false

        var names: Set<String> { deletes.union(records.keys) }
    }
    private struct BatchStateSave {
        let callback: EngineKey
        let reported: Set<String>
    }
    private struct Send {
        let alias: String
        let engine: String
        var returned = false
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
        var needsPendingEvidence = false
        var generation: UInt64?
        var engineLabels: [String: String] = [:]
        var reservedLabels: Set<String> = []
        var tick: UInt64 = 0
        var facts: [AgentFact] = []
        var own: [UUID: Own] = [:]
        var callbacks: [EngineKey: Callback] = [:]
        var batches: [EngineKey: Batch] = [:]
        var sends: [CloudNativeSendOperation: Send] = [:]
        var entities: [String: String] = [:]
        var exchangeCaptures: [UInt64: Set<String>] = [:]
        var exchangeWork: Set<UInt64> = []
        // Only the last touch matters. Additions clear a prior removal.
        var heldRemovals: [String: Set<String>] = [:]
        var nextBatch = 1
        var nextSend = 1
        var nextEntity = 1
        var lastOrdinals: [String: UInt64] = [:]
        var held: Held?
        var stateReturns: [String: UInt64] = [:]
        var adoptingFrom: UInt64?
        var retiredGenerations: Set<UInt64> = []
        var sdkAdmissions: [String: UInt64] = [:]
        mutating func append(_ name: AgentFact.Name, _ label: String, engine: String? = nil) {
            guard tick < 4096 else { failed = true; return }
            tick += 1
            facts.append(.init(name: name, count: tick, engine: engine, operation: label))
        }
        mutating func admitEngine(_ origin: String) throws -> String {
            guard engineLabels[origin] == nil,
                  let label = (1...(reservedLabels.count + 1)).lazy.map({ "engine\($0)" })
                    .first(where: { !reservedLabels.contains($0) }) else {
                throw AgentFailure.execution
            }
            reservedLabels.insert(label)
            engineLabels[origin] = label
            return label
        }
        mutating func bind(_ origin: String, generation: UInt64) throws -> String {
            guard self.generation == generation else { throw AgentFailure.execution }
            if let label = engineLabels[origin] { return label }
            // The generated evidence fixtures have no factory. Live runtimes
            // admit each native engine at the factory boundary.
            guard engineLabels.isEmpty else { throw AgentFailure.execution }
            return try admitEngine(origin)
        }
        mutating func send(_ operation: CloudNativeSendOperation, engine: String) throws -> Send {
            if let existing = sends[operation] {
                guard existing.engine == engine, !existing.returned else { throw AgentFailure.execution }
                return existing
            }
            let original = Send(alias: "send\(nextSend)", engine: engine)
            nextSend += 1
            sends[operation] = original
            return original
        }
        mutating func entity(_ name: String) -> String {
            if let existing = entities[name] { return existing }
            let alias = "entity\(nextEntity)"
            nextEntity += 1
            entities[name] = alias
            return alias
        }
        mutating func reflectState(_ pending: Set<String>, engine: String, callback: Callback, callbackKey: EngineKey) throws {
            let known = batches.values.filter { $0.engine == engine && $0.send != nil }
            let names = Set(known.flatMap(\.names)).sorted()
            append(.stateSaved, "\(callback.alias)-state-batches", engine: engine)
            for name in names {
                guard let alias = entities[name] else { throw AgentFailure.execution }
                let status = pending.contains(name) ? "held" : "absent"
                append(.stateSaved, "\(callback.alias)-\(alias)-read-\(status)", engine: engine)
            }
            let keys = batches.keys.sorted { (batches[$0]?.ordinal ?? 0) < (batches[$1]?.ordinal ?? 0) }
            for key in keys {
                guard var batch = batches[key], batch.engine == engine, let send = batch.send else { continue }
                if batch.reported.isEmpty && !batch.names.isSubset(of: batch.removedWhileHeld) {
                    batch.unreportedStateUnsupported = true
                }
                batches[key] = batch
                guard let witness = batch.names.sorted().first(where: { name in
                          pending.contains(name) ? batch.readded.contains(name) : !heldRemovals[engine, default: []].contains(name)
                      }),
                      let alias = entities[witness] else { continue }
                let status = pending.contains(witness) ? "held" : "absent"
                append(.stateSaved, "\(callback.alias)-\(send)-\(batch.alias)-\(alias)-reflected-\(status)", engine: engine)
                if batch.firstStateSave == nil {
                    batch.firstStateSave = .init(callback: callbackKey, reported: batch.reported)
                }
                batches[key] = batch
            }
        }
    }
    private let state = Mutex(State())
    func setGeneration(_ generation: UInt64, selectedPublication: Bool = false, needsPendingEvidence: Bool = false) throws {
        try state.withLock {
            guard $0.generation == nil else { throw AgentFailure.execution }
            $0.generation = generation
            $0.selectedPublication = selectedPublication
            $0.needsPendingEvidence = needsPendingEvidence
        }
    }
    func fail() { state.withLock { $0.failed = true } }
    func captureFacts(exchange: UInt64, capture: OriginalCloudExchangeCapture) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed, !s.joined, s.generation != nil, exchange > 0,
                  s.exchangeCaptures[exchange] == nil,
                  Set(capture.selectedNames).count == capture.selectedNames.count else { throw AgentFailure.execution }
            s.exchangeCaptures[exchange] = Set(capture.selectedNames)
            return [
                .init(name: .syncResult, count: exchange, operation: "exchange-capture"),
                .init(name: .syncResult, count: capture.boundarySeq, operation: "exchange-boundary")
            ] + capture.selectedNames.map { name in
                .init(name: .syncResult, operation: "exchange-selected-\(s.entity(name))")
            }
        }
    }
    func workFacts(exchange: UInt64, work: OriginalCloudExchangeWork) throws -> [AgentFact] {
        try state.withLock { s in
            let held = Set(work.heldPendingNames), acked = Set(work.heldAcknowledgedNames)
            let unheld = Set(work.pendingUnheldNames)
            guard !s.failed, !s.joined, let selected = s.exchangeCaptures[exchange],
                  !s.exchangeWork.contains(exchange), held.count == work.heldPendingNames.count,
                  acked.count == work.heldAcknowledgedNames.count, unheld.count == work.pendingUnheldNames.count,
                  held.isDisjoint(with: unheld), acked.isSubset(of: selected) else { throw AgentFailure.execution }
            var facts: [AgentFact] = [.init(name: .syncResult, count: exchange, operation: "exchange-work")]
            for (prefix, names) in [("work-held-pending-", held), ("work-held-acked-", acked),
                                    ("work-pending-unheld-", unheld)] {
                facts += names.sorted().map { .init(name: .syncResult, operation: prefix + s.entity($0)) }
            }
            facts += CloudKitSelectedSyncProof.mediaFacts(work.media).map { original in
                var fact = original
                fact.operation = original.operation.map { "work-" + $0 }
                return fact
            }
            s.exchangeWork.insert(exchange)
            return facts
        }
    }
    func check() throws { try state.withLock { if $0.failed { throw AgentFailure.execution } } }
    func drain() -> [AgentFact] { state.withLock { let facts = $0.facts; $0.facts.removeAll(); return facts } }

    func reserveHistoricalLabels(_ records: [AgentRecord]) throws {
        try state.withLock { s in
            guard s.generation == nil, s.engineLabels.isEmpty else { throw AgentFailure.recovery }
            for record in records {
                let facts = record.facts + record.checks.flatMap(\.facts)
                s.reservedLabels.formUnion(facts.compactMap(\.engine))
                for label in facts.compactMap(\.operation).flatMap({ $0.split(separator: "-") }) {
                    func successor(after prefix: String, current: Int) throws -> Int {
                        guard label.hasPrefix(prefix), let n = Int(label.dropFirst(prefix.count)), n > 0 else { return current }
                        guard n < Int.max - 4096 else { throw AgentFailure.recovery }
                        return max(current, n + 1)
                    }
                    s.nextBatch = try successor(after: "batch", current: s.nextBatch)
                    s.nextSend = try successor(after: "send", current: s.nextSend)
                    s.nextEntity = try successor(after: "entity", current: s.nextEntity)
                }
            }
        }
    }

    func admitSmokeSDK(origin: String) throws {
        try state.withLock { s in
            guard !s.failed, !s.joined, s.generation != nil else { throw AgentFailure.execution }
            _ = try s.admitEngine(origin)
        }
    }

    func engineLabel(for origin: String) throws -> String {
        try state.withLock { s in
            guard !s.failed, let label = s.engineLabels[origin] else { throw AgentFailure.execution }
            return label
        }
    }

    func beginAdoption() throws {
        try state.withLock { s in
            guard !s.failed, let generation = s.generation, s.adoptingFrom == nil,
                  s.engineLabels.isEmpty, s.callbacks.isEmpty, s.batches.isEmpty,
                  s.own.values.allSatisfy({ $0.phase == 4 }) else { throw AgentFailure.execution }
            s.adoptingFrom = generation
        }
    }
    // Called at the actual factory boundary after current-Host control/scan and
    // independently retained original-role validation. No reset or caller-selected generation.
    func admitSDK(origin: String, control: SelectedCloudControlObservation, replacement: CoreEvent?) throws {
        try state.withLock { s in
            guard !s.failed, !s.joined, control.requiredScanComplete,
                  s.sdkAdmissions[origin] == nil else { throw AgentFailure.execution }
            if let old = s.adoptingFrom {
                guard s.generation == old, control.generation > old,
                      control.status == .proven, control.provenance == .retainedAdoption,
                      control.bindingMatchesEpoch == .proven, control.activation == .proven, control.ownedBackup == .proven,
                      let replacement, replacement.generation == control.generation,
                      case .collectionReplaced = replacement.kind,
                      s.engineLabels.isEmpty, s.callbacks.isEmpty, s.batches.isEmpty,
                      s.own.values.allSatisfy({ $0.phase == 4 }) else { throw AgentFailure.execution }
                s.retiredGenerations.insert(old)
                s.generation = control.generation
                s.adoptingFrom = nil
                s.append(.applied, "normal-adoption-generation-joined")
            }
            guard s.generation == control.generation, !s.retiredGenerations.contains(control.generation) else { throw AgentFailure.execution }
            let engine = try s.admitEngine(origin)
            s.sdkAdmissions[origin] = control.generation
            s.append(.applied, "required-scan-before-native-sdk", engine: engine)
        }
    }
    // A new opening starts an empty ledger. Only its actual SDK admission is
    // added here; original journal callbacks/commits/ticks are never imported.
    func admitRestoredSDK(origin: String, control: RestoredCloudControlObservation) throws {
        try state.withLock { s in
            guard !s.failed, !s.joined, s.generation == control.generation, control.requiredScanComplete,
                  s.sdkAdmissions[origin] == nil, s.adoptingFrom == nil else { throw AgentFailure.execution }
            let engine = try s.admitEngine(origin)
            s.sdkAdmissions[origin] = control.generation
            s.append(.applied, "required-scan-before-native-sdk", engine: engine)
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
                _ = try s.bind(origin, generation: current.generation)
            } catch { s.failed = true }
        }
    }
    func sdk(_ observation: CloudSDKObservation, current: SmokeCloudObservation? = nil,
             historical: [SmokeHistoricalAck] = []) {
        state.withLock { s in
            do {
                guard !s.failed, !s.joined, let generation = s.generation else { throw AgentFailure.execution }
                if let current, current.engine != observation.origin { throw AgentFailure.execution }
                let origin = observation.origin
                let engine = try s.bind(origin, generation: current?.generation ?? generation)
                let key = EngineKey(origin: origin, id: observation.ordinal)
                let kind: String
                let batch: EngineKey?
                let saved: [CloudRecord]
                var conflicts: [String: Data] = [:]
                var results: [String: RecordResult] = [:]
                var received: [String: Data] = [:]
                var stateDigest: Data?
                var zone: String?
                var group: String?
                switch observation.event {
                case .stateUpdate(let data): kind = "state"; batch = nil; saved = []; stateDigest = Self.hash(data)
                case .sent(let id, let records, let failures, let deleted, let failedDeletes):
                    let handed = EngineKey(origin: origin, id: id)
                    kind = "sent"; batch = handed; saved = records
                    guard s.batches[handed]?.send != nil ||
                          (failures.isEmpty && deleted.isEmpty && failedDeletes.isEmpty) else { throw AgentFailure.execution }
                    for name in records.map(\.name) {
                        guard results.updateValue(.saved, forKey: name) == nil else { throw AgentFailure.execution }
                    }
                    for name in deleted {
                        guard results.updateValue(.deleted, forKey: name) == nil else { throw AgentFailure.execution }
                    }
                    for failure in failures {
                        guard let code = failure.nativeCode, code > 0,
                              results.updateValue(.saveError(code), forKey: failure.name) == nil else { throw AgentFailure.execution }
                        if failure.code == .serverRecordChanged, let server = failure.serverRecord, server.name == failure.name {
                            conflicts[failure.name] = Self.hash(server.envelope)
                        }
                    }
                    for failure in failedDeletes {
                        guard let code = failure.nativeCode, code > 0,
                              results.updateValue(.deleteError(code), forKey: failure.name) == nil else { throw AgentFailure.execution }
                    }
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
                guard historical.isEmpty || kind == "sent" && observation.phase == .committed,
                      Set(historical.map(\.name)).count == historical.count,
                      historical.allSatisfy({ $0.sdk == origin && conflicts[$0.name] == $0.envelopeDigest })
                else { throw AgentFailure.execution }
                let savedRecords = Dictionary(uniqueKeysWithValues: saved.map { ($0.name, Self.hash($0.envelope)) })
                if case .delivered = observation.phase {
                    guard s.callbacks.count < 1024, observation.ordinal > s.lastOrdinals[origin, default: 0],
                          s.callbacks.values.allSatisfy({ $0.engine != engine || $0.phase == 3 }) else { throw AgentFailure.execution }
                    s.lastOrdinals[origin] = observation.ordinal
                    var cb = Callback(alias: "sdk\(s.callbacks.count + 1)", engine: engine, kind: kind, batch: batch)
                    cb.savedRecords = savedRecords; cb.results = results; cb.conflicts = conflicts; cb.stateDigest = stateDigest
                    cb.received = received; cb.zone = zone; cb.group = group
                    s.callbacks[key] = cb
                }
                guard var cb = s.callbacks[key], cb.kind == kind, cb.batch == batch,
                      cb.savedRecords == savedRecords, cb.results == results, cb.conflicts == conflicts, cb.stateDigest == stateDigest,
                      cb.received == received, cb.zone == zone, cb.group == group else { throw AgentFailure.execution }
                let expected: Int
                let suffix: String
                switch observation.phase {
                case .delivered: expected = 0; suffix = "delivered"
                case .committed:
                    expected = 1; suffix = "committed"
                    if let batch {
                        guard let handed = s.batches[batch],
                              results.allSatisfy({ name, result in
                                  result.isDelete ? handed.deletes.contains(name) : handed.records[name] != nil
                              }),
                              saved.allSatisfy({ handed.records[$0.name] == Self.hash($0.envelope) }) else { throw AgentFailure.execution }
                        cb.saved = Set(saved.map(\.name))
                    }
                case .returned: expected = 2; suffix = "returned"
                }
                guard cb.phase == expected else { throw AgentFailure.execution }
                cb.phase += 1
                let batchLabel = batch.flatMap { s.batches[$0]?.alias }.map { "-" + $0 } ?? ""
                s.append(kind == "state" && expected == 1 ? .stateSaved : .engineCallback,
                         cb.alias + "-" + kind + batchLabel + "-" + suffix, engine: engine)
                let callbackTick = s.tick
                if kind == "fetched" && expected == 1 {
                    for name in received.keys.sorted() {
                        let entity = s.entity(name)
                        s.facts.append(.init(name: .engineCallback, engine: engine,
                                             operation: "\(cb.alias)-fetched-\(entity)"))
                    }
                }
                if kind == "state" && expected == 1 {
                    if let pending = observation.pendingNames {
                        try s.reflectState(pending, engine: engine, callback: cb, callbackKey: key)
                    } else if s.needsPendingEvidence { throw AgentFailure.execution }
                }
                if let batch, var handed = s.batches[batch], let send = handed.send {
                    if expected == 1 {
                        for name in results.keys.sorted() {
                            let candidates = s.batches.filter { $0.key.origin == origin && $0.value.open.contains(name) }
                            guard candidates.count == 1, candidates[batch] != nil,
                                  let entity = s.entities[name], let result = results[name] else { throw AgentFailure.execution }
                            let label = "\(send)-\(handed.alias)-\(entity)"
                            s.append(.engineCallback, "\(label)-\(result.label)", engine: engine)
                            if result == .saved, let image = handed.proof?.saveImages.first(where: { $0.name == name }) {
                                s.facts.append(.init(name: .engineCallback, count: image.imageSeq, engine: engine,
                                                     operation: label + (image.held ? "-image-held" : "-image-live")))
                            }
                            if let image = historical.first(where: { $0.name == name }) {
                                s.facts.append(.init(name: .engineCallback, count: image.imageSeq, engine: engine,
                                                     operation: label + (image.held ? "-conflict-image-held" : "-conflict-image-live")))
                            }
                            handed.open.remove(name)
                            handed.reported.insert(name)
                        }
                    } else if expected == 2 {
                        s.append(.engineCallback, "\(send)-\(handed.alias)-batch-returned", engine: engine)
                        handed.open.removeAll()
                        handed.returned = true
                    }
                    s.batches[batch] = handed
                }
                if expected == 2 {
                    cb.returned = callbackTick
                    if kind == "state" { s.stateReturns[engine] = callbackTick }
                }
                s.callbacks[key] = cb
            } catch { s.failed = true }
        }
    }
    func fetchedAcknowledgementFacts(_ acknowledgements: [SmokeHistoricalAck]) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed, !s.joined else { throw AgentFailure.execution }
            var operations = Set<String>()
            return try acknowledgements.map { image in
                let received = s.callbacks.values.filter {
                    $0.kind == "fetched" && $0.phase == 3 && $0.engine == s.engineLabels[image.sdk] &&
                        $0.received[image.name] == image.envelopeDigest
                }
                guard received.count == 1, let callback = received.first, let entity = s.entities[image.name]
                else { throw AgentFailure.execution }
                let label = "\(callback.alias)-own-landed-\(entity)"
                guard operations.insert(label).inserted else { throw AgentFailure.execution }
                return .init(name: .applied, count: image.imageSeq, engine: callback.engine,
                             operation: label + (image.held ? "-held" : "-live"))
            }
        }
    }
    func batch(origin: String, batch: CloudEngineBatch, proof: SmokeBatchProof?, current: SmokeCloudObservation,
               operation: CloudNativeSendOperation? = nil) {
        state.withLock { s in
            do {
                let key = EngineKey(origin: origin, id: batch.id)
                guard !s.failed, !s.joined, s.batches.count < 256, s.batches[key] == nil,
                      current.engine == origin, operation != nil || batch.deletes.isEmpty,
                      Set(batch.saves.map(\.name) + batch.deletes).count == batch.saves.count + batch.deletes.count
                else { throw AgentFailure.execution }
                let engine = try s.bind(origin, generation: current.generation)
                if let proof {
                    guard proof.generation == current.generation, proof.saves == UInt64(batch.saves.count),
                          Set(proof.images.map(\.name)).count == proof.images.count,
                          proof.images.allSatisfy({ image in
                              image.digest?.count == 32 && image.imageSeq <= proof.seq &&
                              batch.saves.contains { $0.name == image.name && Self.hash($0.envelope) == image.envelopeDigest }
                          }) else { throw AgentFailure.execution }
                    if s.needsPendingEvidence || !proof.saveImages.isEmpty {
                        guard proof.saveImages.count == batch.saves.count,
                              Set(proof.saveImages.map(\.name)).count == proof.saveImages.count,
                              proof.saveImages.allSatisfy({ image in
                                  image.digest?.count == 32 && image.imageSeq <= proof.seq &&
                                  batch.saves.contains { $0.name == image.name && Self.hash($0.envelope) == image.envelopeDigest }
                              }) else { throw AgentFailure.execution }
                    }
                }
                let alias = "batch\(s.nextBatch)"
                let send = try operation.map { try s.send($0, engine: engine).alias }
                s.batches[key] = Batch(alias: alias, engine: engine, ordinal: s.nextBatch,
                    records: Dictionary(uniqueKeysWithValues: batch.saves.map { ($0.name, Self.hash($0.envelope)) }),
                    deletes: Set(batch.deletes),
                    proof: proof, session: proof?.session ?? current.activeSession, seq: proof?.seq ?? current.seq,
                    send: send, open: send == nil ? [] : Set(batch.saves.map(\.name) + batch.deletes))
                s.nextBatch += 1
                s.append(.applied, alias + (proof == nil ? "-media-handoff" : "-collection-handoff"), engine: engine)
                s.facts.append(.init(name: .applied, count: UInt64(batch.saves.count), engine: engine, operation: alias + "-save-count"))
                if let send, let operation {
                    if !batch.deletes.isEmpty {
                        s.facts.append(.init(name: .applied, count: UInt64(batch.deletes.count), engine: engine,
                                             operation: alias + "-delete-count"))
                    }
                    let phase = operation.reason == .manual ? "manual-build" : "scheduled-build"
                    s.append(.applied, "\(send)-\(alias)-\(phase)", engine: engine)
                    for record in batch.saves {
                        let entity = s.entity(record.name)
                        s.append(.applied, "\(send)-\(alias)-\(entity)-member", engine: engine)
                    }
                    for name in batch.deletes {
                        let entity = s.entity(name)
                        s.append(.applied, "\(send)-\(alias)-\(entity)-delete-member", engine: engine)
                    }
                }
            } catch { s.failed = true }
        }
    }
    func sendReturned(origin: String, operation: CloudNativeSendOperation) {
        state.withLock { s in
            do {
                guard !s.failed, !s.joined, operation.reason == .manual,
                      let engine = s.engineLabels[origin] else { throw AgentFailure.execution }
                var send = try s.send(operation, engine: engine)
                send.returned = true
                s.sends[operation] = send
                for key in s.batches.keys where key.origin == origin && s.batches[key]?.send == send.alias {
                    s.batches[key]?.open.removeAll()
                }
                s.append(.applied, "\(send.alias)-manual-returned", engine: engine)
            } catch { s.failed = true }
        }
    }
    func pendingUpdated(origin: String, adds: Set<String>, removes: [String: Bool]) {
        state.withLock { s in
            guard !s.failed, !s.joined, let engine = s.engineLabels[origin] else { s.failed = true; return }
            s.append(.applied, "pending-update", engine: engine)
            for name in removes.keys.sorted() {
                let held = removes[name] == true
                let alias = s.entity(name)
                s.append(.applied, "\(alias)-removed-\(held ? "held" : "absent")", engine: engine)
                if held { s.heldRemovals[engine, default: []].insert(name) }
                else { s.heldRemovals[engine]?.remove(name) }
            }
            for name in adds.sorted() {
                let alias = s.entity(name)
                s.append(.applied, "\(alias)-added", engine: engine)
                s.heldRemovals[engine]?.remove(name)
            }
            let removedWhileHeld = Set(removes.filter(\.value).keys)
            for key in s.batches.keys where key.origin == origin {
                guard var batch = s.batches[key] else { continue }
                batch.removedWhileHeld.subtract(removes.keys)
                batch.removedWhileHeld.formUnion(removedWhileHeld.intersection(batch.names))
                batch.removedWhileHeld.subtract(adds)
                if batch.returned {
                    batch.readded.subtract(removes.keys)
                    batch.readded.formUnion(adds.intersection(batch.names))
                }
                s.batches[key] = batch
            }
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
            return [.init(name: .applied, count: c.imageSeq, digest: CloudKitSmokeGeneratedInput.hex(baseline.digest), operation: "c1-held-projection"),
                    .init(name: .applied, count: 0, operation: "r1-held-absent")]
        }
    }
    func requireAcknowledgements(_ entities: [SmokeCloudEntity], observations: [CloudWorkflowObservation]) throws -> [AgentFact] {
        try state.withLock { s in
            guard !s.failed else { throw AgentFailure.execution }
            return try entities.map { entity in
                let candidates = s.callbacks.values.filter { cb in
                    guard cb.kind == "sent", cb.phase == 3, cb.saved.contains(entity.recordName),
                          let batch = cb.batch, let handed = s.batches[batch], let returned = cb.returned,
                          handed.records[entity.recordName] == entity.ackedEnvelopeDigest,
                          !s.batches.contains(where: { $0.key != batch && $0.key.id == batch.id && $0.value.records[entity.recordName] != nil })
                    else { return false }
                    if handed.send != nil {
                        guard let stateSave = handed.firstStateSave, stateSave.reported.contains(entity.recordName),
                              s.callbacks[stateSave.callback]?.phase == 3 else { return false }
                    } else if s.stateReturns[cb.engine, default: 0] <= returned { return false }
                    return observations.contains {
                        if case .savedPendingRemoved(let name, let request) = $0 { return name == entity.recordName && request == batch.id }; return false
                    }
                }
                guard let cb = candidates.sorted(by: { ($0.returned ?? 0) < ($1.returned ?? 0) }).last,
                      let batch = cb.batch, let alias = s.batches[batch]?.alias else { throw AgentFailure.execution }
                let member = s.batches[batch]?.send == nil ? "" : s.entities[entity.recordName].map { "-" + $0 } ?? ""
                return .init(name: .applied, count: cb.returned, engine: cb.engine,
                             operation: entity.label + "-" + cb.alias + "-" + alias + member + "-ack")
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
                let callbacks = callbacks.filter { $0.engine == page.engine }
                guard let pageAt = page.returned, let zone = page.zone,
                      let start = callbacks.last(where: { $0.kind == "fetch-start" && ($0.returned ?? .max) < pageAt }),
                      let startAt = start.returned,
                      let zoneStart = callbacks.last(where: { $0.kind == "fetch-zone-start" && $0.zone == zone && ($0.returned ?? .max) < pageAt }),
                      let zoneStartAt = zoneStart.returned, zoneStartAt > startAt,
                      let zoneEnd = callbacks.first(where: { $0.kind == "fetch-zone-end" && $0.zone == zone && ($0.returned ?? 0) > pageAt }),
                      let zoneEndAt = zoneEnd.returned,
                      callbacks.contains(where: { $0.kind == "fetch-end" && $0.group == start.group && ($0.returned ?? 0) > zoneEndAt }) else { continue }
                return [.init(name: .engineCallback, count: pageAt, engine: page.engine, operation: "n1-original-fetched-returned")]
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
        guard !s.failed, !s.joined, s.generation != nil, s.adoptingFrom == nil,
              s.own.values.allSatisfy({ $0.phase == 4 }),
              s.batches.values.allSatisfy({ $0.send == nil || !$0.reported.isEmpty || !$0.unreportedStateUnsupported }),
              !s.callbacks.isEmpty, s.callbacks.values.allSatisfy({ cb in
                  guard cb.phase == 3 else { return false }
                  if let key = cb.batch, let batch = s.batches[key], batch.send != nil {
                      if cb.results.values.allSatisfy(\.keepsPending) { return true }
                      guard let stateSave = batch.firstStateSave else { return false }
                      return Set(cb.results.keys).isSubset(of: stateSave.reported) && s.callbacks[stateSave.callback]?.phase == 3
                  }
                  guard !cb.saved.isEmpty else { return true }
                  return s.stateReturns[cb.engine, default: 0] > (cb.returned ?? UInt64.max)
              }) else { throw AgentFailure.execution }
        let engines = Set(s.callbacks.values.map(\.engine))
        return [.init(name: .ownedWorkJoined, count: UInt64(s.own.count), operation: "own-joined"),
                .init(name: .ownedWorkJoined, count: UInt64(s.callbacks.count), engine: engines.count == 1 ? engines.first : nil, operation: "sdk-joined")]
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
                .init(name: .applied, count: $0.seq, engine: $0.engine, operation: $0.alias + "-s1-media-snapshot")
            }
            for batch in s.batches.values.sorted(by: { $0.ordinal < $1.ordinal }) {
                guard let proof = batch.proof, proof.session == held.session, proof.seq >= held.seq else { continue }
                facts.append(.init(name: .applied, count: proof.seq, engine: batch.engine, operation: batch.alias + "-s1-snapshot"))
                if let card = proof.images.first(where: { $0.name == held.card }), let digest = card.digest {
                    facts.append(.init(name: .applied, count: card.imageSeq, digest: CloudKitSmokeGeneratedInput.hex(digest), engine: batch.engine, operation: batch.alias + "-c1-held"))
                }
                facts.append(.init(name: .applied, count: 0, engine: batch.engine, operation: batch.alias + "-r1-absent"))
            }
            s.joined = true
            return facts + [.init(name: .applied, count: UInt64(batches.count + mediaBatches.count), operation: "held-batches"),
                    .init(name: .applied, count: UInt64(mediaBatches.count), operation: "held-media-batches"),
                    .init(name: .applied, count: UInt64(batches.count), operation: "held-collection-batches"),
                    .init(name: .applied, count: UInt64(batches.flatMap(\.images).filter { $0.name == held.card }.count), operation: "c1-held-handed"),
                    .init(name: .applied, count: 0, operation: "r1-held-handed")] + originalWork
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
