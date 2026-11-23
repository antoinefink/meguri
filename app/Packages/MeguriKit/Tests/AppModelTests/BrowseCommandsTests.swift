@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// Browse's commands (U275; `browser/workspace` V7, V8, N21.5, N21.8): which
/// show and are enabled for which rows (V8.1), the titles' counts, that a
/// command acts on exactly the rows chosen at their revision (H12, K1), the
/// Delete question's rule (V8.2), the undo step's title (V8.3), VoiceOver's
/// result, Select All and Select Sibling Cards (V7), the rows an undo of a
/// Delete brings back, the kept scroll position (V1) and the saved
/// searches' Rename… and Delete (T13), against a scripted session.
@MainActor @Suite struct BrowseCommandsTests {
    // MARK: V8.1's rules

    private func facts(rows: Int, mode: BrowseMode = .cards, cards: UInt64? = nil, notes: UInt64? = nil, suspended: UInt64? = 0,
                       buried: UInt64? = 0, notetypes: [Int64]? = [8], noteCards: UInt64? = nil) -> BrowseTargetFacts {
        BrowseTargetFacts(mode: mode, rows: rows, cards: cards ?? UInt64(rows), notes: notes ?? UInt64(rows), suspended: suspended,
                          buried: buried, notetypes: notetypes, noteCards: noteCards)
    }

    private func state(_ command: BrowseCommand, _ facts: BrowseTargetFacts?, available: Bool = true) -> BrowseCommandState {
        BrowseCommandState.of(command, facts: facts, available: available)
    }

