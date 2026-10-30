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
