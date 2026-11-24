import CoreBridge
import Foundation

/// MP7's structure jobs as the sync coordinator runs them. Production is the
/// bridge's (`CoreMeguriStructureJobs`); tests pass a fake.
nonisolated public protocol MeguriStructureJobService: Sendable {
    /// Steps 1 to 4 for an accepted save sheet (`notetypes/draft-save` D6).
    /// `onStage` gets each phase as it starts, with its progress while it runs.
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome
    /// Step 5's Upload….
    func uploadNotetypeStructure(onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome
    /// Step 5's durable state: a committed structure save waits for its upload.
    func structureUploadPending() async throws -> Bool
    /// MP7.1's job for an accepted deck package import confirmation.
    func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation,
                           reachable: Bool,
                           onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriDeckImportOutcome
}

nonisolated extension MeguriStructureJobService {
    /// Fakes that run no import: the job does not start.
    func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation,
                           reachable: Bool,
                           onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriDeckImportOutcome { .notStarted(.notReady) }
}

/// The bridge's jobs: the ready binding's round service runs them, the
/// provider reads the pending state (also while the binding is not ready).
nonisolated struct CoreMeguriStructureJobs: MeguriStructureJobService {
    let round: CoreMeguriRoundService?
    let provider: CoreProviderService

    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome {
        guard let round else { return .refused(.notReady) }
        return try await round.saveNotetypeStructure(draft, reachable: reachable) { stage, job in onStage(stage, job.progress) }
    }
    func uploadNotetypeStructure(onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome {
        guard let round else { return .refused(.notReady) }
        return try await round.uploadNotetypeStructure { stage, job in onStage(stage, job.progress) }
    }
    func structureUploadPending() async throws -> Bool { try await provider.structureUploadPending() }
    func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation,
                           reachable: Bool,
                           onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriDeckImportOutcome {
        guard let round else { return .notStarted(.notReady) }
        return try await round.importDeckPackage(session, options: options, confirmation: confirmation, reachable: reachable) {
            stage, job in onStage(stage, job.progress)
        }
    }
}

nonisolated extension MeguriDeckImportOutcome {
    /// The job's outcome as a structure job's, for what the sync status does after it.
    var structureOutcome: MeguriStructureOutcome {
        switch self {
        case .notStarted(let refusal): .refused(refusal)
        case .fullSyncRequired: .fullSyncRequired
        case .roundStopped(let round): .roundStopped(round)
        case .cancelled: .cancelled
        case .unavailable: .unavailable
        // Nothing imported and nothing pending.
        case .notImported: .refused(.unavailable)
        case .imported(_, let upload): .saved(upload)
        // Committed: the upload is pending unless the job said otherwise.
        case .importedWithoutResult(let upload): .saved(upload ?? .pending(.failed))
        }
    }
}

/// What the app does once a structure job (MP7) ended, besides reading the
/// pending state again, which it always does. One row per outcome of the
/// bridge (PR #100's state machine).
nonisolated struct MeguriStructureEffect: Sendable {
    /// What the Note Types section shows for a Save.
    var result: NoteTypeSyncSaveResult
    /// Read the binding's readiness again: the upload held the binding, or a
    /// retained setup operation (settling, finishing, media) owns the next step.
    var readiness = false
    /// After that read, MP6.3's collection choice (T19.5 "full sync required").
    var choose = false
    /// The routine state the status shows from now.
    var routine: MeguriRoutineState?
    /// A stopped round, reduced as a routine round is.
    var round: MeguriRoundReport?
    /// The server now holds this collection: a completed exchange.
    var exchanged = false
    /// An ordinary round sends what the save changed.
    var wake = false

    static func after(_ outcome: MeguriStructureOutcome) -> MeguriStructureEffect {
        switch outcome {
        // The job did not start: nothing saved, nothing pending (the pending
        // read shows an earlier upload that waits, for `uploadPending`).
        case .refused: return .init(result: .notSaved)
        // MP7 step 2: the server needs a full sync already; the draft stays.
        case .fullSyncRequired: return .init(result: .syncStopped, readiness: true, choose: true, routine: .fullSyncRequired)
        // A failed, cancelled, refused or lost-acknowledgement round: nothing saved.
        case .roundStopped(let round): return .init(result: .syncStopped, round: MeguriRoundReport(collection: round, media: nil))
        case .cancelled: return .init(result: .cancelled)
        case .unavailable: return .init(result: .notSaved)
        case .saved(let upload):
            switch upload {
            case .uploaded(let mediaComplete):
                // With its media unfinished, setup's media step continues (MP6.5).
                return mediaComplete
                    ? .init(result: .saved(uploaded: true), readiness: true, routine: .idle, exchanged: true)
                    : .init(result: .saved(uploaded: true), readiness: true)
            case .notNeeded: return .init(result: .saved(uploaded: true), wake: true)
            // The server refused the sign-in: it is asked for again; Upload… follows.
            case .pending(.signIn): return .init(result: .saved(uploaded: false), routine: .authentication)
            case .pending: return .init(result: .saved(uploaded: false))
            case .chooseCollection: return .init(result: .saved(uploaded: false), readiness: true, choose: true)
            case .acceptanceUnknown, .setupInProgress: return .init(result: .saved(uploaded: false), readiness: true)
            }
        }
    }

    /// The toolbar's name for a phase of the job.
    static func stage(_ stage: MeguriStructureStage) -> NoteTypeSyncStage {
        switch stage {
        case .round: .syncing
        case .save: .saving
        case .facts, .serverCopy, .upload, .mediaPrepare, .media: .uploading
        }
    }
}
