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

    public private(set) var list: List?
    /// The rows read, by page; replaced with the list, so every cell draws again.
    public private(set) var store = BrowseRowStore()
    /// Indexes of `list`'s rows (R1152).
    public private(set) var selection: Set<Int> = []
    /// The current row (R1152, H12): the row last chosen.
    public private(set) var currentIndex: Int?
    /// A search has run for half a second without its rows shown or whole.
    public private(set) var showsProgress = false
    /// The latest search failed for a reason other than its text.
    public private(set) var failed = false
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
    @ObservationIgnored private var scrollSerial: UInt64 = 0

    public var content: Content {
        if failed { return .failed }
        guard let list else { return .starting }
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
        /// What to map the selection from when its list is shown (H12).
        let mapping: Mapping?
        var seq: UInt64?
        var handle: BrowseHandle?
        var count = 0
        var revision: UInt64 = 0
        var complete = false
        var shown = false
        var ended = false
        /// Change sets received since it started (H10).
        var changes: [SequencedChangeSet] = []

        init(kind: Kind, text: String, mode: BrowseMode, sort: BrowseSort, columns: [String], queryIsEmpty: Bool,
             mapping: Mapping?) {
            self.kind = kind
            self.text = text
            self.mode = mode
            self.sort = sort
            self.columns = columns
            self.queryIsEmpty = queryIsEmpty
            self.mapping = mapping
        }
    }

    private struct Mapping {
        let from: BrowseMapSource
        let selected: BrowseSelected
        let current: Int64?
    }

    @ObservationIgnored private var session: (any BrowseSearching)?
    @ObservationIgnored private var opening: Task<(any BrowseSearching)?, Never>?
    @ObservationIgnored private var eventsTask: Task<Void, Never>?
    /// The newest request, shown or not.
    private var newest: Request?
    /// A start call is on its way; the next request waits for it.
    @ObservationIgnored private var starting = false
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

    public init(backend: any BrowseBackend) {
        self.backend = backend
    }

    // MARK: Running the query

    /// A query or mode to show: its run text (T4) searched with the column
    /// set's sort for the mode (V2.2, V4.3).
    public func run(_ query: FilterQuery, mode: BrowseMode) {
        lastQuery = query
        lastMode = mode
        hasRun = true
        failed = false
        queryRevision &+= 1
        guard ready() else { return }
        let revision = queryRevision, epoch = epoch, backend = backend
        let notetype = Self.notetype(of: query)
        Task {
            do {
                async let text = backend.runText(query)
                let set = try await columnSet(notetype)
                let runText = try await text
                guard revision == queryRevision, self.epoch == epoch else { return }
                columnSet = set
                issue(Request(kind: .search, text: runText, mode: mode, sort: set.sort(mode), columns: set.columns(mode),
                              queryIsEmpty: query.isEmpty, mapping: mapping()))
            } catch {
                guard revision == queryRevision, self.epoch == epoch else { return }
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
        lastMode = mode
        lastQuery = query
        guard let list, let set = columnSet, list.mode != mode, ready() else { return run(query, mode: mode) }
        let kind: Request.Kind = list.isComplete && !list.released ? .toggle(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: mode, sort: set.sort(mode), columns: set.columns(mode),
                      queryIsEmpty: list.queryIsEmpty, mapping: mapping()))
    }

    /// V4.1: a header click: the column in its first direction, or the
    /// order shown reversed, as a resort of the shown handle (H8) and a
    /// write of the set's sort (V4.3, no undo step).
    public func sort(by key: String) {
        guard let list, BrowseColumns.sortable(key, mode: list.mode), ready() else { return }
        let shown = newest?.mode == list.mode ? (newest?.sort ?? list.sort) : list.sort
        guard let sort = BrowseColumns.click(key, current: shown) else { return }
        if let set = columnSet {
            let backend = backend, mode = list.mode
            Task { try? await backend.changeColumnSet(set.set, mode: mode, change: .sortBy(key)) }
            columnSet = set.sorted(sort, mode: mode)
            columnSets[set.notetypeKey] = columnSet
        }
        let kind: Request.Kind = list.isComplete && !list.released ? .resort(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: list.mode, sort: sort, columns: list.columns,
                      queryIsEmpty: list.queryIsEmpty, mapping: mapping()))
    }

    private func issue(_ request: Request) {
        // The shown list's request, still streaming, is superseded: its handle goes (H6).
        if var shown = list, !shown.isComplete, newest?.shown == true {
            shown.released = true
            list = shown
        }
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
        let epoch = epoch
        Task {
            defer {
                starting = false
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
                if request.seq == nil { request.seq = seq }
            } catch BridgeError.browse(.notComplete) {
                // H8, H11: the handle is not whole yet: the search runs again with the new sort or mode.
                guard request === newest, self.epoch == epoch else { return }
                let again = Request(kind: .search, text: request.text, mode: request.mode, sort: request.sort,
                                    columns: request.columns, queryIsEmpty: request.queryIsEmpty, mapping: request.mapping)
                newest = again
                queued = again
            } catch BridgeError.browse(.unknownHandle) {
                guard request === newest, self.epoch == epoch else { return }
                let again = Request(kind: .search, text: request.text, mode: request.mode, sort: request.sort,
                                    columns: request.columns, queryIsEmpty: request.queryIsEmpty, mapping: nil)
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

    private func fail() {
        failed = true
        stopProgress()
    }

    // MARK: Events (H4)

    /// The request an event belongs to: the newest one, when the event is
    /// newer than every seq returned so far (its start has not returned) or
    /// carries its seq.
    private func request(for seq: UInt64) -> Request? {
        guard let newest, !newest.ended else { return nil }
        if let own = newest.seq { return own == seq ? newest : nil }
        if seq > lastReturned {
            newest.seq = seq
            return newest
        }
        return nil
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
            // T5: a search the text cannot run keeps the results shown.
            if case .browse(.search) = error {
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
        Task {
            async let rows = Self.read(session, handle, revision, first, columns, mode, presenter)
            async let mapped = mapping(request, in: session, to: handle, revision: revision)
            let (page, selected) = await (try? rows, mapped)
            guard request === newest, !request.ended, self.epoch == epoch else { return }
            tokens &+= 1
            let next = List(token: tokens, handle: handle, mode: request.mode, sort: request.sort, columns: request.columns,
                            count: request.count, revision: request.revision, isComplete: request.complete, released: false,
                            text: request.text, queryIsEmpty: request.queryIsEmpty)
            let store = BrowseRowStore()
            if let page { store.page(0).rows = page }
            request.shown = true
            failed = false
            self.store = store
            list = next
            fetching = []
            qaFetching = []
            unapplied = request.changes
            apply(selected, count: next.count, mapped: request.mapping != nil)
            if let current = currentIndex, current > 0 { bringIntoView(current) }
            if request.complete { stopProgress() }
            if !unapplied.isEmpty { applyChanges() }
            readNearRows()
        }
    }

    /// A mapping made when the new list was not whole yet is made again once
    /// it is (H12), unless the user chose rows meanwhile.
    private func remap(_ request: Request) {
        guard request.mapping != nil, let handle = request.handle, let session else { return }
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
        return try? await session.mapSelection(from: mapping.from, selected: mapping.selected, current: mapping.current,
                                               to: handle, targetRevision: revision)
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
        if list.isComplete, !list.released {
            return Mapping(from: .handle(list.handle, revision: list.revision), selected: .ranges(Self.ranges(selection)),
                           current: currentID)
        }
        let ids = selection.sorted().compactMap { row(at: $0)?.id }
        return Mapping(from: .mode(list.mode), selected: .ids(ids), current: currentID)
    }

    /// Ascending, merged ranges of a set of indexes.
    static func ranges(_ indexes: Set<Int>) -> [Range<UInt64>] {
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

    // MARK: Rows

    /// The row at `index`, nil until its page is read.
    public func row(at index: Int) -> BrowseShownRow? {
        store.row(at: index)
    }

    /// A row came on screen: its page and the pages beside it are read (H7:
    /// the visible rows plus a page each side).
    public func rowAppeared(_ index: Int) {
        visibleIndex = index
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
            if store.loaded(candidate) { fetchQA(candidate, list: list, session: session); continue }
            if fetching.contains(candidate) { continue }
            fetch(candidate, list: list, session: session)
        }
        store.evict(around: page)
    }

    private func fetch(_ page: Int, list: List, session: any BrowseSearching) {
        fetching.insert(page)
        let start = page * BrowseRowStore.pageSize
        let range = UInt64(start)..<UInt64(min(list.count, start + BrowseRowStore.pageSize))
        let presenter = presenter, epoch = epoch
        Task {
            let rows = try? await Self.read(session, list.handle, list.revision, range, list.columns, list.mode, presenter)
            guard self.epoch == epoch, let shown = self.list, shown.token == list.token else { return }
            fetching.remove(page)
            guard let rows, shown.revision == list.revision, shown.columns == list.columns else {
                // A prune or a column change came meanwhile: read again in the new revision.
                if shown.revision != list.revision || shown.columns != list.columns { readNearRows() }
                return
            }
            store.page(page).rows = rows
            readNearRows()
        }
    }

    /// Question and Answer, when shown, are rendered for the read pages only
    /// (H7, R1242), after their other cells.
    private func fetchQA(_ page: Int, list: List, session: any BrowseSearching) {
        guard !qaFetching.contains(page), store.page(page).pendingQA else { return }
        let wantsQuestion = list.columns.contains("question"), wantsAnswer = list.columns.contains("answer")
        guard wantsQuestion || wantsAnswer else { return }
        qaFetching.insert(page)
        let which: BrowseQAWhich = wantsQuestion && wantsAnswer ? .both : wantsQuestion ? .question : .answer
        let start = page * BrowseRowStore.pageSize
        let range = UInt64(start)..<UInt64(min(list.count, start + BrowseRowStore.pageSize))
        let presenter = presenter, epoch = epoch, columns = list.columns
        Task {
            let cells = try? await Self.readQA(session, list.handle, list.revision, range, which, columns, presenter)
            guard self.epoch == epoch, let shown = self.list, shown.token == list.token else { return }
            qaFetching.remove(page)
            guard let cells, shown.revision == list.revision, let rows = store.page(page).rows else { return }
            store.page(page).rows = zip(rows, cells).map { $0.filling($1) }
            store.page(page).pendingQA = false
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
        if let newest, !newest.shown, !newest.ended { newest.changes.append(sequenced) }
        if changes.configKeys.contains(where: { $0.name == "meguriBrowserColumnSets" }) || !changes.notetypes.isEmpty
            || changes.wholeCollection {
            columnSets = [:]
            refreshColumnSet()
        }
        guard let list else { return }
        if list.released {
            // H12: the rows of a released list whose card or note is removed go.
            let removed = Set(list.mode == .cards
                ? changes.cards.filter { $0.change == .removed }.map(\.id)
                : changes.notes.filter { $0.change == .removed }.map(\.id))
            if !removed.isEmpty { store.markDeleted(removed) }
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
            defer { applying = false }
            let result: BrowseInvalidation
            do {
                result = try await session.invalidatedRows(list.handle, revision: list.revision, changes: changes)
            } catch BridgeError.browse(.unknownHandle) {
                guard self.epoch == epoch, var shown = self.list, shown.token == list.token else { return }
                shown.released = true
                self.list = shown
                return
            } catch {
                return
            }
            guard self.epoch == epoch, var shown = self.list, shown.token == list.token, shown.revision == list.revision
            else { return }
            unapplied.removeFirst(min(changes.count, unapplied.count))
            if !result.removed.isEmpty {
                let removed = result.removed.map { Int(clamping: $0) }
                selection = Set(selection.compactMap { Self.shifted($0, removing: removed) })
                if let current = currentIndex {
                    currentIndex = Self.shifted(current, removing: removed)
                        ?? (Int(result.count) > 0 ? min(current - Self.below(current, removed), Int(result.count) - 1) : nil)
                    if let current = currentIndex, selection.isEmpty { selection = [current] }
                }
                store = store.removing(removed)
            }
            shown.revision = result.revision
            shown.count = Int(clamping: result.count)
            shown.isComplete = shown.isComplete || result.isComplete
            switch result.refetch {
            case .all: store.unloadAll()
            case .rows(let indexes): store.unload(indexes.map { Int(clamping: $0) })
            }
            self.list = shown
            if shown.isComplete, newest?.shown == true { stopProgress() }
            readNearRows()
            if !unapplied.isEmpty { applyChanges() }
        }
    }

    /// The index `index` takes when the rows at `removed` (ascending) go;
    /// nil when it is one of them.
    static func shifted(_ index: Int, removing removed: [Int]) -> Int? {
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
        let set = try await backend.columnSet(notetype: notetype)
        columnSets[notetype] = set
        return set
    }

    /// The set was written by another device or note types changed: the
    /// shown list takes its columns and its sort.
    private func refreshColumnSet() {
        guard let list, ready() else { return }
        let notetype = Self.notetype(of: lastQuery), epoch = epoch, revision = queryRevision
        Task {
            guard let set = try? await columnSet(notetype), self.epoch == epoch, revision == queryRevision else { return }
            columnSet = set
            guard var shown = self.list, shown.token == list.token else { return }
            if set.columns(shown.mode) != shown.columns {
                shown.columns = set.columns(shown.mode)
                self.list = shown
                store = BrowseRowStore()
                fetching = []
                qaFetching = []
                readNearRows()
            }
            if set.sort(shown.mode) != shown.sort, newest?.shown != false { sortAgain(set.sort(shown.mode)) }
        }
    }

    private func sortAgain(_ sort: BrowseSort) {
        guard let list else { return }
        let kind: Request.Kind = list.isComplete && !list.released ? .resort(list.handle) : .search
        issue(Request(kind: kind, text: list.text, mode: list.mode, sort: sort, columns: list.columns,
                      queryIsEmpty: list.queryIsEmpty, mapping: mapping()))
    }

    /// Use Standard Columns (V2.2): the own set becomes the standard set's copy.
    public func useStandardColumns() {
        guard let set = columnSet, case .notetype(let id) = set.set else { return }
        let backend = backend
        Task { try? await backend.useStandardColumns(notetype: id) }
    }

    // MARK: Counts and progress

    /// V11's t, counted with the sidebar's counts.
    public func setTotal(_ total: UInt64?) {
        if self.total != total { self.total = total }
    }

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
        hasRun = false
    }

    private func resetRequests() {
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
        store.unloadAll()
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

    func loaded(_ number: Int) -> Bool { pages[number]?.rows != nil }

    func unloadAll() {
        for page in pages.values where page.rows != nil {
            page.rows = nil
            page.pendingQA = true
        }
    }

    func unload(_ indexes: [Int]) {
        for number in Set(indexes.map { $0 / Self.pageSize }) {
            pages[number]?.rows = nil
            pages[number]?.pendingQA = true
        }
    }

    func evict(around center: Int) {
        let loaded = pages.filter { $0.value.rows != nil }
        guard loaded.count > Self.keptPages else { return }
        for (number, page) in loaded.sorted(by: { abs($0.key - center) > abs($1.key - center) })
            .prefix(loaded.count - Self.keptPages) where abs(number - center) > 2 {
            page.rows = nil
            page.pendingQA = true
        }
    }

    /// The store after the rows at `removed` (ascending, read or not) go.
    func removing(_ removed: [Int]) -> BrowseRowStore {
        let next = BrowseRowStore()
        var moved: [Int: [Int: BrowseShownRow]] = [:]
        for (number, page) in pages {
            guard let rows = page.rows else { continue }
            for (offset, row) in rows.enumerated() {
                guard let index = BrowseResults.shifted(number * Self.pageSize + offset, removing: removed) else { continue }
                moved[index / Self.pageSize, default: [:]][index % Self.pageSize] = row
            }
        }
        // Only pages whose every row is known come back read.
        for (number, rows) in moved where rows.count == Self.pageSize || rows.keys.max() == rows.count - 1 {
            next.page(number).rows = (0..<rows.count).compactMap { rows[$0] }
        }
        return next
    }

    func markDeleted(_ ids: Set<Int64>) {
        for page in pages.values {
            guard let rows = page.rows, rows.contains(where: { $0.id.map(ids.contains) ?? false }) else { continue }
            page.rows = rows.map { row in
                guard let id = row.id, ids.contains(id) else { return row }
                return BrowseShownRow(status: .deleted, id: nil, noteID: nil, state: nil, font: nil,
                                      sortFieldRightToLeft: false, cells: row.cells.map { _ in .empty })
            }
        }
    }
}

/// One page of rows; cells observe it alone.
@Observable @MainActor
public final class BrowseRowPage {
    /// Nil until read (or after it is let go).
    public internal(set) var rows: [BrowseShownRow]?
    /// Its Question and Answer cells are still to be read.
    @ObservationIgnored var pendingQA = true
}

extension ColumnSetShown {
    nonisolated var notetypeKey: Int64? {
        if case .notetype(let id) = set { return id }
        return nil
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
        self.init(shown: results.list?.count ?? 0, total: results.total, mode: results.list?.mode ?? .cards,
                  wholeCollection: results.list?.queryIsEmpty ?? query.isEmpty, searching: results.showsProgress,
                  selected: results.selection.count)
    }
}
