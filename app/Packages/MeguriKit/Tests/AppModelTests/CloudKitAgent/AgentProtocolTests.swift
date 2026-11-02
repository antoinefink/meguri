import Foundation
import Testing
import Darwin

@MainActor @Suite(.serialized) struct CloudKitAgentProtocolTests {
    static let header = Data("""
    { "protocol_version":1,"job":"cloudkit-signed","run_id":"20320405T060708Z-1","lane":"cloud-signed","source_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","tree_clean":true,"device":"rig7","device_kind":"mac","os_build":"os-sample-7","app_build":"app-sample-7","executable_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","build_receipt":{"job":"receipt-build-7","run_id":"20320405T060708Z-2"},"build_receipt_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","deployment_receipt":{"job":"receipt-install-7","run_id":"20320405T060708Z-3"},"container":"cloud-sample-7","environment":"development","account_scope":"scope7","account_labels":["acct1"],"selection":"smoke","fixture_delivery":{"base":"small","fixtures_receipt":{"job":"fixtures","run_id":"20320405T060708Z-4"},"source":"/fixture-root/suite7/input","manifest":{"path":"suite7-index.json","sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","bytes":10},"frozen_input":{"path":"fixture-input.json","sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","bytes":20}} }
    """.utf8)
    static func command(_ sequence: Int = 1, action: String = "launch", recovery: Bool = false) -> Data {
        Data("{\"header_sha256\":\"\(AgentJSON.hash(header))\",\"command_seq\":\(sequence),\"leg\":\"smoke\",\"action\":\"\(action)\",\"arguments\":\(action == "add_note" ? "{\"fixture\":\"small\",\"entity\":\"n1\"}" : "{}"),\"account_before\":\"acct1\",\"account_after\":\"acct1\"\(recovery ? ",\"recovery_check\":\"smoke-durable-state\"" : "")}\n".utf8)
    }
    private func harness() throws -> (AgentMemoryStorage, AgentFakeRuntime, AgentEngine) {
        let store = AgentMemoryStorage()
        let runtime = AgentFakeRuntime(store: store)
        return (store, runtime, try AgentEngine(storage: store, runtime: runtime, instance: "process1"))
    }
    @Test func duplicateDeliveryCountsOneExecutionAndAcknowledgesOnlyDurableTerminal() async throws {
        let (store, runtime, engine) = try harness()
        let first = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let second = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: first)
        #expect(first == second)
        #expect(runtime.executions == 1 && runtime.lookups == 1)
        #expect(try store.records().filter { $0.kind == .started }.count == 1)
        #expect(try store.records().filter { $0.kind == .done }.count == 1)
        #expect(store.events.firstIndex(of: "durable-started")! < store.events.firstIndex(of: "bootstrap")!)
        #expect(store.events.last == "durable-done")
    }
    @Test func sequentialMutationAndSyncDuplicatesCountEachExecutorOnce() async throws {
        let (store, runtime, engine) = try harness()
        let launch = Self.command()
        let first = try await engine.deliver(header: Self.header, commands: launch, retainedJournal: Data())
        let mutation = launch + Self.command(2, action: "add_note")
        let second = try await engine.deliver(header: Self.header, commands: mutation, retainedJournal: first)
        _ = try await engine.deliver(header: Self.header, commands: mutation, retainedJournal: second)
        let sync = mutation + Self.command(3, action: "sync_now")
        let third = try await engine.deliver(header: Self.header, commands: sync, retainedJournal: second)
        _ = try await engine.deliver(header: Self.header, commands: sync, retainedJournal: third)
        #expect(runtime.executions == 3 && runtime.mutations == 1)
        #expect(try store.records().filter { $0.kind == .done }.count == 3)
    }
    @Test func incompleteCommandHasNoEffectAndCanBeFinished() async throws {
        let (_, runtime, engine) = try harness()
        let incomplete = Data(Self.command().dropLast(3))
        let pending = try await engine.deliver(header: Self.header, commands: incomplete, retainedJournal: Data())
        #expect(pending.isEmpty && runtime.executions == 0 && runtime.lookups == 0)
        _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        #expect(runtime.executions == 1)
    }
    @Test func terminalFlushFailureCannotAcknowledgeAndStillJoinsOwnedWork() async throws {
        let (store, runtime, engine) = try harness()
        store.failDone = true
        await #expect(throws: AgentFailure.storage) {
            try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        }
        #expect(runtime.executions == 1 && runtime.joins == 1 && runtime.workJoined)
        #expect(try !store.records().contains { $0.kind == .done })
    }
    @Test func changedRemovedReorderedAndGappedPrefixRefusesBeforeAnotherExecution() async throws {
        let (store, runtime, engine) = try harness()
        let first = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        var changed = Self.command(); changed.insert(32, at: changed.startIndex)
        for invalid in [changed, Data(), Self.command(3, action: "sync_now"), Self.command(2, action: "add_note") + Self.command()] {
            await #expect(throws: (any Error).self) { try await engine.deliver(header: Self.header, commands: invalid, retainedJournal: first) }
        }
        #expect(runtime.executions == 1)
        #expect(try store.read(.journal) == first)
    }
    @Test func whitespaceIsLegalButRawBytesAndTheirHashesAreImmutable() async throws {
        let (store, runtime, engine) = try harness()
        let command = Data(" \t".utf8) + Self.command()
        let journal = try await engine.deliver(header: Self.header, commands: command, retainedJournal: Data())
        let records = try store.records()
        #expect(records.first?.header_sha256 == AgentJSON.hash(Self.header))
        #expect(records.first?.command_sha256 == AgentJSON.hash(Data(command.dropLast())))
        #expect(try store.read(.header) == Self.header)
        #expect(try store.read(.commands) == command)
        await #expect(throws: AgentFailure.prefix) { try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: journal) }
        #expect(runtime.executions == 1)
    }
    @Test func independentRuntimeBindingsRejectBeforeLookupOrDatabase() async throws {
        for (old, new) in [("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "dddddddddddddddddddddddddddddddddddddddd"),
                           ("app-sample-7", "app-sample-8"), ("os-sample-7", "os-sample-8"),
                           ("\"device\":\"rig7\"", "\"device\":\"rig8\""), ("cloud-sample-7", "cloud-sample-8"),
                           ("development\"", "production\""), ("scope7", "scope8"), ("20320405T060708Z-1", "20320405T060708Z-9"),
                           (String(repeating: "b", count: 64), String(repeating: "d", count: 64))] {
            let (store, runtime, engine) = try harness()
            let changedHeader = Data(String(decoding: Self.header, as: UTF8.self).replacingOccurrences(of: old, with: new).utf8)
            let command = Data(String(decoding: Self.command(), as: UTF8.self).replacingOccurrences(of: AgentJSON.hash(Self.header), with: AgentJSON.hash(changedHeader)).utf8)
            await #expect(throws: (any Error).self) { try await engine.deliver(header: changedHeader, commands: command, retainedJournal: Data()) }
            #expect(runtime.lookups == 0 && runtime.executions == 0 && runtime.databaseCalls == 0)
            #expect(try store.read(.journal).isEmpty)
        }
    }
    @Test func wrongFreshAccountFailsBeforeDatabaseAndSuccessorAndJoinsLateCallbacks() async throws {
        let (store, runtime, engine) = try harness()
        runtime.accountMatches = false
        let failed = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let records = try store.records()
        #expect(runtime.lookups == 1 && runtime.databaseCalls == 0 && runtime.executions == 0)
        #expect(runtime.joins == 1)
        #expect(records.first { $0.kind == .failed }?.failure == .wrongContainer)
        #expect(records.last?.facts.first?.name == .ownedWorkJoined)
        await #expect(throws: AgentFailure.sequence) {
            try await engine.deliver(header: Self.header, commands: Self.command() + Self.command(2, action: "sync_now"), retainedJournal: failed)
        }
        _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: failed)
        #expect(runtime.lookups == 1 && runtime.executions == 0)
    }
    @Test func failedExecutionCancelsAndJoinsOriginalWorkKeepingCallbackOrder() async throws {
        let (store, runtime, engine) = try harness()
        runtime.failExecution = true
        _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let records = try store.records()
        let failure = try #require(records.firstIndex { $0.kind == .failed })
        let late = try #require(records.lastIndex { $0.facts.first?.name == .engineCallback })
        #expect(late > failure)
        #expect(runtime.joins == 1 && runtime.workJoined)
        #expect(records.filter { $0.kind == .done || $0.kind == .failed }.count == 1)
        #expect(records[late].command_seq == 1)
        #expect(!String(decoding: try store.read(.journal), as: UTF8.self).contains("SYNTHETIC_PRIVATE_ERROR"))
    }
    @Test func callbacksFromBootstrapFollowDurableStart() async throws {
        let (store, _, engine) = try harness()
        _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let records = try store.records()
        #expect(records.first?.kind == .started)
        let callback = try #require(records.first { $0.facts.first?.name == .engineCallback })
        #expect(callback.record_seq > 1 && callback.command_seq == 1)
    }
    @Test func preAndPostCommitInterruptionRecoverWithoutMutationReplay() async throws {
        for committed in [false, true] {
            let (store, runtime, firstEngine) = try harness()
            let launch = Self.command()
            let retained = try await firstEngine.deliver(header: Self.header, commands: launch, retainedJournal: Data())
            let mutation = Self.command(2, action: "add_note", recovery: true)
            try seedStarted(store, command: mutation, sequence: 2)
            runtime.committed = committed
            runtime.callback = nil // A killed process cannot retain an observer.
            let restarted = try AgentEngine(storage: store, runtime: runtime, instance: "process2")
            _ = try await restarted.deliver(header: Self.header, commands: launch + mutation, retainedJournal: retained)
            let terminal = try #require(try store.records().last)
            #expect(terminal.kind == .done && terminal.recovered_from != nil)
            #expect(runtime.executions == 1 && runtime.recoveries == 1 && runtime.mutations == 0)
            #expect(terminal.checks.flatMap(\.facts).contains { $0.name == (committed ? .recoveryCommitted : .recoveryNotCommitted) })
        }
    }
    @Test func indeterminateRecoveryIsTerminalAndDoesNotRunSuccessor() async throws {
        let (store, runtime, firstEngine) = try harness()
        let retained = try await firstEngine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let mutation = Self.command(2, action: "add_note", recovery: true)
        try seedStarted(store, command: mutation, sequence: 2)
        runtime.failRecovery = true
        runtime.callback = nil // A killed process cannot retain an observer.
        let restart = try AgentEngine(storage: store, runtime: runtime, instance: "process2")
        let failed = try await restart.deliver(header: Self.header, commands: Self.command() + mutation, retainedJournal: retained)
        await #expect(throws: AgentFailure.sequence) {
            try await restart.deliver(header: Self.header, commands: Self.command() + mutation + Self.command(3, action: "sync_now"), retainedJournal: failed)
        }
        let records = try store.records()
        #expect(records.map(\.record_seq) == Array(1...records.count))
        #expect(records.filter { $0.kind == .failed }.map(\.command_seq) == [2])
        let recoveryCallback = try #require(records.last { $0.facts.first?.name == .engineCallback })
        #expect(recoveryCallback.command_seq == 2 && recoveryCallback.agent_instance == "process2")
        #expect(recoveryCallback.command_sha256 == AgentJSON.hash(Data(mutation.dropLast())))
        #expect(try store.read(.journal) == failed)
        #expect(runtime.recoveries == 1 && runtime.mutations == 0 && runtime.executions == 1)
    }
    @Test func incompleteTailPreservedButCompleteDamageDuplicateTerminalAndLostPrefixRefuse() async throws {
        let (store, runtime, engine) = try harness()
        let retained = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let tail = Data("{\"kind\":\"done\"".utf8)
        store.files[.journal] = retained + tail
        _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: retained)
        #expect(store.diagnostics == [tail])
        #expect(try store.read(.journal) == retained)
        for damaged in [retained + Data("{}\n".utf8), retained + Data(AgentJSON.lines(retained).complete.last!) + Data([10]), Data(retained.dropLast(1)), Data("{}\n".utf8)] {
            store.files[.journal] = damaged
            await #expect(throws: (any Error).self) { try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: retained) }
        }
        #expect(runtime.executions == 1)
    }
    @Test func copiedDeviceJournalAndTerminalWithoutStartAreRejected() async throws {
        let (store, runtime, engine) = try harness()
        let journal = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        let lines = AgentJSON.lines(journal).complete
        store.files[.journal] = lines.last! + Data([10])
        await #expect(throws: (any Error).self) { try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data()) }
        store.files[.journal] = Data(String(decoding: journal, as: UTF8.self).replacingOccurrences(of: AgentJSON.hash(Self.header), with: String(repeating: "d", count: 64)).utf8)
        await #expect(throws: (any Error).self) { try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data()) }
        #expect(runtime.executions == 1)
    }
    @Test func strictJSONAndTypedArgumentsRejectBeforeExecution() async throws {
        for malformed in [Data("{\"a\":1,\"a\":2}".utf8), Data("{\"a\":1,\"\\u0061\":2}".utf8), Data("{\"a\":1e999}".utf8), Data("{\"a\":NaN}".utf8), Data([123, 34, 255, 34, 58, 49, 125]), Data("{\"a\":01}".utf8)] {
            #expect(throws: AgentFailure.json) { try AgentJSON.parse(malformed) }
        }
        let (store, runtime, engine) = try harness()
        for (old, new) in [("\"arguments\":{}", "\"arguments\":{\"text\":\"SYNTHETIC_CONTENT_MARKER\"}"),
                           ("\"arguments\":{}", "\"arguments\":{\"fixture\":3}"),
                           ("\"launch\"", "\"unknown\""), ("\"leg\":\"smoke\"", "\"leg\":\"engine\""),
                           ("\"command_seq\":1", "\"command_seq\":1,\"unexpected\":true")] {
            let bytes = Data(String(decoding: Self.command(), as: UTF8.self).replacingOccurrences(of: old, with: new).utf8)
            await #expect(throws: (any Error).self) { try await engine.deliver(header: Self.header, commands: bytes, retainedJournal: Data()) }
        }
        #expect(runtime.executions == 0 && runtime.lookups == 0)
        #expect(try store.read(.journal).isEmpty)
        let version = Data(String(decoding: Self.header, as: UTF8.self).replacingOccurrences(of: "\"protocol_version\":1", with: "\"protocol_version\":2").utf8)
        #expect(throws: AgentFailure.version) { try AgentHeader.read(version) }
        let headerObject = try #require(try JSONSerialization.jsonObject(with: Self.header) as? [String: Any])
        for key in ["executable_sha256", "build_receipt", "deployment_receipt", "fixture_delivery"] {
            var missing = headerObject
            missing.removeValue(forKey: key)
            let bytes = try JSONSerialization.data(withJSONObject: missing)
            #expect(throws: AgentFailure.shape) { try AgentHeader.read(bytes) }
        }
        var extra = headerObject
        extra["app_sha256"] = String(repeating: "b", count: 64)
        let extraBytes = try JSONSerialization.data(withJSONObject: extra)
        #expect(throws: AgentFailure.shape) { try AgentHeader.read(extraBytes) }
        for (old, new) in [
            ("\"executable_sha256\"", "\"app_sha256\""),
            (String(repeating: "b", count: 64), String(repeating: "B", count: 64)),
            ("\"job\":\"receipt-build-7\"", "\"job\":\"../receipt-build-7\""),
            ("\"job\":\"receipt-build-7\",", ""),
            ("\"job\":\"receipt-install-7\"", "\"job\":\"receipt-install-7\",\"unmodeled_receipt_flag\":true"),
            ("\"run_id\":\"20320405T060708Z-2\"", "\"run_id\":\"unbound-run\""),
            ("\"run_id\":\"20320405T060708Z-3\"", "\"run_id\":3"),
            ("\"job\":\"fixtures\"", "\"job\":\"other\""),
            ("\"source\":\"/fixture-root/suite7/input\"", "\"source\":\"../input\""),
            ("\"base\":\"small\",", ""),
            ("\"base\":\"small\"", "\"base\":\"small\",\"extra\":true"),
            ("\"base\":\"small\"", "\"base\":\"small\",\"preparation_receipt\":null"),
            ("\"base\":\"small\"", "\"base\":\"small\",\"preparation_receipt\":{\"job\":\"prepare\"}"),
            ("\"path\":\"suite7-index.json\"", "\"path\":\"../suite7-index.json\""),
            ("\"path\":\"suite7-index.json\"", "\"path\":\"/suite7-index.json\""),
            ("\"path\":\"suite7-index.json\",", ""),
            ("\"path\":\"suite7-index.json\"", "\"path\":\"suite7-index.json\",\"extra\":true"),
            ("\"path\":\"fixture-input.json\"", "\"path\":\"other.json\""),
            (String(repeating: "e", count: 64), String(repeating: "E", count: 64)),
            ("\"bytes\":10", "\"bytes\":-1"),
            ("\"bytes\":20", "\"bytes\":\"20\""),
            ("\"bytes\":20", "\"bytes\":20,\"bytes\":20")
        ] {
            let bytes = Data(String(decoding: Self.header, as: UTF8.self).replacingOccurrences(of: old, with: new).utf8)
            let expected: AgentFailure = new == "\"bytes\":20,\"bytes\":20" ? .json : .shape
            #expect(throws: expected) { try AgentHeader.read(bytes) }
        }
        let parsed = try AgentHeader.read(Self.header)
        #expect(parsed.executable_sha256 == String(repeating: "b", count: 64))
        #expect(parsed.build_receipt.job == "receipt-build-7" && parsed.deployment_receipt.job == "receipt-install-7")
        #expect(parsed.fixture_delivery.base == "small" && parsed.fixture_delivery.preparation_receipt == nil)
        let prepared = Data(String(decoding: Self.header, as: UTF8.self).replacingOccurrences(of: "\"base\":\"small\"",
            with: "\"base\":\"small\",\"preparation_receipt\":{\"job\":\"prepare\",\"run_id\":\"20320405T060708Z-5\"}").utf8)
        #expect(try AgentHeader.read(prepared).fixture_delivery.preparation_receipt?.job == "prepare")
    }
    @Test func retainedCallbackKeepsAcceptedBindingAndSequenceAfterCandidateRefusals() async throws {
        for refusal in 0..<5 {
            let (store, runtime, engine) = try harness()
            let accepted = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
            let callback = try #require(runtime.callback)
            switch refusal {
            case 0:
                await #expect(throws: AgentFailure.binding) {
                    try await engine.deliver(header: Data([32]) + Self.header, commands: Self.command(), retainedJournal: accepted)
                }
            case 1:
                await #expect(throws: AgentFailure.prefix) {
                    try await engine.deliver(header: Self.header, commands: Data([32]) + Self.command(), retainedJournal: accepted)
                }
            case 2:
                await #expect(throws: AgentFailure.prefix) {
                    try await engine.deliver(header: Self.header, commands: Data(), retainedJournal: accepted)
                }
            case 3:
                await #expect(throws: AgentFailure.sequence) {
                    try await engine.deliver(header: Self.header, commands: Self.command(2, action: "sync_now"), retainedJournal: accepted)
                }
            default:
                store.files[.journal] = AgentJSON.lines(accepted).complete[0] + Data("\n{}\n".utf8)
                await #expect(throws: AgentFailure.shape) {
                    try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
                }
                store.files[.journal] = accepted
            }
            #expect(try store.read(.journal) == accepted)
            try callback(.init(name: .engineCallback, engine: "engine1", operation: "op1"))
            let records = try store.records()
            let late = try #require(records.last)
            #expect(records.map(\.record_seq) == Array(1...records.count))
            #expect(late.kind == .observation && late.command_seq == 1)
            #expect(late.header_sha256 == AgentJSON.hash(Self.header))
            #expect(late.command_sha256 == AgentJSON.hash(Data(Self.command().dropLast())))
            #expect(try store.read(.journal).starts(with: accepted))
            #expect(try store.read(.commands) == Self.command())
            #expect(try store.read(.header) == Self.header)
            let afterCallback = try store.read(.journal)
            #expect(try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: afterCallback) == afterCallback)
            #expect(runtime.executions == 1)
        }
    }
    @Test func uncertainAppendPreservesExactFirstErrorDespiteCancelFailure() async throws {
        for duringCancellation in [false, true] {
            let (store, runtime, engine) = try harness()
            let first = AgentInjectedWriteError()
            store.failDone = !duringCancellation
            store.failObservationAfterFailure = duringCancellation
            store.appendError = first
            runtime.failExecution = duringCancellation
            runtime.joinError = AgentFailure.execution
            do {
                _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
                Issue.record("Expected the injected append error")
            } catch { #expect((error as? AgentInjectedWriteError) === first) }
            #expect(runtime.joins == 1 && runtime.workJoined)
            #expect((runtime.callbackFailure as? AgentInjectedWriteError) === first)
            let original = try store.read(.journal)
            let callback = try #require(runtime.callback)
            do {
                try callback(.init(name: .engineCallback))
                Issue.record("Expected the original callback error")
            } catch { #expect((error as? AgentInjectedWriteError) === first) }
            do {
                _ = try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
                Issue.record("Expected the original error on the sealed writer")
            } catch { #expect((error as? AgentInjectedWriteError) === first) }
            #expect(try store.read(.journal) == original)
            #expect(runtime.joins == 1 && runtime.executions == 1)
        }
    }
    @Test func partialFileAppendFencesLateCancelCallbackWithoutRepair() async throws {
        try await uncertainFileJournal(completeWrite: false)
    }
    @Test func completeFileAppendWithFailedFlushFencesLateCancelCallbackWithoutAcknowledgment() async throws {
        try await uncertainFileJournal(completeWrite: true)
    }
    private func uncertainFileJournal(completeWrite: Bool) async throws {
        let fixture = try AgentDirectoryFixture()
        defer { fixture.remove() }
        let journalURL = fixture.root.appendingPathComponent(AgentFile.journal.rawValue)
        var prefix = Data(), attempted = Data(), faultWrites = 0, failedFlushes = 0
        var terminalWritten = false
        let store = try AgentFileStorage(directory: fixture.root, hooks: .init(write: { fd, bytes, offset in
            let terminal = (try? AgentRecord.read(Data(bytes.dropLast())))?.kind == .done
            if terminal {
                faultWrites += 1
                if offset == 0 { prefix = (try? Data(contentsOf: journalURL)) ?? Data(); attempted = bytes }
                if !completeWrite {
                    if offset != 0 { return -1 }
                    return bytes.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, 13) }
                }
                terminalWritten = true
            }
            return bytes.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!.advanced(by: offset), $0.count - offset) }
        }, flush: { fd in
            if terminalWritten { failedFlushes += 1; return -1 }
            return fsync(fd)
        }))
        let runtime = AgentFakeRuntime(store: store)
        let engine = try AgentEngine(storage: store, runtime: runtime, instance: "file-process")
        await #expect(throws: AgentFailure.storage) {
            try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        }
        #expect(runtime.executions == 1 && runtime.joins == 1 && runtime.workJoined)
        #expect((runtime.callbackFailure as? AgentFailure) == .storage)
        #expect(!prefix.isEmpty && !attempted.isEmpty)
        let expected = prefix + (completeWrite ? attempted : Data(attempted.prefix(13)))
        #expect(try store.read(.journal) == expected)
        let lines = AgentJSON.lines(expected)
        if completeWrite {
            #expect(lines.suffix.isEmpty)
            #expect(try AgentRecord.read(#require(lines.complete.last)).kind == .done)
            #expect(faultWrites == 1 && failedFlushes == 1)
        } else {
            #expect(lines.suffix == Data(attempted.prefix(13)))
            #expect(try !lines.complete.map { try AgentRecord.read($0) }.contains { $0.kind == .done })
            #expect(faultWrites == 2 && failedFlushes == 0)
        }
        let callback = try #require(runtime.callback)
        #expect(throws: AgentFailure.storage) { try callback(.init(name: .engineCallback)) }
        await #expect(throws: AgentFailure.storage) {
            try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        }
        #expect(try store.read(.journal) == expected)
        #expect(runtime.joins == 1 && runtime.executions == 1)
        #expect(faultWrites == (completeWrite ? 1 : 2))
        #expect(failedFlushes == (completeWrite ? 1 : 0))
    }
    @Test func heldExchangeWaiterResumesWithFirstTypedFailureWithoutAnotherEvent() async throws {
        let events = AgentExchangeEvents<Int>()
        let waiting = AsyncStream<Void>.makeStream()
        var entered = waiting.stream.makeAsyncIterator()
        let held = Task { @MainActor in
            try await events.next(where: { $0 > 1 }, onWait: { waiting.continuation.yield(()) })
        }
        _ = await entered.next()
        events.fail(.execution)
        events.fail(.storage)
        await #expect(throws: AgentFailure.execution) { try await held.value }
        await #expect(throws: AgentFailure.execution) { try await events.next(where: { _ in true }) }
        waiting.continuation.finish()
    }
    @Test func requestedExchangeWaiterIgnoresMultipleOlderResultsUntilExactCompletionOrFailure() async throws {
        for fails in [false, true] {
            let events = AgentExchangeEvents<UInt64>()
            // Even the latest previous completion (13) is not enough to reject
            // older buffered automatic results (11 and 12).
            let olderExchanges: [UInt64] = [11, 12, 13]
            for older in olderExchanges { events.yield(older) }
            let waiting = AsyncStream<Void>.makeStream()
            var entered = waiting.stream.makeAsyncIterator()
            var completed = false
            let held = Task { @MainActor in
                defer { completed = true; waiting.continuation.finish() }
                return try await events.next(exchange: 21, identifiedBy: { $0 },
                    onWait: { waiting.continuation.yield(()) })
            }
            // Three buffered results are discarded; the fourth read is held.
            for _ in 0..<4 { _ = try #require(await entered.next()) }
            #expect(!completed)
            events.yield(14) // An older exchange can also finish after the wait.
            _ = try #require(await entered.next())
            #expect(!completed)
            if fails {
                events.fail(.execution)
                events.fail(.storage)
                await #expect(throws: AgentFailure.execution) { try await held.value }
            } else {
                events.yield(21)
                #expect(try await held.value == 21)
            }
            #expect(completed)
            waiting.continuation.finish()
        }
    }
    @Test func implementationMissingHasFailedDispositionRatherThanUserAction() async throws {
        let (store, runtime, engine) = try harness()
        runtime.productFailure = .implementationMissing
        await #expect(throws: AgentFailure.implementationMissing) {
            try await engine.deliver(header: Self.header, commands: Self.command(), retainedJournal: Data())
        }
        #expect(runtime.lookups == 0 && runtime.executions == 0 && runtime.databaseCalls == 0)
        #expect(try store.read(.journal).isEmpty)
        #expect(try store.read(.header).isEmpty)
        #expect(try store.read(.commands).isEmpty)
        #expect(AgentFailure.implementationMissing.disposition == .failed)
        #expect(AgentFailure.awaitingUser.disposition == .awaitingUser)
        #expect(AgentFailure.wrongContainer.disposition == .failed)
    }
    @Test func fileStorageRejectsSymlinkDirectoryAndNonregularTargets() throws {
        let fixture = try AgentDirectoryFixture()
        defer { fixture.remove() }
        let store = try AgentFileStorage(directory: fixture.root)
        let outside = fixture.parent.appendingPathComponent("outside")
        let sentinel = Data("fixture-bytes".utf8)
        try sentinel.write(to: outside)
        let target = fixture.root.appendingPathComponent(AgentFile.journal.rawValue)
        try FileManager.default.createSymbolicLink(at: target, withDestinationURL: outside)
        #expect(throws: AgentFailure.storage) { try store.read(.journal) }
        #expect(throws: AgentFailure.storage) { try store.appendJournal(Data([1])) }
        #expect(throws: AgentFailure.storage) { try store.replace(.journal, with: Data([2])) }
        #expect(throws: AgentFailure.storage) { try store.preserveAndRemoveSuffix(Data([1]), complete: Data()) }
        #expect(try Data(contentsOf: outside) == sentinel)
        try FileManager.default.removeItem(at: target)
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: false)
        #expect(throws: AgentFailure.storage) { try store.read(.journal) }
        #expect(throws: AgentFailure.storage) { try store.appendJournal(Data([1])) }
        #expect(throws: AgentFailure.storage) { try store.replace(.journal, with: Data([2])) }
        try FileManager.default.removeItem(at: target)
        try #require(mkfifo(target.path, 0o600) == 0)
        #expect(throws: AgentFailure.storage) { try store.read(.journal) }
        #expect(throws: AgentFailure.storage) { try store.appendJournal(Data([1])) }
        #expect(throws: AgentFailure.storage) { try store.replace(.journal, with: Data([2])) }
        let alias = fixture.parent.appendingPathComponent("alias")
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: fixture.root)
        #expect(throws: AgentFailure.storage) { try AgentFileStorage(directory: alias) }
        #expect(throws: AgentFailure.storage) { try AgentFileStorage(directory: alias.appendingPathComponent("child")) }
    }
    @Test func fileStoragePinsAdmittedDirectoryAcrossParentReplacement() throws {
        let fixture = try AgentDirectoryFixture()
        defer { fixture.remove() }
        let parent = fixture.parent.appendingPathComponent("area", isDirectory: true)
        let root = parent.appendingPathComponent("run", isDirectory: true)
        let redirected = fixture.parent.appendingPathComponent("redirected", isDirectory: true)
        let divertedRoot = redirected.appendingPathComponent("run", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: divertedRoot, withIntermediateDirectories: true)
        let store = try AgentFileStorage(directory: root)
        let sentinel = Data("redirected-file".utf8)
        try sentinel.write(to: divertedRoot.appendingPathComponent(AgentFile.journal.rawValue))
        let moved = fixture.parent.appendingPathComponent("moved", isDirectory: true)
        try FileManager.default.moveItem(at: parent, to: moved)
        try FileManager.default.createSymbolicLink(at: parent, withDestinationURL: redirected)
        try store.appendJournal(Data("record\npartial".utf8))
        try store.preserveAndRemoveSuffix(Data("partial".utf8), complete: Data("record\n".utf8))
        try store.replace(.status, with: Data("status".utf8))
        #expect(try store.read(.journal) == Data("record\n".utf8))
        #expect(try Data(contentsOf: moved.appendingPathComponent("run/steps.jsonl")) == Data("record\n".utf8))
        #expect(try Data(contentsOf: moved.appendingPathComponent("run/agent-status.json")) == Data("status".utf8))
        #expect(try Data(contentsOf: divertedRoot.appendingPathComponent(AgentFile.journal.rawValue)) == sentinel)
        #expect(!FileManager.default.fileExists(atPath: divertedRoot.appendingPathComponent(AgentFile.status.rawValue).path))
        let diagnostics = try FileManager.default.contentsOfDirectory(at: moved.appendingPathComponent("run"), includingPropertiesForKeys: nil).filter { $0.lastPathComponent.hasPrefix("incomplete-") }
        #expect(diagnostics.count == 1)
        #expect(try Data(contentsOf: #require(diagnostics.first)) == Data("partial".utf8))
    }
    @Test func fileStorageCompletesShortWritesAndFlushesFileAndDirectory() throws {
        let fixture = try AgentDirectoryFixture()
        defer { fixture.remove() }
        var writes = 0, flushedKinds: [mode_t] = []
        let store = try AgentFileStorage(directory: fixture.root, hooks: .init(write: { fd, bytes, offset in
            writes += 1
            return bytes.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!.advanced(by: offset), min(3, $0.count - offset)) }
        }, flush: { fd in
            var info = stat()
            guard fstat(fd, &info) == 0 else { return -1 }
            flushedKinds.append(info.st_mode & S_IFMT)
            return fsync(fd)
        }))
        let bytes = Data("short-write-fixture\n".utf8)
        try store.appendJournal(bytes)
        #expect(try store.read(.journal) == bytes)
        #expect(writes == (bytes.count + 2) / 3)
        #expect(flushedKinds == [mode_t(S_IFREG), mode_t(S_IFDIR)])
    }
    private func seedStarted(_ store: AgentMemoryStorage, command bytes: Data, sequence: Int) throws {
        let record = AgentRecord(header_sha256: AgentJSON.hash(Self.header), record_seq: try store.records().count + 1,
            command_seq: sequence, command_sha256: AgentJSON.hash(Data(bytes.dropLast())), agent_instance: "process1",
            observed_at: "2026-11-02T00:00:00Z", kind: .started, action: .addNote)
        var line = try JSONEncoder().encode(record); line.append(10)
        try store.appendJournal(line)
    }
}

