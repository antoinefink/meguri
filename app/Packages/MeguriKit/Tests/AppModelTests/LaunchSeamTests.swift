import Foundation
import SwiftUI
import Testing

import AppModel

/// The launch seams the app's test hooks inject into (KTD18, U7).
@Suite struct LaunchSeamTests {
    @Test func aFixedClockStaysAtItsInstant() async throws {
        let instant = Date(timeIntervalSince1970: 1_894_669_200)  // 2030-01-15T01:00:00Z
        let clock = AppClock.fixed(instant)
        #expect(clock.now() == instant)
        try await Task.sleep(for: .milliseconds(20))
        #expect(clock.now() == instant)
    }

    @Test func theSystemClockFollowsTheWallClock() {
        let before = Date()
        let now = AppClock.system.now()
        let after = Date()
        #expect(before <= now && now <= after)
    }

    @Test func aClockReadsFromAnyIsolation() async {
        let instant = Date(timeIntervalSince1970: 42)
        let clock = AppClock.fixed(instant)
        let read = await Task.detached { clock.now() }.value
        #expect(read == instant)
    }

    @Test func viewsReadTheSystemClockUnlessOneIsInjected() {
        let instant = Date(timeIntervalSince1970: 42)
        var environment = EnvironmentValues()
        let before = Date()
        let defaultReading = environment.appClock.now()
        #expect(defaultReading >= before)
        environment.appClock = .fixed(instant)
        #expect(environment.appClock.now() == instant)
    }

    @Test func theStandardCollectionLivesInApplicationSupport() throws {
        let support = try #require(FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first)
        let location = CollectionLocation.standard
        #expect(location.directory.pathComponents == support.pathComponents + ["Collection"])
        #expect(location.collectionFile.pathComponents == support.pathComponents + ["Collection", "collection.anki2"])
    }

    @Test func aCollectionLocationCanPointAnywhere() {
        let directory = URL(filePath: "/tmp/meguri-fixture", directoryHint: .isDirectory)
        let location = CollectionLocation(directory: directory)
        #expect(location.directory == directory)
        #expect(location.collectionFile.path(percentEncoded: false) == "/tmp/meguri-fixture/collection.anki2")
    }

    @Test func aDefaultLaunchOverridesNothing() {
        let configuration = LaunchConfiguration()
        let before = Date()
        #expect(configuration.clock.now() >= before)
        #expect(configuration.collectionLocation == .standard)
        #expect(configuration.timeZone == nil)
        #expect(configuration.locale == nil)
        #expect(configuration.ankiConnectPort == nil)
        #expect(configuration.migakuPort == nil)
        #expect(configuration.initialPresentation == nil)
        #expect(configuration.nativeReviewInput == nil)
        #expect(configuration.fieldDiagnostics == nil)
    }

    @Test func anInitialPresentationNamesItsStyle() {
        #expect(InitialPresentation.screen { Text("a") }.style == .screen)
        #expect(InitialPresentation.sheet { Text("a") }.style == .sheet)
        let alert = InitialPresentation.alert(title: "T", message: "M") { Button("OK") {} }
        #expect(alert.style == .alert)
        #expect(alert.alertTitle == "T")
        #expect(alert.alertMessage == "M")
        #expect(alert.alertActions != nil)
    }
}
