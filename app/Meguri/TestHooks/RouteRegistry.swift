#if MEGURI_TEST_HOOKS
import AppModel
import Foundation

extension TestHooks {
    /// A launch route: the screen and state the app opens on (R1963), written
    /// `area/screen[/state…]` with lowercase segments, e.g. `review/answer`.
    struct Route: Hashable, Comparable, CustomStringConvertible {
        let path: String

        init(_ path: String) throws(TestHookError) {
            guard path.wholeMatch(of: /[a-z0-9][a-z0-9-]*(\/[a-z0-9][a-z0-9-]*)+/) != nil else {
                throw TestHookError("-MeguriRoute: '\(path)' is not a route (area/screen[/state], lowercase letters, digits and '-')")
            }
            self.path = path
        }

        var description: String { path }

        static func < (lhs: Route, rhs: Route) -> Bool { lhs.path < rhs.path }
    }

    /// What a route handler gets: the route and the launch configuration with
    /// every other override already applied.
    struct RouteContext {
        let route: Route
        let configuration: LaunchConfiguration
    }

    /// Maps routes to what the app shows at launch. Each UI area registers its
    /// routes (screens and their states, menus, sheets, error states) in
    /// `standard`, from a `<Area>Routes.swift` file in this folder.
    struct RouteRegistry {
        typealias Handler = (RouteContext) -> InitialPresentation

        private var handlers: [Route: Handler] = [:]

        static var standard: RouteRegistry {
            var registry = RouteRegistry()
            SampleRoutes.register(in: &registry)
            DesignSystemRoutes.register(in: &registry)
            return registry
        }

        var routes: [Route] { handlers.keys.sorted() }

        /// Registers `path`; a malformed or repeated path is a programming error.
        mutating func register(_ path: String, _ handler: @escaping Handler) {
            let route: Route
            do {
                route = try Route(path)
            } catch {
                preconditionFailure(error.message)
            }
            precondition(handlers[route] == nil, "route \(path) is registered twice")
            handlers[route] = handler
        }

        func presentation(for route: Route, configuration: LaunchConfiguration) throws(TestHookError) -> InitialPresentation {
            guard let handler = handlers[route] else {
                throw TestHookError(
                    "-MeguriRoute: no route '\(route)'; registered: \(routes.map(\.path).joined(separator: ", "))"
                )
            }
            return handler(RouteContext(route: route, configuration: configuration))
        }
    }
}
#endif
