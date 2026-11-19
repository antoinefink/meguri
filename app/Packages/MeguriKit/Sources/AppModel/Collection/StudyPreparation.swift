import CoreBridge
import Foundation
import Observation

/// What study preparation reads from sync (`sched/app-coordination` F5.1).
/// Read-only: nothing here asks the service for anything.
public struct StudyPreparationSyncFacts: Equatable, Sendable {
    public enum Activity: Equatable, Sendable {
        /// Offline, waiting for a retry, or before the service's first report.
        case waiting
        /// A collection exchange runs.
        case exchanging
        /// No exchange runs. The instant is the end of the last collection
        /// exchange the service reports, when it reports one.
        case settled(endedMilliseconds: Int64?)
    }
    /// False with the service Off, Sync Automatically off or a service that
    /// waits on the person: no exchange is to be expected by itself.
    public var exchangeExpected: Bool
    public var activity: Activity
    public init(exchangeExpected: Bool, activity: Activity) {
        self.exchangeExpected = exchangeExpected; self.activity = activity
    }
    public static let none = Self(exchangeExpected: false, activity: .waiting)
}

/// Makes an open collection ready for study without asking
/// (`sched/fsrs-policy` A1, `sched/app-coordination` F1 and F5). The core
/// decides the status and owns the operation; this type only chooses when to
/// read and when to run. It holds no timer and makes no network call.
@MainActor @Observable
public final class StudyPreparation {
    public struct Job {
        public let cancel: @MainActor () async -> Void
        /// Returns when the job commits, with the amount of work its last
        /// progress report gave; throws when it ends without a commit.
        public let finish: @MainActor () async throws -> UInt64?
        public init(cancel: @escaping @MainActor () async -> Void, finish: @escaping @MainActor () async throws -> UInt64?) {
            self.cancel = cancel; self.finish = finish
        }
    }
    public struct Environment {
        public var status: @MainActor () async throws -> FsrsPolicyStatus
        public var start: @MainActor () async throws -> Job
        public var sessionOpen: @MainActor () -> Bool
        public var syncFacts: @MainActor () -> StudyPreparationSyncFacts
        public var nowMilliseconds: @MainActor () -> Int64
        public var record: FieldDiagnosticRecorder
        public var committed: @MainActor () -> Void
        public init(status: @escaping @MainActor () async throws -> FsrsPolicyStatus,
                    start: @escaping @MainActor () async throws -> Job,
                    sessionOpen: @escaping @MainActor () -> Bool,
                    syncFacts: @escaping @MainActor () -> StudyPreparationSyncFacts,
                    nowMilliseconds: @escaping @MainActor () -> Int64,
                    record: @escaping FieldDiagnosticRecorder = { _ in },
                    committed: @escaping @MainActor () -> Void = {}) {
            self.status = status; self.start = start; self.sessionOpen = sessionOpen; self.syncFacts = syncFacts
            self.nowMilliseconds = nowMilliseconds; self.record = record; self.committed = committed
        }
    }
    public enum Phase: Equatable, Sendable {
        case idle
        /// The status is not `ok` and the job waits for the service's next
        /// collection exchange, or for a study entry point (F5.1).
        case waitingForExchange
        case running
        /// The last job ended without a commit (F5.3).
        case failed
    }
    /// How a study entry point arrives.
    public enum Entry: Equatable, Sendable {
        /// A Study control. After a failed job it is refused without a new run.
        case study
        /// Try Again of F5.3.
        case tryAgain
        /// The core refused the session start as pending (F1 item 5, F4).
        case refusedByCore
    }
    /// Which rule scheduled a job, for its record (F5.4).
    enum Origin { case afterOpen, whileOpen }

