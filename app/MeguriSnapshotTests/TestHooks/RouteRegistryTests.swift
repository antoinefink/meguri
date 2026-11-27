#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import SwiftUI
import Testing

@testable import Meguri

#if os(macOS)
private let expectedBackupsRestorePresentation: InitialPresentation.Style = .screen
#else
private let expectedBackupsRestorePresentation: InitialPresentation.Style = .sheet
#endif

/// Launch routes (U7, R1963), in the app process.
@MainActor
@Suite struct RouteRegistryTests {
    // Independent inventory: keep literal expectations separate from registration.
    // Sync window routes also exist on iOS, where they show the Settings page.
    static let standardRoutes: String = {
        var routes = [
            "browse/search/contains",
            "browse/search/save-search",
            "browse/search/tokens",
            "browse/sidebar/filtered",
            "browse/sidebar/keyboard-target",
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
            "settings/sync/troubleshooting-meguri",
            "settings/sync/wide-meguri-connected",
            "settings/sync/window/meguri-connected",
            "settings/sync/window/meguri-never-signed-in",
            "settings/sync/window/meguri-setup-media",
            "settings/sync/window/meguri-setup-media-paused",
            "settings/sync/window/meguri-sign-in",
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
        #endif
        #if DEBUG && os(macOS)
        routes.append("card/replay-accessibility")
        #endif
        #if os(iOS)
        routes.append("navigation/feedback-cold")
        #endif
        return routes.sorted().joined(separator: ", ")
    }()

    /// `-MeguriStatsRollout` (U296): off, on or open; anything else refuses the launch. Off hides
    /// Statistics, which shows by default on the Mac and the iPad.
    @Test func theStatsRolloutOptionParses() throws {
        #expect(try TestHooks.LaunchOptions(arguments: []).statsRollout == nil)
        #expect(try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "off"]).statsRollout == nil)
        // Off hides Statistics where it shows by default (the Mac, the iPad).
        #expect(try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "off"]).statsRolloutOff)
        #expect(try !TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "on"]).statsRolloutOff)
        #expect(try !TestHooks.LaunchOptions(arguments: []).statsRolloutOff)
        #expect(try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "on"]).statsRollout == .on)
        #expect(try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "open"]).statsRollout == .open)
        #expect(throws: TestHookError.self) { try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "yes"]) }
        let section = try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "open:reviews"])
        #expect(section.statsRollout == .open && section.statsSection == "reviews")
        #expect(try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "open"]).statsSection == nil)
        #expect(throws: TestHookError.self) { try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "open:nowhere"]) }
        #expect(throws: TestHookError.self) { try TestHooks.LaunchOptions(arguments: ["-MeguriStatsRollout", "on:reviews"]) }
    }

    @Test func theStandardRegistryHasTheSampleGalleryPerfShellAndSpikeRoutes() {
        #expect(TestHooks.RouteRegistry.standard.routes.map(\.path).joined(separator: ", ") == Self.standardRoutes)
    }

    @Test(arguments: [
        ("sample/screen", InitialPresentation.Style.screen),
        ("sample/defects", .screen),
        ("sample/sheet", .sheet),
        ("sample/alert", .alert),
        ("designsystem/gallery", .screen),
        ("sample/deck-overview/typical", .screen),
        ("sample/touch-targets", .screen),
        ("card/behaviors/replay-identifier", .screen),
        ("collection/deck-export/options", .sheet),
        ("collection/deck-export/options-changed", .sheet),
        ("collection/import/empty", .sheet),
        ("collection/import/typical", .sheet),
        ("collection/import/overflow", .sheet),
        ("collection/import/error", .sheet),
        ("collection/import/loading", .sheet),
        ("home/deck-list/beside-deck", .screen),
        ("home/deck-list/beside-today", .screen),
        ("home/deck-list/empty", .screen),
        ("home/deck-list/start-empty-waits", .screen),
        ("home/deck-list/typical", .screen),
        ("home/sidebar/typical", .screen),
        ("home/today/after-walk", .screen),
        ("home/today/nothing-waiting", .screen),
        ("home/today/summary", .screen),
        ("settings/account-sync/setup-needed", .screen),
        ("settings/anki-transfer/empty", .screen),
        ("settings/anki-transfer/error", .screen),
        ("settings/anki-transfer/loading", .screen),
        ("settings/anki-transfer/typical", .screen),
        ("settings/anki-transfer/overflow", .screen),
        ("settings/anki-transfer/media-consent", .screen),
        ("settings/anki-transfer/media-progress", .screen),
        ("settings/anki-transfer/media-partial", .screen),
        ("settings/anki-transfer/media-complete", .screen),
        ("settings/anki-transfer/media-unknown", .screen),
        ("settings/backups/empty", .screen),
        ("settings/backups/typical", .screen),
        ("settings/backups/overflow", .screen),
        ("settings/backups/error", .screen),
        ("settings/backups/loading", .screen),
        ("settings/backups/restore/empty", expectedBackupsRestorePresentation),
        ("settings/backups/restore/typical", expectedBackupsRestorePresentation),
        ("settings/backups/restore/overflow", expectedBackupsRestorePresentation),
        ("settings/backups/restore/error", expectedBackupsRestorePresentation),
        ("settings/backups/restore/loading", expectedBackupsRestorePresentation),
        ("settings/import-export/error", .sheet),
        ("settings/import-export/loading", .sheet),
        ("settings/import-export/typical", .sheet),
        ("settings/sync/keep-collection", .screen),
        ("settings/sync/meguri-connected", .screen),
        ("settings/sync/meguri-needs-you", .screen),
        ("settings/sync/meguri-setup-media", .screen),
        ("settings/sync/meguri-setup-media-paused", .screen),
        ("settings/sync/meguri-setup-media-stopped", .screen),
        ("settings/sync/meguri-sign-in", .screen),
        ("settings/sync/off", .screen),
        ("settings/sync/panel/meguri-acceptance-unknown", .screen),
        ("settings/sync/panel/meguri-blocked", .screen),
        ("settings/sync/panel/meguri-choose-collection", .screen),
        ("settings/sync/panel/meguri-media-blocked", .screen),
        ("settings/sync/panel/meguri-preparing", .screen),
        ("settings/sync/panel/meguri-round-failed", .screen),
        ("settings/sync/panel/meguri-setup-media", .screen),
        ("settings/sync/panel/meguri-setup-media-paused", .screen),
        ("settings/sync/panel/meguri-setup-media-stopped", .screen),
        ("settings/sync/panel/meguri-sign-in-refused", .screen),
        ("settings/sync/panel/meguri-syncing", .screen),
        ("settings/sync/panel/meguri-up-to-date", .screen),
        ("settings/sync/panel/off", .screen),
        ("settings/sync/status-needs-you", .screen),
        ("settings/sync/status/meguri-setup-media", .screen),
        ("settings/sync/status/meguri-up-to-date", .screen),
        ("settings/sync/status/off", .screen),
        ("settings/sync/troubleshooting-meguri", .screen),
        ("settings/sync/wide-meguri-connected", .screen),
        ("settings/sync/window/meguri-connected", .screen),
        ("settings/sync/window/meguri-setup-media", .screen),
        ("settings/sync/window/meguri-setup-media-paused", .screen),
        ("settings/sync/meguri-never-signed-in", .screen),
        ("settings/sync/panel/meguri-never-signed-in", .screen),
        ("settings/sync/window/meguri-never-signed-in", .screen),
        ("settings/sync/window/meguri-sign-in", .screen),
        ("settings/sync/window/switch-meguri-off-unknown", .screen),
        ("settings/sync/window/switch-meguri-off-known", .screen),
        ("review/congratulations/finished", .screen),
        ("review/congratulations/finished-with-new-work", .screen),
        ("review/congratulations/ready-to-continue", .screen),
        ("review/congratulations/waiting-unavailable", .screen),
        ("perf/probe", .screen),
        ("spike/card-swap", .screen),
        ("spike/media-seek/audio", .screen),
    ])
    func eachRouteHasItsPresentation(path: String, style: InitialPresentation.Style) throws {
        let presentation = try TestHooks.RouteRegistry.standard.presentation(for: TestHooks.Route(path), configuration: LaunchConfiguration())
        #expect(presentation.style == style)
    }

    @Test func anUnknownRouteNamesTheRegisteredOnes() throws {
        let route = try TestHooks.Route("review/answer")
        #expect(throws: TestHookError("-MeguriRoute: no route 'review/answer'; registered: \(Self.standardRoutes)")) {
            try TestHooks.RouteRegistry.standard.presentation(for: route, configuration: LaunchConfiguration())
        }
    }

    /// A shell route resolves to the shell's state, not to a presentation.
    @Test func aShellRouteOpensTheShellInItsState() throws {
        let route = try TestHooks.Route("shell/root/browse-pushed")
        guard case let .shell(launch) = try TestHooks.RouteRegistry.standard.resolve(route, configuration: LaunchConfiguration()) else {
            Issue.record("shell/root/browse-pushed is not a shell route")
            return
        }
        #expect(launch.probe)
        let navigation = AppNavigation()
        launch.prepare(navigation)
        #expect(navigation.section == .browse)
        #expect(navigation.selectedDeck == TestHooks.ShellSample.spanishMining)
        #expect(navigation.path(.browse) == [.noteTypes])
        #expect(navigation.scrollPosition(for: .section(.browse)) == ScrollPosition(id: "row-20", anchor: .top))
        #expect(throws: TestHookError("-MeguriRoute: 'shell/root/browse-pushed' opens the shell in a state; it presents nothing")) {
            try TestHooks.RouteRegistry.standard.presentation(for: route, configuration: LaunchConfiguration())
        }
    }

    @Test(arguments: ["typical", "loading", "error"])
    func theShellStateRoutesShowNoProbe(state: String) throws {
        guard case let .shell(launch) = try TestHooks.RouteRegistry.standard.resolve(TestHooks.Route("shell/root/\(state)"), configuration: LaunchConfiguration()) else {
            Issue.record("shell/root/\(state) is not a shell route")
            return
        }
        #expect(!launch.probe, "an audit or a snapshot of the shell would include the probe")
    }

    @Test(arguments: [nil, "navigation/feedback", "navigation/feedback-cold", "sample/screen", "shell/root/loading"] as [String?])
    func onlyTouchNavigationFixturesPresentTheirOwnShell(path: String?) throws {
        var options = TestHooks.LaunchOptions()
        if let path { options.route = try TestHooks.Route(path) }
        let session = TestHooks.Session(options: options)
        let navigation = AppNavigation()
        var configuration = LaunchConfiguration()
        configuration.initialPresentation = .screen { EmptyView() }
        let root = TestHooks.Root(session: session, configuration: configuration, navigation: navigation) { EmptyView() }
        #if os(iOS)
        let expected = path == "navigation/feedback" || path == "navigation/feedback-cold"
        #else
        let expected = false
        #endif
        #expect((root.standaloneShellContent != nil) == expected)

        configuration.initialPresentation = nil
        let normalRoot = TestHooks.Root(session: session, configuration: configuration, navigation: navigation) { EmptyView() }
        #expect(normalRoot.standaloneShellContent == nil)
    }

    @Test func handlersSeeTheOtherOverrides() throws {
        var registry = TestHooks.RouteRegistry()
        var seen: TimeZone?
        registry.register("probe/zone") { context in
            seen = context.configuration.timeZone
            return .alert(title: context.route.path, message: "") { Button("OK") {} }
        }
        var configuration = LaunchConfiguration()
        let options = try TestHooks.LaunchOptions(arguments: ["-MeguriTimeZone", "Asia/Tokyo", "-MeguriRoute", "probe/zone"])
        let previous = TimeZone.current.identifier
        defer {
            setenv("TZ", previous, 1)
            tzset()
            NSTimeZone.resetSystemTimeZone()
            NSTimeZone.default = TimeZone(identifier: previous)!
        }
        try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "x.dev"), routes: registry)
        #expect(seen?.identifier == "Asia/Tokyo")
        #expect(configuration.initialPresentation?.alertTitle == "probe/zone")
    }
}
#endif
