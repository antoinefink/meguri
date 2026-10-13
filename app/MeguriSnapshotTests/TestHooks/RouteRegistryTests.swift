#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import SwiftUI
import Testing

@testable import Meguri

/// Launch routes (U7, R1963), in the app process.
@MainActor
@Suite struct RouteRegistryTests {
    @Test func theStandardRegistryHasTheSampleRoutes() {
        #expect(TestHooks.RouteRegistry.standard.routes.map(\.path) == ["sample/alert", "sample/defects", "sample/screen", "sample/sheet"])
    }

    @Test(arguments: [
        ("sample/screen", InitialPresentation.Style.screen),
        ("sample/defects", .screen),
        ("sample/sheet", .sheet),
        ("sample/alert", .alert),
    ])
    func eachSampleRouteHasItsPresentation(path: String, style: InitialPresentation.Style) throws {
        let presentation = try TestHooks.RouteRegistry.standard.presentation(for: TestHooks.Route(path), configuration: LaunchConfiguration())
        #expect(presentation.style == style)
    }

    @Test func anUnknownRouteNamesTheRegisteredOnes() throws {
        let route = try TestHooks.Route("review/answer")
        #expect(throws: TestHookError("-MeguriRoute: no route 'review/answer'; registered: sample/alert, sample/defects, sample/screen, sample/sheet")) {
            try TestHooks.RouteRegistry.standard.presentation(for: route, configuration: LaunchConfiguration())
        }
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
