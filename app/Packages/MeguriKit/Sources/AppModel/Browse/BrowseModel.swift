import CoreBridge
import Foundation
import Observation

/// Browse's state (U270): the query, which the sidebar's filter rows show
/// (`search/filter-tokens` T1, T9), the mode, and the sidebar's content
/// and counts (`browser/workspace` V6, T12.4). Held by `ReviewCollection`
/// above the layout switch, so leaving Browse for Today, a deck or review
/// and coming back finds it as it was (V1); a relaunch starts as T14 says.
/// Reads are pure; explicit tag, saved-search and row actions write through
/// the core's mutation services.
@Observable @MainActor
public final class BrowseModel {
    /// A user saved search (`browser/config-keys` K7.1), in the sidebar's
    /// order (`browser/sidebar` S3.1).
    public nonisolated struct SavedSearch: Hashable, Sendable {
        public let name: String
        public let text: String
        public init(name: String, text: String) {
            self.name = name
            self.text = text
        }
    }

    /// The modifier keys of a click on a filter row (T9).
    public nonisolated enum Click: Sendable, Hashable {
        case plain, command, option
    }

    /// W12 from the note type row menu; the collection owns the question.
    @ObservationIgnored public var deleteNoteType: (@MainActor (Int64) -> Void)?
    @ObservationIgnored public var canDeleteNoteType: @MainActor () -> Bool = { false }

    // MARK: State

    /// What the search field holds (T1); the sidebar is a view of it.
    public internal(set) var query = FilterQuery()
    /// Cards or Notes (V2.1): rows and counts are of this mode (T11).
    public private(set) var mode: BrowseMode = .cards
    /// Browse has opened in this launch (T14's first open has run).
    public private(set) var opened = false
    public private(set) var savedSearches: [SavedSearch] = []
    /// The query each user saved search set when last clicked (T7's reading
    /// of its text): its row shows selected while the query equals it.
    public private(set) var savedReadings: [SavedSearch: FilterQuery] = [:]
    /// Every deck's value as the core makes it (T3), by deck id.
    public private(set) var deckValues: [Int64: FilterValue] = [:]
    public private(set) var notetypes: [NotetypeName] = [] {
        didSet { updateShownNotetypes() }
    }
    /// V2.3's Fields submenu: each note type's field names in field order,
    /// read after the note types (pure reads); read again when note types change.
    public private(set) var notetypeFields: [Int64: [String]] = [:]
    @ObservationIgnored private var fieldsStale = true
    @ObservationIgnored private var fieldsTask: Task<Void, Never>?
    public private(set) var tags: [TagNode] = [] {
        didSet {
            tagIdentities = Set(Self.tagNames(tags))
            tagsRevision &+= 1
            updateShownTags()
        }
    }
    private(set) var tagIdentities: Set<String> = []
    public private(set) var tagsReading = false
    public private(set) var tagsFailed = false
    /// Shared by Browse and Settings; loading tags never opens Browse or runs a search.
    public var tagsLoaded: Bool { tagsEpoch == epoch && tagsGeneration == generation() }
    private var tagsEpoch: UInt64?
    private var tagsGeneration: UInt64?
    private var tagsRequested = false
    private var tagsToken: UInt64 = 0
    @ObservationIgnored private var tagsTask: Task<Void, Never>?
    @ObservationIgnored public var generation: @MainActor () -> UInt64 = { 0 }
    /// Flags 1 to 7's shown names (F4).
    public private(set) var flagNames: [String] = []
    /// Each row's count in the current mode (T12.3 step 4); a row without
    /// one shows none until it is known.
    public private(set) var counts: [BrowseSidebarTag: UInt64] = [:] {
        didSet { updateShownNotetypes() }
    }
    /// The sidebar's filter text (`browser/sidebar` S6).
    public var filter = ""
    /// V6: the folded sections, kept on the device.
    public var folded: Set<BrowseSidebarSection> {
        didSet { if folded != oldValue { saveDevice() } }
    }
    /// V6: the Tags section shows the whole tree (kept on the device).
    public var showsAllTags: Bool {
        didSet { if showsAllTags != oldValue { saveDevice(); updateShownTags() } }
    }
    /// V6: the Note Types section shows every note type (kept on the device).
    public var showsAllNotetypes: Bool {
        didSet { if showsAllNotetypes != oldValue { saveDevice(); updateShownNotetypes() } }
    }
    /// Raised when a selection the list made is not the query's, so the
    /// list draws the query's selection again.
    public private(set) var selectionEcho: UInt64 = 0
    /// The filter row the arrow keys moved to: Return and Space are a click on it.
    @ObservationIgnored public private(set) var keyRow: BrowseSidebarTag?
    /// The filter row clicked last: Return and Space are a second click on it (T9).
    @ObservationIgnored public private(set) var clickedRow: BrowseSidebarTag?

    /// Tag disclosures toggled in this launch, before the tree is read again.
    private var tagExpansion: [String: Bool] = [:]
    /// Each tag's latest disclosure write: a failed write undoes only its own toggle.
    @ObservationIgnored private var tagWrites: [String: UInt64] = [:]
    @ObservationIgnored private var tagWriteToken: UInt64 = 0
    /// The top-level tags' note counts, which pick the eight Tags shows (V6).
    private var tagNotes: [String: UInt64] = [:] {
        didSet { if tagNotes != oldValue { updateShownTags() } }
    }
    @ObservationIgnored private var tagsRevision: UInt64 = 0
    @ObservationIgnored private var filteredTagsCache: (text: String, revision: UInt64, nodes: [BrowseSidebarFilter.Node<TagNode>])?
    var visible = false
    private var stale = true
    /// The names or counts of the latest reload have not all arrived.
    private var readsPending = false
    /// T14's first open has started and not ended: a collection that turns
    /// ready again meanwhile makes it start over.
    private var openInFlight = false
    /// Raised by every change of the query: a reading started before it
    /// (a saved search, the first open) gives way to the newer query.
    var queryRevision: UInt64 = 0
    /// Raised when the collection goes: work started before must not land.
    var epoch: UInt64 = 0
    private var namesToken: UInt64 = 0
    private var countsToken: UInt64 = 0
    @ObservationIgnored private var namesTask: Task<Void, Never>?
    @ObservationIgnored private var countsTask: Task<Void, Never>?
    @ObservationIgnored var device: BrowseDeviceState

