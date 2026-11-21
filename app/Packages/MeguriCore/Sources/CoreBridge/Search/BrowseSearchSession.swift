internal import MeguriCoreBindings
import Instrumentation
import Synchronization

// Browse search sessions (U264, `browser/result-sets` H2 to H12): one
// session per Browse instance, its events on a stream of its own, and the
// calls through its handles. Every call runs on the query lane except
// `close`, which cancels and runs on the control lane. A search never
// writes.

/// A search mode (`search/query-model` Q4): ids are card ids or note ids.
public enum BrowseMode: Sendable, Hashable {
    case cards
    case notes

    init(_ mode: MeguriCoreBindings.BrowseMode) {
        self = mode == .cards ? .cards : .notes
    }
    var raw: MeguriCoreBindings.BrowseMode { self == .cards ? .cards : .notes }
}

public enum BrowseSortDirection: Sendable, Hashable {
    case ascending
    case descending
}

/// A sort (`browser/sorting` S1, S8): a column key (`noteFld`, `cardDue`)
/// or a field column `fld:<name>`, and a direction. A key the mode cannot
/// sort by fails the request with `BrowseFailure.unsortableColumn`.
public struct BrowseSort: Sendable, Hashable {
    public var key: String
    public var direction: BrowseSortDirection
    public init(key: String, direction: BrowseSortDirection) {
        self.key = key
        self.direction = direction
    }
    init(_ raw: MeguriCoreBindings.BrowseSort) {
        self.init(key: raw.key, direction: raw.direction == .ascending ? .ascending : .descending)
    }
    var raw: MeguriCoreBindings.BrowseSort {
        .init(key: key, direction: direction == .ascending ? .ascending : .descending)
    }
}

/// A result handle (H6), opaque and bound to the session that made it.
public struct BrowseHandle: Sendable, Hashable, CustomStringConvertible {
    public let rawValue: UInt64
    public init(rawValue: UInt64) {
        self.rawValue = rawValue
    }
    public var description: String { "handle \(rawValue)" }
}

/// A search error as `failed` gives it (`search/errors` E2, E5, E7).
public struct BrowseSearchError: Sendable, Hashable {
    /// E1's kind name (`invalid_state`, `search_failed`).
    public let kind: String
    /// E2's texts, as the search holds them.
    public let provided: String?
    public let context: String?
    /// E7's message, to show as it is.
    public let message: String
    /// E5's range in UTF-16 code units of the text searched
    /// (`NSRange`/`String.UTF16View` offsets).
    public let utf16Range: Range<Int>

    init(_ raw: MeguriCoreBindings.BrowseSearchError) {
        kind = raw.kind
        provided = raw.provided
        context = raw.context
        message = raw.message
        utf16Range = Int(raw.start)..<Int(raw.end)
    }
}

/// H14's refusals of a session (`BridgeError.browse`). An argument the core
/// rejects (a page of more than 1000 indexes, an unknown column key) is
/// `BridgeError.invalidArgument`; a handle of an older collection is
/// `BridgeError.staleGeneration`.
public enum BrowseFailure: Sendable, Hashable {
    /// A search error (`search/errors` E1 to E3), in a request's `failed`
    /// event only.
    case search(BrowseSearchError)
    case unsortableColumn
    /// The call named another revision than the handle's (H6).
    case staleRevision(current: UInt64)
    /// A released handle, a superseded one, one of another session, or a
    /// closed session (H2, H6).
    case unknownHandle
    /// An index outside the final prefix (H7).
    case outOfRange
    /// `reverse`, `resort` and `toggle` on a handle in progress (H8, H11):
    /// start a new search instead.
    case notComplete

    init(_ raw: MeguriCoreBindings.BrowseFailure) {
        switch raw {
        case .unsortableColumn: self = .unsortableColumn
        case .staleRevision(let current): self = .staleRevision(current: current)
        case .unknownHandle: self = .unknownHandle
        case .outOfRange: self = .outOfRange
        case .notComplete: self = .notComplete
        }
    }
}

