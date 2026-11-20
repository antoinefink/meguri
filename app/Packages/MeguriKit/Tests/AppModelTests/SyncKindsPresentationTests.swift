@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/status-strings` T19.9: each row of T19.4's two tables and of T19.5.
@MainActor @Suite struct SyncKindsPresentationTests {
    private typealias Row = SyncKindRow
    private func one(_ item: CloudPolicyAttention, _ facts: CloudKindFacts = .init()) -> Row? {
        SyncKinds.cloudRows(attention: [item: 2], facts: facts).first
    }

    @Test func cloudStateRowsGiveTheirKindTextAndAction() {
        #expect(SyncKinds.cloudStateRow(.localOnly) == nil)
        #expect(SyncKinds.cloudStateRow(.iCloudUnavailable(.noAccount)) == Row(.needsYou, "sync.status.unavailable.noAccount"))
        #expect(SyncKinds.cloudStateRow(.iCloudUnavailable(.restricted)) == Row(.needsYou, "sync.status.unavailable.restricted"))
        #expect(SyncKinds.cloudStateRow(.iCloudUnavailable(.temporary)) == Row(.waiting, "sync.status.unavailable.temporary"))
        #expect(SyncKinds.cloudStateRow(.iCloudUnavailable(.setupNeeded))
            == Row(.needsYou, "sync.status.unavailable.setupNeeded", .copyDebugInfo))
        #expect(SyncKinds.cloudStateRow(.paused(.decisionRequired), fence: .restore)
            == Row(.needsYou, "sync.needsYou.fence.restore", .chooseCollection))
        #expect(SyncKinds.cloudStateRow(.paused(.decisionRequired), fence: .importCollection)
            == Row(.needsYou, "sync.needsYou.fence.import", .chooseCollection))
        #expect(SyncKinds.cloudStateRow(.paused(.decisionRequired))
            == Row(.needsYou, "sync.needsYou.fence.none", .chooseCollection))
        #expect(SyncKinds.cloudStateRow(.offline)?.kind == .waiting)
        for state: CloudPolicyStatusState in [.paused(.automaticOff), .syncingCollection, .syncingMedia,
                                              .changesPending, .upToDate(asOfMilliseconds: 1)] {
            #expect(SyncKinds.cloudStateRow(state) == nil)
        }
        let off = SyncKinds.cloud(state: .localOnly)
        #expect(off.titleKey == "sync.status.off.title" && off.detailKey == "sync.status.localOnly.detail")
        #expect(off.kind == nil && off.action == nil)
        // The update hold shows as Sync Automatically off (P25.2).
        let hold = SyncKinds.cloud(state: .paused(.automaticOff))
        #expect(hold.titleKey == "sync.status.automaticOff.title" && hold.kind == nil)
    }

    @Test func cloudNeedsYouRowsOneToTwentyFive() {
        #expect(one(.collectionCorrupt) == Row(.needsYou, "sync.attention.collectionCorrupt", .repair))
        #expect(one(.localStorageFull) == Row(.needsYou, "sync.needsYou.localStorageFull", .tryAgain))
        #expect(one(.trackingError) == Row(.needsYou, "sync.attention.trackingError", .tryAgain))
        #expect(one(.unsupportedSchema) == Row(.needsYou, "sync.needsYou.update"))
        let isolated: [(CloudKindFacts.Isolation, Bool, String, SyncStatusAction)] = [
            (.signedOut, true, "sync.attention.signedOut", .resume),
            (.signedOut, false, "sync.attention.signedOut", .setUpAgain),
            (.accountChanged, true, "sync.needsYou.accountChanged", .chooseCollection),
            (.accountChangedBack, true, "sync.attention.accountChangedBack", .resume),
            (.accountChangedBack, false, "sync.attention.accountChangedBack", .setUpAgain),
            (.replacedElsewhere, false, "sync.attention.replacedElsewhere", .chooseCollection),
            (.missing, false, "sync.needsYou.missing", .chooseCollection),
            (.deleted, false, "sync.needsYou.deleted", .chooseCollection),
            (.encryptionReset, false, "sync.needsYou.encryptionReset", .chooseCollection),
            (.fileMismatch, true, "sync.attention.fileMismatch", .setUpAgain),
            (.other, true, "sync.attention.isolated", .setUpAgain)
        ]
        for (reason, resume, key, action) in isolated {
            #expect(one(.isolated, .init(isolation: reason, resumeBindingOffered: resume)) == Row(.needsYou, key, action))
        }
        #expect(one(.rootUnreadable) == Row(.needsYou, "sync.needsYou.rootUnreadable", .chooseCollection))
        #expect(one(.chooseCollection, .init(layoutDecision: true))
            == Row(.needsYou, "sync.attention.chooseCollection", .chooseCollection))
        #expect(one(.chooseCollection) == Row(.needsYou, "sync.needsYou.setUp", .setUpAgain))
        #expect(one(.crtMismatch) == Row(.needsYou, "sync.attention.crtMismatch", .chooseCollection))
        #expect(one(.mediaZoneMissing) == Row(.needsYou, "sync.needsYou.mediaMissingInICloud", .chooseCollection))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .adoptUnproven))
            == Row(.needsYou, "sync.attention.adoptUnproven", .cancelOp))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .publishConflict))
            == Row(.needsYou, "sync.attention.publishConflict", .chooseCollection))
        #expect(one(.quotaFull, .init(publishQuota: true)) == Row(.needsYou, "sync.attention.publishQuota", .cancelOp))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .publishAccount, resumeOpOffered: true))
            == Row(.needsYou, "sync.attention.publishAccount", .resumeOp))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .publishAccount))
            == Row(.needsYou, "sync.attention.publishAccount", .cancelOp))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .publishWaiting, resumeOpOffered: true))
            == Row(.needsYou, "sync.attention.publishWaiting", .resumeOp))
        #expect(one(.quotaFull) == Row(.needsYou, "sync.needsYou.quotaFull", .tryAgain))
        #expect(one(.generationIncomplete, .init(abandonOffered: true))
            == Row(.needsYou, "sync.attention.generationIncomplete", .abandon))
    }

    @Test func cloudWaitingAndIssuesRows() {
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .publishWaiting)) == Row(.waiting, "sync.attention.publishWaiting"))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .adoptAfterReview)) == Row(.waiting, "sync.attention.adoptAfterReview"))
        #expect(one(.recoveryOpWaiting, .init(recoveryOp: .abandonWaiting)) == Row(.waiting, "sync.attention.abandonWaiting"))
        #expect(one(.cloudError) == Row(.waiting, "sync.waiting.cloudError"))
        #expect(one(.exchangeIncomplete) == Row(.waiting, "sync.attention.exchangeIncomplete"))
        #expect(one(.referenceWaiting) == Row(.waiting, "sync.attention.referenceWaiting", count: 2))
        #expect(one(.assetUnavailable) == Row(.waiting, "sync.attention.assetUnavailable", count: 2))
        #expect(one(.mediaChanged) == Row(.waiting, "sync.attention.mediaChanged", count: 2))
        #expect(one(.conflictsUnreadable) == Row(.issues, "sync.attention.conflictsUnreadable"))
        #expect(one(.refused) == Row(.issues, "sync.attention.refused", count: 2))
        #expect(one(.oversize) == Row(.issues, "sync.attention.oversize", count: 2))
        #expect(one(.unreadable) == Row(.issues, "sync.attention.unreadable", .checkDatabase, count: 2))
        #expect(one(.unsyncable) == Row(.issues, "sync.attention.unsyncable", count: 2))
        #expect(one(.claimStuck) == Row(.issues, "sync.attention.claimStuck", count: 2))
        #expect(one(.generationIncomplete) == Row(.issues, "sync.attention.generationIncomplete"))
        #expect(one(.applyFailed) == Row(.issues, "sync.attention.applyFailed", count: 2))
        #expect(one(.rekeyFailed) == Row(.issues, "sync.attention.rekeyFailed", count: 2))
        #expect(one(.mediaMissing) == Row(.issues, "sync.attention.mediaMissing", count: 2))
        // The list's only button is row 37's.
        let all = SyncKinds.cloudRows(attention: Dictionary(uniqueKeysWithValues:
            [CloudPolicyAttention.conflictsUnreadable, .refused, .oversize, .unreadable, .unsyncable, .claimStuck,
             .generationIncomplete, .applyFailed, .rekeyFailed, .mediaMissing].map { ($0, 1) }))
        #expect(all.count == 10 && all.allSatisfy { $0.kind == .issues })
        #expect(all.filter { $0.action != nil }.map(\.action) == [.checkDatabase])
    }

    @Test func firstNeedsYouRowInTableOrderShows() {
        let status = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .changesPending,
            attention: [.quotaFull: 1, .trackingError: 1, .refused: 3, .cloudError: 1])
        #expect(status.kind == .needsYou && status.symbol == SyncKinds.needsYouSymbol)
        #expect(status.titleKey == "sync.status.needsAttention.title")
        #expect(status.detailKey == "sync.attention.trackingError" && status.action == .tryAgain)
        #expect(status.issues == [Row(.issues, "sync.attention.refused", count: 3)])
        // Once it is settled, the next shows.
        let next = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .changesPending,
            attention: [.quotaFull: 1, .refused: 3])
        #expect(next.detailKey == "sync.needsYou.quotaFull")
        // A state row comes before the attention rows.
        let account = SyncKinds.cloud(state: .iCloudUnavailable(.noAccount), attention: [.quotaFull: 1])
        #expect(account.detailKey == "sync.status.unavailable.noAccount" && account.action == nil)
    }

    @Test func waitingOrIssuesAloneKeepTheStateUnderneath() {
        let underneath: [(CloudPolicyStatusState, String, String)] = [
            (.paused(.automaticOff), "sync.status.automaticOff.title", "pause.circle"),
            (.offline, "sync.status.offline.title", "wifi.slash"),
            (.syncingCollection, "sync.status.syncing.title", "arrow.triangle.2.circlepath.icloud"),
            (.syncingMedia, "sync.status.syncingMedia.title", "photo.badge.arrow.down"),
            (.changesPending, "sync.status.pending.title", "icloud.and.arrow.up"),
            (.upToDate(asOfMilliseconds: 5), "sync.status.upToDate.title", "checkmark.icloud")
        ]
        for (state, title, symbol) in underneath {
            let issues = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: state, attention: [.oversize: 4])
            #expect(issues.titleKey == title && issues.symbol == symbol)
            #expect(issues.kind == nil && issues.action == nil && !issues.needsYou)
            #expect(issues.issues == [Row(.issues, "sync.attention.oversize", count: 4)])
            let waiting = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: state,
                attention: [.exchangeIncomplete: 1, .cloudError: 1])
            #expect(waiting.titleKey == title && waiting.symbol == symbol)
            // The first waiting row in the table's order is the detail; no button.
            #expect(waiting.kind == .waiting && waiting.detailKey == "sync.waiting.cloudError")
            #expect(waiting.action == nil && waiting.issues.isEmpty)
        }
    }

    @Test func conflictCopiesChangeNothingInTheMainStatus() {
        let plain = SyncKinds.cloud(state: .upToDate(asOfMilliseconds: 9))
        let conflicts = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .upToDate(asOfMilliseconds: 9),
            attention: [.conflicts: 6])
        #expect(conflicts == plain)
        #expect(SyncKinds.cloudRows(attention: [.conflicts: 6]).isEmpty)
        #expect(SyncKinds.conflictCopies([.conflicts: 6]) == 6)
        #expect(SyncKinds.conflictCopies([:]) == nil && SyncKinds.conflictCopies([.conflicts: 0]) == nil)
        #expect(SyncText.format("sync.trouble.conflictCopies", Int64(6)) == "6 conflict copies are saved on this device.")
        #expect(SyncText.format("sync.trouble.conflictCopies", Int64(1)) == "1 conflict copy is saved on this device.")
    }

    @Test func meguriRowsFollowT18Order() {
        typealias Facts = MeguriKindFacts
        let rows: [(Facts, Row)] = [
            (Facts(handoffRecovery: true, providerBlocked: true, connection: .blocked),
             Row(.needsYou, "sync.needsYou.switchUnfinished", .serviceRecovery)),
            (Facts(providerBlocked: true, connection: .blocked), Row(.needsYou, "sync.needsYou.blocked", .copyDebugInfo)),
            (Facts(connection: .blocked, stage: .failed), Row(.needsYou, "sync.needsYou.meguri.signInStore", .meguriConnectionRetry)),
            (Facts(connection: .unconfigured), Row(.needsYou, "sync.meguri.status.signedOut.detail", .meguriSignIn)),
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
        #expect(SyncKinds.meguri(.init(routine: .exchanging)).titleKey == "sync.status.syncing.title")
        let needs = SyncKinds.meguri(.init(routine: .fullSyncRequired))
        #expect(needs.needsYou && needs.symbol == SyncKinds.needsYouSymbol && needs.action == .chooseCollection)
        // Waiting keeps a Try Again only on the two acceptance-unknown rows.
        #expect(SyncKinds.meguri(.init(routine: .acceptanceUnknown)).action == .meguriTryAgain)
        #expect(SyncKinds.meguri(.init(stage: .acceptanceUnknown)).action == .meguriTryAgain)
        #expect(SyncKinds.meguri(.init(routine: .network)).action == nil)
        #expect(SyncKinds.meguri(.init(routine: .network)).symbol != SyncKinds.needsYouSymbol)
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

    // Rows that T19.4 does not list, kept at the least claim.

    @Test func anAdoptionWaitingOnTransportOffersResumeOnlyWhileItIsOffered() {
        let offered = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .changesPending,
            attention: [.recoveryOpWaiting: 1], facts: .init(recoveryOp: .adoptWaiting, resumeOpOffered: true))
        #expect(offered.needsYou && offered.detailKey == "sync.attention.adoptWaiting" && offered.action == .resumeOp)
        let waiting = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .changesPending,
            attention: [.recoveryOpWaiting: 1], facts: .init(recoveryOp: .adoptWaiting))
        #expect(waiting.kind == .waiting && waiting.detailKey == "sync.attention.adoptWaiting" && waiting.action == nil)
        #expect(waiting.titleKey == "sync.status.pending.title")
        let marks: Set<Character> = [".", "!", "?"]
        #expect(SyncText.value("sync.attention.adoptWaiting").filter(marks.contains).count == 1)
    }

    @Test func iCloudSelectedButNeverSetUpIsRow16NotSyncOff() {
        // The policy still reports `localOnly` until a connection is confirmed.
        let selected = SyncKinds.cloud(state: .localOnly, selectedNotSetUp: true)
        #expect(selected.needsYou && selected.titleKey == "sync.status.needsAttention.title")
        #expect(selected.detailKey == "sync.needsYou.setUp" && selected.action == .setUpAgain)
        // With the service Off the same state stays Sync Off.
        let off = SyncKinds.cloud(state: .localOnly)
        #expect(off.titleKey == "sync.status.off.title" && off.action == nil && !off.needsYou)
        // Any other state ignores the flag.
        let connected = SyncKinds.cloud(state: .changesPending, selectedNotSetUp: true)
        #expect(connected.titleKey == "sync.status.pending.title" && !connected.needsYou)
    }

    @Test func everyActionHasItsCatalogTitle() {
        for action in SyncStatusAction.allCases { #expect(SyncText.value(action.titleKey) != action.titleKey) }
    }
}
