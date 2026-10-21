import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum UndoResult: Sendable, Hashable {
    case global(summary: MutationSummary, step: ReviewStepInfo)
    case session(head: ReviewHead)
    init(_ raw: MeguriCoreBindings.UndoResult) {
        switch raw {
        case .global(let summary, let step): self = .global(summary: MutationSummary(summary), step: ReviewStepInfo(step))
        case .session(let head): self = .session(head: ReviewHead(head))
        }
    }
}
/// Planning and queued writes observe cancellation. Once admitted by the
/// writer, restoration/deletion completes and preserves its committed result.
public final class CoreUndoJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.UndoJob
    init(job: CoreJob, raw: MeguriCoreBindings.UndoJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> UndoResult {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.undoJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return UndoResult(result)
        }
    }
}

/// Pending review restoration is admitted atomically on the interactive lane;
/// committed history is always restored by a cancellable core job.
public enum CoreUndoStart: Sendable {
    case restored(head: ReviewHead)
    case job(CoreUndoJob)
    public func value() async throws(BridgeError) -> UndoResult {
        switch self {
        case .restored(let head): return .session(head: head)
        case .job(let job): return try await job.value()
        }
    }
}
public protocol UndoService: Sendable {
    func peekUndo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus
    func peekRedo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus
    func startUndo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoStart
    func startRedo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoStart
}
public actor CoreUndoService: UndoService {
    public init() {}
    public func peekUndo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus {
        try await InteractiveLane.shared.call(.peekUndo) { ReviewUndoStatus(try MeguriCoreBindings.peekUndo(scope: scope.raw)) }
    }
    public func peekRedo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus {
        try await InteractiveLane.shared.call(.peekRedo) { ReviewUndoStatus(try MeguriCoreBindings.peekRedo(scope: scope.raw)) }
    }
    public func startUndo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoStart {
        if let head = try await InteractiveLane.shared.call(.tryPendingReviewRestore, {
            try MeguriCoreBindings.tryPendingReviewRestore(scope: scope.raw, redo: false).map(ReviewHead.init)
        }) { return .restored(head: head) }
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startUndo) {
            let raw = try MeguriCoreBindings.startUndo(scope: scope.raw)
            return .job(CoreUndoJob(job: try Jobs.track(raw.handle), raw: raw.result))
        }
    }
    public func startRedo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoStart {
        if let head = try await InteractiveLane.shared.call(.tryPendingReviewRestore, {
            try MeguriCoreBindings.tryPendingReviewRestore(scope: scope.raw, redo: true).map(ReviewHead.init)
        }) { return .restored(head: head) }
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startRedo) {
            let raw = try MeguriCoreBindings.startRedo(scope: scope.raw)
            return .job(CoreUndoJob(job: try Jobs.track(raw.handle), raw: raw.result))
        }
    }
}
