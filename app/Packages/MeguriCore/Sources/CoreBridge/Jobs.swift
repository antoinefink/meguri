internal import MeguriCoreBindings
import Instrumentation
import Synchronization

public struct JobID: Hashable, Sendable, CustomStringConvertible {
    public let rawValue: UInt64

    public init(rawValue: UInt64) {
        self.rawValue = rawValue
    }

    public var description: String { "job \(rawValue)" }
}

/// How a job ended, from its `JobFinished` event.
public enum JobOutcome: Sendable, Hashable {
    case succeeded
    case cancelled
    case failed(BridgeError)

    init(_ outcome: MeguriCoreBindings.JobOutcome) {
        switch outcome {
        case .succeeded: self = .succeeded
        case .cancelled: self = .cancelled
        case .failed(let error): self = .failed(BridgeError(error))
        }
    }
}

/// A running core job (KTD9): its id, its cancel token, its throttled
/// progress, and its outcome once the `JobFinished` event arrives.
public final class CoreJob: Sendable {
    public let id: JobID
    public let cancelToken: CancelToken
    /// The newest progress report (`bufferingNewest(1)`); finishes when the
    /// job does.
    public let progress: AsyncStream<CoreProgress>
    let completion: JobCompletion

    init(id: JobID, cancelToken: CancelToken, progress: AsyncStream<CoreProgress>, completion: JobCompletion) {
        self.id = id
        self.cancelToken = cancelToken
        self.progress = progress
        self.completion = completion
    }

    /// Cancels the job through its token. Cancelling the Swift task that
    /// awaits `outcome()` does not cancel the job.
    public func cancel() async throws(BridgeError) {
        try await cancelToken.cancel()
    }

    /// Waits for the job's `JobFinished` event.
    public func outcome() async -> JobOutcome {
        await completion.wait()
    }

    /// Waits for the job and throws unless it succeeded.
    public func value() async throws(BridgeError) {
        switch await outcome() {
        case .succeeded: return
        case .cancelled: throw .cancelled
        case .failed(let error): throw error
        }
    }
}

enum Jobs {
    /// Starts a job with a short `start_*` export on the maintenance lane and
    /// returns it at once. The event stream is started first, so the job's
    /// `JobFinished` event cannot be missed.
    static func start(
        _ name: BridgeCall,
        _ startExport: @escaping @Sendable () throws -> MeguriCoreBindings.JobHandle
    ) async throws(BridgeError) -> CoreJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(name) {
            let handle = try startExport()
            let (progress, continuation) = AsyncStream.makeStream(
                of: CoreProgress.self, bufferingPolicy: .bufferingNewest(1))
            try handle.progress.setListener(listener: ProgressRelay(continuation))
            let completion = JobCompletion(progress: continuation)
            let job = CoreJob(
                id: JobID(rawValue: handle.id), cancelToken: CancelToken(raw: handle.cancel),
                progress: progress, completion: completion)
            JobRegistry.shared.register(job.id, completion)
            return job
        }
    }
}

/// A one-shot result that any number of tasks can await.
final class JobCompletion: Sendable {
    private struct State {
        var outcome: JobOutcome?
        var waiters: [CheckedContinuation<JobOutcome, Never>] = []
    }

    private let state = Mutex(State())
    private let progress: AsyncStream<CoreProgress>.Continuation

    init(progress: AsyncStream<CoreProgress>.Continuation) {
        self.progress = progress
    }

    func resolve(_ outcome: JobOutcome) {
        let waiters = state.withLock { state -> [CheckedContinuation<JobOutcome, Never>] in
            guard state.outcome == nil else { return [] }
            state.outcome = outcome
            defer { state.waiters = [] }
            return state.waiters
        }
        progress.finish()
        for waiter in waiters { waiter.resume(returning: outcome) }
    }

    func wait() async -> JobOutcome {
        await withCheckedContinuation { continuation in
            let ready = state.withLock { state -> JobOutcome? in
                if let outcome = state.outcome { return outcome }
                state.waiters.append(continuation)
                return nil
            }
            if let ready { continuation.resume(returning: ready) }
        }
    }
}

/// Matches `JobFinished` events with started jobs. An event can arrive before
/// `Jobs.start` registers its job, so outcomes of unregistered ids are kept
/// until the job registers.
final class JobRegistry: Sendable {
    static let shared = JobRegistry()

    private struct State {
        var waiting: [JobID: JobCompletion] = [:]
        var unclaimed: [JobID: JobOutcome] = [:]
    }

    private let state = Mutex(State())

    func register(_ id: JobID, _ completion: JobCompletion) {
        let finished = state.withLock { state -> JobOutcome? in
            if let outcome = state.unclaimed.removeValue(forKey: id) { return outcome }
            state.waiting[id] = completion
            return nil
        }
        if let finished { completion.resolve(finished) }
    }

    func finish(_ id: JobID, _ outcome: JobOutcome) {
        let completion = state.withLock { state -> JobCompletion? in
            if let completion = state.waiting.removeValue(forKey: id) { return completion }
            state.unclaimed[id] = outcome
            return nil
        }
        completion?.resolve(outcome)
    }
}
