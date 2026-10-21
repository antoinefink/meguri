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
/// Cancellation is observed before engine orchestration starts. The current
/// U25/deletion APIs do not accept a token after entry; that contract remains
/// pending and callers must await completion before acting on cancellation.
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

public protocol UndoService: Sendable {
    func peekUndo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus
    func peekRedo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus
    func startUndo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoJob
    func startRedo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoJob
}
public actor CoreUndoService: UndoService {
    public init() {}
    public func peekUndo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus {
        try await InteractiveLane.shared.call(.peekUndo) { ReviewUndoStatus(try MeguriCoreBindings.peekUndo(scope: scope.raw)) }
    }
    public func peekRedo(scope: ReviewUndoScope) async throws(BridgeError) -> ReviewUndoStatus {
        try await InteractiveLane.shared.call(.peekRedo) { ReviewUndoStatus(try MeguriCoreBindings.peekRedo(scope: scope.raw)) }
    }
    public func startUndo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startUndo) {
            let raw = try MeguriCoreBindings.startUndo(scope: scope.raw)
            return CoreUndoJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func startRedo(scope: ReviewUndoScope) async throws(BridgeError) -> CoreUndoJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startRedo) {
            let raw = try MeguriCoreBindings.startRedo(scope: scope.raw)
            return CoreUndoJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
}
