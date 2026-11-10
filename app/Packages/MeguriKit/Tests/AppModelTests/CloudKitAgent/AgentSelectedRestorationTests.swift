import Foundation
import Testing

// Prefix evidence only. Real owned progress is proved in Core/Service tests.
@Suite struct AgentSelectedRestorationTests {
    @MainActor private func state(phone: Bool = false, completed: Int = 3, pending: Bool = false,
                                  editGeneration: UInt64 = 7, recordTerminal: Bool = true) throws -> AgentRestoration {
        var object = try #require(JSONSerialization.jsonObject(with: CloudKitAgentProtocolTests.header) as? [String: Any])
        object["selection"] = "sync_now"; object["device_kind"] = phone ? "iphone" : "mac"
        let headerBytes = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        let header = try AgentHeader.read(headerBytes), digest = AgentJSON.hash(headerBytes), program = try header.program
        let commands = try (1...(completed + 1)).map { position in
            let action = program.actions[position - 1]
            var bytes: [String: Any] = ["header_sha256": digest, "command_seq": position, "leg": "sync_now",
                "action": action.rawValue, "arguments": action.arguments, "account_before": "acct1", "account_after": "acct1"]
            if action == .edit { bytes["recovery_check"] = "sync_now.edit" }
            if action == .syncNow { bytes["recovery_check"] = "sync_now.exchange" }
            return try AgentCommand.read(JSONSerialization.data(withJSONObject: bytes), header: digest, sequence: position, program: program)
        }
        var records: [AgentRecord] = []
        func append(_ kind: AgentRecord.Kind, command: AgentCommand, facts: [AgentFact] = []) {
            let checks = kind == .done ? AgentEngine.requiredChecks(command.action).map {
                AgentCheck(name: $0, outcome: .pass, facts: [.init(name: .commandCompleted)])
            } : []
            records.append(.init(header_sha256: digest, record_seq: records.count + 1, command_seq: command.command_seq,
                command_sha256: String(repeating: "1", count: 64), agent_instance: "original1",
                observed_at: "2026-11-10T00:00:00Z", kind: kind, action: kind == .started ? command.action : nil,
                facts: facts, checks: checks))
        }
        for command in commands.prefix(completed + (pending ? 1 : 0)) {
            append(.started, command: command)
            var facts: [AgentFact] = []
            if command.command_seq == 3 {
                facts = [.init(name: .applied, digest: String(repeating: "2", count: 64), operation: "canonical-active"),
                         .init(name: .applied, digest: String(repeating: "3", count: 64), operation: "canonical-complete-epoch"),
                         .init(name: .applied, count: 7, digest: String(repeating: "4", count: 64), operation: "original-control-witness")]
            } else if command.action == .edit {
                facts = [.init(name: .localCommitted, count: 9, digest: String(repeating: "5", count: 64), operation: "n1"),
                         .init(name: .localDurable, count: 2, digest: String(repeating: "5", count: 64), operation: "n1"),
                         .init(name: .localDurable, count: editGeneration, digest: String(repeating: "6", count: 64), operation: "original-edit-witness")]
            } else if command.action == .syncNow {
                facts = [.init(name: .syncResult, count: 1, digest: String(repeating: "7", count: 64), operation: "original-exchange-issued")]
            }
            for fact in facts { append(.observation, command: command, facts: [fact]) }
            if command.action == .syncNow && recordTerminal {
                append(.observation, command: command, facts: [
                    .init(name: .syncResult, count: 1, operation: "terminal-exchange-succeeded"),
                    .init(name: .syncResult, count: 30, operation: "terminal-ended-ms"),
                    .init(name: .syncResult, count: 1, operation: "terminal-selected"),
                    .init(name: .syncResult, count: 0, operation: "terminal-held-back"),
                    .init(name: .syncResult, count: 0, operation: "terminal-newer-pending"),
                    .init(name: .syncResult, count: 0, operation: "terminal-incoming-waiting")
                ])
            }
            if command.command_seq <= completed { append(.done, command: command) }
        }
        return .init(header: header, commands: commands, records: records)
    }
    @MainActor @Test func onlyBoundedPrefixesSelectOriginalOwnedReads() throws {
        let mac = try AgentSelectedRestoration(state())
        #expect(mac.nextCommand == 4 && mac.edit == nil && mac.pending == nil)
        let edited = try AgentSelectedRestoration(state(completed: 4))
        #expect(edited.nextCommand == 5 && edited.edit?.seq == 9 && edited.edit?.revision == 2)
        let interruptedEdit = try AgentSelectedRestoration(state(pending: true))
        #expect(interruptedEdit.pending?.action == .edit && interruptedEdit.edit?.seq == 9)
        let exchange = try AgentSelectedRestoration(state(completed: 4, pending: true))
        #expect(exchange.exchange?.exchange == 1 && exchange.pending?.action == .syncNow)
        #expect(exchange.exchange?.terminal.count == 6)
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, recordTerminal: false))
        }
        let phone = try AgentSelectedRestoration(state(phone: true))
        #expect(phone.nextCommand == 4 && phone.edit == nil)
        let phoneExchange = try AgentSelectedRestoration(state(phone: true, pending: true))
        #expect(phoneExchange.exchange?.exchange == 1)
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(completed: 2)) }
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(phone: true, completed: 4)) }
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(completed: 5)) }
    }
    @MainActor @Test func partialWrongInstanceReorderedOrMissingOriginalWitnessFails() throws {
        let original = try state(pending: true)
        for operation in ["n1", "original-edit-witness", "original-control-witness"] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == operation })
            records[index].facts[0].digest = nil
            #expect(throws: (any Error).self) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
        for field in ["agent_instance", "record_seq", "command_sha256"] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == "original-edit-witness" })
            var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(records[index])) as? [String: Any])
            if field == "record_seq" { json[field] = 1 }
            else { json[field] = field == "agent_instance" ? "fresh2" : String(repeating: "9", count: 64) }
            records[index] = try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
            #expect(throws: (any Error).self) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
    }
    @MainActor @Test func originalEditGenerationBelongsToItsOwnOpening() throws {
        for completed in [3, 4] {
            let restored = try AgentSelectedRestoration(state(completed: completed, pending: true, editGeneration: 8))
            #expect(restored.originalGeneration == 7)
            #expect(restored.edit?.generation == 8)
            #expect(restored.edit?.seq == 9 && restored.edit?.revision == 2)
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(pending: true, editGeneration: 0))
        }
    }
    @MainActor @Test func interruptedExchangeRequiresACompleteResultFromItsOriginalProcess() throws {
        let original = try state(completed: 4, pending: true)
        for foreign in [false, true] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == "terminal-exchange-succeeded" })
            if foreign {
                var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(records[index])) as? [String: Any])
                json["agent_instance"] = "fresh2"
                records[index] = try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
            } else {
                records[index].facts.removeLast()
            }
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
    }
}
