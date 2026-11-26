import Instrumentation
internal import MeguriCoreBindings

/// The phase of a structure save job that is running (`sync/meguri-provider` MP7).
public enum MeguriStructureStage: Sendable, Hashable {
    case round, save, facts, serverCopy, upload, mediaPrepare, media
    init(_ raw: MeguriCoreBindings.ProviderStructureStage) {
        self = switch raw {
        case .round: .round; case .save: .save; case .facts: .facts; case .serverCopy: .serverCopy
        case .upload: .upload; case .mediaPrepare: .mediaPrepare; case .media: .media
        }
    }
}
/// The job did not start: nothing was saved and nothing is pending.
public enum MeguriStructureRefusal: Sendable {
    /// A round, a service change or another structure save is running.
    case busy
    /// Meguri Sync is not the ready service: the save runs alone (D5).
    case notReady
    /// An earlier structure upload is pending: Upload… comes first.
    case uploadPending
    /// Upload… with no pending structure upload.
    case nothingPending
    case round(MeguriRoundRefusal)
    case unavailable
    init(_ raw: MeguriCoreBindings.ProviderStructureRefusal) {
        self = switch raw {
        case .busy: .busy; case .notReady: .notReady; case .uploadPending: .uploadPending
        case .nothingPending: .nothingPending; case .round(let refusal): .round(MeguriRoundRefusal(refusal))
        case .unavailable: .unavailable
        // Only a deck package import is refused this way (`importDeckPackage` reads it first).
        case .importSessionClosed: .unavailable
        }
    }
}
public enum MeguriUploadPendingReason: Sendable, Hashable {
    case unreachable, busy, failed, cancelled, localChanged, storage
    /// The server refused the sign-in: it is asked for again.
    case signIn
    init(_ raw: MeguriCoreBindings.ProviderUploadPending) {
        self = switch raw {
        case .unreachable: .unreachable; case .busy: .busy; case .failed: .failed
        case .cancelled: .cancelled; case .localChanged: .localChanged; case .storage: .storage
        case .signIn: .signIn
        }
    }
}
/// The save is committed; what became of its upload.
public enum MeguriStructureUpload: Sendable, Hashable {
    /// The server holds the collection. With `mediaComplete` the binding is
    /// ready again; otherwise setup's media step continues (MP6.5).
    case uploaded(mediaComplete: Bool)
    /// The op changed no structure: an ordinary round sends it.
    case notNeeded
    /// The needs-you line with Upload… (MP7 step 5).
    case pending(MeguriUploadPendingReason)
    /// MP6.3's collection choice; the upload stays pending meanwhile.
    case chooseCollection
    /// MP6.9 settles the upload first.
    case acceptanceUnknown
    /// A retained setup operation owns the next step.
    case setupInProgress
    init(_ raw: MeguriCoreBindings.ProviderStructureUpload) {
        self = switch raw {
        case .uploaded(let mediaComplete): .uploaded(mediaComplete: mediaComplete)
        case .notNeeded: .notNeeded; case .pending(let reason): .pending(MeguriUploadPendingReason(reason))
        case .chooseCollection: .chooseCollection; case .acceptanceUnknown: .acceptanceUnknown
        case .setupInProgress: .setupInProgress
        }
    }
}
public enum MeguriStructureOutcome: Sendable {
    /// The job did not start.
    case refused(MeguriStructureRefusal)
    /// Nothing was saved: the round found that the server requires a full
    /// sync (MP6.3's choice; the draft stays).
    case fullSyncRequired
    /// Nothing was saved: the round failed, was cancelled or refused, or lost
    /// its acknowledgement.
    case roundStopped(MeguriRoundPhaseResult)
    /// Nothing was saved: cancelled before the save.
    case cancelled
    /// Nothing was saved: the job's record could not be written.
    case unavailable
    case saved(MeguriStructureUpload)
}

