import CoreBridge
import Foundation

/// The core reads Browse's shell uses (U270). Production is
/// `CoreBrowseBackend`; tests and the test hooks' routes pass their own.
/// Every call reads and writes nothing, except `setTagsCollapsed`, a
/// person's own toggle of a tag's disclosure, the saved searches' Save
/// Search…, Rename… and Delete, and the column set and mode writes.
@MainActor public protocol BrowseBackend: AnyObject, Sendable {
    /// T14: the default search and the mode a first open reads.
    func opening() async throws -> BrowseOpening
    /// T7: the query Anki search text reads as.
    func read(_ text: String) async throws -> FilterQuery
    /// T4: the run text the device keeps for a relaunch.
    func runText(_ query: FilterQuery) async throws -> String
    /// T8's commit: syntax in the text becomes tokens after the query's own.
    func commit(_ query: FilterQuery) async throws -> FilterQuery
    /// T8: the text is incomplete at the UTF-16 offset `caret` (D5).
    func isIncomplete(_ text: String, caret: Int) async throws -> Bool
    /// T12: the suggestions for `query` with the caret at `caret` of its
    /// text, or `scope`'s value list; `recent` holds accepted rows' keys.
    func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection?, recent: [String]) async throws
        -> [FilterSuggestion]
    /// T13: Save Search… with the query's run text; one undoable step.
    func saveSearch(name: String, text: String) async throws
    /// T13: Rename… of a user saved search; one undoable step (K7.2).
    func renameSavedSearch(_ old: String, to name: String) async throws
    /// T13: Delete of a user saved search; one undoable step (K7.2).
    func deleteSavedSearch(_ name: String) async throws
    /// The rows of one kind as the core offers them (T12.1's value list):
    /// the saved searches, every deck's value, the note types' values.
    func values(_ section: FilterSection) async throws -> [FilterSuggestionRow]
    /// `notetypes/editor-actions` A2's order.
    func notetypes() async throws -> [NotetypeName]
    func tagTree() async throws -> [TagNode]
    /// The seven flags' shown names (`tags/flags-and-marked` F4).
    func flagNames() async throws -> [String]
    /// T12.4: each item's rows in `mode` (nil when it cannot be counted).
    /// A newer call cancels this one; cancelling the task cancels the job.
    func counts(_ mode: BrowseMode, _ items: [FilterCountItem]) async throws -> [UInt64?]
    func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws

    // The results table (U270a).

    /// H2: a search session for the results table.
    func openSearchSession() async throws -> any BrowseSearching
    /// V2.2: the column set the query's note type (or none) uses.
    func columnSet(notetype: Int64?) async throws -> ColumnSetShown
    /// V2.3, V4.3: a person's change to a set; no undo step (V2.4).
    func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws
    /// V2.2: Use Standard Columns.
    func useStandardColumns(notetype: Int64) async throws
    /// V2.1, K6.1: the Cards / Notes control's write.
    func setNotesMode(_ notes: Bool) async throws
}

/// The shell's reads over CoreBridge.
@MainActor public final class CoreBrowseBackend: BrowseBackend {
    private let tags = CoreTagService()
    private let notetypeReads = CoreNotetypeService()
    /// T15's labels in the app's language, which fixed values match by (T12.2).
    private let labels: FilterLabels
    public init(labels: FilterLabels = .english) { self.labels = labels }

    public func opening() async throws -> BrowseOpening { try await CoreBrowseQuery.opening() }
    public func read(_ text: String) async throws -> FilterQuery { try await CoreBrowseQuery.read(text) }
    public func runText(_ query: FilterQuery) async throws -> String { try await CoreBrowseQuery.runText(query) }
    public func commit(_ query: FilterQuery) async throws -> FilterQuery { try await CoreBrowseQuery.commit(query) }
    public func isIncomplete(_ text: String, caret: Int) async throws -> Bool {
        try await CoreBrowseQuery.isIncomplete(text, caret: caret)
    }
    public func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection?, recent: [String]) async throws
        -> [FilterSuggestion] {
        try await CoreBrowseQuery.suggestions(for: query, caret: caret, scope: scope, recent: recent, labels: labels)
    }
    public func saveSearch(name: String, text: String) async throws {
        _ = try await CoreBrowseQuery.saveSearch(name: name, search: text)
    }
    public func renameSavedSearch(_ old: String, to name: String) async throws {
        _ = try await CoreBrowseQuery.renameSavedSearch(old, to: name)
    }
    public func deleteSavedSearch(_ name: String) async throws {
        _ = try await CoreBrowseQuery.deleteSavedSearches([name])
    }
    public func values(_ section: FilterSection) async throws -> [FilterSuggestionRow] {
        try await CoreBrowseQuery.suggestions(for: FilterQuery(), caret: 0, scope: section, labels: labels).map(\.row)
    }
    public func notetypes() async throws -> [NotetypeName] { try await notetypeReads.notetypeList() }
    public func tagTree() async throws -> [TagNode] { try await tags.tagTree() }
    public func flagNames() async throws -> [String] { try await tags.flagNames() }
    public func counts(_ mode: BrowseMode, _ items: [FilterCountItem]) async throws -> [UInt64?] {
        let job = try await CoreBrowseQuery.startFilterCounts(mode, items: items)
        return try await withTaskCancellationHandler {
            try await job.value()
        } onCancel: {
            Task { try? await job.job.cancel() }
        }
    }
    public func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws {
        _ = try await tags.setTagsCollapsed(names: names, collapsed: collapsed)
    }
    public func openSearchSession() async throws -> any BrowseSearching { try await CoreBrowseSearch.open() }
    public func columnSet(notetype: Int64?) async throws -> ColumnSetShown { try await CoreBrowseQuery.columnSet(notetype: notetype) }
    public func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws {
        _ = try await CoreBrowseQuery.changeColumnSet(set, mode: mode, change: change)
    }
    public func useStandardColumns(notetype: Int64) async throws {
        _ = try await CoreBrowseQuery.useStandardColumns(notetype: notetype)
    }
    public func setNotesMode(_ notes: Bool) async throws { _ = try await CoreBrowseQuery.setNotesMode(notes) }
}
