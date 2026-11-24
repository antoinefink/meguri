@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// Browse's column chooser (U272; `browser/workspace` V2.2 to V2.4, V4.3):
/// showing, hiding and moving a column change the shown list at once, the
/// rows read keep their cells (a column added draws nothing until its rows
/// are read again), and each change is one write of the set in its mode;
/// the last column stays; a note type's set becomes its own on its first
/// change; Reset to Standard Columns and Use Standard Columns; the set read
/// again only after the person's own writes; widths per set and mode, kept
/// on the device across launches.
@MainActor @Suite struct BrowseColumnsTests {
    private func results(_ backend: FakeBrowseBackend, session: FakeSearchSession = FakeSearchSession()) async throws
        -> (BrowseResults, FakeSearchSession) {
        session.results[""] = [1, 2, 3]
        backend.searchSession = session
        let results = BrowseResults(backend: backend)
        results.progressDelay = .seconds(60)
        results.run(FilterQuery(), mode: .cards)
        try await pollUntil { results.row(at: 2) != nil }
        return (results, session)
    }

    private static let standardCards = ["noteFld", "template", "deck", "cardDue", "cardMod"]

    // MARK: Show, hide, move (V2.3)

    @Test func hidingAndMovingKeepTheRowsReadAndWriteEachChangeInItsMode() async throws {
        let backend = FakeBrowseBackend()
        let (results, session) = try await results(backend)
        #expect(results.shownColumns == Self.standardCards && results.columnsMode == .cards)
        let reads = session.rowCalls.count
        results.hideColumn("deck")
        #expect(results.list?.columns == ["noteFld", "template", "cardDue", "cardMod"])
        #expect(results.row(at: 0)?.cells == [.text("noteFld 1"), .text("template 1"), .text("cardDue 1"), .text("cardMod 1")])
        results.moveColumns(["cardDue", "noteFld", "template", "cardMod"])
        #expect(results.row(at: 1)?.cells == [.text("cardDue 2"), .text("noteFld 2"), .text("template 2"), .text("cardMod 2")])
        await results.columnWritesSettled()
        #expect(backend.columnChanges == [.hide("deck"), .reorder(["cardDue", "noteFld", "template", "cardMod"])])
        #expect(backend.columnChangeSets == [.standard, .standard] && backend.columnModes == [.cards, .cards])
        try await Task.sleep(for: .milliseconds(30))
        #expect(session.rowCalls.count == reads, "nothing hidden or moved is read again")
    }

