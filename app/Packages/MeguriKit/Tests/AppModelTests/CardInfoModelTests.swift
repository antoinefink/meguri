@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// The Info tab's model (U274; `browser/workspace` V13, `stats/card-info`
/// C1, C4, C6, C7.3, C10, C11): every row of a reviewed card in the core's
/// order with its history; a new card with no history and no curve; the
/// snapshot shown stays while another card is read; a change set naming the
/// card reads it again, an unrelated one does not; the chosen range is kept
/// for its card; the inspector names no card for no row or several rows,
/// and keeps the tab chosen.
@MainActor @Suite struct CardInfoModelTests {
    @Test func aReviewedCardShowsEveryRowInTheCoresOrderWithItsHistoryAndCurve() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 1)
        #expect(model.content == .loading)
        try await pollUntil { if case .snapshot = model.content { true } else { false } }
        guard case .snapshot(let snapshot) = model.content else { return }
        #expect(snapshot.rows.map(\.key) == FakeCardInfoReader.allKeys)
        #expect(snapshot.rows.first { $0.key == "difficulty" }?.value == "74%")
        #expect(snapshot.history.map(\.ratingText) == ["3", "1"])
        #expect(model.shownRange?.kind == .firstMonth, "C7.3: the last range offered")
        model.range = .firstWeek
        #expect(model.shownRange?.kind == .firstWeek)
        model.refresh()
        await model.settled()
        #expect(model.range == .firstWeek && model.shownRange?.kind == .firstWeek, "a refresh keeps the range")
    }

    @Test func aNewCardShowsItsRowsOnlyWithNoHistoryAndNoCurve() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 2)
        await model.settled()
        guard case .snapshot(let snapshot) = model.content else { Issue.record("no snapshot"); return }
        #expect(snapshot.history.isEmpty && snapshot.curve == nil && model.shownRange == nil)
        #expect(snapshot.rows.map(\.key) == ["added", "position", "reviews", "lapses", "card_type", "note_type", "deck", "preset",
                                             "card_id", "note_id"])
    }

    @Test func theSnapshotShownStaysWhileAnotherCardIsReadAndAChosenRangeGoesWithItsCard() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 1)
        await model.settled()
        model.range = .firstWeek
        reader.delay = .milliseconds(80)
        model.show(card: 2)
        #expect(model.range == nil)
        guard case .snapshot(let shown) = model.content else { Issue.record("no snapshot"); return }
        #expect(shown.cardID == 1, "no flash: card 1 stays until card 2 is read")
        await model.settled()
        guard case .snapshot(let next) = model.content else { Issue.record("no snapshot"); return }
        #expect(next.cardID == 2)
        // A card named, then another before the first read lands: only the last shows.
        model.show(card: 1)
        model.show(card: 2)
        model.show(card: 1)
        await model.settled()
        try await Task.sleep(for: .milliseconds(120))
        if case .snapshot(let last) = model.content { #expect(last.cardID == 1) } else { Issue.record("no snapshot") }
        model.show(card: nil)
        #expect(model.content == .none && model.cardID == nil)
    }

    @Test func aCardTheCoreCannotShowSaysWhy() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 404)
        await model.settled()
        #expect(model.content == .unavailable(.notFound))
        reader.fails = true
        model.show(card: 1)
        await model.settled()
        #expect(model.content == .failed)
    }

    @Test func aChangeSetNamingWhatTheSnapshotReadReadsItAgain() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 1)
        await model.settled()
        #expect(reader.reads == [1])
        model.changesCommitted(Self.changes(cards: [ChangeSet.Card(id: 9, noteID: 90, change: .updated, memoryStateOnly: false)]))
        await model.settled()
        #expect(reader.reads == [1], "another card's change")
        model.changesCommitted(Self.changes(cards: [ChangeSet.Card(id: 1, noteID: 10, change: .updated, memoryStateOnly: false)]))
        await model.settled()
        model.changesCommitted(Self.changes(revlogAdded: [77]))
        await model.settled()
        model.changesCommitted(Self.changes(decks: [ChangeSet.Row(id: 5, change: .updated)]))
        await model.settled()
        model.changesCommitted(Self.changes(deckConfigs: [ChangeSet.Row(id: 3, change: .updated)]))
        await model.settled()
        #expect(reader.reads == [1, 1, 1, 1, 1], "the card, an answer, its deck, its preset")
        model.reset()
        #expect(model.content == .none && model.cardID == nil)
    }

    @Test func aChangeWhileTheTabIsHiddenIsReadWhenItShowsAgain() async throws {
        let reader = FakeCardInfoReader()
        let model = CardInfoModel(reader: reader)
        model.show(card: 1)
        await model.settled()
        model.active = false
        model.changesCommitted(Self.changes(cards: [ChangeSet.Card(id: 1, noteID: 10, change: .updated, memoryStateOnly: false)]))
        model.dayChanged()
        await model.settled()
        #expect(reader.reads == [1], "nothing read while hidden")
        model.active = true
        await model.settled()
        #expect(reader.reads == [1, 1], "read once when shown again")
        model.show(card: 1)
        await model.settled()
        #expect(reader.reads == [1, 1])
    }

    // MARK: The inspector (V7, V13)

    @Test func theInspectorNamesTheOneRowsCardAndNoneForNoRowOrSeveralAndKeepsTheTab() async throws {
        let fixture = try await BrowseInspectorTests.Fixture()
        let inspector = fixture.inspector
        #expect(inspector.tab == .note && inspector.infoCard == nil)
        fixture.select([1])
        try await pollUntil { inspector.content == .note && inspector.note?.card?.id == 2 }
        #expect(inspector.infoCard == 2)
        fixture.select([1, 2])
        try await pollUntil { inspector.content == .several }
        #expect(inspector.infoCard == nil)
        fixture.select([])
        try await pollUntil { inspector.content == .noSelection }
        #expect(inspector.infoCard == nil)
        // The tab is kept on the device.
        var kept: BrowseInspectorDevice?
        inspector.store = { kept = $0 }
        inspector.tab = .info
        #expect(kept?.tab == .info)
        inspector.toggleCardInfo()
        #expect(!inspector.shown, "⌥⌘I with Info showing hides the pane")
        inspector.toggleCardInfo()
        #expect(inspector.shown && inspector.tab == .info)
    }

    // MARK: Fixture

    static func changes(cards: [ChangeSet.Card] = [], decks: [ChangeSet.Row] = [], deckConfigs: [ChangeSet.Row] = [],
                        revlogAdded: [Int64] = []) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .userUndoable, cards: cards, notes: [], notetypes: [], decks: decks,
                  deckConfigs: deckConfigs, configKeys: [], tags: [], revlogAdded: revlogAdded, revlogRemoved: [],
                  mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                  skippedIncomingDecks: [], collectionRow: false)
    }
}

