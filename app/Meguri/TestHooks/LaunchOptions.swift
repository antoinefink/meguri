#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
#if os(iOS)
import UIKit
#endif

extension TestHooks {
    /// The hook launch arguments (R1963), each a `-Meguri<Name> <value>` pair.
    /// Other arguments (`-AppleLanguages`, `-ApplePersistenceIgnoreState`, …)
    /// are left to the system. An unknown `-Meguri…` name, a repeated one, a
    /// missing value or a bad value is an error, never ignored.
    ///
    /// | Argument | Value |
    /// |---|---|
    /// | `-MeguriNativeReviewRoot`, `-MeguriNativeReviewCase`, `-MeguriNativeReviewPlatform` | compiled checkout root, run case component, mac/iphone/ipad; all NativeReview options required together |
    /// | `-MeguriNativeReviewFreezeSHA256`, `-MeguriNativeReviewFreezeBytes` | pinned freeze.json SHA256/size; only the compiled checkout's EmptyCollection fixture is allowed, before any case receipt; no Route/Now/TimeZone |
    /// | `-MeguriImportSource` | iOS only: absolute regular synthetic .colpkg copied to Documents/RF2 Import Tests/review.colpkg for the system picker; changes no collection or route |
    /// | `-MeguriFixture` | absolute path of a collection file (installed as `collection.anki2`) or of a collection directory (installed as the collection directory) |
    /// | `-MeguriFixtureRunID` | Debug .dev only: canonical UUID for an isolated synthetic collection, backups and provider location; give Fixture only on the first launch, reuse the UUID on every relaunch |
    /// | `-MeguriOwnedProviderFixture` | `on` or `off` (default): Mac .dev only, isolated normal-provider launch of this checkout's local-review-1 recipe (owned-sync-review-1 only with admitted transport input) |
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
    /// | `-MeguriBackgroundLaunch` | Mac only: `YES` or `NO`; in Debug, `YES` keeps the app out of the Dock and never active (`BackgroundLaunch`, which reads it itself; Profile ignores it). Not a UI-state option: it changes neither the restored state nor the window size |
    /// | `-MeguriStudyTimings` | `on` or `off` (default): a strip below the shell gives the last Study's way to its first card (`StudyStartTrace`, R1813) as `testhooks.studyStart.timings` |
    /// | `-MeguriHomeSnapshot` | `on` or `off` (default): `on` keeps Home's last read between launches (`HomeSnapshotStore`, N27.2) as a launch with no hook option does; a hook launch otherwise keeps none |
    /// | `-MeguriHomeTrace` | `on` or `off` (default): a strip below the shell gives every Home phase shown since launch, in order (`HomePhase`, N27.1), as `testhooks.home.phases` |
    /// | `-MeguriStatsRollout` | `off`, `on` or `open`: `off` hides Statistics where it shows by default (the Mac, the iPad; U307's part), `on` turns `StatsRollout` on (U296) wherever the app runs, so the sidebar's Statistics row and View › Statistics (⇧⌘S) show; `open` also opens the window on Statistics once the shell starts (captures and performance tests over `-MeguriFixture`); `open:<section>` then scrolls the page to that section (`knowledge`, `reviews`, `cards`, `comingUp`, `answering`, `memory`, `decks`), for Mac captures, which cannot scroll. A `stats/…` route turns it on by itself |
    /// | `-MeguriSyncRender` | a `SyncCoordinator.RenderState` name (`meguriConnected`, `meguriSyncing`, …), optionally followed by `+justFinished`: with `-MeguriFixture`, once the collection opens the shell shows that synthetic sync status in place of the real one; with a shell route, the shell gets the sync button over that status (captures of the sync button; nothing syncs) |
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