    @ObservationIgnored public var environment: Environment?
    /// Fixture seam only: a harness that scripts every collection write turns
    /// this off, and the job then runs from a study entry point alone. No
    /// setting, launch option or command reaches it (F5).
    @ObservationIgnored var runsUnprompted = true
    /// False before an open and after a close: study cannot be prepared.
    public var available: Bool { opened }
    /// The last status read for the open collection; nil before the first read.
    public private(set) var status: FsrsPolicyStatus?
    public private(set) var phase: Phase = .idle
    public var running: Bool { phase == .running }

    private var opened = false
    private var epoch: UInt64 = 0
    private var jobTask: Task<Bool, Never>?
    private var retiredJob: Task<Bool, Never>?
    private var cancelJob: (@MainActor () async -> Void)?
    private var deferredForSession = false
    private var exchangeSeen = false
    private var exchangeMark: Int64 = 0

    public init() {}

    // MARK: Lifecycle

    /// After the collection opens and after `CollectionReplaced` (F1 items 1
    /// and 2). Waits for the status read only, never for the job.
    public func collectionOpened() async {
        retire()
        guard let environment else { return }
        opened = true
        // Only an exchange that ends from now on is evidence (F5.1).
        exchangeSeen = environment.syncFacts().activity == .exchanging
        exchangeMark = environment.nowMilliseconds() - 1
        await read(.afterOpen, mayWait: true)
    }

    /// The collection closed or was replaced: the job is asked to stop and
    /// nothing of it applies to the next collection (F5.2).
    public func collectionEnded() { retire() }

    /// Joins a job that `collectionEnded()` asked to stop.
    public func drain() async { _ = await retiredJob?.value; retiredJob = nil }

    /// Joins the running job, if any. For tests and for close.
    public func waitForJob() async { _ = await jobTask?.value }

    private func retire() {
        epoch &+= 1; opened = false
        if let cancelJob { Task { await cancelJob() } }
        if let jobTask { retiredJob = jobTask }
        jobTask = nil; cancelJob = nil
        status = nil; phase = .idle; deferredForSession = false
    }

    // MARK: Reads (F1)

    /// The service's published state changed. Finds the end of a collection
    /// exchange (F1 item 3) and the states that end the wait of F5.1.
    public func syncChanged() async {
        guard opened, let environment else { return }
        let facts = environment.syncFacts()
        let ended = exchangeEnded(facts)
        if phase == .waitingForExchange {
            if !facts.exchangeExpected { act(.afterOpen, mayWait: false) }
            else if ended { await read(.afterOpen, mayWait: false) }
        } else if ended {
            await read(.whileOpen, mayWait: false)
        }
    }

    /// A committed change set from a sync that touched the switch (F1 item 3).
    public func changesCommitted(_ changes: ChangeSet) async {
        guard opened, changes.source == .cloudIncoming || changes.source == .syncIncoming,
              changes.wholeCollection || changes.configKeys.contains(where: { $0.name == "fsrs" }) else { return }
        // During the wait this read may find `ok`; it never ends the wait early.
        if phase == .waitingForExchange { await read(.afterOpen, mayWait: true) }
        else { await read(.whileOpen, mayWait: false) }
    }

    /// A review session ended (F1 item 4, F3).
    public func sessionEnded() async {
        guard opened, deferredForSession else { return }
        deferredForSession = false
        await read(.whileOpen, mayWait: false)
    }

    // MARK: Study entry points (F4)

    /// True when study may start. Starts or joins the job when the status is
    /// not `ok`; never waits for the service.
    public func prepare(for entry: Entry) async -> Bool {
        guard opened, let environment else { return false }
        let expected = epoch
        if let jobTask { return await jobTask.value && expected == epoch }
        if entry == .refusedByCore || status == nil {
            guard let found = try? await environment.status(), expected == epoch else { return false }
            status = found
            if found == .ok, phase != .running { phase = .idle }
            if let jobTask { return await jobTask.value && expected == epoch }
        }
        if status == .ok { return true }
        if phase == .failed, entry == .study { return false }
        act(phase == .waitingForExchange ? .afterOpen : .whileOpen, mayWait: false, entryPoint: true)
        guard let jobTask else { return false }
        return await jobTask.value && expected == epoch
    }

