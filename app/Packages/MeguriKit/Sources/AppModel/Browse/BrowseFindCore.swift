import CoreBridge
import Foundation

// U277 over CoreBridge: Find and Replace's previews and run (`browser/find-replace`
// F10, F11; `browser/workspace` V9) and Find Duplicates (V10). Each takes the
// rows as index ranges of a handle at a revision, resolved to their notes in the
// core (H12), which refuses a revision the handle has moved past (K1). No note
// id crosses for a target.

public nonisolated extension CoreBrowseSearch {
    func findReplacePreviews() async throws -> any BrowseFindReplacePreviewing {
        CoreBrowseFindReplacePreviews(search: self, previews: try await CoreFindReplacePreviews.open())
    }

    func startFindReplace(_ request: FindReplaceRequest, on target: BrowseTarget, confirmed: UInt64) async throws
        -> BrowseRunning<FindReplaceResult> {
        let job = try await CoreFindReplace.start(request, on: try await selection(target), confirmed: confirmed)
        return Self.running(job.job) { try await job.value() }
    }

    func startFindDuplicates(field: String, on target: BrowseTarget?) async throws -> BrowseRunning<FindDuplicatesResult> {
        let found: BrowseSelection? = if let target { try await selection(target) } else { nil }
        let job = try await CoreBrowseQuery.startFindDuplicates(field: field, in: found)
        return Self.running(job.job) { try await job.value() }
    }

    /// A job's progress and Cancel, and its value.
    private static func running<Value: Sendable>(_ job: CoreJob, value: @escaping @Sendable () async throws -> Value)
        -> BrowseRunning<Value> {
        let progress = AsyncStream<BrowseActionProgress> { continuation in
            let task = Task {
                for await report in job.progress {
                    continuation.yield(BrowseActionProgress(completed: report.completed, total: report.total,
                                                            cancellable: report.cancellable))
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
        return BrowseRunning(progress: progress, cancel: { try? await job.cancel() }, result: value)
    }
}

/// One sheet's previews: each `preview` starts a job that supersedes the
/// one before it; cancelling the caller's task cancels its job.
nonisolated final class CoreBrowseFindReplacePreviews: BrowseFindReplacePreviewing {
    private let search: CoreBrowseSearch
    private let previews: CoreFindReplacePreviews

    init(search: CoreBrowseSearch, previews: CoreFindReplacePreviews) {
        self.search = search
        self.previews = previews
    }

    func preview(_ request: FindReplaceRequest, on target: BrowseTarget) async throws -> FindReplacePreview {
        let started = try await previews.start(request, on: try await search.selection(target))
        return try await withTaskCancellationHandler {
            try await started.value()
        } onCancel: {
            Task { try? await started.job.cancel() }
        }
    }
}
