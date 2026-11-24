import CoreBridge
import Foundation
import Observation

/// Browse's results table (U270a; `browser/workspace` V2.1, V2.2, V4, V11,
/// `browser/result-sets` H3 to H12): the rows of the query, read a page at
/// a time around the rows on screen, never the whole result.
///
/// - **Requests.** A query or mode change starts a search; a header click
///   resorts the shown handle (H8) and Cards / Notes toggles it (H11), both
///   without running the predicate again. A newer request supersedes the
///   older one in the core (H3). Starts are issued one at a time, so the
///   events of a request whose start has not returned yet are its own.
/// - **No flash.** The rows shown stay until the new request's first page
///   of rows is read; only then does the table change. "Searching…" shows
///   only after half a second (`showsProgress`).
/// - **Selection** is a set of indexes of the shown list; a new list maps
///   it with H12's `map_selection`, and H12's no-current rule applies.
/// - **Change sets** (H10) remove rows and read touched rows again in
///   place; membership is never evaluated again.
///
/// Held by `BrowseModel`, above the layout switch (V1).
@Observable @MainActor
public final class BrowseResults {
    /// What the results area shows.
    public nonisolated enum Content: Equatable, Sendable {
        /// The launch's first search has not shown rows yet.
        case starting
        case table
        /// The empty query finds nothing: the collection has no cards (or notes).
        case emptyCollection
        /// The query finds nothing (V16 "No Cards" with Clear Search).
        case noResults
        /// A search failed for another reason than its text (Try Again).
        case failed
    }

    /// The list the table shows.
    public nonisolated struct List: Equatable, Sendable {
        /// Changes whenever another handle is shown.
        public let token: UInt64
        public let handle: BrowseHandle
        public let mode: BrowseMode
        /// The order of the rows, as the header shows it.
        public let sort: BrowseSort
        /// Column keys, the state column aside (V2.3).
        public internal(set) var columns: [String]
        /// The rows known so far (the final prefix while streaming, H6).
        public internal(set) var count: Int
        public internal(set) var revision: UInt64
        public internal(set) var isComplete: Bool
        /// The core released the handle (a superseded search, H12): the rows
        /// read stay, nothing more is read from it.
        public internal(set) var released: Bool
        /// The run text searched, for a new search when a resort or a
        /// toggle needs a complete handle (H8, H11).
        let text: String
        let queryIsEmpty: Bool
    }

    // MARK: State

    public private(set) var list: List? {
        didSet {
            let released = list?.released ?? false
            if released != isDimmed { isDimmed = released }
            if list?.token != oldValue?.token || list?.revision != oldValue?.revision || list?.released != oldValue?.released {
                selectionObserver?()
            }
        }
    }
    /// The rows shown belong to a search that went (superseded, failed):
    /// they show dimmed until the next list (H12, R1108).
    public private(set) var isDimmed = false
    /// The rows read, by page; replaced with the list, so every cell draws again.
    public private(set) var store = BrowseRowStore()
    /// Indexes of `list`'s rows (R1152).
    public private(set) var selection: Set<Int> = [] {
        didSet { if selection != oldValue { selectionObserver?() } }
    }
    /// The current row (R1152, H12): the row last chosen.
    public private(set) var currentIndex: Int?
    /// A search has run for half a second without its rows shown or whole.
    public private(set) var showsProgress = false
    /// The latest search failed for a reason other than its text.
    public private(set) var failed = false
    /// The first search's text could not run (T5): with no rows to keep,
    /// it shows as finding nothing, with Clear Search.
    public private(set) var rejected = false
    /// The rows of the whole collection in the shown mode (V11's t); nil
    /// until counted.
    public private(set) var total: UInt64?
    /// The column set the query uses (V2.2).
    public private(set) var columnSet: ColumnSetShown?
    /// A row the table brings into view.
    public nonisolated struct ScrollTarget: Equatable, Sendable {
        public let token: UInt64
        public let index: Int
        let serial: UInt64
    }

    public private(set) var scrollTarget: ScrollTarget?
    /// The table has the keyboard focus: ⌘⌫ is Note › Delete then (N21.8).
    public var tableFocused = false
    @ObservationIgnored private var scrollSerial: UInt64 = 0

    public var content: Content {
        if failed { return .failed }
        guard let list else { return rejected ? .noResults : .starting }
        if list.count == 0, list.isComplete { return list.queryIsEmpty ? .emptyCollection : .noResults }
        return .table
    }

    /// V11: a request is under way.
    public var isSearching: Bool {
        guard let request = newest else { return list.map { !$0.isComplete && !$0.released } ?? false }
        return !request.shown || !(list?.isComplete ?? true)
    }

    // MARK: Seams

    @ObservationIgnored private let backend: any BrowseBackend
    /// Formats rows off the main thread; the Browse feature gives its own.
    @ObservationIgnored public var presenter: any BrowseRowPresenting = PlainBrowsePresenter()
    @ObservationIgnored public var ready: @MainActor () -> Bool = { true }
    /// How long a search runs before "Searching…" shows.
    @ObservationIgnored public var progressDelay: Duration = .milliseconds(500)
    /// Told when the selection, or the list it indexes, changes (the
    /// commands' counts follow it, U275).
    @ObservationIgnored var selectionObserver: (@MainActor () -> Void)?

    // MARK: Internals

    /// One request of the session (H1).
    private final class Request {
        enum Kind { case search, resort(BrowseHandle), toggle(BrowseHandle) }
        let kind: Kind
        let text: String
        let mode: BrowseMode
        let sort: BrowseSort
        let columns: [String]
        let queryIsEmpty: Bool
        /// What the selection maps from, taken when its list is about to show
        /// (H12), and whether the list was whole then.
        var mapping: Mapping?
        var mappedWhole = false
        /// The user's selection revision the mapping was taken at: a later
        /// choice of rows makes it stale.
        var mappingRevision: UInt64 = 0
        /// The query revision it was issued for: a newer query never shows it.
        var queryRevision: UInt64 = 0
        var seq: UInt64?
        var handle: BrowseHandle?
        var count = 0
        var revision: UInt64 = 0
        var complete = false
        var shown = false
        var ended = false
        /// Change sets received since it started (H10).
        var changes: [SequencedChangeSet] = []
        /// The ids this request's list selects in place of the selection
        /// shown (rows an undo brought back, V8.3).
        var selecting: [Int64]?

        init(kind: Kind, text: String, mode: BrowseMode, sort: BrowseSort, columns: [String], queryIsEmpty: Bool) {
            self.kind = kind
            self.text = text
            self.mode = mode
            self.sort = sort
            self.columns = columns
            self.queryIsEmpty = queryIsEmpty
        }

        /// The same request as a new search: H8 and H11 need a whole handle.
        var asSearch: Request {
            let search = Request(kind: .search, text: text, mode: mode, sort: sort, columns: columns, queryIsEmpty: queryIsEmpty)
            search.queryRevision = queryRevision
            search.selecting = selecting
            return search
        }
    }

    private struct Mapping {
        let from: BrowseMapSource
        let selected: BrowseSelected
        let current: Int64?
        /// The same selection as the ids of the rows read, when `from` is a handle.
        var byIDs: Fallback?

        struct Fallback {
            let from: BrowseMapSource
            let selected: BrowseSelected
            let current: Int64?
        }
    }

