import Foundation

extension SettingsPage {
    /// Anki server sync is hidden for now: iCloud is the only sync in the
    /// interface (Antoine, 2026-11-01). The code stays; UI suites that cover
    /// it launch with `-ShowAnkiServerInterface YES`.
    public static var showsAnkiServer: Bool { UserDefaults.standard.bool(forKey: "ShowAnkiServerInterface") }
    public static var visible: [SettingsPage] {
        let pages = showsAnkiServer ? allCases : allCases.filter { $0 != .ankiServer }
        #if os(macOS)
        return pages
        #else
        return pages.filter { $0 != .integrations }
        #endif
    }
}

extension SettingsPage {
    /// The section's row in a Settings list. N15.1 names the section Sync; the
    /// case keeps its stored name, which the last selected section is saved under.
    public var rowIdentifier: String { "settings.root.\(self == .accountSync ? "sync" : rawValue)" }
}
