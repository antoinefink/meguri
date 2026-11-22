@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// A scripted bridge for MP7's jobs: each call answers what the test set, and
/// the pending state becomes `pendingAfter` once a job has run.
private actor FakeStructureJobs: MeguriStructureJobService {
    var saveAnswer: Result<MeguriStructureOutcome, any Error> = .success(.saved(.uploaded(mediaComplete: true)))
    var uploadAnswer: Result<MeguriStructureOutcome, any Error> = .success(.saved(.uploaded(mediaComplete: true)))
    var pending = false
    var pendingAfter: Bool?
    var stages: [MeguriStructureStage] = [.round, .save, .upload]
    /// The pending read fails (the state cannot be known).
    var pendingFails = false
    /// The round runs until the job is cancelled, then stops.
    var roundUntilCancelled = false
    private(set) var reachable: [Bool] = []
    private(set) var uploads = 0
    private(set) var pendingReads = 0

    func set(save: Result<MeguriStructureOutcome, any Error>? = nil, upload: Result<MeguriStructureOutcome, any Error>? = nil,
             pending: Bool? = nil, pendingAfter: Bool?? = nil, stages: [MeguriStructureStage]? = nil,
             pendingFails: Bool? = nil, roundUntilCancelled: Bool? = nil) {
        if let pendingFails { self.pendingFails = pendingFails }
        if let roundUntilCancelled { self.roundUntilCancelled = roundUntilCancelled }
        if let save { saveAnswer = save }
        if let upload { uploadAnswer = upload }
        if let pending { self.pending = pending }
        if let pendingAfter { self.pendingAfter = pendingAfter }
        if let stages { self.stages = stages }
    }
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome {
        self.reachable.append(reachable)
        if roundUntilCancelled {
            onStage(.round, nil)
            while !Task.isCancelled { try? await Task.sleep(for: .milliseconds(5)) }
            return .roundStopped(.init(outcome: .refused(.interrupted), finalization: nil, trackingFailure: nil))
        }
        for stage in stages { onStage(stage, nil) }
        if let pendingAfter { pending = pendingAfter }
        return try saveAnswer.get()
    }
    func uploadNotetypeStructure(onStage: @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void)
        async throws -> MeguriStructureOutcome {
        uploads += 1
        onStage(.facts, nil)
        if let pendingAfter { pending = pendingAfter }
        return try uploadAnswer.get()
    }
    func structureUploadPending() async throws -> Bool {
        pendingReads += 1
        if pendingFails { throw BridgeError.unavailable(reason: "unreadable") }
        return pending
    }
}

@MainActor private final class StageLog { var stages: [NoteTypeSyncStage] = [] }

private let draft = NotetypeStructureDraft(notetypeId: 7, base: [1], name: "Sample", css: "", fields: [], templates: [],
                                           sortField: 0, skipChecks: false)

/// `sync/meguri-provider` MP7 and `notetypes/draft-save` D6, D7 in the app:
/// every outcome of the bridge's job, the pending upload and its Upload….
@MainActor @Suite struct MeguriStructureSaveTests {
    private func coordinator(_ jobs: FakeStructureJobs) -> SyncCoordinator {
        let sync = SyncCoordinator.renderFixture(.meguriConnected)
        sync.stageMeguriStructureForTests(jobs)
        return sync
    }