    /// The results table (U270a): the query's rows in the mode.
    public let results: BrowseResults
    /// The commands on its rows (U275).
    public let tagDeletion: BrowseTagDeletion
    public let commands: BrowseCommands
    /// The pane beside the table (U273).
    public let inspector: BrowseInspector

    // MARK: The search field (U271)

    /// The suggestions under the field (T12), in the core's order, while
    /// the field has the focus.
    public internal(set) var suggestions: [FilterSuggestion] = []
    /// The kind whose value list the suggestions show (T12.1: Filter By,
    /// Add Another…, Show All), or nil.
    public internal(set) var suggestionScope: FilterSection?
    /// A Field suggestion accepted: the suggestions show its three tests (T12.1).
    public internal(set) var suggestedField: SuggestedField?
    /// The field has the focus, so the suggestions are read.
    public internal(set) var fieldFocused = false
    /// Raised by Find… (⌘F), Add Another… and Edit: the field takes the focus (T10).
    public internal(set) var focusRequests: UInt64 = 0
    /// The values each value token's menu offers after its own (T10), by kind.
    public internal(set) var tokenOffers: [FilterKind: [FilterValue]] = [:]
    /// Counts of values the sidebar has no row for (the day values, Any
    /// Flag, Untagged, Current Deck), for the suggestions (T12.3 step 4).
    public internal(set) var valueCounts: [FilterValue: UInt64] = [:]
    /// Save Search… is asking for a name (T13).
    public var savingSearch: SavingSearch?
    /// Rename… is asking for a saved search's new name (T13).
    public var renamingSearch: RenamingSearch?
    /// A Field token's (or Field suggestion's) Contains… is asking for its text (T10).
    public var editingContains: EditingContains?
    /// Saved searches' suggestions read again after a Save Search.
    @ObservationIgnored var suggestionsToken: UInt64 = 0
    /// The text and the scope the suggestions shown were read for.
    @ObservationIgnored var suggestionsText: String?
    @ObservationIgnored var suggestionsReadScope: FilterSection?
    @ObservationIgnored var offersToken: UInt64 = 0
    @ObservationIgnored var liveRun: Task<Void, Never>?
    /// The query the results last ran.
    @ObservationIgnored var lastRun: FilterQuery?
    /// The system's field empties its text when it takes a suggestion as a
    /// token: the text an emptying edit replaced in this turn, and whether a
    /// suggestion arrived in it, so the typed text survives either order.
    @ObservationIgnored var clearedThisTurn: String?
    /// The value list the suggestions were scoped to when the field emptied
    /// its text this turn: it comes back with the text.
    @ObservationIgnored var clearedScopeThisTurn: FilterSection?
    @ObservationIgnored var acceptedThisTurn = false
    @ObservationIgnored var savingPresentations: UInt64 = 0
    /// The text a commit left, which D5's incomplete rule no longer holds (T8).
    @ObservationIgnored var committedText: String?

    // MARK: Seams

    @ObservationIgnored let backend: any BrowseBackend
    @ObservationIgnored private let store: BrowseDeviceStore
    /// Whether the collection is open and ready for reads.
    @ObservationIgnored public var ready: @MainActor () -> Bool = { true } {
        didSet { let ready = ready; results.ready = { ready() }; commands.ready = { ready() } }
    }
    /// A handed search whose reading failed, tried again on the next appear.
    var pendingSearch: String?
    /// A search handed to Browse (a deep link, R319), taken once: T14's
    /// "with a search" replaces the query by its reading.
    @ObservationIgnored public var takeEntrySearch: @MainActor () -> String? = { nil }

