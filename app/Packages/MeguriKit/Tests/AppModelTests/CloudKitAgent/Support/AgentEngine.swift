import Foundation

// Serial journal producer; terminal replies require a durable record.
@MainActor final class AgentEngine {
    let storage: any AgentStorage
    let runtime: any AgentRuntime
    let instance: String
    private var delivering = false
    private var journalError: (any Error)?
    private var joiningOwnedWork: Task<Void, any Error>?
    private var journalEntries: [AgentRecord] = []
    private var runDigest = ""
    private var acceptedLines: [Data] = []
    private var commands: [AgentCommand] = []
    private var admittedRuntime = false

    init(storage: any AgentStorage, runtime: any AgentRuntime, instance: String) throws {
        guard AgentJSON.label(instance) else { throw AgentFailure.shape }
        self.storage = storage; self.runtime = runtime; self.instance = instance
    }
    static func requiredChecks(_ action: AgentCommand.Action) -> Set<AgentCheck.Name> {
        switch action {
        case .launch: [.integrity]
        case .addNote, .startSession, .answer, .endSession, .addMedia, .edit: [.localDurability]
        case .close: [.integrity, .localDurability]
        case .connect, .publish, .adopt: [.coordinatorResult]
        case .syncNow: [.coordinatorResult]
        case .observe: [.localDurability]
        }
    }
    static func smokeCompleted(_ records: [AgentRecord]) -> Bool {
        programCompleted(records, program: .smoke)
    }
    static func programCompleted(_ records: [AgentRecord], program: AgentProgram) -> Bool {
        guard !records.contains(where: { $0.kind == .failed }) else { return false }
        let starts = records.filter { $0.kind == .started }
        let done = records.filter { $0.kind == .done }
        let positions = Array(1...program.actions.count)
        guard starts.map(\.action) == program.actions.map(Optional.some),
              starts.map(\.command_seq) == positions, done.map(\.command_seq) == positions,
              let close = done.last else { return false }
        return close.checks.contains { $0.name == .integrity && $0.outcome == .pass && $0.facts.contains { $0.name == .integrity } }
            && close.checks.contains { $0.name == .localDurability && $0.outcome == .pass && $0.facts.contains { $0.name == .ownedWorkJoined } }
    }
    private func validateChecks(_ checks: [AgentCheck], command: AgentCommand, recovered: Bool) throws {
        let names = Set(checks.map(\.name))
        guard names.count == checks.count, checks.allSatisfy({ $0.outcome == .pass && !$0.facts.isEmpty }),
              Self.requiredChecks(command.action).isSubset(of: names),
              !recovered || Set<AgentCheck.Name>([.recovery, .integrity]).isSubset(of: names) else { throw AgentFailure.recovery }
        for fact in checks.flatMap(\.facts) { try fact.validate() }
    }
    private func replay(_ bytes: Data, runDigest: String, acceptedLines: [Data], commands: [AgentCommand]) throws -> [AgentRecord] {
        var replayed: [AgentRecord] = []
        var startedByCommand: [Int: AgentRecord] = [:], completedSequences: Set<Int> = []
        var nextSequence = 1, terminalFailureSeen = false
        for line in AgentJSON.lines(bytes).complete {
            let entry = try AgentRecord.read(line)
            guard entry.record_seq == replayed.count + 1,
                  entry.header_sha256 == runDigest,
                  entry.command_seq > 0, entry.command_seq <= commands.count,
                  entry.command_sha256 == AgentJSON.hash(acceptedLines[entry.command_seq - 1]) else { throw AgentFailure.journal }
            switch entry.kind {
            case .started:
                guard !terminalFailureSeen, entry.command_seq == nextSequence, startedByCommand[entry.command_seq] == nil,
                      entry.action == commands[entry.command_seq - 1].action,
                      entry.command_seq == 1 || completedSequences.contains(entry.command_seq - 1) else { throw AgentFailure.journal }
                startedByCommand[entry.command_seq] = entry
            case .observation:
                guard startedByCommand[entry.command_seq] != nil else { throw AgentFailure.journal }
            case .done, .failed:
                guard !terminalFailureSeen, let startedEntry = startedByCommand[entry.command_seq], entry.command_seq == nextSequence,
                      !completedSequences.contains(entry.command_seq) else { throw AgentFailure.journal }
                if let recovered = entry.recovered_from {
                    guard recovered == startedEntry.record_seq,
                          commands[entry.command_seq - 1].recovery_check != nil else { throw AgentFailure.journal }
                }
                if entry.kind == .done {
                    try validateChecks(entry.checks, command: commands[entry.command_seq - 1], recovered: entry.recovered_from != nil)
                }
                completedSequences.insert(entry.command_seq)
                terminalFailureSeen = entry.kind == .failed
                nextSequence += 1
            }
            replayed.append(entry)
        }
        return replayed
    }
    private func append(_ kind: AgentRecord.Kind, command: AgentCommand, action: AgentCommand.Action? = nil,
                        facts: [AgentFact] = [], checks: [AgentCheck] = [], failure: AgentFailure? = nil,
                        recoveredFrom: Int? = nil) throws {
        if let journalError { throw journalError }
        for fact in facts + checks.flatMap(\.facts) { try fact.validate() }
        let entry = AgentRecord(header_sha256: runDigest, record_seq: journalEntries.count + 1,
            command_seq: command.command_seq, command_sha256: AgentJSON.hash(acceptedLines[command.command_seq - 1]),
            agent_instance: instance, observed_at: ISO8601DateFormatter().string(from: Date()), kind: kind,
            action: action, facts: facts, checks: checks, failure: failure,
            error_domain: failure == nil ? nil : .protocolError, error_code: failure == nil ? nil : 1,
            recovered_from: recoveredFrom)
        var bytes = try JSONEncoder().encode(entry)
        _ = try AgentRecord.read(bytes)
        bytes.append(10)
        do { try storage.appendJournal(bytes) }
        catch { fenceJournal(error); throw journalError! }
        journalEntries.append(entry)
    }
    private func observer(_ command: AgentCommand) -> (AgentFact) throws -> Void {
        { [self] fact in try append(.observation, command: command, facts: [fact]) }
    }
    private func fenceJournal(_ error: any Error) {
        guard journalError == nil else { return }
        // Neither a short append nor a failed flush proves what reached disk.
        journalError = error
        if joiningOwnedWork == nil { joiningOwnedWork = Task { try await runtime.cancelAndJoin() } }
    }
    private func finishUncertainJournal() async throws {
        guard let journalError else { return }
        _ = try? await joiningOwnedWork?.value
        throw journalError
    }
    private func cancelOwnedWork() async throws {
        if joiningOwnedWork == nil { joiningOwnedWork = Task { try await runtime.cancelAndJoin() } }
        do { try await joiningOwnedWork?.value }
        catch {
            try await finishUncertainJournal()
            joiningOwnedWork = nil
            throw error
        }
        try await finishUncertainJournal()
        joiningOwnedWork = nil
    }
    private func fail(_ command: AgentCommand, _ error: any Error) async throws {
        try await finishUncertainJournal()
        do { try append(.failed, command: command, failure: (error as? AgentFailure) ?? .execution) }
        catch {
            if journalError != nil { try await finishUncertainJournal() }
            throw error
        }
        try await cancelOwnedWork()
        do { try append(.observation, command: command, facts: [.init(name: .ownedWorkJoined)]) }
        catch {
            try await finishUncertainJournal()
            throw error
        }
    }
    func deliver(header bytes: Data, commands delivered: Data, retainedJournal: Data) async throws -> Data {
        guard !delivering else { throw AgentFailure.busy }
        delivering = true
        defer { delivering = false }
        try await finishUncertainJournal()
        let header = try AgentHeader.read(bytes)
        let program = try header.program
        let candidateHash = AgentJSON.hash(bytes)
        let acceptedHeader = try storage.read(.header)
        guard acceptedHeader.isEmpty || acceptedHeader == bytes else { throw AgentFailure.binding }
        let split = AgentJSON.lines(delivered)
        let candidateBytes = split.complete
        let candidateCommands = try candidateBytes.enumerated().map {
            try AgentCommand.read($0.element, header: candidateHash, sequence: $0.offset + 1, program: program)
        }
        if let close = candidateCommands.first(where: { $0.action == .close }), close.command_seq != candidateCommands.count { throw AgentFailure.sequence }
        let accepted = try storage.read(.commands)
        let completeCommands = candidateBytes.reduce(into: Data()) { $0.append($1); $0.append(10) }
        guard completeCommands.starts(with: accepted) else { throw AgentFailure.prefix }
        let journal = try storage.read(.journal)
        guard retainedJournal.isEmpty || retainedJournal.last == 10,
              journal.starts(with: retainedJournal) else { throw AgentFailure.prefix }
        let journalSplit = AgentJSON.lines(journal)
        let completeJournal = Data(journal.dropLast(journalSplit.suffix.count))
        let candidateEntries = try replay(completeJournal, runDigest: candidateHash, acceptedLines: candidateBytes, commands: candidateCommands)
        let completedEntries = candidateEntries.filter { $0.kind == .done || $0.kind == .failed }
        // One pending command per delivery; failure admits no successor.
        guard candidateCommands.count <= completedEntries.count + 1 else { throw AgentFailure.sequence }
        if let failure = completedEntries.first(where: { $0.kind == .failed }), candidateCommands.count > failure.command_seq { throw AgentFailure.sequence }
        try runtime.verifyProduct(header)
        if !journalSplit.suffix.isEmpty {
            do { try storage.preserveAndRemoveSuffix(journalSplit.suffix, complete: completeJournal) }
            catch {
                fenceJournal(error)
                try await finishUncertainJournal()
            }
        }
        if acceptedHeader.isEmpty { try storage.replace(.header, with: bytes) }
        if accepted != completeCommands { try storage.replace(.commands, with: completeCommands) }
        // No suspension between admission and installing this delivery.
        runDigest = candidateHash
        acceptedLines = candidateBytes
        commands = candidateCommands
        journalEntries = candidateEntries
        runtime.installCallbackJournal { [weak self] in
            guard let self, let started = journalEntries.last(where: { $0.kind == .started }) else {
                throw AgentFailure.journal
            }
            let command = commands[started.command_seq - 1]
            return { [weak self] facts in
                guard let self, !facts.isEmpty else { throw AgentFailure.journal }
                try append(.observation, command: command, facts: facts)
            }
        }
        if completedEntries.contains(where: { $0.kind == .failed }) {
            // A crash after terminal failure can leave cancellation unfinished.
            try await cancelOwnedWork()
            return try storage.read(.journal)
        }
        // A completed delivery needs no fresh runtime. Keep admission deferred
        // so an appended successor must still verify the account and restore.
        if completedEntries.count == commands.count { return try storage.read(.journal) }
        if !admittedRuntime {
            do {
                if !candidateEntries.isEmpty {
                    try await runtime.verifyAccount(header)
                    try await runtime.restore(.init(header: header, commands: commands, records: candidateEntries))
                }
                admittedRuntime = true
            } catch {
                // Only an already-started command can own a failed terminal.
                // Do not start a successor merely to report failed restoration.
                if completedEntries.count < commands.count,
                   journalEntries.contains(where: { $0.kind == .started && $0.command_seq == commands[completedEntries.count].command_seq }) {
                    try await fail(commands[completedEntries.count], error)
                    return try storage.read(.journal)
                }
                try await cancelOwnedWork()
                throw error
            }
        }
        let command = commands[completedEntries.count]
        if let startedEntry = journalEntries.first(where: { $0.kind == .started && $0.command_seq == command.command_seq }) {
            do {
                guard command.recovery_check != nil else { throw AgentFailure.recovery }
                try await runtime.verifyAccount(header)
                let checks = try await runtime.recover(command, started: startedEntry, observe: observer(command))
                try validateChecks(checks, command: command, recovered: true)
                try append(.done, command: command, checks: checks, recoveredFrom: startedEntry.record_seq)
            } catch { try await fail(command, error) }
        } else {
            do {
                try append(.started, command: command, action: command.action)
                try await runtime.verifyAccount(header)
                try append(.observation, command: command, facts: [.init(name: .accountVerified)])
                let checks = try await runtime.execute(command, observe: observer(command))
                try validateChecks(checks, command: command, recovered: false)
                try append(.done, command: command, checks: checks)
            } catch { try await fail(command, error) }
        }
        return try storage.read(.journal)
    }
}
