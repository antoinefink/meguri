import Foundation

nonisolated struct AgentHeader: Codable, Equatable {
    struct Receipt: Codable, Equatable {
        let job: String
        let run_id: String

        static func validateShape(_ value: AgentJSON.Value?) throws {
            guard let value else { throw AgentFailure.shape }
            _ = try value.object(required: ["job", "run_id"])
        }
        static func validRunID(_ value: String) -> Bool {
            value.range(of: #"^[0-9]{8}T[0-9]{6}Z-[0-9]+$"#, options: .regularExpression) == (value.startIndex..<value.endIndex)
        }
        func validate() throws {
            guard AgentJSON.label(job), Self.validRunID(run_id) else { throw AgentFailure.shape }
        }
    }
    struct InputFile: Codable, Equatable {
        let path: String
        let sha256: String
        let bytes: UInt64

        static func validateShape(_ value: AgentJSON.Value?) throws {
            guard let value else { throw AgentFailure.shape }
            _ = try value.object(required: ["path", "sha256", "bytes"])
        }
        func validate() throws {
            guard FixtureDelivery.validPath(path, absolute: false), AgentJSON.digest(sha256) else { throw AgentFailure.shape }
        }
    }
    struct FixtureDelivery: Codable, Equatable {
        let base: String
        let fixtures_receipt: Receipt
        let source: String
        let manifest: InputFile
        let frozen_input: InputFile
        let preparation_receipt: Receipt?

        static func validateShape(_ value: AgentJSON.Value?) throws {
            guard let value else { throw AgentFailure.shape }
            let fields = try value.object(required: ["base", "fixtures_receipt", "source", "manifest", "frozen_input"], optional: ["preparation_receipt"])
            try Receipt.validateShape(fields["fixtures_receipt"])
            try InputFile.validateShape(fields["manifest"])
            try InputFile.validateShape(fields["frozen_input"])
            if let receipt = fields["preparation_receipt"] { try Receipt.validateShape(receipt) }
        }
        static func validPath(_ path: String, absolute: Bool) -> Bool {
            guard !path.isEmpty, !path.utf8.contains(0), path.hasPrefix("/") == absolute else { return false }
            let components = path.split(separator: "/", omittingEmptySubsequences: false)
            let relative = absolute ? components.dropFirst() : components[...]
            return !relative.isEmpty && relative.allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }
        }
        func validate() throws {
            try fixtures_receipt.validate()
            try preparation_receipt?.validate()
            try manifest.validate()
            try frozen_input.validate()
            guard fixtures_receipt.job == "fixtures", Self.validPath(source, absolute: true),
                  frozen_input.path == "fixture-input.json" else { throw AgentFailure.shape }
            guard base == "small" else { throw AgentFailure.unsupported }
        }
    }
    let protocol_version: Int
    let job: String
    let run_id: String
    let lane: String
    let source_commit: String
    let tree_clean: Bool
    let device: String
    let device_kind: String
    let os_build: String
    let app_build: String
    let executable_sha256: String
    let build_receipt: Receipt
    let build_receipt_sha256: String
    let deployment_receipt: Receipt
    let container: String
    let environment: String
    let account_scope: String
    let account_labels: [String]
    let selection: String
    let fixture_delivery: FixtureDelivery

    static func read(_ bytes: Data) throws -> Self {
        let fields = try AgentJSON.parse(bytes).object(required: ["protocol_version", "job", "run_id", "lane", "source_commit", "tree_clean", "device", "device_kind", "os_build", "app_build", "executable_sha256", "build_receipt", "build_receipt_sha256", "deployment_receipt", "container", "environment", "account_scope", "account_labels", "selection", "fixture_delivery"])
        try Receipt.validateShape(fields["build_receipt"])
        try Receipt.validateShape(fields["deployment_receipt"])
        try FixtureDelivery.validateShape(fields["fixture_delivery"])
        let header = try AgentJSON.decode(Self.self, bytes)
        guard header.protocol_version == 1 else { throw AgentFailure.version }
        guard header.job == "cloudkit-signed", header.lane == "cloud-signed", header.tree_clean,
              header.source_commit.utf8.count == 40,
              header.source_commit.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
              AgentJSON.digest(header.executable_sha256), AgentJSON.digest(header.build_receipt_sha256),
              Receipt.validRunID(header.run_id), AgentJSON.label(header.device), AgentJSON.label(header.account_scope),
              ["mac", "iphone", "ipad"].contains(header.device_kind),
              !header.os_build.isEmpty, !header.app_build.isEmpty, !header.container.isEmpty,
              header.account_labels == ["acct1"] else { throw AgentFailure.shape }
        _ = try header.program
        try header.build_receipt.validate()
        try header.deployment_receipt.validate()
        try header.fixture_delivery.validate()
        return header
    }
    var program: AgentProgram { get throws { try .init(selection: selection, deviceKind: device_kind) } }
}