/// A session's events (H4). Exactly one terminal event (`complete`,
/// `failed`, `cancelled`) ends every request, and every event of a request
/// comes before any of a later one.
public enum BrowseSearchEvent: Sendable, Hashable {
    /// The first page is final: at most 200 ids, in revision 0.
    case firstPage(seq: UInt64, handle: BrowseHandle, ids: [Int64])
    /// Streamed requests only, at most one per 100 ms: the final prefix is
    /// `matched` rows long in `revision`.
    case progress(seq: UInt64, matched: UInt64, scanned: UInt64, revision: UInt64)
    case complete(seq: UInt64, handle: BrowseHandle, count: UInt64, revision: UInt64)
    case failed(seq: UInt64, error: BridgeError)
    case cancelled(seq: UInt64)

    public var seq: UInt64 {
        switch self {
        case .firstPage(let seq, _, _), .progress(let seq, _, _, _), .complete(let seq, _, _, _),
             .failed(let seq, _), .cancelled(let seq):
            seq
        }
    }

    public var isTerminal: Bool {
        switch self {
        case .complete, .failed, .cancelled: true
        case .firstPage, .progress: false
        }
    }

    init(_ raw: MeguriCoreBindings.BrowseSearchEvent) {
        switch raw {
        case .firstPage(let seq, let handle, let ids):
            self = .firstPage(seq: seq, handle: BrowseHandle(rawValue: handle), ids: ids)
        case .progress(let seq, let matched, let scanned, let revision):
            self = .progress(seq: seq, matched: matched, scanned: scanned, revision: revision)
        case .complete(let seq, let handle, let count, let revision):
            self = .complete(seq: seq, handle: BrowseHandle(rawValue: handle), count: count, revision: revision)
        case .failed(let seq, .search(let error)): self = .failed(seq: seq, error: .browse(.search(BrowseSearchError(error))))
        case .failed(let seq, .core(let error)): self = .failed(seq: seq, error: BridgeError(error))
        case .cancelled(let seq): self = .cancelled(seq: seq)
        }
    }
}

/// C4's row state.
public struct BrowseRowState: Sendable, Hashable {
    /// 0 none, 1 to 7.
    public let flag: UInt8
    public let marked: Bool
    public let leech: Bool
    public let suspended: Bool
    public let buried: Bool
    public let filtered: Bool
}

/// C5's row background.
public enum BrowseTint: Sendable, Hashable {
    case none
    case flag(UInt8)
    case marked
    case suspended
    case buried
}

/// C6: the browser font of the row's template; each member absent alone.
public struct BrowseFont: Sendable, Hashable {
    public let name: String?
    public let size: UInt32?
}

/// The Answer cell (C10): a blank card's has no text.
public enum BrowseAnswer: Sendable, Hashable {
    case text(String)
    case missingCloze(kind: CardBlankKind)
    case templateError(CardTemplateError, side: CardRenderSide)

    init(_ raw: MeguriCoreBindings.BrowseAnswer) {
        self = switch raw {
        case .text(let text): .text(text)
        case .missingCloze(let kind): .missingCloze(kind: CardBlankKind(kind))
        case .templateError(let kind, let side): .templateError(RenderedCard.error(kind), side: CardRenderSide(side))
        }
    }
}

/// One cell (C9). Due values carry C7.5's wrapped flag.
public enum BrowseCell: Sendable, Hashable {
    case text(String)
    case count(UInt64)
    /// Notes-mode Deck spread over this many decks (at least 2).
    case deckCount(UInt64)
    /// Ease.
    case percent(Int64)
    /// Difficulty and Retrievability, shown rounded.
    case percentReal(Float)
    case position(Int64)
    /// Epoch seconds, shown as a local date.
    case instant(seconds: Int64, wrapped: Bool)
    /// Epoch seconds, shown as a local date and time to the minute.
    case dateTime(seconds: Int64)
    case newPosition(Int64, wrapped: Bool)
    case filtered(wrapped: Bool)
    case newMarker
    case learningMarker
    case timeSpan(seconds: Float)
    case empty
    case question(ExcerptQuestion)
    case answer(BrowseAnswer)
    /// Question or Answer in a row page: `qaCells` gives it (H7).
    case deferred
    /// A field column on a row whose note type has no such field (V3).
    case noField
    /// A field column on a field holding media (V3).
    case media(sound: String?, image: String?)