        /// All five fields are required together. Root must be this build’s
        /// checkout; case resolves only build/mg/ui-test/<case>/expected-replay.
        var nativeReview: NativeReviewInput.Request?
        var fixture: URL?
        var fixtureRunID: UUID?
        var ownedProviderFixture = false
        private(set) var ownedProviderSource: OwnedProviderFixture.Source = .localReview
        var importSource: URL?
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
        var studyTimings = false
        /// Nil when not given: an explicit off differs from a launch with no option.
        var homeSnapshot: Bool?
        var homeTrace = false
        var statsRollout: StatsRolloutOption?
        /// `-MeguriStatsRollout off`: Statistics hidden where it would show (the Mac, the iPad).
        var statsRolloutOff = false
        /// The section `open:<section>` scrolls to (a `StatsSectionID` raw value).
        var statsSection: String?

        /// `-MeguriStatsRollout`'s values beyond `off`.
        enum StatsRolloutOption: String, Equatable {
            case on, open
        }

        /// The sections `open:<section>` names (`stats/statistics-page` S5).
        static let statsSections: Set<String> = ["knowledge", "reviews", "cards", "comingUp", "answering", "memory", "decks"]
        var syncRender: SyncRender?

        /// `-MeguriSyncRender`: the synthetic status the shell shows, and
        /// whether the sync button shows a round that just finished.
        struct SyncRender: Equatable {
            var state: SyncCoordinator.RenderState
            var justFinished = false
        }

        static let prefix = "-Meguri"
        static let names = [
            "Fixture", "FixtureRunID", "Now", "TimeZone", "Locale", "Animations", "ReduceMotion",
            "ReduceTransparency", "WindowSize", "Appearance", "AnkiConnectPort", "MigakuPort", "SizeClass",
            "Integrations", "Route", "BackgroundLaunch", "ImportSource", "OwnedProviderFixture", "StudyTimings", "SyncRender", "StatsRollout",
            "HomeSnapshot", "HomeTrace",
            "NativeReviewRoot", "NativeReviewCase", "NativeReviewPlatform", "NativeReviewFreezeSHA256", "NativeReviewFreezeBytes",
        ]

        init() {}

        /// Parses the launch arguments, without the executable path.
        init(arguments: [String], environment: [String: String] = [:], bundleIdentifier: String = "") throws(TestHookError) {
            try self.init(arguments: arguments, environment: environment, bundleIdentifier: bundleIdentifier,
                initialOwnedProviderSource: .localReview)
        }

        /// In-process tests own this temporary source. Parsing, preparation and
        /// apply still share all production checks; CLI construction stays canonical-only.
        init(arguments: [String], preparedOwnedProviderSourceForTests source: URL) throws(TestHookError) {
            let temporary = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().standardizedFileURL
            guard source.isFileURL, source.pathComponents.starts(with: temporary.pathComponents),
                  source.pathComponents.count > temporary.pathComponents.count,
                  source.path(percentEncoded: false) == source.standardizedFileURL.path(percentEncoded: false),
                  source.resolvingSymlinksInPath().standardizedFileURL == source.standardizedFileURL else {
                throw TestHookError("Prepared owned provider test source must be temporary")
            }
            try self.init(arguments: arguments, environment: [:], bundleIdentifier: "",
                initialOwnedProviderSource: .preparedForTests(source))
        }

