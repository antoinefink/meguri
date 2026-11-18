#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import Testing

@testable import Meguri

/// The `-Meguri…` launch-option parser (U7, R1963), in the app process.
@MainActor
@Suite struct LaunchOptionsTests {
    typealias Options = TestHooks.LaunchOptions

    private var nativeArguments: [String] { [
        "-MeguriNativeReviewRoot", "/owned/checkout", "-MeguriNativeReviewCase", "media-case",
        "-MeguriNativeReviewPlatform", "mac", "-MeguriNativeReviewFreezeSHA256", String(repeating: "a", count: 64),
        "-MeguriNativeReviewFreezeBytes", "4096",
    ] }
    @Test func nativeReviewRequiresCompleteOptionsAndRestrictsInitialReset() throws {
        let options = try Options(arguments: nativeArguments)
        #expect(options.nativeReview?.caseName == "media-case")
        #expect(options.nativeReview?.freezeBytes == 4096)
        #expect(Options().nativeReview == nil)
        var checkout = URL(filePath: #filePath)
        for _ in 0..<4 { checkout.deleteLastPathComponent() }
        let fixture = checkout.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection")
        #expect(try FileManager.default.contentsOfDirectory(atPath: fixture.path) == [".gitkeep"])
        #expect(try Data(contentsOf: fixture.appending(path: ".gitkeep")).isEmpty)
        var initialArguments = nativeArguments
        initialArguments[1] = checkout.path
        let initial = try Options(arguments: initialArguments + ["-MeguriFixture", fixture.path])
        #expect(initial.fixture == fixture)
        #expect(throws: TestHookError.self) {
            try Options(arguments: initialArguments + ["-MeguriFixture", checkout.appending(path: "app/Fixtures/EmptyCollection").path])
        }
        #expect(throws: TestHookError.self) { try Options(arguments: Array(nativeArguments.dropLast(2))) }
        for pair in [["-MeguriFixture", "/fixture"], ["-MeguriRoute", "review/answer"],
                     ["-MeguriNow", "2030-01-15T10:00:00Z"], ["-MeguriTimeZone", "UTC"]] {
            #expect(throws: TestHookError.self) { try Options(arguments: nativeArguments + pair) }
        }
        var bad = nativeArguments; bad[3] = "../foreign"
        #expect(throws: TestHookError.self) { try Options(arguments: bad) }
    }