// Selection is bound by the validated immutable header, never by command text.
nonisolated enum AgentProgram: Equatable {
    case smoke, syncNowMac, syncNowPhone

    init(selection: String, deviceKind: String) throws {
        switch (selection, deviceKind) {
        case ("smoke", "mac"), ("smoke", "iphone"), ("smoke", "ipad"): self = .smoke
        case ("sync_now", "mac"): self = .syncNowMac
        case ("sync_now", "iphone"): self = .syncNowPhone
        default: throw AgentFailure.unsupported
        }
    }
    var selection: String { self == .smoke ? "smoke" : "sync_now" }
    var actions: [AgentCommand.Action] {
        switch self {
        case .smoke: AgentCommand.smokeProgram
        case .syncNowMac: [.launch, .connect, .publish, .edit, .syncNow, .close]
        case .syncNowPhone: [.launch, .connect, .adopt, .syncNow, .observe, .close]
        }
    }
    func requirePosition(_ command: AgentCommand) throws {
        if self == .smoke { try command.requireSmokePosition(); return }
        guard actions.indices.contains(command.command_seq - 1),
              actions[command.command_seq - 1] == command.action else { throw AgentFailure.sequence }
        if let recovery = command.recovery_check {
            let permitted: String? = command.action == .syncNow ? "sync_now.exchange"
                : self == .syncNowMac && command.action == .edit ? "sync_now.edit" : nil
            guard recovery == permitted else { throw AgentFailure.unsupported }
        }
    }
}

nonisolated struct AgentCommand: Codable {
    enum Action: String, Codable, CaseIterable {
        case launch, connect, publish, addNote = "add_note", startSession = "start_session"
        case answer, endSession = "end_session", addMedia = "add_media", syncNow = "sync_now", close
        case adopt, edit, observe
        var arguments: [String: String] {
            switch self {
            case .launch, .syncNow, .close: [:]
            case .connect, .publish, .adopt: ["preview": "p1"]
            case .edit: ["note": "n1", "field": "Q"]
            case .observe: ["note": "n1"]
            case .addNote: ["note": "n1", "deck": "D2"]
            case .startSession: ["session": "s1", "deck": "D2", "card": "c1"]
            case .answer: ["session": "s1", "card": "c1", "rating": "good"]
            case .endSession: ["session": "s1"]
            case .addMedia: ["media": "m1"]
            }
        }
    }
    // The actual signed runtime enforces this complete J3.1 program. Protocol
    // transport doubles may exercise individual actions; they prove no SDK work.
    static let smokeProgram: [Action] = [.launch, .connect, .publish, .addNote, .startSession, .answer, .endSession, .addMedia, .syncNow, .close]
    func requireSmokePosition() throws {
        guard Self.smokeProgram.indices.contains(command_seq - 1),
              Self.smokeProgram[command_seq - 1] == action, recovery_check == nil else { throw AgentFailure.sequence }
    }
    let header_sha256: String
    let command_seq: Int
    let leg: String
    let action: Action
    let arguments: [String: String]
    let account_before: String
    let account_after: String
    let recovery_check: String?

    static func read(_ bytes: Data, header: String, sequence: Int, program: AgentProgram = .smoke) throws -> Self {
        _ = try AgentJSON.parse(bytes).object(required: ["header_sha256", "command_seq", "leg", "action", "arguments", "account_before", "account_after"], optional: ["recovery_check"])
        let command = try AgentJSON.decode(Self.self, bytes)
        guard command.header_sha256 == header else { throw AgentFailure.binding }
        guard command.command_seq == sequence else { throw AgentFailure.sequence }
        guard command.leg == program.selection, command.account_before == "acct1", command.account_after == "acct1" else { throw AgentFailure.unsupported }
        guard command.arguments == command.action.arguments else { throw AgentFailure.shape }
        if program == .smoke {
            // Preserve the transport-double grammar, but never admit J3.2
            // actions into smoke. The signed wrapper still enforces all ten positions.
            guard smokeProgram.contains(command.action) else { throw AgentFailure.unsupported }
            if let check = command.recovery_check, check != "smoke-durable-state" { throw AgentFailure.unsupported }
        } else { try program.requirePosition(command) }
        guard sequence == 1 ? command.action == .launch : command.action != .launch else { throw AgentFailure.sequence }
        return command
    }
}