    /// `sheetReads` defaults to the backend's when it gives them, else the
    /// core's (U276).
    public init(backend: any BrowseBackend, store: BrowseDeviceStore = BrowseDeviceStore(defaults: nil),
                inspectorBackend: any BrowseInspectorBackend = CoreBrowseInspectorBackend(),
                sheetReads: (any BrowseSheetReads)? = nil, cardInfo: (any CardInfoReading)? = nil) {
        self.backend = backend
        self.store = store
        let results = BrowseResults(backend: backend)
        self.results = results
        let commands = BrowseCommands(results: results,
                                      sheetReads: sheetReads ?? (backend as? any BrowseSheetReads) ?? CoreBrowseSheetReads())
        self.commands = commands
        let tagDeletion = BrowseTagDeletion(backend: backend)
        self.tagDeletion = tagDeletion
        let device = store.load()
        let inspector = BrowseInspector(backend: inspectorBackend, results: results, commands: commands,
                                        device: device.inspector ?? BrowseInspectorDevice(),
                                        cardInfo: cardInfo ?? CoreCardInfoReader())
        self.inspector = inspector
        results.selectionObserver = { [weak commands, weak inspector] in
            commands?.selectionChanged()
            inspector?.selectionChanged()
        }
        commands.editNote = { [weak inspector] in inspector?.editNote() }
        commands.saveFirst = { [weak inspector] in await inspector?.flushForBackground() ?? true }
        commands.unsavedEdit = { [weak inspector] in inspector?.unsavedEdit ?? false }
        self.device = device
        folded = device.folded
        showsAllTags = device.showsAllTags
        showsAllNotetypes = device.showsAllNotetypes ?? false
        results.restoreWidths(device.columnWidths ?? [:])
        results.storeWidths = { [weak self] widths in
            guard let self else { return }
            self.device.columnWidths = widths
            self.saveDevice()
        }
        inspector.store = { [weak self] state in
            guard let self else { return }
            self.device.inspector = state
            self.saveDevice()
        }
        // U277: Find and Replace's memory (F2), the field menus' note types, Find Duplicates' token.
        commands.findReplaceMemory = { [weak self] in self?.device.findReplace }
        commands.keepFindReplaceMemory = { [weak self] memory in
            guard let self else { return }
            self.device.findReplace = memory
            self.saveDevice()
        }
        commands.notetypes = { [weak self] in self?.notetypes ?? [] }
        commands.duplicatesFound = { [weak self] found, field in self?.showDuplicates(found, field: field) }
        tagDeletion.available = { [weak commands] in
            guard let commands else { return false }
            return commands.ready() && !commands.busy && commands.deleteQuestion == nil && commands.sheet == nil
                && commands.findReplaceSheet == nil && commands.findDuplicatesSheet == nil
        }
        tagDeletion.saveFirst = { [weak inspector] in await inspector?.flushForBackground() ?? true }
        tagDeletion.generation = { [weak self] in self?.generation() ?? 0 }
        tagDeletion.contains = { [weak self] name in
            guard let self else { return false }
            return !self.tagsReading && !self.tagsFailed && self.hasTag(name)
        }
        tagDeletion.finished = { [weak self] _, _ in
            guard let self else { return }
            // Refresh even when the only tag surface is Settings.
            if self.visible { self.reload() }
            else { await self.refreshTags() }
        }
        commands.otherWorkBusy = { [weak tagDeletion] in tagDeletion?.busy ?? false }
    }

    // MARK: Find Duplicates' token (V10)

    /// The Duplicates token Find Duplicates made, its label's field and
    /// group count, and the query it replaced.
    public nonisolated struct Duplicates: Equatable, Sendable {
        public let noteIDs: [Int64]
        public let field: String
        public let groups: Int
        /// V10's previous query: removing the token restores it; any other
        /// change of the query drops it.
        public internal(set) var previous: FilterQuery?

        var token: FilterToken { .value([.duplicates(noteIDs: noteIDs)], negated: false) }
    }

    /// The Duplicates token's facts while it is in the query.
    public internal(set) var duplicates: Duplicates?

    /// V10: the groups replace the query with one Duplicates token, in Notes
    /// mode, sorted by the field with its column shown while the token is
    /// there (display only).
    func showDuplicates(_ found: FindDuplicatesResult, field: String) {
        // Found again from its own token: the query before the first Find stays the one to restore.
        // A Duplicates query whose previous one was dropped restores nothing (its label facts would be gone).
        let previous: FilterQuery? = if let shown = duplicates, query.tokens.contains(shown.token) { shown.previous } else { query }
        let info = Duplicates(noteIDs: found.noteIds, field: field, groups: found.groups.count, previous: previous)
        if mode != .notes { setMode(.notes, run: false) }
        setQuery(FilterQuery(tokens: [info.token]), duplicates: info)
    }

    /// The Duplicates token's facts as the query changes: removing the
    /// token alone restores the previous query.
    private func duplicatesRule(_ next: FilterQuery) -> FilterQuery {
        guard let info = duplicates else { return next }
        let had = query.tokens.contains(info.token), has = next.tokens.contains(info.token)
        if had, !has {
            duplicates = nil
            let rest = FilterQuery(tokens: query.tokens.filter { $0 != info.token }, text: query.text)
            if let previous = info.previous, next == rest { return previous }
            return next
        }
        if !has {
            duplicates = nil
        } else if next != query {
            duplicates?.previous = nil
        }
        return next
    }

    // MARK: Showing

    /// Browse's sidebar appeared: the first open of the launch runs T14,
    /// then the rows and counts are read, at once and without waiting for
    /// each other.
    public func appear() {
        visible = true
        guard ready() else { return }
        if !opened { open() }
        takeHandedSearch()
        if stale {
            reload()
            // U276: Set Due Date's and Forget's remembered inputs, so their sheets open filled.
            commands.preloadSheetDefaults()
        }
        // The rows of the query, once the first open has read it (T14).
        if !openInFlight, !results.hasRun { showResults() }
    }

    public func disappear() {
        visible = false
        // V12: leaving Browse saves the note the inspector shows.
        inspector.browseLeft()
        // Counts or names still on their way are read again on the next appear.
        if readsPending { stale = true }
        countsTask?.cancel()
    }

    /// T14: the first open on this device reads `defaultSearchText` (the
    /// whole collection when it is not set); later launches read the run
    /// text kept on the device. The mode is the collection's (K6.1).
    private func open() {
        opened = true
        openInFlight = true
        let revision = queryRevision, epoch = epoch, kept = device.runText
        Task {
            let opening = try? await backend.opening()
            guard self.epoch == epoch else { return }
            defer { if self.epoch == epoch { openInFlight = false } }
            // A failed read keeps nothing, so the next appear opens again (T14, K6.4).
            guard let opening else {
                opened = false
                showResults()
                return
            }
            // T14: after a relaunch the mode kept on this device; on a first open, the collection's.
            let notes = device.notesMode ?? opening.notesMode
            if notes != (mode == .notes) {
                mode = notes ? .notes : .cards
                counts = [:]
                valueCounts = [:]
                refreshCounts()
                // A query chosen meanwhile is shown in the restored mode.
                if revision != queryRevision { showResults() }
            }
            device.notesMode = notes
            saveDevice()
            let text = kept ?? opening.defaultSearch
            guard let text, revision == queryRevision else {
                if kept == nil { keepRunText() }
                if revision == queryRevision { showResults() }
                return
            }
            let read: FilterQuery
            do { read = try await backend.read(text) } catch {
                // Not read: the next appear opens again, unless the query changed meanwhile.
                if self.epoch == epoch, revision == queryRevision { opened = false; showResults() }
                return
            }
            guard self.epoch == epoch, revision == queryRevision else { return }
            setQuery(read)
        }
    }

