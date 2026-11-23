import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

/// Deck package (`.apkg`) import below the sheet (`package/apkg-import`,
/// `package/transfer-sheets` X2). One session per file: `inspect`, then one
/// or more `apply` calls until one commits, then `close`.
///
/// The service never chooses the Meguri Sync path. With a ready Meguri Sync
/// binding the sheet passes `askStructure: true`, and runs an accepted
/// confirmation that shows `reasons` through
/// `CoreMeguriRoundService.importDeckPackage` (`sync/meguri-provider` MP7.1).
public protocol DeckPackageImportService: Sendable {
    /// `package/container` P18 step 1: the class the name gives, nil when
    /// the content decides. Pure.
    func classify(name: String) async throws(BridgeError) -> PackageFileClass?
    /// P18 for a file, read in place and read only under its security scope
    /// and a coordinated read (`package/file-intake` K3). Throws
    /// `DeckImportRefusal.unreadableFile` for a folder or a file that cannot
    /// be read.
    func classify(file: URL) async throws -> PackageFileClass
    /// I5: the option values the sheet starts from. A pure read of the open
    /// collection.
    func storedOptions() async throws(BridgeError) -> DeckImportOptions
    /// I2 to I4: the owned source copy, the checks and the facts, with the
    /// stages `extracting` and `analysing`. `value()` throws a
    /// `DeckImportRefusal`; nothing is left behind then, and the selected
    /// file is never changed. The file's security scope and coordinated read
    /// are held until the work ends.
    func inspect(file: URL) -> DeckImportTask<DeckImportSession>
    /// I14: the import as one undoable step "Import", with the stages
    /// `planning`, `media`, `backup` and `committing`. `value()` does not
    /// throw for a refusal: it answers `.refused`. With `askStructure` this
    /// call never commits a plan with a structural reason: it answers
    /// `.confirmationRequired` with the reasons (see the protocol's note).
    func apply(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation?,
               askStructure: Bool) -> DeckImportTask<DeckImportApplyOutcome>
    /// The committed import's result again, nil before a commit or after `close`.
    func result(of session: DeckImportSession) async throws(BridgeError) -> DeckImportResult?
    /// A page of one category of the committed import's log, in its order.
    /// Empty before a commit, after `close` and past the end.
    func logPage(_ session: DeckImportSession, category: DeckImportLogCategory, offset: UInt64,
                 count: UInt32) async throws(BridgeError) -> [DeckImportLogEntry]
    /// Ends the session and removes its staging directory. Idempotent.
    func close(_ session: DeckImportSession) async
}

public final class CoreDeckPackageImportService: DeckPackageImportService, Sendable {
    private let source: CoordinatedSourceOwner
    public init(source: CoordinatedSourceOwner = CoordinatedSourceOwner()) { self.source = source }

    public func classify(name: String) async throws(BridgeError) -> PackageFileClass? {
        try await QueryLane.shared.call(.packageFileClassOfName) {
            try packageFileClassOfName(name: name).map(PackageFileClass.init)
        }
    }

