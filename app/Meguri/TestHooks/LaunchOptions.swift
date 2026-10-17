#if MEGURI_TEST_HOOKS
import AppModel
import Foundation

extension TestHooks {
    /// The hook launch arguments (R1963), each a `-Meguri<Name> <value>` pair.
    /// Other arguments (`-AppleLanguages`, `-ApplePersistenceIgnoreState`, …)
    /// are left to the system. An unknown `-Meguri…` name, a repeated one, a
    /// missing value or a bad value is an error, never ignored.
    ///
    /// | Argument | Value |
    /// |---|---|
    /// | `-MeguriFixture` | absolute path of a collection file (installed as `collection.anki2`) or of a collection directory (installed as the collection directory) |
    /// | `-MeguriNow` | `YYYY-MM-DDTHH:MM:SS` with `Z` or `±HH:MM`; without one, the time is local to `-MeguriTimeZone` (or the system zone) |
    /// | `-MeguriTimeZone` | IANA id, e.g. `Asia/Tokyo` |
    /// | `-MeguriLocale` | locale id, e.g. `en_US`; also pass `-AppleLanguages` and `-AppleLocale` for system strings (the UI-test launch helper does) |
    /// | `-MeguriAnimations` | `on` or `off` |
    /// | `-MeguriReduceMotion`, `-MeguriReduceTransparency` | `on` or `off` |
    /// | `-MeguriWindowSize` | Mac only: `minimum`, `default`, `large` or `<width>x<height>`; a launch with any other hook option and without this one gets `default` |
    /// | `-MeguriAppearance` | Mac only: `light` or `dark`, the app's appearance whatever the system's (NSApp.appearance; launched with `-AppleInterfaceStyle Light` on a Mac in dark mode, the app showed its dark background); iOS takes the device's |
    /// | `-MeguriAnkiConnectPort`, `-MeguriMigakuPort` | 1–65535 |
    /// | `-MeguriSizeClass` | iOS only: `compact` or `regular`, the window's horizontal size class whatever its width (R2–R5 on any simulator); `<bundle id>.testhooks.toggleSizeClass` flips it at run time |
    /// | `-MeguriIntegrations` | Mac only: `on` or `off`, the integrations flag U144 publishes (R11): on, closing the window keeps the app running; starts no server |
    /// | `-MeguriRoute` | a route registered in `RouteRegistry`, e.g. `sample/sheet` |
    struct LaunchOptions: Equatable {
        enum WindowSize: Equatable {
            case minimum, standard, large
            case explicit(width: Int, height: Int)
        }

        enum Appearance: String, Equatable {
            case light, dark
        }

        enum SizeClass: String, Equatable {
            case compact, regular
        }

        var fixture: URL?
        var now: Date?
        var timeZone: TimeZone?
        var locale: Locale?
        var animationsEnabled: Bool?
        var reduceMotion: Bool?
        var reduceTransparency: Bool?
        var windowSize: WindowSize?
        var appearance: Appearance?
        var ankiConnectPort: UInt16?
        var migakuPort: UInt16?
        var sizeClass: SizeClass?
        var integrations: Bool?
        var route: Route?

        static let prefix = "-Meguri"
        static let names = [
            "Fixture", "Now", "TimeZone", "Locale", "Animations", "ReduceMotion",
            "ReduceTransparency", "WindowSize", "Appearance", "AnkiConnectPort", "MigakuPort", "SizeClass",
            "Integrations", "Route",
        ]

        init() {}

        /// Parses the launch arguments, without the executable path.
        init(arguments: [String]) throws(TestHookError) {
            var values: [String: String] = [:]
            var index = arguments.startIndex
            while index < arguments.endIndex {
                let argument = arguments[index]
                index += 1
                guard argument.hasPrefix(Self.prefix) else { continue }
                let name = String(argument.dropFirst(Self.prefix.count))
                guard Self.names.contains(name) else {
                    throw TestHookError("unknown launch option \(argument); known: \(Self.names.map { Self.prefix + $0 }.joined(separator: ", "))")
                }
                guard values[name] == nil else {
                    throw TestHookError("launch option \(argument) is given twice")
                }
                guard index < arguments.endIndex else {
                    throw TestHookError("launch option \(argument) needs a value")
                }
                values[name] = arguments[index]
                index += 1
            }

            if let value = values["Fixture"] {
                guard value.hasPrefix("/") else {
                    throw TestHookError("-MeguriFixture needs an absolute path, got '\(value)'")
                }
                fixture = URL(filePath: value)
            }
            if let value = values["TimeZone"] {
                guard let zone = TimeZone(identifier: value) else {
                    throw TestHookError("-MeguriTimeZone: unknown time zone '\(value)'")
                }
                timeZone = zone
            }
            if let value = values["Now"] {
                now = try Self.instant(value, localZone: timeZone ?? .current)
            }
            if let value = values["Locale"] {
                guard Locale.availableIdentifiers.contains(value) else {
                    throw TestHookError("-MeguriLocale: unknown locale '\(value)'")
                }
                locale = Locale(identifier: value)
            }
            animationsEnabled = try values["Animations"].map { value throws(TestHookError) in try Self.onOff(value, "Animations") }
            reduceMotion = try values["ReduceMotion"].map { value throws(TestHookError) in try Self.onOff(value, "ReduceMotion") }
            reduceTransparency = try values["ReduceTransparency"].map { value throws(TestHookError) in
                try Self.onOff(value, "ReduceTransparency")
            }
            if let value = values["WindowSize"] {
                windowSize = try Self.windowSize(value)
            }
            if let value = values["Appearance"] {
                guard let named = Appearance(rawValue: value) else {
                    throw TestHookError("-MeguriAppearance takes light or dark, got '\(value)'")
                }
                appearance = named
            }
            ankiConnectPort = try values["AnkiConnectPort"].map { value throws(TestHookError) in try Self.port(value, "AnkiConnectPort") }
            migakuPort = try values["MigakuPort"].map { value throws(TestHookError) in try Self.port(value, "MigakuPort") }
            if let value = values["SizeClass"] {
                guard let named = SizeClass(rawValue: value) else {
                    throw TestHookError("-MeguriSizeClass takes compact or regular, got '\(value)'")
                }
                sizeClass = named
            }
            integrations = try values["Integrations"].map { value throws(TestHookError) in try Self.onOff(value, "Integrations") }
            if let value = values["Route"] {
                route = try Route(value)
            }
        }