    @ObservationIgnored private var session: (any BrowseSearching)?
    @ObservationIgnored private var opening: Task<(any BrowseSearching)?, Never>?
    @ObservationIgnored private var eventsTask: Task<Void, Never>?
    /// The newest request, shown or not.
    private var newest: Request?
    /// A start call is on its way; the next request waits for it.
    @ObservationIgnored private var starting = false
    /// The request whose start is on its way: an event newer than every seq
    /// returned is its own, even when a newer request waits behind it.
    @ObservationIgnored private var inFlight: Request?
    /// A run text is being read: Cards / Notes and header clicks apply to the
    /// search it starts.
    @ObservationIgnored private var runPending = false
    /// A header click made while the run text was read, in its mode.
    @ObservationIgnored private var sortOverride: (mode: BrowseMode, sort: BrowseSort)?
    /// The set's sort writes, one after another.
    @ObservationIgnored private var sortWrite: Task<Void, Never>?
    /// Ids of cards and notes change sets removed while this query showed (`restoreRows`).
    @ObservationIgnored private var goneCards: Set<Int64> = []
    @ObservationIgnored private var goneNotes: Set<Int64> = []
    /// The page whose neighbours were last kept.
    @ObservationIgnored private var keptCenter: Int?
    @ObservationIgnored private var queued: Request?
    /// The newest seq a start returned.
    @ObservationIgnored private var lastReturned: UInt64 = 0
    @ObservationIgnored private var tokens: UInt64 = 0
    /// Raised by every query change: a reading of an older one gives way.
    @ObservationIgnored private var queryRevision: UInt64 = 0
    /// Raised by every selection the user makes: a later mapping gives way.
    @ObservationIgnored private var selectionRevision: UInt64 = 0
    /// Raised when the collection goes: nothing started before lands.
    @ObservationIgnored private var epoch: UInt64 = 0
    @ObservationIgnored private var progressTask: Task<Void, Never>?
    /// The last query and mode run, for Try Again and a new generation.
    @ObservationIgnored private var lastQuery = FilterQuery()
    @ObservationIgnored private var lastMode: BrowseMode = .cards
    @ObservationIgnored private(set) var hasRun = false
    @ObservationIgnored private var columnSets: [Int64?: ColumnSetShown] = [:]
    /// The list's change sets not yet applied (H10), and the one call applying them.
    @ObservationIgnored private var unapplied: [SequencedChangeSet] = []
    @ObservationIgnored private var applying = false
    // Paging.
    @ObservationIgnored private var visibleIndex = 0
    @ObservationIgnored private var fetching: Set<Int> = []
    @ObservationIgnored private var qaFetching: Set<Int> = []
    /// Failed reads tried again, by page.
    @ObservationIgnored private var retries: [Int: Int] = [:]
    @ObservationIgnored private var qaRetries: [Int: Int] = [:]

    public init(backend: any BrowseBackend) {
        self.backend = backend
    }

    // MARK: Running the query

    /// A query or mode to show: its run text (T4) searched with the column
    /// set's sort for the mode (V2.2, V4.3).
    public func run(_ query: FilterQuery, mode: BrowseMode) {
        if query != lastQuery { sortOverride = nil; goneCards = []; goneNotes = [] }
        rejected = false
        lastQuery = query
        lastMode = mode
        hasRun = true
        queryRevision &+= 1
        guard ready() else { return }
        runPending = true
        let revision = queryRevision, epoch = epoch, backend = backend
        let notetype = Self.notetype(of: query)
        Task {
            do {
                async let text = backend.runText(query)
                let set = try await columnSet(notetype)
                let runText = try await text
                guard revision == queryRevision, self.epoch == epoch else { return }
                runPending = false
                columnSet = set
                // The mode and a header click as they are now, not as they were when the run began.
                let mode = lastMode
                var sort = set.sort(mode)
                if let override = sortOverride, override.mode == mode {
                    // A header click made while the text was read sorts this query's set.
                    sort = override.sort
                    writeSort(sort, set: set.set, mode: mode)
                    columnSet = set.sorted(sort, mode: mode)
                    columnSets[set.notetypeKey] = columnSet
                }
                issue(Request(kind: .search, text: runText, mode: mode, sort: sort, columns: set.columns(mode),
                              queryIsEmpty: query.isEmpty))
            } catch {
                guard revision == queryRevision, self.epoch == epoch else { return }
                runPending = false
                fail()
            }
        }
    }

    /// The run the results need when they show again or the collection is
    /// ready again: the last query and mode, or none before the first run.
    public func runAgain() {
        guard hasRun else { return }
        run(lastQuery, mode: lastMode)
    }

    /// Try Again (the error state).
    public func retry() { runAgain() }

