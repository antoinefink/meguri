internal import MeguriCoreBindings

/// One progress report of a core job. Rust sends at most about ten a second
/// (R1858); `CoreJob.progress` keeps only the newest.
public enum CoreProgressPhase: Sendable, Hashable {
    case planning
    case committing
    case named(String)
}
public struct CoreProgress: Sendable, Hashable {
    public let phase: CoreProgressPhase
    public var cancellable: Bool { phase != .committing }
    public let etaMilliseconds: UInt64?
    public let job: JobID
    public let completed: UInt64
    /// `nil` while the amount of work is unknown.
    public let total: UInt64?

    public let collectionMediaPrepared: CollectionImportMedia?

    init(_ update: MeguriCoreBindings.ProgressUpdate) {
        switch update.phase {
        case .planning: phase = .planning
        case .committing: phase = .committing
        case .named(let name): phase = .named(name)
        }
        etaMilliseconds = update.etaMillis
        job = JobID(rawValue: update.jobId)
        completed = update.completed
        total = update.total
        collectionMediaPrepared = update.collectionMediaPrepared.map(CollectionImportMedia.init)
    }
}

/// The Swift side of a job's progress callback. It runs on the reporting job's
/// thread and only yields into the stream (KTD9).
final class ProgressRelay: MeguriCoreBindings.ProgressListener {
    private let continuation: AsyncStream<CoreProgress>.Continuation

    init(_ continuation: AsyncStream<CoreProgress>.Continuation) {
        self.continuation = continuation
    }

    func onProgress(update: MeguriCoreBindings.ProgressUpdate) {
        continuation.yield(CoreProgress(update))
    }
}