        /// Installs the fixture and overrides the configuration. The route is
        /// resolved last, so its view sees every other override; a shell
        /// route's launch is returned (the app sets its navigation up).
        @discardableResult
        func apply(
            to configuration: inout LaunchConfiguration,
            installer: FixtureInstaller,
            routes: RouteRegistry
        ) throws(TestHookError) -> ShellLaunch? {
            #if os(macOS)
            if sizeClass != nil {
                throw TestHookError("-MeguriSizeClass applies to iOS only: the Mac has no size class")
            }
            #else
            if windowSize != nil {
                throw TestHookError("-MeguriWindowSize applies to the Mac only")
            }
            if appearance != nil {
                throw TestHookError("-MeguriAppearance applies to the Mac only")
            }
            if integrations != nil {
                throw TestHookError("-MeguriIntegrations applies to the Mac only")
            }
            #endif
            if let fixture {
                try installer.install(from: fixture, into: configuration.collectionLocation)
            }
            if let now {
                configuration.clock = .fixed(now)
            }
            if let timeZone {
                configuration.timeZone = timeZone
                // The process zone too, for code that reads TimeZone.current
                // or the TZ variable.
                setenv("TZ", timeZone.identifier, 1)
                tzset()
                NSTimeZone.resetSystemTimeZone()
                NSTimeZone.default = timeZone
            }
            if let locale {
                configuration.locale = locale
            }
            if let ankiConnectPort {
                configuration.ankiConnectPort = ankiConnectPort
            }
            if let migakuPort {
                configuration.migakuPort = migakuPort
            }
            guard let route else { return nil }
            switch try routes.resolve(route, configuration: configuration) {
            case let .presentation(presentation):
                configuration.initialPresentation = presentation
                return nil
            case let .shell(launch):
                return launch
            }
        }

        private static func onOff(_ value: String, _ name: String) throws(TestHookError) -> Bool {
            switch value {
            case "on": return true
            case "off": return false
            default: throw TestHookError("-Meguri\(name) takes on or off, got '\(value)'")
            }
        }

        private static func port(_ value: String, _ name: String) throws(TestHookError) -> UInt16 {
            guard let port = UInt16(value), port > 0 else {
                throw TestHookError("-Meguri\(name) takes a port from 1 to 65535, got '\(value)'")
            }
            return port
        }

        /// A `-MeguriWindowSize` value, or a window resize request's
        /// (`AppearanceOverrides.WindowHookView`).
        static func windowSize(_ value: String) throws(TestHookError) -> WindowSize {
            switch value {
            case "minimum": return .minimum
            case "default": return .standard
            case "large": return .large
            default:
                let parts = value.split(separator: "x", omittingEmptySubsequences: false)
                guard parts.count == 2, let width = Int(parts[0]), let height = Int(parts[1]), width > 0, height > 0 else {
                    throw TestHookError("-MeguriWindowSize takes minimum, default, large or <width>x<height>, got '\(value)'")
                }
                return .explicit(width: width, height: height)
            }
        }

        /// `YYYY-MM-DDTHH:MM:SS` plus `Z`, `±HH:MM` or nothing (local to `localZone`).
        static func instant(_ value: String, localZone: TimeZone) throws(TestHookError) -> Date {
            let invalid = TestHookError("-MeguriNow takes YYYY-MM-DDTHH:MM:SS with an optional Z or ±HH:MM, got '\(value)'")
            let pattern = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(Z|[+-]\d{2}:\d{2})?$/
            guard let match = value.wholeMatch(of: pattern) else { throw invalid }
            var zone = localZone
            if let designator = match.7 {
                if designator == "Z" {
                    zone = TimeZone(secondsFromGMT: 0)!
                } else {
                    let sign = designator.first == "-" ? -1 : 1
                    let hours = Int(designator.dropFirst().prefix(2))!
                    let minutes = Int(designator.suffix(2))!
                    guard hours < 24, minutes < 60, let offset = TimeZone(secondsFromGMT: sign * (hours * 3600 + minutes * 60)) else {
                        throw invalid
                    }
                    zone = offset
                }
            }
            var calendar = Calendar(identifier: .gregorian)
            calendar.timeZone = zone
            let components = DateComponents(
                calendar: calendar, timeZone: zone,
                year: Int(match.1), month: Int(match.2), day: Int(match.3),
                hour: Int(match.4), minute: Int(match.5), second: Int(match.6)
            )
            guard components.isValidDate, let date = components.date else { throw invalid }
            return date
        }
    }
}
#endif
