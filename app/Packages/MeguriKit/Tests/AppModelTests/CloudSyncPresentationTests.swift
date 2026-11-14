@testable import AppModel
import CoreBridge
import Foundation
import SwiftUI
import Testing

@MainActor @Suite struct CloudSyncPresentationTests {
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