    public func classify(file: URL) async throws -> PackageFileClass {
        let name = file.lastPathComponent
        do {
            return try await source.withReadablePackage(at: file) { coordinated in
                // K3 step 3: only a regular file is classified.
                guard (try? coordinated.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile == true else {
                    throw DeckImportRefusal.unreadableFile
                }
                return try await QueryLane.shared.call(.packageFileClass) {
                    PackageFileClass(try packageFileClass(path: coordinated.path, name: name))
                }
            }
        } catch let error as BridgeError { throw error
        } catch { throw DeckImportRefusal.unreadableFile }
    }

    public func storedOptions() async throws(BridgeError) -> DeckImportOptions {
        try await QueryLane.shared.call(.deckImportStoredOptions) { DeckImportOptions(try deckImportStoredOptions()) }
    }

    public func inspect(file: URL) -> DeckImportTask<DeckImportSession> {
        let source = source
        let name = file.lastPathComponent
        return DeckImportTask { progress in
            // The accessor's consumer runs in a task of its own, which a
            // cancel of this one does not reach: the request is carried here.
            let cancellation = DeckImportCancellation()
            do {
                // Subscribed before the job can end.
                try await CoreEventStream.shared.start()
                return try await withTaskCancellationHandler {
                  try await source.withReadablePackage(at: file) { coordinated in
                    if cancellation.requested { throw DeckImportRefusal.cancelled }
                    let started = try await MaintenanceLane.shared.call(.startDeckImportInspect) {
                        try startDeckImportInspect(path: coordinated.path, name: name)
                    }
                    await Self.drain(started.handle, progress: progress, cancellation: cancellation)
                    let outcome = try await MaintenanceLane.shared.call(.deckImportInspectJobOutcome) {
                        try started.result.outcome()
                    }
                    switch outcome {
                    case .inspected(let raw, let facts):
                        let session = DeckImportSession(raw: raw, facts: DeckImportFacts(facts), fileName: name)
                        // A caller that cancelled takes no session: it is closed here.
                        if cancellation.requested {
                            try? await MaintenanceLane.shared.call(.deckImportSessionClose) { try raw.close() }
                            throw DeckImportRefusal.cancelled
                        }
                        return session
                    case .refused(let refusal): throw DeckImportRefusal(refusal)
                    case nil: throw DeckImportRefusal.engine(.internal("Inspect ended without an outcome"))
                    }
                  }
                } onCancel: { cancellation.cancel() }
            } catch let refusal as DeckImportRefusal { throw refusal
            } catch let error as BridgeError { throw DeckImportRefusal(error)
            } catch is CancellationError { throw DeckImportRefusal.cancelled
            } catch { throw DeckImportRefusal.unreadableFile }
        }
    }

    public func apply(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation?,
                      askStructure: Bool) -> DeckImportTask<DeckImportApplyOutcome> {
        DeckImportTask { progress in
            guard let raw = session.raw else { return .refused(.sessionClosed) }
            let token = confirmation?.raw
            let cancellation = DeckImportCancellation()
            return await withTaskCancellationHandler {
              do throws(BridgeError) {
                try await CoreEventStream.shared.start()
                let started = try await MaintenanceLane.shared.call(.deckImportSessionStartApply) {
                    try raw.startApply(options: options.raw, confirmation: token, askStructure: askStructure)
                }
                await Self.drain(started.handle, progress: progress, cancellation: cancellation)
                if let outcome = try await MaintenanceLane.shared.call(.deckImportApplyJobOutcome, { try started.result.outcome() }) {
                    return DeckImportApplyOutcome(outcome)
                }
                // The job ended without its answer. The session knows whether it committed.
                if let result = try await MaintenanceLane.shared.call(.deckImportSessionResult, { try raw.result() }) {
                    return .imported(DeckImportResult(result))
                }
                return .refused(.engine(.internal("Apply ended without an outcome")))
              } catch {
                // A failed call around the job says nothing about the commit: the session does.
                if let result = try? await MaintenanceLane.shared.call(.deckImportSessionResult, { try raw.result() }) {
                    return .imported(DeckImportResult(result))
                }
                return .refused(DeckImportRefusal(error))
              }
            } onCancel: { cancellation.cancel() }
        }
    }

    public func result(of session: DeckImportSession) async throws(BridgeError) -> DeckImportResult? {
        guard let raw = session.raw else { return nil }
        return try await MaintenanceLane.shared.call(.deckImportSessionResult) { try raw.result().map(DeckImportResult.init) }
    }

    public func logPage(_ session: DeckImportSession, category: DeckImportLogCategory, offset: UInt64,
                        count: UInt32) async throws(BridgeError) -> [DeckImportLogEntry] {
        guard let raw = session.raw else { return [] }
        return try await QueryLane.shared.call(.deckImportSessionLogPage) {
            try raw.logPage(category: category.raw, offset: offset, count: count).map {
                DeckImportLogEntry(noteId: $0.noteId, fields: $0.fields, existingId: $0.existingId)
            }
        }
    }

    public func close(_ session: DeckImportSession) async {
        guard let raw = session.raw else { return }
        try? await MaintenanceLane.shared.call(.deckImportSessionClose) { try raw.close() }
    }

    /// Waits for the job's end, relaying its progress. A cancel request
    /// reaches the job through its token; the job still ends with its own
    /// outcome.
    static func drain(_ handle: MeguriCoreBindings.JobHandle, progress: @escaping @Sendable (DeckImportProgress) -> Void,
                      cancellation: DeckImportCancellation) async {
        // A job whose listener could not be set has already drained.
        guard case .tracked(let job) = await Jobs.trackRetainingDrain(handle) else { return }
        cancellation.attach(job)
        let updates = Task { for await update in job.progress { progress(DeckImportProgress(update)) } }
        _ = await job.outcome()
        await updates.value
    }
}

/// A cancel request remembered until the job exists, then sent to it. No
/// lock is held across a call into the core.
final class DeckImportCancellation: Sendable {
    private struct State { var requested = false; var job: CoreJob? }
    private let state = Mutex(State())
    var requested: Bool { state.withLock { $0.requested } }
    func cancel() {
        let job = state.withLock { state -> CoreJob? in state.requested = true; return state.job }
        if let job { Task { try? await job.cancel() } }
    }
    func attach(_ job: CoreJob) {
        let cancel = state.withLock { state -> Bool in state.job = job; return state.requested }
        if cancel { Task { try? await job.cancel() } }
    }
}

/// What `importDeckPackage` answered (`sync/meguri-provider` MP7.1).
public enum MeguriDeckImportOutcome: Sendable {
    /// The job did not start: nothing was imported. `.notReady` means that
    /// Meguri Sync is no longer the ready service: the import runs alone
    /// (`DeckPackageImportService.apply` with `askStructure: false`), after
    /// the confirmation is shown again without its sync paragraph when it
    /// still has rows.
    case notStarted(MeguriStructureRefusal)
    /// Nothing was imported: the round found that the server requires a
    /// full sync (MP6.3's collection choice). The session stays open.
    case fullSyncRequired
    /// Nothing was imported: the round failed, was cancelled or refused, or
    /// lost its acknowledgement.
    case roundStopped(MeguriRoundPhaseResult)
    /// Nothing was imported: cancelled before the import.
    case cancelled
    /// Nothing was imported: the job's record could not be written.
    case unavailable
    /// Nothing was imported and nothing is pending: the plan changed since
    /// the confirmation (`.confirmationRequired`, shown again; accepting it
    /// starts a new job), or the import was refused (`.refused`).
    case notImported(DeckImportApplyOutcome)
    /// The import is committed. `upload` says what became of the upload:
    /// `.notNeeded` when the plan carried no structural reason after the
    /// round (an ordinary round sends the import), `.uploaded`, or
    /// `.pending` with the needs-you line and Upload… (a review session
    /// open, the server unreachable, a cancel, `mediaPending`).
    case imported(DeckImportResult, upload: MeguriStructureUpload)
    /// The import is committed and its result cannot be shown. `upload` is
    /// nil when the job could not say what became of it: the sync status
    /// shows the needs-you line while one is pending. Never say that the
    /// collection is unchanged, never offer to import again.
    case importedWithoutResult(upload: MeguriStructureUpload?)
}

extension CoreMeguriRoundService {
    /// MP7.1's job for an accepted import confirmation that showed the sync
    /// paragraph: one normal round, the import with what the confirmation
    /// covers, the upload. `reachable` false is the unreachable case: no
    /// round, the import, and the upload left pending.
    ///
    /// Cancelling the task asks the core to stop where MP7 allows: before
    /// the import nothing is imported; after it the upload is left pending.
    public func importDeckPackage(
        _ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation, reachable: Bool,
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void = { _, _ in }
    ) async throws -> MeguriDeckImportOutcome {
        guard let raw = session.raw else { return .notImported(.refused(.sessionClosed)) }
        let drains = drains
        let (connection, credentials) = try await admittedInputs()
        let op = MeguriCoreBindings.ProviderStructureOp.import(session: raw, options: options.raw,
                                                               confirmation: confirmation.raw)
        return try await owned(since: drains, busy: .notStarted(.busy)) {
            try await CoreEventStream.shared.start()
            let started: MeguriCoreBindings.ProviderStructureStart
            do throws(BridgeError) {
                started = try await MaintenanceLane.shared.call(.providerStartStructureSave) {
                    try connection.startStructureSave(credentials: credentials, op: op, reachable: reachable)
                }
            } catch {
                // Nothing started: the host changed, or the call was refused.
                return .notImported(.refused(DeckImportRefusal(error)))
            }
            return await Self.driveImport(started, session: raw, onStage: onStage)
        }
    }

