import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// `sync/status-strings` T21.1 and T21.3: which lines the service change
/// confirmation shows for each old service, new value and state of its counts.
@MainActor struct ServiceSwitchConfirmationTests {
    private let english = Locale(identifier: "en_US")

    @Test(arguments: [
        (SyncProviderSelection.meguri, "localOnly", "sync.switch.title.off", "sync.switch.message.off.meguri", "sync.switch.confirm.off"),
        (.iCloud, "localOnly", "sync.switch.title.off", "sync.switch.message.off.iCloud", "sync.switch.confirm.off"),
        (.iCloud, "meguri", "sync.switch.title.meguri", "sync.switch.message.meguri", "sync.switch.confirm.meguri"),
        (.meguri, "iCloud", "sync.switch.title.iCloud", "sync.switch.message.iCloud", "sync.switch.confirm.iCloud"),
    ])
    func eachChangeHasItsQuestionMessageAndButton(source: SyncProviderSelection, destination: String,
                                                  title: String, message: String, confirm: String) {
        let confirmation = ServiceSwitchConfirmation(source: source, destination: destination, facts: .init(changes: .unknown))
        #expect(confirmation.titleKey == title && confirmation.messageKey == message && confirmation.confirmKey == confirm)
        #expect(SyncText.value(title).hasSuffix("?"))
    }

    // MARK: Meguri Sync as the old service