@MainActor private final class AgentMemoryStorage: AgentStorage {
    var files: [AgentFile: Data] = [:]
    var diagnostics: [Data] = []
    var events: [String] = []
    var failDone = false
    var failObservationAfterFailure = false
    var appendError: (any Error) = AgentFailure.storage
    func read(_ file: AgentFile) throws -> Data { files[file] ?? Data() }
    func replace(_ file: AgentFile, with bytes: Data) throws { files[file] = bytes }
    func appendJournal(_ bytes: Data) throws {
        let record = try AgentRecord.read(Data(bytes.dropLast()))
        if failDone && record.kind == .done { throw appendError }
        if failObservationAfterFailure && record.kind == .observation {
            let prior = try records()
            if prior.contains(where: { $0.kind == .failed }) { throw appendError }
        }
        files[.journal, default: Data()].append(bytes)
        events.append("durable-" + record.kind.rawValue)
    }
    func preserveAndRemoveSuffix(_ suffix: Data, complete: Data) throws { diagnostics.append(suffix); files[.journal] = complete }
    func records() throws -> [AgentRecord] { try AgentJSON.lines(read(.journal)).complete.map { try AgentRecord.read($0) } }
}

@MainActor private final class AgentFakeRuntime: AgentRuntime {
    let store: any AgentStorage
    var executions = 0, lookups = 0, databaseCalls = 0, recoveries = 0, mutations = 0, joins = 0
    var accountMatches = true, failExecution = false, failRecovery = false, committed = false, workJoined = false
    var callback: ((AgentFact) throws -> Void)?
    var productFailure: AgentFailure?
    var callbackFailure: (any Error)?
    var joinError: (any Error)?
    init(store: any AgentStorage) { self.store = store }
    func verifyProduct(_ header: AgentHeader) throws {
        if let productFailure { throw productFailure }
        let actual = try AgentHeader.read(CloudKitAgentProtocolTests.header)
        if header.container != actual.container || header.environment != actual.environment || header.account_scope != actual.account_scope { throw AgentFailure.wrongContainer }
        guard header == actual else { throw AgentFailure.binding }
    }
    func verifyAccount(_ header: AgentHeader) async throws {
        #expect(try AgentJSON.lines(store.read(.journal)).complete.map { try AgentRecord.read($0) }.contains { $0.kind == .started })
        lookups += 1
        guard accountMatches else { throw AgentFailure.wrongContainer }
    }
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        executions += 1; callback = observe
        (store as? AgentMemoryStorage)?.events.append("bootstrap")
        try observe(.init(name: .engineCallback, engine: "engine1", operation: "op1"))
        if failExecution { throw NSError(domain: "SYNTHETIC_PRIVATE_ERROR", code: 7) }
        databaseCalls += 1
        if command.action == .addNote { mutations += 1; committed = true }
        return AgentEngine.requiredChecks(command.action).map { .init(name: $0, outcome: .pass, facts: [.init(name: .commandCompleted, count: 1)]) }
    }
    func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        recoveries += 1; callback = observe
        guard !failRecovery else { throw AgentFailure.recovery }
        let fact = AgentFact(name: committed ? .recoveryCommitted : .recoveryNotCommitted)
        try observe(fact)
        return AgentEngine.requiredChecks(command.action).union([.recovery, .integrity]).map {
            .init(name: $0, outcome: .pass, facts: [fact])
        }
    }
    func cancelAndJoin() async throws {
        joins += 1
        do { try callback?(.init(name: .engineCallback, engine: "engine1", operation: "op1")) }
        catch { callbackFailure = error }
        await Task.yield()
        workJoined = true
        if let joinError { throw joinError }
    }
}

private final class AgentInjectedWriteError: Error {}

@MainActor private struct AgentDirectoryFixture {
    let parent: URL
    let root: URL
    init() throws {
        // Canonicalize only the trusted OS fixture root, before creating owned paths.
        guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
        defer { free(resolved) }
        let temporaryRoot = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
        parent = temporaryRoot
            .appendingPathComponent("agent-fixture-" + UUID().uuidString, isDirectory: true)
        root = parent.appendingPathComponent("run", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
    }
    func remove() { try? FileManager.default.removeItem(at: parent) }
}
