import Foundation

/// What Browse keeps on this device across launches (`browser/config-keys`
/// K11; never in the collection, never synced): the query's run text for a
/// relaunch (`search/filter-tokens` T14), the sidebar's folded sections and
/// the Tags section's Show All choice (`browser/workspace` V6).
nonisolated public struct BrowseDeviceState: Codable, Equatable, Sendable {
    /// T4's run text of the query Browse had when the app last saw it; nil
    /// before Browse first opens on this device (or this collection).
    public var runText: String?
    /// The mode Browse last showed on this device (T14); nil before.
    public var notesMode: Bool?
    public var folded: Set<BrowseSidebarSection>
    public var showsAllTags: Bool

    public init(runText: String? = nil, notesMode: Bool? = nil, folded: Set<BrowseSidebarSection> = BrowseSidebarSection.foldedByDefault,
                showsAllTags: Bool = false) {
        self.runText = runText
        self.notesMode = notesMode
        self.folded = folded
        self.showsAllTags = showsAllTags
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

    public func save(_ state: BrowseDeviceState) {
        guard let defaults, let data = try? JSONEncoder().encode(state) else { return }
        defaults.set(data, forKey: Self.key)
    }
}