    /// V2.1: Cards / Notes. A complete handle toggles without a run (H11);
    /// otherwise the search runs again in the other mode.
    public func setMode(_ mode: BrowseMode, query: FilterQuery) {
        guard query == lastQuery else { return run(query, mode: mode) }
        lastMode = mode
        // A run text being read takes the mode when its search starts.
        if runPending { return }
        // A search not shown yet is the query: it runs again in the mode.
        guard let list, let set = columnSet, newest?.shown != false, list.mode != mode, ready() else {
            return run(query, mode: mode)
        }
        let kind: Request.Kind = list.isComplete && !list.released ? .toggle(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: mode, sort: set.sort(mode), columns: set.columns(mode),
                      queryIsEmpty: list.queryIsEmpty))
    }

    /// V4.1: a header click: the column in its first direction, or the
    /// order shown reversed, as a resort of the shown handle (H8) and a
    /// write of the set's sort (V4.3, no undo step).
    public func sort(by key: String) {
        guard let list, ready() else { return }
        // The order asked for last: a request not shown yet, a run being read, or the list.
        let pending = newest.flatMap { $0.shown || $0.ended ? nil : $0 }
        let mode = runPending ? lastMode : pending?.mode ?? list.mode
        guard BrowseColumns.sortable(key, mode: mode) else { return }
        let current = runPending ? (sortOverride.flatMap { $0.mode == mode ? $0.sort : nil } ?? columnSet?.sort(mode))
            : pending?.sort ?? list.sort
        guard let current, let sort = BrowseColumns.click(key, current: current) else { return }
        if runPending {
            // The set is the new query's, not known yet: it is written when its search starts.
            sortOverride = (mode, sort)
            return
        }
        if let set = columnSet {
            writeSort(sort, set: set.set, mode: mode)
            columnSet = set.sorted(sort, mode: mode)
            columnSets[set.notetypeKey] = columnSet
        }
        if let pending {
            // Not shown yet: its search runs again with the new order.
            issue(Request(kind: .search, text: pending.text, mode: pending.mode, sort: sort, columns: pending.columns,
                          queryIsEmpty: pending.queryIsEmpty))
            return
        }
        let kind: Request.Kind = list.isComplete && !list.released ? .resort(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: list.mode, sort: sort, columns: list.columns,
                      queryIsEmpty: list.queryIsEmpty))
    }

    /// V4.3: the set's sort, written in click order, as the order shown.
    private func writeSort(_ sort: BrowseSort, set: ColumnSetID, mode: BrowseMode) {
        let previous = sortWrite, backend = backend
        sortWrite = Task {
            await previous?.value
            try? await backend.changeColumnSet(set, mode: mode, change: .setSort(sort))
        }
    }

    private func issue(_ request: Request) {
        // The shown list's request, still streaming, is superseded: its handle goes (H6).
        if var shown = list, !shown.isComplete, newest?.shown == true {
            shown.released = true
            list = shown
        }
        request.queryRevision = queryRevision
        newest = request
        startProgress()
        guard !starting else {
            queued = request
            return
        }
        start(request)
    }

    private func start(_ request: Request) {
        starting = true
        inFlight = request
        let epoch = epoch
        Task {
            defer {
                starting = false
                if inFlight === request { inFlight = nil }
                if let next = queued {
                    queued = nil
                    if next === newest { start(next) }
                }
            }
            guard let session = await openSession(), self.epoch == epoch else { return }
            do {
                let seq: UInt64 = switch request.kind {
                case .search: try await session.startSearch(request.text, mode: request.mode, sort: request.sort)
                case .resort(let handle): try await session.resort(handle, sort: request.sort)
                case .toggle(let handle): try await session.toggle(handle, sort: request.sort)
                }
                lastReturned = max(lastReturned, seq)
                request.seq = seq
            } catch BridgeError.browse(.notComplete), BridgeError.browse(.unknownHandle) {
                // H8, H11: the handle is not whole yet, or gone: the search runs again with the new sort or mode.
                guard request === newest, self.epoch == epoch else { return }
                let again = request.asSearch
                newest = again
                queued = again
            } catch {
                guard request === newest, self.epoch == epoch else { return }
                request.ended = true
                fail()
            }
        }
    }

    private func openSession() async -> (any BrowseSearching)? {
        if let session { return session }
        if let opening { return await opening.value }
        let backend = backend
        let task = Task<(any BrowseSearching)?, Never> { try? await backend.openSearchSession() }
        opening = task
        let opened = await task.value
        opening = nil
        guard let opened else {
            fail()
            return nil
        }
        session = opened
        eventsTask = Task { [weak self] in
            for await event in opened.events {
                guard let self else { return }
                self.receive(event)
            }
        }
        return opened
    }

    /// A failure with rows shown keeps them, dimmed (H12, R1108); with
    /// none, the error state offers Try Again.
    private func fail() {
        if var shown = list, shown.count > 0 {
            shown.released = true
            list = shown
        } else {
            failed = true
        }
        stopProgress()
    }

    // MARK: Events (H4)

    /// The request an event belongs to: the newest one, when the event is
    /// newer than every seq returned so far and it is the one starting, or
    /// it carries its seq.
    private func request(for seq: UInt64) -> Request? {
        if seq > lastReturned, let inFlight {
            // The request whose start is on its way; a newer one waiting behind it drops its events.
            if inFlight.seq == nil { inFlight.seq = seq }
            return inFlight === newest && !inFlight.ended && inFlight.seq == seq ? inFlight : nil
        }
        guard let newest, !newest.ended, newest.seq == seq else { return nil }
        return newest
    }

    func receive(_ event: BrowseSearchEvent) {
        guard let request = request(for: event.seq) else { return }
        switch event {
        case .firstPage(_, let handle, let ids):
            request.handle = handle
            request.count = ids.count
            show(request)
        case .progress(_, let matched, _, let revision):
            if request.shown, var shown = list, shown.handle == request.handle {
                // A progress older than a prune counts in a revision gone (H6).
                guard revision >= shown.revision else { return }
                shown.count = Int(matched)
                list = shown
                remap(request)
            } else {
                request.count = Int(matched)
                request.revision = revision
            }
        case .complete(_, let handle, let count, let revision):
            request.complete = true
            request.handle = handle
            if request.shown, var shown = list, shown.handle == handle {
                // Terminal events are never ignored (H4): a revision older than
                // the last prune keeps the count that prune gave.
                if revision >= shown.revision { shown.count = Int(count) }
                shown.isComplete = true
                list = shown
                stopProgress()
                remap(request)
            } else {
                request.count = Int(count)
                request.revision = revision
            }
        case .failed(_, let error):
            request.ended = true
            if request.shown, var shown = list {
                shown.released = true
                list = shown
            }
            // T5: a search the text cannot run keeps the results shown; any
            // other failure keeps the rows read too, dimmed (H12, R1108), and
            // shows Try Again only when there are none.
            if case .browse(.search) = error {
                if list == nil { rejected = true }
                stopProgress()
            } else {
                fail()
            }
        case .cancelled:
            request.ended = true
            if request.shown, var shown = list {
                shown.released = true
                list = shown
            }
            stopProgress()
        }
    }

    /// The request's first page is final: its rows are read, and the
    /// selection mapped, before the table changes (no flash).
    private func show(_ request: Request) {
        guard let handle = request.handle, let session else { return }
        let epoch = epoch, revision = request.revision, presenter = presenter
        let first = 0..<UInt64(min(request.count, BrowseRowStore.pageSize))
        let columns = request.columns, mode = request.mode
        // The selection as it is now: rows chosen, or pruned, since the request began count;
        // or the rows an undo brought back.
        request.mapping = request.selecting.map { Mapping(from: .mode(request.mode), selected: .ids($0), current: $0.first) }
            ?? mapping()
        request.mappedWhole = request.complete
        request.mappingRevision = selectionRevision
        let selectionAtShow = selectionRevision
        Task {
            async let rows = Self.read(session, handle, revision, first, columns, mode, presenter)
            async let mapped = mapping(request, in: session, to: handle, revision: revision)
            let (page, selected) = await (try? rows, mapped)
            // A query chosen since (its search not issued yet) keeps the list shown.
            guard request === newest, !request.ended, self.epoch == epoch, request.queryRevision == queryRevision else { return }
            // Its first rows could not be read: the rows shown stay (dimmed), or Try Again.
            guard page != nil || first.isEmpty else {
                request.ended = true
                return fail()
            }
            tokens &+= 1
            let next = List(token: tokens, handle: handle, mode: request.mode, sort: request.sort, columns: request.columns,
                            count: request.count, revision: request.revision, isComplete: request.complete, released: false,
                            text: request.text, queryIsEmpty: request.queryIsEmpty)
            let store = BrowseRowStore()
            if let page { store.page(0).set(page) }
            request.shown = true
            failed = false
            rejected = false
            self.store = store
            list = next
            // A column shown or hidden while this request was on its way (a resort, Cards / Notes).
            if !runPending, let set = columnSet, set.columns(next.mode) != next.columns { showColumns(set.columns(next.mode)) }
            keptCenter = nil
            fetching = []
            qaFetching = []
            unapplied = request.changes
            if selectionAtShow == selectionRevision { apply(selected, count: next.count, mapped: request.mapping != nil) }
            else {
                // Rows chosen during the read: the mapping is stale, now and at completion.
                selection = []
                currentIndex = nil
                request.mapping = nil
            }
            retries = [:]
            qaRetries = [:]
            if let current = currentIndex, current > 0 { bringIntoView(current) }
            if request.complete { stopProgress() }
            // A list that became whole while its first rows were read maps the selection again.
            if request.complete, !request.mappedWhole { remap(request) }
            if !unapplied.isEmpty { applyChanges() }
            readNearRows()
        }
    }

    /// A mapping made when the new list was not whole yet is made again once
    /// it is (H12), unless the user chose rows meanwhile.
    private func remap(_ request: Request) {
        // Rows the user chose since the mapping was taken stay as chosen.
        guard request.mapping != nil, request.mappingRevision == selectionRevision, let handle = request.handle,
              let session else { return }
        let revision = selectionRevision, epoch = epoch, listRevision = list?.revision ?? 0
        Task {
            let selected = await mapping(request, in: session, to: handle, revision: listRevision)
            guard self.epoch == epoch, revision == selectionRevision, request === newest, let list, list.handle == handle,
                  list.revision == listRevision else { return }
            apply(selected, count: list.count, mapped: true)
        }
    }

    private func mapping(_ request: Request, in session: any BrowseSearching, to handle: BrowseHandle,
                         revision: UInt64) async -> BrowseMappedSelection? {
        guard let mapping = request.mapping else { return nil }
        do {
            return try await session.mapSelection(from: mapping.from, selected: mapping.selected, current: mapping.current,
                                                  to: handle, targetRevision: revision)
        } catch {
            // The source handle went (H6): the ids of the rows read map instead.
            guard let fallback = mapping.byIDs else { return nil }
            return try? await session.mapSelection(from: fallback.from, selected: fallback.selected, current: fallback.current,
                                                   to: handle, targetRevision: revision)
        }
    }

    /// H12's rules on a mapped selection: a current row and nothing else
    /// selected selects it; no current row makes the first selected row
    /// current; nothing selected selects the first row. Without a selection
    /// to map, nothing is selected.
    private func apply(_ mapped: BrowseMappedSelection?, count: Int, mapped wasMapped: Bool) {
        guard wasMapped else {
            selection = []
            currentIndex = nil
            return
        }
        var selected = Set<Int>()
        for range in mapped?.selected ?? [] {
            selected.formUnion(Int(clamping: range.lowerBound)..<Int(clamping: min(range.upperBound, UInt64(count))))
        }
        var current = mapped?.current.map { Int(clamping: $0) }.flatMap { $0 < count ? $0 : nil }
        if let index = current, selected.isEmpty { selected = [index] }
        if current == nil { current = selected.min() }
        if selected.isEmpty, count > 0 {
            selected = [0]
            current = 0
        }
        selection = selected
        currentIndex = current
    }

    /// What the selection maps from when another list replaces this one:
    /// index ranges of a whole live handle, else the ids of rows read.
    private func mapping() -> Mapping? {
        guard let list, !selection.isEmpty || currentIndex != nil else { return nil }
        let currentID = currentIndex.flatMap { row(at: $0)?.id }
        let ids = { self.selection.sorted().compactMap { self.row(at: $0)?.id } }
        if list.isComplete, !list.released {
            let fallback = Mapping.Fallback(from: .mode(list.mode), selected: .ids(ids()), current: currentID)
            return Mapping(from: .handle(list.handle, revision: list.revision), selected: .ranges(Self.ranges(selection)),
                           current: currentID, byIDs: selection.count <= 10_000 ? fallback : nil)
        }
        return Mapping(from: .mode(list.mode), selected: .ids(ids()), current: currentID)
    }

    /// Ascending, merged ranges of a set of indexes.
    nonisolated static func ranges(_ indexes: Set<Int>) -> [Range<UInt64>] {
        var ranges: [Range<UInt64>] = []
        for index in indexes.sorted() {
            let value = UInt64(index)
            if let last = ranges.last, last.upperBound == value {
                ranges[ranges.count - 1] = last.lowerBound..<(value + 1)
            } else {
                ranges.append(value..<(value + 1))
            }
        }
        return ranges
    }

    // MARK: Selection

    /// The table's selection changed: the row added alone becomes current;
    /// a current row no longer selected gives way to the first selected.
    public func select(_ indexes: Set<Int>) {
        guard indexes != selection else { return }
        let added = indexes.subtracting(selection)
        if added.count == 1 { currentIndex = added.first }
        else if let current = currentIndex, !indexes.contains(current) { currentIndex = indexes.min() }
        else if currentIndex == nil { currentIndex = indexes.min() }
        if indexes.isEmpty { currentIndex = nil }
        selection = indexes
        selectionRevision &+= 1
    }

    /// V7, R1153: Select All: every row of the results, rows not read yet
    /// included. The current row stays when it is one of them.
    public func selectAll() {
        guard let list, list.count > 0 else { return }
        select(Set(0..<list.count))
    }

    /// Selects the rows of `ranges` of the list shown at `revision` (Select
    /// Sibling Cards), keeping the current row when it is one of them;
    /// nothing changes when another list or revision shows now.
    func select(_ ranges: [Range<UInt64>], token: UInt64, revision: UInt64) {
        guard let list, list.token == token, list.revision == revision else { return }
        var rows = Set<Int>()
        for range in ranges {
            rows.formUnion(Int(clamping: range.lowerBound)..<Int(clamping: min(range.upperBound, UInt64(list.count))))
        }
        guard !rows.isEmpty else { return }
        select(rows)
    }

    /// The rows `indexes` (the selection when nil) as a command takes them
    /// (H12): nil when there are none, or when the list shown cannot take a
    /// command (a released handle: its rows show dimmed, R1108).
    public func target(_ indexes: Set<Int>? = nil) -> BrowseTarget? {
        guard let list, !list.released, !isDimmed else { return nil }
        let rows = (indexes ?? selection).filter { $0 >= 0 && $0 < list.count }
        guard !rows.isEmpty else { return nil }
        return BrowseTarget(handle: list.handle, revision: list.revision, ranges: Self.ranges(rows), mode: list.mode)
    }

    /// The session commands run in, once one is open.
    var commandSession: (any BrowseSearching)? { session }

    /// The card (Cards mode) and the note of the row at `index` of the list
    /// shown, from the rows read or else read for it (the inspector, U273).
    /// Nil when the row is gone, an error row, or the list moved on.
    func rowIDs(at index: Int) async -> (card: Int64?, note: Int64)? {
        guard let list, index >= 0, index < list.count else { return nil }
        if let row = row(at: index), row.status == .row, let id = row.id, let note = row.noteID {
            return (list.mode == .cards ? id : nil, note)
        }
        guard let session, !list.released else { return nil }
        let range = UInt64(index)..<UInt64(index + 1)
        guard let read = try? await session.rows(list.handle, revision: list.revision, range: range, columns: []),
              case .row(let row)? = read.first, self.list?.token == list.token else { return nil }
        return (list.mode == .cards ? row.id : nil, row.noteID)
    }

    /// Raised by every selection the user makes.
    var userSelections: UInt64 { selectionRevision }

    /// The Sort Field text of the row at `index` (V8.2's question), from
    /// the rows read when the list shows that column, else read for it.
    func sortFieldText(at index: Int, in target: BrowseTarget) async -> String? {
        // The row read, while it is still that row of that revision.
        if let list, list.handle == target.handle, list.revision == target.revision,
           let column = list.columns.firstIndex(of: "noteFld"), let row = row(at: index), row.cells.indices.contains(column),
           case .text(let text) = row.cells[column] {
            return text
        }
        guard let session else { return nil }
        let range = UInt64(index)..<UInt64(index + 1)
        guard let read = try? await session.rows(target.handle, revision: target.revision, range: range, columns: ["noteFld"]),
              case .row(let row)? = read.first, case .text(let text)? = row.cells.first else { return nil }
        return text
    }

    // MARK: Kept scroll (V1)

    /// The row at the middle of the table when Browse was left, in the list
    /// it belongs to: coming back shows it there again (V1).
    @ObservationIgnored private(set) var keptRow: (token: UInt64, index: Int)?
    /// The rows the table draws now.
    @ObservationIgnored private var onScreen: Set<Int> = []
    /// The table is on screen and has been brought back to its kept row:
    /// only then do the rows it draws move the kept row.
    @ObservationIgnored private var tableShown = false

    /// The table appeared: the row to bring back to its middle, if the list
    /// it shows is the one it left.
    public func tableAppeared() -> Int? {
        tableShown = false
        trace("appear kept=\(keptRow.map { "\($0.token):\($0.index)" } ?? "nil") list=\(list?.token ?? 0)")
        guard let kept = keptRow, kept.token == list?.token else { pendingRestore = nil; return nil }
        pendingRestore = kept.index > 0 ? kept.index : nil
        return kept.index
    }

    /// The last steps of the kept scroll (eight short lines), which Debug
    /// builds show to the UI suites.
    public private(set) var scrollTrace: [String] = []
    public func trace(_ step: @autoclosure () -> String) {
        scrollTrace = Array((scrollTrace + [step()]).suffix(8))
    }

    /// The header's last report of its columns (the UI suites read it).
    public private(set) var columnTrace = ""
    public func traceColumns(_ step: String) {
        if columnTrace != step { columnTrace = step }
    }

    /// The Mac: the kept row its table view brings back itself, once.
    @ObservationIgnored public private(set) var pendingRestore: Int?
    public func restored() { pendingRestore = nil }

    /// The table is back at its kept row (or had none): the rows it draws
    /// from now on are kept.
    public func tableSettled() {
        tableShown = true
    }

    /// The rows the table's scroll view shows, as it reports them (the Mac:
    /// its rows' appear and disappear events do not follow a scroll there).
    /// At its top, the table comes back unscrolled: the kept row is 0.
    public func tableScrolled(first: Int, last: Int, token: UInt64? = nil) {
        scrollReported = true
        // A table of an earlier list reports nothing about this one.
        guard tableShown, let list, last >= first, token.map({ $0 == list.token }) ?? true else {
            trace("ignored \(first)-\(last) shown=\(tableShown)")
            return
        }
        trace("kept \(first)-\(last)")
        keptRow = (list.token, first <= 0 ? 0 : (first + last) / 2)
    }
    @ObservationIgnored private var scrollReported = false

    public func tableDisappeared() {
        tableShown = false
        onScreen = []
    }

    /// A row left the screen.
    public func rowDisappeared(_ index: Int) {
        onScreen.remove(index)
    }

    /// Kept on appear only: when Browse is left every row disappears, and
    /// that must not move the row kept. The rows on screen are the run of
    /// drawn rows around the one that appeared: rows drawn before a jump
    /// whose disappearance has not come yet are not part of it.
    private func keepCenter(around index: Int) {
        guard !scrollReported, tableShown, let list, onScreen.contains(index) else { return }
        var low = index, high = index
        while onScreen.contains(low - 1) { low -= 1 }
        while onScreen.contains(high + 1) { high += 1 }
        // At its top, the table comes back unscrolled: centring a row there
        // drew the first row under the column header (owner's Mac, 2026-11-23).
        keptRow = (list.token, low == 0 ? 0 : (low + high) / 2)
    }

    // MARK: Rows

    /// The row at `index`, nil until its page is read.
    public func row(at index: Int) -> BrowseShownRow? {
        store.row(at: index)
    }

    /// A row came on screen: its page and the pages beside it are read (H7:
    /// the visible rows plus a page each side).
    public func rowAppeared(_ index: Int) {
        visibleIndex = index
        onScreen.insert(index)
        keepCenter(around: index)
        readNearRows()
    }

    /// Reads the pages nearest the rows on screen, two at a time; pages the
    /// table scrolled past before their turn are not read.
    private func readNearRows() {
        guard let list, !list.released, let session, list.count > 0 else { return }
        let page = visibleIndex / BrowseRowStore.pageSize
        let pages = (list.count - 1) / BrowseRowStore.pageSize
        for candidate in [page, page + 1, page - 1] where (0...pages).contains(candidate) {
            guard fetching.count < 2 else { break }
            if store.loaded(candidate, count: list.count) { fetchQA(candidate, list: list, session: session); continue }
            if fetching.contains(candidate) { continue }
            fetch(candidate, list: list, session: session)
        }
        if keptCenter != page {
            keptCenter = page
            store.evict(around: page)
        }
    }

    private func fetch(_ page: Int, list: List, session: any BrowseSearching) {
        fetching.insert(page)
        let start = page * BrowseRowStore.pageSize
        let range = UInt64(start)..<UInt64(min(list.count, start + BrowseRowStore.pageSize))
        let presenter = presenter, epoch = epoch, store = store, generation = store.page(page).generation
        Task {
            let rows = try? await Self.read(session, list.handle, list.revision, range, list.columns, list.mode, presenter)
            guard self.epoch == epoch, let shown = self.list, shown.token == list.token, self.store === store else { return }
            fetching.remove(page)
            // Rows read before the page was marked stale (a change, another day) are read again.
            guard store.page(page).generation == generation else { return readNearRows() }
            guard let rows, shown.revision == list.revision, shown.columns == list.columns else {
                // A prune or a column change came meanwhile: read again in the new revision.
                if shown.revision != list.revision || shown.columns != list.columns { return readNearRows() }
                // A failed read is tried again twice, a moment later, then waits for the rows to show again.
                let tries = retries[page, default: 0]
                guard tries < 2 else { return }
                retries[page] = tries + 1
                try? await Task.sleep(for: .milliseconds(250))
                guard self.list?.token == list.token else { return }
                return readNearRows()
            }
            retries[page] = nil
            store.page(page).set(rows)
            readNearRows()
        }
    }

    /// Question and Answer, when shown, are rendered for the read pages only
    /// (H7, R1242), after their other cells.
    private func fetchQA(_ page: Int, list: List, session: any BrowseSearching) {
        guard !qaFetching.contains(page), store.page(page).pendingQA else { return }
        let generation = store.page(page).generation, store = store
        let wantsQuestion = list.columns.contains("question"), wantsAnswer = list.columns.contains("answer")
        guard wantsQuestion || wantsAnswer else { return }
        qaFetching.insert(page)
        let which: BrowseQAWhich = wantsQuestion && wantsAnswer ? .both : wantsQuestion ? .question : .answer
        let start = page * BrowseRowStore.pageSize
        let range = UInt64(start)..<UInt64(min(list.count, start + BrowseRowStore.pageSize))
        let presenter = presenter, epoch = epoch, columns = list.columns
        Task {
            let cells = try? await Self.readQA(session, list.handle, list.revision, range, which, columns, presenter)
            // A read for a replaced store leaves the new store's entries alone.
            guard self.epoch == epoch, let shown = self.list, shown.token == list.token, self.store === store else { return }
            // Cells read for another layout or before the page changed do not land; the page's are read
            // again at once, with a fresh budget, whatever happened to this read.
            guard shown.revision == list.revision, shown.columns == columns, store.page(page).generation == generation else {
                qaFetching.remove(page)
                qaRetries[page] = nil
                return readNearRows()
            }
            guard let cells, let rows = store.page(page).rows, rows.count == cells.count else {
                // A failed read is tried again twice, a moment later, the page held meanwhile; then the
                // cells wait for the rows to show again.
                let tries = qaRetries[page, default: 0]
                guard tries < 2 else {
                    qaFetching.remove(page)
                    return
                }
                qaRetries[page] = tries + 1
                try? await Task.sleep(for: .milliseconds(250))
                guard self.list?.token == list.token, self.store === store else { return }
                qaFetching.remove(page)
                return readNearRows()
            }
            qaFetching.remove(page)
            qaRetries[page] = nil
            store.page(page).fill(zip(rows, cells).map { $0.filling($1) })
        }
    }

    @concurrent
    private nonisolated static func read(_ session: any BrowseSearching, _ handle: BrowseHandle, _ revision: UInt64,
                                         _ range: Range<UInt64>, _ columns: [String], _ mode: BrowseMode,
                                         _ presenter: any BrowseRowPresenting) async throws -> [BrowseShownRow] {
        guard !range.isEmpty else { return [] }
        return try await session.rows(handle, revision: revision, range: range, columns: columns).map {
            presenter.present($0, columns: columns, mode: mode)
        }
    }

    @concurrent
    private nonisolated static func readQA(_ session: any BrowseSearching, _ handle: BrowseHandle, _ revision: UInt64,
                                           _ range: Range<UInt64>, _ which: BrowseQAWhich, _ columns: [String],
                                           _ presenter: any BrowseRowPresenting) async throws -> [[Int: BrowseShownCell]] {
        try await session.qaCells(handle, revision: revision, range: range, which: which).map {
            presenter.present($0, columns: columns)
        }
    }

    // MARK: Change sets (H10)

    /// A committed change set: the shown rows it removes go, those it
    /// touches are read again; a request not shown yet keeps it for when it
    /// is.
    public func changesCommitted(_ changes: ChangeSet, seq: UInt64) {
        let sequenced = SequencedChangeSet(seq: seq, changes: changes)
        defer { restoreRows(changes) }
        if let newest, !newest.shown, !newest.ended { newest.changes.append(sequenced) }
        if changes.configKeys.contains(where: { $0.name == "meguriBrowserColumnSets" }) || !changes.notetypes.isEmpty
            || changes.wholeCollection {
            // The person's own column writes come back here: the set is read
            // again once the last of them is written, so an echo of an earlier
            // one never shows a list the person has already changed.
            if columnWritesPending > 0 {
                refreshAfterColumnWrites = true
            } else {
                columnSets = [:]
                refreshColumnSet()
            }
        }
        guard let list else { return }
        if list.released {
            // H12: the rows of a released list whose card or note is removed go.
            let removed = Set(list.mode == .cards
                ? changes.cards.filter { $0.change == .removed }.map(\.id)
                : changes.notes.filter { $0.change == .removed }.map(\.id))
            let indexes = store.indexes(of: removed)
            guard !indexes.isEmpty, var shown = self.list else { return }
            selection = Set(selection.compactMap { Self.shifted($0, removing: indexes) })
            currentIndex = currentIndex.flatMap { Self.shifted($0, removing: indexes) }
            shown.count -= indexes.count
            store = store.removing(indexes, count: shown.count)
            fetching = []
            qaFetching = []
            self.list = shown
            return
        }
        unapplied.append(sequenced)
        applyChanges()
    }

    private func applyChanges() {
        guard !applying, let list, !list.released, !unapplied.isEmpty, let session else { return }
        applying = true
        let changes = unapplied, epoch = epoch
        Task {
            var again = false
            defer {
                applying = false
                // Another list came meanwhile: its change sets are applied to it now.
                if again, !unapplied.isEmpty { applyChanges() }
            }
            let result: BrowseInvalidation
            do {
                result = try await session.invalidatedRows(list.handle, revision: list.revision, changes: changes)
            } catch BridgeError.browse(.unknownHandle) {
                guard self.epoch == epoch, var shown = self.list, shown.token == list.token else {
                    // Another list is shown: its queued change sets are applied to it.
                    if self.epoch == epoch { again = true }
                    return
                }
                shown.released = true
                self.list = shown
                return
            } catch {
                // Another list's queued change sets are applied to it; this list's wait for the next change.
                if self.list?.token != list.token { again = true }
                return
            }
            guard self.epoch == epoch, var shown = self.list, shown.token == list.token, shown.revision == list.revision
            else {
                again = true
                return
            }
            unapplied.removeFirst(min(changes.count, unapplied.count))
            if !result.removed.isEmpty {
                let removed = result.removed.map { Int(clamping: $0) }
                selection = Set(selection.compactMap { Self.shifted($0, removing: removed) })
                if let current = currentIndex {
                    currentIndex = Self.shifted(current, removing: removed)
                        ?? (Int(result.count) > 0 ? min(current - Self.below(current, removed), Int(result.count) - 1) : nil)
                    if let current = currentIndex, selection.isEmpty { selection = [current] }
                }
                store = store.removing(removed, count: Int(clamping: result.count))
                fetching = []
                qaFetching = []
            }
            shown.revision = result.revision
            shown.count = Int(clamping: result.count)
            shown.isComplete = shown.isComplete || result.isComplete
            switch result.refetch {
            case .all: store.markStaleAll()
            case .rows(let indexes): store.markStale(indexes.map { Int(clamping: $0) })
            }
            self.list = shown
            if shown.isComplete, newest?.shown == true { stopProgress() }
            readNearRows()
            again = true
        }
    }

    /// V8.3: Undo of a Delete brings its rows back. H10 never adds a row, so
    /// a change set that adds back an id a change set removed while this
    /// query showed runs the search again, with those rows selected.
    private func restoreRows(_ changes: ChangeSet) {
        // Card ids and note ids are separate spaces (H12): each is kept on its
        // own, so an undo after a Cards / Notes switch still finds its rows.
        if goneCards.count < Self.goneLimit { goneCards.formUnion(changes.cards.lazy.filter { $0.change == .removed }.map(\.id)) }
        if goneNotes.count < Self.goneLimit { goneNotes.formUnion(changes.notes.lazy.filter { $0.change == .removed }.map(\.id)) }
        let mode = list?.mode ?? lastMode
        let rows = mode == .cards ? changes.cards.map { ($0.id, $0.change) } : changes.notes.map { ($0.id, $0.change) }
        let gone = mode == .cards ? goneCards : goneNotes
        let back = rows.compactMap { $0.1 == .added && gone.contains($0.0) ? $0.0 : nil }
        guard !back.isEmpty else { return }
        if mode == .cards { goneCards.subtract(back) } else { goneNotes.subtract(back) }
        guard let list, !runPending, newest?.shown != false, ready() else { return }
        let request = Request(kind: .search, text: list.text, mode: list.mode, sort: list.sort, columns: list.columns,
                              queryIsEmpty: list.queryIsEmpty)
        request.selecting = back
        issue(request)
    }

    /// The ids kept for `restoreRows`, at most.
    static let goneLimit = 100_000

    /// The index `index` takes when the rows at `removed` (ascending) go;
    /// nil when it is one of them.
    nonisolated static func shifted(_ index: Int, removing removed: [Int]) -> Int? {
        var low = 0, high = removed.count
        while low < high {
            let mid = (low + high) / 2
            if removed[mid] < index { low = mid + 1 } else { high = mid }
        }
        if low < removed.count, removed[low] == index { return nil }
        return index - low
    }

    private static func below(_ index: Int, _ removed: [Int]) -> Int {
        removed.prefix { $0 < index }.count
    }

    // MARK: Column sets (V2.2)

    /// V2.2: the note type whose own set the query uses: a Note Type token
    /// with exactly one value, set to is.
    static func notetype(of query: FilterQuery) -> Int64? {
        for case .value(let values, false) in query.tokens {
            if values.count == 1, case .notetype(let id, _) = values[0] { return id }
        }
        return nil
    }

    private func columnSet(_ notetype: Int64?) async throws -> ColumnSetShown {
        if let set = columnSets[notetype] { return set }
        let epoch = epoch, columns = columnsRevision
        let set = try await backend.columnSet(notetype: notetype)
        // A read that began before the collection went, or before a column change, does not fill the cache.
        if self.epoch == epoch, columns == columnsRevision { columnSets[notetype] = set }
        return set
    }

    /// The set was written by another device or note types changed: the
    /// shown list takes its columns and its sort.
    private func refreshColumnSet() {
        guard let list, ready() else { return }
        let notetype = Self.notetype(of: lastQuery), epoch = epoch, revision = queryRevision, columns = columnsRevision
        Task {
            guard let set = try? await columnSet(notetype), self.epoch == epoch, revision == queryRevision else { return }
            // The person changed the columns while the set was read: their change shows, and is read back later.
            guard columns == columnsRevision else {
                if columnWritesPending > 0 { refreshAfterColumnWrites = true }
                return
            }
            columnSet = set
            guard let shown = self.list, shown.token == list.token else { return }
            if set.columns(shown.mode) != shown.columns { showColumns(set.columns(shown.mode)) }
            if set.sort(shown.mode) != shown.sort, newest?.shown != false { sortAgain(set.sort(shown.mode)) }
        }
    }

    private func sortAgain(_ sort: BrowseSort) {
        guard let list else { return }
        let kind: Request.Kind = list.isComplete && !list.released ? .resort(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: list.mode, sort: sort, columns: list.columns,
                      queryIsEmpty: list.queryIsEmpty))
    }

    /// Use Standard Columns (V2.2): the own set becomes the standard set's copy.
    public func useStandardColumns() {
        guard let set = columnSet, case .notetype(let id) = set.set, set.own else { return }
        let backend = backend
        columnsRevision &+= 1
        write {
            do { try await backend.useStandardColumns(notetype: id) } catch { self.refreshAfterColumnWrites = true }
        }
    }

    // MARK: Choosing columns (U272; V2.3, V2.4)

    /// The 19 columns of `browser/columns` C2, every one offered in both modes.
    public nonisolated static let catalog = ["noteFld", "question", "answer", "template", "deck", "cardDue", "cardEase",
                                             "cardIvl", "cardReps", "cardLapses", "noteCrt", "cardMod", "noteMod", "note",
                                             "noteTags", "originalPosition", "stability", "difficulty", "retrievability"]

    /// V2.3's default standard set, when nothing is stored.
    public nonisolated static func defaultColumns(_ mode: BrowseMode) -> [String] {
        mode == .cards ? ["noteFld", "template", "deck", "cardDue", "cardMod"]
            : ["noteFld", "note", "template", "noteTags", "noteCrt"]
    }

    /// The mode the header shows (a run being read takes the last mode chosen).
    public var columnsMode: BrowseMode { runPending ? lastMode : list?.mode ?? lastMode }

    /// The columns the header shows, in order (V2.3).
    public var shownColumns: [String] {
        if let list, list.mode == columnsMode { return list.columns }
        return columnSet?.columns(columnsMode) ?? []
    }

    /// The set's own name for its widths on this device (K11): per set and mode.
    public var widthsKey: String {
        // A note type's set that is not its own yet is the standard set, live (V2.2).
        let set = switch columnSet.flatMap({ $0.own ? $0.set : nil }) ?? .standard {
        case .standard: "std"
        case .notetype(let id): String(id)
        }
        return "\(set)/\(columnsMode == .cards ? "cards" : "notes")"
    }

    /// Checking a column in the header's menu adds it at the end (V2.3).
    public func showColumn(_ key: String) {
        let columns = shownColumns
        guard !columns.contains(key), Self.offered(key) else { return }
        changeColumns(columns + [key], writes: [.show(key)])
    }

    /// Unchecking it removes it; the last column stays (R1133).
    public func hideColumn(_ key: String) {
        let columns = shownColumns
        guard columns.contains(key), columns.count > 1 else { return }
        changeColumns(columns.filter { $0 != key }, writes: [.hide(key)])
    }

    /// The header's menu: checks or unchecks `key`.
    public func toggleColumn(_ key: String) {
        shownColumns.contains(key) ? hideColumn(key) : showColumn(key)
    }

    /// Dragging a header reorders the list (V2.3, R1135): every column once.
    public func moveColumns(_ keys: [String]) {
        let columns = shownColumns
        guard keys != columns, keys.count == columns.count, Set(keys) == Set(columns) else { return }
        changeColumns(keys, writes: [.reorder(keys)])
    }

    /// Whether the set shown can be reset: a note type's own set (Use
    /// Standard Columns), or a standard set that differs from V2.3's default
    /// columns, or one with widths of its own.
    public var canResetColumns: Bool {
        guard let set = columnSet else { return false }
        if case .notetype = set.set { return set.own }
        _ = widthsEdits
        return shownColumns != Self.defaultColumns(columnsMode) || !(columnWidths[widthsKey]?.isEmpty ?? true)
    }

    /// The header's "Reset to Standard Columns": in a note type's own set,
    /// Use Standard Columns (V2.2); in the standard set, V2.3's default
    /// columns for the mode shown, in their order, and the widths of this
    /// device. The sort stays.
    public func resetColumns() {
        guard let set = columnSet else { return }
        if case .notetype = set.set {
            useStandardColumns()
            return
        }
        columnWidths[widthsKey] = nil
        storeWidths(columnWidths)
        widthsRevision &+= 1
        widthsEdits &+= 1
        let columns = shownColumns, defaults = Self.defaultColumns(columnsMode)
        guard columns != defaults else { return }
        // Shown first, then hidden, so the list never empties; then the order.
        var writes = defaults.filter { !columns.contains($0) }.map { ColumnSetChange.show($0) }
        writes += columns.filter { !defaults.contains($0) }.map { ColumnSetChange.hide($0) }
        writes.append(.reorder(defaults))
        changeColumns(defaults, writes: writes)
    }

    /// The catalog's columns and field columns; nothing else is ever shown (R1137).
    nonisolated static func offered(_ key: String) -> Bool {
        catalog.contains(key) || (key.hasPrefix("fld:") && key.count > 4)
    }

    /// Shows `columns` at once (the rows read keep the cells they have; a
    /// column added draws nothing until its rows are read again) and writes
    /// the change to the set, with no undo step (V2.4).
    private func changeColumns(_ columns: [String], writes: [ColumnSetChange]) {
        // While a query's run text is read, its set is not known yet: nothing is changed.
        guard let set = columnSet, ready(), !runPending else { return }
        let mode = columnsMode
        columnsRevision &+= 1
        // The first change to a note type's set makes it its own (V2.2).
        let next = set.with(columns, mode: mode)
        columnSet = next
        columnSets[Self.notetype(of: lastQuery)] = next
        if let list, list.mode == mode { showColumns(columns) }
        let backend = backend, target = set.set
        write {
            do {
                for change in writes { try await backend.changeColumnSet(target, mode: mode, change: change) }
            } catch {
                // Not written: the set is read back, so the table shows what the collection holds.
                self.refreshAfterColumnWrites = true
            }
        }
    }

    /// The shown list takes `columns`: the rows read are mapped to them, and
    /// read again when a column was added.
    private func showColumns(_ columns: [String]) {
        guard var shown = list, shown.columns != columns else { return }
        let old = shown.columns
        let map = columns.map { old.firstIndex(of: $0) }
        store.remap(map)
        shown.columns = columns
        list = shown
        if map.contains(nil) {
            store.markStaleAll()
            readNearRows()
        }
    }

    /// The column writes, one after another; the set is read again after the last.
    private func write(_ body: @escaping @MainActor () async -> Void) {
        columnWritesPending += 1
        let previous = columnWrite, epoch = epoch
        columnWrite = Task {
            await previous?.value
            // A write queued before the collection went never reaches the next one.
            if self.epoch == epoch { await body() }
            guard self.epoch == epoch else { return }
            columnWritesPending -= 1
            if columnWritesPending == 0, refreshAfterColumnWrites {
                refreshAfterColumnWrites = false
                columnSets = [:]
                refreshColumnSet()
            }
        }
    }

    /// The column writes under way (tests).
    func columnWritesSettled() async { await columnWrite?.value }

    @ObservationIgnored private var columnWrite: Task<Void, Never>?
    @ObservationIgnored private var columnWritesPending = 0
    @ObservationIgnored private var refreshAfterColumnWrites = false
    /// Raised by every column change the person makes: a set read before it does not show.
    @ObservationIgnored private var columnsRevision: UInt64 = 0

    // MARK: Widths (V2.3, K11: per device, per set and per mode)

    /// The widths the person set, by `widthsKey`, then column key; points.
    @ObservationIgnored public private(set) var columnWidths: [String: [String: Double]] = [:]
    /// Raised when widths are reset: the table lays its columns out again.
    public private(set) var widthsRevision: UInt64 = 0
    /// Keeps the widths on the device (`BrowseDeviceState.columnWidths`).
    @ObservationIgnored var storeWidths: ([String: [String: Double]]) -> Void = { _ in }

    /// The width the person gave `key` in the set and mode shown.
    public func width(of key: String) -> Double? { columnWidths[widthsKey]?[key] }

    /// Raised when the person sets a width (Reset's enabled state follows it).
    private var widthsEdits: UInt64 = 0

    /// Dragging a header's edge (or Size Columns to Fit) set these widths, in
    /// the set and mode shown, or in `set` (a `widthsKey`).
    public func setWidths(_ widths: [String: Double], in set: String? = nil) {
        let set = set ?? widthsKey
        var current = columnWidths[set] ?? [:]
        for (key, width) in widths where width.isFinite && width > 0 { current[key] = (width * 2).rounded() / 2 }
        guard current != columnWidths[set] else { return }
        columnWidths[set] = current
        widthsEdits &+= 1
        storeWidths(columnWidths)
    }

    /// The widths kept on the device, at launch.
    func restoreWidths(_ widths: [String: [String: Double]]) { columnWidths = widths }

    // MARK: Counts and progress

    /// V11's t, counted with the sidebar's counts.
    public func setTotal(_ total: UInt64?, mode: BrowseMode) {
        if self.total != total { self.total = total }
        if totalMode != mode { totalMode = mode }
    }

    /// The mode `total` counts.
    public private(set) var totalMode: BrowseMode = .cards

    private func startProgress() {
        progressTask?.cancel()
        let delay = progressDelay
        progressTask = Task {
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            if isSearching { showsProgress = true }
        }
    }

    private func stopProgress() {
        progressTask?.cancel()
        progressTask = nil
        if showsProgress { showsProgress = false }
    }

    // MARK: The collection

    /// The collection is ready in a new generation: the old handles are
    /// gone (H6); the rows read stay until the query's new rows arrive.
    public func collectionReady() {
        epoch &+= 1
        resetRequests()
        if var shown = list {
            shown.released = true
            list = shown
        }
        columnSets = [:]
        runAgain()
    }

    /// The collection was replaced: nothing of the old one stays.
    public func collectionReplaced() {
        epoch &+= 1
        resetRequests()
        list = nil
        store = BrowseRowStore()
        selection = []
        currentIndex = nil
        columnSets = [:]
        columnSet = nil
        total = nil
        failed = false
        rejected = false
        hasRun = false
        goneCards = []
        goneNotes = []
        keptRow = nil
    }

    private func resetRequests() {
        columnWritesPending = 0
        refreshAfterColumnWrites = false
        newest = nil
        queued = nil
        unapplied = []
        fetching = []
        qaFetching = []
        stopProgress()
    }

    @ObservationIgnored private var presenterKey: String?

    /// The feature's presenter; a new key (another locale, time zone or
    /// day) formats the rows read again.
    public func setPresenter(_ presenter: any BrowseRowPresenting, key: String) {
        guard key != presenterKey else { return }
        let first = presenterKey == nil && list == nil
        presenterKey = key
        self.presenter = presenter
        if !first { dayChanged() }
    }

    /// The day changed: dates read "Today" and "Tomorrow" from another day.
    public func dayChanged() {
        store.markStaleAll()
        readNearRows()
    }

    // MARK: Test hooks

    /// Brings `index` into view, as a jump of the scroll position (the
    /// performance tests' scrolling, U270a).
    @_spi(MeguriTestHooks) public func scroll(to index: Int) {
        bringIntoView(index)
    }

    private func bringIntoView(_ index: Int) {
        guard let list else { return }
        scrollSerial &+= 1
        scrollTarget = ScrollTarget(token: list.token, index: min(max(index, 0), max(list.count - 1, 0)), serial: scrollSerial)
    }
}

