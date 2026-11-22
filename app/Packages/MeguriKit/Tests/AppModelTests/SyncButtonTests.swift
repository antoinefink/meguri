@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sync/status-strings` T22 and native-interface-contract N23.6: what a tap on
/// the toolbar's sync button does, and the feedback after a sync it started.
@MainActor @Suite struct SyncButtonTests {
    private func tap(_ status: SyncMainStatus, service: Bool = true, offered: Bool = false,
                     available: Bool = true, moving: Bool = false) -> SyncButton.Tap {
        let slot = SyncPanel.slot(status, serviceSelected: service, offered: offered)
        return SyncButton.tap(status, serviceSelected: service, slot: slot, syncNowAvailable: available, moving: moving)
    }

    @Test func t22_1ATapSyncsInTheOrdinaryStates() {
        // Up to date, waiting, a failure that retries: a tap syncs at once.
        for status in [SyncKinds.meguri(.init(routine: .idle)), SyncKinds.meguri(.init(routine: .pending)),
                       SyncKinds.meguri(.init(routine: .network)), SyncKinds.meguri(.init(routine: .server)),
                       SyncKinds.cloud(state: .upToDate(asOfMilliseconds: 1)), SyncKinds.cloud(state: .changesPending)] {
            #expect(!status.needsYou, "\(status.titleKey)")
            #expect(tap(status) == .syncNow, "\(status.titleKey)")
        }
    }

    @Test func t22_1NeedsYouAndNoServiceOpenThePanel() {
        // A blind sync cannot resolve a state that needs the person.
        let refused = SyncKinds.meguri(.init(routine: .authentication))
        #expect(refused.needsYou)
        #expect(tap(refused, offered: true) == .openPanel)
        #expect(tap(refused, offered: false, available: false) == .openPanel)
        let account = SyncKinds.cloud(state: .iCloudUnavailable(.noAccount))
        #expect(tap(account) == .openPanel)
        let blocked = SyncKinds.meguri(.init(providerBlocked: true))
        #expect(tap(blocked, offered: true) == .openPanel)
        // Even with Sync Now available, needs you opens the panel.
        #expect(tap(refused, available: true) == .openPanel)
        // No service: the panel, whose one button is Sync Settings.
        let off = SyncKinds.cloud(state: .localOnly)
        #expect(tap(off, service: false) == .openPanel)
    }

    @Test func t22_1AStatusWithItsOwnActionOpensThePanel() {
        // Cancel Setup while setup prepares (it moves too), Try Again for an
        // acceptance that is not known: only the panel offers them.
        let preparing = SyncKinds.meguri(.init(stage: .preparing))
        #expect(tap(preparing, offered: true, moving: true) == .openPanel)
        let unknown = SyncKinds.meguri(.init(routine: .acceptanceUnknown))
        #expect(tap(unknown, offered: true, available: false) == .openPanel)
    }

    @Test func t22_1ATapWhileMovingDoesNothing() {
        // No second sync: a tap during a round or a transfer does nothing.
        for status in [SyncKinds.meguri(.init(routine: .exchanging)), SyncKinds.meguri(.init(stage: .media)),
                       SyncKinds.cloud(state: .syncingCollection), SyncKinds.cloud(state: .syncingMedia)] {
            #expect(tap(status, available: true, moving: true) == .nothing, "\(status.titleKey)")
            #expect(tap(status, available: false, moving: true) == .nothing, "\(status.titleKey)")
        }
    }

    @Test func t22_1AnUnavailableSyncNowOpensThePanel() {
        // Meguri Sync before its setup finishes, round media blocked: Sync Now
        // cannot run, and the panel says why.
        let idle = SyncKinds.meguri(.init(routine: .idle))
        #expect(tap(idle, available: false) == .openPanel)
        let media = SyncKinds.meguri(.init(routine: .mediaBlocked))
        #expect(!media.needsYou)
        #expect(tap(media, available: false) == .openPanel)
    }

    @Test func t22_1ASuccessShowsTheAccentOnceAfterTheSyncItStarted() {
        var feedback = SyncButton.Feedback()
        let start = Date(timeIntervalSince1970: 1_000)
        // Nothing requested: a round that moves and ends gives nothing.
        #expect(feedback.observe(moving: true, upToDate: false, needsYou: false, now: start) == nil)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start) == nil)
        #expect(!feedback.accented)
        // Requested, moving, then up to date: the success, once.
        feedback.requested(at: start)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start) == nil)
        #expect(feedback.observe(moving: true, upToDate: false, needsYou: false, now: start) == nil)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 2) == .succeeded)
        #expect(feedback.accented)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 3) == nil)
        feedback.endAccent()
        #expect(!feedback.accented)
    }

    @Test func t22_1AFailureEndsWithoutTheAccent() {
        let start = Date(timeIntervalSince1970: 1_000)
        // It ran and ended waiting (no network): its own symbol, no accent.
        var feedback = SyncButton.Feedback()
        feedback.requested(at: start)
        _ = feedback.observe(moving: true, upToDate: false, needsYou: false, now: start)
        #expect(feedback.observe(moving: false, upToDate: false, needsYou: false, now: start + 1) == .ended)
        #expect(!feedback.accented)
        // It ran and ended needing the person.
        feedback.requested(at: start)
        _ = feedback.observe(moving: true, upToDate: false, needsYou: false, now: start)
        #expect(feedback.observe(moving: false, upToDate: false, needsYou: true, now: start + 1) == .ended)
        // Refused at once into needs you, before it showed moving.
        feedback.requested(at: start)
        #expect(feedback.observe(moving: false, upToDate: false, needsYou: true, now: start + 1) == .ended)
        #expect(!feedback.accented)
    }

    @Test func t22_1ARequestThatNeverMovesIsDropped() {
        let start = Date(timeIntervalSince1970: 1_000)
        var feedback = SyncButton.Feedback()
        feedback.requested(at: start)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 5) == nil)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 25) == nil)
        // A later round the person did not start gives nothing.
        #expect(feedback.observe(moving: true, upToDate: false, needsYou: false, now: start + 30) == nil)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 31) == nil)
        #expect(!feedback.accented)
        // Expired with no change seen meanwhile: the next moving status is not its work.
        feedback.requested(at: start)
        #expect(feedback.observe(moving: true, upToDate: false, needsYou: false, now: start + 40) == nil)
        #expect(feedback.observe(moving: false, upToDate: true, needsYou: false, now: start + 41) == nil)
        #expect(!feedback.accented)
    }
}
