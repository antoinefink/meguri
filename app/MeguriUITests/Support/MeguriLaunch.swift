import Foundation

/// Launch arguments for the app's test hooks (U7, R1963). Each field maps to
/// one `-Meguri…` option; see `TestHooks.LaunchOptions` in the app.
struct MeguriLaunch {
    var fixture: URL?
    var ownedProviderFixture: Bool?
    var importSource: URL?
    var now: String?
    var timeZone: String?
    var locale: String?
    var animations: Bool?
    var reduceMotion: Bool?
    var reduceTransparency: Bool?
    var windowSize: String?
    /// Mac only: "light" or "dark".
    var appearance: String?
    var ankiConnectPort: Int?
    var migakuPort: Int?
    var route: String?
    /// Extra raw arguments, appended last.
    var extra: [String] = []

    var arguments: [String] {
        // Windows and sheets are not restored from a previous run. The Mac
        // window frame still is (SwiftUI keeps it in the `NSWindow Frame main`
        // default); the hooks reset it on every launch with a hook option.
        var arguments = ["-ApplePersistenceIgnoreState", "YES"]
        func add(_ name: String, _ value: String?) {
            if let value { arguments += ["-Meguri\(name)", value] }
        }
        func onOff(_ value: Bool?) -> String? { value.map { $0 ? "on" : "off" } }
        add("Fixture", fixture?.path(percentEncoded: false))
        add("OwnedProviderFixture", onOff(ownedProviderFixture))
        add("ImportSource", importSource?.path(percentEncoded: false))
        add("Now", now)
        add("TimeZone", timeZone)
        if let locale {
            add("Locale", locale)
            // System strings and Locale.current follow the same locale.
            let language = String(locale.prefix { $0 != "_" })
            arguments += ["-AppleLanguages", "(\(language))", "-AppleLocale", locale]
        }
        add("Animations", onOff(animations))
        add("ReduceMotion", onOff(reduceMotion))
        add("ReduceTransparency", onOff(reduceTransparency))
        add("WindowSize", windowSize)
        add("Appearance", appearance)
        add("AnkiConnectPort", ankiConnectPort.map(String.init))
        add("MigakuPort", migakuPort.map(String.init))
        add("Route", route)
        return arguments + extra
    }
}