    init(_ raw: MeguriCoreBindings.BrowseCell) {
        self = switch raw {
        case .text(let text): .text(text)
        case .count(let count): .count(count)
        case .deckCount(let decks): .deckCount(decks)
        case .percentInteger(let percent): .percent(percent)
        case .percentReal(let percent): .percentReal(percent)
        case .position(let position): .position(position)
        case .instant(let secs, let wrapped): .instant(seconds: secs, wrapped: wrapped)
        case .dateTime(let secs): .dateTime(seconds: secs)
        case .newPosition(let position, let wrapped): .newPosition(position, wrapped: wrapped)
        case .filtered(let wrapped): .filtered(wrapped: wrapped)
        case .newMarker: .newMarker
        case .learningMarker: .learningMarker
        case .timeSpan(let seconds): .timeSpan(seconds: seconds)
        case .empty: .empty
        case .question(let question): .question(ExcerptQuestion(question))
        case .answer(let answer): .answer(BrowseAnswer(answer))
        case .deferred: .deferred
        case .noField: .noField
        case .media(let sound, let image): .media(sound: sound, image: image)
        }
    }
}

/// A row record (C3).
public struct BrowseRow: Sendable, Hashable {
    /// The card id in cards mode, the note id in notes mode.
    public let id: Int64
    public let noteID: Int64
    public let state: BrowseRowState
    public let tint: BrowseTint
    public let font: BrowseFont
    /// C6: the Sort Field cell is laid out right to left.
    public let sortFieldRightToLeft: Bool
    /// One per requested column, in request order.
    public let cells: [BrowseCell]
}

/// One index of a page: its row, or its row error (C3.1).
public enum BrowseRowResult: Sendable, Hashable {
    case row(BrowseRow)
    /// The row's own card or note no longer exists.
    case deleted
    /// A record the row needs is missing, or the note has no card.
    case damaged
    /// A record exists but does not decode.
    case unreadable(table: String, column: String, row: String)

    init(_ raw: MeguriCoreBindings.BrowseRowResult) {
        switch raw {
        case .row(let row):
            let tint: BrowseTint = switch row.tint {
            case .none: .none
            case .flag(let flag): .flag(flag)
            case .marked: .marked
            case .suspended: .suspended
            case .buried: .buried
            }
            let state = row.state
            self = .row(BrowseRow(
                id: row.id, noteID: row.noteId,
                state: BrowseRowState(flag: state.flag, marked: state.marked, leech: state.leech, suspended: state.suspended,
                                      buried: state.buried, filtered: state.filtered),
                tint: tint, font: BrowseFont(name: row.font.name, size: row.font.size),
                sortFieldRightToLeft: row.sortFieldRtl, cells: row.cells.map(BrowseCell.init)))
        case .deleted: self = .deleted
        case .damaged: self = .damaged
        case .unreadable(let table, let column, let row): self = .unreadable(table: table, column: column, row: row)
        }
    }
}

/// Which cells `qaCells` computes (H7, C10).
public enum BrowseQAWhich: Sendable, Hashable {
    case question
    case answer
    case both

    var raw: MeguriCoreBindings.BrowseQaWhich {
        switch self {
        case .question: .question
        case .answer: .answer
        case .both: .both
        }
    }
}

/// One index of `qaCells`: the cells asked for, or the row error in their
/// place (H7).
public enum BrowseQAResult: Sendable, Hashable {
    case cells(question: ExcerptQuestion?, answer: BrowseAnswer?)
    case deleted
    case damaged
    case unreadable(table: String, column: String, row: String)

    init(_ raw: MeguriCoreBindings.BrowseQaResult) {
        switch raw {
        case .cells(let question, let answer):
            self = .cells(question: question.map(ExcerptQuestion.init), answer: answer.map(BrowseAnswer.init))
        case .deleted: self = .deleted
        case .damaged: self = .damaged
        case .unreadable(let table, let column, let row): self = .unreadable(table: table, column: column, row: row)
        }
    }
}

/// H6's state of a handle in its current revision.
public struct BrowseHandleState: Sendable, Hashable {
    public let mode: BrowseMode
    public let revision: UInt64
    /// The final prefix's length while in progress, the whole count after.
    public let count: UInt64
    public let isComplete: Bool
    public let sort: BrowseSort
}

/// What `invalidatedRows` says (H10).
public struct BrowseInvalidation: Sendable, Hashable {
    public enum Refetch: Sendable, Hashable {
        /// Every row: a note type or a deck changed, or the whole collection.
        case all
        /// Indexes of the new revision, ascending.
        case rows([UInt64])
    }
    /// Indexes in the revision the call named, ascending.
    public let removed: [UInt64]
    public let refetch: Refetch
    public let revision: UInt64
    public let isComplete: Bool
    /// The final prefix's length while in progress, the whole count after.
    public let count: UInt64
}