    /// Advances the job until its outcome, as `drive` does for a save.
    private static func driveImport(
        _ started: MeguriCoreBindings.ProviderStructureStart, session: MeguriCoreBindings.DeckImportSession,
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void
    ) async -> MeguriDeckImportOutcome {
        // Whether the session committed, whatever the job could report.
        @Sendable func committed() async -> DeckImportResult?? {
            do throws(BridgeError) {
                let read = try await MaintenanceLane.shared.call(.deckImportSessionResult) { try session.result() }
                return .some(read.map(DeckImportResult.init))
            } catch { return nil }
        }
        let job: MeguriCoreBindings.ProviderStructureJob
        switch started {
        case .refused(.importSessionClosed): return .notImported(.refused(.sessionClosed))
        case .refused(let refusal): return .notStarted(MeguriStructureRefusal(refusal))
        case .started(let value): job = value
        }
        var forwarded = false
        while true {
            if Task.isCancelled, !forwarded {
                forwarded = true
                try? await MaintenanceLane.shared.call(.providerStructureCancel) { try job.cancel() }
            }
            let step: MeguriCoreBindings.ProviderStructureStep
            do throws(BridgeError) {
                step = try await MaintenanceLane.shared.call(.providerStructureAdvance) { try job.advance() }
            } catch {
                // The op's own refusal before the apply answered (a changed
                // generation, a job that could not start), or a job whose
                // owner changed after the import: the session says which.
                switch await committed() {
                case .some(.some(let result)): return .imported(result, upload: .pending(.failed))
                case .some(.none): return .notImported(.refused(DeckImportRefusal(error)))
                case .none: return .importedWithoutResult(upload: nil)
                }
            }
            switch step {
            case .running(let stage, let handle):
                if case .tracked(let phase) = await Jobs.trackRetainingDrain(handle) {
                    onStage(MeguriStructureStage(stage), phase)
                    _ = await withTaskCancellationHandler { await phase.outcome() } onCancel: {
                        Task { try? await MaintenanceLane.shared.call(.providerStructureCancel) { try job.cancel() } }
                    }
                }
            case .finished(let outcome):
                let answer = try? await MaintenanceLane.shared.call(.providerStructureImportOutcome) {
                    try job.importOutcome().map(DeckImportApplyOutcome.init)
                }
                switch outcome {
                case .fullSyncRequired: return .fullSyncRequired
                case .cancelled: return .cancelled
                case .unavailable: return .unavailable
                case .roundStopped(let round):
                    guard let phase = try? await MaintenanceLane.shared.call(.providerRoundOutcome, { try round.outcome() }) else {
                        return .unavailable
                    }
                    return .roundStopped(.init(outcome: MeguriRoundPhaseOutcome(phase),
                        finalization: MeguriRoundFinalization(round), trackingFailure: nil))
                case .importNotCommitted:
                    guard let answer else { return .unavailable }
                    if case .refused(.committedWithoutResult) = answer { return .importedWithoutResult(upload: nil) }
                    return .notImported(answer)
                case .saved(let upload):
                    // The rows are committed, with or without the result.
                    if case .imported(let result)? = answer {
                        return .imported(result, upload: MeguriStructureUpload(upload))
                    }
                    if case .some(.some(let result)) = await committed() {
                        return .imported(result, upload: MeguriStructureUpload(upload))
                    }
                    return .importedWithoutResult(upload: MeguriStructureUpload(upload))
                }
            }
        }
    }
}

/// P18 step 1, for the fake: the same rule as the core's, by name alone.
private func fakeClass(of name: String) -> PackageFileClass? {
    let name = name.lowercased()
    if name.hasSuffix(".colpkg") || name == "collection.apkg" || (name.hasPrefix("backup-") && name.hasSuffix(".apkg")) {
        return .collectionPackage
    }
    return name.hasSuffix(".apkg") || name.hasSuffix(".zip") ? .deckPackage : nil
}

/// A scripted import for UI tests and previews: no core, no file access.
/// Each `apply` takes the next scripted answer; the last one repeats.
public final class FakeDeckPackageImportService: DeckPackageImportService, Sendable {
    public struct Script: Sendable {
        public var storedOptions = DeckImportOptions()
        /// What `inspect` answers.
        public var inspect: Result<DeckImportFacts, DeckImportRefusal>
        /// What each `apply` answers, in order.
        public var applies: [DeckImportApplyOutcome]
        public var log: [DeckImportLogCategory: [DeckImportLogEntry]] = [:]
        /// The stages each call reports before it answers.
        public var inspectStages: [DeckImportProgress] = [.init(stage: .extracting), .init(stage: .analysing)]
        public var applyStages: [DeckImportProgress] = [.init(stage: .planning), .init(stage: .committing)]
        /// When set, every call waits here after its stages until the test
        /// lets it go; a cancel while it waits answers `cancelled`.
        public var hold: (@Sendable () async -> Void)?
        public init(inspect: Result<DeckImportFacts, DeckImportRefusal>, applies: [DeckImportApplyOutcome]) {
            self.inspect = inspect; self.applies = applies
        }
    }
    /// One recorded `apply`.
    public struct Applied: Sendable {
        public let session: UInt64
        public let options: DeckImportOptions
        public let confirmation: UInt64?
        public let askStructure: Bool
    }
    private struct State {
        var script: Script
        var next: UInt64 = 1
        var open: Set<UInt64> = []
        var committed: [UInt64: DeckImportResult] = [:]
        var applied: [Applied] = []
        var answered = 0
    }
    private let state: Mutex<State>
    public init(_ script: Script) { state = Mutex(State(script: script)) }
    /// Every `apply` made, in order.
    public var applied: [Applied] { state.withLock { $0.applied } }
    /// The sessions that are inspected and not closed.
    public var openSessions: Int { state.withLock { $0.open.count } }