    @Test func meguriWithNothingKnownSaysSoInOneSentence() {
        let confirmation = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly", facts: .init(changes: .unknown))
        #expect(confirmation.unsent.isEmpty && confirmation.closing == .unknown)
        #expect(confirmation.message(source: .meguri, locale: english) == """
            This collection stays on this device and stops syncing. You’ll be signed out of Meguri Sync. Nothing on your server is deleted.

            Meguri couldn’t check whether everything on this device has been sent to Meguri Sync. Anything not sent stays only on this device.
            """)
    }

    @Test func meguriWithPendingWorkAndKnownMediaListsThem() {
        // Meguri Sync's answers are among its changes (the coordinator reports heldBack 0 then).
        let confirmation = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly",
                                                     facts: .init(changes: .some, heldBack: 0, media: 3))
        #expect(confirmation.unsent == [.someChanges, .media(3)] && confirmation.closing == .listed)
        #expect(confirmation.message(source: .meguri, locale: english).hasSuffix("""
            Not yet sent to Meguri Sync:
            • Some changes
            • 3 media files

            These stay only on this device.
            """))
        // With the media count not known, the rest is said to be unknown.
        let partial = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly",
                                                facts: .init(changes: .some, heldBack: 0, media: nil))
        #expect(partial.unsent == [.someChanges] && partial.closing == .unknownBeyond)
        #expect(partial.message(source: .meguri, locale: english).hasSuffix("""
            • Some changes

            Meguri couldn’t check whether anything else is waiting. Anything not sent stays only on this device.
            """))
    }

    @Test func meguriNeverReadsAsAllSent() {
        let confirmation = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly",
                                                     facts: .init(changes: .count(0), media: 0))
        #expect(confirmation.unsent.isEmpty && confirmation.closing == .unknown)
    }

    // MARK: iCloud as the old service

    @Test func iCloudWithEverythingSentSaysSoQuietly() {
        let confirmation = ServiceSwitchConfirmation(source: .iCloud, destination: "localOnly",
                                                     facts: .init(changes: .count(0), heldBack: 0, media: 0))
        #expect(confirmation.unsent.isEmpty && confirmation.closing == .allSent)
        #expect(confirmation.message(source: .iCloud, locale: english) == """
            This collection stays on this device and stops syncing. Nothing in iCloud is deleted.

            Everything on this device has been sent to iCloud.
            """)
    }

    @Test func iCloudWithKnownCountsListsEachAboveZero() {
        let confirmation = ServiceSwitchConfirmation(source: .iCloud, destination: "localOnly",
                                                     facts: .init(changes: .count(12), heldBack: 3, media: 2))
        #expect(confirmation.unsent == [.changes(12), .heldBack(3), .media(2)] && confirmation.closing == .listed)
        #expect(confirmation.message(source: .iCloud, locale: english).hasSuffix("""
            Not yet sent to iCloud:
            • 12 changes
            • 3 review answers
            • 2 media files

            These stay only on this device.
            """))
        let one = ServiceSwitchConfirmation(source: .iCloud, destination: "meguri",
                                            facts: .init(changes: .count(1), heldBack: 0, media: 1))
        #expect(one.unsent == [.changes(1), .media(1)])
        #expect(one.message(source: .iCloud, locale: english).contains("• 1 change\n• 1 media file\n"))
    }

    @Test func iCloudWithUnknownMediaIsNeverAllSent() {
        let none = ServiceSwitchConfirmation(source: .iCloud, destination: "localOnly",
                                             facts: .init(changes: .count(0), heldBack: 0, media: nil))
        #expect(none.unsent.isEmpty && none.closing == .unknown)
        let held = ServiceSwitchConfirmation(source: .iCloud, destination: "localOnly",
                                             facts: .init(changes: .count(0), heldBack: 4, media: nil))
        #expect(held.unsent == [.heldBack(4)] && held.closing == .unknownBeyond)
    }

    @Test func theAcceptanceLineComesLast() {
        let confirmation = ServiceSwitchConfirmation(source: .meguri, destination: "iCloud",
                                                     facts: .init(changes: .unknown, acceptanceUnknown: true))
        #expect(confirmation.message(source: .meguri, locale: english).hasSuffix(SyncText.value("sync.switch.acceptanceUnknown")))
    }

    // MARK: Names

    @Test func noTextNamesAServiceThatIsNotInvolved() {
        for facts in [ServiceSwitchFacts(changes: .unknown), .init(changes: .some, media: 3)] {
            let text = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly", facts: facts)
                .message(source: .meguri, locale: english)
            #expect(!text.contains("iCloud"), "\(text)")
        }
        for facts in [ServiceSwitchFacts(changes: .count(0), heldBack: 0, media: 0), .init(changes: .count(12), heldBack: 3, media: nil)] {
            let text = ServiceSwitchConfirmation(source: .iCloud, destination: "localOnly", facts: facts)
                .message(source: .iCloud, locale: english)
            #expect(!text.contains("Meguri Sync"), "\(text)")
            #expect(!text.contains("iCloud Sync"), "\(text)")
        }
    }

    // MARK: Facts the coordinator reads

    @Test func theCoordinatorReadsMeguriPendingWorkAsSomeAndOtherwiseUnknown() {
        let sync = SyncCoordinator.renderFixture(.meguriConnected)
        let facts = sync.serviceSwitchFacts(source: .meguri)
        #expect(facts.changes == .unknown && facts.heldBack == nil && facts.media == nil && !facts.acceptanceUnknown)
        let acceptance = SyncCoordinator.renderFixture(.meguriAcceptanceUnknown).serviceSwitchFacts(source: .meguri)
        #expect(acceptance.acceptanceUnknown)
    }

    @Test func signedInBeforeSurvivesARelaunchAndClearsWithTheConnection() throws {
        let suite = "meguri.tests.signedInBefore.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let first = SyncCoordinator(directory: directory, configuration: nil, defaults: defaults)
        // A round's credential failure proves an earlier sign-in, and is kept.
        #expect(first.applyMeguriRoundCredentialFailure(MeguriRoundAdmissionFailure.credentialsMissing))
        #expect(defaults.bool(forKey: "meguri.sync.signedInBefore"))
        let relaunched = SyncCoordinator(directory: directory, configuration: nil, defaults: defaults)
        #expect(relaunched.meguriKindFacts.signedInBefore)
    }

    @Test func aRefusedFirstSignInStaysSetupWhenTheAddressIsEdited() {
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
                                   configuration: nil, defaults: nil)
        sync.renderFirstSignInRefusedForTests()
        #expect(sync.meguriKindFacts.signInRefused && !sync.meguriKindFacts.signedInBefore)
        sync.meguriAddressEdited()
        #expect(sync.meguriSignInErrorKey == nil && sync.meguriKindFacts.signInRefused)
    }

    @Test func theCoordinatorReadsICloudCountsFromItsStatus() {
        let facts = SyncCoordinator.renderFixture(.iCloudNeedsYou).serviceSwitchFacts(source: .iCloud)
        #expect(facts.changes == .count(12) && facts.heldBack == 0 && facts.media == 0)
        // iCloud as the source with Meguri Sync selected reads nothing from Meguri Sync's round.
        let other = SyncCoordinator.renderFixture(.meguriConnected).serviceSwitchFacts(source: .iCloud)
        #expect(other.changes == .unknown && other.media == nil)
    }
}
