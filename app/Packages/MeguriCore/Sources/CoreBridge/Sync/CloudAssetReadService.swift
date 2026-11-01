import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum CloudAssetReadKind: Sendable, Equatable {
    case retry, saveConflictFirst

    init(_ raw: MeguriCoreBindings.CloudAssetReadKind) {
        self = switch raw {
        case .retry: .retry
        case .saveConflictFirst: .saveConflictFirst
        }
    }
}

/// Only Rust-issued work carries selection authority. Metadata cannot create it.
public struct CloudAssetReadWork: Sendable {
    let raw: MeguriCoreBindings.CloudAssetReadWork
}

public struct CloudAssetReadIssuedWork: Sendable {
    public let handle: CloudAssetReadWork
    public let name: String
    public let zone: String
    public let kind: CloudAssetReadKind

    init(_ raw: MeguriCoreBindings.CloudAssetReadIssuedWork) {
        handle = .init(raw: raw.handle)
        name = raw.name
        zone = raw.zone
        kind = .init(raw.kind)
    }
}

public struct CloudAssetReadCompletion: Sendable, Equatable {
    /// Intake acceptance is not a WAL/ACK claim. Rust drives its normal writer
    /// and owns any Deferred disposition.
    public let intakeAccepted: Bool

    init(_ raw: MeguriCoreBindings.CloudAssetReadCompletion) {
        intakeAccepted = raw.intakeAccepted
    }
}

public enum CloudAssetReadResultFailure: Error, Sendable {
    case assetLeaseMismatch
}

/// Descriptive SDK facts only. Rust retains error classification authority.
public struct CloudAssetReadFailureFacts: Sendable, Equatable {
    public let code: Int64?
    public let retryAfterMilliseconds: Int64?
}

/// The actual SDK record plus ownership of its copied pa file. Inline records
/// need no lease; failed reads carry no record. Rust validates payload content.
public struct CloudAssetReadResult: Sendable {
    public let record: CloudRecord?
    public let failureFacts: CloudAssetReadFailureFacts?
    private let asset: CloudMediaStagedAsset?

    public init(record: CloudRecord, asset: CloudMediaStagedAsset? = nil) throws {
        guard record.assetURL?.path == asset?.url.path else {
            throw CloudAssetReadResultFailure.assetLeaseMismatch
        }
        self.record = record
        self.asset = asset
        failureFacts = nil
    }

    private init(failure: CloudAssetReadFailureFacts) {
        record = nil; asset = nil; failureFacts = failure
    }
    public static let failed = CloudAssetReadResult(failure: .init(code: nil, retryAfterMilliseconds: nil))

    /// Native transport construction, without retaining SDK errors or records.
    static func nativeFailure(code: Int64?, retryAfterMilliseconds: Int64?) -> Self {
        Self(failure: .init(code: code, retryAfterMilliseconds: retryAfterMilliseconds))
    }

    /// A raw path alone does not own a file. Retain the entire result through
    /// the awaited maintenance call, including errors and task cancellation.
    func withRaw<T: Sendable>(_ body: @Sendable (MeguriCoreBindings.CloudRecord?) async throws -> T) async rethrows -> T {
        defer { withExtendedLifetime(self) {} }
        return try await body(record?.raw)
    }
}

/// Enqueue an already prepared SDK operation synchronously. No await, UI,
/// provider read or core reentry is permitted inside this callback.
public protocol CloudAssetReadQueue: Sendable {
    func enqueue() -> Bool
}

final class ForeignAssetReadQueue: MeguriCoreBindings.CloudAssetReadQueue {
    private let queue: any CloudAssetReadQueue
    init(_ queue: any CloudAssetReadQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}

/// The retained Rust pass owns its original Host, Runtime and policy execution.
/// Every terminal call uses this exact handle, including after host replacement.
public struct CoreCloudAssetReadPass: Sendable {
    private let raw: MeguriCoreBindings.CloudAssetReadPass
    init(_ raw: MeguriCoreBindings.CloudAssetReadPass) { self.raw = raw }

    func reportActivity(started: Bool, failures: [CloudAssetReadFailureFacts] = [], failed: Bool = false, appCancelled: Bool = false) async throws -> [CloudPolicyOutput] {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudAssetReadPassOperationActivity) {
            try raw.operationActivity(facts: .init(started: started,
                failures: failures.map { .init(code: $0.code, retryAfterMs: $0.retryAfterMilliseconds) },
                failed: failed, appCancelled: appCancelled)).map(CloudPolicyOutput.init)
        }
    }

    public func next() async throws -> CloudAssetReadIssuedWork? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudAssetReadPassNext) {
            try raw.next().map(CloudAssetReadIssuedWork.init)
        }
    }

    /// False means no SDK reply exists. An error can follow accepted enqueue:
    /// the caller must retain/join the original SDK operation and complete its
    /// original work. This bridge never cancels or drops accepted operations.
    public func submit(_ work: CloudAssetReadWork, queue: any CloudAssetReadQueue) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudAssetReadPassSubmit) {
            try raw.submit(work: work.raw, queue: ForeignAssetReadQueue(queue))
        }
    }

    public func complete(_ work: CloudAssetReadWork, result: CloudAssetReadResult) async throws -> CloudAssetReadCompletion {
        let raw = self.raw
        return try await result.withRaw { record in
            try await MaintenanceLane.shared.call(.cloudAssetReadPassComplete) {
                CloudAssetReadCompletion(try raw.complete(work: work.raw, record: record))
            }
        }
    }

    /// False requires joining the accepted SDK operation and completing its
    /// original work; only an unstarted selection can be cancelled here.
    public func cancel(_ work: CloudAssetReadWork) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudAssetReadPassCancel) {
            try raw.cancel(work: work.raw)
        }
    }

    /// Stops selection. Rust preserves accepted operations and retry accounting.
    public func finish() async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudAssetReadPassFinish) { try raw.finish() }
    }
}
