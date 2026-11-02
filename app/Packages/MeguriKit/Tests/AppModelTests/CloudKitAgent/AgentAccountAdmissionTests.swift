import CoreBridge
import Darwin
import Foundation
import Testing

// Generated protocol callbacks only. None of these are signed/runtime evidence.
@MainActor @Suite(.serialized) struct AgentAccountAdmissionTests {
    private static let identity = Data([0, 21, 65, 255])
    private static var header: Data { CloudKitAgentProtocolTests.header }

    @MainActor private final class Fixture {
        let directory: URL
        let support: URL
        let privateDirectories: [URL]
        let file: URL
        let headerBytes: Data
        private(set) var productChecks = 0
        private(set) var rootAdmissions = 0
        var lookups = 0
        var operations = 0

        init(headerBytes: Data = AgentAccountAdmissionTests.header) throws {
            self.headerBytes = headerBytes
            // Only the trusted OS temporary root is canonicalized; all private
            // descendants are fixed, owned test paths, as in the reader tests.
            guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            directory = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
                .appendingPathComponent("meguri-account-admission-" + UUID().uuidString, isDirectory: true)
            support = directory.appendingPathComponent("Application Support", isDirectory: true)
            var path = support
            let header = try AgentHeader.read(headerBytes)
            privateDirectories = ["Meguri", "CloudKitSignedPrivate", header.run_id, header.device].map {
                path = path.appendingPathComponent($0, isDirectory: true)
                return path
            }
            file = path.appendingPathComponent("expected-accounts.json")
            guard mkdir(directory.path, 0o700) == 0 else { throw AgentFailure.storage }
            do {
                for url in [support] + privateDirectories {
                    guard mkdir(url.path, 0o700) == 0, chmod(url.path, 0o700) == 0 else { throw AgentFailure.storage }
                }
            } catch {
                try? FileManager.default.removeItem(at: directory)
                throw AgentFailure.storage
            }
        }
        func remove() { try? FileManager.default.removeItem(at: directory) }
        func writeMapping(boundTo bytes: Data? = nil, identity: Data = AgentAccountAdmissionTests.identity) throws {
            let original = bytes ?? headerBytes
            let header = try AgentHeader.read(original)
            let object: [String: Any] = [
                "protocol_version": 1, "header_sha256": AgentJSON.hash(original), "job": header.job,
                "run_id": header.run_id, "source_commit": header.source_commit, "device": header.device,
                "executable_sha256": header.executable_sha256, "container": header.container,
                "environment": header.environment, "account_scope": header.account_scope,
                "accounts": ["acct1": identity.map(Int.init)]
            ]
            try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]).write(to: file)
            guard chmod(file.path, 0o600) == 0 else { throw AgentFailure.storage }
        }
        func admit(fakeProductFailure: AgentFailure? = nil) throws -> AgentAccountAdmission {
            try AgentAccountAdmission(originalHeaderBytes: headerBytes, header: AgentHeader.read(headerBytes),
                productCheck: {
                    self.productChecks += 1
                    if let fakeProductFailure { throw fakeProductFailure }
                }, applicationSupportRoot: {
                    self.rootAdmissions += 1
                    return self.support
                })
        }
        func fakeRead(_ value: CloudResolvedAccount?) async -> CloudResolvedAccount? {
            lookups += 1
            return value
        }
        func fakeOperation(_ admission: AgentAccountAdmission) throws {
            // Agent-owned command admission; not an SDK packet/start interceptor.
            try admission.revalidate()
            operations += 1
        }
    }

    @MainActor private final class SuspendedFakeLookup {
        private var reply: CheckedContinuation<CloudResolvedAccount?, Never>?
        private var observer: CheckedContinuation<Void, Never>?
        func read() async -> CloudResolvedAccount? {
            await withCheckedContinuation { continuation in
                reply = continuation
                observer?.resume(); observer = nil
            }
        }
        func waitUntilSuspended() async {
            if reply == nil { await withCheckedContinuation { observer = $0 } }
        }
        func finish() {
            reply?.resume(returning: .init(status: .available, identity: AgentAccountAdmissionTests.identity))
            reply = nil
        }
    }
    private static var matching: CloudResolvedAccount { .init(status: .available, identity: identity) }

    // Generated callbacks exercise AgentEngine's existing lifecycle only.
    // This runtime supplies no signed product or live SDK acceptance evidence.
    @MainActor private final class ProtocolRuntime: AgentRuntime {
        let fixture: Fixture
        let admission: AgentAccountAdmission
        var lookup: SuspendedFakeLookup?
        var callback: ((AgentFact) throws -> Void)?
        private(set) var joins = 0
        private(set) var drained = false
        init(fixture: Fixture, admission: AgentAccountAdmission) {
            self.fixture = fixture; self.admission = admission
        }
        func verifyProduct(_ header: AgentHeader) throws {
            guard header == (try AgentHeader.read(fixture.headerBytes)) else { throw AgentFailure.binding }
        }
        func verifyAccount(_ header: AgentHeader) async throws {
            try admission.requireHeader(header)
            _ = try await admission.resolveAccount {
                if let lookup {
                    fixture.lookups += 1
                    return await lookup.read()
                }
                return await fixture.fakeRead(AgentAccountAdmissionTests.matching)
            }
        }
        func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
            try fixture.fakeOperation(admission)
            callback = observe
            return AgentEngine.requiredChecks(command.action).map {
                .init(name: $0, outcome: .pass, facts: [.init(name: .commandCompleted, count: 1)])
            }
        }
        func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
            throw AgentFailure.implementationMissing
        }
        func cancelAndJoin() async throws {
            joins += 1
            // The original owned callback can drain without account admission.
            try callback?(.init(name: .engineCallback, engine: "engine1", operation: "op1"))
            await Task.yield()
            drained = true
        }
    }

    @Test func fakeProductFailurePrecedesProtectedInputAndAllCallbacks() async throws {
        let fixture = try Fixture()
        defer { fixture.remove() }
        // No mapping exists; a missing verifier must still be the first failure.
        await #expect(throws: AgentFailure.implementationMissing) {
            let admission = try fixture.admit(fakeProductFailure: .implementationMissing)
            _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
            try fixture.fakeOperation(admission)
        }
        #expect(fixture.productChecks == 1 && fixture.rootAdmissions == 0)
        #expect(fixture.lookups == 0 && fixture.operations == 0)
    }

    @Test func missingApprovedMappingPrecedesEveryLookup() async throws {
        let fixture = try Fixture()
        defer { fixture.remove() }
        await #expect(throws: AgentFailure.awaitingUser) {
            let admission = try fixture.admit()
            _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
            try fixture.fakeOperation(admission)
        }
        #expect(fixture.productChecks == 1 && fixture.rootAdmissions == 1)
        #expect(fixture.lookups == 0 && fixture.operations == 0)
    }

    @Test func unknownMissingNonavailableAndMismatchedAccountsNeverStartWork() async throws {
        let values: [CloudResolvedAccount?] = [nil,
            .init(status: .available, identity: nil), .init(status: .available, identity: Data()),
            .init(status: .available, identity: Data([9, 8, 7])),
            .init(status: .noAccount, identity: nil), .init(status: .couldNotDetermine, identity: nil),
            .init(status: .temporarilyUnavailable, identity: nil), .init(status: .restricted, identity: Self.identity)]
        for value in values {
            let fixture = try Fixture()
            defer { fixture.remove() }
            try fixture.writeMapping()
            let admission = try fixture.admit()
            await #expect(throws: AgentFailure.wrongContainer) {
                _ = try await admission.resolveAccount { await fixture.fakeRead(value) }
                try fixture.fakeOperation(admission)
            }
            await #expect(throws: AgentFailure.wrongContainer) {
                _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
            }
            #expect(fixture.lookups == 1 && fixture.operations == 0)
        }
    }

    @Test func freshMatchingFakeObservationPermitsExactlyOneTestedCallback() async throws {
        let fixture = try Fixture()
        defer { fixture.remove() }
        try fixture.writeMapping()
        let admission = try fixture.admit()
        _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
        try fixture.fakeOperation(admission)
        #expect(fixture.productChecks == 1 && fixture.rootAdmissions == 1)
        #expect(fixture.lookups == 1 && fixture.operations == 1)
    }

    @Test func protectedMutationDuringSuspendedLookupCannotReturnAnAcceptedAccount() async throws {
        // Real content, authority and scope changes, not injected reader results.
        for mutation in 0..<4 {
            let fixture = try Fixture()
            defer { fixture.remove() }
            try fixture.writeMapping()
            let admission = try fixture.admit()
            let lookup = SuspendedFakeLookup()
            var accepted = 0
            let task = Task {
                _ = try await admission.resolveAccount {
                    fixture.lookups += 1
                    return await lookup.read()
                }
                accepted += 1
                try fixture.fakeOperation(admission)
            }
            await lookup.waitUntilSuspended()
            // Always resume/join the task, even if fixture mutation itself fails.
            do {
                switch mutation {
                case 0: try fixture.writeMapping(identity: Data([1, 2, 3]))
                case 1:
                    guard chmod(fixture.privateDirectories[0].path, 0o755) == 0 else { throw AgentFailure.storage }
                case 2:
                    let parent = fixture.privateDirectories[1]
                    let moved = parent.appendingPathExtension("moved")
                    try FileManager.default.moveItem(at: parent, to: moved)
                    guard symlink(moved.path, parent.path) == 0 else { throw AgentFailure.storage }
                default:
                    var bytes = try Data(contentsOf: fixture.file)
                    bytes = Data(String(decoding: bytes, as: UTF8.self)
                        .replacingOccurrences(of: "\"scope7\"", with: "\"changed-scope\"").utf8)
                    try bytes.write(to: fixture.file)
                }
            } catch {
                lookup.finish()
                _ = await task.result
                throw error
            }
            lookup.finish()
            let result = await task.result
            if case .failure(let error) = result { #expect(error as? AgentFailure == .storage) }
            else { Issue.record("Protected mutation admitted a fake lookup") }
            #expect(fixture.lookups == 1 && accepted == 0 && fixture.operations == 0)
        }
    }

    @Test func mutationBeforeLaterLookupOrCommandBlocksBothBoundaries() async throws {
        for mutation in 0..<3 {
            let fixture = try Fixture()
            defer { fixture.remove() }
            try fixture.writeMapping()
            let admission = try fixture.admit()
            _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
            if mutation == 0 { try fixture.writeMapping(identity: Data([4, 5, 6])) }
            else if mutation == 1 {
                guard chmod(fixture.file.path, 0o644) == 0 else { throw AgentFailure.storage }
            } else {
                let parent = fixture.privateDirectories.last!
                let moved = parent.appendingPathExtension("moved")
                try FileManager.default.moveItem(at: parent, to: moved)
                try FileManager.default.moveItem(at: moved, to: parent)
            }
            #expect(throws: AgentFailure.storage) { try fixture.fakeOperation(admission) }
            await #expect(throws: AgentFailure.storage) {
                _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
            }
            #expect(fixture.lookups == 1 && fixture.operations == 0)
        }
    }

    @Test func laterChangedObservedAccountDoesNotUsePriorSuccess() async throws {
        let fixture = try Fixture()
        defer { fixture.remove() }
        try fixture.writeMapping()
        let admission = try fixture.admit()
        _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
        try fixture.fakeOperation(admission)
        await #expect(throws: AgentFailure.wrongContainer) {
            _ = try await admission.resolveAccount {
                await fixture.fakeRead(.init(status: .available, identity: Data([8, 9])))
            }
            try fixture.fakeOperation(admission)
        }
        #expect(throws: AgentFailure.wrongContainer) { try fixture.fakeOperation(admission) }
        #expect(fixture.lookups == 2 && fixture.operations == 1)
    }

    @Test func originalHeaderBytesAreRetainedRatherThanReserialized() async throws {
        var original = Self.header
        original.append(contentsOf: [32, 10, 32])
        let fixture = try Fixture(headerBytes: original)
        defer { fixture.remove() }
        #expect(try AgentHeader.read(original) == AgentHeader.read(Self.header))
        try fixture.writeMapping()
        let admission = try fixture.admit()
        try admission.requireHeader(AgentHeader.read(Self.header))
        _ = try await admission.resolveAccount { await fixture.fakeRead(Self.matching) }
        try fixture.fakeOperation(admission)
        #expect(fixture.operations == 1)
        // A semantically equal header's hash cannot replace the retained bytes.
        try fixture.writeMapping(boundTo: Self.header)
        #expect(throws: AgentFailure.storage) { try fixture.fakeOperation(admission) }
        #expect(throws: AgentFailure.binding) { _ = try fixture.admit() }
    }

    @Test func observedProtectedChangeRejectsNextCommandAndDrainsOriginalCallback() async throws {
        let fixture = try Fixture()
        defer { fixture.remove() }
        try fixture.writeMapping()
        let admission = try fixture.admit()
        let storage = try AgentFileStorage(directory: fixture.directory)
        let runtime = ProtocolRuntime(fixture: fixture, admission: admission)
        let engine = try AgentEngine(storage: storage, runtime: runtime, instance: "generated-protocol")
        let first = CloudKitAgentProtocolTests.command()
        let retained = try await engine.deliver(header: fixture.headerBytes, commands: first, retainedJournal: Data())
        #expect(fixture.operations == 1 && fixture.lookups == 1 && runtime.joins == 0)

        let lookup = SuspendedFakeLookup()
        runtime.lookup = lookup
        let next = first + CloudKitAgentProtocolTests.command(2, action: "sync_now")
        let task = Task {
            try await engine.deliver(header: fixture.headerBytes, commands: next, retainedJournal: retained)
        }
        await lookup.waitUntilSuspended()
        do {
            try fixture.writeMapping(identity: Data([4, 5, 6]))
            #expect(throws: AgentFailure.storage) { try admission.revalidate() }
            // Restoring the input cannot restore this run's admission.
            try fixture.writeMapping()
        } catch {
            lookup.finish()
            _ = await task.result
            throw error
        }
        lookup.finish()
        let failed = try await task.value
        let records = try AgentJSON.lines(failed).complete.map { try AgentRecord.read($0) }
        let failure = try #require(records.firstIndex { $0.kind == .failed })
        let callback = try #require(records.lastIndex { $0.facts.first?.name == .engineCallback })
        #expect(records[failure].command_seq == 2 && records[failure].failure == .storage)
        #expect(callback > failure && records[callback].command_seq == 1)
        #expect(records.last?.facts.first?.name == .ownedWorkJoined)
        #expect(fixture.operations == 1 && fixture.lookups == 2 && runtime.joins == 1 && runtime.drained)
        #expect(throws: AgentFailure.storage) { try fixture.fakeOperation(admission) }
    }

    @Test func throwingFakeLookupHasOnlyFixedFailureAndCannotStartWork() async throws {
        enum FakeFailure: Error { case generated }
        let fixture = try Fixture()
        defer { fixture.remove() }
        try fixture.writeMapping()
        let admission = try fixture.admit()
        await #expect(throws: AgentFailure.execution) {
            _ = try await admission.resolveAccount { fixture.lookups += 1; throw FakeFailure.generated }
            try fixture.fakeOperation(admission)
        }
        #expect(fixture.lookups == 1 && fixture.operations == 0)
    }
}