    // MARK: The query

    func setQuery(_ nextQuery: FilterQuery, duplicates arriving: Duplicates? = nil) {
        let next: FilterQuery
        if let arriving {
            duplicates = arriving
            next = FilterQuery(tokens: [arriving.token])
        } else {
            next = duplicatesRule(nextQuery)
        }
        results.displayField = duplicates.flatMap { next.tokens.contains($0.token) ? $0.field : nil }
        queryRevision &+= 1
        liveRun?.cancel()
        let tokensChanged = next.tokens != query.tokens
        if next != query { query = next }
        keepRunText()
        showResults()
        if tokensChanged { refreshTokenOffers(); updateShownNotetypes() }
        if fieldFocused { refreshSuggestions() }
    }

    /// The results show the query in the mode.
    func showResults() {
        lastRun = query
        results.run(query, mode: mode)
    }

    /// The no-results state's Clear Search: the empty query.
    public func clearQuery() {
        pendingSearch = nil
        // Clear Search empties the query; it does not restore the one before Find Duplicates.
        duplicates?.previous = nil
        setQuery(FilterQuery())
    }

    /// V2.1: the Cards / Notes control. The query stays (T11); the rows
    /// toggle (H11), the counts are of the new mode, and the mode is written
    /// to the collection (K6.1) and kept on the device (T14).
    public func setMode(_ next: BrowseMode) { setMode(next, run: true) }

    /// `run` false: the caller sets a new query, which runs in the mode.
    private func setMode(_ next: BrowseMode, run: Bool) {
        guard next != mode else { return }
        mode = next
        device.notesMode = next == .notes
        saveDevice()
        counts = [:]
        valueCounts = [:]
        refreshCounts()
        if run { results.setMode(next, query: query) }
        writeMode(next)
    }

    /// The mode a refused write left unwritten, written again when the
    /// collection is ready again.
    private var unwrittenMode: BrowseMode?
    /// Raised only when the collection is replaced: a mode write for the
    /// same collection survives it becoming ready again.
    private var replacements: UInt64 = 0
    /// Raised each time the collection becomes ready again.
    private var readies: UInt64 = 0

    private func writeMode(_ next: BrowseMode) {
        let backend = backend, replacements = replacements, readies = readies
        unwrittenMode = nil
        Task {
            do { try await backend.setNotesMode(next == .notes) } catch {
                guard self.replacements == replacements, mode == next else { return }
                unwrittenMode = next
                // Refused across the collection becoming ready again: written once more now.
                if self.readies != readies, ready() { writeMode(next) }
            }
        }
    }

    /// The device keeps the query's run text for a relaunch (T14).
    func keepRunText() {
        let revision = queryRevision, epoch = epoch, query = query
        Task {
            guard let text = try? await backend.runText(query), self.epoch == epoch, revision == queryRevision else { return }
            device.runText = text
            saveDevice()
        }
    }

    /// T14 "with a search", and T9's saved searches: the whole query
    /// becomes T7's reading of `text`, unless the query changed meanwhile.
    func replace(byReading text: String, handed: Bool = false, savedAs name: String? = nil) {
        queryRevision &+= 1
        let revision = queryRevision, epoch = epoch
        // A handed search is kept until it is applied: a failed read, or one the
        // collection turning ready again discards, is tried again.
        if handed { pendingSearch = text }
        Task {
            guard let read = try? await backend.read(text), self.epoch == epoch, revision == queryRevision else { return }
            if handed, pendingSearch == text { pendingSearch = nil }
            if let name, savedSearches.contains(SavedSearch(name: name, text: text)) {
                savedReadings[SavedSearch(name: name, text: text)] = read
            }
            setQuery(read)
        }
    }

    /// The search handed to Browse, or one whose reading failed before.
    private func takeHandedSearch() {
        if let search = takeEntrySearch() ?? pendingSearch, !search.isEmpty { replace(byReading: search, handed: true) }
    }

    /// T9: a click on a filter row.
    public func activate(_ row: BrowseSidebarTag, click: Click = .plain) {
        pendingSearch = nil
        switch row {
        case .builtIn(let search):
            // A second click on the saved search the query holds clears it (T9).
            setQuery(query == search.query ? FilterQuery() : search.query)
        case .saved(let name):
            guard let saved = savedSearches.first(where: { $0.name == name }) else { return }
            if savedReadings[saved] == query { return setQuery(FilterQuery()) }
            replace(byReading: saved.text, savedAs: name)
        case .showAllTags:
            showsAllTags = true
        case .showAllNotetypes:
            showsAllNotetypes = true
        case .section:
            return
        default:
            guard let value = value(of: row) else { return }
            switch click {
            case .plain: setQuery(query.clicking(value))
            case .command: setQuery(query.commandClicking(value))
            case .option: setQuery(query.optionClicking(value))
            }
        }
    }

    /// The value a filter row stands for (T3); nil for a row that is not a
    /// value, or a deck the core does not offer.
    public func value(of row: BrowseSidebarTag) -> FilterValue? {
        switch row {
        case .state(let state): .state(state)
        case .flag(0): .noFlag
        case .flag(let flag): .flag(flag)
        case .deck(let id): deckValues[id]
        case .notetype(let id): notetypes.first { $0.id == id }.map { .notetype(id: $0.id, name: $0.name) }
        case .tag(let name): .tag(name)
        case .builtIn(let search): search.value
        case .section, .saved, .showAllTags, .showAllNotetypes: nil
        }
    }

