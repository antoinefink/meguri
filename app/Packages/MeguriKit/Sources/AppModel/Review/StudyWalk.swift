import CoreBridge
import Foundation
import Observation

/// Today's Study (contract N24.3, `decks/deck-list` H12): the top-level normal decks with
/// cards waiting, one ordinary session per deck, in one review presentation. The walk
/// keeps the list, its turns and its numbers; `ReviewCollection` starts and ends the
/// sessions. Its identity is the presentation's, so a new deck never presents a new review.
@Observable
@MainActor
public final class StudyWalk {
    public nonisolated struct Deck: Hashable, Sendable {
        public let id: Int64
        /// The deck's full name (a top-level deck: its only component).
        public let name: String
        /// Its cards waiting when the walk started (the deck list's counts, `sched/counts` C5).
        public let waiting: Int
        public init(id: Int64, name: String, waiting: Int) { self.id = id; self.name = name; self.waiting = waiting }
    }

    /// The walk's list: the top-level normal decks with cards waiting, in the deck list's order.
    /// Filtered decks are never in it (H3).
    public nonisolated static func decks(from nodes: [CountedDeckNode]) -> [Deck] {
        nodes.filter { !$0.filtered && $0.counts.waiting > 0 }
            .map { Deck(id: $0.id, name: $0.name, waiting: $0.counts.waiting) }
    }

    /// What a walk leaves for Today's Previous Session (N24.2): the decks it answered in,
    /// the different cards, its active time and when it ended.
    public nonisolated struct Summary: Equatable, Sendable {
        public let decks: [Int64]
        public let cards: Int
        public let seconds: UInt64
        public let endedAt: Date
    }

    /// Where a deck of the list stands. The progress bar counts the decks still to come,
    /// those left for the end included, at their waiting counts from the start (N24.3).
    enum Standing: Equatable { case toCome, leftForEnd, done }

    /// The order of turns (N24.3): the list once, then round it again while a pass answers
    /// a card. `answers` is the walk's total so far.
    struct Cursor: Equatable {
        let count: Int
        private(set) var index = -1
        private(set) var pass = 0
        private var answersAtPassStart = 0

        init(count: Int) { self.count = count }

        /// The next deck's index to try, or nil when the walk ends: after a whole pass that
        /// answered no card, a pass in which no deck had one included.
        mutating func next(answers: Int) -> Int? {
            index += 1
            if index >= count {
                guard count > 0, answers > answersAtPassStart else { return nil }
                pass += 1; answersAtPassStart = answers; index = 0
            }
            return index
        }

        /// After the open deck's Keep Going the walk goes on as in Turns: a new pass.
        mutating func restart(answers: Int) {
            index = -1; pass += 1; answersAtPassStart = answers
        }
    }

    public let decks: [Deck]
    /// The open deck's session.
    public private(set) var current: ReviewCoordinator?
    /// Between decks, or deciding whether one is next: the review shows its progress
    /// indicator in place of a terminal screen (N24.3: "no screen, title or pause").
    public private(set) var advancing = true
    /// No deck has a card ready: the open deck shows the walk's finished screen.
    public private(set) var ended = false
    /// Back to Decks, a refused start or a closed collection: nothing more starts.
    public private(set) var stopped = false
    /// Sessions started, the open one included.
    public private(set) var turns = 0
    var cursor: Cursor
    private(set) var standing: [Int64: Standing]
    private var endedCards: Set<Int64> = []
    private var endedAnswers = 0
    private var endedMilliseconds: UInt64 = 0
    /// The decks answered in, in the order of their first answer.
    private var answeredDecks: [Int64] = []
    /// The records of sessions already counted (a record's id is its session's: an object's
    /// address can come back for the next deck's).
    @ObservationIgnored private var closedSessions: Set<UUID> = []

    public init(decks: [Deck]) {
        self.decks = decks
        cursor = Cursor(count: decks.count)
        standing = Dictionary(decks.map { ($0.id, Standing.toCome) }) { first, _ in first }
    }

    // MARK: Numbers

    /// Answers over the whole walk.
    public var answers: Int { endedAnswers + (open?.record.answers.count ?? 0) }

    /// N16.5 for the walk: the different cards answered in it and its active study time.
    public var summary: ReviewSessionSummary {
        var cards = endedCards
        var milliseconds = endedMilliseconds
        if let open {
            cards.formUnion(open.record.answers.map(\.cardID))
            milliseconds += open.activeMilliseconds
        }
        return ReviewSessionSummary(cards: cards.count, seconds: milliseconds / 1_000)
    }

    /// N16.1 over the walk: its answers, against those plus the open deck's queue and the
    /// cards waiting, as the list counted them at the start, of the decks still to come.
    public var progress: ReviewSessionProgress {
        let openDeck = open?.record.deckID
        let toCome = decks.reduce(0) { sum, deck in
            deck.id != openDeck && standing[deck.id] != .done ? sum + deck.waiting : sum
        }
        let queue = open.map { Int($0.counts.new) + Int($0.counts.learning) + Int($0.counts.review) } ?? 0
        return ReviewSessionProgress(answered: answers, remaining: queue + toCome)
    }

    func previousSession(at date: Date) -> Summary? {
        var decks = answeredDecks
        if let open, !open.record.answers.isEmpty, !decks.contains(open.record.deckID) { decks.append(open.record.deckID) }
        let summary = self.summary
        guard summary.cards > 0 else { return nil }
        return Summary(decks: decks, cards: summary.cards, seconds: summary.seconds, endedAt: date)
    }

    /// The open session, until it ends.
    private var open: ReviewCoordinator? {
        guard let current, !closedSessions.contains(current.record.id) else { return nil }
        return current
    }

    // MARK: Turns, as `ReviewCollection` takes them

    func present(_ model: ReviewCoordinator) {
        current = model
        turns += 1
        standing[model.record.deckID] = .done
    }

    /// The deck the walk would study next, nil when it ends.
    func nextDeck() -> Deck? {
        cursor.next(answers: answers).map { decks[$0] }
    }

    func markAdvancing() { advancing = true }

    func beginAdvance() {
        advancing = true
        if ended { ended = false; cursor.restart(answers: answers) }
    }

    /// A deck passed over: nothing ready at its turn. It stays to come while learning
    /// cards are due later today (left for the end), otherwise it is done.
    func skip(_ deck: Deck, learningLater: Bool) {
        standing[deck.id] = learningLater ? .leftForEnd : .done
    }

    /// The open session ended at its turn's end: its numbers join the walk's.
    func close(_ model: ReviewCoordinator, leftForEnd: Bool) {
        guard closedSessions.insert(model.record.id).inserted else { return }
        let record = model.record
        endedAnswers += record.answers.count
        endedCards.formUnion(record.answers.map(\.cardID))
        endedMilliseconds += model.activeMilliseconds
        if !record.answers.isEmpty, !answeredDecks.contains(record.deckID) { answeredDecks.append(record.deckID) }
        standing[record.deckID] = leftForEnd ? .leftForEnd : .done
    }

    /// The open deck has a card again (a start that found one, Keep Going): no progress
    /// indicator over it.
    func settle() { advancing = false }

    /// No deck of the list has a card ready: the finished screen shows.
    func finish() { advancing = false; ended = true }

    func stop() { stopped = true; advancing = false }
}
