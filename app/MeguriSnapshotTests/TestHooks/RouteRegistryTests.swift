#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import SwiftUI
import Testing

@testable import Meguri

/// Launch routes (U7, R1963), in the app process.
@MainActor
@Suite struct RouteRegistryTests {
    static let standardRoutes: String = {
        #if DEBUG && os(macOS)
        let replayAccessibilityRoute = "card/replay-accessibility, "
        #else
        let replayAccessibilityRoute = ""
        #endif
        return "card/behaviors, card/behaviors/fallback, card/behaviors/graphic, card/behaviors/replay-identifier, card/behaviors/separator, \(replayAccessibilityRoute)collection/import/empty, collection/import/error, collection/import/loading, collection/import/overflow, collection/import/typical, designsystem/gallery, home/deck-list/beside-deck, home/deck-list/beside-today, home/deck-list/empty, home/deck-list/typical, home/sidebar/typical, home/today/after-walk, home/today/nothing-waiting, home/today/summary, perf/probe, review/congratulations/finished, review/congratulations/finished-with-new-work, review/congratulations/ready-to-continue, review/congratulations/waiting-unavailable, sample/alert, sample/deck-overview/empty, sample/deck-overview/error, sample/deck-overview/loading, sample/deck-overview/overflow, sample/deck-overview/typical, sample/defects, sample/screen, sample/sheet, sample/touch-targets, settings/account-sync/setup-needed, settings/anki-transfer/empty, settings/anki-transfer/error, settings/anki-transfer/loading, settings/anki-transfer/media-complete, settings/anki-transfer/media-consent, settings/anki-transfer/media-partial, settings/anki-transfer/media-progress, settings/anki-transfer/media-unknown, settings/anki-transfer/overflow, settings/anki-transfer/typical, settings/import-export/error, settings/import-export/loading, settings/import-export/typical, settings/sync/icloud-connected, settings/sync/icloud-issues, settings/sync/icloud-needs-you, settings/sync/icloud-selected, settings/sync/issues-list, settings/sync/keep-collection, settings/sync/meguri-connected, settings/sync/meguri-needs-you, settings/sync/meguri-setup-media, settings/sync/meguri-setup-media-paused, settings/sync/meguri-setup-media-stopped, settings/sync/meguri-sign-in, settings/sync/off, settings/sync/panel/icloud-issues, settings/sync/panel/icloud-needs-you, settings/sync/panel/icloud-no-account, settings/sync/panel/icloud-not-set-up, settings/sync/panel/icloud-setup-needed, settings/sync/panel/icloud-syncing, settings/sync/panel/icloud-up-to-date, settings/sync/panel/meguri-acceptance-unknown, settings/sync/panel/meguri-blocked, settings/sync/panel/meguri-choose-collection, settings/sync/panel/meguri-media-blocked, settings/sync/panel/meguri-preparing, settings/sync/panel/meguri-round-failed, settings/sync/panel/meguri-setup-media, settings/sync/panel/meguri-setup-media-paused, settings/sync/panel/meguri-setup-media-stopped, settings/sync/panel/meguri-sign-in-refused, settings/sync/panel/meguri-syncing, settings/sync/panel/meguri-up-to-date, settings/sync/panel/off, settings/sync/status-needs-you, settings/sync/status/meguri-setup-media, settings/sync/status/meguri-up-to-date, settings/sync/status/off, settings/sync/troubleshooting-icloud, settings/sync/troubleshooting-meguri, settings/sync/wide-icloud-needs-you, settings/sync/wide-meguri-connected, settings/sync/window/icloud-connected, settings/sync/window/meguri-connected, settings/sync/window/meguri-setup-media, settings/sync/window/meguri-setup-media-paused, shell/root/browse-pushed, shell/root/deck-page, shell/root/error, shell/root/loading, shell/root/review-answer, shell/root/typical, spike/card-swap, spike/card-swap/capture, spike/card-swap/typein, spike/media-seek, spike/media-seek/audio, spike/storage/read, spike/storage/write"
    }()

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
        ("collection/import/empty", .sheet),
        ("collection/import/typical", .sheet),
        ("collection/import/overflow", .sheet),
        ("collection/import/error", .sheet),
        ("collection/import/loading", .sheet),
        ("home/deck-list/beside-deck", .screen),
        ("home/deck-list/beside-today", .screen),
        ("home/deck-list/empty", .screen),
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
        ("settings/import-export/error", .sheet),
        ("settings/import-export/loading", .sheet),
        ("settings/import-export/typical", .sheet),
        ("settings/sync/icloud-connected", .screen),
        ("settings/sync/icloud-issues", .screen),
        ("settings/sync/icloud-needs-you", .screen),
        ("settings/sync/icloud-selected", .screen),
        ("settings/sync/issues-list", .screen),
        ("settings/sync/keep-collection", .screen),
        ("settings/sync/meguri-connected", .screen),
        ("settings/sync/meguri-needs-you", .screen),
        ("settings/sync/meguri-setup-media", .screen),
        ("settings/sync/meguri-setup-media-paused", .screen),
        ("settings/sync/meguri-setup-media-stopped", .screen),
        ("settings/sync/meguri-sign-in", .screen),
        ("settings/sync/off", .screen),
        ("settings/sync/panel/icloud-issues", .screen),
        ("settings/sync/panel/icloud-needs-you", .screen),
        ("settings/sync/panel/icloud-no-account", .screen),
        ("settings/sync/panel/icloud-not-set-up", .screen),
        ("settings/sync/panel/icloud-setup-needed", .screen),
        ("settings/sync/panel/icloud-syncing", .screen),
        ("settings/sync/panel/icloud-up-to-date", .screen),
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
        ("settings/sync/troubleshooting-icloud", .screen),
        ("settings/sync/troubleshooting-meguri", .screen),
        ("settings/sync/wide-icloud-needs-you", .screen),
        ("settings/sync/wide-meguri-connected", .screen),
        ("settings/sync/window/icloud-connected", .screen),
        ("settings/sync/window/meguri-connected", .screen),
        ("settings/sync/window/meguri-setup-media", .screen),
        ("settings/sync/window/meguri-setup-media-paused", .screen),
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
