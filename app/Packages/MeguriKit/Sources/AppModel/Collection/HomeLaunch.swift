import CoreBridge
import Foundation

/// The last read Home keeps for a launch (N27.2): the snapshot, and whether its counts are
/// still this study day's.
public nonisolated struct KeptHome: Equatable, Sendable {
    public let snapshot: HomeSnapshot
    /// False once the study day the counts were read in has ended: names and the tree's
    /// shape show, the counts are held back until the read (N27.3).
    public let countsCurrent: Bool
    public let nodes: [CountedDeckNode]

    public init(snapshot: HomeSnapshot, now: Date) {
        self.snapshot = snapshot
        countsCurrent = snapshot.countsCurrent(now: now)
        nodes = snapshot.nodes(countsCurrent: countsCurrent)
    }
}

/// What Home's root shows (N27.1). The welcome and "No Decks" say something about the
/// collection, so each shows only once it is known to be true; "not known yet" is its own
/// state, which shows the last read (`kept`) or the page's own background (`pending`).
public nonisolated enum HomePhase: Equatable, Sendable {
    /// There is no collection to open on this device: the welcome (N18.3).
    case welcome
    /// Not known yet and nothing kept: the page's background, with the system's small
    /// progress indicator only after half a second (N27.3).
    case pending
    /// Not known yet: the last read of this collection, replaced in place by the first read.
    case kept
    /// This launch's read of the open collection.
    case decks
    /// A finished read found no deck.
    case noDecks
    /// The open collection's first deck-tree read failed: its error, with Try Again.
    case readFailed
    case unavailable
    case failed(String)
    case creationRecovery

    /// `treeRead`: a read of the open generation's deck tree has been applied.
    /// `readFailed`: the open generation's first read failed and none has applied since.
    /// `kept`: a kept read with at least one deck is there to show.
    public static func resolve(state: ReviewCollection.State, treeRead: Bool, readFailed: Bool = false,
                               decksEmpty: Bool, kept: Bool) -> HomePhase {
        switch state {
        case .noFile: return .welcome
        case .creationRecovery: return .creationRecovery
        case .failed(let message): return .failed(message)
        case .ready where treeRead: return decksEmpty ? .noDecks : .decks
        case .ready where readFailed: return .readFailed
        // A tree already read that an exclusive phase holds back keeps the busy state; before
        // any read, the kept read shows, else the busy state the core reported.
        case .unavailable where treeRead: return .unavailable
        case .unavailable: return kept ? .kept : .unavailable
        case .unopened, .loading, .ready: return kept ? .kept : .pending
        }
    }

    /// The deck list (or Today) shows: read, or kept.
    public var showsDecks: Bool { self == .decks || self == .kept }
    /// Nothing is known about the decks yet.
    public var unknown: Bool { self == .pending || self == .kept }
}

extension ReviewCollection {
    /// A read of the open generation's deck tree has been applied (N27.1).
    public var deckTreeRead: Bool { state == .ready && deckTreeGeneration != nil && deckTreeGeneration == generation }

    public var homePhase: HomePhase {
        let read = deckTreeGeneration != nil && deckTreeGeneration == generation
        return HomePhase.resolve(state: state, treeRead: read, readFailed: deckTreeFailedGeneration == generation,
                                 decksEmpty: decks.isEmpty, kept: keptHome.map { !$0.nodes.isEmpty } ?? false)
    }

    /// The tree Home draws: the read one, or the kept one while nothing is known yet.
    /// Actions never use it: they wait for the read and act on `decks` (N27.4).
    public var homeDecks: [CountedDeckNode] {
        if homePhase == .kept, let keptHome { return keptHome.nodes }
        return decks
    }

    /// The kept counts are from an earlier study day: rows draw none, Today and the deck
    /// page wait for the read, and nothing speaks a zero for them (N27.3).
    public var homeCountsHeld: Bool { homePhase == .kept && keptHome?.countsCurrent == false }

    /// A deck of `homeDecks`.
    public func homeDeck(_ id: Int64) -> CountedDeckNode? {
        guard homePhase == .kept else { return deck(id) }
        return Self.find(id, in: homeDecks)
    }

    /// The deck page's content: the read one, or the kept deck while nothing is known yet.
    public func homeDeckPage(_ id: Int64) -> DeckPageDetail? {
        guard homePhase == .kept else { return state == .ready ? deckPage(id) : nil }
        return homeDeck(id).map { DeckPageDetail(id: $0.id, name: $0.name, counts: $0.counts) }
    }

    /// Today's numbers: read for this collection, or kept from this study day.
    public var homeStudiedToday: StudiedToday? {
        studiedToday ?? (keptHome?.countsCurrent == true ? keptHome?.snapshot.studied?.value : nil)
    }

    public var homeNextLearningReadyAt: Date? {
        if todayNumbersGeneration == generation, deckTreeRead { return nextLearningReadyAt }
        return keptHome?.countsCurrent == true ? keptHome?.snapshot.nextLearningReadyAt : nextLearningReadyAt
    }

    /// Today's Previous Session (N24.2): the walk's in this launch, else the last deck's
    /// session the store keeps; kept while nothing is known yet.
    public var homePreviousSession: HomeSnapshot.Previous? {
        if homePhase == .kept { return keptHome?.snapshot.previous }
        return previousSession
    }

    /// The tab layout's list: the previous session's answers (N2).
    public var homePreviousAnswerCount: Int? {
        if homePhase == .kept { return keptHome?.snapshot.previousAnswers }
        return previousRecord?.answers.count
    }

    var previousSession: HomeSnapshot.Previous? {
        if let walk = previousWalk {
            return .init(deckNames: walk.decks.compactMap { fullDeckName($0) }, walk: true,
                         cards: walk.cards, endedAt: walk.endedAt)
        }
        guard let record = previousRecord, let last = record.answers.last else { return nil }
        return .init(deckNames: fullDeckName(record.deckID).map { [$0] } ?? [], walk: false,
                     cards: Set(record.answers.map(\.cardID)).count, endedAt: record.endedAt ?? last.answeredAt)
    }

    /// Today's Study on kept content: it looks as it will, and its press waits for the read.
    public var studyAllKept: Bool {
        homePhase == .kept && !homeCountsHeld && !StudyWalk.decks(from: homeDecks).isEmpty
            && navigation.sheet == nil && review == nil
    }

    /// A deck page's Study on kept content (N27.4).
    public func studyKept(deckID: Int64) -> Bool {
        homePhase == .kept && !homeCountsHeld && navigation.sheet == nil && review == nil
            && (homeDeck(deckID).map { DeckWaiting(counts: $0.counts, filtered: $0.filtered).offersAddNote != true } ?? false)
    }

    /// Waits while nothing is known yet, at most `firstReadWait`, and only while nothing else
    /// was asked for since `intent`. True when the read arrived and the press still stands.
    func awaitFirstRead(intent: UInt64) async -> Bool {
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: firstReadWait)
        while homePhase.unknown || (deckTreeRead && !deckActionsReady), navigation.intentRevision == intent,
              clock.now < deadline {
            try? await Task.sleep(for: .milliseconds(20))
        }
        return deckTreeRead && deckActionsReady && navigation.intentRevision == intent
    }

    static func find(_ id: Int64, in nodes: [CountedDeckNode]) -> CountedDeckNode? {
        for node in nodes {
            if node.id == id { return node }
            if let found = find(id, in: node.children) { return found }
        }
        return nil
    }
}