    public func classify(name: String) async throws(BridgeError) -> PackageFileClass? { fakeClass(of: name) }
    public func classify(file: URL) async throws -> PackageFileClass { fakeClass(of: file.lastPathComponent) ?? .unsupported }
    public func storedOptions() async throws(BridgeError) -> DeckImportOptions { state.withLock { $0.script.storedOptions } }

    public func inspect(file: URL) -> DeckImportTask<DeckImportSession> {
        let (script, name) = (state.withLock { $0.script }, file.lastPathComponent)
        return DeckImportTask { [self] progress in
            for stage in script.inspectStages { progress(stage) }
            await script.hold?()
            if Task.isCancelled { throw DeckImportRefusal.cancelled }
            if fakeClass(of: name) == .collectionPackage { throw DeckImportRefusal.collectionPackage }
            let facts = try script.inspect.get()
            let id = state.withLock { state -> UInt64 in
                defer { state.next += 1 }
                state.open.insert(state.next)
                return state.next
            }
            return DeckImportSession(fake: id, facts: facts, fileName: name)
        }
    }

    public func apply(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation?,
                      askStructure: Bool) -> DeckImportTask<DeckImportApplyOutcome> {
        let script = state.withLock { $0.script }
        return DeckImportTask { [self] progress in
            guard let id = session.fakeId else { return .refused(.sessionClosed) }
            for stage in script.applyStages where stage.stage != .committing { progress(stage) }
            await script.hold?()
            if Task.isCancelled { return .refused(.cancelled) }
            let answer = state.withLock { state -> DeckImportApplyOutcome in
                guard state.open.contains(id), state.committed[id] == nil else { return .refused(.sessionClosed) }
                state.applied.append(Applied(session: id, options: options, confirmation: confirmation?.fakeToken,
                                             askStructure: askStructure))
                guard !state.script.applies.isEmpty else { return .refused(.stale) }
                let answer = state.script.applies[min(state.answered, state.script.applies.count - 1)]
                state.answered += 1
                if case .imported(let result) = answer { state.committed[id] = result }
                return answer
            }
            if case .imported = answer { for stage in script.applyStages where stage.stage == .committing { progress(stage) } }
            return answer
        }
    }

    public func result(of session: DeckImportSession) async throws(BridgeError) -> DeckImportResult? {
        state.withLock { state in session.fakeId.flatMap { state.open.contains($0) ? state.committed[$0] : nil } }
    }

    public func logPage(_ session: DeckImportSession, category: DeckImportLogCategory, offset: UInt64,
                        count: UInt32) async throws(BridgeError) -> [DeckImportLogEntry] {
        state.withLock { state in
            guard let id = session.fakeId, state.open.contains(id), state.committed[id] != nil else { return [] }
            let entries = state.script.log[category] ?? []
            let start = Int(min(offset, UInt64(entries.count)))
            return Array(entries[start..<min(entries.count, start + Int(count))])
        }
    }

    public func close(_ session: DeckImportSession) async {
        if let id = session.fakeId { state.withLock { _ = $0.open.remove(id) } }
    }
}
