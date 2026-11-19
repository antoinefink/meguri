@testable import AppModel
@testable import CoreBridge
import Foundation
import SwiftUI
import Testing

@MainActor @Suite struct CloudSyncPresentationTests {
    @Test func meguriSignInErrorsSeparateLocalRecoveryFromAuthenticationAndNetwork() {
        let cases: [(any Error, String)] = [
            (MeguriSignInLocalFailure.busy, "busy"),
            (MeguriSignInLocalFailure.attachment, "localSetup"),
            (MeguriSignInLocalFailure.credentialStore, "credentialStore"),
            (BridgeError.busy(runningOperation: "synthetic operation"), "busy"),
            (MeguriProviderCredentialFailure.access(-1), "credentialStore"),
            (MeguriProviderCredentialFailure.malformedPayload, "credentialStore"),
            (MeguriProviderCredentialFailure.wrongConnection, "credentialStore"),
            (MeguriProviderAuthenticationFailure.username, "credentials"),
            (MeguriProviderAuthenticationFailure.credentials, "credentials"),
            (MeguriProviderAuthenticationFailure.network, "network"),
            (MeguriProviderAuthenticationFailure.redirectRefused, "redirect"),
            (MeguriProviderAuthenticationFailure.connectionChanged, "other"),
            (NSError(domain: "synthetic arbitrary error", code: 1), "other")
        ]
        for (error, suffix) in cases {
            let key = SyncCoordinator.meguriAuthenticationKey(error)
            #expect(key == "sync.meguri.error.\(suffix)")
            #expect(SyncText.value(key) != key)
            #expect(!SyncText.value(key).contains("synthetic"))
        }
        #expect(SyncText.value("sync.meguri.signIn.title") == "Sign In to Meguri Sync")
    }
    @Test func meguriSchedulingDeniesEveryPendingWriteButAllowsDurableManualOff() {
        let on = CloudPolicyDeviceSettings(enabled: true, paused: false, automatic: true, media: true)
        let off = CloudPolicyDeviceSettings(enabled: true, paused: false, automatic: false, media: true)
        let paused = CloudPolicyDeviceSettings(enabled: true, paused: true, automatic: true, media: true)
        let disabled = CloudPolicyDeviceSettings(enabled: false, paused: false, automatic: true, media: true)
        let mediaOff = CloudPolicyDeviceSettings(enabled: true, paused: false, automatic: true, media: false)
        let cases = [(on, true, true), (off, true, false), (paused, false, false),
                     (disabled, false, false), (mediaOff, true, true)]
        for (committed, manualAllowed, automaticAllowed) in cases {
            #expect(SyncCoordinator.meguriSchedulingAllowed(automatic: false,
                committed: committed, desired: nil) == manualAllowed)
            #expect(SyncCoordinator.meguriSchedulingAllowed(automatic: true,
                committed: committed, desired: nil) == automaticAllowed)
            // Includes identical writes, Pause, Resume and media-only writes.
            for desired in [on, off, paused, disabled, mediaOff] {
                for automatic in [false, true] {
                    #expect(!SyncCoordinator.meguriSchedulingAllowed(automatic: automatic,
                        committed: committed, desired: desired))
                }
            }
        }
    }
    @Test func meguriPendingWriteSuppressesEntryAndAvailabilityRetryUntilCommit() {
        let on = CloudPolicyDeviceSettings(enabled: true, paused: false, automatic: true, media: true)
        let off = CloudPolicyDeviceSettings(enabled: true, paused: false, automatic: false, media: true)
        // Use the production admission shared by enqueue, task entry and retry.
        // These settings inputs grant no owner, Ready binding or network job.
        #expect(SyncCoordinator.meguriSchedulingAllowed(automatic: true, committed: on, desired: nil))
        let wake = MeguriWakeAvailability(generation: 7)
        let snapshot = wake.revision
        wake.observe(kind: .available, sequence: 40, generation: 7)
        for automatic in [false, true] {
            for (committed, desired) in [(on, off), (off, on), (on, on)] {
                let allowed = SyncCoordinator.meguriSchedulingAllowed(automatic: automatic,
                    committed: committed, desired: desired)
                #expect(!allowed)
                #expect(!wake.consume(collectionOutcome: .refused(.busy), since: snapshot, allowed: allowed))
            }
        }
        let committedOn = SyncCoordinator.meguriSchedulingAllowed(automatic: true, committed: on, desired: nil)
        #expect(wake.consume(collectionOutcome: .refused(.busy), since: snapshot, allowed: committedOn))
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: snapshot, allowed: committedOn))
    }
    @Test func meguriSettlingOnlyDisablesTheOrdinaryRoundRoute() {
        let cases: [(Bool, MeguriProviderConnectionPhase, MeguriSetupStage?, Bool, MeguriRoutineState?)] = [
            (true, .setupRequired, nil, false, .idle),
            (false, .blocked, nil, false, .idle),
            (false, .setupRequired, nil, true, .idle),
            (false, .signedOut, nil, false, nil),
            (false, .setupRequired, .acceptanceUnknown, false, nil)
        ]
        let completed: Int64 = 1_700_000_000_000
        let now = Date(timeIntervalSince1970: Double(completed) / 1_000 + 60)
        for (stopped, connection, stage, paused, routine) in cases {
            let original = MeguriSyncStatusPresentation(stopped: stopped, connection: connection,
                stage: stage, localCollection: true, paused: paused, routine: routine, lastExchange: completed)
            let settling = MeguriSyncStatusPresentation(stopped: stopped, connection: connection,
                stage: stage, localCollection: true, paused: paused, routine: routine,
                lastExchange: completed, settling: true)
            #expect(settling.route == original.route)
            #expect(settling.titleKey == original.titleKey && settling.detailKey == original.detailKey)
            #expect(settling.detailLines(now: now) == original.detailLines(now: now))
        }
    }
    @Test func meguriAutomaticControlCannotCreateReadinessOrChangeProtectedSettings() {
        // Offline coordinator only; do not manufacture a Ready binding/service.
        for paused in [false, true] {
            let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
                configuration: nil, defaults: nil)
            sync.setDeviceSettings(paused: paused, automatic: false, media: true)
            let settings = sync.settings
            #expect(!sync.meguriAutomaticSettingVisible && !sync.meguriAutomaticSettingAvailable)
            sync.setMeguriAutomatic(true)
            sync.setMeguriAutomatic(false)
            #expect(sync.settings == settings && !sync.deviceSettingsSettling)
            #expect(sync.meguriPhase == .unconfigured && sync.meguriRoutine == nil && !sync.meguriBusy)
            #expect(sync.providerState == nil && sync.meguriLastCommand == nil && sync.meguriRoundReport == nil)
            #expect(sync.applyMeguriRoundCredentialFailure(MeguriProviderCredentialFailure.wrongConnection))
            #expect(!sync.meguriAutomaticSettingVisible && !sync.meguriAutomaticSettingAvailable)
            sync.setMeguriAutomatic(true)
            #expect(sync.settings == settings && sync.meguriPhase == .blocked && !sync.deviceSettingsSettling)
        }
    }
    @Test func meguriReplacementRowsRenderActualNumericValuesWithPlainLabels() {
        // These are display inputs, not an issued preview, receipt or confirmation.
        let rows = MeguriSyncStatusPresentation.replacementCountRows(cards: 237, notes: 184, reviews: 96)
        #expect(rows.map(\.label) == ["Cards", "Notes", "Reviews"])
        #expect(rows.map(\.value) == ["237", "184", "96"])
        let empty = MeguriSyncStatusPresentation.replacementCountRows(cards: 0, notes: 0, reviews: 0)
        #expect(empty.map(\.value) == ["0", "0", "0"])
        #expect(SyncText.value("sync.counts.cards") == "Cards")
        #expect(SyncText.value("sync.counts.notes") == "Notes")
        #expect(SyncText.value("sync.counts.reviews") == "Reviews")
    }
    @Test func meguriToolbarSymbolsFollowTheReportedPresentationAndBusyStages() {
        let attention = "exclamationmark.arrow.triangle.2.circlepath"
        let account = "person.crop.circle.badge.exclamationmark"
        let busy = "arrow.triangle.2.circlepath"
        let stages: [(MeguriSetupStage, String, Bool)] = [
            (.required, account, false), (.preview, account, false),
            (.preparing, busy, true), (.collection, busy, true), (.media, busy, true),
            (.mediaBlocked, attention, false), (.acceptanceUnknown, attention, false),
            (.acceptedLocalIncomplete, attention, false), (.failed, attention, false)
        ]
        for (stage, symbol, isBusy) in stages {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: stage, localCollection: true, paused: false, routine: nil)
            #expect(presentation.symbol == symbol)
            #expect(presentation.isBusy == isBusy)
        }
        let rounds: [(MeguriRoutineState, String, Bool)] = [
            (.unreported, attention, false), (.exchanging, busy, true),
            (.idle, "checkmark.circle", false), (.pending, "arrow.up.circle", false),
            (.waiting, "arrow.up.circle", false),
            (.mediaBlocked, attention, false), (.authentication, account, false),
            (.acceptanceUnknown, attention, false), (.fullSyncRequired, account, false),
            (.network, attention, false), (.server, attention, false),
            (.cancelled, attention, false), (.failed, attention, false)
        ]
        for (round, symbol, isBusy) in rounds {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: round)
            #expect(presentation.symbol == symbol)
            #expect(presentation.isBusy == isBusy)
        }
        let connections: [(MeguriProviderConnectionPhase, String, Bool)] = [
            (.unconfigured, account, false), (.signedOut, account, false),
            (.cancelled, account, false), (.authenticating, busy, true),
            (.setupRequired, account, false), (.blocked, attention, false)
        ]
        for (connection, symbol, isBusy) in connections {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: connection,
                stage: nil, localCollection: true, paused: false, routine: nil)
            #expect(presentation.symbol == symbol)
            #expect(presentation.isBusy == isBusy)
        }
        let paused = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: nil, localCollection: true, paused: true, routine: .pending)
        #expect(paused.symbol == "pause.circle" && !paused.isBusy)
        let stopped = MeguriSyncStatusPresentation(stopped: true, connection: .authenticating,
            stage: .preparing, localCollection: true, paused: false, routine: .exchanging)
        #expect(stopped.symbol == attention && !stopped.isBusy)
        let blocked = MeguriSyncStatusPresentation(stopped: false, connection: .blocked,
            stage: .media, localCollection: true, paused: false, routine: .exchanging)
        #expect(blocked.symbol == attention && !blocked.isBusy)
    }
    @Test func meguriSyncNowUsesRecoveryPauseAndSetupBeforeAnyRound() {
        let stopped = MeguriSyncStatusPresentation(stopped: true, connection: .setupRequired,
            stage: .required, localCollection: true, paused: true, routine: .idle)
        #expect(stopped.titleKey == "sync.meguri.status.stopped.title")
        #expect(stopped.detailKey == nil)
        #expect(stopped.route == .disabled("sync.meguri.now.unavailable.stopped"))
        let blocked = MeguriSyncStatusPresentation(stopped: false, connection: .blocked,
            stage: nil, localCollection: true, paused: true, routine: .acceptanceUnknown)
        #expect(blocked.route == .disabled("sync.meguri.now.unavailable.stopped"))
        let paused = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: .acceptanceUnknown, localCollection: true, paused: true, routine: nil)
        #expect(paused.route == .disabled("sync.meguri.now.unavailable.paused"))
        // A setup's unknown acceptance opens the explicit retained recovery.
        // A routine's unknown acceptance cannot start a writer.
        let setup = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: .acceptanceUnknown, localCollection: true, paused: false, routine: nil)
        #expect(setup.titleKey == "sync.meguri.status.acceptanceUnknown.title")
        #expect(setup.route == .account("sync.meguri.setup.continue"))
        let round = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: nil, localCollection: true, paused: false, routine: .acceptanceUnknown)
        #expect(round.route == .disabled("sync.meguri.now.unavailable.acceptanceUnknown"))
        #expect(round.detailKey == "sync.meguri.round.acceptanceUnknown")
    }
    @Test func meguriRoutineOutcomesDoNotTurnFailuresIntoIdleOrSuccess() {
        let cases: [(MeguriRoutineState, String, String?, MeguriSyncNowRoute)] = [
            (.idle, "on", nil, .round),
            (.pending, "pending", "sync.meguri.status.pending.detail", .round),
            (.waiting, "pending", nil, .round),
            (.fullSyncRequired, "chooseCollection", "sync.meguri.round.fullSyncRequired", .account("sync.meguri.now.chooseCollection")),
            (.mediaBlocked, "mediaBlocked", "sync.meguri.media.blocked", .account("sync.meguri.media.showAffected")),
            (.authentication, "signInRequired", "sync.meguri.status.signInRequired.detail", .disabled("sync.meguri.now.unavailable.signIn")),
            (.exchanging, "syncingCollection", nil, .disabled(nil)),
            (.network, "failed", "sync.meguri.round.network", .round),
            (.server, "failed", "sync.meguri.round.server", .round),
            (.cancelled, "failed", "sync.meguri.round.cancelled", .round),
            (.failed, "stopped", nil, .disabled(nil)),
            (.unreported, "stopped", nil, .round),
        ]
        for (state, title, detail, route) in cases {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: state)
            #expect(presentation.titleKey == "sync.meguri.status.\(title).title")
            #expect(presentation.detailKey == detail)
            #expect(presentation.route == route)
            let settling = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: state, settling: true)
            #expect(settling.route == (route == .round ? .disabled(nil) : route))
            #expect(settling.titleKey == presentation.titleKey && settling.detailKey == presentation.detailKey)
            #expect(settling.symbol == presentation.symbol && settling.isBusy == presentation.isBusy)
        }
    }
    @Test func reviewShowsSyncStatusOnlyWhenSyncNeedsThePerson() {
        // N16.2. iCloud: a decision, an account or a recorded problem.
        let needing: [CloudPolicyStatusState] = [.needsAttention, .paused(.decisionRequired),
            .iCloudUnavailable(.noAccount), .iCloudUnavailable(.restricted), .iCloudUnavailable(.setupNeeded)]
        for state in needing { #expect(SyncStatusPresentation.needsUser(state), "\(state)") }
        let quiet: [CloudPolicyStatusState?] = [nil, .localOnly, .iCloudUnavailable(.temporary), .paused(.userPaused),
            .paused(.automaticOff), .offline, .syncingCollection, .syncingMedia, .changesPending,
            .upToDate(asOfMilliseconds: 1), .awaitingEvidence]
        for state in quiet { #expect(!SyncStatusPresentation.needsUser(state), "\(String(describing: state))") }
        // Meguri Sync: by routine state once set up.
        let routines: [(MeguriRoutineState, Bool)] = [(.unreported, false), (.exchanging, false), (.idle, false),
            (.pending, false), (.waiting, false), (.network, false), (.server, false), (.cancelled, false),
            (.failed, false), (.mediaBlocked, true), (.authentication, true), (.acceptanceUnknown, true),
            (.fullSyncRequired, true)]
        for (routine, expected) in routines {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: routine)
            #expect(presentation.needsUser == expected, "\(routine)")
        }
        let paused = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: nil,
            localCollection: true, paused: true, routine: .idle)
        #expect(!paused.needsUser)
        let signedOut = MeguriSyncStatusPresentation(stopped: false, connection: .signedOut, stage: nil,
            localCollection: true, paused: false, routine: nil)
        #expect(signedOut.needsUser)
        let choosing = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .required,
            localCollection: true, paused: false, routine: nil)
        #expect(choosing.needsUser)
        let transferring = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: .collection,
            localCollection: true, paused: false, routine: nil)
        #expect(!transferring.needsUser)
    }
    @Test func meguriLastExchangeDetailsFollowRoutineAndStatusPrecedence() {
        // Presentation inputs only; no invented ready binding or server result.
        let completed: Int64 = 1_700_000_000_000
        let now = Date(timeIntervalSince1970: Double(completed) / 1_000 + 60)
        let exchange = SyncText.format("sync.meguri.lastExchange", SyncText.instant(completed, now: now))
        let pending = SyncText.value("sync.meguri.status.pending.detail")
        for routine in [MeguriRoutineState.idle, .pending] {
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: routine, lastExchange: completed)
            #expect(presentation.detailLines(now: now) == (routine == .idle ? [exchange] : [pending, exchange]))
            #expect(presentation.route == .round && !presentation.isBusy)
            let settling = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: routine,
                lastExchange: completed, settling: true)
            #expect(settling.route == .disabled(nil))
            #expect(settling.detailLines(now: now) == presentation.detailLines(now: now))
            let unavailable = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: routine)
            #expect(unavailable.detailLines(now: now) == (routine == .idle ? [] : [pending]))
            var precedence: [(Bool, MeguriProviderConnectionPhase, MeguriSetupStage?, Bool)] = [
                (true, .setupRequired, nil, false), (false, .blocked, nil, false),
                (false, .unconfigured, nil, false), (false, .signedOut, nil, false),
                (false, .cancelled, nil, false), (false, .authenticating, nil, false),
                (false, .setupRequired, nil, true)
            ]
            let stages: [MeguriSetupStage] = [.required, .preparing, .preview, .collection, .media,
                .mediaBlocked, .acceptanceUnknown, .acceptedLocalIncomplete, .failed]
            for stage in stages { precedence.append((false, .setupRequired, stage, false)) }
            for (stopped, connection, stage, paused) in precedence {
                let hidden = MeguriSyncStatusPresentation(stopped: stopped, connection: connection,
                    stage: stage, localCollection: true, paused: paused, routine: routine, lastExchange: completed)
                let expected = hidden.detailKey.map { [SyncText.value($0)] } ?? []
                #expect(hidden.detailLines(now: now) == expected)
            }
        }
        let otherRoutines: [MeguriRoutineState?] = [nil, .unreported, .exchanging, .waiting, .mediaBlocked,
            .authentication, .acceptanceUnknown, .fullSyncRequired, .network, .server, .cancelled, .failed]
        for routine in otherRoutines {
            let hidden = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: routine, lastExchange: completed)
            let expected = hidden.detailKey.map { [SyncText.value($0)] } ?? []
            #expect(hidden.detailLines(now: now) == expected)
        }
    }
    @Test func meguriOnlyBusyStartRefusalWaitsWithoutClaimingUnsentChanges() {
        // Exercise the same typed-outcome classifier used at both production
        // phase starts, without manufacturing a ready binding or server result.
        let outcomes: [(MeguriRoundPhaseOutcome, MeguriRoutineState)] = [
            (.refused(.busy), .waiting), (.pending, .failed),
            (.refused(.notSelected), .failed), (.refused(.paused), .failed),
            (.refused(.notReady), .failed), (.refused(.bindingChanged), .failed),
            (.refused(.connectionChanged), .failed), (.refused(.accountChanged), .failed),
            (.refused(.publicationChanged), .failed), (.refused(.interrupted), .failed),
            (.refused(.storage), .failed)
        ]
        for (outcome, expected) in outcomes {
            let state = SyncCoordinator.meguriUnstartedPhaseState(outcome)
            #expect(state == expected)
            let presentation = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
                stage: nil, localCollection: true, paused: false, routine: state)
            #expect(!presentation.isBusy && presentation.detailKey == nil)
            if expected == .waiting {
                #expect(presentation.titleKey == "sync.meguri.status.pending.title")
                #expect(SyncText.value(presentation.titleKey) == "Waiting to Sync")
                #expect(presentation.route == .round)
            } else {
                #expect(presentation.titleKey == "sync.meguri.status.stopped.title")
                #expect(presentation.route == .disabled(nil))
            }
        }
        let safeguards: [(Bool, MeguriProviderConnectionPhase, Bool, MeguriSyncNowRoute)] = [
            (true, .setupRequired, false, .disabled("sync.meguri.now.unavailable.stopped")),
            (false, .blocked, false, .disabled("sync.meguri.now.unavailable.stopped")),
            (false, .setupRequired, true, .disabled("sync.meguri.now.unavailable.paused"))
        ]
        for (stopped, connection, paused, route) in safeguards {
            let presentation = MeguriSyncStatusPresentation(stopped: stopped, connection: connection,
                stage: nil, localCollection: true, paused: paused, routine: .waiting)
            #expect(presentation.route == route)
        }
    }
    @Test func meguriInFlightAvailabilityReplaysBusyCollectionOnlyOnceInTheOriginalWake() {
        // Exercise the production wake record with local start outcomes only;
        // no Ready binding, collection terminal or server result is manufactured.
        let wake = MeguriWakeAvailability(generation: 7)
        let beforeFullRound = wake.revision
        // The slot released, but the refused start has not returned to Swift.
        wake.observe(kind: .available, sequence: 40, generation: 7)
        wake.observe(kind: .available, sequence: 41, generation: 7)
        #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        let beforeAdditionalRound = wake.revision
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeAdditionalRound, allowed: true))
        wake.observe(kind: .available, sequence: 42, generation: 7)
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeAdditionalRound, allowed: true))
        // A later wake has its own budget and cannot inherit this observation.
        let laterWake = MeguriWakeAvailability(generation: 7)
        #expect(!laterWake.consume(collectionOutcome: .refused(.busy), since: laterWake.revision, allowed: true))
    }
    @Test func meguriAvailabilityMustBeNewDuringThisAttemptAndFromItsGeneration() {
        let wake = MeguriWakeAvailability(generation: 7)
        wake.observe(kind: .available, sequence: 40, generation: 7)
        let beforeFullRound = wake.revision
        wake.observe(kind: .available, sequence: 100, generation: 8)
        wake.observe(kind: .available, sequence: 101, generation: 6)
        wake.observe(kind: .available, sequence: 40, generation: 7)
        wake.observe(kind: .available, sequence: 39, generation: 7)
        #expect(wake.revision == beforeFullRound)
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        wake.observe(kind: .available, sequence: 41, generation: 7)
        #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
    }
    @Test func meguriAvailabilityCannotReplayOtherStartOutcomesOrBypassAdmission() {
        let outcomes: [MeguriRoundPhaseOutcome] = [.pending,
            .refused(.notSelected), .refused(.paused), .refused(.notReady), .refused(.bindingChanged),
            .refused(.connectionChanged), .refused(.accountChanged), .refused(.publicationChanged),
            .refused(.interrupted), .refused(.storage)]
        for outcome in outcomes {
            let wake = MeguriWakeAvailability(generation: 7)
            let beforeFullRound = wake.revision
            wake.observe(kind: .available, sequence: 40, generation: 7)
            #expect(!wake.consume(collectionOutcome: outcome, since: beforeFullRound, allowed: true))
        }
        let wake = MeguriWakeAvailability(generation: 7)
        let beforeFullRound = wake.revision
        wake.observe(kind: .available, sequence: 40, generation: 7)
        // The caller combines the unchanged owner/attempt/facade guards with
        // automatic consent, readiness, edit, pause and cancellation admission.
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: false))
        #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
    }
    @Test func meguriCancelledWakeCannotConsumeAvailability() async {
        let wake = MeguriWakeAvailability(generation: 7)
        let beforeFullRound = wake.revision
        wake.observe(kind: .available, sequence: 40, generation: 7)
        await Task { @MainActor in
            withUnsafeCurrentTask { $0?.cancel() }
            #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        }.value
    }
    @Test func meguriSessionEndedAlonePreservesOneBusyCollectionRetry() {
        let wake = MeguriWakeAvailability(generation: 7)
        wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 40, generation: 7)
        let beforeFullRound = wake.revision
        wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 100, generation: 8)
        wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 101, generation: 6)
        wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 40, generation: 7)
        wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 39, generation: 7)
        #expect(wake.revision == beforeFullRound)
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        // Real review admission released while the original Busy is returning;
        // no lifecycle slot release or Available event is required.
        wake.observe(kind: .sessionEnded(sessionId: 2), sequence: 41, generation: 7)
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: false))
        #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        let beforeAdditionalRound = wake.revision
        wake.observe(kind: .sessionEnded(sessionId: 3), sequence: 42, generation: 7)
        #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeAdditionalRound, allowed: true))
    }
    @Test func meguriReleaseSignalsShareOneRetryBudget() {
        let orders: [[CoreEvent.Kind]] = [
            [.available, .sessionEnded(sessionId: 1)],
            [.sessionEnded(sessionId: 1), .available]
        ]
        for order in orders {
            // Cover both coalesced signals and the other signal arriving only
            // during the retry. Neither order creates a second retry budget.
            for coalesced in [false, true] {
                let wake = MeguriWakeAvailability(generation: 7)
                let beforeFullRound = wake.revision
                wake.observe(kind: order[0], sequence: 40, generation: 7)
                if coalesced { wake.observe(kind: order[1], sequence: 41, generation: 7) }
                #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
                let beforeAdditionalRound = wake.revision
                wake.observe(kind: order[1], sequence: 42, generation: 7)
                #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeAdditionalRound, allowed: true))
            }
        }
    }
    @Test func meguriUnrelatedEventsCannotSpendBusyCollectionRetry() {
        let changes = ChangeSet(wholeCollection: false, source: .userUndoable,
            cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [],
            revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
            skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)
        let unrelated: [CoreEvent.Kind] = [.changes(changes), .sessionStarted(sessionId: 1, deckId: 1),
            .waitingUntil(sessionId: 1, instant: 0), .mediaArrived(names: []),
            .lifecycle(.opening), .collectionReplaced(generation: 7), .maintenanceDeadline(at: nil),
            .rolledOver(oldDay: 1, newDay: 2), .timeZoneChanged(zoneID: "UTC"), .nextDayAtChanged(nextDayAt: 0)]
        for kind in unrelated {
            let wake = MeguriWakeAvailability(generation: 7)
            let beforeFullRound = wake.revision
            wake.observe(kind: kind, sequence: 40, generation: 7)
            #expect(wake.revision == beforeFullRound)
            #expect(!wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
            wake.observe(kind: .sessionEnded(sessionId: 1), sequence: 41, generation: 7)
            #expect(wake.consume(collectionOutcome: .refused(.busy), since: beforeFullRound, allowed: true))
        }
    }
    @Test func meguriLocalRoundCredentialFailuresOfferExistingConnectionRecovery() {
        // Exercise the production local transition without fabricating a ready
        // binding or a server result. ProviderRoundOwnerTests covers admission.
        let cases: [(any Error, MeguriProviderConnectionPhase, String, MeguriSyncNowRoute)] = [
            (MeguriRoundAdmissionFailure.credentialsMissing, .signedOut, "signedOut", .account("sync.meguri.setup.continue")),
            (MeguriProviderCredentialFailure.access(-25308), .blocked, "connectionBlocked", .disabled("sync.meguri.now.unavailable.stopped")),
            (MeguriProviderCredentialFailure.malformedPayload, .blocked, "connectionBlocked", .disabled("sync.meguri.now.unavailable.stopped")),
            (MeguriProviderCredentialFailure.wrongConnection, .blocked, "connectionBlocked", .disabled("sync.meguri.now.unavailable.stopped"))
        ]
        for (error, phase, status, route) in cases {
            let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
                configuration: nil, defaults: nil)
            let settings = sync.settings
            #expect(sync.applyMeguriRoundCredentialFailure(error))
            #expect(sync.meguriPhase == phase)
            #expect(sync.meguriRoutine == nil && sync.meguriSignInErrorKey == nil)
            #expect(sync.meguriStatus.titleKey == "sync.meguri.status.\(status).title")
            #expect(sync.meguriStatus.detailKey == "sync.meguri.status.\(status).detail")
            #expect(sync.meguriStatus.route == route && !sync.meguriStatus.isBusy)
            #expect(sync.settings.enabled == settings.enabled && sync.settings.paused == settings.paused)
            #expect(sync.settings.automatic == settings.automatic && sync.settings.media == settings.media)
            sync.syncNow(); sync.retryMeguriRoundMedia()
            #expect(!sync.meguriBusy && sync.meguriRoundReport == nil && sync.meguriLastCommand == nil)
        }
        #expect(SyncText.value("sync.meguri.status.signedOut.detail") == "Sign in to your Meguri Sync server to continue.")
        #expect(SyncText.value("sync.meguri.connection.retry") == "Try Again")
    }
    @Test func meguriLocalRoundCredentialRecoveryDoesNotClassifyOtherErrorsOrEditedAddresses() {
        let errors: [any Error] = [MeguriRoundAdmissionFailure.connectionUnavailable,
            MeguriRoundAdmissionFailure.retired, MeguriRoundAdmissionFailure.busy,
            MeguriBootstrapAdmissionFailure.credentialsMissing, MeguriProviderAuthenticationFailure.credentials,
            BridgeError.internal("Synthetic failure"), CancellationError()]
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
            configuration: nil, defaults: nil)
        for error in errors {
            #expect(!sync.applyMeguriRoundCredentialFailure(error))
            #expect(sync.meguriPhase == .unconfigured && sync.meguriRoutine == nil)
        }
        sync.meguriAddressEdited()
        #expect(!sync.applyMeguriRoundCredentialFailure(MeguriRoundAdmissionFailure.credentialsMissing))
        #expect(!sync.applyMeguriRoundCredentialFailure(MeguriProviderCredentialFailure.wrongConnection))
        #expect(sync.meguriPhase == .unconfigured)
    }
    @Test func meguriCancelledRoundCannotPublishLocalCredentialRecovery() async {
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
            configuration: nil, defaults: nil)
        await Task { @MainActor in
            withUnsafeCurrentTask { $0?.cancel() }
            #expect(!sync.applyMeguriRoundCredentialFailure(MeguriRoundAdmissionFailure.credentialsMissing))
            #expect(!sync.applyMeguriRoundCredentialFailure(MeguriProviderCredentialFailure.malformedPayload))
        }.value
        #expect(sync.meguriPhase == .unconfigured && sync.meguriRoutine == nil)
    }
    @Test func meguriSetupPresentationCannotGrantOpaqueRecoveryOrReadyAuthority() {
        let local = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: .required, localCollection: true, paused: false, routine: nil)
        let download = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: .required, localCollection: false, paused: false, routine: nil)
        #expect(local.detailKey == "sync.meguri.setup.choose")
        #expect(download.detailKey == "sync.meguri.setup.download.detail")
        let accepted = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired,
            stage: .acceptedLocalIncomplete, localCollection: true, paused: false, routine: nil)
        #expect(accepted.titleKey == "sync.meguri.status.acceptedLocalIncomplete.title")
        #expect(accepted.route == .account("sync.meguri.setup.continue"))
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
            configuration: nil, defaults: nil)
        #expect(!sync.meguriCanFinish && !sync.meguriCanChoose && !sync.meguriCanRetryMedia)
        sync.confirmMeguriPreview(); sync.finishMeguriSetup(); sync.retryMeguriMedia(); sync.syncNow()
        #expect(!sync.meguriBusy && sync.meguriLastCommand == nil && sync.meguriRoundReport == nil)
        #expect(sync.meguriRoutine == nil && sync.meguriPreview == nil)
    }
    @Test func meguriT18CopyKeepsCollectionMediaAndUnknownEffectsDistinct() {
        #expect(SyncText.value("sync.service.none.footer") == "Choose a service to sync this collection.")
        #expect(SyncText.value("sync.meguri.signIn.footer") == "Signing in doesn’t send or change your collection.")
        #expect(SyncText.value("sync.meguri.signOut.failed") == "Sign-out didn’t finish.")
        #expect(SyncText.value("sync.meguri.setup.collectionDone") == "The collection transferred. Syncing starts when the media finishes.")
        #expect(SyncText.value("sync.meguri.setup.finish") == "Finish Setup")
        #expect(SyncText.value("sync.meguri.setup.useDevice") == "Use This Device’s Collection…")
        #expect(SyncText.value("sync.meguri.setup.useServer") == "Use Server Collection…")
        #expect(SyncText.format("sync.meguri.media.filesLeft", Int64(1)) == "1 media file left")
        #expect(SyncText.format("sync.meguri.media.filesLeft", Int64(2)) == "2 media files left")
        #expect(SyncText.format("sync.meguri.media.notTransferred", Int64(1)) == "1 media file hasn’t transferred.")
        #expect(SyncText.format("sync.meguri.media.remotePending", Int64(2)) == "2 files on the server haven’t reached this device.")
        #expect(SyncText.format("sync.switch.heldBack", Int64(1), "iCloud Sync") == "1 review answer on this device hasn’t been sent to iCloud Sync yet.")
        #expect(SyncText.format("sync.switch.pending", Int64(2), "Meguri Sync") == "2 changes on this device haven’t been sent to Meguri Sync.")
        #expect(SyncText.format("sync.switch.media", Int64(1), "Meguri Sync") == "1 media file hasn’t synced with Meguri Sync.")
        #expect(MeguriSyncStatusPresentation.failureKey(.storage) == "sync.meguri.setup.error.storage")
        #expect(MeguriSyncStatusPresentation.failureKey(.operationMismatch) == nil)
    }
    @Test func unknownEvidenceAndSetupCannotClaimUpToDate() {
        let unknown = SyncStatusPresentation(state: .awaitingEvidence, setup: .attached)
        #expect(unknown.detailKey == "sync.counts.unknown")
        #expect(unknown.symbol != "checkmark.icloud")
        let setup = SyncStatusPresentation(state: nil, setup: .configurationUnavailable)
        #expect(setup.detailKey == "sync.status.unavailable.setupNeeded")
        #expect(SyncStatusPresentation.availability(nil, setup: .attached) == .unavailable(.collectionUnavailable))
        #expect(SyncStatusPresentation.availability(nil, setup: .configurationUnavailable) == .unavailable(.iCloudUnavailable))
        let projections: [CloudPolicySyncNowAvailability] = [
            .available, .unavailable(.decisionRequired), .unavailable(.reviewActive), .unavailable(.collectionUnavailable)
        ]
        for projected in projections {
            #expect(SyncStatusPresentation.availability(projected, setup: .configurationUnavailable) == projected)
        }
    }
    @Test func reviewCopyUsesCoreEligibilityEvenWhileDisplayEvidenceIsUnknown() {
        let controls = CloudPolicyControls(enable: .pause, bindingUsable: true, bindingActive: true,
            bindingStopped: false, decisionRequired: false, userPaused: false, automaticEligible: true)
        let display = SyncStatusPresentation(state: .awaitingEvidence, setup: .attached)
        #expect(display.detailKey == "sync.counts.unknown")
        #expect(SyncStatusPresentation.reviewAvailabilityKey(controls) == "sync.now.unavailable.reviewActive")
        #expect(SyncStatusPresentation.reviewAvailabilityKey(.unknown) == "sync.now.unavailable.reviewActive.manual")
        #expect(SyncStatusPresentation.reviewAvailabilityKey(nil) == "sync.now.unavailable.reviewActive.manual")
        #expect(SyncStatusPresentation.reviewAvailabilityKey(.init(enable: .pause, bindingUsable: true,
            bindingActive: true, automaticEligible: false)) == "sync.now.unavailable.reviewActive.manual")
        #expect(CloudPolicyControls.unknown.bindingUsable == nil)
        #expect(CloudPolicyControls.unknown.enable == nil)
        #expect(CloudPolicyControls.unknown.automaticEligible == nil)
    }
    @Test func onlyBackendOfferedDirectionsCanBeConfirmed() {
        #expect(SyncStatusPresentation.permitsPublication(.confirmUpload))
        #expect(!SyncStatusPresentation.permitsAdoption(.confirmUpload))
        #expect(SyncStatusPresentation.permitsAdoption(.confirmAdoption))
        #expect(!SyncStatusPresentation.permitsPublication(.confirmAdoption))
        #expect(SyncStatusPresentation.permitsPublication(.chooseCollection))
        #expect(SyncStatusPresentation.permitsAdoption(.chooseCollection))
        #expect(!SyncStatusPresentation.permitsPublication(.cloudUnreadable))
        #expect(!SyncStatusPresentation.permitsAdoption(.cloudUnreadable))
    }
    @Test func publicationRequiresTheExplicitPreviewOrNormalConnectUploadPage() {
        let cases: [(CloudConnectionDecision, Bool, Bool)] = [
            (.confirmUpload, true, true), (.chooseCollection, true, false),
            (.confirmAdoption, false, false), (.cloudUnreadable, false, false),
            (.noCollection, false, false), (.confirmResume, false, false)
        ]
        for (decision, replacement, connect) in cases {
            #expect(SyncStatusPresentation.permitsPublication(decision, on: .replaceCloud) == replacement)
            #expect(SyncStatusPresentation.permitsPublication(decision, on: .connect) == connect)
            // The choice entry only navigates; it cannot itself confirm publication.
            #expect(!SyncStatusPresentation.permitsPublication(decision, on: .publicationChoice))
            #expect(!SyncStatusPresentation.permitsPublication(decision, on: .replaceLocal))
        }
        #expect(SyncText.value("sync.decision.replace").hasSuffix("…"))
    }
    @Test func publicationReadinessRequiresAvailableAccountAndIdleUnpausedPreview() {
        let allowed: [CloudConnectionDecision] = [.confirmUpload, .chooseCollection]
        for decision in allowed {
            #expect(SyncStatusPresentation.publicationReady(decision, accountAvailable: true, paused: false, busy: false))
            #expect(!SyncStatusPresentation.publicationReady(decision, accountAvailable: false, paused: false, busy: false))
            #expect(!SyncStatusPresentation.publicationReady(decision, accountAvailable: true, paused: true, busy: false))
            #expect(!SyncStatusPresentation.publicationReady(decision, accountAvailable: true, paused: false, busy: true))
        }
        // No retained ready preview, an adoption, or unreadable cloud cannot publish.
        let refused: [CloudConnectionDecision?] = [nil, .confirmAdoption, .cloudUnreadable, .noCollection, .confirmResume]
        for decision in refused {
            #expect(!SyncStatusPresentation.publicationReady(decision, accountAvailable: true, paused: false, busy: false))
        }
    }
    @Test func resumeIsAnExplicitDirectionAndOtherChoicesRemainSeparate() {
        let decisions: [CloudConnectionDecision] = [.noCollection, .confirmUpload, .confirmAdoption,
            .confirmResume, .chooseCollection, .cloudUnreadable]
        for decision in decisions {
            #expect(SyncStatusPresentation.permitsResume(decision) == (decision == .confirmResume))
            #expect(SyncStatusPresentation.permits(.resume, decision: decision) == (decision == .confirmResume))
            #expect(SyncStatusPresentation.permitsPublication(decision)
                == (decision == .confirmUpload || decision == .chooseCollection))
            #expect(SyncStatusPresentation.permitsAdoption(decision)
                == (decision == .confirmAdoption || decision == .chooseCollection))
        }
        #expect(SyncStatusPresentation.connectionErrorKey(.accountMismatch) == "sync.recovery.error.accountMismatch")
        #expect(SyncStatusPresentation.connectionErrorKey(.accountUnavailable) == "sync.recovery.error.accountUnavailable")
        let failures: [CloudConnectionFailure] = [.wrongOwner, .consumed, .closed, .notOffered]
        for failure in failures {
            #expect(SyncStatusPresentation.connectionErrorKey(failure) == "sync.recovery.error.notOffered")
        }
        #expect(SyncText.value("sync.connect.continue.title") == "Continue Syncing This Collection?")
        #expect(SyncText.value("sync.connect.continue.confirm") == "Continue Syncing")
        #expect(SyncText.value("sync.connect.downloading") == "Downloading your collection from iCloud…")
        #expect(SyncText.format("sync.connect.continue.message", Int64(1)) == "1 unsent change on this device will sync.")
        #expect(SyncText.format("sync.connect.continue.message", Int64(2)) == "2 unsent changes on this device will sync.")
    }
    @Test func mobileAccountRoutePreservesTheReviewAndReturnsToTheSameSession() {
        final class RetainedSession {}
        let session = RetainedSession()
        let navigation = AppNavigation()
        navigation.openDeckPage(DeckID(42))
        navigation.startReview(.init(deck: DeckID(42), session: session))
        let section = navigation.section
        let deck = navigation.homeDeckPage
        #expect(navigation.openAccountSync())
        #expect(navigation.sheet == .accountSync)
        #expect(navigation.section == section && navigation.homeDeckPage == deck)
        #expect(navigation.review?.session === session)
        navigation.dismissSheet()
        #expect(navigation.review?.session === session)
        #expect(navigation.columnVisibility == .detailOnly)
    }
    @Test func accountRouteCannotReplaceAnOwnedFileSheet() {
        let navigation = AppNavigation()
        navigation.present(.collectionEntry)
        #expect(!navigation.openAccountSync())
        #expect(navigation.sheet == .collectionEntry)
        navigation.dismissSheet()
        #expect(navigation.openAccountSync())
        #expect(navigation.section == .settings)
        #expect(navigation.path(.settings) == [.settingsPage(.accountSync)])
    }

    @Test func compactStatusWaitsForNativeDismissalAndConsumesOriginalEntryAndHostOnce() {
        for entry in [CloudSyncEntry.connect, .publicationChoice, .replaceLocal, .replaceCloud] {
            let owner = handoffCollection()
            let presentation = owner.syncPresentation
            owner.queueCloudSyncAfterStatusDismissal(entry)
            #expect(owner.navigation.sheet == nil && !presentation.statusHandoffPending)
            presentation.connectionPath = [.replaceLocal]
            presentation.statusPresented = true
            let id = presentation.statusPresentationID
            owner.queueCloudSyncAfterStatusDismissal(entry, host: .settings)
            // A second action during dismissal cannot overwrite the user's choice.
            owner.queueCloudSyncAfterStatusDismissal(.replaceCloud, host: .main)
            #expect(!presentation.statusPresented && presentation.statusHandoffPending)
            #expect(owner.navigation.sheet == nil)
            #expect(presentation.connectionPath == [.replaceLocal])
            owner.compactSyncStatusDidDismiss(UUID())
            #expect(owner.navigation.sheet == nil && presentation.statusHandoffPending)
            owner.compactSyncStatusDidDismiss(id)
            #expect(owner.navigation.sheet == .cloudSync(entry))
            #expect(owner.navigation.sheetHost == .settings)
            #expect(presentation.connectionPath.isEmpty && !presentation.statusHandoffPending)
            owner.navigation.dismissSheet()
            owner.compactSyncStatusDidDismiss(id)
            #expect(owner.navigation.sheet == nil)
        }
    }

    @Test func cancelledOrSwipedStatusCannotPresentLaterAndOldCallbackCannotConsumeReopenedRequest() {
        let owner = handoffCollection()
        let presentation = owner.syncPresentation
        presentation.statusPresented = true
        let swipedID = presentation.statusPresentationID
        presentation.statusPresented = false // Native swipe with no entry selected.
        owner.compactSyncStatusDidDismiss(swipedID)
        #expect(owner.navigation.sheet == nil)
        presentation.statusPresented = true
        let cancelledID = presentation.statusPresentationID
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        presentation.cancelStatusHandoff() // Also used when available size class changes.
        owner.compactSyncStatusDidDismiss(cancelledID)
        #expect(owner.navigation.sheet == nil && !presentation.statusOwnsInput)
        presentation.statusPresented = true
        let reopenedID = presentation.statusPresentationID
        owner.queueCloudSyncAfterStatusDismissal(.publicationChoice, host: .settings)
        owner.compactSyncStatusDidDismiss(cancelledID)
        #expect(owner.navigation.sheet == nil && presentation.statusHandoffPending)
        owner.compactSyncStatusDidDismiss(reopenedID)
        #expect(owner.navigation.sheet == .cloudSync(.publicationChoice))
        #expect(owner.navigation.sheetHost == .settings)
    }

    @Test func statusCannotQueueOverAnOwnedModalAndDismissalRechecksModalOwnership() {
        let owner = handoffCollection()
        let presentation = owner.syncPresentation
        owner.navigation.present(.collectionEntry)
        presentation.statusPresented = true
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        #expect(!presentation.statusHandoffPending)
        #expect(owner.navigation.sheet == .collectionEntry)
        owner.navigation.dismissSheet()
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        let id = presentation.statusPresentationID
        owner.navigation.present(.collectionEntry)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == .collectionEntry && !presentation.statusHandoffPending)
        owner.navigation.dismissSheet()
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil)
    }

    @Test func sceneDeactivationCancelsHandoffAcrossReactivation() {
        let owner = handoffCollection()
        owner.syncPresentation.statusPresented = true
        let id = owner.syncPresentation.statusPresentationID
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        owner.setSceneActive(false)
        owner.setSceneActive(true)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil && !owner.syncPresentation.statusHandoffPending)
    }

    @Test func reopeningStatusCancelsItsEarlierChoiceAndNormalSettingsEntryRemainsImmediate() {
        let owner = handoffCollection()
        owner.syncPresentation.statusPresented = true
        let id = owner.syncPresentation.statusPresentationID
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        owner.syncPresentation.statusPresented = true
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil && !owner.syncPresentation.statusHandoffPending)
        #expect(owner.syncPresentation.statusPresented)
        owner.navigation.present(.accountSync)
        owner.presentCloudSync(.replaceLocal, host: .settings)
        #expect(owner.navigation.sheet == .cloudSync(.replaceLocal))
        #expect(owner.navigation.sheetHost == .settings && !owner.syncPresentation.statusOwnsInput)
    }

    @Test func changedNavigationOrReviewAndCollectionResetInvalidatePendingEntry() {
        final class Session {}
        for change in 0..<3 {
            let owner = handoffCollection()
            owner.syncPresentation.statusPresented = true
            let id = owner.syncPresentation.statusPresentationID
            owner.queueCloudSyncAfterStatusDismissal(.connect)
            switch change {
            case 0: owner.navigation.show(.settings)
            case 1: owner.navigation.startReview(.init(deck: DeckID(42), session: Session()))
            default: owner.navigation.collectionReplaced()
            }
            owner.compactSyncStatusDidDismiss(id)
            #expect(owner.navigation.sheet == nil && !owner.syncPresentation.statusHandoffPending)
        }
    }

    @Test func pendingStatusHandoffOwnsInputAndPreservesTheReviewSessionAndDeck() {
        final class Session {}
        let owner = handoffCollection()
        let session = Session()
        owner.navigation.openDeckPage(DeckID(42))
        owner.navigation.startReview(.init(deck: DeckID(42), session: session))
        let section = owner.navigation.section
        let deck = owner.navigation.homeDeckPage
        owner.syncPresentation.statusPresented = true
        let id = owner.syncPresentation.statusPresentationID
        #expect(owner.syncPresentation.statusOwnsInput && !SyncKeyboardOwnership.admits(owner))
        owner.queueCloudSyncAfterStatusDismissal(.connect)
        #expect(owner.syncPresentation.statusOwnsInput && !owner.reviewIsActive)
        #expect(!SyncKeyboardOwnership.admits(owner) && owner.navigation.sheet == nil)
        #expect(owner.navigation.review?.session === session)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == .cloudSync(.connect) && !SyncKeyboardOwnership.admits(owner))
        owner.navigation.dismissSheet()
        #expect(owner.navigation.section == section && owner.navigation.homeDeckPage == deck)
        #expect(owner.navigation.review?.session === session)
        #expect(owner.navigation.columnVisibility == .detailOnly)
    }

    @Test func compactAccountSyncWaitsForDismissalAndReturnsToTheSameReview() {
        // Placeholder session proves retained navigation identity only, not native ordering or review input.
        final class Session {}
        let owner = handoffCollection()
        let session = Session()
        owner.navigation.openDeckPage(DeckID(42))
        owner.navigation.startReview(.init(deck: DeckID(42), session: session))
        let section = owner.navigation.section
        let deck = owner.navigation.homeDeckPage
        let reviewID = owner.navigation.review?.id
        let presentation = owner.syncPresentation
        presentation.connectionPath = [.replaceLocal]
        presentation.statusPresented = true
        let id = presentation.statusPresentationID
        owner.queueAccountSyncAfterStatusDismissal()
        owner.queueCloudSyncAfterStatusDismissal(.connect) // Cannot replace the first destination.
        #expect(!presentation.statusPresented && presentation.statusHandoffPending)
        #expect(owner.navigation.sheet == nil && !owner.reviewIsActive)
        #expect(presentation.statusOwnsInput && !SyncKeyboardOwnership.admits(owner))
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == .accountSync && owner.navigation.sheetHost == .main)
        #expect(!presentation.statusHandoffPending && !SyncKeyboardOwnership.admits(owner))
        #expect(presentation.connectionPath == [.replaceLocal])
        #expect(owner.navigation.section == section && owner.navigation.homeDeckPage == deck)
        #expect(owner.navigation.review?.id == reviewID && owner.navigation.review?.session === session)
        owner.navigation.dismissSheet()
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil)
        #expect(owner.navigation.section == section && owner.navigation.homeDeckPage == deck)
        #expect(owner.navigation.review?.id == reviewID && owner.navigation.review?.session === session)
        #expect(owner.navigation.columnVisibility == .detailOnly)
    }

    @Test func compactAccountSyncWithoutReviewWaitsThenPushesNormalSettingsRoute() {
        let owner = handoffCollection()
        let presentation = owner.syncPresentation
        owner.queueAccountSyncAfterStatusDismissal()
        #expect(!presentation.statusHandoffPending && owner.navigation.section == .home)
        presentation.statusPresented = true
        let id = presentation.statusPresentationID
        owner.queueAccountSyncAfterStatusDismissal()
        #expect(owner.navigation.sheet == nil && owner.navigation.section == .home)
        #expect(owner.navigation.path(.settings).isEmpty && presentation.statusOwnsInput)
        owner.compactSyncStatusDidDismiss(UUID())
        #expect(owner.navigation.section == .home && presentation.statusHandoffPending)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil && owner.navigation.section == .settings)
        #expect(owner.navigation.path(.settings) == [.settingsPage(.accountSync)])
        #expect(!presentation.statusOwnsInput)
        owner.navigation.show(.home)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.section == .home)
    }

    @Test func compactAccountSyncRejectsStaleOrCancelledRequestsAndOldReopenCallbacks() {
        final class Session {}
        for change in 0..<6 {
            let owner = handoffCollection()
            var presentation = owner.syncPresentation
            presentation.statusPresented = true
            let oldID = presentation.statusPresentationID
            owner.queueAccountSyncAfterStatusDismissal()
            switch change {
            case 0: presentation.cancelStatusHandoff()
            case 1:
                owner.setSceneActive(false)
                owner.setSceneActive(true)
            case 2: owner.navigation.openDeckPage(DeckID(99))
            case 3: owner.navigation.startReview(.init(deck: DeckID(42), session: Session()))
            case 4:
                owner.navigation.collectionReplaced()
                presentation = owner.syncPresentation
            default: presentation.statusPresented = true
            }
            owner.compactSyncStatusDidDismiss(oldID)
            #expect(owner.navigation.sheet == nil && !presentation.statusHandoffPending)
            #expect(owner.navigation.section != .settings && owner.navigation.path(.settings).isEmpty)
            presentation.statusPresented = true
            let newID = presentation.statusPresentationID
            owner.queueAccountSyncAfterStatusDismissal()
            owner.compactSyncStatusDidDismiss(oldID)
            #expect(owner.navigation.sheet == nil && presentation.statusHandoffPending)
            owner.compactSyncStatusDidDismiss(newID)
            #expect(!presentation.statusHandoffPending)
            if owner.navigation.review != nil { #expect(owner.navigation.sheet == .accountSync) }
            else {
                #expect(owner.navigation.section == .settings)
                #expect(owner.navigation.path(.settings) == [.settingsPage(.accountSync)])
            }
        }
    }

    @Test func compactAccountSyncCannotReplaceAForeignModalBeforeOrAfterQueueing() {
        let owner = handoffCollection()
        let presentation = owner.syncPresentation
        owner.navigation.present(.collectionEntry)
        presentation.statusPresented = true
        owner.queueAccountSyncAfterStatusDismissal()
        #expect(!presentation.statusHandoffPending && owner.navigation.sheet == .collectionEntry)
        owner.navigation.dismissSheet()
        let id = presentation.statusPresentationID
        owner.queueAccountSyncAfterStatusDismissal()
        owner.navigation.present(.collectionEntry)
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == .collectionEntry && !presentation.statusHandoffPending)
        #expect(owner.navigation.section == .home && owner.navigation.path(.settings).isEmpty)
        owner.navigation.dismissSheet()
        owner.compactSyncStatusDidDismiss(id)
        #expect(owner.navigation.sheet == nil && owner.navigation.section == .home)
    }

    private func handoffCollection() -> ReviewCollection {
        // Presentation tests never start a Host, perform preview reads or alter account state.
        ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation(),
            sync: SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: nil))
    }
}