        private init(arguments: [String], environment: [String: String], bundleIdentifier: String,
                     initialOwnedProviderSource: OwnedProviderFixture.Source) throws(TestHookError) {
            ownedProviderSource = initialOwnedProviderSource
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

            if let value = values["OwnedProviderFixture"] {
                ownedProviderFixture = try Self.onOff(value, "OwnedProviderFixture")
            }
            if let value = values["FixtureRunID"] {
                #if DEBUG
                guard bundleIdentifier == "com.antoinefink.meguri.dev",
                      let id = UUID(uuidString: value), id.uuidString == value.uppercased(),
                      ["OwnedProviderFixture", "Route", "ImportSource", "HomeSnapshot", "SyncRender", "Integrations"]
                        .allSatisfy({ values[$0] == nil }),
                      !values.keys.contains(where: { $0.hasPrefix("NativeReview") }),
                      !arguments.contains("-ShowAnkiServerInterface") else {
                    throw TestHookError("FixtureRunID requires a canonical UUID and a normal Debug Meguri Dev fixture launch")
                }
                fixtureRunID = id
                #else
                throw TestHookError("FixtureRunID requires Debug")
                #endif
            }
            // Admit both sanctioned entries before selecting a second identity.
            // No ambient environment or caller-supplied URL grants that identity.
            #if os(macOS)
            do {
                if try OwnedProviderTransport.Request.admit(environment: environment,
                    ownedMode: ownedProviderFixture && bundleIdentifier.hasSuffix(".dev")) != nil {
                    ownedProviderSource = .ownedSyncReview
                }
            } catch { throw OwnedProviderTransport.refusal }
            #else
            guard environment["MEGURI_OWNED_PROVIDER_TRANSPORT_RUN_ID"] == nil,
                  environment["MEGURI_OWNED_PROVIDER_TRANSPORT_MANIFEST_SHA256"] == nil else {
                throw TestHookError("Owned provider transport refused")
            }
            #endif
            if ownedProviderFixture {
                let permitted = ["OwnedProviderFixture", "Fixture", "Now", "TimeZone", "Locale", "Animations",
                    "WindowSize", "Appearance", "ReduceMotion", "ReduceTransparency"]
                guard values.keys.allSatisfy({ permitted.contains($0) }),
                      !arguments.contains("-ShowAnkiServerInterface") else {
                    throw TestHookError("OwnedProviderFixture requires a normal foreground launch without routes, imports, native replay or integrations")
                }
                guard values["Fixture"] == ownedProviderSource.url.path(percentEncoded: false) else {
                    throw TestHookError("OwnedProviderFixture requires exactly its admitted checkout recipe directory")
                }
                if ownedProviderSource == .ownedSyncReview, values["Now"] != nil {
                    throw TestHookError("Owned provider transport requires the system clock")
                }
            }

            let nativeNames = ["NativeReviewRoot", "NativeReviewCase", "NativeReviewPlatform", "NativeReviewFreezeSHA256", "NativeReviewFreezeBytes"]
            if nativeNames.contains(where: { values[$0] != nil }) {
                guard nativeNames.allSatisfy({ values[$0] != nil }), let root = values["NativeReviewRoot"], root.hasPrefix("/"),
                      let caseName = values["NativeReviewCase"], !caseName.isEmpty, caseName.count <= 120,
                      caseName.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || [45, 46, 95].contains($0) }),
                      caseName.first?.isLetter == true || caseName.first?.isNumber == true,
                      let platform = values["NativeReviewPlatform"], ["mac", "iphone", "ipad"].contains(platform),
                      let sha = values["NativeReviewFreezeSHA256"], sha.utf8.count == 64,
                      sha.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
                      let size = values["NativeReviewFreezeBytes"].flatMap(UInt64.init), size > 0, size <= 128 * 1024 else {
                    throw TestHookError("NativeReview requires root, case, platform and an externally pinned freeze SHA256/byte count")
                }
                guard ["Route", "Now", "TimeZone"].allSatisfy({ values[$0] == nil }) else {
                    throw TestHookError("NativeReview owns its clock; Route, Now and TimeZone are incompatible")
                }
                if let source = values["Fixture"], source != URL(filePath: root).appending(path: "app/Fixtures/CollectionEntry/EmptyCollection").path {
                    throw TestHookError("NativeReview permits only the checkout EmptyCollection fixture")
                }
                nativeReview = .init(root: URL(filePath: root), caseName: caseName, platform: platform, freezeSHA256: sha, freezeBytes: size)
            }

            if let value = values["ImportSource"] {
                guard value.hasPrefix("/") else { throw TestHookError("-MeguriImportSource needs an absolute path") }
                importSource = URL(filePath: value)
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
            studyTimings = try values["StudyTimings"].map { value throws(TestHookError) in try Self.onOff(value, "StudyTimings") } ?? false
            homeSnapshot = try values["HomeSnapshot"].map { value throws(TestHookError) in try Self.onOff(value, "HomeSnapshot") }
            homeTrace = try values["HomeTrace"].map { value throws(TestHookError) in try Self.onOff(value, "HomeTrace") } ?? false
            if let value = values["Route"] {
                route = try Route(value)
            }
            statsRolloutOff = values["StatsRollout"] == "off"
            if let value = values["StatsRollout"], value != "off" {
                let parts = value.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
                guard let option = StatsRolloutOption(rawValue: parts[0]),
                      parts.count == 1 || (option == .open && Self.statsSections.contains(parts[1])) else {
                    throw TestHookError("-MeguriStatsRollout takes off, on, open or open:<section>, got '\(value)'")
                }
                statsRollout = option
                statsSection = parts.count == 2 ? parts[1] : nil
            }
            if let value = values["SyncRender"] {
                let parts = value.split(separator: "+", maxSplits: 1).map(String.init)
                guard let state = parts.first.flatMap(SyncCoordinator.RenderState.init(rawValue:)),
                      parts.count == 1 || parts[1] == "justFinished" else {
                    throw TestHookError("-MeguriSyncRender takes a render state name, optionally +justFinished, got '\(value)'")
                }
                guard (values["Fixture"] != nil) != (values["Route"] != nil) else {
                    throw TestHookError("-MeguriSyncRender needs -MeguriFixture (over the opened collection) or a shell -MeguriRoute, not both")
                }
                syncRender = SyncRender(state: state, justFinished: parts.count == 2)
            }
            if let value = values["BackgroundLaunch"] {
                guard value == "YES" || value == "NO" else {
                    throw TestHookError("-MeguriBackgroundLaunch takes YES or NO, got '\(value)'")
                }
                #if !os(macOS)
                throw TestHookError("-MeguriBackgroundLaunch applies to the Mac only")
                #endif
            }
        }

        func prepareOwnedProviderFixture(bundleIdentifier: String) throws(TestHookError) -> OwnedProviderFixture? {
            guard ownedProviderFixture else { return nil }
            try validateOwnedProviderFixture(bundleIdentifier: bundleIdentifier)
            return try OwnedProviderFixture(source: ownedProviderSource)
        }

        private func validateOwnedProviderFixture(bundleIdentifier: String) throws(TestHookError) {
            #if !os(macOS)
            throw TestHookError("OwnedProviderFixture applies to the Mac only")
            #else
            guard bundleIdentifier.hasSuffix(".dev") else { throw TestHookError("OwnedProviderFixture requires a .dev bundle") }
            guard nativeReview == nil, importSource == nil, route == nil, sizeClass == nil,
                  integrations == nil, ankiConnectPort == nil, migakuPort == nil else {
                throw TestHookError("OwnedProviderFixture requires normal production composition")
            }
            guard ownedProviderSource != .ownedSyncReview || now == nil else {
                throw TestHookError("Owned provider transport requires the system clock")
            }
            try OwnedProviderFixture.validateSource(fixture, source: ownedProviderSource)
            #endif
        }

        /// Installs the fixture and overrides the configuration. The route is
        /// resolved last, so its view sees every other override; a shell
        /// route's launch is returned (the app sets its navigation up).
        @discardableResult
        func apply(
            to configuration: inout LaunchConfiguration,
            installer: FixtureInstaller,
            routes: RouteRegistry,
            ownedProvider: OwnedProviderFixture? = nil
        ) throws(TestHookError) -> ShellLaunch? {
            if let fixtureRunID {
                configuration.collectionLocation = try FixtureRun.location(id: fixtureRunID, fixture: fixture,
                    bundleIdentifier: installer.bundleIdentifier)
            }
            if ownedProviderFixture {
                try validateOwnedProviderFixture(bundleIdentifier: installer.bundleIdentifier)
                guard ownedProvider != nil else { throw TestHookError("OwnedProviderFixture requires its prepared owned context") }
                guard ownedProvider?.validatedSource == fixture else {
                    throw TestHookError("OwnedProviderFixture requires the exact prepared source")
                }
            } else if ownedProvider != nil {
                throw TestHookError("OwnedProviderFixture context requires explicit opt-in")
            }
            // A hook launch starts from what it installs: no last read of an earlier run
            // shows unless it asks (N27.2).
            if self != LaunchOptions(), homeSnapshot != true { configuration.homeSnapshotFile = nil }
            if let nativeReview {
                guard installer.bundleIdentifier.hasSuffix(".dev") else { throw TestHookError("NativeReview requires a .dev bundle") }
                #if os(macOS)
                let platform = "mac"
                #else
                let platform = UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
                #endif
                guard nativeReview.platform == platform else { throw TestHookError("NativeReview platform does not match this device") }
                let input = NativeReviewInput(request: nativeReview, collection: configuration.collectionLocation)
                if let fixture {
                    do { try input.validateInitialReset(from: fixture) }
                    catch { throw TestHookError("NativeReview reset refused: \(error)") }
                }
                configuration.nativeReviewInput = input
                configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_698_000))
                configuration.timeZone = TimeZone(identifier: "Europe/Paris")
            }
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
            if let importSource {
                #if os(iOS)
                try ImportSourceInstaller.install(from: importSource, bundleIdentifier: installer.bundleIdentifier)
                #else
                throw TestHookError("-MeguriImportSource applies to iOS only")
                #endif
            }
            if route?.path == "settings/account-sync/setup-needed", fixture == nil {
                throw TestHookError("settings/account-sync/setup-needed requires -MeguriFixture with a generated collection")
            }
            if let ownedProvider {
                configuration.collectionLocation = ownedProvider.collectionLocation
                try installer.install(from: ownedProvider.validatedSource, into: configuration.collectionLocation)
            } else if let fixture {
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
                if syncRender != nil { throw TestHookError("-MeguriSyncRender with a route needs a shell route") }
                configuration.initialPresentation = presentation
                return nil
            case var .shell(launch):
                if let syncRender { SyncRoutes.addRender(syncRender, to: &launch.features, configuration: configuration) }
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
    /// Source availability only. This helper never inspects or imports a package.
    enum ImportSourceInstaller {
        static func install(from source: URL, bundleIdentifier: String, documents: URL = .documentsDirectory) throws(TestHookError) {
            guard bundleIdentifier.hasSuffix(".dev") else { throw TestHookError("ImportSourceInstaller requires a .dev bundle") }
            do {
                let values = try source.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                guard source.isFileURL, source.pathExtension.lowercased() == "colpkg",
                      values.isRegularFile == true, values.isSymbolicLink != true else {
                    throw TestHookError("-MeguriImportSource requires a regular public synthetic .colpkg fixture")
                }
                let directory = documents.appending(path: "RF2 Import Tests", directoryHint: .isDirectory)
                let files = FileManager.default
                if files.fileExists(atPath: directory.path) {
                    let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                    guard values.isDirectory == true, values.isSymbolicLink != true else {
                        throw TestHookError("Import test source directory must be a regular directory")
                    }
                } else { try files.createDirectory(at: directory, withIntermediateDirectories: true) }
                let destination = directory.appending(path: "review.colpkg")
                guard source.standardizedFileURL != destination.standardizedFileURL else {
                    throw TestHookError("Import fixture source must be outside the dedicated destination")
                }
                if files.fileExists(atPath: destination.path) { try files.removeItem(at: destination) }
                try files.copyItem(at: source, to: destination)
            } catch let error as TestHookError { throw error }
            catch { throw TestHookError("Couldn’t prepare import picker source: \(error.localizedDescription)") }
        }
    }

}
#endif
