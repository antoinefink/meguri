#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import SwiftUI
import Testing

@testable import Meguri

/// Launch routes (U7, R1963), in the app process.
@MainActor
@Suite struct RouteRegistryTests {
    static let standardRoutes = "card/behaviors, card/behaviors/fallback, card/behaviors/graphic, card/behaviors/separator, collection/import/empty, collection/import/error, collection/import/loading, collection/import/overflow, collection/import/typical, designsystem/gallery, home/deck-list/empty, home/deck-list/typical, perf/probe, sample/alert, sample/deck-overview/empty, sample/deck-overview/error, sample/deck-overview/loading, sample/deck-overview/overflow, sample/deck-overview/typical, sample/defects, sample/screen, sample/sheet, sample/touch-targets, settings/anki-transfer/empty, settings/anki-transfer/error, settings/anki-transfer/loading, settings/anki-transfer/typical, settings/import-export/error, settings/import-export/loading, settings/import-export/typical, shell/root/browse-pushed, shell/root/deck-page, shell/root/error, shell/root/loading, shell/root/review-answer, shell/root/typical, spike/card-swap, spike/card-swap/capture, spike/card-swap/typein, spike/media-seek, spike/media-seek/audio, spike/storage/read, spike/storage/write"

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
        ("collection/import/empty", .sheet),
        ("collection/import/typical", .sheet),
        ("collection/import/overflow", .sheet),
        ("collection/import/error", .sheet),
        ("collection/import/loading", .sheet),
        ("home/deck-list/empty", .screen),
        ("home/deck-list/typical", .screen),
        ("settings/anki-transfer/empty", .screen),
        ("settings/anki-transfer/error", .screen),
        ("settings/anki-transfer/loading", .screen),
        ("settings/anki-transfer/typical", .screen),
        ("settings/import-export/error", .sheet),
        ("settings/import-export/loading", .sheet),
        ("settings/import-export/typical", .sheet),
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
