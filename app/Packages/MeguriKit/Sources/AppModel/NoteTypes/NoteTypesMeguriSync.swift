import CoreBridge
import Foundation

/// Whether Save and Upload can run now (`notetypes/draft-save` D3, D6;
/// `sync/meguri-provider` MP7). With Meguri Sync chosen, set up or not, the
/// sheet is Meguri Sync's: a structural save never runs on this device
/// alone while the server may hold the collection.
public nonisolated enum NoteTypeSyncState: Sendable, Hashable {
    /// The binding is ready: Save and Upload runs MP7's job.
    case ready
    /// Meguri Sync is chosen but not set up, or its binding is not read yet:
    /// the sheet says to set it up.
    case notSetUp
    /// An earlier structure save waits for Upload… (MP7 step 5), which comes first.
    case uploadPending
}

/// The part of MP7's job that the toolbar names while it runs.
public nonisolated enum NoteTypeSyncStage: Sendable, Hashable {
    /// Step 2, the normal round.
    case syncing
    /// Step 3, the save: "Updating <i> of <n> notes" once counted.
    case saving
    /// Step 4, the upload and its media.
    case uploading
}

/// What the job did, as the section shows it. The sync status shows the rest.
public nonisolated enum NoteTypeSyncSaveResult: Sendable, Hashable {
    /// The save is committed on this device: the draft goes. `uploaded`
    /// false: the server does not hold it yet, and the sync status says
    /// what to do (Upload…, a collection choice, a setup step).
    case saved(uploaded: Bool)
    /// Nothing was saved: Meguri Sync stopped before the save (the round
    /// failed or needs the person); the sync status says why. The draft stays.
    case syncStopped
    /// Nothing was saved: the job was cancelled before the save.
    case cancelled
    /// Nothing was saved: the job could not run.
    case notSaved
}

/// Meguri Sync's side of a note type save, the sync coordinator in the app.
/// Tests pass their own.
@MainActor public protocol NoteTypesMeguriSync: AnyObject {
    var noteTypeSyncState: NoteTypeSyncState { get }
    /// D4: the reachability check made as the sheet opens. False only when
    /// the server is known not to answer.
    func noteTypeSyncReachable() async -> Bool
    /// MP7 steps 1 to 4 for an accepted sheet. `reachable` false is D4's
    /// unreachable case: the save, and the upload left pending. Throws the
    /// op's refusal (`LossesChanged`, `Stale`, a failed backup): nothing was
    /// saved and nothing is pending.
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void) async throws -> NoteTypeSyncSaveResult
    /// Asks the running job to stop where MP7 allows: before the save
    /// nothing is saved; once saved, the upload is left pending.
    func cancelNotetypeStructureSave()
}
