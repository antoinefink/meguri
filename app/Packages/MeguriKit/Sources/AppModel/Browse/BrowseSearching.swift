import CoreBridge
import Foundation

/// One Browse search session (`browser/result-sets` H2) as the results
/// table uses it (U270a). Production is `CoreBrowseSearch`, over CoreBridge's
/// `BrowseSearchSession`; tests and the test hooks' routes give their own.
/// Every call runs off the main thread and reads nothing but the
/// collection: a search never writes (H9).
public nonisolated protocol BrowseSearching: AnyObject, Sendable {
    /// H4's events, in order, one consumer. An event of a request older than
    /// the newest one started is dropped before it gets here (R1084).
    var events: AsyncStream<BrowseSearchEvent> { get }
    /// H3: returns the request's seq; its events may come before it returns.
    func startSearch(_ text: String, mode: BrowseMode, sort: BrowseSort) async throws -> UInt64
    /// H8: a complete handle sorted again without a run.
    func resort(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64
    /// H11: a complete handle's search in the other mode.
    func toggle(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64
    /// H7: at most 1,000 rows of `range`, Question and Answer deferred.
    func rows(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, columns: [String]) async throws
        -> [BrowseRowResult]
    /// H7: the deferred Question and Answer cells of `range`.
    func qaCells(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, which: BrowseQAWhich) async throws
        -> [BrowseQAResult]
    /// H10: the change sets applied to the handle, and the rows to read again.
    func invalidatedRows(_ handle: BrowseHandle, revision: UInt64, changes: [SequencedChangeSet]) async throws
        -> BrowseInvalidation
    /// H12: a selection and a current row mapped into `target`.
    func mapSelection(from source: BrowseMapSource, selected: BrowseSelected, current: Int64?, to target: BrowseHandle,
                      targetRevision: UInt64) async throws -> BrowseMappedSelection

    // The commands (U275; `browser/workspace` V7, V8). Each takes the rows a
    // command acts on as index ranges of a handle at a revision (H12): the
    // core resolves them, and refuses them when that revision is no longer
    // the handle's, so a command never acts on rows other than those chosen.

    /// V7, V8.1: the counts of the target cards and notes (B1).
    func counts(of target: BrowseTarget) async throws -> BrowseSelectionCounts
    /// V8.2: every card of the target notes, those the search did not find
    /// included: the cards a Delete removes.
    func cardsOfNotes(of target: BrowseTarget) async throws -> UInt64
    /// V7: every row of `target`'s handle whose note is a target note (Select
    /// Sibling Cards), as ascending ranges of that handle at its revision.
    func siblingRows(of target: BrowseTarget) async throws -> [Range<UInt64>]
    /// V8.1: a card action on the target cards, one undo step.
    func run(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BulkActionResult
    /// V8.1: a note action on the target notes, one undo step.
    func run(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BulkActionResult
    /// U276's sheets: the action started, with its progress and Cancel
    /// (honoured until its commit, `browser/bulk-framework` K3).
    func start(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BrowseRunningAction
    func start(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BrowseRunningAction
    /// V8.1: the tags the target notes store, each once, sorted (Remove
    /// Tags' field suggests only these).
    func tags(of target: BrowseTarget) async throws -> [String]

    // Find and Replace and Find Duplicates (U277; `browser/workspace` V9,
    // V10): the sheets' reads and the one write, each in the core.

    /// V9's previews of one sheet: each newer one supersedes the older.
    func findReplacePreviews() async throws -> any BrowseFindReplacePreviewing
    /// V9, F11: the run on the target's notes, one undo step; `confirmed`
    /// is the preview's count of notes that change.
    func startFindReplace(_ request: FindReplaceRequest, on target: BrowseTarget, confirmed: UInt64) async throws
        -> BrowseRunning<FindReplaceResult>
    /// V10's Find among every note (nil) or the target's notes; it writes nothing.
    func startFindDuplicates(field: String, on target: BrowseTarget?) async throws -> BrowseRunning<FindDuplicatesResult>
}

/// One Find and Replace sheet's previews (V9, F10): pure reads, each newer
/// call superseding the older in the core. Cancelling the calling task
/// cancels its preview.
public nonisolated protocol BrowseFindReplacePreviewing: AnyObject, Sendable {
    func preview(_ request: FindReplaceRequest, on target: BrowseTarget) async throws -> FindReplacePreview
}

/// A job under way (U277): its progress, its Cancel (honoured until its
/// commit for a write), and its value once it ends. Cancel ends it with
/// `BridgeError.cancelled`.
public nonisolated struct BrowseRunning<Value: Sendable>: Sendable {
    public let progress: AsyncStream<BrowseActionProgress>
    public let cancel: @Sendable () async -> Void
    public let result: @Sendable () async throws -> Value

    public init(progress: AsyncStream<BrowseActionProgress>, cancel: @escaping @Sendable () async -> Void,
                result: @escaping @Sendable () async throws -> Value) {
        self.progress = progress
        self.cancel = cancel
        self.result = result
    }

    /// A job already running to its end with no progress and no Cancel.
    public static func finishing(_ run: @escaping @Sendable () async throws -> Value) -> BrowseRunning<Value> {
        BrowseRunning(progress: AsyncStream { $0.finish() }, cancel: {}, result: run)
    }
}

/// A session without the find commands (the test hooks' samples): each fails.
public extension BrowseSearching {
    func findReplacePreviews() async throws -> any BrowseFindReplacePreviewing { throw BrowseCommandUnsupported() }
    func startFindReplace(_ request: FindReplaceRequest, on target: BrowseTarget, confirmed: UInt64) async throws
        -> BrowseRunning<FindReplaceResult> {
        throw BrowseCommandUnsupported()
    }
    func startFindDuplicates(field: String, on target: BrowseTarget?) async throws -> BrowseRunning<FindDuplicatesResult> {
        throw BrowseCommandUnsupported()
    }
}

/// A bulk action under way (U276): its progress while it plans, its Cancel,
/// and its result once it ends. Cancel ends it with `BridgeError.cancelled`
/// and writes nothing while it can still be cancelled.
public nonisolated struct BrowseRunningAction: Sendable {
    public let progress: AsyncStream<BrowseActionProgress>
    public let cancel: @Sendable () async -> Void
    public let result: @Sendable () async throws -> BulkActionResult

    public init(progress: AsyncStream<BrowseActionProgress>, cancel: @escaping @Sendable () async -> Void,
                result: @escaping @Sendable () async throws -> BulkActionResult) {
        self.progress = progress
        self.cancel = cancel
        self.result = result
    }

    /// An action that is already running to its end with no progress and
    /// no Cancel (a session without jobs).
    public static func finishing(_ run: @escaping @Sendable () async throws -> BulkActionResult) -> BrowseRunningAction {
        BrowseRunningAction(progress: AsyncStream { $0.finish() }, cancel: {}, result: run)
    }
}

/// A running action's progress: the work done of its total (nil while not
/// known), and whether Cancel still stops it (false once it commits).
public nonisolated struct BrowseActionProgress: Sendable, Hashable {
    public let completed: UInt64
    public let total: UInt64?
    public let cancellable: Bool

    public init(completed: UInt64, total: UInt64?, cancellable: Bool) {
        self.completed = completed
        self.total = total
        self.cancellable = cancellable
    }
}

/// The rows a command acts on (`browser/result-sets` H12): index ranges of
/// a handle at one revision, in the handle's mode.
public nonisolated struct BrowseTarget: Sendable, Hashable {
    public let handle: BrowseHandle
    public let revision: UInt64
    /// Ascending, merged.
    public let ranges: [Range<UInt64>]
    public let mode: BrowseMode

    public init(handle: BrowseHandle, revision: UInt64, ranges: [Range<UInt64>], mode: BrowseMode) {
        self.handle = handle
        self.revision = revision
        self.ranges = ranges
        self.mode = mode
    }

    /// The number of rows.
    public var rows: Int { ranges.reduce(0) { $0 + Int(clamping: $1.count) } }
}

/// A session that runs no command (the test hooks' sample collections):
/// each command fails, so its controls stay as they were.
public extension BrowseSearching {
    func counts(of target: BrowseTarget) async throws -> BrowseSelectionCounts { throw BrowseCommandUnsupported() }
    func cardsOfNotes(of target: BrowseTarget) async throws -> UInt64 { throw BrowseCommandUnsupported() }
    func siblingRows(of target: BrowseTarget) async throws -> [Range<UInt64>] { throw BrowseCommandUnsupported() }
    func run(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BulkActionResult {
        throw BrowseCommandUnsupported()
    }
    func run(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BulkActionResult {
        throw BrowseCommandUnsupported()
    }
    func tags(of target: BrowseTarget) async throws -> [String] { throw BrowseCommandUnsupported() }
}

/// A session whose actions run to their end without progress or Cancel.
public extension BrowseSearching {
    func start(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        .finishing { try await self.run(action, on: target) }
    }
    func start(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        .finishing { try await self.run(action, on: target) }
    }
}

/// A session without commands was asked to run one.
public nonisolated struct BrowseCommandUnsupported: Error, Sendable {}

/// The production session, over CoreBridge (U264).
public nonisolated final class CoreBrowseSearch: BrowseSearching {
    private let session: BrowseSearchSession

    public init(_ session: BrowseSearchSession) {
        self.session = session
    }

    public static func open() async throws -> CoreBrowseSearch {
        CoreBrowseSearch(try await BrowseSearchSession.open())
    }

    public var events: AsyncStream<BrowseSearchEvent> { session.events }

    public func startSearch(_ text: String, mode: BrowseMode, sort: BrowseSort) async throws -> UInt64 {
        try await session.startSearch(text, mode: mode, sort: sort)
    }

    public func resort(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
        try await session.resort(handle, sort: sort)
    }

    public func toggle(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
        try await session.toggle(handle, sort: sort)
    }

    public func rows(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, columns: [String]) async throws
        -> [BrowseRowResult] {
        try await session.rows(handle, revision: revision, range: range, columns: columns)
    }

    public func qaCells(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, which: BrowseQAWhich) async throws
        -> [BrowseQAResult] {
        try await session.qaCells(handle, revision: revision, range: range, which: which)
    }

    public func invalidatedRows(_ handle: BrowseHandle, revision: UInt64, changes: [SequencedChangeSet]) async throws
        -> BrowseInvalidation {
        try await session.invalidatedRows(handle, revision: revision, changes: changes)
    }

    public func mapSelection(from source: BrowseMapSource, selected: BrowseSelected, current: Int64?, to target: BrowseHandle,
                             targetRevision: UInt64) async throws -> BrowseMappedSelection {
        try await session.mapSelection(from: source, selected: selected, current: current, to: target,
                                       targetRevision: targetRevision)
    }

    func selection(_ target: BrowseTarget) async throws -> BrowseSelection {
        try await session.selection(target.handle, revision: target.revision, ranges: target.ranges)
    }

    public func counts(of target: BrowseTarget) async throws -> BrowseSelectionCounts {
        try await CoreBrowseQuery.startSelectionCounts(try await selection(target)).value()
    }

    /// Read in the core from the selection's ranges (U276): no note id
    /// crosses the FFI, whatever the selection's size (V17).
    public func cardsOfNotes(of target: BrowseTarget) async throws -> UInt64 {
        try await Self.cancelling(try await CoreBrowseQuery.startNoteCards(try await selection(target)))
    }

    /// The handle's rows of the selected rows' notes, read from the handle
    /// in the core (U276).
    public func siblingRows(of target: BrowseTarget) async throws -> [Range<UInt64>] {
        try await selection(target).siblingRows()
    }

    public func tags(of target: BrowseTarget) async throws -> [String] {
        try await Self.cancelling(try await CoreBrowseQuery.startSelectionTags(try await selection(target)))
    }

    /// A read job's value; cancelling the task cancels the job, so a
    /// sheet that closes frees the search reader its read holds.
    private static func cancelling<T: Sendable>(_ job: CoreBrowseReadJob<T>) async throws -> T {
        try await withTaskCancellationHandler {
            try await job.value()
        } onCancel: {
            Task { try? await job.job.cancel() }
        }
    }

    public func run(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BulkActionResult {
        try await CoreBulkActions.start(action, on: try await selection(target)).value()
    }

    public func run(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BulkActionResult {
        try await CoreBulkActions.start(action, on: try await selection(target)).value()
    }

    public func start(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        Self.running(try await CoreBulkActions.start(action, on: try await selection(target)))
    }

    public func start(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        Self.running(try await CoreBulkActions.start(action, on: try await selection(target)))
    }

    private static func running(_ job: CoreBulkActionJob) -> BrowseRunningAction {
        let progress = AsyncStream<BrowseActionProgress> { continuation in
            let task = Task {
                for await report in job.job.progress {
                    continuation.yield(BrowseActionProgress(completed: report.completed, total: report.total,
                                                            cancellable: report.cancellable))
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
        return BrowseRunningAction(progress: progress, cancel: { try? await job.job.cancel() },
                                   result: { try await job.value() })
    }
}

// MARK: What the cells draw

/// One cell as the table draws it, already formatted (`browser/workspace`
/// V2.5, V3, V5): text at the leading edge, values (numbers, dates, spans)
/// at the trailing edge in tabular figures.
public nonisolated enum BrowseShownCell: Sendable, Hashable {
    case text(String)
    case value(String)
    /// V5.1: the deck's last name component, then its parent path joined
    /// with " › " (nil at the top level); `full` is the whole path, for the
    /// help tag and VoiceOver.
    case deck(name: String, parent: String?, full: String)
    /// V3: a field column on a row whose note type has no such field.
    case noField
    /// V3: a field holding a sound, an image, or both.
    case media(sound: Bool, image: Bool)
    /// Question or Answer, not computed yet (`qaCells` gives it).
    case pending
    case empty
}

/// One row as the table draws it.
public nonisolated struct BrowseShownRow: Sendable, Hashable {
    /// C3.1: a row, or the error in its place.
    public enum Status: Sendable, Hashable {
        case row, deleted, damaged, unreadable
    }

    public let status: Status
    /// The card id (Cards) or the note id (Notes); nil for a row error.
    public let id: Int64?
    public let noteID: Int64?
    public let state: BrowseRowState?
    public let font: BrowseFont?
    public let sortFieldRightToLeft: Bool
    /// One per column of the list, in its order.
    public let cells: [BrowseShownCell]

    public init(status: Status, id: Int64?, noteID: Int64?, state: BrowseRowState?, font: BrowseFont?,
                sortFieldRightToLeft: Bool, cells: [BrowseShownCell]) {
        self.status = status
        self.id = id
        self.noteID = noteID
        self.state = state
        self.font = font
        self.sortFieldRightToLeft = sortFieldRightToLeft
        self.cells = cells
    }

    /// V2.5: a suspended or buried row's text is secondary.
    public var isDimmed: Bool { status != .row || state?.suspended == true || state?.buried == true }

    /// The same row for another column list: `map` gives each new column's
    /// index in this row's cells, or nil for a column not read yet.
    public func remapped(_ map: [Int?]) -> BrowseShownRow {
        BrowseShownRow(status: status, id: id, noteID: noteID, state: state, font: font,
                       sortFieldRightToLeft: sortFieldRightToLeft,
                       cells: map.map { index in index.flatMap { cells.indices.contains($0) ? cells[$0] : nil } ?? .pending })
    }

    /// The same row with Question and Answer filled in.
    public func filling(_ cells: [Int: BrowseShownCell]) -> BrowseShownRow {
        var next = self.cells
        for (index, cell) in cells where next.indices.contains(index) { next[index] = cell }
        return BrowseShownRow(status: status, id: id, noteID: noteID, state: state, font: font,
                              sortFieldRightToLeft: sortFieldRightToLeft, cells: next)
    }
}

/// Formats rows off the main thread (`browser/workspace` V5, the Browse
/// feature's strings and the device's locale), so a page that arrives while
/// the table scrolls costs the main thread only its cells' drawing.
public nonisolated protocol BrowseRowPresenting: Sendable {
    func present(_ row: BrowseRowResult, columns: [String], mode: BrowseMode) -> BrowseShownRow
    /// The cells `qaCells` gives for one row, as the column at `index` of
    /// `columns` draws them.
    func present(_ cells: BrowseQAResult, columns: [String]) -> [Int: BrowseShownCell]
}

/// A presenter with no strings of its own (the model's tests): text as it
/// is, values as their plain decimal or epoch text.
public nonisolated struct PlainBrowsePresenter: BrowseRowPresenting {
    public init() {}

    public func present(_ row: BrowseRowResult, columns: [String], mode: BrowseMode) -> BrowseShownRow {
        switch row {
        case .row(let row):
            BrowseShownRow(status: .row, id: row.id, noteID: row.noteID, state: row.state, font: row.font,
                           sortFieldRightToLeft: row.sortFieldRightToLeft, cells: row.cells.map(Self.cell))
        case .deleted: Self.error(.deleted, columns)
        case .damaged: Self.error(.damaged, columns)
        case .unreadable: Self.error(.unreadable, columns)
        }
    }

    public func present(_ cells: BrowseQAResult, columns: [String]) -> [Int: BrowseShownCell] {
        guard case .cells(let question, let answer) = cells else { return [:] }
        var shown: [Int: BrowseShownCell] = [:]
        for (index, key) in columns.enumerated() {
            if key == "question", case .text(let text)? = question { shown[index] = .text(text) }
            if key == "answer", case .text(let text)? = answer { shown[index] = .text(text) }
        }
        return shown
    }

    private static func error(_ status: BrowseShownRow.Status, _ columns: [String]) -> BrowseShownRow {
        BrowseShownRow(status: status, id: nil, noteID: nil, state: nil, font: nil, sortFieldRightToLeft: false,
                       cells: columns.map { _ in .empty })
    }

    private static func cell(_ cell: BrowseCell) -> BrowseShownCell {
        switch cell {
        case .text(let text): .text(text)
        case .count(let n): .value(String(n))
        case .deckCount(let n): .value("(\(n))")
        case .percent(let n): .value("\(n)%")
        case .percentReal(let n): .value("\(n)%")
        case .position(let n), .newPosition(let n, _): .value(String(n))
        case .instant(let seconds, _), .dateTime(let seconds): .value(String(seconds))
        case .filtered: .value("(filtered)")
        case .newMarker: .value("(new)")
        case .learningMarker: .value("(learning)")
        case .timeSpan(let seconds): .value(String(seconds))
        case .empty: .empty
        case .question(.text(let text)): .text(text)
        case .question, .answer: .empty
        case .deferred: .pending
        case .noField: .noField
        case .media(let sound, let image): .media(sound: sound != nil, image: image != nil)
        }
    }
}

// MARK: Columns

/// The column catalog's sort metadata (`browser/sorting` S1,
/// `browser/workspace` V4.1).
public nonisolated enum BrowseColumns {
    /// Whether a header click on `key` sorts in `mode`.
    public static func sortable(_ key: String, mode: BrowseMode) -> Bool {
        if key.hasPrefix("fld:") { return true }
        switch key {
        case "question", "answer": return false
        case "stability", "difficulty", "retrievability": return mode == .cards
        default: return firstDirection(key) != nil
        }
    }

    /// The direction a first click on `key`'s header sorts in.
    public static func firstDirection(_ key: String) -> BrowseSortDirection? {
        if key.hasPrefix("fld:") { return .ascending }
        switch key {
        case "noteFld", "deck", "note", "noteTags": return .ascending
        case "template", "cardDue", "cardEase", "cardIvl", "cardReps", "cardLapses", "noteCrt", "cardMod", "noteMod",
             "originalPosition", "stability", "difficulty", "retrievability":
            return .descending
        default: return nil
        }
    }

    /// The sort a click on `key`'s header asks for when `current` is shown:
    /// the column in its first direction, or `current` reversed (V4.1).
    public static func click(_ key: String, current: BrowseSort) -> BrowseSort? {
        if key == current.key {
            return BrowseSort(key: key, direction: current.direction == .ascending ? .descending : .ascending)
        }
        return firstDirection(key).map { BrowseSort(key: key, direction: $0) }
    }
}