    /// T9: the filter rows that show selected, those whose value is in its
    /// kind's **is** token.
    public var selectedRows: Set<BrowseSidebarTag> {
        var rows = Set<BrowseSidebarTag>()
        // A saved search shows selected while the query is the one it sets (T9).
        for search in FilterBuiltIn.allCases where query == search.query { rows.insert(.builtIn(search)) }
        for (saved, reading) in savedReadings where reading == query { rows.insert(.saved(saved.name)) }
        for case .value(let values, false) in query.tokens {
            for value in values {
                switch value {
                case .deck(let id, _, _): rows.insert(.deck(id))
                case .notetype(let id, _): rows.insert(.notetype(id))
                case .tag(let name): rows.insert(.tag(name))
                case .state(let state) where state != .leech: rows.insert(.state(state))
                case .flag(let flag): rows.insert(.flag(flag))
                case .noFlag: rows.insert(.flag(0))
                default: break
                }
            }
        }
        return rows
    }

    /// A click on `row` would remove its filter (T9): a saved search the
    /// query holds, or the one value of its kind's **is** token.
    public func activationRemoves(_ row: BrowseSidebarTag) -> Bool {
        switch row {
        case .builtIn, .saved: return selectedRows.contains(row)
        default:
            guard let value = value(of: row) else { return false }
            return query.clicking(value).tokens.count < query.tokens.count
        }
    }

    /// T9: Return or Space on the list's selection is a second click on its
    /// one filter row, where that removes the filter. True when it acted.
    @discardableResult
    public func activateByKey(_ selection: Set<BrowseSidebarTag>) -> Bool {
        let filters = selection.filter { if case .section = $0 { false } else { true } }
        guard filters.count == 1, let row = filters.first, activationRemoves(row) else { return false }
        activate(row)
        return true
    }

    /// The list's own selection: the Browse row alone (N21.1, 2026-11-24).
    /// The filter rows the query selects (`selectedRows`) are drawn by the
    /// rows themselves, so one row only carries the system's selection.
    public var listSelection: Set<BrowseSidebarTag> { [.section(.browse)] }

    /// The list's selection after a click, read as T9's input: a section
    /// row navigates (its section is returned); one filter row alone is a
    /// click (or ⌥-click) on it; with ⌘, each filter row in it is a ⌘-click;
    /// anything else (a ⇧ range, an empty selection) changes nothing. The
    /// list then draws its own selection again. The list's selection holds
    /// no filter row (N21.1), so no click needs the rows the list draws.
    public func listSelectionChanged(to new: Set<BrowseSidebarTag>, click: Click) -> AppSection? {
        defer { selectionEcho &+= 1 }
        // Any section but Browse in the new selection navigates, even alongside
        // filter rows.
        if case .section(let section)? = new.first(where: {
            if case .section(let section) = $0 { section != .browse } else { false }
        }) {
            return section
        }
        let rows = new.filter { if case .section = $0 { false } else { true } }
        switch click {
        case .command:
            for row in rows { activate(row, click: .command) }
        case .plain, .option:
            if rows.count == 1, let row = rows.first { activate(row, click: click) }
        }
        return nil
    }

    // MARK: The arrow keys in the sidebar (T9)

    /// What the sidebar's list does once its selection was read.
    public nonisolated enum ListSelectionOutcome: Sendable, Hashable {
        /// The list keeps the selection it has: the arrow keys stand on a row.
        case keep
        /// A click on the row the keys stood on: the list draws `listSelection` again now.
        case restore
        /// Any other click: the list draws `listSelection` again after the event.
        case restoreAfterEvent
        case navigate(AppSection)
    }

    /// The list reported a selection it made itself. The arrow keys (`byKey`)
    /// move through the filter rows without changing the query, and the row
    /// they stand on is the list's whole selection. A click is then read
    /// without that row: a ⌘-click on another row leaves it in the list's
    /// selection, and it was not clicked; a ⌘-click on it empties the
    /// selection, and is a ⌘-click on it.
    public func sidebarSelectionChanged(to rows: Set<BrowseSidebarTag>, byKey: Bool, click: Click) -> ListSelectionOutcome {
        guard rows != listSelection else {
            keyRow = nil
            return .keep
        }
        if let keyRow, rows == [keyRow] { return .keep }
        if let keyRow, rows.isEmpty, !byKey {
            sidebarRowClicked(keyRow, click: click)
            return .restore
        }
        if byKey, rows.count == 1, let row = rows.first, !Self.isSection(row) {
            keyRow = row
            return .keep
        }
        var rows = rows
        if let keyRow, rows.count > 1 { rows.remove(keyRow) }
        let clicked = rows.filter { !Self.isSection($0) }
        keyRow = nil
        clickedRow = clicked.count == 1 ? clicked.first : nil
        if let section = listSelectionChanged(to: rows, click: click) { return .navigate(section) }
        return .restoreAfterEvent
    }

    /// A click landed on `row`. Where the arrow keys stand on it, a plain or
    /// ⌥-click leaves the list's selection as it is, so the list reports
    /// nothing; it is T9's click on that row all the same. True when it was
    /// read here; on any other row the list's selection reports the click.
    @discardableResult
    public func sidebarRowClicked(_ row: BrowseSidebarTag, click: Click) -> Bool {
        guard keyRow == row else { return false }
        keyRow = nil
        clickedRow = row
        activate(row, click: click)
        return true
    }

    /// T9: Return or Space on the row the keys stand on is a click on it;
    /// after a click, a second click on that row where it removes its filter.
    @discardableResult
    public func activateSidebarRowByKey() -> Bool {
        if let keyRow {
            activate(keyRow)
            return true
        }
        guard let clickedRow else { return false }
        return activateByKey([clickedRow])
    }

