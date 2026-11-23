internal import MeguriCoreBindings
import Foundation
import Instrumentation
import Synchronization

// The Statistics page's request (U103, U292; `stats/history-reads` H15,
// H16, H18; `stats/graphs-common` G8, G9.5, G10). One request per refresh
// names every part; the core runs it as a job off the main thread, on its
// query reader, and returns the parts by tier as each is ready, with
// progress for the parts that wait, then its end. Reads only.

/// The page's deck filter (`stats/history-reads` H1).
public enum StatsPageScope: Sendable, Hashable, Codable {
    case allDecks
    /// A deck and its subdecks, with the filtered-deck cards whose home deck is among them (G2.1).
    case deck(Int64)
    /// Not offered in this version: the page's own parts fail with `unsupportedScope`.
    case search(String)

    var raw: MeguriCoreBindings.StatsScope {
        switch self {
        case .allDecks: .allDecks
        case .deck(let id): .deck(id: id)
        case .search(let text): .search(text: text)
        }
    }
}

/// G3's history values.
public enum StatsHistory: Sendable, Hashable, Codable {
    /// Days −366 to 0 (and rows after `N`).
    case last12Months
    case allHistory

    init(_ raw: MeguriCoreBindings.StatsHistory) {
        switch raw {
        case .last12Months: self = .last12Months
        case .allHistory: self = .allHistory
        }
    }
}

/// The page's period (`stats/statistics-page` S3.2).
public enum StatsPagePeriod: Sendable, Hashable, Codable, CaseIterable {
    case oneMonth, threeMonths, sixMonths, oneYear, all

    /// The history a request for this period reads (G3's round 11 line): 1M to 1Y read the
    /// last 12 months and re-bin in Swift; All reads all history.
    public var history: StatsHistory {
        self == .all ? .allHistory : .last12Months
    }

    /// Whether moving from `previous` to this period needs a new request: only a change to or
    /// from All does (S10.5, H16); a change among 1M to 1Y re-bins the parts already returned.
    public func needsNewRequest(after previous: StatsPagePeriod) -> Bool {
        history != previous.history
    }

    var raw: MeguriCoreBindings.StatsPeriod {
        switch self {
        case .oneMonth: .oneMonth
        case .threeMonths: .threeMonths
        case .sixMonths: .sixMonths
        case .oneYear: .oneYear
        case .all: .all
        }
    }
}

/// The tiers of `stats/statistics-page` S10.1, in the order the parts return.
public enum StatsTier: Int, Sendable, Hashable, Comparable {
    case present = 1, reviewLog, knowledge
    public static func < (lhs: StatsTier, rhs: StatsTier) -> Bool { lhs.rawValue < rhs.rawValue }
}

/// A part of the page request (H5); the raw value is its name.
public enum StatsPart: String, Sendable, Hashable, Codable, CaseIterable {
    case cardCounts = "card_counts"
    case futureDue = "future_due"
    case intervals
    case stability
    case difficulty
    case retrievability
    case studiedToday = "studied_today"
    case reviews
    case byAnswer = "by_answer"
    case retention
    case retentionByDay = "retention_by_day"
    case hours
    case buttons
    case added
    case cardStatesOverTime = "card_states_over_time"
    case started
    case byDeck = "by_deck"
    case activityDays = "activity_days"
    case knowledgeOverTime = "knowledge_over_time"

    /// H5's tier.
    public var tier: StatsTier {
        switch self {
        case .cardCounts, .futureDue, .intervals, .stability, .difficulty, .retrievability, .studiedToday: .present
        case .knowledgeOverTime: .knowledge
        default: .reviewLog
        }
    }

    init(_ raw: MeguriCoreBindings.StatsPart) {
        switch raw {
        case .cardCounts: self = .cardCounts
        case .futureDue: self = .futureDue
        case .intervals: self = .intervals
        case .stability: self = .stability
        case .difficulty: self = .difficulty
        case .retrievability: self = .retrievability
        case .studiedToday: self = .studiedToday
        case .reviews: self = .reviews
        case .byAnswer: self = .byAnswer
        case .retention: self = .retention
        case .retentionByDay: self = .retentionByDay
        case .hours: self = .hours
        case .buttons: self = .buttons
        case .added: self = .added
        case .cardStatesOverTime: self = .cardStatesOverTime
        case .started: self = .started
        case .byDeck: self = .byDeck
        case .activityDays: self = .activityDays
        case .knowledgeOverTime: self = .knowledgeOverTime
        }
    }