/// The rows read for one list, by page of 200 (`browser/result-sets` H1).
/// A page keeps its object for the list's life, so a cell that drew it
/// draws it again when its rows arrive; only its rows are let go.
@MainActor
public final class BrowseRowStore {
    public static let pageSize = 200
    /// Pages kept beyond this many are let go, farthest first.
    static let keptPages = 24

    private var pages: [Int: BrowseRowPage] = [:]

    public init() {}

    public func page(_ number: Int) -> BrowseRowPage {
        if let page = pages[number] { return page }
        let page = BrowseRowPage()
        pages[number] = page
        return page
    }

    public func row(at index: Int) -> BrowseShownRow? {
        guard index >= 0 else { return nil }
        let rows = page(index / Self.pageSize).rows
        let offset = index % Self.pageSize
        guard let rows, rows.indices.contains(offset) else { return nil }
        return rows[offset]
    }

    /// Read, and not marked stale since.
    /// Read, not marked stale since, and holding every row the page has in
    /// a list of `count` rows (a streaming list's last page grows).
    func loaded(_ number: Int, count: Int) -> Bool {
        guard let page = pages[number], let rows = page.rows, !page.stale else { return false }
        return rows.count == min(Self.pageSize, count - number * Self.pageSize)
    }

    /// The rows stay on screen until they are read again (no flash).
    func markStaleAll() {
        for page in pages.values where page.rows != nil { page.markStale() }
    }

