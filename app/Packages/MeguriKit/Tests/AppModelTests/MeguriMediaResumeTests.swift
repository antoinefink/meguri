@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/meguri-provider` MP6.5, changed 2026-11-21 (resume fix): setup's
/// media step, stopped before it finished, continues by itself.
@MainActor @Suite struct MeguriMediaResumeTests {
    private static func receipt(_ phase: ManualTransferPhase, direction: ManualTransferDirection = .import,
                                acceptance: ManualTransferAcceptance = .notSent) -> MeguriBootstrapReceipt {
        .init(direction: direction, phase: phase, acceptance: acceptance)
    }

    // The launch path: what a relaunch shows and does for the retained operation.

    @Test func aRelaunchDuringTheMediaStepPausesItAndResumesItByItself() {
        // The owner's report: the collection transferred, the media was still
        // downloading, Meguri quit. The retained operation reads back as
        // collection complete or media incomplete. Before the fix it showed
        // as media blocked, whose only Retry was in Troubleshooting.
        for phase in [ManualTransferPhase.collectionComplete, .mediaIncomplete, .complete] {
            let stage = SyncCoordinator.meguriRetainedMediaStage(Self.receipt(phase), stopped: false)
            #expect(stage == .mediaPaused, "\(phase)")
            #expect(SyncCoordinator.meguriLaunchResume(stage: stage, foreground: true) == .launch)
            #expect(SyncCoordinator.meguriRetainedMediaStage(Self.receipt(phase), stopped: true) == .mediaStopped)
        }
        // Not in the foreground, or not paused: the launch starts nothing.
        #expect(SyncCoordinator.meguriLaunchResume(stage: .mediaPaused, foreground: false) == nil)
        #expect(SyncCoordinator.meguriLaunchResume(stage: .mediaPaused, foreground: nil) == nil)
        for stage: MeguriSetupStage? in [nil, .required, .failed, .mediaBlocked, .mediaStopped, .acceptanceUnknown] {
            #expect(SyncCoordinator.meguriLaunchResume(stage: stage, foreground: true) == nil)
        }
        // An operation whose collection step is not done never resumes a media step.
        for phase in [ManualTransferPhase.preparing, .preview, .confirmedIntent, .transferring, .remoteAccepted,
                      .activating, .superseded, .cancelled, .failed, .needsDecision] {
            #expect(SyncCoordinator.meguriRetainedMediaStage(Self.receipt(phase), stopped: false) == nil, "\(phase)")
        }
    }

    @Test func thePausedStepIsWaitingWithTryAgainOnTheStatusRow() {
        // One status row: its title, one sentence with the recorded count, and its action.
        let paused = SyncKinds.meguri(.init(stage: .mediaPaused, mediaFilesLeft: 4_431))
        #expect(paused.kind == .waiting && !paused.needsYou && !paused.hasIssues)
        #expect(paused.titleKey == "sync.meguri.status.downloadPaused.title")
        #expect(paused.detailKey == "sync.meguri.media.left" && paused.detailCount == 4_431)
        #expect(paused.action == .meguriRetryMedia && SyncStatusAction.meguriRetryMedia.titleKey == "sync.recovery.tryAgain")
        #expect(paused.symbol == SyncSymbol.waiting)
        // With no recorded count it says no number and names no direction.
        let unknown = SyncKinds.meguri(.init(stage: .mediaPaused, mediaFilesLeft: 0))
        #expect(unknown.titleKey == "sync.meguri.status.transferPaused.title")
        #expect(unknown.detailKey == "sync.meguri.media.left.unknown" && unknown.detailCount == nil)
        // The panel shows the same action, as its default button (N23.3).
        #expect(SyncPanel.slot(paused, serviceSelected: true, offered: true) == .action(.meguriRetryMedia))
        #expect(SyncPanel.slotIsDefault(.action(.meguriRetryMedia)))
        let route = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .mediaPaused, routine: nil)
        #expect(!route.isBusy && !route.needsUser && route.route == .account("sync.meguri.setup.continue"))
    }

    @Test func afterItsAutomaticAttemptsTheStepNeedsTheUser() {
        let stopped = SyncKinds.meguri(.init(stage: .mediaStopped, mediaFilesLeft: 12))
        #expect(stopped.needsYou && stopped.symbol == SyncKinds.needsYouSymbol)
        #expect(stopped.detailKey == "sync.needsYou.meguri.mediaStopped.download" && stopped.action == .meguriRetryMedia)
        #expect(SyncKinds.meguri(.init(stage: .mediaStopped)).detailKey == "sync.needsYou.meguri.mediaStopped")
        #expect(MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .mediaStopped, routine: nil).needsUser)
        #expect(SyncPanel.slot(stopped, serviceSelected: true, offered: true) == .action(.meguriRetryMedia))
    }

    @Test func automaticAttemptsBackOffThenStopUntilTryAgain() {
        var resume = MeguriMediaResume()
        for trigger in [MeguriMediaResume.Trigger.launch, .foreground, .network, .recovered, .retry, .user] {
            #expect(resume.admits(trigger))
        }
        resume.started(.launch)
        #expect(resume.finished(.unfinished) == .seconds(30))
        #expect(resume.admits(.network) && !resume.stopped)
        #expect(resume.finished(.unfinished) == .seconds(120))
        #expect(resume.finished(.unfinished) == nil)
        #expect(resume.stopped && !resume.admits(.foreground) && !resume.admits(.network) && !resume.admits(.retry))
        // Try Again is always admitted and starts the attempts again.
        #expect(resume.admits(.user))
        resume.started(.user)
        #expect(!resume.stopped && resume.admits(.network))
        #expect(resume.finished(.ready) == nil && resume.failures == 0)
        // Nothing ran (a blocked phase, the update hold): no attempt is used.
        #expect(resume.finished(.notRun) == nil && resume.failures == 0 && !resume.stopped)
        // What only the person can clear stops the automatic attempts at once.
        for outcome in [MeguriMediaResume.Outcome.stopped, .blocked] {
            var other = MeguriMediaResume()
            #expect(other.finished(outcome) == nil && other.stopped && !other.admits(.launch), "\(outcome)")
        }
    }

    @Test func aRefusalSaysWhetherTheStepCanRunAgain() {
        typealias R = MeguriMediaResume
        // A quit, a busy core or a lost answer: it continues by itself.
        for failure in [MeguriBootstrapFailure.busy, .cancelled, .unavailable, .mediaIncomplete] {
            #expect(R.outcome(refused: failure, accounting: nil) == .unfinished, "\(failure)")
        }
        // A changed account or connection is setup's sign-in row; a new sign-in lets the step continue.
        for failure in [MeguriBootstrapFailure.accountChanged, .connectionChanged, .username] {
            #expect(R.outcome(refused: failure, accounting: nil) == .failed, "\(failure)")
            var resume = R()
            #expect(resume.finished(.failed) == nil && !resume.stopped)
        }
        #expect(R.outcome(refused: .notSelected, accounting: nil) == .notRun)
        // Anything else waits for Try Again rather than showing a setup failure with no action.
        for failure in [MeguriBootstrapFailure.diskFull, .storage, .notComplete, .mediaEpochMissing, .mediaEpochChanged,
                        .publicationChanged, .documentChanged, .bindingChanged, .operationMismatch] {
            #expect(R.outcome(refused: failure, accounting: nil) == .stopped, "\(failure)")
        }
    }

    // The coordinator's record of an attempt: its waits, its stop, and the trigger a wait fires.

    @Test func theCoordinatorSchedulesTheNextAttemptAndStopsAfterTheThird() async throws {
        let sync = SyncCoordinator.renderFixture(.meguriSetupMediaPaused)
        var fired: [MeguriMediaResume.Trigger] = []
        var waits: [Duration?] = []
        sync.meguriMediaResumeObserver = { fired.append($0) }
        sync.meguriMediaRetryDelay = { wait in waits.append(wait); return wait.map { _ in .milliseconds(20) } }
        sync.recordMeguriMediaAttempt(.unfinished)
        #expect(waits == [.seconds(30)] && sync.meguriStage == .mediaPaused)
        for _ in 0..<100 where fired.isEmpty { try await Task.sleep(for: .milliseconds(10)) }
        #expect(fired == [.retry])
        sync.recordMeguriMediaAttempt(.unfinished)
        #expect(waits.last == .seconds(120) && sync.meguriStage == .mediaPaused)
        sync.recordMeguriMediaAttempt(.unfinished)
        // The third ends the automatic attempts: the status needs the person.
        #expect(waits.last == .some(nil) && sync.meguriStage == .mediaStopped && sync.mainStatus.needsYou)
        #expect(sync.mainStatus.action == .meguriRetryMedia)
    }
}