    var raw: MeguriCoreBindings.StatsPart {
        switch self {
        case .cardCounts: .cardCounts
        case .futureDue: .futureDue
        case .intervals: .intervals
        case .stability: .stability
        case .difficulty: .difficulty
        case .retrievability: .retrievability
        case .studiedToday: .studiedToday
        case .reviews: .reviews
        case .byAnswer: .byAnswer
        case .retention: .retention
        case .retentionByDay: .retentionByDay
        case .hours: .hours
        case .buttons: .buttons
        case .added: .added
        case .cardStatesOverTime: .cardStatesOverTime
        case .started: .started
        case .byDeck: .byDeck
        case .activityDays: .activityDays
        case .knowledgeOverTime: .knowledgeOverTime
        }
    }
}

/// One refresh of the page (G8.3): one request naming its parts.
public struct StatsPageRequest: Sendable, Hashable {
    public var scope: StatsPageScope
    public var period: StatsPagePeriod
    /// The parts wanted; one named twice is returned once.
    public var parts: [StatsPart]
    /// The refresh's id, returned in the header.
    public var refresh: UInt64

    public init(scope: StatsPageScope, period: StatsPagePeriod, parts: [StatsPart] = StatsPart.allCases,
                refresh: UInt64) {
        self.scope = scope; self.period = period; self.parts = parts; self.refresh = refresh
    }

    var raw: MeguriCoreBindings.StatsPageRequest {
        .init(scope: scope.raw, period: period.raw, parts: parts.map(\.raw), refresh: refresh)
    }
}

/// G1's context: every value of a request is computed for it, so the page needs no clock of
/// its own for its days. A later read for the result on screen passes it back.
public struct StatsContext: Sendable, Hashable, Codable {
    /// `N`, epoch seconds.
    public let nextDayAt: Int64
    /// The scheduler's day number.
    public let today: UInt32
    public let rolloverHour: UInt8
    public let nowMilliseconds: Int64
    /// Seconds east of UTC at now.
    public let utcOffsetSeconds: Int32
    /// The device zone's IANA identifier.
    public let zoneIdentifier: String
    public let generation: UInt64
    public let seq: UInt64

    public init(nextDayAt: Int64, today: UInt32, rolloverHour: UInt8, nowMilliseconds: Int64, utcOffsetSeconds: Int32,
                zoneIdentifier: String, generation: UInt64, seq: UInt64) {
        self.nextDayAt = nextDayAt; self.today = today; self.rolloverHour = rolloverHour
        self.nowMilliseconds = nowMilliseconds; self.utcOffsetSeconds = utcOffsetSeconds
        self.zoneIdentifier = zoneIdentifier; self.generation = generation; self.seq = seq
    }

    init(_ raw: MeguriCoreBindings.StatsContext) {
        nextDayAt = raw.nextDayAt; today = raw.today; rolloverHour = raw.rolloverHour; nowMilliseconds = raw.nowMs
        utcOffsetSeconds = raw.utcOffsetSecs; zoneIdentifier = raw.zone; generation = raw.generation; seq = raw.seq
    }

    var raw: MeguriCoreBindings.StatsContext {
        .init(nextDayAt: nextDayAt, today: today, rolloverHour: rolloverHour, nowMs: nowMilliseconds,
              utcOffsetSecs: utcOffsetSeconds, zone: zoneIdentifier, generation: generation, seq: seq)
    }
}

/// G7's keys and `fsrs`, as the request read them. The page writes none of them (S3.4).
public struct StatsPreferences: Sendable, Hashable, Codable {
    /// 0 Sunday, 1 Monday, 5 Friday, 6 Saturday.
    public let firstDayOfWeek: UInt8
    public let cardCountsSeparateInactive: Bool
    public let futureDueShowBacklog: Bool
    public let fsrs: Bool

    public init(firstDayOfWeek: UInt8, cardCountsSeparateInactive: Bool, futureDueShowBacklog: Bool, fsrs: Bool) {
        self.firstDayOfWeek = firstDayOfWeek; self.cardCountsSeparateInactive = cardCountsSeparateInactive
        self.futureDueShowBacklog = futureDueShowBacklog; self.fsrs = fsrs
    }

    init(_ raw: MeguriCoreBindings.StatsPreferences) {
        firstDayOfWeek = raw.firstDayOfWeek; cardCountsSeparateInactive = raw.cardCountsSeparateInactive
        futureDueShowBacklog = raw.futureDueShowBacklog; fsrs = raw.fsrs
    }
}