    /// The outcome table (PR #100's state machine) as the section and the status show it.
    @Test func everyOutcomeMapsToWhatTheSectionAndTheStatusShow() {
        typealias E = MeguriStructureEffect
        let stopped = MeguriRoundPhaseResult(outcome: .refused(.busy), finalization: nil, trackingFailure: nil)
        let rows: [(MeguriStructureOutcome, NoteTypeSyncSaveResult, readiness: Bool, choose: Bool, routine: MeguriRoutineState?, wake: Bool)] = [
            (.refused(.busy), .notSaved, false, false, nil, false),
            (.refused(.notReady), .notSaved, false, false, nil, false),
            (.refused(.uploadPending), .notSaved, false, false, nil, false),
            (.refused(.nothingPending), .notSaved, false, false, nil, false),
            (.refused(.unavailable), .notSaved, false, false, nil, false),
            (.fullSyncRequired, .syncStopped, true, true, .fullSyncRequired, false),
            (.roundStopped(stopped), .syncStopped, false, false, nil, false),
            (.cancelled, .cancelled, false, false, nil, false),
            (.unavailable, .notSaved, false, false, nil, false),
            (.saved(.uploaded(mediaComplete: true)), .saved(uploaded: true), true, false, .idle, false),
            (.saved(.uploaded(mediaComplete: false)), .saved(uploaded: true), true, false, nil, false),
            (.saved(.notNeeded), .saved(uploaded: true), false, false, nil, true),
            (.saved(.pending(.unreachable)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.failed)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.busy)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.cancelled)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.localChanged)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.storage)), .saved(uploaded: false), false, false, nil, false),
            (.saved(.pending(.signIn)), .saved(uploaded: false), false, false, .authentication, false),
            (.saved(.chooseCollection), .saved(uploaded: false), true, true, nil, false),
            (.saved(.acceptanceUnknown), .saved(uploaded: false), true, false, nil, false),
            (.saved(.setupInProgress), .saved(uploaded: false), true, false, nil, false),
        ]
        for (outcome, result, readiness, choose, routine, wake) in rows {
            let effect = E.after(outcome)
            #expect(effect.result == result && effect.readiness == readiness && effect.choose == choose
                && effect.routine == routine && effect.wake == wake, "\(outcome)")
        }
        #expect(E.after(.roundStopped(stopped)).round != nil)
        #expect(E.after(.saved(.uploaded(mediaComplete: true))).exchanged)
        #expect([MeguriStructureStage.round, .save, .facts, .serverCopy, .upload, .mediaPrepare, .media].map(E.stage)
            == [.syncing, .saving, .uploading, .uploading, .uploading, .uploading, .uploading])
    }

    @Test func aReadyBindingSavesSyncsAndUploads() async throws {
        let jobs = FakeStructureJobs()
        let sync = coordinator(jobs)
        #expect(sync.noteTypeSyncState == .ready)
        let seen = StageLog()
        let result = try await sync.saveNotetypeStructure(draft, reachable: true) { stage, _, _ in seen.stages.append(stage) }
        for _ in 0..<20 { await Task.yield() }
        #expect(result == .saved(uploaded: true) && !sync.meguriStructureUploadPending)
        #expect(sync.meguriRoutine == .idle && sync.meguriStructureStage == nil && !sync.meguriBusy)
        #expect(await jobs.reachable == [true])
        #expect(seen.stages == [.syncing, .saving, .uploading], "each phase as it starts")
        #expect(!sync.mainStatus.needsYou)
    }

    /// Offline (D4's unreachable case): saved here, the upload pending, the needs-you line with Upload….
    @Test func offlineSavesHereAndLeavesTheUploadPending() async throws {
        let jobs = FakeStructureJobs()
        await jobs.set(save: .success(.saved(.pending(.unreachable))), pendingAfter: true, stages: [.save])
        let sync = coordinator(jobs)
        let result = try await sync.saveNotetypeStructure(draft, reachable: false) { _, _, _ in }
        #expect(result == .saved(uploaded: false))
        #expect(await jobs.reachable == [false])
        #expect(sync.meguriStructureUploadPending && sync.noteTypeSyncState == .uploadPending)
        let status = sync.mainStatus
        #expect(status.needsYou && status.detailKey == "sync.needsYou.meguri.structureUpload" && status.action == .meguriUploadStructure)
        // No round while it waits; Upload… is never the panel's default button (it replaces the server's collection).
        #expect(sync.meguriStatus.route == .disabled(nil) && sync.meguriStatus.needsUser)
        #expect(!SyncPanel.slotIsDefault(.action(.meguriUploadStructure)))
    }

    /// The state is the core's, read on load: a relaunch shows the line again.
    @Test func aPendingUploadSurvivesARelaunch() async {
        let jobs = FakeStructureJobs()
        await jobs.set(pending: true)
        let relaunched = coordinator(jobs)
        #expect(!relaunched.meguriStructureUploadPending)
        #expect(await relaunched.refreshMeguriStructurePending() == true)
        #expect(relaunched.mainStatus.detailKey == "sync.needsYou.meguri.structureUpload")
        #expect(relaunched.noteTypeSyncState == .uploadPending)
    }

    @Test func uploadRunsTheUploadAndClearsTheLine() async {
        let jobs = FakeStructureJobs()
        await jobs.set(upload: .success(.saved(.uploaded(mediaComplete: true))), pending: true, pendingAfter: false)
        let sync = coordinator(jobs)
        await sync.refreshMeguriStructurePending()
        #expect(sync.meguriCanUploadStructure)
        await sync.uploadNotetypeStructure()?.value
        #expect(await jobs.uploads == 1)
        #expect(!sync.meguriStructureUploadPending && sync.meguriRoutine == .idle && !sync.mainStatus.needsYou)
        // Nothing waits: Upload… does nothing.
        #expect(sync.uploadNotetypeStructure() == nil)
    }

    @Test func aFailedUploadKeepsTheLine() async {
        let jobs = FakeStructureJobs()
        await jobs.set(upload: .success(.saved(.pending(.failed))), pending: true, pendingAfter: true)
        let sync = coordinator(jobs)
        await sync.refreshMeguriStructurePending()
        await sync.uploadNotetypeStructure()?.value
        #expect(sync.meguriStructureUploadPending && sync.mainStatus.action == .meguriUploadStructure)
    }

    /// An error after the commit: the pending state, read again, says the save is here.
    @Test func anErrorAfterTheCommitIsStillASaveAndAnErrorBeforeItIsNot() async {
        let jobs = FakeStructureJobs()
        await jobs.set(save: .failure(BridgeError.internal("lane")), pendingAfter: true)
        let sync = coordinator(jobs)
        let saved = try? await sync.saveNotetypeStructure(draft, reachable: true) { _, _, _ in }
        #expect(saved == .saved(uploaded: false) && sync.meguriStructureUploadPending)
        #expect(await jobs.pendingReads >= 1)

        let other = FakeStructureJobs()
        await other.set(save: .failure(BridgeError.stale(table: .notetypes, key: "7")), pendingAfter: false)
        let fresh = coordinator(other)
        await #expect(throws: BridgeError.self) { try await fresh.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } }
        #expect(!fresh.meguriStructureUploadPending && !fresh.meguriBusy)
    }

    /// D6 step 1: a round that needs the person stops the job; the status says why.
    @Test func aStoppedRoundSavesNothingAndTheStatusSaysWhy() async throws {
        let jobs = FakeStructureJobs()
        await jobs.set(save: .success(.fullSyncRequired), stages: [.round])
        let sync = coordinator(jobs)
        #expect(try await sync.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } == .syncStopped)
        #expect(sync.meguriRoutine == .fullSyncRequired && sync.mainStatus.detailKey == "sync.needsYou.meguri.chooseCollection")

        let failed = FakeStructureJobs()
        await failed.set(save: .success(.roundStopped(.init(outcome: .refused(.busy), finalization: nil, trackingFailure: nil))))
        let other = coordinator(failed)
        #expect(try await other.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } == .syncStopped)
        #expect(other.meguriRoutine == .waiting && !other.meguriStructureUploadPending)
    }

    /// A cancel belongs to the save it was asked for: an earlier one does not stop the next save.
    @Test func anEarlierCancelDoesNotStopTheNextSave() async throws {
        let jobs = FakeStructureJobs()
        let sync = coordinator(jobs)
        sync.cancelNotetypeStructureSave()
        #expect(try await sync.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } == .saved(uploaded: true))
    }

    /// Leaving the app during the round stops it: a cancel, not "couldn't sync".
    @Test func aCancelDuringTheRoundIsACancelNotASyncFailure() async throws {
        let jobs = FakeStructureJobs()
        await jobs.set(roundUntilCancelled: true)
        let sync = coordinator(jobs)
        let save = Task { try await sync.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } }
        for _ in 0..<500 where sync.meguriStructureStage != .round { await Task.yield() }
        #expect(sync.meguriStructureStage == .round)
        sync.cancelNotetypeStructureSave()
        #expect(try await save.value == .cancelled)
        #expect(sync.meguriStructureStage == nil && !sync.meguriBusy)
    }

    /// An error whose save cannot be known is neither "saved" (the draft would go) nor "nothing was changed".
    @Test func anErrorWithAnUnreadablePendingStateIsUnconfirmed() async throws {
        let jobs = FakeStructureJobs()
        await jobs.set(save: .failure(BridgeError.internal("lane")), pendingFails: true)
        let sync = coordinator(jobs)
        #expect(try await sync.saveNotetypeStructure(draft, reachable: true) { _, _, _ in } == .unconfirmed)
    }

    /// Undoing the save withdraws the line; redoing it brings it back.
    @Test func noteTypeChangesReadThePendingStateAgain() async {
        let jobs = FakeStructureJobs()
        await jobs.set(pending: true)
        let sync = coordinator(jobs)
        await sync.refreshMeguriStructurePending()
        await jobs.set(pending: false)
        sync.notetypesChanged()
        for _ in 0..<200 where sync.meguriStructureUploadPending { await Task.yield() }
        #expect(!sync.meguriStructureUploadPending, "undo")
        await jobs.set(pending: true)
        sync.notetypesChanged()
        for _ in 0..<200 where !sync.meguriStructureUploadPending { await Task.yield() }
        #expect(sync.meguriStructureUploadPending, "redo")
    }

    /// The status while the job runs: what moves, no needs-you, Sync Now off.
    @Test func theStatusNamesTheJobsPhase() {
        func title(_ stage: MeguriStructureStage) -> String {
            SyncKinds.meguri(MeguriKindFacts(routine: .idle, structureStage: stage)).titleKey
        }
        #expect(title(.round) == "sync.status.syncing.title" && title(.save) == "sync.status.syncing.title")
        #expect(title(.upload) == "sync.meguri.status.transferringCollection.title")
        #expect(title(.media) == "sync.meguri.status.transferringMedia.title")
        #expect(SyncKinds.meguri(MeguriKindFacts(routine: .idle, structureStage: .upload)).symbol == SyncSymbol.moving)
        let running = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: nil, routine: .idle,
                                                   structureRunning: true)
        #expect(running.route == .disabled(nil) && running.isBusy && !running.needsUser)
        // A refused sign-in is asked for before Upload….
        #expect(SyncKinds.meguriRow(MeguriKindFacts(routine: .authentication, structureUploadPending: true)).action == .meguriSignIn)
        // While its upload holds the binding (setup required after a relaunch), the line still shows.
        #expect(SyncKinds.meguriRow(MeguriKindFacts(stage: .required, structureUploadPending: true)).action == .meguriUploadStructure)
        #expect(MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .required, routine: nil,
                                             structureUploadPending: true).needsUser)
        // There, a sign-in the server refused during the upload is asked for first.
        #expect(SyncKinds.meguriRow(MeguriKindFacts(stage: .required, routine: .authentication, structureUploadPending: true)).action
            == .meguriSignIn)
        // A setup step (the collection choice after Upload…) comes before the line.
        #expect(SyncKinds.meguriRow(MeguriKindFacts(stage: .failed, failureKey: MeguriSetupSequence.fullSyncKey,
                                                    structureUploadPending: true)).action == .chooseCollection)
    }

    /// Any Meguri Sync selection counts; without a ready binding the sheet says why: set it up, or wait for setup.
    @Test func meguriSyncChosenButNotReadySaysWhy() {
        #expect(SyncCoordinator.renderFixture(.meguriSignIn).noteTypeSyncState == .notSetUp)
        #expect(SyncCoordinator.renderFixture(.meguriSetupMedia).noteTypeSyncState == .busy)
        #expect(SyncCoordinator.renderFixture(.meguriConnected).noteTypeSyncState == .busy, "binding not read yet")
        #expect(SyncCoordinator.renderFixture(.off).noteTypeSyncState == .notSetUp)
        #expect(SyncCoordinator.renderFixture(.meguriStructureUploadPending).noteTypeSyncState == .uploadPending)
    }
}
