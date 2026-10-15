import Foundation
import XCTest

/// The launch-option test hooks (U7, R1963, R1967). Debug and Profile builds
/// open any registered route with the requested overrides; Release and
/// AppStore builds contain no hook code and ignore the options. A build
/// without the `.dev` bundle id is never launched on the Mac: it would
/// replace the daily-driver app.
@MainActor
final class TestHooksTests: XCTestCase {
    // MARK: Hook builds (Debug, Profile)

    /// What the sample sheet shows for the fixtures in app/Fixtures/TestHooks:
    /// each installed file's path, size and first 12 hex digits of its
    /// SHA-256 (from `shasum -a 256`). The two sheet tests install different
    /// fixtures and run in name order, so each replaces what the other left:
    /// a silently skipped install fails.
    static let singleFileFixture = "collection.anki2 69 cf43867b9ced"
    static let directoryFixture = "collection.anki2 87 18165af9294c; collection.media/a.txt 33 f68ad4f37031"

    func testSampleSheetRouteAppliesEveryOverride() throws {
        let build = try requireHooks()
        let app = try launch(
            MeguriLaunch(
                fixture: build.hookFixture("single-file.fixture"), now: "2030-01-15T10:00:00", timeZone: "Asia/Tokyo", locale: "en_US",
                animations: false, reduceMotion: true, reduceTransparency: true,
                ankiConnectPort: 18765, migakuPort: 54432, route: "sample/sheet"
            ),
            build
        )
        XCTAssert(app.staticTexts["sample.sheet.title"].waitForExistence(timeout: 15), "the sample sheet is not on screen")
        XCTAssertEqual(text(app, "sample.sheet.timeZone"), "Asia/Tokyo")
        XCTAssertEqual(text(app, "sample.sheet.processTimeZone"), "Asia/Tokyo")
        XCTAssertEqual(text(app, "sample.sheet.locale"), "en_US")
        // 10:00 local to Asia/Tokyo (UTC+9, no DST), a Tuesday.
        XCTAssertEqual(text(app, "sample.sheet.clock"), "2030-01-15T10:00:00+09:00")
        XCTAssertEqual(text(app, "sample.sheet.date"), "Tuesday, January 15, 2030")
        XCTAssertEqual(text(app, "sample.sheet.fixture"), Self.singleFileFixture)
        XCTAssertEqual(text(app, "sample.sheet.reduceMotion"), "on")
        XCTAssertEqual(text(app, "sample.sheet.reduceTransparency"), "on")
        #if !os(macOS)
        XCTAssertEqual(text(app, "sample.sheet.animations"), "off")
        #endif
        XCTAssertEqual(text(app, "sample.sheet.ports"), "ankiconnect 18765, migaku 54432")
        XCTAssertEqual(waitForText(app, "sample.sheet.coreFeatures", notEqualTo: "loading"), build.expectedCoreFeatures)
    }

    func testInstantWithZoneDesignatorDirectoryFixtureAndOffOverrides() throws {
        let build = try requireHooks()
        let app = try launch(
            MeguriLaunch(
                fixture: build.hookFixture("directory"), now: "2030-01-15T10:00:00Z", timeZone: "America/New_York", locale: "fr_FR",
                animations: true, reduceMotion: false, reduceTransparency: false, route: "sample/sheet"
            ),
            build
        )
        XCTAssert(app.staticTexts["sample.sheet.title"].waitForExistence(timeout: 15), "the sample sheet is not on screen")
        XCTAssertEqual(text(app, "sample.sheet.timeZone"), "America/New_York")
        XCTAssertEqual(text(app, "sample.sheet.processTimeZone"), "America/New_York")
        XCTAssertEqual(text(app, "sample.sheet.locale"), "fr_FR")
        // 10:00 UTC is 05:00 in New York in January (UTC-5).
        XCTAssertEqual(text(app, "sample.sheet.clock"), "2030-01-15T05:00:00-05:00")
        XCTAssertEqual(text(app, "sample.sheet.date"), "mardi 15 janvier 2030")
        XCTAssertEqual(text(app, "sample.sheet.fixture"), Self.directoryFixture)
        XCTAssertEqual(text(app, "sample.sheet.reduceMotion"), "off")
        XCTAssertEqual(text(app, "sample.sheet.reduceTransparency"), "off")
        #if !os(macOS)
        XCTAssertEqual(text(app, "sample.sheet.animations"), "on")
        #endif
        XCTAssertEqual(text(app, "sample.sheet.ports"), "ankiconnect default, migaku default")
    }

