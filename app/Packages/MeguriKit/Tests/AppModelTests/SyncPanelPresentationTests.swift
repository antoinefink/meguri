@testable import AppModel
@testable import CoreBridge
#if canImport(AppKit)
import AppKit
#endif
import Foundation
import Testing

/// `sync/status-strings` T20.3 and native-interface-contract N23 (round 10).
@MainActor @Suite struct SyncPanelPresentationTests {
    private typealias Slot = SyncPanel.Slot

    @Test func routineStagesKeepOneIndeterminateIndicatorAndOnlyUseCoreRemainders() {
        for phase in [MeguriRoutinePhase.preparing, .collection, .applying, .checkingMedia, .syncingMedia] {
            let status = SyncKinds.meguri(.init(routine: .exchanging, routinePhase: phase))
            #expect(status.titleKey == phase.titleKey && status.action == nil && !status.needsYou)
            #expect(status.detailCount == nil)
            #expect(SyncPanel.progress(status, meguri: true, setupValue: nil,
                meguriMoving: true, cloudState: nil) == .indeterminate)
        }
        let known = SyncKinds.meguri(.init(routine: .exchanging, routinePhase: .syncingMedia, routineFilesLeft: 42))
        #expect(known.detailKey == "sync.meguri.round.filesLeft" && known.detailCount == 42)
        #expect(SyncText.format("sync.meguri.round.filesLeft", Int64(1)) == "1 file left to download.")
        #expect(SyncText.format("sync.meguri.round.filesLeft", Int64(42)) == "42 files left to download.")
        let zero = SyncKinds.meguri(.init(routine: .exchanging, routinePhase: .syncingMedia, routineFilesLeft: 0))
        #expect(zero.detailCount == 0)
        for phase in [MeguriRoutinePhase.preparing, .collection, .applying, .checkingMedia] {
            let stale = SyncKinds.meguri(.init(routine: .exchanging, routinePhase: phase, routineFilesLeft: 42))
            #expect(stale.detailCount == nil && stale.detailKey == nil)
        }
        let blocked = SyncKinds.meguri(.init(providerBlocked: true, routine: .exchanging, routinePhase: .applying))
        #expect(blocked.needsYou && blocked.action == .copyDebugInfo)
        let setup = SyncKinds.meguri(.init(stage: .media, routine: .exchanging, routinePhase: .applying))
        #expect(setup.titleKey != MeguriRoutinePhase.applying.titleKey)
    }

    @Test func n23_3TableGivesTheButtonsAndTheDefault() {
        // Sync off: Sync Settings alone, the default.
        let off = SyncKinds.cloud(state: .localOnly)
        let offSlot = SyncPanel.slot(off, serviceSelected: false, offered: false)
        #expect(offSlot == .none && SyncPanel.settingsIsDefault(offSlot) && !SyncPanel.slotIsDefault(offSlot))
        // Needs you with an action that can run: the action takes Sync Now's place.
        let refused = SyncKinds.meguri(.init(routine: .authentication))
        let refusedSlot = SyncPanel.slot(refused, serviceSelected: true, offered: true)
        #expect(refusedSlot == .action(.meguriSignIn))
        #expect(SyncPanel.slotIsDefault(refusedSlot) && !SyncPanel.settingsIsDefault(refusedSlot))
        let blocked = SyncKinds.meguri(.init(providerBlocked: true))
        #expect(SyncPanel.slot(blocked, serviceSelected: true, offered: true) == .action(.copyDebugInfo))
        // Needs you whose action cannot run now, or that has none: Sync Settings alone.
        #expect(SyncPanel.slot(refused, serviceSelected: true, offered: false) == .nothingToRun)
        let account = SyncKinds.cloud(state: .iCloudUnavailable(.noAccount))
        #expect(account.needsYou && account.action == nil)
        let nothing = SyncPanel.slot(account, serviceSelected: true, offered: false)
        #expect(nothing == .nothingToRun && !SyncPanel.settingsIsDefault(nothing) && !SyncPanel.slotIsDefault(nothing))
        // Actions that run a replacement at once, Cancel Setup and Copy Debug Info
        // (a diagnostic, changed 2026-11-22) are never the default.
        for action in [SyncStatusAction.meguriFinishSetup, .resumeOp, .meguriCancel, .copyDebugInfo, .repair] {
            #expect(!SyncPanel.slotIsDefault(.action(action)), "\(action)")
        }
        #expect(SyncPanel.slotIsDefault(.action(.chooseCollection)))
        // Any other status: Sync Now, the default.
        for status in [SyncKinds.cloud(state: .upToDate(asOfMilliseconds: 1)), SyncKinds.cloud(state: .syncingCollection),
                       SyncKinds.meguri(.init(routine: .exchanging)), SyncKinds.meguri(.init(routine: .network)),
                       SyncKinds.meguri(.init(stage: .media))] {
            let slot = SyncPanel.slot(status, serviceSelected: true, offered: false)
            #expect(slot == .syncNow && SyncPanel.slotIsDefault(slot), "\(status.titleKey)")
        }
        // A non-needs-you row's own action takes the place; Cancel is never the default.
        let preparing = SyncKinds.meguri(.init(stage: .preparing))
        let cancel = SyncPanel.slot(preparing, serviceSelected: true, offered: true)
        #expect(cancel == .action(.meguriCancel) && !SyncPanel.slotIsDefault(cancel) && !SyncPanel.settingsIsDefault(cancel))
        // Not offered (nothing runs yet): Sync Now keeps its place.
        #expect(SyncPanel.slot(preparing, serviceSelected: true, offered: false) == .syncNow)
        let unknown = SyncKinds.meguri(.init(routine: .acceptanceUnknown))
        #expect(SyncPanel.slot(unknown, serviceSelected: true, offered: true) == .action(.meguriTryAgain))
        // Issues: their action is in the list, never the panel's.
        let issues = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .upToDate(asOfMilliseconds: 1),
            attention: [.unreadable: 1])
        #expect(issues.hasIssues && SyncPanel.slot(issues, serviceSelected: true, offered: true) == .syncNow)
        let media = SyncKinds.meguri(.init(routine: .mediaBlocked))
        #expect(media.kind == .issues && SyncPanel.slot(media, serviceSelected: true, offered: true) == .syncNow)
        // Resume fix: with no affected files known there is no list to open, so
        // Try Again is the status row's action and the panel's.
        let unlisted = SyncKinds.meguri(.init(routine: .mediaBlocked, mediaListKnown: false))
        #expect(unlisted.kind == .waiting && !unlisted.hasIssues && unlisted.action == .meguriRetryMedia)
        #expect(SyncPanel.slot(unlisted, serviceSelected: true, offered: true) == .action(.meguriRetryMedia))
        let fixture = SyncCoordinator.renderFixture(.meguriMediaBlocked)
        #expect(fixture.meguriAffectedFileCount == nil && fixture.mainStatus.action == .meguriRetryMedia)
        #expect(!fixture.mainStatus.hasIssues)
    }

    @Test func barShowsWhileSomethingMovesAndNeverWhenTheStatusNeedsYou() {
        let moving = SyncKinds.cloud(state: .syncingCollection)
        #expect(SyncPanel.progress(moving, meguri: false, setupValue: nil, meguriMoving: false,
            cloudState: .syncingCollection) == .indeterminate)
        #expect(SyncPanel.progress(moving, meguri: false, setupValue: nil, meguriMoving: false,
            cloudState: .syncingMedia) == .indeterminate)
        // The bar shows with the moving shape only: never under a waiting or up-to-date state,
        // whatever media does underneath (N23.1); `progress` takes no media input for that reason.
        for state: CloudPolicyStatusState in [.changesPending, .upToDate(asOfMilliseconds: 1), .offline] {
            #expect(SyncPanel.progress(SyncKinds.cloud(state: state), meguri: false, setupValue: nil, meguriMoving: false,
                cloudState: state) == nil, "\(state)")
        }
        // A setup stage the Account row shows no value for has no bar either.
        #expect(SyncPanel.progress(SyncKinds.meguri(.init(stage: .collection)), meguri: true, setupValue: nil,
            meguriMoving: false, cloudState: nil) == nil)
        let idle = SyncKinds.cloud(state: .upToDate(asOfMilliseconds: 1))
        #expect(SyncPanel.progress(idle, meguri: false, setupValue: nil, meguriMoving: false,
            cloudState: .upToDate(asOfMilliseconds: 1)) == nil)
        let setup = SyncKinds.meguri(.init(stage: .media))
        #expect(SyncPanel.progress(setup, meguri: true, setupValue: .fraction(0.25), meguriMoving: true,
            cloudState: nil) == .fraction(0.25))
        #expect(SyncPanel.progress(setup, meguri: true, setupValue: .indeterminate, meguriMoving: true,
            cloudState: nil) == .indeterminate)
        let round = SyncKinds.meguri(.init(routine: .exchanging))
        #expect(SyncPanel.progress(round, meguri: true, setupValue: nil, meguriMoving: true,
            cloudState: nil) == .indeterminate)
        let needsYou = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .syncingMedia, attention: [.quotaFull: 1])
        #expect(SyncPanel.progress(needsYou, meguri: false, setupValue: nil, meguriMoving: false,
            cloudState: .syncingMedia) == nil)
    }

    @Test func setupBarIsTheAccountRowsValueAndCountsTheFilesLeft() {
        let sync = SyncCoordinator.renderFixture(.meguriSetupMedia)
        // MP6.5: files done over done plus left, the same value as the Account row.
        #expect(sync.meguriProgressValue == .fraction(18_945.0 / 44_000.0))
        #expect(sync.panelProgress == .fraction(18_945.0 / 44_000.0))
        // Resume fix: the count is in the status sentence, so no files line repeats it.
        #expect(sync.panelFilesLeft == nil)
        #expect(sync.mainStatus.titleKey == "sync.meguri.status.downloadingMedia.title")
        #expect(sync.mainStatus.detailKey == "sync.meguri.media.left" && sync.mainStatus.detailCount == 25_055)
        #expect(sync.serviceChangeBusyKey == "sync.switch.busy.mediaDownload")
        #expect(SyncCoordinator.renderFixture(.meguriConnected).serviceChangeBusyKey == "sync.switch.busy")
        #expect(sync.mainStatus.symbol == SyncSymbol.moving)
        // A blocked provider stops the bar (MP6.5).
        let blocked = SyncCoordinator.renderFixture(.meguriBlocked)
        #expect(blocked.panelProgress == nil && blocked.panelFilesLeft == nil)
        #expect(blocked.mainStatus.needsYou && blocked.mainStatus.action == .copyDebugInfo)
        #expect(!SyncCoordinator.renderFixture(.off).panelServiceSelected)
        #expect(SyncCoordinator.renderFixture(.iCloudConnected).panelServiceSelected)
    }

    @Test func bothServicesUseTheFiveCloudSymbols() {
        let family: Set = [SyncSymbol.upToDate, SyncSymbol.moving, SyncSymbol.waiting, SyncSymbol.needsYou, SyncSymbol.off]
        #expect(family.count == 5 && family.allSatisfy { $0.contains("icloud") })
        let cloud: [(CloudPolicyStatusState, String)] = [
            (.localOnly, SyncSymbol.off), (.upToDate(asOfMilliseconds: 1), SyncSymbol.upToDate),
            (.syncingCollection, SyncSymbol.moving), (.syncingMedia, SyncSymbol.moving),
            (.changesPending, SyncSymbol.waiting), (.offline, SyncSymbol.waiting),
            (.paused(.automaticOff), SyncSymbol.waiting), (.iCloudUnavailable(.temporary), SyncSymbol.waiting),
            (.iCloudUnavailable(.noAccount), SyncSymbol.needsYou), (.paused(.decisionRequired), SyncSymbol.needsYou)
        ]
        for (state, symbol) in cloud { #expect(SyncKinds.cloud(state: state).symbol == symbol, "\(state)") }
        for state in SyncCoordinator.RenderState.allCases {
            #expect(family.contains(SyncCoordinator.renderFixture(state).mainStatus.symbol), "\(state)")
        }
        #expect(SyncCoordinator.renderFixture(.meguriConnected).mainStatus.symbol == SyncSymbol.upToDate)
        #expect(SyncCoordinator.renderFixture(.meguriSyncing).mainStatus.symbol == SyncSymbol.moving)
        #expect(SyncCoordinator.renderFixture(.meguriRoundFailed).mainStatus.symbol == SyncSymbol.waiting)
        #expect(SyncCoordinator.renderFixture(.meguriSignInRefused).mainStatus.symbol == SyncSymbol.needsYou)
        #expect(SyncCoordinator.renderFixture(.off).mainStatus.symbol == SyncSymbol.off)
        // Nothing moves before preparation (N23.2), and the title is not repeated as the detail.
        let required = SyncKinds.meguri(.init(stage: .required))
        #expect(required.symbol == SyncSymbol.waiting && required.detailKey == required.titleKey)
    }

    @Test func syncNowStaysDisabledWhileMovingAndTheTitleIsNotRepeated() {
        #expect(SyncPanel.syncNowEnabled(available: true, moving: false))
        #expect(!SyncPanel.syncNowEnabled(available: true, moving: true))
        #expect(!SyncPanel.syncNowEnabled(available: false, moving: false))
        // N23.1: a detail that repeats the title is not shown.
        #expect(SyncKinds.meguri(.init(stage: .required)).shownDetailKey == nil)
        #expect(SyncKinds.meguri(.init(stage: .preparing)).shownDetailKey == "sync.meguri.setup.preparing")
        // N23.3 row 5: with no round reported, or round media blocked, Sync Now has its place
        // but does not run (T18.7's routes stay File › Sync Now's).
        for routine: MeguriRoutineState? in [nil, .mediaBlocked] {
            let status = SyncKinds.meguri(.init(routine: routine))
            #expect(SyncPanel.slot(status, serviceSelected: true, offered: false) == .syncNow)
            let route = MeguriSyncStatusPresentation(stopped: false, connection: .setupRequired, stage: nil, routine: routine).route
            #expect(route != .round, "\(String(describing: routine))")
        }
    }

    @Test func iCloudSymbolsAreDrawnForICloudOnly() {
        let shapes = [SyncSymbol.upToDate, SyncSymbol.moving, SyncSymbol.waiting, SyncSymbol.needsYou, SyncSymbol.off]
        for shape in shapes where shape != SyncSymbol.off {
            #expect(SyncSymbol.glyph(shape, iCloud: true) == shape)
        }
        // Sync Off is no service, so never an `icloud` symbol.
        #expect(SyncSymbol.glyph(SyncSymbol.off, iCloud: true) == "cloud")
        // N23.2 (2026-11-22): one unrestricted system symbol per state, nothing drawn inside a cloud.
        let other = shapes.map { SyncSymbol.glyph($0, iCloud: false) }
        #expect(other == ["checkmark.circle", "arrow.trianglehead.2.clockwise.rotate.90", "cloud",
                          "exclamationmark.arrow.trianglehead.2.clockwise.rotate.90", "cloud"])
        #expect(other.allSatisfy { !$0.contains("icloud") })
        #if canImport(AppKit)
        for name in other + shapes.filter({ $0 != SyncSymbol.off }) {
            #expect(NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil, "\(name)")
        }
        #endif
        #expect(!SyncCoordinator.renderFixture(.meguriConnected).symbolsAreICloud)
        #expect(!SyncCoordinator.renderFixture(.off).symbolsAreICloud)
        #expect(!SyncCoordinator.renderFixture(.meguriBlocked).symbolsAreICloud)
        #expect(SyncCoordinator.renderFixture(.iCloudConnected).symbolsAreICloud)
    }
}