nonisolated struct AgentFact: Codable, Equatable {
    enum Name: String, Codable {
        case accountVerified, coreOpened, localCommitted, localDurable, syncResult
        case recoveryCommitted, recoveryNotCommitted, integrity, ownedWorkJoined
        case engineCallback, stateSaved, applied, commandCompleted
    }
    enum Domain: String, Codable { case protocolError = "agent", core, cloudKit, cancellation }
    let name: Name
    var count: UInt64? = nil
    var digest: String? = nil
    var engine: String? = nil
    var operation: String? = nil
    var error_domain: Domain? = nil
    var error_code: Int? = nil

    func validate() throws {
        if let digest, !AgentJSON.digest(digest) { throw AgentFailure.shape }
        for label in [engine, operation].compactMap({ $0 }) where !AgentJSON.label(label) { throw AgentFailure.shape }
        guard (error_domain == nil) == (error_code == nil) else { throw AgentFailure.shape }
    }
    static let keys: Set<String> = ["name", "count", "digest", "engine", "operation", "error_domain", "error_code"]
}

nonisolated struct AgentCheck: Codable, Equatable {
    enum Name: String, Codable { case localDurability, coordinatorResult, recovery, integrity }
    enum Outcome: String, Codable { case pass, fail, open }
    let name: Name
    let outcome: Outcome
    let facts: [AgentFact]
}