    @Test func aColumnShownDrawsNothingUntilItsRowsAreReadAgainAndTheOthersStay() async throws {
        let backend = FakeBrowseBackend()
        let (results, session) = try await results(backend)
        session.rowsDelay = .milliseconds(80)
        results.showColumn("cardIvl")
        // At once: the new column at the end, empty; the other cells as they were.
        #expect(results.list?.columns == Self.standardCards + ["cardIvl"])
        #expect(results.row(at: 0)?.cells.prefix(5) == [.text("noteFld 1"), .text("template 1"), .text("deck 1"), .text("cardDue 1"),
                                                         .text("cardMod 1")])
        #expect(results.row(at: 0)?.cells.last == .pending)
        try await pollUntil { results.row(at: 0)?.cells.last == .text("cardIvl 1") }
        await results.columnWritesSettled()
        #expect(backend.columnChanges == [.show("cardIvl")])
        // A column already shown, or one no catalog holds, is not added again.
        results.showColumn("cardIvl")
        results.showColumn("activeCols")
        await results.columnWritesSettled()
        #expect(backend.columnChanges == [.show("cardIvl")])
    }

    @Test func theLastColumnStays() async throws {
        let backend = FakeBrowseBackend()
        let (results, _) = try await results(backend)
        for key in ["template", "deck", "cardDue", "cardMod"] { results.hideColumn(key) }
        #expect(results.shownColumns == ["noteFld"] && results.canResetColumns)
        results.hideColumn("noteFld")
        results.toggleColumn("noteFld")
        await results.columnWritesSettled()
        #expect(results.shownColumns == ["noteFld"])
        #expect(backend.columnChanges.count == 4, "R1133: the last column cannot go")
    }

    @Test func eachModeKeepsItsOwnColumnsAndSort() async throws {
        let backend = FakeBrowseBackend()
        let (results, _) = try await results(backend)
        results.hideColumn("deck")
        results.sort(by: "cardDue")
        try await pollUntil { results.list?.sort.key == "cardDue" }
        results.setMode(.notes, query: FilterQuery())
        try await pollUntil { results.list?.mode == .notes }
        #expect(results.shownColumns == ["noteFld", "note", "template", "noteTags", "noteCrt"], "Notes mode's own list")
        #expect(results.list?.sort == BrowseSort(key: "noteFld", direction: .ascending))
        results.showColumn("cardReps")
        results.setMode(.cards, query: FilterQuery())
        try await pollUntil { results.list?.mode == .cards }
        #expect(results.shownColumns == ["noteFld", "template", "cardDue", "cardMod"])
        #expect(results.list?.sort == BrowseSort(key: "cardDue", direction: .descending))
        await results.columnWritesSettled()
        try await pollUntil { backend.columnChanges.count == 3 }
        // The sort and the columns are written by two queues: their order between them is not fixed.
        let shown = { (pairs: [(ColumnSetChange, BrowseMode)]) in pairs.map { "\($0.0) in \($0.1)" }.sorted() }
        #expect(shown(Array(zip(backend.columnChanges, backend.columnModes)))
            == shown([(.hide("deck"), .cards), (.setSort(BrowseSort(key: "cardDue", direction: .descending)), .cards),
                      (.show("cardReps"), .notes)]))
    }

    // MARK: Sets (V2.2, V2.4)

    @Test func aNoteTypesSetBecomesItsOwnOnItsFirstChangeAndUseStandardColumnsResetsIt() async throws {
        let backend = FakeBrowseBackend()
        let standard = FakeBrowseBackend.standard
        backend.ownSets[7] = ColumnSetShown(set: .notetype(7), own: false, cards: standard.cards, notes: standard.notes,
                                            cardsSort: standard.cardsSort, notesSort: standard.notesSort)
        let session = FakeSearchSession()
        session.results["run:1"] = [1, 2]
        backend.searchSession = session
        let results = BrowseResults(backend: backend)
        results.progressDelay = .seconds(60)
        results.run(FilterQuery(tokens: [.value([.notetype(id: 7, name: "Vocabulary")], negated: false)]), mode: .cards)
        try await pollUntil { results.row(at: 1) != nil }
        #expect(results.columnSet?.own == false && !results.canResetColumns)
        results.showColumn("fld:Front")
        #expect(results.columnSet?.own == true && results.canResetColumns)
        #expect(results.widthsKey == "7/cards")
        await results.columnWritesSettled()
        #expect(backend.columnChangeSets == [.notetype(7)] && backend.columnChanges == [.show("fld:Front")])
        results.resetColumns()
        await results.columnWritesSettled()
        #expect(backend.useStandardCalls == [7])
    }

    @Test func resetToStandardColumnsGivesTheDefaultColumnsInOrderAndForgetsTheWidths() async throws {
        let backend = FakeBrowseBackend()
        let (results, _) = try await results(backend)
        #expect(!results.canResetColumns)
        results.hideColumn("deck")
        results.showColumn("cardIvl")
        results.setWidths(["cardDue": 120])
        #expect(results.canResetColumns && results.width(of: "cardDue") == 120)
        let revision = results.widthsRevision
        results.resetColumns()
        #expect(results.shownColumns == Self.standardCards && results.width(of: "cardDue") == nil)
        #expect(results.widthsRevision != revision && !results.canResetColumns)
        await results.columnWritesSettled()
        #expect(backend.columnChanges.suffix(3) == [.show("deck"), .hide("cardIvl"), .reorder(Self.standardCards)],
                "shown first, so the list never empties; then the order")
    }

    @Test func theSetIsReadAgainOnlyAfterThePersonsOwnWrites() async throws {
        let backend = FakeBrowseBackend()
        backend.persistsColumns = true
        let (results, _) = try await results(backend)
        backend.columnChangeDelay = .milliseconds(120)
        results.hideColumn("deck")
        // The write's echo comes before the write is done: the list shown keeps the change.
        results.changesCommitted(Self.columnSetsChanged, seq: 50)
        try await Task.sleep(for: .milliseconds(40))
        #expect(results.shownColumns == ["noteFld", "template", "cardDue", "cardMod"])
        await results.columnWritesSettled()
        try await Task.sleep(for: .milliseconds(40))
        #expect(results.shownColumns == ["noteFld", "template", "cardDue", "cardMod"])
        // Another device's change, with no write of ours under way, shows at once, keeping the rows read.
        backend.columnChangeDelay = nil
        try await backend.changeColumnSet(.standard, mode: .cards, change: .show("cardLapses"))
        results.changesCommitted(Self.columnSetsChanged, seq: 51)
        try await pollUntil { results.shownColumns.last == "cardLapses" }
        #expect(results.row(at: 0)?.cells.first == .text("noteFld 1"))
    }

    // MARK: Widths (V2.3, K11)

    @Test func widthsAreKeptPerSetAndModeOnTheDeviceAcrossLaunches() async throws {
        let defaults = try #require(UserDefaults(suiteName: "BrowseColumnsTests.\(UUID().uuidString)"))
        let store = BrowseDeviceStore(defaults: defaults)
        let backend = FakeBrowseBackend()
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        backend.searchSession = session
        let first = BrowseModel(backend: backend, store: store)
        first.results.run(FilterQuery(), mode: .cards)
        try await pollUntil { first.results.list != nil }
        first.results.setWidths(["deck": 210.3, "cardDue": 96])
        #expect(first.results.width(of: "deck") == 210.5, "kept to the half point")
        first.results.setMode(.notes, query: FilterQuery())
        try await pollUntil { first.results.list?.mode == .notes }
        #expect(first.results.width(of: "deck") == nil, "Notes mode has its own widths")
        first.results.setWidths(["note": 150])
        // A relaunch reads them back.
        let relaunched = FakeBrowseBackend()
        let again = FakeSearchSession()
        again.results[""] = [1, 2, 3]
        relaunched.searchSession = again
        let second = BrowseModel(backend: relaunched, store: store)
        second.results.run(FilterQuery(), mode: .cards)
        try await pollUntil { second.results.list != nil }
        #expect(second.results.width(of: "deck") == 210.5 && second.results.width(of: "cardDue") == 96)
        #expect(store.load().columnWidths?["std/notes"] == ["note": 150])
    }

    // MARK: Fixture

    static let columnSetsChanged = ChangeSet(
        wholeCollection: false, source: .userNonUndoable, cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [],
        configKeys: [ChangeSet.Named(name: "meguriBrowserColumnSets", change: .updated)], tags: [], revlogAdded: [],
        revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
        skippedIncomingDecks: [], collectionRow: false)
}
