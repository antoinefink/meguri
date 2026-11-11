import Foundation

// Replays original, content-free SDK observations. It never starts an engine,
// imports a new process's callbacks or supplies a current tracking proof.
nonisolated struct AgentOriginalExchangeSDK {
    struct Image {
        let entity: String
        let engine: String
        let callback: String
        let batch: String?
        let record: Int
        let seq: UInt64
        let held: Bool
        let envelopeDigest: String
    }
    let acknowledgements: [String: [Image]]

    struct EarlierAutomaticAcknowledgement {
        let image: Image
        let operation: String
        let callbackReturned: UInt64
    }
    private let earlierAutomatic: [EarlierAutomaticAcknowledgement]

    func earlierAutomaticAcknowledgement(envelopeDigest: String) throws -> EarlierAutomaticAcknowledgement {
        guard AgentJSON.digest(envelopeDigest) else { throw AgentFailure.recovery }
        let matches = earlierAutomatic.filter { $0.image.envelopeDigest == envelopeDigest }
        guard Set(matches.map { $0.image.entity }).count == 1,
              let latest = matches.max(by: { $0.image.record < $1.image.record }) else { throw AgentFailure.recovery }
        return latest
    }

    init(records: [AgentRecord], issuance: AgentRecord, capture: AgentRecord, terminal: AgentRecord,
         boundary: UInt64, selected: Set<String>, accounting: AgentOriginalExchangeAccounting) throws {
        guard issuance.kind == .observation, issuance.agent_instance == capture.agent_instance,
              issuance.header_sha256 == capture.header_sha256, issuance.command_seq == capture.command_seq,
              issuance.command_sha256 == capture.command_sha256, issuance.record_seq < capture.record_seq,
              let issued = issuance.facts.first, let witness = issued.digest, AgentJSON.digest(witness),
              let exchange = capture.facts.first?.count, exchange > 0,
              issuance.facts == [.init(name: .syncResult, count: exchange, digest: witness,
                                      operation: "original-exchange-issued")] else { throw AgentFailure.recovery }
        var ledger = Ledger()
        for record in records where record.kind == .observation && record.agent_instance == capture.agent_instance &&
            record.record_seq < terminal.record_seq {
            try ledger.ingest(record)
        }
        try ledger.checkOrder(after: capture.record_seq)
        for (key, batch) in ledger.batches where ledger.handoffs[key]?.collection == true {
            for (entity, result) in batch.results where result.kind == "saved" && result.record > capture.record_seq {
                guard ledger.images.contains(where: { $0.batch == key && $0.entity == entity && $0.record == result.record })
                else { throw AgentFailure.recovery }
            }
        }
        let images = ledger.images.filter {
            selected.contains($0.entity) && $0.seq >= boundary && $0.record > capture.record_seq
        }
        let acknowledged = Dictionary(grouping: images, by: \.entity)
        let held = Set(acknowledged.compactMap { $0.value.allSatisfy(\.held) ? $0.key : nil })
        guard Set(acknowledged.keys) == accounting.outcomes[.acked, default: []],
              held == accounting.heldAcknowledged else { throw AgentFailure.recovery }
        for entity in accounting.outcomes[.absent, default: []] {
            guard ledger.batches.values.contains(where: { batch in
                batch.results[entity].map { $0.kind == "save-error-11" && $0.record > capture.record_seq } == true
            }) else { throw AgentFailure.recovery }
        }
        self.acknowledgements = acknowledged
        // This proves only a completed earlier automatic operation. A manual
        // build or a later result is not attributed to this exchange by time.
        self.earlierAutomatic = ledger.images.compactMap { image in
            guard !image.held, image.record < issuance.record_seq,
                  let key = image.batch, ledger.handoffs[key]?.collection == true,
                  let batch = ledger.batches[key], batch.engine == image.engine,
                  let send = ledger.sends[batch.send], send.engine == image.engine, !send.manual,
                  let result = batch.results[image.entity], result.record == image.record,
                  let callback = ledger.callbacks[image.callback], callback.engine == image.engine,
                  callback.phases.count == 3, let returned = callback.phases.last,
                  returned.record < issuance.record_seq,
                  let reflected = batch.reflected, result.record < reflected.record, result.tick < reflected.tick,
                  let state = ledger.callbacks.values.first(where: {
                      $0.engine == image.engine && $0.event == "state" && $0.phases.count == 3 &&
                          $0.phases[1].record == reflected.record
                  }), let stateReturned = state.phases.last, stateReturned.record < issuance.record_seq
            else { return nil }
            return .init(image: image, operation: batch.send, callbackReturned: returned.tick)
        }
    }

    private struct Position {
        let record: Int
        let tick: UInt64
    }
    private struct Callback {
        let engine: String
        let event: String
        let batch: String?
        var phases: [Position] = []
    }
    private struct Result {
        let kind: String
        let record: Int
        let tick: UInt64
        let error: Int?
    }
    private struct Handoff {
        let engine: String
        let collection: Bool
        var saves: UInt64?
        var deletes: UInt64 = 0
    }
    private struct Batch {
        let engine: String
        let send: String
        let built: Int
        var members: [String: Bool] = [:] // true means a save, false a deletion.
        var open: Set<String> = []
        var results: [String: Result] = [:]
        var callback: String?
        var returned: Int?
        var reflected: Position?
        var unsupportedUnreportedState = false
    }
    private struct Send {
        let engine: String
        let manual: Bool
        var returned = false
    }
    private struct Touch {
        let kind: String
        let record: Int
    }
    private struct Ledger {
        var tick: UInt64 = 0
        var engines: Set<String> = []
        var callbacks: [String: Callback] = [:]
        var handoffs: [String: Handoff] = [:]
        var batches: [String: Batch] = [:]
        var sends: [String: Send] = [:]
        var pending: [String: [String: Touch]] = [:]
        var fetched: [String: Set<String>] = [:]
        var images: [Image] = []
        private let kept: Set<Int> = [3, 4, 6, 7, 9, 23, 36]

        mutating func ingest(_ record: AgentRecord) throws {
            if record.facts.contains(where: { $0.operation?.hasSuffix("-state-committed") == true && $0.engine != nil }) {
                try state(record)
            } else if record.facts.contains(where: { $0.operation == "pending-update" }) {
                try update(record)
            } else {
                for fact in record.facts { try ingest(fact, record: record.record_seq) }
            }
        }

        private static func numbered(_ value: String, _ prefix: String) -> Bool {
            guard value.hasPrefix(prefix), let n = UInt64(value.dropFirst(prefix.count)), n > 0 else { return false }
            return value == prefix + String(n)
        }
        private func shape(_ fact: AgentFact, name: AgentFact.Name, timed: Bool = true) throws -> String {
            guard let engine = fact.engine, Self.numbered(engine, "engine"),
                  let label = fact.operation,
                  fact == .init(name: name, count: fact.count, engine: engine, operation: label),
                  !timed || fact.count.map({ $0 > tick && $0 <= 4096 }) == true
            else { throw AgentFailure.engineOrderNotShown }
            return engine
        }
        private func imageShape(_ fact: AgentFact, name: AgentFact.Name) throws -> (String, String) {
            guard let digest = fact.digest, AgentJSON.digest(digest) else { throw AgentFailure.recovery }
            var metadata = fact
            metadata.digest = nil
            return (try shape(metadata, name: name, timed: false), digest)
        }
        private mutating func advance(_ fact: AgentFact, name: AgentFact.Name) throws -> String {
            let engine = try shape(fact, name: name)
            guard engines.contains(engine), let count = fact.count else { throw AgentFailure.engineOrderNotShown }
            tick = count
            return engine
        }
        private mutating func ingest(_ fact: AgentFact, record: Int) throws {
            guard let label = fact.operation else { return }
            let parts = label.split(separator: "-").map(String.init)
            if label == "required-scan-before-native-sdk" {
                let engine = try shape(fact, name: .applied)
                guard engines.insert(engine).inserted, let count = fact.count else { throw AgentFailure.engineOrderNotShown }
                tick = count
            } else if let first = parts.first, Self.numbered(first, "batch") {
                try handoff(fact, parts: parts)
            } else if let first = parts.first, Self.numbered(first, "send") {
                try send(fact, parts: parts, record: record)
            } else if let first = parts.first, Self.numbered(first, "sdk") {
                try sdk(fact, parts: parts, record: record)
            } else if fact.engine != nil, let first = parts.first,
                      ["sdk", "send", "batch", "entity"].contains(where: { first.hasPrefix($0) }) {
                throw AgentFailure.engineOrderNotShown
            }
        }

        private mutating func handoff(_ fact: AgentFact, parts: [String]) throws {
            let suffix = parts.dropFirst().joined(separator: "-"), name = parts[0]
            if suffix == "collection-handoff" || suffix == "media-handoff" {
                let engine = try advance(fact, name: .applied)
                guard handoffs[name] == nil else { throw AgentFailure.engineOrderNotShown }
                handoffs[name] = .init(engine: engine, collection: suffix == "collection-handoff")
            } else if suffix == "save-count" || suffix == "delete-count" {
                let engine = try shape(fact, name: .applied, timed: false)
                guard var value = handoffs[name], value.engine == engine, let count = fact.count,
                      batches[name] == nil else { throw AgentFailure.engineOrderNotShown }
                if suffix == "save-count" {
                    guard value.saves == nil else { throw AgentFailure.engineOrderNotShown }
                    value.saves = count
                } else {
                    guard value.deletes == 0, count > 0 else { throw AgentFailure.engineOrderNotShown }
                    value.deletes = count
                }
                handoffs[name] = value
            } else { throw AgentFailure.engineOrderNotShown }
        }

        private mutating func send(_ fact: AgentFact, parts: [String], record: Int) throws {
            let operation = parts[0]
            if parts.count == 3, parts[1...] == ["manual", "returned"] {
                let engine = try advance(fact, name: .applied)
                guard var send = sends[operation], send.engine == engine, send.manual, !send.returned
                else { throw AgentFailure.engineOrderNotShown }
                send.returned = true; sends[operation] = send
                for key in Array(batches.keys) where batches[key]?.send == operation { batches[key]?.open.removeAll() }
                return
            }
            guard parts.count >= 4, Self.numbered(parts[1], "batch") else { throw AgentFailure.engineOrderNotShown }
            let key = parts[1], suffix = parts.dropFirst(2).joined(separator: "-")
            if suffix == "manual-build" || suffix == "scheduled-build" {
                let engine = try advance(fact, name: .applied), manual = suffix == "manual-build"
                if sends[operation] == nil { sends[operation] = .init(engine: engine, manual: manual) }
                guard let origin = sends[operation], origin.engine == engine, origin.manual == manual, !origin.returned,
                      let handed = handoffs[key], handed.engine == engine, handed.saves != nil, batches[key] == nil
                else { throw AgentFailure.engineOrderNotShown }
                batches[key] = .init(engine: engine, send: operation, built: record)
                return
            }
            guard var batch = batches[key], batch.send == operation else { throw AgentFailure.engineOrderNotShown }
            if suffix == "batch-returned" {
                let engine = try advance(fact, name: .engineCallback)
                guard engine == batch.engine, let callback = batch.callback, let observed = callbacks[callback],
                      observed.phases.count == 3, observed.phases.last?.record == record, batch.returned == nil
                else { throw AgentFailure.engineOrderNotShown }
                batch.returned = record; batch.open.removeAll(); batches[key] = batch
                return
            }
            guard parts.count >= 4, Self.numbered(parts[2], "entity") else { throw AgentFailure.engineOrderNotShown }
            let entity = parts[2], kind = parts.dropFirst(3).joined(separator: "-")
            if kind == "member" || kind == "delete-member" {
                let engine = try advance(fact, name: .applied), saving = kind == "member"
                guard batch.engine == engine, batch.callback == nil, batch.members[entity] == nil,
                      let handed = handoffs[key], let saves = handed.saves,
                      UInt64(batch.members.values.filter({ $0 == saving }).count) < (saving ? saves : handed.deletes)
                else { throw AgentFailure.engineOrderNotShown }
                batch.members[entity] = saving; batch.open.insert(entity); batches[key] = batch
            } else if kind.hasPrefix("image-") || kind.hasPrefix("conflict-image-") {
                let (engine, digest) = try imageShape(fact, name: .engineCallback)
                let conflict = kind.hasPrefix("conflict-"), flag = parts.last
                guard flag == "held" || flag == "live", kind == (conflict ? "conflict-image-" : "image-") + (flag ?? ""),
                      let seq = fact.count, engine == batch.engine,
                      let result = batch.results[entity], result.record == record,
                      result.kind == (conflict ? "save-error-14" : "saved"), let callback = batch.callback,
                      !images.contains(where: { $0.batch == key && $0.entity == entity })
                else { throw AgentFailure.recovery }
                images.append(.init(entity: entity, engine: engine, callback: callback, batch: key, record: record,
                                    seq: seq, held: flag == "held", envelopeDigest: digest))
            } else {
                let engine = try advance(fact, name: .engineCallback)
                let saving = kind == "saved" || kind.hasPrefix("save-error-")
                let deleting = kind == "deleted" || kind.hasPrefix("delete-error-")
                let code = kind.contains("-error-") ? Int(parts.last ?? "") : nil
                guard saving || deleting,
                      !kind.contains("-error-") || code.map({ $0 > 0 && kind == (saving ? "save-error-" : "delete-error-") + String($0) }) == true,
                      batch.engine == engine, batch.members[entity] == saving,
                      let callback = batch.callback, let observed = callbacks[callback],
                      observed.phases.count == 2, observed.phases.last?.record == record,
                      batches.filter({ $0.value.engine == engine && $0.value.open.contains(entity) }).map(\.key) == [key],
                      let count = fact.count else { throw AgentFailure.engineOrderNotShown }
                batch.open.remove(entity)
                batch.results[entity] = .init(kind: kind, record: record, tick: count, error: code)
                batches[key] = batch
            }
        }

        private mutating func sdk(_ fact: AgentFact, parts: [String], record: Int) throws {
            let key = parts[0]
            if parts.count == 3, parts[1] == "fetched", Self.numbered(parts[2], "entity") {
                let engine = try shape(fact, name: .engineCallback, timed: false)
                guard fact.count == nil, let callback = callbacks[key], callback.engine == engine,
                      callback.event == "fetched", callback.phases.count == 2, callback.phases.last?.record == record,
                      fetched[key, default: []].insert(parts[2]).inserted else { throw AgentFailure.recovery }
                return
            }
            if parts.count == 5, parts[1...2] == ["own", "landed"] {
                let (engine, digest) = try imageShape(fact, name: .applied)
                guard Self.numbered(parts[3], "entity"), let seq = fact.count, parts[4] == "held" || parts[4] == "live",
                      let callback = callbacks[key], callback.engine == engine, callback.event == "fetched",
                      callback.phases.count == 3, let returned = callback.phases.last, returned.record < record,
                      fetched[key, default: []].contains(parts[3]),
                      !images.contains(where: { $0.callback == key && $0.entity == parts[3] })
                else { throw AgentFailure.recovery }
                images.append(.init(entity: parts[3], engine: engine, callback: key, batch: nil,
                                    record: record, seq: seq, held: parts[4] == "held", envelopeDigest: digest))
                return
            }
            guard let phase = parts.last, ["delivered", "committed", "returned"].contains(phase)
            else { throw AgentFailure.engineOrderNotShown }
            let event: String, batch: String?
            if parts.count == 4, parts[1] == "sent", Self.numbered(parts[2], "batch") {
                event = "sent"; batch = parts[2]
            } else {
                event = parts.dropFirst().dropLast().joined(separator: "-"); batch = nil
                guard ["state", "fetch-start", "fetch-zone-start", "fetched", "fetch-zone-end", "fetch-end"].contains(event)
                else { throw AgentFailure.engineOrderNotShown }
            }
            let engine = try advance(fact, name: event == "state" && phase == "committed" ? .stateSaved : .engineCallback)
            if phase == "delivered" {
                guard key == "sdk\(callbacks.count + 1)", callbacks[key] == nil,
                      callbacks.values.allSatisfy({ $0.engine != engine || $0.phases.count == 3 })
                else { throw AgentFailure.engineOrderNotShown }
                callbacks[key] = .init(engine: engine, event: event, batch: batch)
                if let batch {
                    guard var value = batches[batch], let handed = handoffs[batch], value.engine == engine,
                          value.callback == nil, UInt64(value.members.values.filter({ $0 }).count) == handed.saves,
                          UInt64(value.members.values.filter({ !$0 }).count) == handed.deletes
                    else { throw AgentFailure.engineOrderNotShown }
                    value.callback = key; batches[batch] = value
                }
            }
            guard var callback = callbacks[key], callback.engine == engine, callback.event == event, callback.batch == batch,
                  callback.phases.count < 3, ["delivered", "committed", "returned"][callback.phases.count] == phase,
                  let count = fact.count else { throw AgentFailure.engineOrderNotShown }
            callback.phases.append(.init(record: record, tick: count)); callbacks[key] = callback
        }

        private mutating func update(_ record: AgentRecord) throws {
            guard let marker = record.facts.first, marker.operation == "pending-update" else { throw AgentFailure.engineOrderNotShown }
            let engine = try advance(marker, name: .applied)
            var touches: [String: String] = [:]
            for fact in record.facts.dropFirst() {
                let parts = (fact.operation ?? "").split(separator: "-").map(String.init)
                guard parts.count >= 2, Self.numbered(parts[0], "entity"),
                      try advance(fact, name: .applied) == engine else { throw AgentFailure.engineOrderNotShown }
                let action = parts.dropFirst().joined(separator: "-")
                guard ["added", "removed-held", "removed-absent"].contains(action),
                      touches[parts[0]] == nil || touches[parts[0]]?.hasPrefix("removed-") == true && action == "added"
                else { throw AgentFailure.engineOrderNotShown }
                touches[parts[0]] = action
            }
            for (entity, action) in touches { pending[engine, default: [:]][entity] = .init(kind: action, record: record.record_seq) }
        }

        private mutating func state(_ record: AgentRecord) throws {
            guard let committed = record.facts.first, let label = committed.operation,
                  let callback = label.split(separator: "-").first.map(String.init), Self.numbered(callback, "sdk"),
                  label == callback + "-state-committed"
            else { throw AgentFailure.engineOrderNotShown }
            try ingest(committed, record: record.record_seq)
            guard let engine = committed.engine, let observed = callbacks[callback], observed.event == "state",
                  observed.engine == engine, observed.phases.count == 2, observed.phases.last?.record == record.record_seq,
                  record.facts.count >= 2 else { throw AgentFailure.engineOrderNotShown }
            let marker = record.facts[1]
            guard marker.operation == callback + "-state-batches", try advance(marker, name: .stateSaved) == engine
            else { throw AgentFailure.engineOrderNotShown }
            let known = batches.filter { $0.value.engine == engine }
            let members = Set(known.values.flatMap { $0.members.keys })
            var reads: [String: String] = [:], claims: [String: (String, String)] = [:]
            for fact in record.facts.dropFirst(2) {
                let parts = (fact.operation ?? "").split(separator: "-").map(String.init)
                guard parts.first == callback, try advance(fact, name: .stateSaved) == engine else { throw AgentFailure.engineOrderNotShown }
                if parts.count == 4, parts[2] == "read" {
                    guard claims.isEmpty, members.contains(parts[1]), ["held", "absent"].contains(parts[3]),
                          reads.updateValue(parts[3], forKey: parts[1]) == nil else { throw AgentFailure.engineOrderNotShown }
                } else {
                    guard parts.count == 6, parts[4] == "reflected", ["held", "absent"].contains(parts[5]),
                          let batch = known[parts[2]], batch.send == parts[1], batch.members[parts[3]] != nil,
                          claims.updateValue((parts[3], parts[5]), forKey: parts[2]) == nil
                    else { throw AgentFailure.engineOrderNotShown }
                }
            }
            guard Set(reads.keys) == members else { throw AgentFailure.engineOrderNotShown }
            var expected: Set<String> = []
            for (key, var batch) in known {
                if batch.results.isEmpty && !batch.members.keys.allSatisfy({ entity in
                    pending[engine]?[entity].map { $0.kind == "removed-held" && $0.record > batch.built } == true
                }) { batch.unsupportedUnreportedState = true }
                let witnesses = batch.members.keys.filter { entity in
                    let touch = pending[engine]?[entity]
                    if reads[entity] == "absent" { return touch?.kind != "removed-held" }
                    guard reads[entity] == "held", let touch, touch.kind == "added", let returned = batch.returned else { return false }
                    return touch.record > returned
                }
                if !witnesses.isEmpty {
                    expected.insert(key)
                    guard let claim = claims[key], witnesses.contains(claim.0), reads[claim.0] == claim.1,
                          let count = committed.count else { throw AgentFailure.engineOrderNotShown }
                    if batch.reflected == nil { batch.reflected = .init(record: record.record_seq, tick: count) }
                }
                batches[key] = batch
            }
            guard Set(claims.keys) == expected else { throw AgentFailure.engineOrderNotShown }
        }

        func checkOrder(after capture: Int) throws {
            for batch in batches.values {
                if batch.results.isEmpty {
                    guard !batch.unsupportedUnreportedState else { throw AgentFailure.engineOrderNotShown }
                    continue
                }
                if batch.results.values.allSatisfy({ $0.error.map { kept.contains($0) } == true }) { continue }
                for result in batch.results.values where result.record > capture {
                    if let saved = batch.reflected {
                        guard result.record < saved.record, result.tick < saved.tick else { throw AgentFailure.engineOrderNotShown }
                    } else {
                        guard !callbacks.values.contains(where: {
                            $0.engine == batch.engine && $0.event == "state" && $0.phases.count >= 2 && $0.phases[1].record > result.record
                        }) else { throw AgentFailure.engineOrderNotShown }
                    }
                }
            }
        }
    }
}
