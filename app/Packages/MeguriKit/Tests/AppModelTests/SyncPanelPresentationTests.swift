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

}