    /// The rows shown changed, or the sidebar is built again: Return and
    /// Space never act on a row out of sight, nor on a click the sidebar
    /// now showing did not see.
    public func forgetSidebarKeyRow() {
        keyRow = nil
        clickedRow = nil
    }

    private static func isSection(_ row: BrowseSidebarTag) -> Bool {
        if case .section = row { true } else { false }
    }

    // MARK: Rows and counts

    /// The collection's rows changed (`engine/change-stream`): the names
    /// and counts are read again while Browse shows, else when it next does.
    public func changesCommitted(_ changes: ChangeSet, seq: UInt64) {
        if changes.wholeCollection || !changes.notetypes.isEmpty { fieldsStale = true }
        results.changesCommitted(changes, seq: seq)
        // Registry deletion and its Undo change which notes a tag query finds.
        // Keep the query (including a deleted value), but read its results again.
        if changes.tags.contains(where: { $0.change == .removed || $0.change == .added }) {
            results.runAgain()
        }
        inspector.changesCommitted(changes, seq: seq)
        // The selection's counts (suspended, buried, note types) may have changed.
        if changes.wholeCollection || !changes.cards.isEmpty || !changes.notes.isEmpty { commands.rowsChanged() }
        // Saved searches and flag names are config keys (K7, K8).
        let names = changes.configKeys.contains { $0.name == "savedFilters" || $0.name == "flagLabels" }
        guard changes.wholeCollection || !changes.cards.isEmpty || !changes.notes.isEmpty || !changes.decks.isEmpty
            || !changes.tags.isEmpty || !changes.notetypes.isEmpty || names else { return }
        stale = true
        if changes.wholeCollection || !changes.tags.isEmpty || !changes.notes.isEmpty {
            invalidateTags()
            if tagsRequested, !visible, ready() { readTags() }
        }
        if visible, ready() { reload() }
    }

    /// The scheduler day or the time zone changed: day values count again.
    public func dayChanged() {
        results.dayChanged()
        inspector.cardInfo.dayChanged()
        stale = true
        if visible, ready() { reload() }
    }

    /// The collection became ready again (a new generation): its names and
    /// counts are read again.
    public func collectionReady() {
        epoch &+= 1
        // A field read cut short by the new generation is made again.
        fieldsStale = true
        // A save on its way went with the old epoch: Save can be tried again.
        if savingSearch?.saving == true { savingSearch?.markFailed() }
        tagDeletion.reset()
        invalidateTags()
        commands.reset()
        results.collectionReady()
        inspector.reset()
        readies &+= 1
        if let unwritten = unwrittenMode, unwritten == mode { writeMode(unwritten) }
        stale = true
        countsTask?.cancel()
        if openInFlight {
            opened = false
            openInFlight = false
        } else if opened {
            // A save of the run text dropped with the old epoch is made again.
            keepRunText()
        }
        if tagsRequested, !visible, ready() { readTags() }
        if visible, ready() {
            if !opened { open() }
            takeHandedSearch()
            reload()
            commands.preloadSheetDefaults()
        }
    }

    /// The collection was replaced: the next open is a first open (T14).
    public func collectionReplaced() {
        epoch &+= 1
        tagDeletion.reset()
        invalidateTags()
        commands.reset()
        results.collectionReplaced()
        inspector.reset()
        countsTask?.cancel()
        opened = false
        openInFlight = false
        mode = .cards
        unwrittenMode = nil
        replacements &+= 1
        query = FilterQuery()
        duplicates = nil
        results.displayField = nil
        queryRevision &+= 1
        savedSearches = []; savedReadings = [:]; deckValues = [:]; notetypes = []; tags = []; flagNames = []
        notetypeFields = [:]; fieldsStale = true; fieldsTask?.cancel()
        counts = [:]; tagNotes = [:]; tagExpansion = [:]; tagWrites = [:]; filter = ""
        liveRun?.cancel()
        suggestions = []; suggestionScope = nil; suggestedField = nil; tokenOffers = [:]; valueCounts = [:]
        savingSearch = nil; renamingSearch = nil; editingContains = nil; committedText = nil
        stale = true
        device.runText = nil
        device.notesMode = nil
        pendingSearch = nil
        saveDevice()
    }

    public func refreshTags() async {
        await readTags()?.value
    }

    private func invalidateTags() {
        tagsToken &+= 1
        tagsTask?.cancel()
        tagsTask = nil
        tagsEpoch = nil; tagsGeneration = nil
        tagsReading = false; tagsFailed = false
    }

    @discardableResult private func readTags() -> Task<Void, Never>? {
        tagsRequested = true
        guard ready() else { return nil }
        tagsTask?.cancel()
        tagsToken &+= 1
        let token = tagsToken, epoch = epoch, generation = generation()
        tagsReading = true
        tagsTask = Task {
            defer { if token == tagsToken { tagsReading = false; tagsTask = nil } }
            guard token == tagsToken, self.epoch == epoch, self.generation() == generation,
                  ready(), !Task.isCancelled else { return }
            do {
                let tree = try await backend.tagTree()
                guard token == tagsToken, self.epoch == epoch, self.generation() == generation,
                      ready(), !Task.isCancelled else { return }
                tags = tree; tagExpansion = [:]
                tagsEpoch = epoch; tagsGeneration = generation
                tagsFailed = false
            } catch {
                guard token == tagsToken, self.epoch == epoch, self.generation() == generation,
                      ready(), !Task.isCancelled else { return }
                tagsFailed = true
            }
        }
        return tagsTask
    }

