import Darwin
import Foundation
import Testing

// Protocol evidence only. The independent on-disk synthetic operation store
// below is deliberately not a Core/SDK witness or signed execution result.
@MainActor @Suite(.serialized) struct CloudKitAgentSelectedProgramTests {
    private static let mac: [(String, [String: String])] = [
        ("launch", [:]), ("connect", ["preview": "p1"]), ("publish", ["preview": "p1"]),
        ("edit", ["note": "n1", "field": "Q"]), ("sync_now", [:]), ("close", [:])
    ]
    private static let phone: [(String, [String: String])] = [
        ("launch", [:]), ("connect", ["preview": "p1"]), ("adopt", ["preview": "p1"]),
        ("sync_now", [:]), ("observe", ["note": "n1"]), ("close", [:])
    ]
    fileprivate static func header(kind: String = "mac", selection: String = "sync_now") throws -> Data {
        var fields = try #require(JSONSerialization.jsonObject(with: CloudKitAgentProtocolTests.header) as? [String: Any])
        fields["selection"] = selection; fields["device_kind"] = kind
        return try JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys])
    }
    fileprivate static func command(header: Data, sequence: Int, action: String, arguments: [String: String],
                                    recovery: String? = nil) throws -> Data {
        var fields: [String: Any] = ["header_sha256": AgentJSON.hash(header), "command_seq": sequence,
            "leg": "sync_now", "action": action, "arguments": arguments, "account_before": "acct1", "account_after": "acct1"]
        if let recovery { fields["recovery_check"] = recovery }
        var bytes = try JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]); bytes.append(10)
        return bytes
    }
    private static func prefix(_ count: Int, header: Data, phone: Bool = false, recovery: String? = nil) throws -> Data {
        try (phone ? Self.phone : mac).prefix(count).enumerated().reduce(into: Data()) { bytes, entry in
            bytes.append(try command(header: header, sequence: entry.offset + 1, action: entry.element.0,
                arguments: entry.element.1, recovery: entry.offset + 1 == count ? recovery : nil))
        }
    }
    @Test func exactSelectedGrammarPositionsKindsAndRecoveryChecks() throws {
        for (kind, rows) in [("mac", Self.mac), ("iphone", Self.phone)] {
            let bytes = try Self.header(kind: kind), header = try AgentHeader.read(bytes), program = try header.program
            #expect(program.actions.map(\.rawValue) == rows.map(\.0))
            for (offset, row) in rows.enumerated() {
                let position = offset + 1
                let line = try Self.command(header: bytes, sequence: position, action: row.0, arguments: row.1)
                let command = try AgentCommand.read(Data(line.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                try program.requirePosition(command)
                for replacement in rows where replacement.0 != row.0 {
                    let bad = try Self.command(header: bytes, sequence: position, action: replacement.0, arguments: replacement.1)
                    #expect(throws: AgentFailure.sequence) {
                        try AgentCommand.read(Data(bad.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                    }
                }
                for badArguments in [["extra": "value"], row.1.merging(["content": "not-admitted"]) { _, rhs in rhs }] {
                    let bad = try Self.command(header: bytes, sequence: position, action: row.0, arguments: badArguments)
                    #expect(throws: AgentFailure.shape) {
                        try AgentCommand.read(Data(bad.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                    }
                }
                let wrongLiteralCases = row.1.map { argument in
                    row.1.merging([argument.key: "invalid-argument-literal"]) { _, replacement in replacement }
                }
                for rejectedArguments in wrongLiteralCases {
                    let rejectedLine = try Self.command(header: bytes, sequence: position, action: row.0, arguments: rejectedArguments)
                    #expect(throws: AgentFailure.shape) {
                        try AgentCommand.read(Data(rejectedLine.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                    }
                }
                for check in ["sync_now.edit", "sync_now.exchange", "smoke-durable-state"] {
                    let recovered = try Self.command(header: bytes, sequence: position, action: row.0, arguments: row.1, recovery: check)
                    let allowed = row.0 == "sync_now" && check == "sync_now.exchange"
                        || kind == "mac" && position == 4 && check == "sync_now.edit"
                    if allowed {
                        _ = try AgentCommand.read(Data(recovered.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                    } else {
                        #expect(throws: AgentFailure.unsupported) {
                            try AgentCommand.read(Data(recovered.dropLast()), header: AgentJSON.hash(bytes), sequence: position, program: program)
                        }
                    }
                }
            }
            let extra = try Self.command(header: bytes, sequence: 7, action: "close", arguments: [:])
            #expect(throws: AgentFailure.sequence) {
                try AgentCommand.read(Data(extra.dropLast()), header: AgentJSON.hash(bytes), sequence: 7, program: program)
            }
        }
        #expect(throws: AgentFailure.unsupported) { try AgentHeader.read(Self.header(kind: "ipad")) }
        #expect(throws: AgentFailure.unsupported) { try AgentHeader.read(Self.header(selection: "sync-now")) }
        for kind in ["mac", "iphone", "ipad"] {
            #expect(try AgentHeader.read(Self.header(kind: kind, selection: "smoke")).program == .smoke)
        }
        // Existing smoke has no new action or selected recovery vocabulary.
        for action in ["adopt", "edit", "observe"] {
            let command = CloudKitAgentProtocolTests.command(2, action: action)
            #expect(throws: AgentFailure.unsupported) {
                try AgentCommand.read(Data(command.dropLast()), header: AgentJSON.hash(CloudKitAgentProtocolTests.header), sequence: 2)
            }
        }
    }

    @Test func selectedTransportNeedsItsOwnSixActionJoinedTerminalAndNeverDuplicatesEffects() async throws {
        for phone in [false, true] {
            let fixture = try SelectedFixture(); defer { fixture.remove() }
            let bytes = try Self.header(kind: phone ? "iphone" : "mac"), program = try AgentHeader.read(bytes).program
            let runtime = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
            let engine = try AgentEngine(storage: fixture.storage, runtime: runtime, instance: "selected1")
            var retained = Data()
            for count in 1...6 {
                let commands = try Self.prefix(count, header: bytes, phone: phone)
                retained = try await engine.deliver(header: bytes, commands: commands, retainedJournal: retained)
                #expect(AgentEngine.programCompleted(try fixture.records(), program: program) == (count == 6))
                #expect(!AgentEngine.smokeCompleted(try fixture.records()))
                #expect(try await engine.deliver(header: bytes, commands: commands, retainedJournal: retained) == retained)
            }
            #expect(runtime.executions == 6)
            #expect(try runtime.receipts().map(\.sequence) == Array(1...6))
            let wrongProgram: AgentProgram = phone ? .syncNowMac : .syncNowPhone
            #expect(!AgentEngine.programCompleted(try fixture.records(), program: wrongProgram))
            var withoutJoin = try fixture.records()
            let terminal = try #require(withoutJoin.lastIndex { $0.kind == .done })
            withoutJoin[terminal].checks.removeAll { $0.name == .localDurability }
            #expect(!AgentEngine.programCompleted(withoutJoin, program: program))
        }
    }

    @Test func receiverManualExchangeHasNoMutationAndWaitsOnlyForItsOriginalResult() async throws {
        let events = AgentExchangeEvents<UInt64>()
        var order: [String] = [], requests = 0, settles = 0
        events.yield(7); events.yield(8); events.yield(12)
        let actual = try await AgentManualExchange.run(prepare: { order.append("prepare") }, request: {
            requests += 1; order.append("request"); return true
        }, afterRequest: { settles += 1; order.append("settle") }, exactResult: {
            order.append("read-original")
            return try await events.next(exchange: 12, identifiedBy: { $0 })
        }, afterResult: { settles += 1; order.append("settle") })
        #expect(actual == 12 && requests == 1 && settles == 2)
        #expect(order == ["prepare", "request", "settle", "read-original", "settle"])
        // No mutation or review callbacks exist in the reusable execution path.
        await #expect(throws: AgentFailure.execution) {
            try await AgentManualExchange.run(prepare: {}, request: { false },
                afterRequest: { Issue.record("Refused exchange must not settle") },
                exactResult: { Issue.record("Refused exchange must not read another result"); return 99 },
                afterResult: { Issue.record("Refused exchange must not finish") })
        }
        var resultReads = 0
        await #expect(throws: AgentFailure.execution) {
            try await AgentManualExchange.run(prepare: { throw AgentFailure.execution },
                request: { requests += 1; return true }, afterRequest: {}, exactResult: { resultReads += 1; return 99 }, afterResult: {})
        }
        #expect(requests == 1 && resultReads == 0)
        var failedRequests = 0, failedReads = 0
        await #expect(throws: AgentFailure.execution) {
            try await AgentManualExchange.run(prepare: {}, request: { failedRequests += 1; return true },
                afterRequest: {}, exactResult: { () throws -> Int in failedReads += 1; throw AgentFailure.execution },
                afterResult: { Issue.record("Failed original result must not complete") })
        }
        #expect(failedRequests == 1 && failedReads == 1)
    }

    @Test func freshFullyJoinedSmokeDeliveryNeedsNoAccountProgressOrRestoration() async throws {
        let fixture = try SelectedFixture(); defer { fixture.remove() }
        let bytes = CloudKitAgentProtocolTests.header
        let original = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        let first = try AgentEngine(storage: fixture.storage, runtime: original, instance: "smoke-original")
        var commands = Data(), retained = Data()
        for (offset, action) in AgentCommand.smokeProgram.enumerated() {
            commands.append(CloudKitAgentProtocolTests.command(offset + 1, action: action.rawValue))
            retained = try await first.deliver(header: bytes, commands: commands, retainedJournal: retained)
        }
        #expect(AgentEngine.smokeCompleted(try fixture.records()))
        #expect(original.executions == 10)
        let fresh = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        // Model the actual smoke runtime's deliberate missing restore boundary.
        fresh.restoreFailure = .implementationMissing
        let restart = try AgentEngine(storage: fixture.storage, runtime: fresh, instance: "smoke-fresh")
        let changedCommands = Data([32]) + commands
        await #expect(throws: AgentFailure.prefix) {
            try await restart.deliver(header: bytes, commands: changedCommands, retainedJournal: retained)
        }
        await #expect(throws: AgentFailure.prefix) {
            try await restart.deliver(header: bytes, commands: commands, retainedJournal: retained + Data("{}\n".utf8))
        }
        await #expect(throws: AgentFailure.binding) {
            try await restart.deliver(header: bytes + Data([32]), commands: commands, retainedJournal: retained)
        }
        #expect(fresh.order.isEmpty)
        #expect(try await restart.deliver(header: bytes, commands: commands, retainedJournal: retained) == retained)
        #expect(fresh.order == ["product"])
        fresh.productFailure = .binding
        await #expect(throws: AgentFailure.binding) {
            try await restart.deliver(header: bytes, commands: commands, retainedJournal: retained)
        }
        #expect(fresh.order == ["product", "product"])
        #expect(fresh.accountReads == 0 && fresh.progressReads == 0 && fresh.restores == 0)
        #expect(fresh.executions == 0 && fresh.recoveries == 0 && fresh.joins == 0)
        #expect(try fixture.storage.read(.journal) == retained)
        #expect(try fixture.storage.read(.commands) == commands)
        #expect(try fixture.storage.read(.header) == bytes)
    }

    @Test func completedPrefixNoOpDefersIndependentRestorationUntilSuccessorExactlyOnce() async throws {
        let fixture = try SelectedFixture(); defer { fixture.remove() }
        let bytes = try Self.header(), launch = try Self.prefix(1, header: bytes)
        let original = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        let first = try AgentEngine(storage: fixture.storage, runtime: original, instance: "before-kill")
        let retained = try await first.deliver(header: bytes, commands: launch, retainedJournal: Data())
        let fresh = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        let restart = try AgentEngine(storage: fixture.storage, runtime: fresh, instance: "after-kill")
        #expect(fresh.executions == 0 && fresh.restores == 0)
        #expect(try await restart.deliver(header: bytes, commands: launch, retainedJournal: retained) == retained)
        #expect(fresh.restores == 0 && fresh.executions == 0 && fresh.accountReads == 0 && fresh.progressReads == 0)
        #expect(fresh.order == ["product"])
        #expect(try fixture.storage.read(.journal) == retained)
        await #expect(throws: AgentFailure.prefix) {
            try await restart.deliver(header: bytes, commands: Data([32]) + launch, retainedJournal: retained)
        }
        #expect(fresh.order == ["product"] && fresh.accountReads == 0 && fresh.restores == 0 && fresh.progressReads == 0)
        let result = try await restart.deliver(header: bytes, commands: Self.prefix(2, header: bytes), retainedJournal: retained)
        #expect(fresh.executions == 1 && fresh.restores == 1 && fresh.accountReads == 2 && original.executions == 1)
        #expect(fresh.order == ["product", "product", "account", "restore", "account", "execute-2"])
        #expect(try fresh.receipts().map(\.sequence) == [1, 2])
        #expect(try await restart.deliver(header: bytes, commands: Self.prefix(2, header: bytes), retainedJournal: result) == result)
        #expect(fresh.executions == 1 && fresh.restores == 1 && fresh.accountReads == 2)
        _ = try await restart.deliver(header: bytes, commands: Self.prefix(3, header: bytes), retainedJournal: result)
        #expect(fresh.executions == 2 && fresh.restores == 1 && fresh.accountReads == 3)
        #expect(try fresh.receipts().map(\.sequence) == [1, 2, 3])
    }

    @Test func restoreBindingAccountAndRetainedPrefixFailBeforeProgressReadOrCoreEffect() async throws {
        for failure in ["binding", "product", "account", "prefix"] {
            let fixture = try SelectedFixture(); defer { fixture.remove() }
            let bytes = try Self.header(), launch = try Self.prefix(1, header: bytes)
            let original = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
            let first = try AgentEngine(storage: fixture.storage, runtime: original, instance: "original")
            let retained = try await first.deliver(header: bytes, commands: launch, retainedJournal: Data())
            let fresh = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
            fresh.productFailure = failure == "product" ? .binding : nil
            fresh.accountFailure = failure == "account" ? .wrongContainer : nil
            let restart = try AgentEngine(storage: fixture.storage, runtime: fresh, instance: "fresh")
            let deliveredHeader = failure == "binding" ? bytes + Data([32]) : bytes
            let retainedPrefix = failure == "prefix" ? retained + Data("{}\n".utf8) : retained
            await #expect(throws: (any Error).self) {
                try await restart.deliver(header: deliveredHeader, commands: Self.prefix(2, header: bytes), retainedJournal: retainedPrefix)
            }
            #expect(fresh.restores == 0 && fresh.progressReads == 0 && fresh.executions == 0)
            if failure != "account" { #expect(fresh.accountReads == 0) }
            #expect(try fixture.storage.read(.journal) == retained)
        }
    }

    @Test func freshRuntimeIncompleteEditUsesOriginalDurableReceiptOrFailsWithoutReplay() async throws {
        for evidence in ["original", "missing", "different-operation"] {
            let fixture = try SelectedFixture(failDone: 4); defer { fixture.remove() }
            let bytes = try Self.header()
            let original = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
            let first = try AgentEngine(storage: fixture.storage, runtime: original, instance: "old")
            var retained = Data()
            for count in 1...3 {
                retained = try await first.deliver(header: bytes, commands: Self.prefix(count, header: bytes), retainedJournal: retained)
            }
            let commands = try Self.prefix(4, header: bytes, recovery: "sync_now.edit")
            await #expect(throws: AgentFailure.storage) {
                try await first.deliver(header: bytes, commands: commands, retainedJournal: retained)
            }
            // The old runtime is never used again; receipts survive separately.
            let state = try AgentFileStorage(directory: fixture.progress)
            var receipts = try JSONDecoder().decode([SelectedProtocolReceipt].self, from: state.read(.status))
            if evidence == "missing" { receipts.removeLast() }
            if evidence == "different-operation" {
                let last = receipts.removeLast()
                receipts.append(.init(sequence: last.sequence, action: last.action, commandDigest: String(repeating: "f", count: 64)))
            }
            try state.replace(.status, with: JSONEncoder().encode(receipts))
            let fresh = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
            let freshStorage = try AgentFileStorage(directory: fixture.root)
            let restart = try AgentEngine(storage: freshStorage, runtime: fresh, instance: "new")
            let result = try await restart.deliver(header: bytes, commands: commands, retainedJournal: retained)
            let records = try fixture.records(), terminal = try #require(records.last { $0.kind == .done || $0.kind == .failed })
            #expect(fresh.executions == 0 && fresh.restores == 1 && fresh.recoveries == 1)
            #expect(records.filter { $0.kind == .started && $0.command_seq == 4 }.count == 1)
            if evidence == "original" {
                let start = try #require(records.first { $0.kind == .started && $0.command_seq == 4 })
                #expect(terminal.kind == .done && terminal.recovered_from == start.record_seq)
                let successor = commands + (try Self.command(header: bytes, sequence: 5, action: "sync_now", arguments: [:]))
                _ = try await restart.deliver(header: bytes, commands: successor, retainedJournal: result)
                #expect(fresh.executions == 1)
                #expect(try fresh.receipts().filter { $0.action == "edit" }.count == 1)
            } else {
                #expect(terminal.kind == .failed && terminal.failure == .implementationMissing && terminal.recovered_from == nil)
                let successor = commands + (try Self.command(header: bytes, sequence: 5, action: "sync_now", arguments: [:]))
                await #expect(throws: AgentFailure.sequence) {
                    try await restart.deliver(header: bytes, commands: successor, retainedJournal: result)
                }
                #expect(fresh.executions == 0)
            }
        }
    }

    @Test func completedJournalWithoutIndependentProgressNeverAuthorizesSuccessor() async throws {
        let fixture = try SelectedFixture(); defer { fixture.remove() }
        let bytes = try Self.header()
        let original = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        let first = try AgentEngine(storage: fixture.storage, runtime: original, instance: "old")
        let retained = try await first.deliver(header: bytes, commands: Self.prefix(1, header: bytes), retainedJournal: Data())
        try AgentFileStorage(directory: fixture.progress).replace(.status, with: JSONEncoder().encode([SelectedProtocolReceipt]()))
        let fresh = try SelectedProtocolRuntime(fixture: fixture, header: bytes)
        let restart = try AgentEngine(storage: fixture.storage, runtime: fresh, instance: "new")
        await #expect(throws: AgentFailure.implementationMissing) {
            try await restart.deliver(header: bytes, commands: Self.prefix(2, header: bytes), retainedJournal: retained)
        }
        #expect(fresh.restores == 1 && fresh.executions == 0 && fresh.recoveries == 0)
        #expect(try fixture.storage.read(.journal) == retained)
    }
}