    func testSampleScreenRoute() throws {
        let build = try requireHooks()
        let app = try launch(MeguriLaunch(animations: false, route: "sample/screen"), build)
        XCTAssert(app.staticTexts["sample.screen.title"].waitForExistence(timeout: 15), "the sample screen is not on screen")
    }

    /// An error-state route shows the route's own buttons, never only the
    /// system's OK. System alerts drop the accessibility identifiers of their
    /// buttons (iOS and the Mac), so the test reaches them through the alert
    /// and their labels.
    func testSampleAlertRoute() throws {
        let build = try requireHooks()
        let app = try launch(MeguriLaunch(locale: "en_US", animations: false, route: "sample/alert"), build)
        #if os(macOS)
        let alert = app.sheets.firstMatch
        #else
        let alert = app.alerts.firstMatch
        #endif
        XCTAssert(alert.waitForExistence(timeout: 15), "the sample error alert is not on screen")
        XCTAssert(alert.staticTexts["The sample error state."].exists, "the alert lacks the sample message")
        XCTAssertEqual(Set(alert.buttons.allElementsBoundByIndex.map(\.label)), ["Try Again", "Cancel"])
        // On the Mac a click on a window that is not key only activates it.
        try app.bringToFront(bundleIdentifier: try build.bundleIdentifier)
        alert.buttons["Cancel"].tap()
        XCTAssert(alert.waitForNonExistence(timeout: 5), "Cancel did not dismiss the alert")
    }

    func testBadLaunchOptionsReplaceTheAppWithTheHookError() throws {
        let build = try requireHooks()
        let cases: [(MeguriLaunch, String)] = [
            (MeguriLaunch(route: "nope/missing"), "no route 'nope/missing'; registered: designsystem/gallery, sample/alert, sample/deck-overview/empty, sample/deck-overview/error, sample/deck-overview/loading, sample/deck-overview/overflow, sample/deck-overview/typical, sample/defects, sample/screen, sample/sheet, sample/touch-targets"),
            (MeguriLaunch(extra: ["-MeguriBogus", "1"]), "unknown launch option -MeguriBogus"),
            (MeguriLaunch(now: "2030-02-30T10:00:00Z"), "-MeguriNow takes YYYY-MM-DDTHH:MM:SS"),
            (MeguriLaunch(timeZone: "Mars/Olympus"), "unknown time zone 'Mars/Olympus'"),
            (MeguriLaunch(fixture: URL(filePath: "/nonexistent/meguri/collection.anki2")), "no file or directory at /nonexistent/meguri/collection.anki2"),
        ]
        for (options, expected) in cases {
            let app = try launch(options, build)
            let message = app.staticTexts["testhooks.error.message"]
            XCTAssert(message.waitForExistence(timeout: 15), "no hook error for \(options.arguments)")
            XCTAssert(text(app, "testhooks.error.message").contains(expected), "'\(text(app, "testhooks.error.message"))' lacks '\(expected)'")
            XCTAssertFalse(app.staticTexts["home.placeholder.title"].exists, "the app ran despite the hook error")
            app.terminate()
        }
    }

    func testMacWindowSize() throws {
        #if os(macOS)
        let build = try requireHooks()
        let app = try launch(MeguriLaunch(animations: false, windowSize: "1000x700"), build)
        XCTAssert(app.staticTexts["home.placeholder.title"].waitForExistence(timeout: 15))
        let window = app.windows.firstMatch
        let resized = expectation(for: NSPredicate { _, _ in abs(window.frame.width - 1000) < 1 }, evaluatedWith: nil)
        wait(for: [resized], timeout: 10)
        XCTAssertGreaterThanOrEqual(window.frame.height, 700)

        // SwiftUI saved that frame. A hook launch without a size must still
        // open at the default (WindowMetrics.standard, 1180x780).
        app.terminate()
        let defaultApp = try launch(MeguriLaunch(animations: false, route: "sample/screen"), build)
        XCTAssert(defaultApp.staticTexts["sample.screen.title"].waitForExistence(timeout: 15))
        let defaultWindow = defaultApp.windows.firstMatch
        let reset = expectation(for: NSPredicate { _, _ in abs(defaultWindow.frame.width - 1180) < 1 }, evaluatedWith: nil)
        wait(for: [reset], timeout: 10)
        #else
        throw XCTSkip("the window size option is Mac only")
        #endif
    }