    func markStale(_ indexes: [Int]) {
        for number in Set(indexes.map { $0 / Self.pageSize }) { pages[number]?.markStale() }
    }

    func evict(around center: Int) {
        let loaded = pages.filter { $0.value.rows != nil }
        guard loaded.count > Self.keptPages else { return }
        for (number, page) in loaded.sorted(by: { abs($0.key - center) > abs($1.key - center) })
            .prefix(loaded.count - Self.keptPages) where abs(number - center) > 2 {
            page.letGo()
        }
    }

    /// The rows read take another column list: `map` gives, for each new
    /// column, its index in the old list, or nil for one added.
    func remap(_ map: [Int?]) {
        for page in pages.values { page.remap(map) }
    }

    /// The indexes of the read rows whose id is in `ids`, ascending.
    func indexes(of ids: Set<Int64>) -> [Int] {
        var found: [Int] = []
        for (number, page) in pages {
            for (offset, row) in (page.rows ?? []).enumerated() where row.id.map(ids.contains) ?? false {
                found.append(number * Self.pageSize + offset)
            }
        }
        return found.sorted()
    }

    /// The store after the rows at `removed` (ascending, read or not) go,
    /// for a list of `count` rows. A page comes back read only when every
    /// row it holds in the new list is known.
    func removing(_ removed: [Int], count: Int) -> BrowseRowStore {
        let next = BrowseRowStore()
        var moved: [Int: [Int: BrowseShownRow]] = [:]
        var stale: Set<Int> = []
        for (number, page) in pages {
            guard let rows = page.rows else { continue }
            for (offset, row) in rows.enumerated() {
                guard let index = BrowseResults.shifted(number * Self.pageSize + offset, removing: removed) else { continue }
                moved[index / Self.pageSize, default: [:]][index % Self.pageSize] = row
                if page.stale { stale.insert(index / Self.pageSize) }
            }
        }
        for (number, rows) in moved {
            let expected = min(Self.pageSize, count - number * Self.pageSize)
            guard expected > 0, rows.count == expected, (0..<expected).allSatisfy({ rows[$0] != nil }) else { continue }
            next.page(number).set((0..<expected).compactMap { rows[$0] })
            if stale.contains(number) { next.page(number).markStale() }
        }
        return next
    }
}

