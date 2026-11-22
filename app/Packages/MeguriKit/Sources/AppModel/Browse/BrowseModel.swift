import CoreBridge
import Foundation
import Observation

/// Browse's state (U270): the query, which the sidebar's filter rows show
/// (`search/filter-tokens` T1, T9), the mode, and the sidebar's content
/// and counts (`browser/workspace` V6, T12.4). Held by `ReviewCollection`
/// above the layout switch, so leaving Browse for Today, a deck or review
/// and coming back finds it as it was (V1); a relaunch starts as T14 says.
/// Every read here is pure: nothing writes the collection but a person's
/// toggle of a tag's disclosure.
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

    // MARK: State

    /// What the search field holds (T1); the sidebar is a view of it.
    public internal(set) var query = FilterQuery()
    /// Cards or Notes (V2.1): rows and counts are of this mode (T11).
    public private(set) var mode: BrowseMode = .cards
    /// Browse has opened in this launch (T14's first open has run).
    public private(set) var opened = false
    public private(set) var savedSearches: [SavedSearch] = []
    /// Every deck's value as the core makes it (T3), by deck id.
    public private(set) var deckValues: [Int64: FilterValue] = [:]
    public private(set) var notetypes: [NotetypeName] = []
    public private(set) var tags: [TagNode] = [] {
        didSet { tagsRevision &+= 1; updateShownTags() }
    }
    /// Flags 1 to 7's shown names (F4).
    public private(set) var flagNames: [String] = []
    /// Each row's count in the current mode (T12.3 step 4); a row without
    /// one shows none until it is known.
    public private(set) var counts: [BrowseSidebarTag: UInt64] = [:]
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
    /// Raised when a selection the list made is not the query's, so the
    /// list draws the query's selection again.
    public private(set) var selectionEcho: UInt64 = 0

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
    /// A Field token's (or Field suggestion's) Contains… is asking for its text (T10).
    public var editingContains: EditingContains?
    /// Saved searches' suggestions read again after a Save Search.
    @ObservationIgnored var suggestionsToken: UInt64 = 0
    /// The text the suggestions shown were read for.
    @ObservationIgnored var suggestionsText: String?
    @ObservationIgnored var offersToken: UInt64 = 0
    @ObservationIgnored var liveRun: Task<Void, Never>?
    /// The text a commit left, which D5's incomplete rule no longer holds (T8).
    @ObservationIgnored var committedText: String?

    // MARK: Seams

    @ObservationIgnored let backend: any BrowseBackend
    @ObservationIgnored private let store: BrowseDeviceStore
    /// Whether the collection is open and ready for reads.
    @ObservationIgnored public var ready: @MainActor () -> Bool = { true } {
        didSet { let ready = ready; results.ready = { ready() } }
    }
    /// A handed search whose reading failed, tried again on the next appear.
    var pendingSearch: String?
    /// A search handed to Browse (a deep link, R319), taken once: T14's
    /// "with a search" replaces the query by its reading.
    @ObservationIgnored public var takeEntrySearch: @MainActor () -> String? = { nil }

    public init(backend: any BrowseBackend, store: BrowseDeviceStore = BrowseDeviceStore(defaults: nil)) {
        self.backend = backend
        self.store = store
        results = BrowseResults(backend: backend)
        let device = store.load()
        self.device = device
        folded = device.folded
        showsAllTags = device.showsAllTags
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
        if stale { reload() }
        // The rows of the query, once the first open has read it (T14).
        if !openInFlight, !results.hasRun { showResults() }
    }

    public func disappear() {
        visible = false
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

    func setQuery(_ next: FilterQuery) {
        queryRevision &+= 1
        liveRun?.cancel()
        let tokensChanged = next.tokens != query.tokens
        if next != query { query = next }
        keepRunText()
        showResults()
        if tokensChanged { refreshTokenOffers() }
        if fieldFocused { refreshSuggestions() }
    }

    /// The results show the query in the mode.
    func showResults() {
        results.run(query, mode: mode)
    }

    /// The no-results state's Clear Search: the empty query.
    public func clearQuery() {
        pendingSearch = nil
        setQuery(FilterQuery())
    }

    /// V2.1: the Cards / Notes control. The query stays (T11); the rows
    /// toggle (H11), the counts are of the new mode, and the mode is written
    /// to the collection (K6.1) and kept on the device (T14).
    public func setMode(_ next: BrowseMode) {
        guard next != mode else { return }
        mode = next
        device.notesMode = next == .notes
        saveDevice()
        counts = [:]
        valueCounts = [:]
        refreshCounts()
        results.setMode(next, query: query)
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
    func replace(byReading text: String, handed: Bool = false) {
        queryRevision &+= 1
        let revision = queryRevision, epoch = epoch
        // A handed search is kept until it is applied: a failed read, or one the
        // collection turning ready again discards, is tried again.
        if handed { pendingSearch = text }
        Task {
            guard let read = try? await backend.read(text), self.epoch == epoch, revision == queryRevision else { return }
            if handed, pendingSearch == text { pendingSearch = nil }
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
            setQuery(search.query)
        case .saved(let name):
            guard let saved = savedSearches.first(where: { $0.name == name }) else { return }
            replace(byReading: saved.text)
        case .showAllTags:
            showsAllTags = true
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
        case .section, .saved, .showAllTags: nil
        }
    }

    /// T9: the filter rows that show selected, those whose value is in its
    /// kind's **is** token.
    public var selectedRows: Set<BrowseSidebarTag> {
        var rows = Set<BrowseSidebarTag>()
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

    /// The list's selection: the Browse row, then the selected filter rows.
    public var listSelection: Set<BrowseSidebarTag> {
        selectedRows.union([.section(.browse)])
    }

    /// The list's selection after a click or a key, read as T9's input:
    /// a section row navigates (its section is returned); one filter row
    /// alone is a click (or ⌥-click) on it; with ⌘, each row that came or
    /// went is a ⌘-click; anything else (a ⇧ range, an empty selection)
    /// changes nothing. The list then draws the query's selection again.
    /// `shown` is the rows the list draws: a selected row it does not draw
    /// (folded, filtered out, under a collapsed parent) is not in `new`, and
    /// is not taken as unselected.
    public func listSelectionChanged(to new: Set<BrowseSidebarTag>, click: Click,
                                     shown: Set<BrowseSidebarTag>? = nil) -> AppSection? {
        let old = shown.map { listSelection.intersection($0.union([.section(.browse)])) } ?? listSelection
        defer { selectionEcho &+= 1 }
        if case .section(let section)? = new.subtracting(old).first(where: { if case .section = $0 { true } else { false } }),
           section != .browse {
            return section
        }
        let rows = new.filter { if case .section = $0 { false } else { true } }
        switch click {
        case .command:
            for row in rows.symmetricDifference(old.filter { if case .section = $0 { false } else { true } }) {
                activate(row, click: .command)
            }
        case .plain, .option:
            if rows.count == 1, let row = rows.first { activate(row, click: click) }
        }
        return nil
    }

    // MARK: Rows and counts

    /// The collection's rows changed (`engine/change-stream`): the names
    /// and counts are read again while Browse shows, else when it next does.
    public func changesCommitted(_ changes: ChangeSet, seq: UInt64) {
        results.changesCommitted(changes, seq: seq)
        // Saved searches and flag names are config keys (K7, K8).
        let names = changes.configKeys.contains { $0.name == "savedFilters" || $0.name == "flagLabels" }
        guard changes.wholeCollection || !changes.cards.isEmpty || !changes.notes.isEmpty || !changes.decks.isEmpty
            || !changes.tags.isEmpty || !changes.notetypes.isEmpty || names else { return }
        stale = true
        if visible, ready() { reload() }
    }

    /// The scheduler day or the time zone changed: day values count again.
    public func dayChanged() {
        results.dayChanged()
        stale = true
        if visible, ready() { reload() }
    }

    /// The collection became ready again (a new generation): its names and
    /// counts are read again.
    public func collectionReady() {
        epoch &+= 1
        results.collectionReady()
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
        if visible, ready() {
            if !opened { open() }
            takeHandedSearch()
            reload()
        }
    }

    /// The collection was replaced: the next open is a first open (T14).
    public func collectionReplaced() {
        epoch &+= 1
        results.collectionReplaced()
        countsTask?.cancel()
        opened = false
        openInFlight = false
        mode = .cards
        unwrittenMode = nil
        replacements &+= 1
        query = FilterQuery()
        queryRevision &+= 1
        savedSearches = []; deckValues = [:]; notetypes = []; tags = []; flagNames = []
        counts = [:]; tagNotes = [:]; tagExpansion = [:]; tagWrites = [:]; filter = ""
        liveRun?.cancel()
        suggestions = []; suggestionScope = nil; suggestedField = nil; tokenOffers = [:]; valueCounts = [:]
        savingSearch = nil; editingContains = nil; committedText = nil
        stale = true
        device.runText = nil
        device.notesMode = nil
        pendingSearch = nil
        saveDevice()
    }

    private func reload() {
        stale = false
        readsPending = true
        namesToken &+= 1
        let token = namesToken, epoch = epoch, backend = backend
        namesTask = Task {
            async let saved = try? backend.values(.savedSearches)
            async let decks = try? backend.values(.decks)
            async let notetypes = try? backend.notetypes()
            async let tags = try? backend.tagTree()
            async let flags = try? backend.flagNames()
            let (savedRows, deckRows, notetypeList, tagTree, flagList) = await (saved, decks, notetypes, tags, flags)
            guard token == namesToken, self.epoch == epoch else { return }
            // A read that failed is tried again on the next appear or change.
            if savedRows == nil || deckRows == nil || notetypeList == nil || tagTree == nil || flagList == nil { stale = true }
            if let savedRows {
                savedSearches = savedRows.compactMap {
                    if case .savedSearch(let name, let text) = $0 { SavedSearch(name: name, text: text) } else { nil }
                }
            }
            if let deckRows {
                var values: [Int64: FilterValue] = [:]
                for case .value(let value) in deckRows {
                    if case .deck(let id, _, _) = value { values[id] = value }
                }
                deckValues = values
            }
            if let notetypeList { self.notetypes = notetypeList }
            if let tagTree { self.tags = tagTree; tagExpansion = [:] }
            if let flagList { flagNames = flagList }
            // The names the field's menus and suggestions offer may have changed.
            refreshTokenOffers()
            if fieldFocused { refreshSuggestions() }
            // Hidden meanwhile: the counts are read when Browse shows again.
            if visible { refreshCounts() } else { stale = true }
        }
    }

    /// The values the suggestions offer that no sidebar row counts (T12.3
    /// step 4), counted in the same read so the first suggestion screen's
    /// counts come with the sidebar's (T12.4's budget).
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
        countsTask?.cancel()
        countsToken &+= 1
        let token = countsToken, epoch = epoch, mode = mode, backend = backend
        let rows = countedRows
        let topTags = tags.count > Self.tagsShownWhole ? tags.map(\.fullName) : []
        countsTask = Task(priority: .utility) {
            // Then the values only the suggestions show; the last item is the whole collection: V11's total.
            let extra = Self.suggestedValues
            guard var values = try? await backend.counts(mode, rows.map(\.1) + extra.map { .value($0) } + [.query(FilterQuery())]),
                  !Task.isCancelled, token == countsToken, self.epoch == epoch else { return }
            results.setTotal(values.popLast() ?? nil, mode: mode)
            var extraCounts: [FilterValue: UInt64] = [:]
            for (value, count) in zip(extra, values.suffix(extra.count)) {
                if let count { extraCounts[value] = count }
            }
            values.removeLast(min(extra.count, values.count))
            valueCounts = extraCounts
            var next: [BrowseSidebarTag: UInt64] = [:]
            for (row, value) in zip(rows.map(\.0), values) {
                if let value { next[row] = value }
            }
            counts = next
            guard !topTags.isEmpty else { readsPending = false; return }
            if mode == .notes {
                tagNotes = Dictionary(uniqueKeysWithValues: topTags.compactMap { name in next[.tag(name)].map { (name, $0) } })
                readsPending = false
                return
            }
            // The refresh stays pending until the tags' note counts arrive too.
            let notes: [UInt64?]
            do { notes = try await backend.counts(.notes, topTags.map { .value(.tag($0)) }) } catch {
                if token == countsToken, self.epoch == epoch { stale = true }
                return
            }
            guard !Task.isCancelled, token == countsToken, self.epoch == epoch else { return }
            readsPending = false
            var byTag: [String: UInt64] = [:]
            for (name, value) in zip(topTags, notes) {
                if let value { byTag[name] = value }
            }
            tagNotes = byTag
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

    // MARK: Device state

    func saveDevice() {
        device.folded = folded
        device.showsAllTags = showsAllTags
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
