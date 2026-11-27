internal import MeguriCoreBindings
import Synchronization

/// One progress report of a core job. Rust sends at most about ten a second
/// (R1858); `CoreJob.progress` keeps only the newest.
public enum CoreProgressPhase: Sendable, Hashable {
    case planning
    case committing
    case named(String)
}
public enum MeguriRoundMilestone: String, Sendable, Hashable, Codable {
    case collectionStarted, serverAcknowledged, applyStarted, applyCompleted
    case mediaCheckStarted, mediaTransferStarted

    init(_ raw: MeguriCoreBindings.RoutineSyncMilestone) {
        self = switch raw {
        case .collectionStarted: .collectionStarted
        case .serverAcknowledged: .serverAcknowledged
        case .applyStarted: .applyStarted
        case .applyCompleted: .applyCompleted
        case .mediaCheckStarted: .mediaCheckStarted
        case .mediaTransferStarted: .mediaTransferStarted
        }
    }
}
public struct CoreProgress: Sendable, Hashable {
    let sequence: UInt64
    public let routineSync: [MeguriRoundMilestone]
    public let syncFilesLeft: UInt64?
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
        sequence = update.sequence
        routineSync = update.routineSync.map(MeguriRoundMilestone.init)
        syncFilesLeft = update.syncFilesLeft
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
    private let latestRoutineSequence = Mutex<UInt64?>(nil)

    init(_ continuation: AsyncStream<CoreProgress>.Continuation) {
        self.continuation = continuation
    }

    func onProgress(update: MeguriCoreBindings.ProgressUpdate) {
        latestRoutineSequence.withLock { latest in
            // Attachment replay can arrive after a newer worker callback.
            // Reject it before it can evict cumulative evidence from the buffer.
            // Other producers may publish typed facts at an unchanged sequence.
            if !update.routineSync.isEmpty || latest != nil {
                if let latest, update.sequence <= latest { return }
                latest = update.sequence
            }
            continuation.yield(CoreProgress(update))
        }
    }
}