    // MARK: Builds without hooks (Release, AppStore)

    func testLaunchOptionsAreIgnoredWithoutHooks() throws {
        let build = try BuildUnderTest.current()
        guard !build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has hooks") }
        let app = try launch(
            MeguriLaunch(
                fixture: build.hookFixture("single-file.fixture"), now: "2030-01-15T10:00:00Z", timeZone: "Asia/Tokyo", locale: "en_US",
                reduceMotion: true, route: "sample/sheet", extra: ["-MeguriBogus", "1"]
            ),
            build
        )
        XCTAssert(app.staticTexts["home.placeholder.title"].waitForExistence(timeout: 15), "the app did not open on Home")
        XCTAssertFalse(app.staticTexts["sample.sheet.title"].waitForExistence(timeout: 3), "a Release-style build followed -MeguriRoute")
        XCTAssertFalse(app.staticTexts["testhooks.error.message"].exists, "a Release-style build parsed the launch options")
    }

    // MARK: Every build

    /// The hook types and the Rust test-hook exports are compiled into Debug
    /// and Profile and absent from Release and AppStore (R1967). The scan
    /// looks for the Swift type names (type metadata keeps them even in a
    /// stripped binary) and the uniffi export names. An optimized build drops
    /// the metadata of types used only statically, but keeps the `TestHooks`
    /// namespace whenever any hook type has metadata: hook builds must show
    /// it and the Rust exports, builds without hooks none of the names.
    func testHookCodeIsInHookBuildsOnly() throws {
        let build = try BuildUnderTest.current()
        let files = try build.machOFiles
        XCTAssertFalse(files.isEmpty, "no executable in \(build.appBundle.path(percentEncoded: false))")
        let binaries = try files.map { try Data(contentsOf: $0, options: .alwaysMapped) }
        func contains(_ marker: String) -> Bool {
            binaries.contains { $0.range(of: Data(marker.utf8)) != nil }
        }
        let swiftNames = ["TestHooks", "LaunchOptions", "FixtureInstaller", "RouteRegistry", "AppearanceOverrides", "TestHookError"]
        let rustExports = "_uniffi_meguri_ffi_fn_func_test_"
        let names = files.map(\.lastPathComponent)
        if build.hasHooks {
            XCTAssertTrue(contains("TestHooks\0"), "TestHooks is missing from \(names): the scan would prove nothing")
            XCTAssertTrue(contains(rustExports), "the Rust test hooks are missing from \(names): the scan would prove nothing")
        } else {
            for name in swiftNames {
                XCTAssertFalse(contains(name + "\0"), "\(name) is in the \(build.configuration.rawValue) build (\(names))")
            }
            XCTAssertFalse(contains(rustExports), "Rust test hooks are in the \(build.configuration.rawValue) build (\(names))")
        }
    }

    func testIdentifiersMatchTheConfiguration() throws {
        let build = try BuildUnderTest.current()
        XCTAssertEqual(try build.bundleIdentifier, build.expectedBundleIdentifier)
        XCTAssertEqual(try build.appInfo["MeguriAppGroupIdentifier"] as? String, build.expectedAppGroup)
    }

    // MARK: Helpers

    private func requireHooks() throws -> BuildUnderTest {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        return build
    }

    private func launch(_ options: MeguriLaunch, _ build: BuildUnderTest) throws -> XCUIApplication {
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = options.arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func text(_ app: XCUIApplication, _ identifier: String) -> String {
        let element = app.staticTexts[identifier]
        guard element.waitForExistence(timeout: 5) else { return "<\(identifier) missing>" }
        if !element.label.isEmpty { return element.label }
        return element.value as? String ?? ""
    }

    private func waitForText(_ app: XCUIApplication, _ identifier: String, notEqualTo placeholder: String) -> String {
        let deadline = Date().addingTimeInterval(10)
        var current = text(app, identifier)
        while current == placeholder, Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            current = text(app, identifier)
        }
        return current
    }
}
