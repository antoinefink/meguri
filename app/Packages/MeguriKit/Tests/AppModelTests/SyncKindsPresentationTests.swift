@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/status-strings` T19.9: each row of T19.4's two tables and of T19.5.
@MainActor @Suite struct SyncKindsPresentationTests {
    private typealias Row = SyncKindRow



    /// T19.5 (MP7 step 5): an acceptance unknown comes before Upload…, as a round's or as setup's.
    @Test func acceptanceUnknownComesBeforeTheStructureUpload() {
        typealias Facts = MeguriKindFacts
        #expect(SyncKinds.meguriRow(Facts(routine: .acceptanceUnknown, structureUploadPending: true))
            == Row(.waiting, "sync.meguri.round.acceptanceUnknown", .meguriTryAgain))
        #expect(SyncKinds.meguriRow(Facts(stage: .acceptanceUnknown, structureUploadPending: true))
            == Row(.waiting, "sync.meguri.setup.acceptanceUnknown", .meguriTryAgain))
        #expect(SyncKinds.meguriRow(Facts(routine: .idle, structureUploadPending: true))
            == Row(.needsYou, "sync.needsYou.meguri.structureUpload", .meguriUploadStructure))
        #expect(SyncKinds.meguriRow(Facts(routine: .authentication, structureUploadPending: true))
            == Row(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn))
    }

    /// T21.2: the quiet "setup not finished" status, and the rows that stay needs you.
    @Test func meguriBeforeTheFirstSignInIsQuiet() {
        typealias Facts = MeguriKindFacts
        let notSetUp = SyncKinds.meguri(Facts(connection: .unconfigured))
        #expect(notSetUp.kind == nil && notSetUp.symbol == SyncSymbol.off)
        #expect(notSetUp.titleKey == "sync.meguri.status.notSetUp.title" && notSetUp.action == .meguriSignIn)
        #expect(SyncText.value("sync.meguri.status.notSetUp.title") == "Not Set Up")
        #expect(SyncText.value("sync.meguri.status.notSetUp.detail") == "Sign in to your Meguri Sync server to finish setting up.")
        #expect(SyncText.value("sync.meguri.address.prompt") == "https://sync.example.com")
        #expect(SyncText.value("sync.meguri.required.prompt") == "Required")
        for facts in [Facts(connection: .signedOut, signedInBefore: false), Facts(connection: .cancelled, signedInBefore: false)] {
            #expect(SyncKinds.meguriSetupNotFinished(facts) && SyncKinds.meguri(facts).symbol == SyncSymbol.off)
        }
        // Something that worked and now needs the person keeps the needs-you symbol.
        for facts in [Facts(connection: .signedOut), Facts(routine: .authentication),
                      Facts(stage: .failed, failureKey: "sync.meguri.setup.error.identityChanged")] {
            #expect(!SyncKinds.meguriSetupNotFinished(facts))
            #expect(SyncKinds.meguri(facts).kind == .needsYou && SyncKinds.meguri(facts).symbol == SyncKinds.needsYouSymbol)
        }
        // The first sign-in running, and a first Sign In that failed (the owner reports blocked), stay setup.
        for facts in [Facts(connection: .authenticating, signedInBefore: false),
                      Facts(connection: .blocked, signedInBefore: false, signInRefused: true)] {
            #expect(SyncKinds.meguriSetupNotFinished(facts) && SyncKinds.meguri(facts).kind == nil, "\(facts)")
        }
        #expect(SyncKinds.meguri(Facts(connection: .authenticating, signedInBefore: false)).titleKey == "sync.meguri.status.signingIn.title")
        // A store failure with no sign-in attempt, or after a sign-in, still needs the person.
        #expect(SyncKinds.meguri(Facts(connection: .blocked, signedInBefore: false)).kind == .needsYou)
        #expect(SyncKinds.meguri(Facts(connection: .blocked, signInRefused: true)).kind == .needsYou)
        // Nothing waits on the person: the review's status button stays hidden (N16.2).
        #expect(!SyncCoordinator.renderFixture(.meguriNeverSignedIn).needsUser)
        #expect(SyncCoordinator.renderFixture(.meguriSignIn).needsUser)
        // The provider's own problems come first.
        #expect(!SyncKinds.meguriSetupNotFinished(Facts(providerBlocked: true, connection: .unconfigured)))
        #expect(!SyncKinds.meguriSetupNotFinished(Facts(handoffRecovery: true, connection: .unconfigured)))
        // The coordinator: never signed in is quiet; signed in and then signed out needs the person.
        let never = SyncCoordinator.renderFixture(.meguriNeverSignedIn)
        #expect(never.meguriSetupNotFinished && never.mainStatus.kind == nil)
        let signedOut = SyncCoordinator.renderFixture(.meguriSignIn)
        #expect(!signedOut.meguriSetupNotFinished && signedOut.mainStatus.kind == .needsYou)
        #expect(!SyncCoordinator.renderFixture(.meguriConnected).meguriSetupNotFinished)
        #expect(!SyncCoordinator.renderFixture(.off).meguriSetupNotFinished)
    }

    @Test func meguriRowsFollowT18Order() {
        typealias Facts = MeguriKindFacts
        let rows: [(Facts, Row)] = [
            (Facts(handoffRecovery: true, providerBlocked: true, connection: .blocked),
             Row(.needsYou, "sync.needsYou.switchUnfinished", .serviceRecovery)),
            (Facts(providerBlocked: true, connection: .blocked), Row(.needsYou, "sync.needsYou.blocked", .copyDebugInfo)),
            (Facts(connection: .blocked, stage: .failed), Row(.needsYou, "sync.needsYou.meguri.signInStore", .meguriConnectionRetry)),
            // T21.2 (2026-11-22): before any sign-in, setup is unfinished, not a problem.
            (Facts(connection: .unconfigured), Row(nil, "sync.meguri.status.notSetUp.detail", .meguriSignIn)),
            (Facts(connection: .signedOut, signedInBefore: false), Row(nil, "sync.meguri.status.notSetUp.detail", .meguriSignIn)),
            (Facts(connection: .cancelled, signedInBefore: false), Row(nil, "sync.meguri.status.notSetUp.detail", .meguriSignIn)),
            (Facts(connection: .signedOut), Row(.needsYou, "sync.meguri.status.signedOut.detail", .meguriSignIn)),
            (Facts(connection: .cancelled), Row(.needsYou, "sync.meguri.status.signedOut.detail", .meguriSignIn)),
            (Facts(connection: .authenticating), Row(nil, nil)),
            (Facts(stage: .required), Row(nil, "sync.service.settingUp")),
            (Facts(stage: .preparing), Row(nil, "sync.meguri.setup.preparing", .meguriCancel)),
            (Facts(stage: .preview), Row(nil, nil)),
            (Facts(stage: .collection), Row(nil, nil)),
            (Facts(stage: .acceptanceUnknown), Row(.waiting, "sync.meguri.setup.acceptanceUnknown", .meguriTryAgain)),
            (Facts(stage: .mediaBlocked), Row(.issues, "sync.meguri.media.blocked", .meguriRetryMedia)),
            (Facts(stage: .acceptedLocalIncomplete, finishOffered: true),
             Row(.needsYou, "sync.needsYou.meguri.finishSetup", .meguriFinishSetup)),
            (Facts(stage: .acceptedLocalIncomplete), Row(.waiting, "sync.needsYou.meguri.finishSetup")),
            (Facts(stage: .failed, failureKey: "sync.meguri.setup.error.activationInterrupted", finishOffered: true),
             Row(.needsYou, "sync.needsYou.meguri.activation", .meguriFinishSetup)),
            (Facts(stage: .failed, failureKey: "sync.meguri.setup.error.activationInterrupted"),
             Row(.waiting, "sync.needsYou.meguri.activation")),
            (Facts(stage: .failed, failureKey: "sync.meguri.setup.error.authentication"),
             Row(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn)),
            (Facts(stage: .failed, failureKey: "sync.meguri.setup.error.identityChanged"),
             Row(.needsYou, "sync.needsYou.meguri.identityChanged", .meguriSignIn)),
            (Facts(stage: .failed, failureKey: "sync.meguri.setup.error.storage"),
             Row(.needsYou, "sync.needsYou.meguri.storage", .meguriTryAgain)),
            (Facts(stage: .failed, failureKey: MeguriSyncStatusPresentation.cancelledFailureKey), Row(nil, nil)),
            (Facts(), Row(.waiting, "sync.meguri.status.setupUnavailable")),
            (Facts(routine: .idle), Row(nil, nil)),
            (Facts(routine: .idle, automatic: false), Row(nil, "sync.status.automaticOff.detail")),
            (Facts(routine: .pending), Row(nil, "sync.status.pending.detail")),
            (Facts(routine: .exchanging), Row(nil, nil)),
            (Facts(routine: .mediaBlocked), Row(.issues, "sync.meguri.media.blocked", .meguriRetryMedia)),
            (Facts(routine: .authentication), Row(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn)),
            (Facts(routine: .acceptanceUnknown), Row(.waiting, "sync.meguri.round.acceptanceUnknown", .meguriTryAgain)),
            (Facts(routine: .fullSyncRequired), Row(.needsYou, "sync.needsYou.meguri.chooseCollection", .chooseCollection)),
            (Facts(routine: .network), Row(.waiting, "sync.meguri.round.network")),
            (Facts(routine: .server), Row(.waiting, "sync.meguri.round.server")),
            (Facts(routine: .failed), Row(.waiting, nil)),
            (Facts(routine: .cancelled), Row(nil, "sync.status.pending.detail"))
        ]
        for (facts, row) in rows { #expect(SyncKinds.meguriRow(facts) == row, "\(facts)") }
        for key in ["remoteUnavailable", "remoteChanged", "localChanged", "backupFailed", "server", "invalidDownload"] {
            #expect(SyncKinds.meguriRow(Facts(stage: .failed, failureKey: "sync.meguri.setup.error.\(key)"))
                == Row(.needsYou, "sync.needsYou.meguri.setupFailed", .meguriTryAgain))
        }
    }

    @Test func meguriMainStatusUsesTheSharedStatusWords() {
        let idle = SyncKinds.meguri(.init(routine: .idle))
        #expect(idle.titleKey == "sync.status.upToDate.title" && idle.kind == nil && idle.action == nil)
        let manual = SyncKinds.meguri(.init(routine: .idle, automatic: false))
        #expect(manual.titleKey == "sync.status.automaticOff.title")
        #expect(manual.detailKey == "sync.status.automaticOff.detail")
        #expect(SyncKinds.meguri(.init(routine: .pending)).titleKey == "sync.status.pending.title")
        #expect(SyncKinds.meguri(.init(routine: .exchanging)).titleKey == "sync.meguri.round.preparing.title")
        let needs = SyncKinds.meguri(.init(routine: .fullSyncRequired))
        #expect(needs.needsYou && needs.symbol == SyncKinds.needsYouSymbol && needs.action == .chooseCollection)
        // Waiting keeps a Try Again only on the two acceptance-unknown rows.
        #expect(SyncKinds.meguri(.init(routine: .acceptanceUnknown)).action == .meguriTryAgain)
        #expect(SyncKinds.meguri(.init(stage: .acceptanceUnknown)).action == .meguriTryAgain)
        #expect(SyncKinds.meguri(.init(routine: .network)).action == nil)
        #expect(SyncKinds.meguri(.init(routine: .network)).symbol != SyncKinds.needsYouSymbol)
        let failed = SyncKinds.meguri(.init(routine: .failed))
        #expect(failed.titleKey == "sync.meguri.status.failed.title" && failed.detailKey == nil)
        #expect(SyncText.value(failed.titleKey) == "Last Exchange Didn’t Finish")
        let fixture = SyncCoordinator.renderFixture(.meguriRoundFailed).mainStatus
        #expect(fixture.titleKey == failed.titleKey && fixture.detailKey == nil)
        // Media blocked is an issues row that opens Affected Files.
        let blocked = SyncKinds.meguri(.init(routine: .mediaBlocked))
        #expect(blocked.kind == .issues && blocked.issuesRowKey == "sync.meguri.media.notTransferred")
        #expect(blocked.detailKey == "sync.meguri.media.blocked" && blocked.action == nil)
        #expect(blocked.issues.first?.action == .meguriRetryMedia)
        #expect(blocked.symbol != SyncKinds.needsYouSymbol)
    }

    @Test func syncNowReasonsNameSyncSettings() {
        #expect(SyncText.value(SyncKinds.syncNowOffKey).contains("Sync settings"))
        let signIn = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: nil, routine: .authentication)
        #expect(signIn.route == .disabled("sync.meguri.now.unavailable.signIn.sync"))
        let stopped = MeguriSyncStatusPresentation(stopped: true, connection: .setupRequired, stage: nil, routine: .idle)
        #expect(stopped.route == .disabled("sync.meguri.now.unavailable.stopped.sync"))
        // T19.7: full sync required leads to the status action, not a Sync Now text of its own.
        let choose = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: nil, routine: .fullSyncRequired)
        #expect(choose.route == .account("sync.decision.action"))
    }

    // First-run report: a setup transfer is not a blocked provider (MP1, MP6.5).

    @Test func setupMediaWithAReadableProviderShowsTheTransferNotAStop() {
        // Core now reports the setup's own closed admission as setup required, not blocked.
        let media = SyncKinds.meguri(.init(connection: .setupRequired, stage: .media))
        #expect(!media.needsYou && media.detailKey == "sync.meguri.media.left.unknown")
        #expect(media.titleKey == "sync.meguri.status.transferringMedia.title")
        // Resume fix: files left to download name the direction, and the sentence carries the count.
        let downloading = SyncKinds.meguri(.init(connection: .setupRequired, stage: .media, mediaFilesLeft: 4_431))
        #expect(downloading.titleKey == "sync.meguri.status.downloadingMedia.title")
        #expect(downloading.detailKey == "sync.meguri.media.left" && downloading.detailCount == 4_431)
        let running = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .media, routine: nil)
        #expect(running.isBusy && !running.needsUser)
        // Unreadable metadata is the one needs-you sentence, and Sync Now says it stopped.
        let blocked = SyncKinds.meguri(.init(providerBlocked: true, connection: .setupRequired, stage: .media))
        #expect(blocked.needsYou && blocked.detailKey == "sync.needsYou.blocked" && blocked.action == .copyDebugInfo)
        let stopped = MeguriSyncStatusPresentation(stopped: true, connection: .setupRequired, stage: .media, routine: nil)
        #expect(!stopped.isBusy && stopped.needsUser && stopped.route == .disabled("sync.meguri.now.unavailable.stopped.sync"))
    }

    @Test func aBlockedProviderShowsNoTransferProgress() {
        #expect(MeguriSyncStatusPresentation.progress(busy: true, stopped: true, stage: .media,
            completed: 3, total: 9, mediaDone: 3, mediaAwaiting: 6) == nil)
        #expect(MeguriSyncStatusPresentation.progress(busy: false, stopped: false, stage: .media,
            completed: nil, total: nil, mediaDone: 3, mediaAwaiting: 6) == nil)
    }

    @Test func blockingStopsRunningMediaAndRoundsButNotACollectionStep() {
        typealias P = MeguriSyncStatusPresentation
        #expect(P.stopsWorkOnBlock(was: .setupRequired, now: .blocked, stage: .media, roundRunning: false))
        #expect(P.stopsWorkOnBlock(was: .setupRequired, now: .blocked, stage: nil, roundRunning: true))
        #expect(!P.stopsWorkOnBlock(was: .setupRequired, now: .blocked, stage: nil, roundRunning: false))
        for stage in [MeguriSetupStage.preparing, .collection, .preview, .required] {
            #expect(!P.stopsWorkOnBlock(was: .setupRequired, now: .blocked, stage: stage, roundRunning: true))
        }
        // Only the transition stops: a phase already blocked, or one that is not, does nothing.
        #expect(!P.stopsWorkOnBlock(was: .blocked, now: .blocked, stage: .media, roundRunning: true))
        #expect(!P.stopsWorkOnBlock(was: .setupRequired, now: .setupRequired, stage: .media, roundRunning: true))
        #expect(!P.stopsWorkOnBlock(was: .blocked, now: .setupRequired, stage: .media, roundRunning: true))
    }

    /// The media step's job reports each request's bytes with a total, and each
    /// media step with none. The indicator follows the step's files only, so
    /// those alternating reports never switch it between a bar and a spinner.
    @Test func mediaProgressIsOneValueAcrossBatches() throws {
        typealias P = MeguriSyncStatusPresentation
        // Before the first media count the step is indeterminate, whatever the job reports.
        #expect(P.progress(busy: true, stopped: false, stage: .media, completed: 10, total: 400,
            mediaDone: nil, mediaAwaiting: nil) == .indeterminate)
        let batches: [(completed: UInt64, total: UInt64?, done: UInt64, awaiting: UInt64)] = [
            (120, 4_096, 100, 28_680), (40, nil, 100, 28_680), (2_000, 8_192, 1_200, 27_580),
            (1_250, nil, 1_250, 27_530), (8_000, 8_000, 3_725, 25_055)]
        var last = -1.0
        for batch in batches {
            let value = P.progress(busy: true, stopped: false, stage: .media, completed: batch.completed,
                total: batch.total, mediaDone: batch.done, mediaAwaiting: batch.awaiting)
            let fraction = try #require(value?.fraction)
            #expect(fraction == Double(batch.done) / Double(batch.done + batch.awaiting))
            #expect(fraction >= last)
            last = fraction
        }
        // A step with no file to move stays indeterminate rather than dividing by zero.
        #expect(P.progress(busy: true, stopped: false, stage: .media, completed: nil, total: nil,
            mediaDone: 0, mediaAwaiting: 0) == .indeterminate)
        // Other steps keep their job's own total.
        #expect(P.progress(busy: true, stopped: false, stage: .collection, completed: 5, total: 10,
            mediaDone: nil, mediaAwaiting: nil) == .fraction(0.5))
        #expect(P.progress(busy: true, stopped: false, stage: .collection, completed: 5, total: nil,
            mediaDone: nil, mediaAwaiting: nil) == .indeterminate)
        #expect(P.progress(busy: true, stopped: false, stage: nil, completed: 12, total: 10,
            mediaDone: nil, mediaAwaiting: nil) == .fraction(1))
        #expect(MeguriProgressValue.indeterminate.fraction == nil)
    }

    // Rows that T19.4 does not list, kept at the least claim.



    @Test func everyActionHasItsCatalogTitle() {
        for action in SyncStatusAction.allCases { #expect(SyncText.value(action.titleKey) != action.titleKey) }
    }
}
