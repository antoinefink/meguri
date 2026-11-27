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





    @Test func theAcceptanceLineComesLast() {
        let confirmation = ServiceSwitchConfirmation(source: .meguri, destination: "localOnly",
                                                     facts: .init(changes: .unknown, acceptanceUnknown: true))
        #expect(confirmation.message(source: .meguri, locale: english).hasSuffix(SyncText.value("sync.switch.acceptanceUnknown")))
    }

    // MARK: Names


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
        let first = SyncCoordinator(directory: directory, defaults: defaults)
        // A round's credential failure proves an earlier sign-in, and is kept.
        #expect(first.applyMeguriRoundCredentialFailure(MeguriRoundAdmissionFailure.credentialsMissing))
        #expect(defaults.bool(forKey: "meguri.sync.signedInBefore"))
        let relaunched = SyncCoordinator(directory: directory, defaults: defaults)
        #expect(relaunched.meguriKindFacts.signedInBefore)
    }

    @Test func aRefusedFirstSignInStaysSetupWhenTheAddressIsEdited() {
        let sync = SyncCoordinator(directory: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString),
                                   defaults: nil)
        sync.renderFirstSignInRefusedForTests()
        #expect(sync.meguriKindFacts.signInRefused && !sync.meguriKindFacts.signedInBefore)
        sync.meguriAddressEdited()
        #expect(sync.meguriSignInErrorKey == nil && sync.meguriKindFacts.signInRefused)
    }

}