/// Card 1: reviewed twice, in deck 5 with preset 3, a curve over two ranges;
/// card 2: new; any other card: not found.
@MainActor final class FakeCardInfoReader: CardInfoReading {
    var reads: [Int64] = []
    var delay: Duration?
    var fails = false

    static let allKeys = ["added", "first_review", "latest_review", "due", "position", "interval", "stability", "difficulty",
                          "retrievability", "reviews", "lapses", "average_time", "total_time", "card_type", "note_type", "deck",
                          "preset", "card_id", "note_id", "custom_data"]

    func cardInfo(_ cardID: Int64) async throws -> CardInfoResult {
        reads.append(cardID)
        if let delay { try? await Task.sleep(for: delay) }
        if fails { throw BridgeError.unavailable(reason: "sample") }
        switch cardID {
        case 1: return .found(Self.reviewed)
        case 2: return .found(Self.new)
        default: return .unavailable(.notFound)
        }
    }

    func debugJSON(_ cardID: Int64) async throws -> String? { cardID == 404 ? nil : "{\"card\": \(cardID)}" }

    static let reviewed: CardInfoSnapshot = {
        let values = ["2030-01-01", "2030-01-02", "2030-01-10", "2030-01-25", "3", "15 days", "1.48 months (45 days)", "74%", "90%",
                      "2", "1", "4.5 seconds", "9 seconds", "Card 1", "Basic", "Languages::Verbs", "Default", "1", "10", "a=1"]
        let point = { (instant: Double, retrievability: Double, future: Bool) in
            CardInfoCurvePoint(instant: instant, retrievability: retrievability, future: future, elapsedText: "1 day",
                               retrievabilityText: "\(retrievability)%", stabilityText: "45 days")
        }
        let week = CardInfoCurveRange(kind: .firstWeek, spanDays: 7, points: [point(0, 100, false), point(604_800, 92, false)],
                                      yMin: 90.4, showsRetention: true)
        let month = CardInfoCurveRange(kind: .firstMonth, spanDays: 30,
                                       points: [point(0, 100, false), point(864_000, 88, false), point(864_000, 88, true),
                                                point(2_592_000, 80, true)], yMin: 76, showsRetention: true)
        return CardInfoSnapshot(
            cardID: 1, noteID: 10, rows: zip(allKeys, values).map { CardInfoStat(key: $0, value: $1) },
            history: [
                CardInfoReview(id: 1_894_000_002_000, kind: .review, rating: 3, date: "2030-01-10", dateTime: "2030-01-10 @ 09:00",
                               kindText: "Review", ratingText: "3", interval: "15 days", ease: "D:74%", time: "3s",
                               stability: "1.48 months"),
                CardInfoReview(id: 1_894_000_001_000, kind: .learn, rating: 1, date: "2030-01-02", dateTime: "2030-01-02 @ 08:00",
                               kindText: "Learn", ratingText: "1", interval: "10 minutes", ease: "", time: "6s", stability: "1 day"),
            ],
            curve: CardInfoCurve(ranges: [week, month], defaultRange: .firstMonth, desiredRetention: 0.9, desiredRetentionText: "90%"),
            dependencies: CardInfoDependencies(card: 1, note: 10, notetype: 42, deck: 5, homeDeck: 5, preset: 3,
                                               revlog: [1_894_000_001_000, 1_894_000_002_000], dueFromDayNumber: true))
    }()

    static let new = CardInfoSnapshot(
        cardID: 2, noteID: 20,
        rows: zip(["added", "position", "reviews", "lapses", "card_type", "note_type", "deck", "preset", "card_id", "note_id"],
                  ["2030-01-05", "7", "0", "0", "Card 1", "Basic", "Languages", "Default", "2", "20"])
            .map { CardInfoStat(key: $0, value: $1) },
        history: [], curve: nil,
        dependencies: CardInfoDependencies(card: 2, note: 20, notetype: 42, deck: 4, homeDeck: 4, preset: 3, revlog: [],
                                           dueFromDayNumber: false))
}