/// One page of rows; cells observe it alone.
@Observable @MainActor
public final class BrowseRowPage {
    /// Nil until read (or after it is let go).
    public private(set) var rows: [BrowseShownRow]?
    /// The rows shown are older than a change: they are read again.
    @ObservationIgnored private(set) var stale = false
    /// Raised by every mark: a read begun before it does not land.
    @ObservationIgnored private(set) var generation: UInt64 = 0
    /// Its Question and Answer cells are still to be read.
    @ObservationIgnored var pendingQA = true

    /// New rows: their Question and Answer are read again, and a read of
    /// those begun before does not land.
    func set(_ rows: [BrowseShownRow]) {
        self.rows = rows
        stale = false
        pendingQA = true
        generation &+= 1
    }

    /// The same rows with their Question and Answer cells.
    func fill(_ rows: [BrowseShownRow]) {
        self.rows = rows
        pendingQA = false
    }

    /// The same rows with their cells in another column order; a column
    /// added draws nothing until the rows are read again.
    func remap(_ map: [Int?]) {
        guard let rows else { return }
        self.rows = rows.map { $0.remapped(map) }
    }

    func markStale() {
        stale = true
        pendingQA = true
        generation &+= 1
    }

    func letGo() {
        rows = nil
        stale = false
        pendingQA = true
        generation &+= 1
    }
}