nonisolated struct AgentRecord: Codable {
    enum Kind: String, Codable { case started, observation, done, failed }
    let header_sha256: String
    let record_seq: Int
    let command_seq: Int
    let command_sha256: String
    let agent_instance: String
    let observed_at: String
    let kind: Kind
    var action: AgentCommand.Action? = nil
    var account: String? = nil
    var facts: [AgentFact] = []
    var checks: [AgentCheck] = []
    var failure: AgentFailure? = nil
    var error_domain: AgentFact.Domain? = nil
    var error_code: Int? = nil
    var recovered_from: Int? = nil

    static func read(_ bytes: Data) throws -> Self {
        let fields = try AgentJSON.parse(bytes).object(required: ["header_sha256", "record_seq", "command_seq", "command_sha256", "agent_instance", "observed_at", "kind", "facts", "checks"], optional: ["action", "account", "failure", "error_domain", "error_code", "recovered_from"])
        func facts(_ value: AgentJSON.Value?) throws {
            guard let value, case .array(let items) = value else { throw AgentFailure.shape }
            for item in items { _ = try item.object(required: ["name"], optional: AgentFact.keys.subtracting(["name"])) }
        }
        try facts(fields["facts"])
        guard let checksValue = fields["checks"], case .array(let checks) = checksValue else { throw AgentFailure.shape }
        for item in checks {
            let check = try item.object(required: ["name", "outcome", "facts"])
            try facts(check["facts"])
        }
        let record = try AgentJSON.decode(Self.self, bytes)
        guard AgentJSON.label(record.agent_instance),
              ISO8601DateFormatter().date(from: record.observed_at) != nil,
              record.observed_at.hasSuffix("Z"),
              (record.error_domain == nil) == (record.error_code == nil),
              record.account == nil || record.account == "acct1" else { throw AgentFailure.shape }
        for fact in record.facts + record.checks.flatMap(\.facts) { try fact.validate() }
        switch record.kind {
        case .started:
            guard record.action != nil, record.account == nil, record.failure == nil, record.recovered_from == nil,
                  record.facts.isEmpty, record.checks.isEmpty, record.error_domain == nil else { throw AgentFailure.journal }
        case .observation:
            guard record.action == nil, record.account == nil, record.failure == nil, record.recovered_from == nil,
                  !record.facts.isEmpty, record.checks.isEmpty, record.error_domain == nil else { throw AgentFailure.journal }
        case .done:
            guard record.action == nil, record.account == nil, record.failure == nil, record.error_domain == nil,
                  !record.checks.isEmpty, record.checks.allSatisfy({ $0.outcome == .pass }) else { throw AgentFailure.journal }
        case .failed:
            guard record.action == nil, record.account == nil, record.failure != nil,
                  record.error_domain != nil, record.recovered_from == nil else { throw AgentFailure.journal }
        }
        return record
    }
}

typealias AgentObservationWriter = @MainActor (AgentFact) throws -> Void
typealias AgentCallbackJournal = @MainActor () throws -> AgentObservationWriter

@MainActor protocol AgentRuntime: AnyObject {
    func installCallbackJournal(_ journal: @escaping AgentCallbackJournal)
    // Must use independent product/source/device/signing observations. No
    // identity lookup or database operation is permitted in this method.
    func verifyProduct(_ header: AgentHeader) throws
    // Compare a current account observation with the protected run scope.
    func verifyAccount(_ header: AgentHeader) async throws
    // Called once for a fresh runtime, after exact prefix/product/account
    // admission, before any successor or interrupted-operation inspection.
    // Records select what must be proved; they are not proof of Core progress.
    func restore(_ state: AgentRestoration) async throws
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck]
    func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck]
    func cancelAndJoin() async throws
}

nonisolated struct AgentRestoration {
    let header: AgentHeader
    let commands: [AgentCommand]
    let records: [AgentRecord]
}

extension AgentRuntime {
    func installCallbackJournal(_ journal: @escaping AgentCallbackJournal) {}

    func restore(_ state: AgentRestoration) async throws {
        throw AgentFailure.implementationMissing
    }
}

// The held iterator must resume when execution fails, even without a record.
@MainActor final class AgentExchangeEvents<Value: Sendable> {
    private let events = AsyncStream<Value>.makeStream()
    private var failure: AgentFailure?

    func yield(_ value: Value) { if failure == nil { events.continuation.yield(value) } }
    func fail(_ reason: AgentFailure) {
        guard failure == nil else { return }
        failure = reason
        events.continuation.finish()
    }
    func finish() { events.continuation.finish() }
    func checkFailure() throws { if let failure { throw failure } }
    func next(exchange requested: UInt64, identifiedBy identity: (Value) -> UInt64,
              onWait: (() -> Void)? = nil) async throws -> Value {
        try await next(where: { identity($0) == requested }, onWait: onWait)
    }
    func next(where accepts: (Value) -> Bool, onWait: (() -> Void)? = nil) async throws -> Value {
        try checkFailure()
        var iterator = events.stream.makeAsyncIterator()
        while true {
            onWait?()
            guard let value = await iterator.next() else {
                try checkFailure()
                throw AgentFailure.execution
            }
            try checkFailure()
            if accepts(value) { return value }
        }
    }
}