private nonisolated struct SelectedProtocolReceipt: Codable {
    let sequence: Int
    let action: String
    let commandDigest: String
}

@MainActor private final class SelectedProtocolRuntime: AgentRuntime {
    let fixture: SelectedFixture, headerBytes: Data, progress: AgentFileStorage
    var productFailure: AgentFailure?, accountFailure: AgentFailure?, restoreFailure: AgentFailure?
    var executions = 0, restores = 0, recoveries = 0, accountReads = 0, progressReads = 0, joins = 0
    var order: [String] = []
    private var restored = false
    init(fixture: SelectedFixture, header: Data) throws {
        self.fixture = fixture; headerBytes = header
        progress = try AgentFileStorage(directory: fixture.progress)
    }
    func verifyProduct(_ header: AgentHeader) throws {
        order.append("product")
        if let productFailure { throw productFailure }
        guard header == (try AgentHeader.read(headerBytes)) else { throw AgentFailure.binding }
    }
    func verifyAccount(_ header: AgentHeader) async throws {
        order.append("account"); accountReads += 1
        guard try fixture.storage.read(.header) == headerBytes else { throw AgentFailure.binding }
        if let accountFailure { throw accountFailure }
    }
    func receipts() throws -> [SelectedProtocolReceipt] {
        progressReads += 1
        let bytes = try progress.read(.status)
        return bytes.isEmpty ? [] : try JSONDecoder().decode([SelectedProtocolReceipt].self, from: bytes)
    }
    private func digest(_ sequence: Int) throws -> String {
        let lines = AgentJSON.lines(try fixture.storage.read(.commands)).complete
        guard lines.indices.contains(sequence - 1) else { throw AgentFailure.binding }
        return AgentJSON.hash(lines[sequence - 1])
    }
    private func requireOriginal(_ command: AgentCommand, in receipts: [SelectedProtocolReceipt]) throws -> SelectedProtocolReceipt {
        let matches = receipts.filter { $0.sequence == command.command_seq }
        guard matches.count == 1, let receipt = matches.first, receipt.action == command.action.rawValue,
              receipt.commandDigest == (try digest(command.command_seq)) else { throw AgentFailure.implementationMissing }
        return receipt
    }
    func restore(_ state: AgentRestoration) async throws {
        order.append("restore"); restores += 1
        if let restoreFailure { throw restoreFailure }
        guard accountReads > 0, state.header == (try AgentHeader.read(headerBytes)) else { throw AgentFailure.binding }
        let durable = try receipts()
        for done in state.records where done.kind == .done {
            _ = try requireOriginal(state.commands[done.command_seq - 1], in: durable)
        }
        restored = true
    }
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        order.append("execute-\(command.command_seq)")
        guard command.command_seq == 1 || restored || executions > 0 else { throw AgentFailure.implementationMissing }
        var durable = try receipts()
        guard !durable.contains(where: { $0.sequence == command.command_seq }) else { throw AgentFailure.execution }
        durable.append(.init(sequence: command.command_seq, action: command.action.rawValue, commandDigest: try digest(command.command_seq)))
        try progress.replace(.status, with: JSONEncoder().encode(durable))
        executions += 1
        return checks(command)
    }
    private func checks(_ command: AgentCommand) -> [AgentCheck] {
        AgentEngine.requiredChecks(command.action).map {
            let name: AgentFact.Name = command.action == .close ? ($0 == .integrity ? .integrity : .ownedWorkJoined) : .commandCompleted
            return .init(name: $0, outcome: .pass, facts: [.init(name: name, count: UInt64(command.command_seq))])
        }
    }
    func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        recoveries += 1
        guard restored, started.command_sha256 == (try digest(command.command_seq)) else { throw AgentFailure.binding }
        _ = try requireOriginal(command, in: receipts())
        let fact = AgentFact(name: .recoveryCommitted, count: UInt64(command.command_seq))
        try observe(fact)
        return checks(command) + [.init(name: .recovery, outcome: .pass, facts: [fact]),
                                  .init(name: .integrity, outcome: .pass, facts: [.init(name: .integrity)])]
    }
    func cancelAndJoin() async throws { joins += 1 }
}

@MainActor private struct SelectedFixture {
    let root: URL, progress: URL, storage: AgentFileStorage
    init(failDone: Int? = nil) throws {
        guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
        defer { free(resolved) }
        root = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
            .appendingPathComponent("selected-agent-" + UUID().uuidString, isDirectory: true)
        progress = root.appendingPathComponent("synthetic-core", isDirectory: true)
        try FileManager.default.createDirectory(at: progress, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        storage = try AgentFileStorage(directory: root, hooks: .init(write: { fd, bytes, offset in
            if let failDone, let record = try? AgentRecord.read(Data(bytes.dropLast())),
               record.kind == .done, record.command_seq == failDone { return -1 }
            return bytes.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!.advanced(by: offset), $0.count - offset) }
        }))
    }
    func records() throws -> [AgentRecord] {
        try AgentJSON.lines(storage.read(.journal)).complete.map { try AgentRecord.read($0) }
    }
    func remove() { try? FileManager.default.removeItem(at: root) }
}