    @Test func initialResetFromAnotherCheckoutCannotTouchTheCollection() throws {
        let scratch = FileManager.default.temporaryDirectory.appending(path: "native-reset-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: scratch) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = .init(directory: scratch)
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        let bytes = Data("existing development database".utf8)
        try bytes.write(to: configuration.collectionLocation.collectionFile)
        let options = try Options(arguments: nativeArguments + ["-MeguriFixture", "/owned/checkout/app/Fixtures/CollectionEntry/EmptyCollection"])
        #expect(throws: TestHookError.self) {
            try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "test.dev"), routes: .standard)
        }
        #expect(try Data(contentsOf: configuration.collectionLocation.collectionFile) == bytes)
        #expect(configuration.nativeReviewInput == nil)
    }
    @Test func nativeReviewGuardRunsBeforeAnyReleaseConfigurationChange() throws {
        let options = try Options(arguments: nativeArguments)
        var configuration = LaunchConfiguration()
        #expect(throws: TestHookError("NativeReview requires a .dev bundle")) {
            try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "com.antoinefink.meguri"), routes: .standard)
        }
        #expect(configuration.nativeReviewInput == nil && !configuration.clock.isFixed)
        #if os(macOS)
        try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "com.antoinefink.meguri.dev"), routes: .standard)
        #expect(configuration.nativeReviewInput != nil && configuration.clock.isFixed)
        #expect(configuration.timeZone?.identifier == "Europe/Paris")
        #else
        #expect(throws: TestHookError.self) {
            try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "com.antoinefink.meguri.dev"), routes: .standard)
        }
        #expect(configuration.nativeReviewInput == nil)
        #endif
    }

    @Test func parsesEveryOption() throws {
        let options = try Options(arguments: [
            "-MeguriFixture", "/tmp/fixture/collection.anki2",
            "-MeguriImportSource", "/tmp/fixture/review.colpkg",
            "-MeguriNow", "2030-01-15T10:00:00",
            "-MeguriTimeZone", "Asia/Tokyo",
            "-MeguriLocale", "en_US",
            "-MeguriAnimations", "off",
            "-MeguriReduceMotion", "on",
            "-MeguriReduceTransparency", "off",
            "-MeguriWindowSize", "1000x700",
            "-MeguriAppearance", "dark",
            "-MeguriAnkiConnectPort", "18765",
            "-MeguriMigakuPort", "54432",
            "-MeguriSizeClass", "regular",
            "-MeguriIntegrations", "on",
            "-MeguriRoute", "review/answer",
        ])
        #expect(options.importSource == URL(filePath: "/tmp/fixture/review.colpkg"))
        #expect(options.fixture == URL(filePath: "/tmp/fixture/collection.anki2"))
        // 10:00 in Tokyo (UTC+9) is 01:00 UTC.
        #expect(options.now == Date(timeIntervalSince1970: 1_894_669_200))
        #expect(options.timeZone?.identifier == "Asia/Tokyo")
        #expect(options.locale?.identifier == "en_US")
        #expect(options.animationsEnabled == false)
        #expect(options.reduceMotion == true)
        #expect(options.reduceTransparency == false)
        #expect(options.windowSize == .explicit(width: 1000, height: 700))
        #expect(options.appearance == .dark)
        #expect(options.ankiConnectPort == 18765)
        #expect(options.migakuPort == 54432)
        #expect(options.sizeClass == .regular)
        #expect(options.integrations == true)
        #expect(options.route?.path == "review/answer")
    }

    @Test func leavesOtherArgumentsToTheSystem() throws {
        let options = try Options(arguments: ["-AppleLanguages", "(en)", "-NSDocumentRevisionsDebugMode", "YES", "plain"])
        #expect(options == Options())
    }

    @Test(arguments: [
        ("2030-01-15T10:00:00Z", 1_894_701_600.0),
        ("2030-01-15T10:00:00+05:30", 1_894_681_800.0),
        ("2030-01-15T10:00:00-08:00", 1_894_730_400.0),
    ])
    func zoneDesignatorsWinOverTheLaunchZone(value: String, seconds: Double) throws {
        let options = try Options(arguments: ["-MeguriTimeZone", "Asia/Tokyo", "-MeguriNow", value])
        #expect(options.now == Date(timeIntervalSince1970: seconds))
    }

    @Test func namedWindowSizes() throws {
        #expect(try Options(arguments: ["-MeguriWindowSize", "minimum"]).windowSize == .minimum)
        #expect(try Options(arguments: ["-MeguriWindowSize", "default"]).windowSize == .standard)
        #expect(try Options(arguments: ["-MeguriWindowSize", "large"]).windowSize == .large)
    }

    @Test func theAppearanceIsMacOnly() throws {
        let options = try Options(arguments: ["-MeguriAppearance", "light"])
        #expect(options.appearance == .light)
        var configuration = LaunchConfiguration()
        let apply = {
            _ = try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "x.dev"), routes: .standard)
        }
        #if os(macOS)
        try apply()
        #else
        #expect(throws: TestHookError("-MeguriAppearance applies to the Mac only"), performing: apply)
        #endif
    }

    /// The size class is iOS's (the Mac has none), the integrations flag the
    /// Mac's (R11).
    @Test func theSizeClassIsIOSOnlyAndTheIntegrationsFlagMacOnly() throws {
        #expect(try Options(arguments: ["-MeguriSizeClass", "compact"]).sizeClass == .compact)
        #expect(try Options(arguments: ["-MeguriIntegrations", "off"]).integrations == false)
        let installer = TestHooks.FixtureInstaller(bundleIdentifier: "x.dev")
        var configuration = LaunchConfiguration()
        let sizeClass = try Options(arguments: ["-MeguriSizeClass", "regular"])
        let integrations = try Options(arguments: ["-MeguriIntegrations", "on"])
        #if os(macOS)
        #expect(throws: TestHookError("-MeguriSizeClass applies to iOS only: the Mac has no size class")) {
            try sizeClass.apply(to: &configuration, installer: installer, routes: .standard)
        }
        try integrations.apply(to: &configuration, installer: installer, routes: .standard)
        #else
        try sizeClass.apply(to: &configuration, installer: installer, routes: .standard)
        #expect(throws: TestHookError("-MeguriIntegrations applies to the Mac only")) {
            try integrations.apply(to: &configuration, installer: installer, routes: .standard)
        }
        #endif
    }

    @Test func theWindowSizeIsMacOnly() throws {
        let options = try Options(arguments: ["-MeguriWindowSize", "large"])
        var configuration = LaunchConfiguration()
        let apply = {
            _ = try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "x.dev"), routes: .standard)
        }
        #if os(macOS)
        try apply()
        #else
        #expect(throws: TestHookError("-MeguriWindowSize applies to the Mac only"), performing: apply)
        #endif
    }

    @Test(arguments: [
        (["-MeguriBogus", "1"], "unknown launch option -MeguriBogus"),
        (["-MeguriRoute", "a/b", "-MeguriRoute", "c/d"], "-MeguriRoute is given twice"),
        (["-MeguriRoute"], "-MeguriRoute needs a value"),
        (["-MeguriImportSource", "relative/review.colpkg"], "-MeguriImportSource needs an absolute path"),
        (["-MeguriFixture", "relative/collection.anki2"], "-MeguriFixture needs an absolute path"),
        (["-MeguriNow", "2030-01-15 10:00:00"], "-MeguriNow takes"),
        (["-MeguriNow", "2030-02-30T10:00:00Z"], "-MeguriNow takes"),
        (["-MeguriNow", "2030-01-15T10:00:00+25:00"], "-MeguriNow takes"),
        (["-MeguriTimeZone", "Mars/Olympus"], "unknown time zone 'Mars/Olympus'"),
        (["-MeguriLocale", "xx_NOPE"], "unknown locale 'xx_NOPE'"),
        (["-MeguriSizeClass", "wide"], "-MeguriSizeClass takes compact or regular, got 'wide'"),
        (["-MeguriIntegrations", "yes"], "-MeguriIntegrations takes on or off, got 'yes'"),
        (["-MeguriAnimations", "yes"], "-MeguriAnimations takes on or off"),
        (["-MeguriReduceMotion", "1"], "-MeguriReduceMotion takes on or off"),
        (["-MeguriAnkiConnectPort", "0"], "-MeguriAnkiConnectPort takes a port from 1 to 65535"),
        (["-MeguriMigakuPort", "70000"], "-MeguriMigakuPort takes a port from 1 to 65535"),
        (["-MeguriWindowSize", "1000x"], "-MeguriWindowSize takes minimum, default, large or <width>x<height>"),
        (["-MeguriWindowSize", "0x700"], "-MeguriWindowSize takes"),
        (["-MeguriAppearance", "Dark"], "-MeguriAppearance takes light or dark, got 'Dark'"),
        (["-MeguriRoute", "sample"], "is not a route"),
        (["-MeguriRoute", "Sample/Sheet"], "is not a route"),
        (["-MeguriRoute", "sample//sheet"], "is not a route"),
    ])
    func rejectsBadArguments(arguments: [String], message: String) {
        #expect {
            try Options(arguments: arguments)
        } throws: { error in
            (error as? TestHookError)?.message.contains(message) == true
        }
    }
    @Test func ownedProviderFixtureIsIsolatedAndInertByDefault() throws {
        typealias Owned = TestHooks.OwnedProviderFixture
        let files = FileManager.default
        let scratch = files.temporaryDirectory.appending(path: "owned-provider-options-\(UUID().uuidString)")
        try files.createDirectory(at: scratch, withIntermediateDirectories: false)
        defer { try? files.removeItem(at: scratch) }
        let destination = scratch.appending(path: "Destination")
        try files.createDirectory(at: destination, withIntermediateDirectories: false)
        let sentinel = destination.appending(path: "collection.anki2")
        let bytes = Data("synthetic destination must survive rejected options".utf8)
        try bytes.write(to: sentinel)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = .init(directory: destination)
        let initialLocation = configuration.collectionLocation
        let installer = TestHooks.FixtureInstaller(bundleIdentifier: "test.dev")

        #expect(!Options().ownedProviderFixture)
        let off = try Options(arguments: ["-MeguriOwnedProviderFixture", "off"])
        #expect(off == Options())
        #expect(try off.prepareOwnedProviderFixture(bundleIdentifier: "release") == nil)
        #expect(try Options().prepareOwnedProviderFixture(bundleIdentifier: "test.dev") == nil)
        try off.apply(to: &configuration, installer: installer, routes: .standard)
        #expect(configuration.collectionLocation == initialLocation)
        #expect(try Data(contentsOf: sentinel) == bytes)

        let flag = ["-MeguriOwnedProviderFixture", "on"]
        let arguments = flag + ["-MeguriFixture", Owned.source.path(percentEncoded: false)]
        let on = try Options(arguments: arguments)
        #expect(on.ownedProviderFixture)
        #expect(on != Options())
        for bad in [["-MeguriOwnedProviderFixture"], ["-MeguriOwnedProviderFixture", "yes"],
                    flag + ["-MeguriOwnedProviderFixture", "off"], flag,
                    flag + ["-MeguriFixture", "relative/local-review-1"],
                    flag + ["-MeguriFixture", scratch.path],
                    flag + ["-MeguriFixture", Owned.source.path + "/../local-review-1"]] {
            #expect(throws: TestHookError.self) { try Options(arguments: bad) }
        }
        for pair in [["-MeguriRoute", "settings/account-sync/setup-needed"],
                     ["-MeguriImportSource", scratch.appending(path: "synthetic.colpkg").path],
                     ["-MeguriBackgroundLaunch", "YES"], ["-MeguriBackgroundLaunch", "NO"],
                     ["-MeguriAnkiConnectPort", "18765"], ["-MeguriMigakuPort", "18766"],
                     ["-MeguriIntegrations", "off"], ["-MeguriIntegrations", "on"],
                     ["-MeguriSizeClass", "regular"], ["-ShowAnkiServerInterface", "YES"], nativeArguments] {
            #expect(throws: TestHookError.self) { try Options(arguments: arguments + pair) }
        }
        let alias = scratch.appending(path: "alias")
        try files.createSymbolicLink(at: alias, withDestinationURL: Owned.source)
        #expect(throws: TestHookError.self) { try Options(arguments: flag + ["-MeguriFixture", alias.path]) }
        #expect(throws: TestHookError.self) { try Owned.validateSource(nil) }
        #expect(throws: TestHookError.self) { try Owned.validateSource(scratch.appending(path: "absent")) }
        #expect(throws: TestHookError.self) { try Owned.validateSource(alias) }
        #expect(throws: TestHookError.self) { try on.prepareOwnedProviderFixture(bundleIdentifier: "test.release") }
        #expect(throws: TestHookError.self) {
            try on.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "test.release"), routes: .standard)
        }
        #expect(throws: TestHookError.self) { try on.apply(to: &configuration, installer: installer, routes: .standard) }
        #expect(configuration.collectionLocation == initialLocation)
        #expect(configuration.nativeReviewInput == nil && !configuration.clock.isFixed)
        #expect(try Data(contentsOf: sentinel) == bytes)
        #expect(try files.contentsOfDirectory(atPath: destination.path) == ["collection.anki2"])

        #if os(macOS)
        // The public generated recipe is read-only; all destinations and defaults
        // below are fresh synthetic inputs. MG stages the recipe for the UI lane.
        let ownedResult = try on.prepareOwnedProviderFixture(bundleIdentifier: "test.dev")
        let owned = try #require(ownedResult)
        defer { _ = owned.cleanup(afterClosing: true) }
        let other = try Owned()
        defer { _ = other.cleanup(afterClosing: true) }
        let root = owned.collectionLocation.directory.deletingLastPathComponent()
        let otherRoot = other.collectionLocation.directory.deletingLastPathComponent()
        #expect(root != otherRoot)
        #expect(owned.collectionLocation.directory.lastPathComponent == "Collection")
        #expect(owned.collectionLocation.backupsDirectory == root.appending(path: "Backups", directoryHint: .isDirectory))
        #expect(owned.installationDirectory == root.appending(path: "Installation", directoryHint: .isDirectory))
        #expect(owned.collectionLocation != .standard)
        #expect(!files.fileExists(atPath: owned.installationDirectory.path))
        #expect(!files.fileExists(atPath: owned.collectionLocation.directory.path))
        let attributes = try files.attributesOfItem(atPath: root.path)
        let permissions = try #require(attributes[.posixPermissions] as? NSNumber)
        #expect(permissions.intValue & 0o777 == 0o700)
        #expect(owned.defaults !== other.defaults)
        let key = "synthetic-owned-marker"
        #expect(owned.defaults.object(forKey: key) == nil)
        owned.defaults.set("first", forKey: key)
        #expect(other.defaults.object(forKey: key) == nil)
        other.defaults.set("second", forKey: key)
        #expect(owned.defaults.string(forKey: key) == "first")
        #expect(throws: TestHookError.self) {
            try off.apply(to: &configuration, installer: installer, routes: .standard, ownedProvider: owned)
        }
        #expect(configuration.collectionLocation == initialLocation)
        try on.apply(to: &configuration, installer: installer, routes: .standard, ownedProvider: owned)
        #expect(configuration.collectionLocation == owned.collectionLocation)
        #expect(files.fileExists(atPath: configuration.collectionLocation.collectionFile.path))
        #expect(!files.fileExists(atPath: owned.installationDirectory.path), "No provider choice or state is synthesized")
        #expect(try Data(contentsOf: sentinel) == bytes)
        #expect(!owned.cleanup(afterClosing: false))
        #expect(files.fileExists(atPath: root.path))
        #expect(owned.defaults.string(forKey: key) == "first")
        #expect(owned.cleanup(afterClosing: true))
        #expect(!files.fileExists(atPath: root.path))
        #expect(owned.defaults.object(forKey: key) == nil)
        #expect(owned.cleanup(afterClosing: true))
        #expect(files.fileExists(atPath: otherRoot.path))
        #expect(other.defaults.string(forKey: key) == "second")
        #expect(other.cleanup(afterClosing: true))
        #else
        #expect(throws: TestHookError("OwnedProviderFixture applies to the Mac only")) {
            try on.prepareOwnedProviderFixture(bundleIdentifier: "test.dev")
        }
        #expect(configuration.collectionLocation == initialLocation)
        #expect(try Data(contentsOf: sentinel) == bytes)
        #endif
    }
}
#endif