    @Test func theCommandsShowAndEnableAsV81Says() {
        let every: [BrowseCommand] = [.changeDeck, .addTags, .removeTags, .flag(0), .flag(3), .suspend, .unsuspend, .bury, .unbury,
                                      .setDueDate, .forget, .changeNoteType, .editNoteType, .findReplace, .findDuplicates, .delete,
                                      .editNote]
        // No selection: everything is disabled (Find and Replace and Find Duplicates need none, but wait for U277).
        for command in every { #expect(!state(command, nil).enabled, "\(command) with no selection") }
        // Three cards, none suspended or buried, one note type.
        let plain = facts(rows: 3)
        let built: Set<BrowseCommand> = [.flag(0), .flag(3), .suspend, .bury, .delete, .editNoteType]
        for command in every {
            #expect(state(command, plain).enabled == built.contains(command), "\(command) on three plain cards")
        }
        #expect(!state(.unsuspend, plain).shown && !state(.unbury, plain).shown)
        #expect(state(.suspend, plain).shown && state(.bury, plain).shown && state(.suspend, plain).count == 3)
        // Their sheets come later: shown, disabled.
        for later: BrowseCommand in [.changeDeck, .addTags, .removeTags, .setDueDate, .forget, .changeNoteType, .findReplace,
                                     .findDuplicates, .editNote] {
            #expect(state(later, plain).shown && !state(later, plain).enabled, "\(later) waits for its sheet")
        }
        // All suspended: Unsuspend with its count, no Suspend; a mix shows both.
        let suspended = facts(rows: 3, suspended: 3)
        #expect(!state(.suspend, suspended).shown && state(.unsuspend, suspended).enabled && state(.unsuspend, suspended).count == 3)
        let mix = facts(rows: 3, suspended: 1, buried: 2)
        #expect(state(.suspend, mix).enabled && state(.unsuspend, mix).enabled && state(.unsuspend, mix).count == 1)
        #expect(state(.bury, mix).enabled && state(.unbury, mix).enabled && state(.unbury, mix).count == 2)
        // Notes mode: the target cards are the notes' cards, and Suspend counts them.
        let notes = facts(rows: 2, mode: .notes, cards: 5, notes: 2, suspended: 0)
        #expect(state(.suspend, notes).count == 5 && state(.delete, notes).count == 2)
        // Two note types: Change Note Type and Edit Note Type are not shown.
        let two = facts(rows: 2, notetypes: [7, 8])
        #expect(!state(.changeNoteType, two).shown && !state(.editNoteType, two).shown)
        // Counts not known yet: both of a pair show, and Edit Note Type waits.
        let unknown = BrowseTargetFacts(mode: .notes, rows: 2)
        #expect(state(.suspend, unknown).shown && state(.unsuspend, unknown).shown && state(.unsuspend, unknown).count == nil)
        #expect(state(.editNoteType, unknown).shown && !state(.editNoteType, unknown).enabled)
        // A list that cannot take a command (released, a command running): nothing is enabled.
        for command in every { #expect(!state(command, plain, available: false).enabled) }
    }

    @Test func deleteAsksForSeveralNotesOrOneNoteWithOtherCards() {
        #expect(!state(.delete, facts(rows: 1, noteCards: 1)).asks)
        #expect(state(.delete, facts(rows: 1, noteCards: 3)).asks)
        #expect(state(.delete, facts(rows: 2, noteCards: nil)).asks)
        // Two cards of one note in Cards mode: one note.
        #expect(state(.delete, facts(rows: 2, notes: 1, noteCards: 2)).asks)
        #expect(state(.delete, facts(rows: 2, notes: 1, noteCards: 2)).count == 1)
    }

    @Test func undoStepsTakeTheCommandsDisplayNames() {
        #expect(BrowseStepNames.display(.suspend, count: 1) == "Suspend Card")
        #expect(BrowseStepNames.display(.suspend, count: 12) == "Suspend Cards")
        #expect(BrowseStepNames.display(.unsuspend, count: 1) == "Unsuspend Card")
        #expect(BrowseStepNames.display(.unbury, count: 4) == "Unbury Cards")
        #expect(BrowseStepNames.display(.bury, count: nil) == "Bury Cards")
        #expect(BrowseStepNames.display(.flag(3), count: nil) == "Flag")
        #expect(BrowseStepNames.display(.delete, count: 1) == "Delete Note")
        #expect(BrowseStepNames.display(.delete, count: 3) == "Delete Notes")
    }

    // MARK: Running on the real rows

    private func browse(_ cards: [Int64] = [1, 2, 3, 4]) async throws -> (BrowseModel, FakeSearchSession) {
        let backend = FakeBrowseBackend(), session = FakeSearchSession()
        session.results[""] = cards
        backend.searchSession = session
        let model = BrowseModel(backend: backend)
        model.results.progressDelay = .seconds(60)
        model.results.run(FilterQuery(), mode: .cards)
        await until { model.results.list?.count == cards.count && model.results.row(at: min(cards.count, 200) - 1) != nil }
        return (model, session)
    }

    private func select(_ model: BrowseModel, _ rows: Set<Int>) async {
        model.results.select(rows)
        await until { model.commands.facts?.rows == rows.count && model.commands.facts?.notetypes != nil }
    }

    @Test func suspendActsOnTheSelectedRowsAtTheirRevisionAndTitlesItsUndoStep() async throws {
        let (model, session) = try await browse()
        var steps: [(UInt64, String)] = []
        model.commands.stepRecorded = { steps.append(($0, $1)) }
        await select(model, [1, 2])
        let handle = try #require(model.results.list?.handle)
        #expect(model.commands.state(.suspend).enabled && model.commands.state(.suspend).count == 2)
        model.commands.perform(.suspend)
        await until { !model.commands.busy && model.commands.announcement != nil }
        #expect(session.actions == [.init(card: .suspend, note: nil, target: BrowseTarget(handle: handle, revision: 0, ranges: [1..<3],
                                                                                           mode: .cards))])
        #expect(steps.map(\.1) == ["Suspend Cards"] && steps.first?.0 == 500)
        #expect(model.commands.announcement?.kind == .suspended && model.commands.announcement?.count == 2)
        // The rows' counts are read again: now Unsuspend shows, with its count, and Suspend does not.
        await until { model.commands.state(.unsuspend).enabled }
        #expect(model.commands.state(.unsuspend).count == 2 && !model.commands.state(.suspend).shown)
        // Unsuspend acts on the suspended targets only (the core's Unsuspend): its step is titled after it.
        model.commands.perform(.unsuspend)
        await until { steps.count == 2 }
        #expect(steps.last?.1 == "Unsuspend Cards" && session.suspendedCards.isEmpty)
    }

    @Test func aContextMenuActsOnTheRowsItWasOpenedOnNotTheSelection() async throws {
        let (model, session) = try await browse()
        await select(model, [0])
        let handle = try #require(model.results.list?.handle)
        // A secondary click on row 3 alone: its own counts, read from the row on screen.
        #expect(model.commands.state(.bury, rows: [3]).count == 1)
        model.commands.perform(.bury, rows: [3])
        await until { session.actions.count == 1 && !model.commands.busy }
        #expect(session.actions.first?.target == BrowseTarget(handle: handle, revision: 0, ranges: [3..<4], mode: .cards))
        #expect(session.buriedCards == [4] && model.results.selection == [0])
        model.commands.perform(.flag(5), rows: [0, 1])
        await until { session.actions.count == 2 }
        #expect(session.actions.last == .init(card: .setFlag(5), note: nil,
                                              target: BrowseTarget(handle: handle, revision: 0, ranges: [0..<2], mode: .cards)))
    }

    @Test func aContextMenusRowsAreTakenWhenItIsDrawnAndRefusedWhenTheListMovedOn() async throws {
        let (model, session) = try await browse()
        await select(model, [0])
        // The menu is drawn on row 2; then a prune moves the core's handle on.
        let target = try #require(model.commands.target(rows: [2]))
        let handle = try #require(model.results.list?.handle)
        session.revisions[handle.rawValue] = 1
        model.commands.perform(.suspend, on: target)
        await until { model.commands.failed == .suspend }
        // Nothing acted on the row now at index 2, and the selection was not taken instead.
        #expect(session.actions.isEmpty && session.suspendedCards.isEmpty)
    }

    @Test func unsuspendOnAMenusRowsWithNoneSuspendedDoesNothing() async throws {
        let (model, session) = try await browse()
        model.setMode(.notes)
        await until { model.results.list?.mode == .notes && model.results.row(at: 0) != nil }
        await select(model, [0])
        // Notes mode: the rows on screen do not tell; both of the pair show for another row.
        let target = try #require(model.commands.target(rows: [1]))
        #expect(model.commands.state(.unsuspend, on: target).shown)
        model.commands.perform(.unsuspend, on: target)
        try await Task.sleep(for: .milliseconds(50))
        #expect(session.actions.isEmpty && model.commands.failed == nil && !model.commands.busy)
    }

    @Test func rowsTheListNoLongerHasAtThatRevisionAreRefusedAndNothingIsWritten() async throws {
        let (model, session) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        await select(model, [2])
        // The core's handle moved on (a prune the table has not applied yet): K1 refuses the rows.
        let handle = try #require(model.results.list?.handle)
        session.revisions[handle.rawValue] = 1
        model.commands.perform(.suspend)
        await until { model.commands.failed == .suspend }
        #expect(session.actions.isEmpty && session.suspendedCards.isEmpty && steps.isEmpty)
        #expect(model.commands.announcement == nil)
    }

    @Test func noCommandRunsOnARowsListThatWasReleased() async throws {
        let (model, session) = try await browse()
        await select(model, [0])
        // A search that fails after its rows showed keeps them dimmed (R1108): no handle can resolve them.
        session.failNext = .unavailable(reason: "sample")
        model.results.run(FilterQuery(text: "zindro"), mode: .cards)
        await until { model.results.isDimmed }
        #expect(model.results.target() == nil)
        #expect(!model.commands.state(.suspend).enabled && !model.commands.state(.delete).enabled)
        model.commands.perform(.suspend)
        try await Task.sleep(for: .milliseconds(20))
        #expect(session.actions.isEmpty)
    }

    // MARK: Delete (V8.2)

    @Test func oneNoteWithOneCardIsDeletedAtOnce() async throws {
        let (model, session) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        session.cardsOfNote = [10: [1]]
        await select(model, [0])
        #expect(BrowseCommandState.of(.delete, facts: model.commands.facts, available: true).asks == false)
        model.commands.perform(.delete)
        await until { session.actions.count == 1 && !model.commands.busy }
        #expect(model.commands.deleteQuestion == nil && session.actions.first?.note == .deleteNotes)
        #expect(steps == ["Delete Note"] && model.commands.announcement?.kind == .deleted)
    }

    @Test func oneNoteWithOtherCardsAsksAndRunsOnTheRowsItAskedAbout() async throws {
        let (model, session) = try await browse()
        session.cardsOfNote = [10: [1, 7, 8]]
        await select(model, [0])
        await until { model.commands.state(.delete).asks }
        model.commands.perform(.delete)
        await until { model.commands.deleteQuestion != nil }
        let question = try #require(model.commands.deleteQuestion)
        #expect(question.notes == 1 && question.cards == 3 && question.sortField == "noteFld 1" && session.actions.isEmpty)
        // A Mac alert is modal on its window; the delete still runs on the rows it asked about.
        model.results.select([3])
        model.commands.confirmDelete(question)
        await until { session.actions.count == 1 && !model.commands.busy }
        #expect(session.actions.first?.target.ranges == [0..<1] && session.actions.first?.note == .deleteNotes)
    }

    @Test func severalNotesAskAndCancelChangesNothing() async throws {
        let (model, session) = try await browse()
        await select(model, [0, 1, 2])
        model.commands.perform(.delete)
        await until { model.commands.deleteQuestion != nil }
        #expect(model.commands.deleteQuestion?.notes == 3 && model.commands.deleteQuestion?.sortField == nil)
        // While the question shows no other command runs.
        #expect(!model.commands.state(.suspend).enabled)
        model.commands.cancelDelete()
        #expect(model.commands.deleteQuestion == nil && session.actions.isEmpty)
        #expect(model.commands.state(.suspend).enabled)
    }

    @Test func undoOfADeleteBringsItsRowsBackSelected() async throws {
        let (model, session) = try await browse()
        let results = model.results
        await select(model, [1])
        // The delete's change set removes card 2's row (H10)...
        session.invalidation = BrowseInvalidation(removed: [1], refetch: .rows([]), revision: 1, isComplete: true, count: 3)
        results.changesCommitted(BrowseResultsTests.changes(removing: 2), seq: 7)
        await until { results.list?.count == 3 }
        session.invalidation = nil
        let starts = session.starts.count
        // ...and its undo adds it back: the search runs again, with it selected.
        results.changesCommitted(Self.changes(adding: 2), seq: 8)
        await until { session.starts.count == starts + 1 && results.list?.count == 4 }
        await until { results.selection == [1] }
        #expect(results.row(at: 1)?.id == 2 && results.currentIndex == 1)
    }

    // MARK: Selection commands (V7)

    @Test func selectAllAndSelectSiblingCards() async throws {
        let (model, session) = try await browse()
        session.notesOf = [1: 10, 2: 10, 3: 30, 4: 10]
        await select(model, [0])
        #expect(model.commands.canSelectSiblings)
        model.commands.selectSiblings()
        await until { model.results.selection == [0, 1, 3] }
        #expect(model.results.currentIndex == 0)
        model.commands.selectAll()
        #expect(model.results.selection == [0, 1, 2, 3])
    }

    @Test func selectSiblingCardsIsForCardsMode() async throws {
        let (model, _) = try await browse()
        model.setMode(.notes)
        await until { model.results.list?.mode == .notes }
        model.results.select([0])
        #expect(!model.commands.canSelectSiblings)
    }

    // MARK: Kept scroll (V1)

    @Test func theTableComesBackToTheRowsItShowedWhenBrowseWasLeft() async throws {
        let (model, _) = try await browse(Array(1...1_000))
        let results = model.results
        #expect(results.tableAppeared() == nil)
        results.tableSettled()
        for index in 300...340 { results.rowAppeared(index) }
        for index in 300..<310 { results.rowDisappeared(index) }
        results.rowAppeared(341)
        // Leaving Browse: every row goes; that does not move the row kept.
        for index in 310...341 { results.rowDisappeared(index) }
        results.tableDisappeared()
        // Coming back: the table first draws its first rows, then is brought back.
        for index in 0...30 { results.rowAppeared(index) }
        #expect(results.tableAppeared() == 325)
    }

    // MARK: Saved searches (T13)

    @Test func savedSearchesAreRenamedAndDeleted() async throws {
        let backend = FakeBrowseBackend()
        let model = BrowseModel(backend: backend)
        model.appear()
        await until { model.savedSearches.map(\.name) == ["Hard Ones"] }
        model.startRenaming("Hard Ones")
        #expect(model.renamingSearch?.old == "Hard Ones")
        // K7.2: quotes are dropped; the same name changes nothing.
        model.rename(to: "Hard Ones")
        #expect(model.renamingSearch == nil && backend.renames.isEmpty)
        model.startRenaming("Hard Ones")
        model.rename(to: "  Easy \"Ones\" ")
        await until { backend.renames.count == 1 }
        #expect(backend.renames.first?.0 == "Hard Ones" && backend.renames.first?.1 == "Easy Ones")
        model.deleteSavedSearch("Hard Ones")
        await until { backend.deletedSearches == ["Hard Ones"] }
        // A built-in saved search is not a user's: neither command takes it.
        model.startRenaming("Due Today")
        model.deleteSavedSearch("Due Today")
        try await Task.sleep(for: .milliseconds(20))
        #expect(model.renamingSearch == nil && backend.deletedSearches == ["Hard Ones"])
    }

    @Test func renamingToAnotherSavedSearchsNameAsksFirst() async throws {
        let backend = FakeBrowseBackend()
        backend.extraSaved = [.savedSearch(name: "Other", text: "tag:x")]
        let model = BrowseModel(backend: backend)
        model.appear()
        await until { model.savedSearches.count == 2 }
        model.startRenaming("Hard Ones")
        model.rename(to: "Other")
        #expect(model.renamingSearch?.replacing == "Other" && backend.renames.isEmpty)
        model.keepRenaming()
        #expect(model.renamingSearch?.replacing == nil && model.renamingSearch?.old == "Hard Ones")
        model.rename(to: "Other")
        model.rename(to: "Other")
        await until { backend.renames.count == 1 }
        #expect(model.renamingSearch == nil)
    }

    static func changes(adding card: Int64) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .userUndoable,
                  cards: [ChangeSet.Card(id: card, noteID: card * 10, change: .added, memoryStateOnly: false)],
                  notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [],
                  revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                  skippedIncomingDecks: [], collectionRow: false)
    }
}