    private func reload() {
        readTags()
        stale = false
        readsPending = true
        namesToken &+= 1
        let token = namesToken, epoch = epoch, backend = backend
        namesTask = Task {
            async let saved = try? backend.values(.savedSearches)
            async let decks = try? backend.values(.decks)
            async let notetypes = try? backend.notetypes()
            async let flags = try? backend.flagNames()
            let (savedRows, deckRows, notetypeList, flagList) = await (saved, decks, notetypes, flags)
            guard token == namesToken, self.epoch == epoch else { return }
            // A read that failed is tried again on the next appear or change.
            if savedRows == nil || deckRows == nil || notetypeList == nil || flagList == nil { stale = true }
            if let savedRows {
                savedSearches = savedRows.compactMap {
                    if case .savedSearch(let name, let text) = $0 { SavedSearch(name: name, text: text) } else { nil }
                }
                // A saved search renamed, edited or deleted is read again on its next click.
                savedReadings = savedReadings.filter { saved, _ in savedSearches.contains(saved) }
            }
            if let deckRows {
                var values: [Int64: FilterValue] = [:]
                for case .value(let value) in deckRows {
                    if case .deck(let id, _, _) = value { values[id] = value }
                }
                deckValues = values
            }
            if let notetypeList {
                self.notetypes = notetypeList
                readFields(notetypeList)
            }
            await tagsTask?.value
            guard token == namesToken, self.epoch == epoch else { return }
            if tagsFailed { stale = true }
            if let flagList { flagNames = flagList }
            // The names the field's menus and suggestions offer may have changed.
            refreshTokenOffers()
            if fieldFocused { refreshSuggestions() }
            // Hidden meanwhile: the counts are read when Browse shows again.
            if visible { refreshCounts() } else { stale = true }
        }
    }

    /// The values the suggestions offer that no sidebar row counts (T12.3
    /// step 4), counted in a second read once the sidebar's counts are in.
    static let suggestedValues: [FilterValue] = [.currentDeck, .untagged, .anyFlag]
        + FilterDue.allCases.map { .due($0) }
        + FilterPeriod.allCases.flatMap { [FilterValue.added($0), .edited($0), .studied($0)] }

    /// Every row the sidebar can show, with what it counts.
    private var countedRows: [(BrowseSidebarTag, FilterCountItem)] {
        var rows: [(BrowseSidebarTag, FilterCountItem)] = FilterBuiltIn.allCases.map { (.builtIn($0), .value($0.value)) }
        rows += savedSearches.map { (.saved($0.name), .saved($0.text)) }
        rows += Self.states.map { (.state($0), .value(.state($0))) }
        rows += (0...7).map { (.flag(UInt8($0)), .value($0 == 0 ? .noFlag : .flag(UInt8($0)))) }
        rows += deckValues.sorted { $0.key < $1.key }.map { (.deck($0.key), .value($0.value)) }
        rows += notetypes.map { (.notetype($0.id), .value(.notetype(id: $0.id, name: $0.name))) }
        func walk(_ nodes: [TagNode]) {
            for node in nodes {
                rows.append((.tag(node.fullName), .value(.tag(node.fullName))))
                walk(node.children)
            }
        }
        walk(tags)
        return rows
    }

    /// T12.4: the counts, off the main thread at utility priority; a newer
    /// refresh supersedes this one (the core cancels its job) and the
    /// counts shown stay until the new ones arrive. In Cards mode, with
    /// more than 12 top-level tags, the top-level tags' note counts follow,
    /// to pick the eight Tags shows (V6).
    private func refreshCounts() {
        // Pending until every read of it lands, whatever started it (a reload,
        // a mode change): leaving Browse meanwhile reads the counts again.
        readsPending = true
        countsTask?.cancel()
        countsToken &+= 1
        let token = countsToken, epoch = epoch, mode = mode, backend = backend
        let rows = countedRows
        let topTags = tags.count > Self.tagsShownWhole ? tags.map(\.fullName) : []
        countsTask = Task(priority: .utility) {
            // The last item is the whole collection: V11's total.
            guard var values = try? await backend.counts(mode, rows.map(\.1) + [.query(FilterQuery())]), !Task.isCancelled,
                  token == countsToken, self.epoch == epoch else { return }
            results.setTotal(values.popLast() ?? nil, mode: mode)
            var next: [BrowseSidebarTag: UInt64] = [:]
            for (row, value) in zip(rows.map(\.0), values) {
                if let value { next[row] = value }
            }
            counts = next
            if !topTags.isEmpty, mode == .notes {
                tagNotes = Dictionary(uniqueKeysWithValues: topTags.compactMap { name in next[.tag(name)].map { (name, $0) } })
            } else if !topTags.isEmpty {
                // The refresh stays pending until the tags' note counts arrive too.
                let notes: [UInt64?]
                do { notes = try await backend.counts(.notes, topTags.map { .value(.tag($0)) }) } catch {
                    if token == countsToken, self.epoch == epoch { stale = true }
                    return
                }
                guard !Task.isCancelled, token == countsToken, self.epoch == epoch else { return }
                var byTag: [String: UInt64] = [:]
                for (name, value) in zip(topTags, notes) {
                    if let value { byTag[name] = value }
                }
                tagNotes = byTag
            }
            // Then the values only the suggestions show, after the sidebar's own,
            // so their slower day searches never hold the sidebar back.
            let extra = Self.suggestedValues
            let extraCounts: [UInt64?]
            do { extraCounts = try await backend.counts(mode, extra.map { .value($0) }) } catch {
                // Read again on the next appear or change; the sidebar's counts stay.
                if token == countsToken, self.epoch == epoch { stale = true }
                return
            }
            guard !Task.isCancelled, token == countsToken, self.epoch == epoch else { return }
            readsPending = false
            var shown: [FilterValue: UInt64] = [:]
            for (value, count) in zip(extra, extraCounts) {
                if let count { shown[value] = count }
            }
            valueCounts = shown
        }
    }