extension CoreMeguriRoundService {
    /// MP7's job for an accepted save sheet (`notetypes/draft-save` D6): one
    /// normal round, the save with the losses the sheet showed, the upload.
    /// `reachable` false is D4's unreachable case: no round, the save, and
    /// the upload left pending.
    ///
    /// When the op refuses or fails (`Stale`, `LossesChanged` with the new
    /// counts, a safety backup that could not be made), this call throws
    /// that error, as the local save's job reports it: nothing was saved
    /// and nothing is pending. Cancelling the
    /// task asks the core to stop where MP7 allows; the call still returns
    /// the job's outcome.
    public func saveNotetypeStructure(
        _ draft: NotetypeStructureDraft, reachable: Bool,
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void = { _, _ in }
    ) async throws -> MeguriStructureOutcome {
        let drains = drains
        let (connection, credentials) = try await admittedInputs()
        let op = MeguriCoreBindings.ProviderStructureOp.save(draft: draft.raw)
        return try await owned(since: drains, busy: .refused(.busy)) {
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.providerStartStructureSave) {
                try connection.startStructureSave(credentials: credentials, op: op, reachable: reachable)
            }
            return try await Self.drive(started, onStage: onStage)
        }
    }
    /// W12's accepted question: the same MP7 job, with the published removal op.
    public func removeNotetypeStructure(
        _ request: NotetypeRemoval, reachable: Bool,
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void = { _, _ in }
    ) async throws -> MeguriStructureOutcome {
        guard let expectedGeneration = request.expectedGeneration, expectedGeneration != 0 else {
            throw BridgeError.invalidArgument("Note type removal requires a collection scope")
        }
        let drains = drains
        let (connection, credentials) = try await admittedInputs()
        let op = MeguriCoreBindings.ProviderStructureOp.remove(request: request.raw)
        return try await owned(since: drains, busy: .refused(.busy)) {
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.providerStartStructureSaveForGeneration) {
                try connection.startStructureSaveForGeneration(credentials: credentials, op: op,
                    reachable: reachable, generation: expectedGeneration)
            }
            return try await Self.drive(started, onStage: onStage)
        }
    }
    /// One structure job at a time on this facade, registered so that
    /// `cancelAndDrain` and retirement stop and join it. `busy` is the
    /// answer while another one runs.
    func owned<Output: Sendable>(since drains: UInt64, busy: Output,
                                 _ work: @escaping @Sendable () async throws -> Output) async throws -> Output {
        // A drain that ran while the credentials were read: nothing starts after it.
        guard drains == self.drains else { throw CancellationError() }
        guard structureWork == nil else { return busy }
        let task = Task { try await work() }
        structureWork = Task { _ = await withTaskCancellationHandler { await task.result } onCancel: { task.cancel() } }
        let result = await withTaskCancellationHandler { await task.result } onCancel: { task.cancel() }
        structureWork = nil
        return try result.get()
    }
    /// MP7 step 5's Upload…: the server's facts and the upload, with no
    /// round and no further question.
    public func uploadNotetypeStructure(
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void = { _, _ in }
    ) async throws -> MeguriStructureOutcome {
        let drains = drains
        let (connection, credentials) = try await admittedInputs()
        return try await owned(since: drains, busy: .refused(.busy)) {
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.providerStartStructureUpload) {
                try connection.startStructureUpload(credentials: credentials)
            }
            return try await Self.drive(started, onStage: onStage)
        }
    }
    /// Advances the job until its outcome. It is never left half way: the
    /// core holds the provider's lane until the outcome is read.
    private static func drive(
        _ started: MeguriCoreBindings.ProviderStructureStart,
        onStage: @escaping @Sendable (MeguriStructureStage, CoreJob) -> Void
    ) async throws -> MeguriStructureOutcome {
        let job: MeguriCoreBindings.ProviderStructureJob
        switch started {
        case .refused(let refusal): return .refused(MeguriStructureRefusal(refusal))
        case .started(let value): job = value
        }
        var forwarded = false
        while true {
            // A cancellation reaches the core before the next phase can start.
            if Task.isCancelled, !forwarded {
                forwarded = true
                try? await MaintenanceLane.shared.call(.providerStructureCancel) { try job.cancel() }
            }
            let step = try await MaintenanceLane.shared.call(.providerStructureAdvance) { try job.advance() }
            switch step {
            case .finished(let outcome):
                switch outcome {
                case .fullSyncRequired: return .fullSyncRequired
                case .cancelled: return .cancelled
                case .unavailable: return .unavailable
                // Only a deck package import ends this way (`importDeckPackage`).
                case .importNotCommitted: return .unavailable
                case .saved(let upload): return .saved(MeguriStructureUpload(upload))
                case .roundStopped(let round):
                    let phase = try await MaintenanceLane.shared.call(.providerRoundOutcome) { try round.outcome() }
                    return .roundStopped(.init(outcome: MeguriRoundPhaseOutcome(phase),
                        finalization: MeguriRoundFinalization(round), trackingFailure: nil))
                }
            case .running(let stage, let handle):
                // A phase whose listener could not be set has already drained.
                if case .tracked(let phase) = await Jobs.trackRetainingDrain(handle) {
                    onStage(MeguriStructureStage(stage), phase)
                    _ = await withTaskCancellationHandler { await phase.outcome() } onCancel: {
                        Task { try? await MaintenanceLane.shared.call(.providerStructureCancel) { try job.cancel() } }
                    }
                }
            }
        }
    }
}
