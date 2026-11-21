@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/status-strings` T20.3 and native-interface-contract N23 (round 10).
@MainActor @Suite struct SyncPanelPresentationTests {
    private typealias Slot = SyncPanel.Slot

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
        // Actions that run a replacement at once, and Cancel Setup, are never the default.
        for action in [SyncStatusAction.meguriFinishSetup, .resumeOp, .meguriCancel] {
            #expect(!SyncPanel.slotIsDefault(.action(action)), "\(action)")
        }
        #expect(SyncPanel.slotIsDefault(.action(.chooseCollection)) && SyncPanel.slotIsDefault(.action(.copyDebugInfo)))
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
    }

    @Test func barShowsWhileSomethingMovesAndNeverWhenTheStatusNeedsYou() {
        let moving = SyncKinds.cloud(state: .syncingCollection)
        #expect(SyncPanel.progress(moving, meguri: false, setupValue: nil, meguriBusy: false,
            cloudState: .syncingCollection) == .indeterminate)
        #expect(SyncPanel.progress(moving, meguri: false, setupValue: nil, meguriBusy: false,
            cloudState: .syncingMedia) == .indeterminate)
        // No bar beside a checkmark, whatever media does underneath (N23.1).
        #expect(SyncPanel.progress(SyncKinds.cloud(state: .changesPending), meguri: false, setupValue: nil, meguriBusy: false,
            cloudState: .changesPending) == nil)
        let idle = SyncKinds.cloud(state: .upToDate(asOfMilliseconds: 1))
        #expect(SyncPanel.progress(idle, meguri: false, setupValue: nil, meguriBusy: false,
            cloudState: .upToDate(asOfMilliseconds: 1)) == nil)
        let setup = SyncKinds.meguri(.init(stage: .media))
        #expect(SyncPanel.progress(setup, meguri: true, setupValue: .fraction(0.25), meguriBusy: true,
            cloudState: nil) == .fraction(0.25))
        #expect(SyncPanel.progress(setup, meguri: true, setupValue: .indeterminate, meguriBusy: true,
            cloudState: nil) == .indeterminate)
        let round = SyncKinds.meguri(.init(routine: .exchanging))
        #expect(SyncPanel.progress(round, meguri: true, setupValue: nil, meguriBusy: true,
            cloudState: nil) == .indeterminate)
        let needsYou = SyncKinds.cloud(state: .needsAttention, stateWithoutAttention: .syncingMedia, attention: [.quotaFull: 1])
        #expect(SyncPanel.progress(needsYou, meguri: false, setupValue: nil, meguriBusy: false,
            cloudState: .syncingMedia) == nil)
    }

    @Test func setupBarIsTheAccountRowsValueAndCountsTheFilesLeft() {
        let sync = SyncCoordinator.renderFixture(.meguriSetupMedia)
        // MP6.5: files done over done plus left, the same value as the Account row.
        #expect(sync.meguriProgressValue == .fraction(18_945.0 / 44_000.0))
        #expect(sync.panelProgress == .fraction(18_945.0 / 44_000.0))
        #expect(sync.panelFilesLeft == 25_055)
        #expect(sync.mainStatus.titleKey == "sync.meguri.status.transferringMedia.title")
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

    @Test func iCloudSymbolsAreDrawnForICloudOnly() {
        let shapes = [SyncSymbol.upToDate, SyncSymbol.moving, SyncSymbol.waiting, SyncSymbol.needsYou, SyncSymbol.off]
        for shape in shapes where shape != SyncSymbol.off {
            #expect(SyncSymbol.glyph(shape, iCloud: true) == .system(shape))
        }
        // Sync Off is no service, so never an `icloud` symbol.
        #expect(SyncSymbol.glyph(SyncSymbol.off, iCloud: true) == .cloud("slash"))
        #expect(shapes.map { SyncSymbol.glyph($0, iCloud: false) }
            == [.cloud("checkmark"), .cloud("arrows"), .cloud("none"), .cloud("exclamationmark"), .cloud("slash")])
        #expect(!SyncCoordinator.renderFixture(.meguriConnected).symbolsAreICloud)
        #expect(!SyncCoordinator.renderFixture(.off).symbolsAreICloud)
        #expect(!SyncCoordinator.renderFixture(.meguriBlocked).symbolsAreICloud)
        #expect(SyncCoordinator.renderFixture(.iCloudConnected).symbolsAreICloud)
    }
}
