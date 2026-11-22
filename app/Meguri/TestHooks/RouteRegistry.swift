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
    /// `standard`, from a `<Area>Routes.swift` file in this folder. A route
    /// either presents a view over the shell (`register`) or puts the shell
    /// itself in a state, with the features it hosts (`registerShell`).
    struct RouteRegistry {
        typealias Handler = (RouteContext) -> InitialPresentation
        typealias ShellHandler = (RouteContext) -> ShellLaunch

        enum Resolution {
            case presentation(InitialPresentation)
            case shell(ShellLaunch)
        }

        private var handlers: [Route: Handler] = [:]
        private var shellHandlers: [Route: ShellHandler] = [:]

        static var standard: RouteRegistry {
            var registry = RouteRegistry()
            SampleRoutes.register(in: &registry)
            DesignSystemRoutes.register(in: &registry)
            PerfRoutes.register(in: &registry)
            ShellRoutes.register(in: &registry)
            SpikeRoutes.register(in: &registry)
            CardBehaviorRoutes.register(in: &registry)
            #if DEBUG && os(macOS)
            ReplayAccessibilityRoute.register(in: &registry)
            #endif
            CollectionEntryRoutes.register(in: &registry)
            SyncRoutes.register(in: &registry)
            ReviewRoutes.register(in: &registry)
            BrowseRoutes.register(in: &registry)
            return registry
        }

        var routes: [Route] { (Array(handlers.keys) + Array(shellHandlers.keys)).sorted() }

        /// Registers `path`; a malformed or repeated path is a programming error.
        mutating func register(_ path: String, _ handler: @escaping Handler) {
            handlers[newRoute(path)] = handler
        }

        /// Registers a shell route: `path` opens the shell in a state.
        mutating func registerShell(_ path: String, _ handler: @escaping ShellHandler) {
            shellHandlers[newRoute(path)] = handler
        }

        private func newRoute(_ path: String) -> Route {
            let route: Route
            do {
                route = try Route(path)
            } catch {
                preconditionFailure(error.message)
            }
            precondition(handlers[route] == nil && shellHandlers[route] == nil, "route \(path) is registered twice")
            return route
        }

        func resolve(_ route: Route, configuration: LaunchConfiguration) throws(TestHookError) -> Resolution {
            let context = RouteContext(route: route, configuration: configuration)
            if let handler = handlers[route] {
                return .presentation(handler(context))
            }
            if let handler = shellHandlers[route] {
                return .shell(handler(context))
            }
            throw TestHookError("-MeguriRoute: no route '\(route)'; registered: \(routes.map(\.path).joined(separator: ", "))")
        }

        /// The presentation a presenting route shows.
        func presentation(for route: Route, configuration: LaunchConfiguration) throws(TestHookError) -> InitialPresentation {
            guard case let .presentation(presentation) = try resolve(route, configuration: configuration) else {
                throw TestHookError("-MeguriRoute: '\(route)' opens the shell in a state; it presents nothing")
            }
            return presentation
        }
    }
}
#endif