    // MARK: Private

    private func exchangeEnded(_ facts: StudyPreparationSyncFacts) -> Bool {
        switch facts.activity {
        case .exchanging: exchangeSeen = true; return false
        // An exchange that stopped for the network is still to come (F5.1).
        case .waiting: exchangeSeen = false; return false
        case .settled(let ended):
            let seen = exchangeSeen
            exchangeSeen = false
            var newer = false
            if let ended, ended > exchangeMark { exchangeMark = ended; newer = true }
            return seen || newer
        }
    }

    private func read(_ origin: Origin, mayWait: Bool) async {
        guard let environment else { return }
        let expected = epoch
        // An unreadable status stays unknown: the next entry point reads again.
        guard let found = try? await environment.status(), expected == epoch else { return }
        status = found
        if found == .ok {
            deferredForSession = false
            if phase != .running { phase = .idle }
            return
        }
        act(origin, mayWait: mayWait)
    }

    private func act(_ origin: Origin, mayWait: Bool, entryPoint: Bool = false) {
        guard let environment, jobTask == nil, let cause = status, cause != .ok else { return }
        guard runsUnprompted || entryPoint else { return }
        // The session goes on; the job runs after it (F3).
        if environment.sessionOpen() { deferredForSession = true; return }
        if mayWait, environment.syncFacts().exchangeExpected { phase = .waitingForExchange; return }
        launch(cause, origin)
    }

    private func launch(_ cause: FsrsPolicyStatus, _ origin: Origin) {
        phase = .running
        let expected = epoch
        jobTask = Task { await self.run(cause: cause, origin: origin, expected: expected) }
    }

    private func run(cause: FsrsPolicyStatus, origin: Origin, expected: UInt64) async -> Bool {
        guard let environment else { return false }
        var computed: UInt64?
        var failure: (any Error)?
        do {
            let job = try await environment.start()
            if expected == epoch { cancelJob = job.cancel } else { await job.cancel() }
            computed = try await job.finish()
        } catch { failure = error }
        let current = expected == epoch
        if current { jobTask = nil; cancelJob = nil }
        environment.record(Self.observation(cause: cause, origin: origin, computed: computed, failure: failure, current: current))
        guard current else { return false }
        guard let failure else {
            status = .ok; phase = .idle; deferredForSession = false
            environment.committed()
            return true
        }
        if case BridgeError.sessionActive = failure {
            // A session started first; its end runs the job (F3).
            deferredForSession = true; phase = .idle
            return false
        }
        phase = .failed
        return false
    }

    /// One content-free entry per job (F5.4). The existing timeline vocabulary
    /// carries it: the operation is the status that caused the job (`activate`
    /// for `needsEnable`, `recover` for `needsReEnable`), the stage is the rule
    /// that scheduled it (`activation` after an open, `statusObserved` while
    /// the collection is open), the count is the work the job reported.
    static func observation(cause: FsrsPolicyStatus, origin: Origin, computed: UInt64?,
                            failure: (any Error)?, current: Bool) -> FieldDiagnosticObservation {
        let outcome: FieldDiagnosticObservation.Outcome
        var reason: FieldDiagnosticObservation.Failure?
        switch failure {
        case nil: outcome = .succeeded
        case BridgeError.cancelled?: outcome = .cancelled
        case BridgeError.sessionActive?: outcome = .refused
        case BridgeError.busy?, BridgeError.unavailable?: outcome = current ? .failed : .cancelled; reason = .unavailable
        default: outcome = current ? .failed : .cancelled; reason = .other
        }
        return .init(component: .scheduling, operation: cause == .needsReEnable ? .recover : .activate,
            stage: origin == .afterOpen ? .activation : .statusObserved, outcome: outcome,
            failure: outcome == .failed ? reason : nil, count: computed)
    }
}