extension ColumnSetShown {
    nonisolated var notetypeKey: Int64? {
        if case .notetype(let id) = set { return id }
        return nil
    }

    /// The set with `columns` in `mode`; a note type's set becomes its own (V2.2).
    nonisolated func with(_ columns: [String], mode: BrowseMode) -> ColumnSetShown {
        ColumnSetShown(set: set, own: own || set != .standard, cards: mode == .cards ? columns : cards,
                       notes: mode == .notes ? columns : notes, cardsSort: cardsSort, notesSort: notesSort)
    }

    nonisolated func sorted(_ sort: BrowseSort, mode: BrowseMode) -> ColumnSetShown {
        ColumnSetShown(set: set, own: own, cards: cards, notes: notes, cardsSort: mode == .cards ? sort : cardsSort,
                       notesSort: mode == .notes ? sort : notesSort)
    }
}

/// What the status line says (`browser/workspace` V11): the rows shown of
/// the collection's total, "Searching…" while a search runs past half a
/// second, and the number selected.
public nonisolated struct BrowseStatus: Equatable, Sendable {
    /// r: the rows of the results (known so far while streaming).
    public let shown: Int
    /// t: the collection's cards or notes; nil until counted.
    public let total: UInt64?
    public let mode: BrowseMode
    /// The empty query: the text is "<t> cards".
    public let wholeCollection: Bool
    public let searching: Bool
    public let selected: Int

    public init(shown: Int, total: UInt64?, mode: BrowseMode, wholeCollection: Bool, searching: Bool, selected: Int) {
        self.shown = shown
        self.total = total
        self.mode = mode
        self.wholeCollection = wholeCollection
        self.searching = searching
        self.selected = selected
    }

    @MainActor public init(results: BrowseResults, query: FilterQuery) {
        let mode = results.list?.mode ?? .cards
        self.init(shown: results.list?.count ?? 0, total: results.totalMode == mode ? results.total : nil, mode: mode,
                  wholeCollection: results.list?.queryIsEmpty ?? query.isEmpty, searching: results.showsProgress,
                  selected: results.selection.count)
    }
}