/// The scope's browse text (`stats/browse-queries` Q2).
public enum StatsBrowseScope: Sendable, Hashable, Codable {
    /// All Decks: an item's query is used alone.
    case none
    case text(String)
    /// No item of the scope is clickable.
    case unavailable

    init(_ raw: MeguriCoreBindings.StatsBrowseScope) {
        switch raw {
        case .none: self = .none
        case .text(let text): self = .text(text)
        case .unavailable: self = .unavailable
        }
    }
}

/// An unreadable row (G4): its table, column and row key.
public struct StatsUnreadable: Sendable, Hashable, Codable {
    public let table: String
    public let column: String
    public let key: Int64

    public init(table: String, column: String, key: Int64) {
        self.table = table; self.column = column; self.key = key
    }

    init(_ raw: MeguriCoreBindings.StatsUnreadable) {
        table = raw.table; column = raw.column; key = raw.key
    }
}

/// Why one part has no value; the request's other parts still return (H15).
public enum StatsPartFailure: Sendable, Hashable, Codable {
    /// A Search scope for one of the page's own parts (H18).
    case unsupportedScope
    /// An unreadable row of the scope outside the window, for the parts that read all history.
    case unreadableRow(StatsUnreadable)
    /// The part's core unit has not landed.
    case notBuilt
    /// A core error that stopped the whole part.
    case failed(String)
    /// Its bytes do not decode as the part's value: never shown as data.
    case undecodable(String)

    init(_ raw: MeguriCoreBindings.StatsPartFailure) {
        switch raw {
        case .unsupportedScope: self = .unsupportedScope
        case .unreadableRow(let row): self = .unreadableRow(StatsUnreadable(row))
        case .notBuilt: self = .notBuilt
        case .failed(let message): self = .failed(message)
        }
    }
}

/// H9's `oldest_day`.
public enum StatsOldestDay: Sendable, Hashable, Codable {
    /// The smallest day (H1) of a row of the scope.
    case day(Int)
    /// The scope has no row.
    case noRows
    /// Not known (H18): the All axis starts at the window's first month.
    case unavailable(StatsPartFailure)

    init(_ raw: MeguriCoreBindings.StatsOldestDay) {
        switch raw {
        case .day(let day): self = .day(Int(day))
        case .noRows: self = .noRows
        case .unavailable(let failure): self = .unavailable(StatsPartFailure(failure))
        }
    }
}

/// What every part of a request is computed for (G8.2, H5's last line).
public struct StatsPageHeader: Sendable, Hashable, Codable {
    public let refresh: UInt64
    /// The history the request read.
    public let history: StatsHistory
    public let context: StatsContext
    public let preferences: StatsPreferences
    public let browseScope: StatsBrowseScope
    public let oldestDay: StatsOldestDay

    public init(refresh: UInt64, history: StatsHistory, context: StatsContext, preferences: StatsPreferences,
                browseScope: StatsBrowseScope, oldestDay: StatsOldestDay) {
        self.refresh = refresh; self.history = history; self.context = context; self.preferences = preferences
        self.browseScope = browseScope; self.oldestDay = oldestDay
    }

    init(_ raw: MeguriCoreBindings.StatsPageHeader) {
        refresh = raw.refresh; history = StatsHistory(raw.history); context = StatsContext(raw.context)
        preferences = StatsPreferences(raw.preferences); browseScope = StatsBrowseScope(raw.browseScope)
        oldestDay = StatsOldestDay(raw.oldestDay)
    }
}

/// A part's result.
public enum StatsPartOutcome: Sendable, Hashable, Codable {
    case data(StatsPartValue)
    case failed(StatsPartFailure)
}

/// One part, returned, with the header of its request.
public struct StatsPartResult: Sendable, Hashable {
    public let part: StatsPart
    public let header: StatsPageHeader
    public let outcome: StatsPartOutcome

    public init(part: StatsPart, header: StatsPageHeader, outcome: StatsPartOutcome) {
        self.part = part; self.header = header; self.outcome = outcome
    }
}

/// How far a waiting part has got (H15), at most every 100 ms.
public enum StatsProgress: Sendable, Hashable {
    /// Between 0 and 1; never goes back.
    case fraction(Double)
    case indeterminate

    init(_ raw: MeguriCoreBindings.StatsProgress) {
        switch raw {
        case .fraction(let fraction): self = .fraction(fraction)
        case .indeterminate: self = .indeterminate
        }
    }
}

