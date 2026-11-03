import AppModel
import CoreBridge
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
}