    /// The five states of the Card State section (V6; Leech is a saved search).
    public static let states: [FilterState] = [.new, .learning, .review, .suspended, .buried]
    /// V6: up to this many top-level tags, the Tags section shows them all.
    public static let tagsShownWhole = 12
    /// V6: otherwise it shows this many, those with the most notes.
    public static let tagsShownFew = 8

    /// V6: the top-level tags the Tags section shows, in tree order, and
    /// the number Show All names (nil when every tag shows): with more than
    /// 12, the 8 with the most notes (descendants counted), ties in tree
    /// order; before the note counts are known, the first 8.
    public private(set) var shownTopLevelTags: (tags: [TagNode], showAll: Int?) = ([], nil)

    /// Kept up to date when the tags, their note counts or Show All change,
    /// so drawing the sidebar never sorts the tags again.
    private func updateShownTags() {
        guard tags.count > Self.tagsShownWhole, !showsAllTags else {
            shownTopLevelTags = (tags, nil)
            return
        }
        let ranked = tags.indices.sorted { a, b in
            let na = tagNotes[tags[a].fullName] ?? 0, nb = tagNotes[tags[b].fullName] ?? 0
            return na != nb ? na > nb : a < b
        }
        let kept = Set(ranked.prefix(Self.tagsShownFew))
        shownTopLevelTags = (tags.indices.filter(kept.contains).map { tags[$0] }, tags.count)
    }

    /// V6: the note types the Note Types section shows, in name order, and
    /// the number Show All names (nil when every note type shows): with more
    /// than 12, the 8 with the most rows in the mode, ties in name order,
    /// and every note type the query selects; before the counts are known,
    /// the first 8.
    public private(set) var shownNotetypes: (rows: [NotetypeName], showAll: Int?) = ([], nil)

    /// Kept up to date when the note types, the counts, the query or Show
    /// All change, so drawing the sidebar never sorts them again.
    func updateShownNotetypes() {
        guard notetypes.count > Self.tagsShownWhole, !showsAllNotetypes else {
            if shownNotetypes.rows != notetypes || shownNotetypes.showAll != nil { shownNotetypes = (notetypes, nil) }
            return
        }
        let ranked = notetypes.indices.sorted { a, b in
            let na = counts[.notetype(notetypes[a].id)] ?? 0, nb = counts[.notetype(notetypes[b].id)] ?? 0
            return na != nb ? na > nb : a < b
        }
        var kept = Set(ranked.prefix(Self.tagsShownFew))
        for index in notetypes.indices where query.selects(.notetype(id: notetypes[index].id, name: notetypes[index].name)) {
            kept.insert(index)
        }
        let rows = notetypes.indices.filter(kept.contains).map { notetypes[$0] }
        if shownNotetypes.rows != rows || shownNotetypes.showAll != notetypes.count { shownNotetypes = (rows, notetypes.count) }
    }

    /// S6 over the tag tree, once per filter text and tree (the tree can
    /// hold thousands of tags; a sidebar redraw must not walk it again).
    public func filteredTags(_ filter: BrowseSidebarFilter, rootMatched: Bool) -> [BrowseSidebarFilter.Node<TagNode>] {
        guard filter.isOn else { return [] }
        let key = filter.text + (rootMatched ? "\u{0}root" : "")
        if let cache = filteredTagsCache, cache.text == key, cache.revision == tagsRevision { return cache.nodes }
        let nodes = filter.tree(tags, label: \.name, children: \.children, rootMatched: rootMatched)
        filteredTagsCache = (key, tagsRevision, nodes)
        return nodes
    }

    // MARK: Tag disclosures

    /// A tag's disclosure: the tag's own expansion (`browser/config-keys`
    /// K6.2's note); a person's toggle writes it.
    public func tagExpanded(_ node: TagNode) -> Bool {
        tagExpansion[node.fullName] ?? !node.collapsed
    }

    public func setTagExpanded(_ node: TagNode, _ expanded: Bool) {
        guard tagExpanded(node) != expanded else { return }
        tagExpansion[node.fullName] = expanded
        tagWriteToken &+= 1
        let backend = backend, name = node.fullName, token = tagWriteToken, epoch = epoch
        tagWrites[name] = token
        Task {
            do { try await backend.setTagsCollapsed([name], collapsed: !expanded) } catch {
                // Not written: the disclosure shows the stored state again, unless toggled since.
                if self.epoch == epoch, tagWrites[name] == token {
                    tagExpansion[name] = nil
                    tagWrites[name] = nil
                }
            }
        }
    }

    /// The field names of `list`'s note types, unless already read for them.
    private func readFields(_ list: [NotetypeName]) {
        let ids = Set(list.map(\.id))
        guard fieldsStale || Set(notetypeFields.keys) != ids else { return }
        fieldsStale = false
        fieldsTask?.cancel()
        let backend = backend, epoch = epoch
        fieldsTask = Task {
            var fields: [Int64: [String]] = [:]
            for type in list {
                guard !Task.isCancelled else { return }
                if let names = try? await backend.fieldNames(notetype: type.id) { fields[type.id] = names }
            }
            guard self.epoch == epoch, !Task.isCancelled else { return }
            if fields != notetypeFields { notetypeFields = fields }
        }
    }

    // MARK: Device state

    func saveDevice() {
        device.folded = folded
        device.showsAllTags = showsAllTags
        device.showsAllNotetypes = showsAllNotetypes ? true : nil
        store.save(device)
    }

    // MARK: Test hooks

    /// Puts the model in a sampled state without the core (test-hook routes).
    @_spi(MeguriTestHooks) public func prepareForTestHooks(query: FilterQuery) async {
        opened = true
        setQuery(query)
        reload()
        await namesTask?.value
        await countsTask?.value
    }
}