/// A `changes` core event's change set with the event's seq (H10).
public struct SequencedChangeSet: Sendable, Hashable {
    public let seq: UInt64
    public let changes: ChangeSet
    public init(seq: UInt64, changes: ChangeSet) {
        self.seq = seq
        self.changes = changes
    }
}

/// Where `mapSelection` maps from (H12).
public enum BrowseMapSource: Sendable, Hashable {
    /// A handle of the session, at a revision.
    case handle(BrowseHandle, revision: UInt64)
    /// A released handle's mode: the selection and the current row are ids
    /// the caller holds (rows it fetched).
    case mode(BrowseMode)

    var raw: MeguriCoreBindings.BrowseMapFrom {
        switch self {
        case .handle(let handle, let revision): .handle(handle: handle.rawValue, revision: revision)
        case .mode(let mode): .mode(mode: mode.raw)
        }
    }
}

/// What is selected in the source of `mapSelection` (H12).
public enum BrowseSelected: Sendable, Hashable {
    /// Index ranges of a `.handle` source.
    case ranges([Range<UInt64>])
    /// Ids of the source's mode.
    case ids([Int64])

    var raw: MeguriCoreBindings.BrowseSelected {
        switch self {
        case .ranges(let ranges): .ranges(ranges: ranges.map(\.rawRange))
        case .ids(let ids): .ids(ids: ids)
        }
    }
}

/// What `mapSelection` gives: ascending merged ranges of the target and its
/// current index. When `current` is nil the no-current rule (H12) is the
/// caller's.
public struct BrowseMappedSelection: Sendable, Hashable {
    public let selected: [Range<UInt64>]
    public let current: UInt64?
}

extension Range where Bound == UInt64 {
    var rawRange: MeguriCoreBindings.BrowseIndexRange { .init(start: lowerBound, end: upperBound) }
    init(_ raw: MeguriCoreBindings.BrowseIndexRange) {
        self = raw.start..<raw.end
    }
}

/// A selection kept as index ranges of a handle (H12) until a command
/// resolves it inside the core. It does not keep its session open: once the
/// session is closed or the handle released, resolving it fails with
/// `BrowseFailure.unknownHandle`.
public final class BrowseSelection: Sendable {
    let raw: MeguriCoreBindings.BrowseSelection
    init(_ raw: MeguriCoreBindings.BrowseSelection) {
        self.raw = raw
    }

    /// H12's `resolve_selection` for `target`.
    public func resolve(_ target: BrowseMode) async throws(BridgeError) -> [Int64] {
        let raw = raw
        return try await QueryLane.shared.call(.browseSelectionResolve) { try raw.resolve(target: target.raw) }
    }
}

/// The newest request seq the session started (R1084's second guard).
final class NewestRequest: Sendable {
    let seq = Mutex<UInt64>(0)
}

/// One Browse search session (H1, H2). Its events arrive on `events`, in
/// order; events of a request older than the newest one this object
/// started are dropped (R1084). Releasing the object, or `close()`,
/// cancels its in-flight request and releases its handles.
public final class BrowseSearchSession: Sendable {
    /// Single consumer. Finishes when the session closes.
    public let events: AsyncStream<BrowseSearchEvent>

    private let raw: MeguriCoreBindings.BrowseSearchSession
    private let newest: NewestRequest
    private let rawEvents: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>.Continuation

    private init(
        raw: MeguriCoreBindings.BrowseSearchSession,
        stream: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>,
        continuation: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>.Continuation
    ) {
        self.raw = raw
        rawEvents = continuation
        let newest = NewestRequest()
        self.newest = newest
        let (events, output) = AsyncStream.makeStream(of: BrowseSearchEvent.self, bufferingPolicy: .unbounded)
        self.events = events
        Task.detached(name: "BrowseSearchSession router") {
            await Self.route(stream, newest: newest, to: output)
        }
    }

    /// Converts each event and yields it, unless its request is older than
    /// the newest one started (R1084); finishes `output` when `stream` ends.
    static func route(_ stream: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>, newest: NewestRequest,
                      to output: AsyncStream<BrowseSearchEvent>.Continuation) async {
        for await raw in stream {
            let event = BrowseSearchEvent(raw)
            guard event.seq >= newest.seq.withLock({ $0 }) else { continue }
            output.yield(event)
        }
        output.finish()
    }