/// G8.2's scope error: the page keeps what it shows.
public enum StatsScopeError: Sendable, Hashable {
    /// A Search scope's error, its range in UTF-16 units of the scope's text.
    case search(BrowseSearchError)
    /// A deck id that names no deck.
    case unknownDeck
}

/// How a request ended: always its last message.
public enum StatsPageEnd: Sendable, Hashable {
    /// Every part named has returned or failed.
    case complete
    /// Before any part: every part fails alike.
    case scopeError(StatsScopeError)
    /// An unreadable row of the scope in the window, before any part.
    case unreadableRow(StatsUnreadable)
    /// A commit landed while a part waited: the parts not returned are dropped, those returned
    /// hold for their context. The page, stale anyway (G10), refreshes.
    case contextChanged
    /// Cancelled: nothing after the cancel was delivered.
    case cancelled
    /// The request could not run.
    case failed(BridgeError)

    init(_ raw: MeguriCoreBindings.StatsPageEnd) {
        switch raw {
        case .complete: self = .complete
        case .scopeError(.search(let error)): self = .scopeError(.search(BrowseSearchError(error)))
        case .scopeError(.unknownDeck): self = .scopeError(.unknownDeck)
        case .unreadableRow(let row): self = .unreadableRow(StatsUnreadable(row))
        case .contextChanged: self = .contextChanged
        case .cancelled: self = .cancelled
        case .failed(let error): self = .failed(BridgeError(error))
        }
    }
}

/// A message of a request, in order. `end` is the last.
public enum StatsPageMessage: Sendable, Hashable {
    case part(StatsPartResult)
    case progress(StatsPart, StatsProgress)
    case end(StatsPageEnd)
}

/// Whether a request ended with `cancel()`.
final class StatsPageCancel: Sendable {
    let cancelled = Mutex(false)
}

/// What the router reads: the core's messages, then the job's end.
enum StatsPageEvent: Sendable {
    case core(MeguriCoreBindings.StatsPageMessage)
    /// The job's `JobFinished`. It ends the stream only when no `End` came: a job cancelled
    /// before a worker took it never runs, and a job that panicked sent none.
    case jobEnded(JobOutcome)
}

/// One running page request. Iterate it once: its messages in the order the core sent them,
/// the last always `.end`. After `cancel()` (or when the consuming task is cancelled) no part
/// or progress is delivered, not even one already buffered or the stray one the core can let
/// through, and the end is `.cancelled`.
public final class StatsPageStream: AsyncSequence, Sendable {
    public typealias Element = StatsPageMessage

    /// The core job; nil for a stream that replays given messages.
    let job: CoreJob?
    private let messages: AsyncStream<StatsPageMessage>
    private let state: StatsPageCancel
    private let stop: @Sendable () async -> Void

    init(job: CoreJob, messages: AsyncStream<StatsPageMessage>, state: StatsPageCancel) {
        self.job = job; self.messages = messages; self.state = state
        stop = { try? await job.cancel() }
    }

    /// A stream of `messages`, for a fake `StatsService` in tests and previews: no core job runs.
    /// It behaves as a core stream does: after `cancel()` (which then runs `onCancel`) only
    /// `.end(.cancelled)` is delivered, and a stream that finishes without its end gives that end.
    public init(replaying messages: AsyncStream<StatsPageMessage>,
                onCancel: @escaping @Sendable () async -> Void = {}) {
        job = nil; self.messages = messages; state = StatsPageCancel(); stop = onCancel
    }

    public func makeAsyncIterator() -> StatsPageIterator {
        StatsPageIterator(base: messages.makeAsyncIterator(), state: state)
    }

    /// Cancels the request (H16): from now on only its `.cancelled` end is delivered; the core
    /// stops within 50 ms. A history build the request started goes on.
    public func cancel() async {
        state.cancelled.withLock { $0 = true }
        await stop()
    }

