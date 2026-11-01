import Foundation

extension SettingsPage {
    /// Anki server sync is hidden for now: iCloud is the only sync in the
    /// interface (Antoine, 2026-11-01). The code stays; UI suites that cover
    /// it launch with `-ShowAnkiServerInterface YES`.
    public static var showsAnkiServer: Bool { UserDefaults.standard.bool(forKey: "ShowAnkiServerInterface") }
    public static var visible: [SettingsPage] {
        showsAnkiServer ? allCases : allCases.filter { $0 != .ankiServer }
    }
}