    deinit {
        rawEvents.finish()
    }

    /// H2: a new session. It needs no open collection; its requests fail
    /// with the core's refusal while none is open.
    public static func open() async throws(BridgeError) -> BrowseSearchSession {
        let (stream, continuation) = AsyncStream.makeStream(
            of: MeguriCoreBindings.BrowseSearchEvent.self, bufferingPolicy: .unbounded)
        let relay = BrowseSearchRelay(continuation)
        let raw = try await QueryLane.shared.call(.openSearchSession) { try openSearchSession(listener: relay) }
        return BrowseSearchSession(raw: raw, stream: stream, continuation: continuation)
    }

    private func started(_ seq: UInt64) -> UInt64 {
        newest.seq.withLock { $0 = max($0, seq) }
        return seq
    }

    /// H3: searches `text` as given (apply the search-field rule first) in
    /// `mode`, sorted by `sort`; returns the request's seq at once. It
    /// supersedes the session's earlier requests; `cancel` cancels it too.
    public func startSearch(_ text: String, mode: BrowseMode, sort: BrowseSort,
                            cancel: CancelToken? = nil) async throws(BridgeError) -> UInt64 {
        let raw = raw
        let seq = try await QueryLane.shared.call(.browseStartSearch) {
            try raw.startSearch(text: text, mode: mode.raw, sort: sort.raw, cancel: cancel?.raw)
        }
        return started(seq)
    }

    /// H8: sorts a complete handle's rows by `sort` without running the
    /// search again; returns the new request's seq.
    /// `BrowseFailure.notComplete` while the handle's request runs.
    public func resort(_ handle: BrowseHandle, sort: BrowseSort,
                       cancel: CancelToken? = nil) async throws(BridgeError) -> UInt64 {
        let raw = raw
        let seq = try await QueryLane.shared.call(.browseResort) {
            try raw.resort(handle: handle.rawValue, sort: sort.raw, cancel: cancel?.raw)
        }
        return started(seq)
    }

    /// H11: the handle's search in the other mode, sorted by `sort` (that
    /// mode's sort); returns the new request's seq.
    /// `BrowseFailure.notComplete` while the handle's request runs.
    public func toggle(_ handle: BrowseHandle, sort: BrowseSort,
                       cancel: CancelToken? = nil) async throws(BridgeError) -> UInt64 {
        let raw = raw
        let seq = try await QueryLane.shared.call(.browseToggle) {
            try raw.toggle(handle: handle.rawValue, sort: sort.raw, cancel: cancel?.raw)
        }
        return started(seq)
    }

    /// H8: reverses a complete handle; returns its new revision, in which
    /// index i is count − 1 − i.
    public func reverse(_ handle: BrowseHandle, revision: UInt64) async throws(BridgeError) -> UInt64 {
        let raw = raw
        return try await QueryLane.shared.call(.browseReverse) { try raw.reverse(handle: handle.rawValue, revision: revision) }
    }

    /// H10: applies every `changes` core event received since the request
    /// started, in seq order, and says which rows to refetch.
    public func invalidatedRows(_ handle: BrowseHandle, revision: UInt64,
                                changes: [SequencedChangeSet]) async throws(BridgeError) -> BrowseInvalidation {
        let raw = raw
        let sets = changes.map { MeguriCoreBindings.SequencedChangeSet(seq: $0.seq, changes: $0.changes.raw) }
        return try await QueryLane.shared.call(.browseInvalidatedRows) {
            let result = try raw.invalidatedRows(handle: handle.rawValue, revision: revision, changeSets: sets)
            return BrowseInvalidation(removed: result.removed, refetch: result.refetchAll ? .all : .rows(result.refetch),
                                      revision: result.revision, isComplete: result.complete, count: result.count)
        }
    }

    /// H7: the rows of `range` (at most 1000 indexes) with the cells of
    /// `columns`, read from the collection as it is now; Question and
    /// Answer come back `.deferred`.
    public func rows(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>,
                     columns: [String]) async throws(BridgeError) -> [BrowseRowResult] {
        let raw = raw
        return try await QueryLane.shared.call(.browseRows) {
            try raw.rows(handle: handle.rawValue, revision: revision, range: range.rawRange, columns: columns)
                .map(BrowseRowResult.init)
        }
    }