    /// Starts `request` and returns its stream at once.
    static func start(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream {
        let (raw, rawInput) = AsyncStream.makeStream(of: StatsPageEvent.self, bufferingPolicy: .unbounded)
        let relay = StatsPageRelay(rawInput)
        let wanted = request.raw
        let job = try await Jobs.start(.startStatsPage) { try startStatsPage(request: wanted, listener: relay) }
        let state = StatsPageCancel()
        let (messages, output) = AsyncStream.makeStream(of: StatsPageMessage.self, bufferingPolicy: .unbounded)
        output.onTermination = { termination in
            guard case .cancelled = termination else { return }
            state.cancelled.withLock { $0 = true }
            Task.detached { try? await job.cancel() }
        }
        Task.detached(name: "StatsPageStream router") {
            await route(raw, state: state, to: output) { try? await job.cancel() }
        }
        // `JobFinished` follows the core's `End`, so this comes after it in the raw stream;
        // once the relay has finished that stream it does nothing.
        Task.detached(name: "StatsPageStream job") {
            rawInput.yield(.jobEnded(await job.outcome()))
            rawInput.finish()
        }
        return StatsPageStream(job: job, messages: messages, state: state)
    }

    /// Converts each message and yields it, keeping the header for the parts that follow it,
    /// until the end. Once cancelled, only the end goes out, as `.cancelled`. A part before any
    /// header ends the request as failed (`abandon` cancels it), never dropping a part quietly.
    static func route(_ stream: AsyncStream<StatsPageEvent>, state: StatsPageCancel,
                      to output: AsyncStream<StatsPageMessage>.Continuation,
                      abandon: @escaping @Sendable () async -> Void = {}) async {
        var header: StatsPageHeader?
        func finish(_ end: StatsPageEnd) {
            // Compared and yielded under one lock, as below.
            state.cancelled.withLock { cancelled in
                _ = output.yield(.end(cancelled ? .cancelled : end))
            }
            output.finish()
        }
        for await event in stream {
            let message: StatsPageMessage
            switch event {
            case .core(.header(let raw)):
                header = StatsPageHeader(raw)
                continue
            case .core(.part(let part, let outcome)):
                guard let header else {
                    finish(.failed(.internal("a part arrived before its header")))
                    await abandon()
                    return
                }
                let part = StatsPart(part)
                message = .part(StatsPartResult(part: part, header: header, outcome: decode(part, outcome)))
            case .core(.progress(let part, let progress)):
                message = .progress(StatsPart(part), StatsProgress(progress))
            case .core(.end(let end)):
                finish(StatsPageEnd(end))
                return
            case .jobEnded(let outcome):
                switch outcome {
                case .succeeded: finish(.failed(.internal("the page request ended without its end")))
                case .cancelled: finish(.cancelled)
                case .failed(let error): finish(.failed(error))
                }
                return
            }
            // Once `cancel()` has set the flag, no later message goes out (yield never blocks).
            state.cancelled.withLock { cancelled in
                if !cancelled { _ = output.yield(message) }
            }
        }
        finish(.failed(.internal("the page request ended without its end")))
    }

    static func decode(_ part: StatsPart, _ outcome: MeguriCoreBindings.StatsPartOutcome) -> StatsPartOutcome {
        switch outcome {
        case .failed(let failure): return .failed(StatsPartFailure(failure))
        case .data(let bytes):
            do throws(StatsBytes.Failure) {
                return .data(try StatsPartValue.decode(part, bytes))
            } catch {
                return .failed(.undecodable(error.reason))
            }
        }
    }
}

/// Reads a page stream: once the request is cancelled, it skips every buffered part and
/// progress and gives `.end(.cancelled)`; a stream that stops without its end (its reader's
/// task was cancelled) gives that end too.
public struct StatsPageIterator: AsyncIteratorProtocol {
    var base: AsyncStream<StatsPageMessage>.Iterator
    let state: StatsPageCancel
    var ended = false

    public mutating func next() async -> StatsPageMessage? {
        guard !ended else { return nil }
        while let message = await base.next() {
            let cancelled = state.cancelled.withLock { $0 }
            switch message {
            case .end(let end):
                ended = true
                return .end(cancelled ? .cancelled : end)
            case .part, .progress:
                if !cancelled { return message }
            }
        }
        ended = true
        state.cancelled.withLock { $0 = true }
        return .end(.cancelled)
    }
}

/// The Swift side of a request's listener. It runs on the request's core thread and only yields
/// (KTD9); the end closes its stream.
final class StatsPageRelay: MeguriCoreBindings.StatsPageListener {
    /// How many relays are alive: the core releases a request's relay once its job has ended.
    static let live = Mutex(0)

    private let continuation: AsyncStream<StatsPageEvent>.Continuation

    init(_ continuation: AsyncStream<StatsPageEvent>.Continuation) {
        self.continuation = continuation
        Self.live.withLock { $0 += 1 }
    }

    deinit {
        Self.live.withLock { $0 -= 1 }
    }

