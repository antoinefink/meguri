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
        let fallback = SyncKinds.meguri(.init(routine: .exchanging))
        #expect(fallback.titleKey == MeguriRoutinePhase.preparing.titleKey)
        #expect(fallback.detailKey == nil && fallback.detailCount == nil)
        #expect(fallback.action == nil && !fallback.needsYou)
        #expect(SyncPanel.progress(fallback, setupValue: nil, moving: true) == .indeterminate)
        for phase in [MeguriRoutinePhase.preparing, .collection, .applying, .checkingMedia, .syncingMedia] {
            let status = SyncKinds.meguri(.init(routine: .exchanging, routinePhase: phase))
            #expect(status.titleKey == phase.titleKey && status.action == nil && !status.needsYou)
            #expect(status.detailCount == nil)
            #expect(SyncPanel.progress(status, setupValue: nil, moving: true) == .indeterminate)
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

    @Test func barShowsMeguriSetupAndRoundsButNeverNeedsYouOrOff() {
        let setup = SyncKinds.meguri(.init(stage: .media))
        #expect(SyncPanel.progress(setup, setupValue: .fraction(0.25), moving: true) == .fraction(0.25))
        #expect(SyncPanel.progress(setup, setupValue: .indeterminate, moving: true) == .indeterminate)
        let round = SyncKinds.meguri(.init(routine: .exchanging))
        #expect(SyncPanel.progress(round, setupValue: nil, moving: true) == .indeterminate)
        #expect(SyncPanel.progress(SyncKinds.off, setupValue: nil, moving: false) == nil)
        let needsYou = SyncKinds.meguri(.init(routine: .authentication))
        #expect(SyncPanel.progress(needsYou, setupValue: .fraction(0.5), moving: true) == nil)
    }
    @Test func statusUsesNeutralGlyphsWhileOffKeepsItsOwnSemanticValue() {
        #expect(SyncSymbol.glyph(SyncSymbol.off) == "cloud")
        #expect(SyncSymbol.off != SyncSymbol.waiting)
        #expect(SyncSymbol.glyph(SyncSymbol.upToDate) == "checkmark.circle")
        #expect(SyncSymbol.glyph(SyncSymbol.moving) == "arrow.trianglehead.2.clockwise.rotate.90")
        #expect(SyncSymbol.glyph(SyncSymbol.needsYou) == "exclamationmark.arrow.trianglehead.2.clockwise.rotate.90")
    }
}