    /// H7: the Question cells, the Answer cells or both for `range` (at
    /// most 1000 indexes), cancellable between rows through `cancel`.
    public func qaCells(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, which: BrowseQAWhich,
                        cancel: CancelToken? = nil) async throws(BridgeError) -> [BrowseQAResult] {
        let raw = raw
        return try await QueryLane.shared.call(.browseQaCells) {
            try raw.qaCells(handle: handle.rawValue, revision: revision, range: range.rawRange, which: which.raw,
                            cancel: cancel?.raw).map(BrowseQAResult.init)
        }
    }

    /// H12: the index of a card id (`kind` cards) or a note id (`kind`
    /// notes) in the handle's final prefix, or nil.
    public func index(of id: Int64, kind: BrowseMode, in handle: BrowseHandle,
                      revision: UInt64) async throws(BridgeError) -> UInt64? {
        let raw = raw
        return try await QueryLane.shared.call(.browseIndexOf) {
            try raw.indexOf(handle: handle.rawValue, revision: revision, kind: kind.raw, id: id)
        }
    }

    /// H12: maps a selection and the current row (an id of the source's
    /// mode) to `target`, over its final prefix at `targetRevision`.
    public func mapSelection(from source: BrowseMapSource, selected: BrowseSelected, current: Int64?,
                             to target: BrowseHandle, targetRevision: UInt64) async throws(BridgeError) -> BrowseMappedSelection {
        let raw = raw
        return try await QueryLane.shared.call(.browseMapSelection) {
            let mapped = try raw.mapSelection(from: source.raw, selected: selected.raw, current: current,
                                              to: target.rawValue, toRevision: targetRevision)
            return BrowseMappedSelection(selected: mapped.selected.map { Range<UInt64>($0) }, current: mapped.current)
        }
    }

    /// H12: the ids an action works on, from index ranges of the handle.
    /// Prefer `selection(_:revision:ranges:)` for a bulk action, so a large
    /// selection is resolved inside the core.
    public func resolveSelection(_ handle: BrowseHandle, revision: UInt64, ranges: [Range<UInt64>],
                                 target: BrowseMode) async throws(BridgeError) -> [Int64] {
        let raw = raw
        return try await QueryLane.shared.call(.browseResolveSelection) {
            try raw.resolveSelection(handle: handle.rawValue, revision: revision, ranges: ranges.map(\.rawRange),
                                     target: target.raw)
        }
    }

    /// H12: the selection of `ranges` at `revision`, resolved only when a
    /// command runs.
    public func selection(_ handle: BrowseHandle, revision: UInt64,
                          ranges: [Range<UInt64>]) async throws(BridgeError) -> BrowseSelection {
        let raw = raw
        return try await QueryLane.shared.call(.browseSelection) {
            BrowseSelection(try raw.selection(handle: handle.rawValue, revision: revision, ranges: ranges.map(\.rawRange)))
        }
    }

    /// H6: the handle's mode, revision, count, state and sort.
    public func state(of handle: BrowseHandle) async throws(BridgeError) -> BrowseHandleState {
        let raw = raw
        return try await QueryLane.shared.call(.browseHandleState) {
            let state = try raw.handleState(handle: handle.rawValue)
            return BrowseHandleState(mode: BrowseMode(state.mode), revision: state.revision, count: state.count,
                                     isComplete: state.complete, sort: BrowseSort(state.sort))
        }
    }

    /// H2: cancels the in-flight request, releases every handle and ends
    /// `events`. Later calls fail with `BrowseFailure.unknownHandle`.
    public func close() async throws(BridgeError) {
        let raw = raw
        try await ControlLane.shared.call(.browseClose) { try raw.close() }
        rawEvents.finish()
    }
}

/// The Swift side of a session's listener. It runs on a core thread and
/// only yields (KTD9).
final class BrowseSearchRelay: MeguriCoreBindings.BrowseSearchListener {
    private let continuation: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>.Continuation

    init(_ continuation: AsyncStream<MeguriCoreBindings.BrowseSearchEvent>.Continuation) {
        self.continuation = continuation
    }

    func onEvent(event: MeguriCoreBindings.BrowseSearchEvent) {
        continuation.yield(event)
    }
}
