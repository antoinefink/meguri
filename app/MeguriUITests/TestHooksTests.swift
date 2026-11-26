import Foundation
import XCTest
#if os(macOS)
import AppKit
#endif

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
        alert.buttons["Cancel"].clickOrTap()
        XCTAssert(alert.waitForNonExistence(timeout: 5), "Cancel did not dismiss the alert")
    }

    func testBadLaunchOptionsReplaceTheAppWithTheHookError() throws {
        let build = try requireHooks()
        let cases: [(MeguriLaunch, String)] = [
            (MeguriLaunch(route: "nope/missing"), "-MeguriRoute: no route 'nope/missing'; registered: \(standardRoutes(for: build))"),
            (MeguriLaunch(extra: ["-MeguriBogus", "1"]), "unknown launch option -MeguriBogus"),
            (MeguriLaunch(now: "2030-02-30T10:00:00Z"), "-MeguriNow takes YYYY-MM-DDTHH:MM:SS"),
            (MeguriLaunch(timeZone: "Mars/Olympus"), "unknown time zone 'Mars/Olympus'"),
            (MeguriLaunch(fixture: URL(filePath: "/nonexistent/meguri/collection.anki2")), "no file or directory at /nonexistent/meguri/collection.anki2"),
        ]
        for (options, expected) in cases {
            let app = try launch(options, build)
            let message = app.staticTexts["testhooks.error.message"]
            XCTAssert(message.waitForExistence(timeout: 15), "no hook error for \(options.arguments)")
            let actual = text(app, "testhooks.error.message")
            if options.route != nil {
                // The unknown-route diagnostic must name exactly every current route.
                XCTAssertEqual(actual, expected)
            } else {
                XCTAssert(actual.contains(expected), "'\(actual)' lacks '\(expected)'")
            }
            XCTAssertFalse(app.descendants(matching: .any)["home.collection.screen"].exists, "the app ran despite the hook error")
            app.terminate()
        }
    }

    func testMacWindowSize() throws {
        #if os(macOS)
        let build = try requireHooks()
        let app = try launch(MeguriLaunch(animations: false, windowSize: "1000x700"), build)
        XCTAssert(app.descendants(matching: .any)["home.collection.screen"].waitForExistence(timeout: 15))
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

    /// U8d. A launch without `-MeguriBackgroundLaunch`, as by hand or by
    /// these suites, takes the regular policy: a Dock tile. With it, a Debug
    /// build stays a UI element (no Dock tile), as `tools/mg` launches it
    /// hidden on Antoine's Mac; Profile has no switch (R1866).
    func testOnlyABackgroundLaunchStaysOutOfTheDock() throws {
        #if os(macOS)
        let build = try requireHooks()
        let app = try launch(MeguriLaunch(animations: false, route: "sample/screen"), build)
        XCTAssert(app.staticTexts["sample.screen.title"].waitForExistence(timeout: 15), "the sample screen is not on screen")
        XCTAssertEqual(try running(build).activationPolicy, .regular, "a launch without the argument has no Dock tile")
        app.terminate()

        let background = try launch(MeguriLaunch(animations: false, route: "sample/screen", extra: ["-MeguriBackgroundLaunch", "YES"]), build)
        XCTAssert(background.staticTexts["sample.screen.title"].waitForExistence(timeout: 15), "the sample screen is not on screen")
        if build.configuration == .debug {
            XCTAssertEqual(try running(build).activationPolicy, .accessory, "a Debug background launch has a Dock tile")
        } else {
            XCTAssertEqual(try running(build).activationPolicy, .regular, "Profile took the background launch, which only Debug has")
        }
        #else
        throw XCTSkip("the background launch is Mac only")
        #endif
    }

    // MARK: Builds without hooks (Release, AppStore)

    func testLaunchOptionsAreIgnoredWithoutHooks() throws {
        let build = try BuildUnderTest.current()
        guard !build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has hooks") }
        let app = try launch(
            MeguriLaunch(
                fixture: build.hookFixture("single-file.fixture"), now: "2030-01-15T10:00:00Z", timeZone: "Asia/Tokyo", locale: "en_US",
                reduceMotion: true, route: "sample/sheet", extra: ["-MeguriImportSource", "/nonexistent/public.colpkg"] + ["-MeguriBogus", "1"]
            ),
            build
        )
        XCTAssert(app.descendants(matching: .any)["home.collection.screen"].waitForExistence(timeout: 15), "the app did not open on Home")
        XCTAssertFalse(app.staticTexts["sample.sheet.title"].waitForExistence(timeout: 3), "a Release-style build followed -MeguriRoute")
        XCTAssertFalse(app.staticTexts["testhooks.error.message"].exists, "a Release-style build parsed the launch options")
        XCTAssertFalse(app.descendants(matching: .any)["home.collection.error"].firstMatch.exists, "a Release-style build applied the hook fixture")
        XCTAssertFalse(app.descendants(matching: .any)["home.collection.retry"].firstMatch.exists, "a Release-style build applied the hook fixture and failed Home")
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
        let swiftNames = ["TestHooks", "LaunchOptions", "FixtureInstaller", "ImportSourceInstaller", "RouteRegistry", "AppearanceOverrides", "BackgroundLaunch", "TestHookError"]
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
        #if os(macOS)
        // The Mac shows the localized name only while Info.plist's display
        // name is the bundle's file name, Meguri.app.
        XCTAssertEqual(try build.appInfo["CFBundleDisplayName"] as? String, "Meguri")
        XCTAssertEqual(try build.appInfo["LSHasLocalizedDisplayName"] as? Bool, true)
        #else
        XCTAssertEqual(try build.appInfo["CFBundleDisplayName"] as? String, build.expectedDisplayName)
        #endif
        #if os(macOS) || targetEnvironment(simulator)
        // The localized names (Meguri/Identity): the Mac's app menu and About
        // panel read CFBundleName, which Info.plist keeps as PRODUCT_NAME.
        #if os(macOS)
        let strings = build.appBundle.appending(path: "Contents/Resources/en.lproj/InfoPlist.strings")
        #else
        let strings = build.appBundle.appending(path: "en.lproj/InfoPlist.strings")
        #endif
        let names = try XCTUnwrap(PropertyListSerialization.propertyList(
            from: Data(contentsOf: strings), format: nil) as? [String: String])
        XCTAssertEqual(names, ["CFBundleName": build.expectedDisplayName, "CFBundleDisplayName": build.expectedDisplayName])
        #endif
    }

    // MARK: Helpers

    // Independent inventory of the app's hook routes, including the full Statistics list.
    // Sync window routes also exist on iOS, where they show the Settings page.
    private func standardRoutes(for build: BuildUnderTest) -> String {
        var routes = [
            "browse/search/contains",
            "browse/search/save-search",
            "browse/search/tokens",
            "browse/sidebar/filtered",
            "browse/sidebar/today",
            "browse/sidebar/typical",
            "browse/table/empty-collection",
            "browse/table/error",
            "browse/table/large",
            "browse/table/no-results",
            "browse/table/notes",
            "browse/table/typical",
            "card/behaviors",
            "card/behaviors/fallback",
            "card/behaviors/graphic",
            "card/behaviors/replay-identifier",
            "card/behaviors/separator",
            "collection/deck-import/confirm-losses",
            "collection/deck-import/confirm-sync",
            "collection/deck-import/confirm-sync-losses",
            "collection/deck-import/confirm-unreachable",
            "collection/deck-import/drop-target",
            "collection/deck-import/imported-without-result",
            "collection/deck-import/importing",
            "collection/deck-import/importing-finishing",
            "collection/deck-import/live",
            "collection/deck-import/live-confirm",
            "collection/deck-import/live-refused",
            "collection/deck-import/options-changed",
            "collection/deck-import/options-large",
            "collection/deck-import/options-small",
            "collection/deck-import/refused-kind-conflict",
            "collection/deck-import/refused-rows",
            "collection/deck-import/refused-space",
            "collection/deck-import/refused-sync",
            "collection/deck-import/refused-too-new",
            "collection/deck-import/result",
            "collection/deck-import/result-log",
            "collection/deck-import/result-undone",
            "collection/deck-export/options",
            "collection/deck-export/options-changed",
            "collection/import/empty",
            "collection/import/error",
            "collection/import/loading",
            "collection/import/overflow",
            "collection/import/typical",
            "designsystem/gallery",
            "home/deck-list/beside-deck",
            "home/deck-list/beside-today",
            "home/deck-list/empty",
            "home/deck-list/start-empty-waits",
            "home/deck-list/typical",
            "home/sidebar/typical",
            "home/today/after-walk",
            "home/today/nothing-waiting",
            "home/today/summary",
            "navigation/feedback",
            "notetypes/cards/card-text",
            "notetypes/cards/issues",
            "notetypes/cards/preview-back",
            "notetypes/cards/preview-cloze",
            "notetypes/cards/preview-edited",
            "notetypes/cards/preview-empty",
            "notetypes/cards/preview-error",
            "notetypes/cards/preview-hidden",
            "notetypes/cards/styling",
            "notetypes/cards/typical",
            "notetypes/delete/meguri-sync",
            "notetypes/fields/edited",
            "notetypes/fields/inspector-hidden",
            "notetypes/fields/typical",
            "notetypes/first-run",
            "notetypes/list/typical",
            "notetypes/save/losses",
            "notetypes/save/meguri-not-set-up",
            "notetypes/save/meguri-sync",
            "notetypes/save/meguri-unreachable",
            "notetypes/save/uploading",
            "perf/probe",
            "review/congratulations/finished",
            "review/congratulations/finished-with-new-work",
            "review/congratulations/ready-to-continue",
            "review/congratulations/waiting-unavailable",
            "sample/alert",
            "sample/deck-overview/empty",
            "sample/deck-overview/error",
            "sample/deck-overview/loading",
            "sample/deck-overview/overflow",
            "sample/deck-overview/typical",
            "sample/defects",
            "sample/screen",
            "sample/sheet",
            "sample/touch-targets",
            "settings/account-sync/setup-needed",
            "settings/anki-transfer/empty",
            "settings/anki-transfer/error",
            "settings/anki-transfer/loading",
            "settings/anki-transfer/media-complete",
            "settings/anki-transfer/media-consent",
            "settings/anki-transfer/media-partial",
            "settings/anki-transfer/media-progress",
            "settings/anki-transfer/media-unknown",
            "settings/anki-transfer/overflow",
            "settings/anki-transfer/typical",
            "settings/backups/empty",
            "settings/backups/error",
            "settings/backups/loading",
            "settings/backups/overflow",
            "settings/backups/restore/empty",
            "settings/backups/restore/error",
            "settings/backups/restore/loading",
            "settings/backups/restore/overflow",
            "settings/backups/restore/typical",
            "settings/backups/typical",
            "settings/import-export/error",
            "settings/import-export/loading",
            "settings/import-export/typical",
            "settings/sync/icloud-connected",
            "settings/sync/icloud-issues",
            "settings/sync/icloud-needs-you",
            "settings/sync/icloud-selected",
            "settings/sync/issues-list",
            "settings/sync/keep-collection",
            "settings/sync/meguri-connected",
            "settings/sync/meguri-needs-you",
            "settings/sync/meguri-never-signed-in",
            "settings/sync/meguri-setup-media",
            "settings/sync/meguri-setup-media-paused",
            "settings/sync/meguri-setup-media-stopped",
            "settings/sync/meguri-sign-in",
            "settings/sync/meguri-structure-upload",
            "settings/sync/off",
            "settings/sync/panel/icloud-issues",
            "settings/sync/panel/icloud-needs-you",
            "settings/sync/panel/icloud-no-account",
            "settings/sync/panel/icloud-not-set-up",
            "settings/sync/panel/icloud-setup-needed",
            "settings/sync/panel/icloud-syncing",
            "settings/sync/panel/icloud-up-to-date",
            "settings/sync/panel/meguri-acceptance-unknown",
            "settings/sync/panel/meguri-blocked",
            "settings/sync/panel/meguri-choose-collection",
            "settings/sync/panel/meguri-media-blocked",
            "settings/sync/panel/meguri-never-signed-in",
            "settings/sync/panel/meguri-preparing",
            "settings/sync/panel/meguri-round-failed",
            "settings/sync/panel/meguri-setup-media",
            "settings/sync/panel/meguri-setup-media-paused",
            "settings/sync/panel/meguri-setup-media-stopped",
            "settings/sync/panel/meguri-sign-in-refused",
            "settings/sync/panel/meguri-structure-upload",
            "settings/sync/panel/meguri-syncing",
            "settings/sync/panel/meguri-up-to-date",
            "settings/sync/panel/off",
            "settings/sync/status-needs-you",
            "settings/sync/status/meguri-setup-media",
            "settings/sync/status/meguri-up-to-date",
            "settings/sync/status/off",
            "settings/sync/troubleshooting-icloud",
            "settings/sync/troubleshooting-meguri",
            "settings/sync/wide-icloud-needs-you",
            "settings/sync/wide-meguri-connected",
            "settings/sync/window/icloud-connected",
            "settings/sync/window/meguri-connected",
            "settings/sync/window/meguri-never-signed-in",
            "settings/sync/window/meguri-setup-media",
            "settings/sync/window/meguri-setup-media-paused",
            "settings/sync/window/meguri-sign-in",
            "settings/sync/window/switch-icloud-meguri-known",
            "settings/sync/window/switch-icloud-off-known",
            "settings/sync/window/switch-icloud-off-none",
            "settings/sync/window/switch-icloud-off-unknown",
            "settings/sync/window/switch-meguri-icloud-unknown",
            "settings/sync/window/switch-meguri-off-known",
            "settings/sync/window/switch-meguri-off-unknown",
            "shell/root/browse-pushed",
            "shell/root/deck-page",
            "shell/root/error",
            "shell/root/loading",
            "shell/root/review-answer",
            "shell/root/typical",
            "spike/card-swap",
            "spike/card-swap/capture",
            "spike/card-swap/typein",
            "spike/media-seek",
            "spike/media-seek/audio",
            "spike/storage/read",
            "spike/storage/write",
            "stats/chart/activity",
            "stats/chart/activity/1m",
            "stats/chart/activity/1y",
            "stats/chart/activity/3m",
            "stats/chart/activity/6m",
            "stats/chart/activity/ax5",
            "stats/chart/activity/failed",
            "stats/chart/activity/long",
            "stats/chart/activity/long-pinned",
            "stats/chart/activity/no-streak",
            "stats/chart/activity/pinned",
            "stats/chart/activity/recent",
            "stats/chart/added-and-started",
            "stats/chart/answers",
            "stats/chart/answers/1m",
            "stats/chart/answers/1y",
            "stats/chart/answers/3m",
            "stats/chart/answers/6m",
            "stats/chart/answers/contrast",
            "stats/chart/answers/deck",
            "stats/chart/answers/empty",
            "stats/chart/answers/failed",
            "stats/chart/answers/loading",
            "stats/chart/answers/pinned",
            "stats/chart/by-deck",
            "stats/chart/by-deck/1m",
            "stats/chart/by-deck/1y",
            "stats/chart/by-deck/contrast",
            "stats/chart/by-deck/deck",
            "stats/chart/by-deck/empty",
            "stats/chart/by-deck/failed",
            "stats/chart/by-deck/leaf",
            "stats/chart/by-deck/loading",
            "stats/chart/card-counts",
            "stats/chart/card-counts/failed",
            "stats/chart/cards-over-time",
            "stats/chart/cards-over-time/1m",
            "stats/chart/cards-over-time/1y",
            "stats/chart/cards-over-time/3m",
            "stats/chart/cards-over-time/6m",
            "stats/chart/cards-over-time/cold",
            "stats/chart/cards-over-time/contrast",
            "stats/chart/cards-over-time/deck",
            "stats/chart/cards-over-time/empty",
            "stats/chart/cards-over-time/failed",
            "stats/chart/cards-over-time/loading",
            "stats/chart/cards-over-time/pinned",
            "stats/chart/difficulty",
            "stats/chart/forgotten",
            "stats/chart/knowledge-over-time",
            "stats/chart/retention",
            "stats/chart/retrievability",
            "stats/chart/retrievability/contrast",
            "stats/chart/retrievability/deck",
            "stats/chart/retrievability/empty",
            "stats/chart/retrievability/failed",
            "stats/chart/retrievability/loading",
            "stats/chart/retrievability/pinned",
            "stats/chart/reviews-due",
            "stats/chart/reviews-due/1m",
            "stats/chart/reviews-due/1y",
            "stats/chart/reviews-due/3m",
            "stats/chart/reviews-due/6m",
            "stats/chart/reviews-due/contrast",
            "stats/chart/reviews-due/deck",
            "stats/chart/reviews-due/empty",
            "stats/chart/reviews-due/failed",
            "stats/chart/reviews-due/loading",
            "stats/chart/reviews-due/pinned",
            "stats/chart/reviews-over-time",
            "stats/chart/reviews-over-time/1m",
            "stats/chart/reviews-over-time/1y",
            "stats/chart/reviews-over-time/3m",
            "stats/chart/reviews-over-time/6m",
            "stats/chart/reviews-over-time/by-answer",
            "stats/chart/reviews-over-time/contrast",
            "stats/chart/reviews-over-time/deck",
            "stats/chart/reviews-over-time/empty",
            "stats/chart/reviews-over-time/failed",
            "stats/chart/reviews-over-time/loading",
            "stats/chart/reviews-over-time/pinned",
            "stats/chart/reviews-over-time/time",
            "stats/chart/stability",
            "stats/chart/stability/failed",
            "stats/chart/time-of-day",
            "stats/chart/time-of-day/failed",
            "stats/charts/areas",
            "stats/charts/areas/1m",
            "stats/charts/areas/ax5",
            "stats/charts/areas/contrast",
            "stats/charts/bars",
            "stats/charts/bars/1m",
            "stats/charts/bars/1y",
            "stats/charts/bars/3m",
            "stats/charts/bars/ax5",
            "stats/charts/bars/contrast",
            "stats/charts/edges",
            "stats/charts/edges/contrast",
            "stats/charts/lines",
            "stats/charts/lines/1y",
            "stats/charts/lines/ax5",
            "stats/charts/lines/contrast",
            "stats/charts/selection",
            "stats/charts/selection/1m",
            "stats/charts/selection/ax5",
            "stats/charts/selection/contrast",
            "stats/charts/states",
            "stats/charts/states/ax5",
            "stats/charts/states/contrast",
            "stats/deck/japanese",
            "stats/deck/languages",
            "stats/deck/review-ahead",
            "stats/page/ax5",
            "stats/page/cold",
            "stats/page/contrast",
            "stats/page/deleted-deck",
            "stats/page/failed-box",
            "stats/page/first-run",
            "stats/page/loading",
            "stats/page/loading-knowledge",
            "stats/page/no-cards",
            "stats/page/no-collection",
            "stats/page/no-reviews",
            "stats/page/pinned",
            "stats/page/slow",
            "stats/page/typical",
            "stats/page/unavailable",
            "stats/page/updating",
            "stats/period/1m",
            "stats/period/1y",
            "stats/period/3m",
            "stats/period/6m",
        ]
        #if os(macOS)
        routes += [
            "settings/integrations/listening",
            "settings/integrations/port-conflict",
        ]
        // Use the app's configuration: the UI runner's DEBUG flag is not its witness.
        if build.configuration == .debug {
            routes.append("card/replay-accessibility")
        }
        #endif
        return routes.sorted().joined(separator: ", ")
    }

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

    #if os(macOS)
    /// The running app under test, as AppKit sees it from this runner.
    private func running(_ build: BuildUnderTest) throws -> NSRunningApplication {
        let identifier = try build.bundleIdentifier
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: identifier).first { !$0.isTerminated }
        return try XCTUnwrap(running, "\(identifier) is not running")
    }
    #endif

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