    func onMessage(message: MeguriCoreBindings.StatsPageMessage) {
        continuation.yield(.core(message))
        if case .end = message { continuation.finish() }
    }
}

/// G10: whether an event makes the page's results stale. Every published change set does
/// (`StatsChanged{graphsStale}`), and so do a new day, a moved day boundary, a zone change and a
/// replaced collection; the page refreshes half a second after the last one (S11) and drops its
/// kept results on `collectionReplaced` (S10.2).
public enum StatsStaleness: Sendable, Hashable {
    /// A change set: the values, scopes or preferences may differ.
    case graphsStale
    /// `N`, today or the offset changed.
    case dayChanged
    /// Another collection is open now: kept results are dropped.
    case collectionReplaced

    public init?(_ kind: CoreEvent.Kind) {
        switch kind {
        case .changes: self = .graphsStale
        case .rolledOver, .nextDayAtChanged, .timeZoneChanged: self = .dayChanged
        case .collectionReplaced: self = .collectionReplaced
        default: return nil
        }
    }
}

/// H13's row kinds for the cards behind a day.
public enum StatsDayKind: Sendable, Hashable {
    /// The rows Reviews Over Time counts.
    case reviewed
    /// Activity's rows.
    case answered
    /// Forgotten's rows (answered Again).
    case forgotten

    var raw: MeguriCoreBindings.StatsDayKind {
        switch self {
        case .reviewed: .reviewed
        case .answered: .answered
        case .forgotten: .forgotten
        }
    }
}

/// What H13 gives.
public enum StatsDayCards: Sendable, Hashable {
    /// The existing cards with such a row on the day, ascending; never empty.
    case cards([Int64])
    case noCards
    /// A Search scope (H18).
    case unsupportedScope
    case scopeError(StatsScopeError)
    case unreadableRow(StatsUnreadable)
    case cancelled

    init(_ raw: MeguriCoreBindings.StatsDayCards) {
        switch raw {
        case .cards(let ids): self = .cards(ids)
        case .noCards: self = .noCards
        case .unsupportedScope: self = .unsupportedScope
        case .scopeError(.search(let error)): self = .scopeError(.search(BrowseSearchError(error)))
        case .scopeError(.unknownDeck): self = .scopeError(.unknownDeck)
        case .unreadableRow(let row): self = .unreadableRow(StatsUnreadable(row))
        case .cancelled: self = .cancelled
        }
    }
}

/// The Statistics page's reads (U103, U292).
public protocol StatsService: Sendable {
    /// H13: the cards behind `day` (H1) of the result on screen, whose header gave `context`.
    /// A commit since that result does not refuse it.
    func dayCards(scope: StatsPageScope, context: StatsContext, day: Int, kind: StatsDayKind,
                  cancel: CancelToken?) async throws(BridgeError) -> StatsDayCards
    /// Starts one page request and returns its stream at once.
    func page(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream
    /// G9.5, H14.4: the memory warning. Releases the column caches and the history component
    /// (persisted first); the next request reloads them.
    func releaseCaches() async throws(BridgeError)
    /// G9.2: Home has been idle for 2 s after an open; starts the caches' background build.
    func homeIdle() async throws(BridgeError)
}

/// Page requests and the release run as core jobs; the cards behind a day are a short read on
/// the query lane, the idle signal a short call on the maintenance lane. None runs on the main actor or writes the collection.
public struct CoreStatsService: StatsService {
    public init() {}

    public func page(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream {
        if Task.isCancelled { throw .cancelled }
        return try await StatsPageStream.start(request)
    }

    public func releaseCaches() async throws(BridgeError) {
        // A job: the release waits on the derived writer, which can be busy for a while.
        try await Jobs.start(.startReleaseStatsCaches) { try startReleaseStatsCaches() }.value()
    }

    public func dayCards(scope: StatsPageScope, context: StatsContext, day: Int, kind: StatsDayKind,
                         cancel: CancelToken? = nil) async throws(BridgeError) -> StatsDayCards {
        if Task.isCancelled { throw .cancelled }
        guard let day = Int32(exactly: day) else { throw .invalidArgument("day \(day)") }
        let rawScope = scope.raw, rawContext = context.raw, rawKind = kind.raw, token = cancel?.raw
        return try await QueryLane.shared.call(.statsDayCards) {
            StatsDayCards(try statsDayCards(scope: rawScope, context: rawContext, day: day, kind: rawKind, cancel: token))
        }
    }

    public func homeIdle() async throws(BridgeError) {
        try await MaintenanceLane.shared.call(.statsHomeIdle) { try statsHomeIdle() }
    }
}
