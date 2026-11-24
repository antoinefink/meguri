import Foundation

/// What Browse keeps on this device across launches (`browser/config-keys`
/// K11; never in the collection, never synced): the query's run text for a
/// relaunch (`search/filter-tokens` T14), the sidebar's folded sections,
/// the Tags section's Show All choice (`browser/workspace` V6) and the
/// suggestions accepted (T12.3).
nonisolated public struct BrowseDeviceState: Codable, Equatable, Sendable {
    /// T4's run text of the query Browse had when the app last saw it; nil
    /// before Browse first opens on this device (or this collection).
    public var runText: String?
    /// The mode Browse last showed on this device (T14); nil before.
    public var notesMode: Bool?
    public var folded: Set<BrowseSidebarSection>
    public var showsAllTags: Bool
    /// The suggestions accepted, newest first (`search/editing-services`
    /// D6.5 step 4); nil before the first.
    public var recentSuggestions: [String]?
    /// The inspector's own (U273, `browser/inspector` I8, V12); nil before.
    public var inspector: BrowseInspectorDevice?
    /// The columns' widths the person set (U272, `browser/workspace` V2.3):
    /// by set and mode ("std/cards", "<note type id>/notes"), then column
    /// key, in points; nil before the first.
    public var columnWidths: [String: [String: Double]]?
    /// Find and Replace's remembered inputs (U277, `browser/find-replace`
    /// F2, `browser/workspace` V9); nil before the first Replace.
    public var findReplace: BrowseFindReplaceDevice?

    public init(runText: String? = nil, notesMode: Bool? = nil, folded: Set<BrowseSidebarSection> = BrowseSidebarSection.foldedByDefault,
                showsAllTags: Bool = false) {
        self.runText = runText
        self.notesMode = notesMode
        self.folded = folded
        self.showsAllTags = showsAllTags
    }
}

/// What Find and Replace remembers on this device (F2, V9): the three
/// options, the In choice as its entry's text and position, and the Find
/// and Replace With histories, newest first, at most 30 each.
nonisolated public struct BrowseFindReplaceDevice: Codable, Equatable, Sendable {
    public var matchCase = false
    public var wholeWords = false
    public var regex = false
    /// The In entry chosen last, as `BrowseFieldEntry.Kind.memory` writes
    /// it, and its position in the menu.
    public var inEntry: String?
    public var inPosition: Int?
    public var finds: [String] = []
    public var replaces: [String] = []

    public init() {}

    /// F2: the histories' length.
    public static let historyLimit = 30

    /// F2: `text` enters at the front; one already there moves to the front
    /// (compared code point by code point); an empty text never enters.
    public static func entering(_ text: String, into history: [String]) -> [String] {
        guard !text.isEmpty else { return history }
        let scalars = Array(text.unicodeScalars)
        return Array(([text] + history.filter { Array($0.unicodeScalars) != scalars }).prefix(historyLimit))
    }
}

/// Keeps `BrowseDeviceState` in the app's own defaults. With nil defaults
/// it keeps nothing (test-hook launches start fresh and leave nothing).
public struct BrowseDeviceStore {
    public static let key = "meguri.browse.deviceState"
    private let defaults: UserDefaults?

    public init(defaults: UserDefaults?) {
        self.defaults = defaults
    }

    /// The saved state, or the defaults when none was saved or it cannot be
    /// read (written by a later version, damaged).
    public func load() -> BrowseDeviceState {
        guard let data = defaults?.data(forKey: Self.key),
              let state = try? JSONDecoder().decode(BrowseDeviceState.self, from: data) else { return BrowseDeviceState() }
        return state
    }

    /// The collection was replaced: what this device kept of the old one's
    /// query and mode is dropped, so the next open is a first open (T14).
    public func forgetCollection() {
        guard let defaults, defaults.data(forKey: Self.key) != nil else { return }
        var state = load()
        state.runText = nil
        state.notesMode = nil
        save(state)
    }

    public func save(_ state: BrowseDeviceState) {
        guard let defaults, let data = try? JSONEncoder().encode(state) else { return }
        defaults.set(data, forKey: Self.key)
    }
}
