import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

/// Raw SDK error facts only. Core owns tracking and reset handling.
public struct CloudZoneDeletionFailure: Sendable, Equatable, Codable {
    public let code: Int64?
    public let retryAfterMilliseconds: Int64?
    public let encryptedDataReset: Bool
    public init(code: Int64?, retryAfterMilliseconds: Int64? = nil, encryptedDataReset: Bool = false) {
        self.code = code; self.retryAfterMilliseconds = retryAfterMilliseconds; self.encryptedDataReset = encryptedDataReset
    }
    init(_ raw: MeguriCoreBindings.CloudZoneDeletionFailure) {
        self.init(code: raw.code, retryAfterMilliseconds: raw.retryAfterMs, encryptedDataReset: raw.encryptedDataReset)
    }
    var raw: MeguriCoreBindings.CloudZoneDeletionFailure {
        .init(code: code, retryAfterMs: retryAfterMilliseconds, encryptedDataReset: encryptedDataReset)
    }
}
public struct CloudZoneDeletionResult: Sendable, Equatable {
    public let zone: String
    public let failure: CloudZoneDeletionFailure?
    public init(zone: String, failure: CloudZoneDeletionFailure?) { self.zone = zone; self.failure = failure }
}
public struct CloudZoneDeletionRead: Sendable {
    public let read: CloudControlRead
    public let failure: CloudZoneDeletionFailure?
    public let terminal: CloudZoneDeletionFailure?
    public init(read: CloudControlRead, failure: CloudZoneDeletionFailure? = nil, terminal: CloudZoneDeletionFailure? = nil) {
        self.read = read; self.failure = failure; self.terminal = terminal
    }
}
public struct CloudZoneDeletionReply: Sendable {
    public let results: [CloudZoneDeletionResult]
    public let terminal: CloudZoneDeletionFailure?
    public init(results: [CloudZoneDeletionResult], terminal: CloudZoneDeletionFailure? = nil) {
        self.results = results; self.terminal = terminal
    }
}
public struct CloudZoneDeletionBatch: Sendable {
    public let zones: [String]
    init(zones: [String]) { self.zones = zones }
}
public protocol CloudZoneDeletionQueue: Sendable { func enqueue() -> Bool }
private final class ForeignZoneDeletionQueue: MeguriCoreBindings.CloudZoneDeletionQueue {
    let queue: any CloudZoneDeletionQueue
    init(_ queue: any CloudZoneDeletionQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}
public struct CoreCloudZoneDeletionPass: Sendable {
    private let raw: MeguriCoreBindings.CloudZoneDeletionPass
    init(_ raw: MeguriCoreBindings.CloudZoneDeletionPass) { self.raw = raw }
    public func activeTarget() async throws -> CloudControlTarget {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudZoneDeletionPassActiveTarget) { CloudControlTarget(try raw.activeTarget()) }
    }
    public func submitRead(queue: any CloudZoneDeletionQueue) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudZoneDeletionPassSubmitRead) { try raw.submitRead(queue: ForeignZoneDeletionQueue(queue)) }
    }
    public func completeRead(_ result: CloudZoneDeletionRead) async throws -> CloudZoneDeletionBatch? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudZoneDeletionPassCompleteRead) {
            let zones = try raw.completeRead(read: result.read.raw, failure: result.failure?.raw, terminal: result.terminal?.raw)
            return zones.isEmpty ? nil : CloudZoneDeletionBatch(zones: zones)
        }
    }
    public func submitDeletion(queue: any CloudZoneDeletionQueue) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudZoneDeletionPassSubmitDeletion) { try raw.submitDeletion(queue: ForeignZoneDeletionQueue(queue)) }
    }
    public func completeDeletion(_ result: CloudZoneDeletionReply) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudZoneDeletionPassCompleteDeletion) {
            try raw.completeDeletion(results: result.results.map { .init(zone: $0.zone, failure: $0.failure?.raw) }, terminal: result.terminal?.raw)
        }
    }
    public func finish() async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudZoneDeletionPassFinish) { try raw.finish() }
    }
}
extension CoreCloudPolicyExecution {
    func beginZoneDeletionPass() async throws -> CoreCloudZoneDeletionPass? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginZoneDeletionPass) {
            try raw.beginZoneDeletionPass().map(CoreCloudZoneDeletionPass.init)
        }
    }
}

public enum CloudZoneDeletionDiagnostic: Sendable, Equatable {
    case read(found: Bool, allowed: UInt64, failure: CloudZoneDeletionFailure?, terminal: CloudZoneDeletionFailure?)
    case deleted(results: [CloudZoneDeletionResult], terminal: CloudZoneDeletionFailure?)
    case localFailure(category: CloudZoneDeletionLocalFailure, failure: CloudZoneDeletionFailure)
    init(_ raw: MeguriCoreBindings.CloudZoneDeletionDiagnostic) {
        self = switch raw {
        case .read(let found, let allowed, let failure, let terminal):
            .read(found: found, allowed: allowed, failure: failure.map(CloudZoneDeletionFailure.init), terminal: terminal.map(CloudZoneDeletionFailure.init))
        case .deleted(let results, let terminal):
            .deleted(results: results.map { .init(zone: $0.zone, failure: $0.failure.map(CloudZoneDeletionFailure.init)) }, terminal: terminal.map(CloudZoneDeletionFailure.init))
        }
    }
}

/// Fixed categories for failures that may precede any server callback.
public enum CloudZoneDeletionLocalFailure: String, Sendable, Codable {
    case capabilityUnavailable, preparation, admission, queueRejected, transport, invalidReply, completion, cancelled, closed, busy
}

/// Process-local evidence is capped independently of Core's callback history.
public enum CloudZoneDeletionDiagnostics {
    private static let local = Mutex<[CloudZoneDeletionDiagnostic]>([])
    public static func localFailures() -> [CloudZoneDeletionDiagnostic] { local.withLock { $0 } }
    static func record(_ category: CloudZoneDeletionLocalFailure, error: any Error) {
        let facts = CloudZoneDeletionFailure.evidence(error)
        let category = error is CancellationError ? CloudZoneDeletionLocalFailure.cancelled : category
        local.withLock {
            if $0.count == 64 { $0.removeFirst() }
            $0.append(.localFailure(category: category, failure: facts))
        }
    }
}

extension CloudZoneDeletionFailure {
    static func evidence(_ error: any Error) -> Self {
        #if canImport(CloudKit)
        return .native(error)
        #else
        return .init(code: nil)
        #endif
    }
}
