import Foundation

// Additional bounded classification of an Engine-validated complete journal.
// It selects owned reads; none of these historical facts is live SDK evidence.
nonisolated struct AgentSelectedRestoration {
    struct Edit {
        let generation: UInt64
        let seq: UInt64
        let revision: UInt64
        let digest: String
        let witness: String
    }
    struct Exchange {
        let exchange: UInt64
        let witness: String
        let boundary: UInt64
        let selected: Set<String>
        let terminal: [AgentFact]
    }
    let state: AgentRestoration
    let nextCommand: Int
    let originalGeneration: UInt64
    let activeDigest: String
    let epochDigest: String
    let controlWitness: String
    let edit: Edit?
    let exchange: Exchange?
    let pending: AgentRecord?

    // Exact accounting has no revision-specific source ACK or original event
    // chain. Keep Mac completion unavailable until a separate reader proves it.
    static func requireExchangeRecovery(_ program: AgentProgram) throws {
        guard program == .syncNowPhone else { throw AgentFailure.implementationMissing }
    }

    init(_ state: AgentRestoration) throws {
        let program = try state.header.program
        guard program == .syncNowMac || program == .syncNowPhone,
              !state.records.contains(where: { $0.kind == .failed }) else { throw AgentFailure.implementationMissing }
        let completed = state.records.filter { $0.kind == .done }
        let starts = state.records.filter { $0.kind == .started }
        let count = completed.count
        guard (count == 3 || program == .syncNowMac && count == 4),
              completed.map(\.command_seq) == Array(1...count),
              starts.count == count || starts.count == count + 1,
              starts.map(\.command_seq) == Array(1...starts.count),
              state.commands.count == count + 1 else { throw AgentFailure.implementationMissing }
        for (offset, command) in state.commands.enumerated() {
            try program.requirePosition(command)
            guard command.command_seq == offset + 1 else { throw AgentFailure.journal }
        }
        // Engine already checked hashes/order. Require originals here as well,
        // including the observer's original instance rather than the new one.
        for (offset, record) in state.records.enumerated() {
            guard record.record_seq == offset + 1,
                  let start = starts.first(where: { $0.command_seq == record.command_seq }),
                  record.header_sha256 == start.header_sha256,
                  record.command_sha256 == start.command_sha256,
                  record.record_seq >= start.record_seq else { throw AgentFailure.journal }
        }
        guard let launchInstance = starts.first?.agent_instance,
              starts.prefix(3).allSatisfy({ $0.agent_instance == launchInstance }) else { throw AgentFailure.journal }
        guard zip(starts, state.commands).allSatisfy({ $0.0.action == $0.1.action }) else { throw AgentFailure.journal }
        var completedStarts = Set<Int>()
        for terminal in completed {
            let original = starts[terminal.command_seq - 1]
            guard completedStarts.insert(original.record_seq).inserted,
                  original.record_seq < terminal.record_seq else { throw AgentFailure.journal }
            switch terminal.recovered_from {
            case .some(let recovered) where recovered == original.record_seq: break
            case nil where terminal.agent_instance == original.agent_instance: break
            default: throw AgentFailure.journal
            }
        }
        func observation(_ command: Int, _ name: AgentFact.Name, _ operation: String) throws -> (AgentRecord, AgentFact) {
            let start = starts[command - 1]
            var selected: (AgentRecord, AgentFact)?
            for record in state.records where record.command_seq == command && record.kind == .observation {
                for fact in record.facts where fact.name == name && fact.operation == operation {
                    guard selected == nil else { throw AgentFailure.recovery }
                    guard record.agent_instance == start.agent_instance,
                          record.record_seq > start.record_seq else { throw AgentFailure.recovery }
                    for terminal in completed where terminal.command_seq == command {
                        guard record.record_seq < terminal.record_seq else { throw AgentFailure.recovery }
                    }
                    selected = (record, fact)
                }
            }
            guard let selected else { throw AgentFailure.recovery }
            return selected
        }
        let active = try observation(3, .applied, "canonical-active")
        let epoch = try observation(3, .applied, "canonical-complete-epoch")
        let control = try observation(3, .applied, "original-control-witness")
        guard let activeDigest = active.1.digest, let epochDigest = epoch.1.digest,
              let controlWitness = control.1.digest, let originalGeneration = control.1.count,
              originalGeneration > 0, active.0.record_seq < epoch.0.record_seq,
              epoch.0.record_seq < control.0.record_seq else { throw AgentFailure.recovery }
        for digest in [activeDigest, epochDigest, controlWitness] {
            guard AgentJSON.digest(digest) else { throw AgentFailure.recovery }
        }
        var edit: Edit?
        if program == .syncNowMac && starts.count >= 4 {
            let committed = try observation(4, .localCommitted, "n1")
            let durable = try observation(4, .localDurable, "n1")
            let witness = try observation(4, .localDurable, "original-edit-witness")
            guard let seq = committed.1.count, seq > 0, let revision = durable.1.count, revision > 0,
                  let digest = committed.1.digest, AgentJSON.digest(digest), digest == durable.1.digest,
                  let editGeneration = witness.1.count, editGeneration > 0,
                  let correlation = witness.1.digest, AgentJSON.digest(correlation),
                  committed.0.record_seq < durable.0.record_seq, durable.0.record_seq < witness.0.record_seq else { throw AgentFailure.recovery }
            edit = .init(generation: editGeneration, seq: seq, revision: revision, digest: digest, witness: correlation)
        }
        let pending = starts.count > count ? starts.last : nil
        var exchange: Exchange?
        if pending != nil {
            let command = state.commands[count]
            if command.action == .syncNow && command.recovery_check == "sync_now.exchange" {
                let issued = try observation(command.command_seq, .syncResult, "original-exchange-issued")
                guard let id = issued.1.count, id > 0, let witness = issued.1.digest,
                      AgentJSON.digest(witness) else { throw AgentFailure.recovery }
                let captured = try observation(command.command_seq, .syncResult, "exchange-capture").0
                guard captured.facts.count >= 2, captured.record_seq > issued.0.record_seq,
                      captured.facts[0] == .init(name: .syncResult, count: id, operation: "exchange-capture"),
                      let boundary = captured.facts[1].count,
                      captured.facts[1] == .init(name: .syncResult, count: boundary, operation: "exchange-boundary")
                else { throw AgentFailure.recovery }
                var selected = Set<String>()
                for fact in captured.facts.dropFirst(2) {
                    let prefix = "exchange-selected-entity"
                    guard let label = fact.operation, label.hasPrefix(prefix),
                          let number = UInt64(label.dropFirst(prefix.count)), number > 0, label == prefix + String(number),
                          fact == .init(name: .syncResult, operation: label), selected.insert("entity\(number)").inserted
                    else { throw AgentFailure.recovery }
                }
                let resultRecords = state.records.filter { record in
                    record.command_seq == command.command_seq && record.kind == .observation &&
                        record.facts.contains { $0.operation?.hasPrefix("terminal-") == true }
                }
                guard resultRecords.count == 1, let terminal = resultRecords.first,
                      terminal.agent_instance == issued.0.agent_instance,
                      terminal.record_seq > captured.record_seq,
                      terminal.facts.first == .init(name: .syncResult, count: id, operation: "terminal-exchange-succeeded"),
                      terminal.facts.dropFirst().prefix(5).map(\.operation) ==
                        ["terminal-ended-ms", "terminal-selected", "terminal-held-back",
                         "terminal-newer-pending", "terminal-incoming-waiting"],
                      terminal.facts[2].count == UInt64(selected.count),
                      terminal.facts.allSatisfy({ fact in
                          fact.count != nil && fact.operation?.hasPrefix("terminal-") == true &&
                              fact == .init(name: .syncResult, count: fact.count, operation: fact.operation)
                      }),
                      Set(terminal.facts.map(\.operation)).count == terminal.facts.count
                else { throw AgentFailure.recovery }
                exchange = .init(exchange: id, witness: witness, boundary: boundary, selected: selected, terminal: terminal.facts)
            } else if !(program == .syncNowMac && command.action == .edit && command.recovery_check == "sync_now.edit" && edit != nil) {
                throw AgentFailure.implementationMissing
            }
        }
        self.state = state; self.nextCommand = count + 1; self.originalGeneration = originalGeneration
        self.activeDigest = activeDigest; self.epochDigest = epochDigest; self.controlWitness = controlWitness
        self.edit = edit; self.exchange = exchange; self.pending = pending
    }
}
