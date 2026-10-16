import Foundation

/// The UI state a relaunch restores (R12, R13, R20): the last section, the
/// deck whose page was open, and whether the sidebar was shown. Never a
/// review: R455's resume after a termination is the review session's own
/// (U175). The Mac window's frame, full-screen state and sidebar width are
/// the `Window` scene's restoration (R12).
nonisolated public struct UIState: Codable, Equatable, Sendable {
    public var section: AppSection
    public var selectedDeck: DeckID?
    public var sidebarVisible: Bool

    public init(section: AppSection, selectedDeck: DeckID?, sidebarVisible: Bool) {
        self.section = section
        self.selectedDeck = selectedDeck
        self.sidebarVisible = sidebarVisible
    }
}

/// Keeps the `UIState` in the app's own defaults: on this device only, never
/// in the collection and never synced (KTD12, R20).
public struct UIStateStore {
    public static let key = "meguri.shell.uiState"

    private let defaults: UserDefaults?

    /// A store in `defaults`; with nil, a store that keeps nothing (test-hook
    /// launches start from a fresh state and leave none behind).
    public init(defaults: UserDefaults?) {
        self.defaults = defaults
    }

    /// The app's standard defaults.
    public static var standard: UIStateStore { UIStateStore(defaults: .standard) }

    /// The saved state, or nil when none was saved or it cannot be read
    /// (written by a later version, damaged).
    public func load() -> UIState? {
        guard let data = defaults?.data(forKey: Self.key) else { return nil }
        return try? JSONDecoder().decode(UIState.self, from: data)
    }

    public func save(_ state: UIState) {
        guard let defaults, let data = try? JSONEncoder().encode(state) else { return }
        defaults.set(data, forKey: Self.key)
    }
}
