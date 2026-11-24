@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// U276's command sheets (`browser/workspace` V8.1, V8.2, V8.3; N21.6)
/// against a scripted session: what each sheet lists and checks, when its
/// default button is enabled, the exact action it sends on the rows it
/// opened on, the refusal of a list that moved on (nothing written), Cancel
/// before and during the run, the undo step's title and VoiceOver's result.
@MainActor @Suite struct BrowseSheetsTests {
    private func browse(_ cards: [Int64] = [1, 2, 3, 4]) async throws -> (BrowseModel, FakeSearchSession, FakeSheetReads) {
        let backend = FakeBrowseBackend(), session = FakeSearchSession(), reads = FakeSheetReads()
        session.results[""] = cards
        backend.searchSession = session
        let model = BrowseModel(backend: backend, sheetReads: reads)
        model.results.progressDelay = .seconds(60)
        model.results.run(FilterQuery(), mode: .cards)
        await until { model.results.list?.count == cards.count && model.results.row(at: cards.count - 1) != nil }
        return (model, session, reads)
    }

    private func select(_ model: BrowseModel, _ rows: Set<Int>) async {
        model.results.select(rows)
        await until { model.commands.facts?.rows == rows.count && model.commands.facts?.notetypes != nil }
    }

    private func open(_ command: BrowseCommand, _ model: BrowseModel) async throws -> BrowseSheet {
        model.commands.perform(command)
        let sheet = try #require(model.commands.sheet)
        await until { sheet.counts != nil }
        return sheet
    }

    // MARK: Change Deck

    @Test func changeDeckListsNormalDecksMarksTheCurrentOneAndCreatesFromTheTypedName() async throws {
        let (model, session, reads) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        session.deckIDs = [3]
        await select(model, [1, 2])
        let handle = try #require(model.results.list?.handle)
        #expect(model.commands.state(.changeDeck).enabled)
        let sheet = try await open(.changeDeck, model)
        #expect(sheet.kind == .changeDeck)
        #expect(sheet.target == BrowseTarget(handle: handle, revision: 0, ranges: [1..<3], mode: .cards))
        await until { sheet.decks != nil }
        // Filtered decks are never targets (B7.1); the default deck is left out while no card is in it (H6.1).
        #expect(sheet.deckRows.map(\.id) == [2, 3, 5])
        #expect(sheet.deckRows.map(\.current) == [false, true, false])
        #expect(sheet.deckRows[1].name == "Spanish" && sheet.deckRows[1].parents == ["Lang"])
        // With no text the selection's own deck is highlighted; ↑ and ↓ wrap.
        #expect(sheet.highlightedDeck == .deck(3))
        sheet.moveHighlight(by: 1)
        #expect(sheet.highlightedDeck == .deck(5))
        sheet.moveHighlight(by: 1)
        #expect(sheet.highlightedDeck == .deck(2))
        sheet.moveHighlight(by: -1)
        #expect(sheet.highlightedDeck == .deck(5))
        // Typing filters (H6.2) and highlights the first row again.
        sheet.deckQuery = "lang sp"
        #expect(sheet.deckRows.map(\.id) == [3] && sheet.highlightedDeck == .deck(3))
        // Other commands wait while the sheet is open.
        #expect(!model.commands.state(.suspend).enabled)
        // No deck has the typed name: the create row, once D18's read says so.
        sheet.deckQuery = "Lang::Verbs"
        #expect(sheet.deckChoices.isEmpty && !sheet.canConfirm)
        await until { sheet.offersCreate }
        #expect(sheet.highlightedDeck == .create && sheet.canConfirm)
        #expect(reads.matched.last == "Lang::Verbs")
        sheet.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions == [.init(card: .changeDeckToName("Lang::Verbs"), note: nil,
                                          target: BrowseTarget(handle: handle, revision: 0, ranges: [1..<3], mode: .cards))])
        #expect(steps == ["Change Deck"])
        #expect(model.commands.announcement?.kind == .moved && model.commands.announcement?.count == 2)
    }

    @Test func changeDeckToAnExistingDeckByReturnOrADoubleClick() async throws {
        let (model, session, reads) = try await browse()
        reads.existing = ["Spanish"]
        await select(model, [0])
        let sheet = try await open(.changeDeck, model)
        await until { sheet.decks != nil }
        sheet.deckQuery = "Spanish"
        // A deck has the name: no create row.
        try await Task.sleep(for: .milliseconds(50))
        #expect(!sheet.offersCreate && sheet.highlightedDeck == .deck(3))
        // A double click on a listed row chooses it.
        sheet.deckQuery = ""
        sheet.choose(.deck(5))
        await until { model.commands.sheet == nil }
        #expect(session.actions.map(\.card) == [.changeDeck(deck: 5)])
    }

    // MARK: The rows the sheet opened on

    @Test func aSheetOnAListThatMovedOnIsRefusedAndWritesNothing() async throws {
        let (model, session, _) = try await browse()
        await select(model, [0, 1])
        let handle = try #require(model.results.list?.handle)
        let sheet = try await open(.forget, model)
        // The core's list moved on while the sheet was open (a change pruned rows elsewhere).
        session.revisions[handle.rawValue] = 1
        sheet.confirm()
        await until { sheet.failure != nil }
        #expect(sheet.failure == .rowsChanged && sheet.phase == .editing)
        #expect(session.actions.isEmpty && model.commands.sheet === sheet)
        // It never tries other rows: a second confirm is refused the same way.
        sheet.confirm()
        await until { sheet.failure != nil && !sheet.isRunning }
        #expect(session.actions.isEmpty)
        sheet.cancel()
        #expect(model.commands.sheet == nil)
    }

    @Test func aContextMenusSheetActsOnItsOwnRowsNotTheSelection() async throws {
        let (model, session, _) = try await browse()
        await select(model, [0])
        let handle = try #require(model.results.list?.handle)
        let target = try #require(model.commands.target(rows: [2, 3]))
        model.commands.perform(.setDueDate, on: target)
        let sheet = try #require(model.commands.sheet)
        // The selection changes meanwhile: the sheet keeps its rows.
        model.results.select([1])
        await until { if case .valid = sheet.due { true } else { false } }
        sheet.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions == [.init(card: .setDueDate("0"), note: nil,
                                          target: BrowseTarget(handle: handle, revision: 0, ranges: [2..<4], mode: .cards))])
    }

    // MARK: Tags

    @Test func addAndRemoveTagsSendTheTokensAsOneTextAndNeedOne() async throws {
        let (model, session, _) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        session.noteTags = ["alpha", "Beta::gamma"]
        await select(model, [0, 1])
        let add = try await open(.addTags, model)
        #expect(!add.canConfirm)
        add.confirm()
        #expect(session.actions.isEmpty, "B9: the entry never sends a text that gives no tag")
        // Typed text not yet a token enables the button; the field makes it a token first.
        add.typingTag = true
        #expect(add.canConfirm)
        add.tags = ["zeta", "qzfern::qzleaf", "zeta", "two words"]
        #expect(add.tagList == ["zeta", "qzfern::qzleaf", "two", "words"])
        add.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions.last?.note == .addTags("zeta qzfern::qzleaf two words"))
        #expect(model.commands.announcement?.kind == .tagsChanged)

        await until { model.commands.state(.removeTags).enabled }
        let remove = try await open(.removeTags, model)
        // Remove suggests only the target notes' own tags (V8.1).
        await until { remove.selectionTags != nil }
        #expect(remove.selectionTags == ["alpha", "Beta::gamma"])
        remove.tags = ["Beta::gamma"]
        remove.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions.last?.note == .removeTags("Beta::gamma"))
        #expect(steps == ["Add Tags", "Remove Tags"])
    }

    // MARK: Set Due Date

    @Test func setDueDatePrefillsChecksAndSendsTheTextAsTyped() async throws {
        let (model, session, reads) = try await browse()
        reads.defaults = BrowseSheetDefaults(setDueDays: "5-2!", restorePosition: true, resetCounts: false)
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        // The remembered text read ahead, as Browse's appear does.
        model.commands.preloadSheetDefaults()
        try await Task.sleep(for: .milliseconds(20))
        await select(model, [0, 1, 2])
        let sheet = try await open(.setDueDate, model)
        #expect(sheet.dueText == "5-2!")
        await until { sheet.due == .valid(DueDays(minimum: 2, maximum: 5, resetInterval: true)) }
        #expect(sheet.canConfirm)
        sheet.dueText = "3--4"
        await until { sheet.due == .invalid(.malformed) }
        #expect(!sheet.canConfirm)
        sheet.dueText = "4294967296"
        await until { sheet.due == .invalid(.tooLarge) }
        sheet.dueText = ""
        #expect(sheet.due == .invalid(.empty) && !sheet.canConfirm)
        sheet.dueText = "0"
        await until { sheet.due == .valid(DueDays(minimum: 0, maximum: 0, resetInterval: false)) }
        sheet.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions.map(\.card) == [.setDueDate("0")])
        #expect(steps == ["Set Due Date"] && model.commands.announcement?.kind == .rescheduled)
    }

    @Test func aLatePrefillNeverReplacesWhatThePersonTyped() async throws {
        let (model, _, reads) = try await browse()
        reads.defaults = BrowseSheetDefaults(setDueDays: "9", restorePosition: true, resetCounts: false)
        reads.holdDefaults = true
        await select(model, [0])
        let sheet = try await open(.setDueDate, model)
        // Before the remembered text arrives the field shows no refusal.
        #expect(sheet.due == .checking && !sheet.canConfirm)
        sheet.dueText = "4"
        reads.releaseDefaults()
        await until { if case .valid = sheet.due { true } else { false } }
        try await Task.sleep(for: .milliseconds(30))
        #expect(sheet.dueText == "4")
    }

    @Test func aFailedDefaultsReadFillsTheKeysOwnDefaults() async throws {
        let (model, _, reads) = try await browse()
        reads.defaultsFail = true
        await select(model, [0])
        let sheet = try await open(.setDueDate, model)
        await until { sheet.dueText == "0" }
        await until { sheet.canConfirm }
    }

    @Test func aResetWhileACommandRunsCancelsIt() async throws {
        let (model, session, _) = try await browse()
        session.holdsActions = true
        await select(model, [0])
        let sheet = try await open(.forget, model)
        sheet.confirm()
        await until { session.heldAction.started == 1 }
        try await Task.sleep(for: .milliseconds(20))
        model.commands.reset()
        await until { session.heldAction.cancelled }
        #expect(model.commands.sheet == nil && session.actions.isEmpty)
    }

    @Test func defaultsReadBeforeTheCollectionWentAreDropped() async throws {
        let (model, _, reads) = try await browse()
        reads.holdDefaults = true
        await select(model, [0])
        let sheet = try await open(.setDueDate, model)
        model.commands.reset()
        reads.releaseDefaults()
        try await Task.sleep(for: .milliseconds(30))
        #expect(sheet.dueText.isEmpty && model.commands.sheet == nil)
    }

    @Test func theOwnDeckHighlightComesOnceAndNeverMovesUnderThePerson() async throws {
        let (model, session, _) = try await browse()
        session.deckIDs = [5]
        await select(model, [0])
        let sheet = try await open(.changeDeck, model)
        await until { sheet.decks != nil }
        #expect(sheet.highlightedDeck == .deck(5))
        sheet.moveHighlight(by: 1)
        #expect(sheet.highlightedDeck == .deck(2))
        // Counts read again (the selection's checkmarks) leave the choice alone.
        sheet.counts = sheet.counts
        #expect(sheet.highlightedDeck == .deck(2))
    }

    // MARK: Forget

    @Test func forgetSendsItsTwoOptionsAndCancelSendsNothing() async throws {
        let (model, session, reads) = try await browse()
        reads.defaults = BrowseSheetDefaults(setDueDays: "0", restorePosition: false, resetCounts: false)
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        await select(model, [3])
        let first = try await open(.forget, model)
        // The defaults arrive after the sheet opened: they fill it.
        await until { first.restorePosition == false }
        first.cancel()
        #expect(model.commands.sheet == nil && session.actions.isEmpty)
        let sheet = try await open(.forget, model)
        #expect(sheet.restorePosition == false && sheet.resetCounts == false)
        sheet.restorePosition = true
        sheet.resetCounts = true
        sheet.confirm()
        await until { model.commands.sheet == nil }
        #expect(session.actions.map(\.card) == [.forget(restorePosition: true, resetCounts: true)])
        #expect(steps == ["Forget"] && model.commands.announcement?.kind == .forgotten)
    }

    // MARK: Progress and Cancel

    @Test func aRunningCommandShowsItsProgressAndCancelStopsIt() async throws {
        let (model, session, _) = try await browse()
        session.holdsActions = true
        await select(model, [0, 1])
        let sheet = try await open(.forget, model)
        sheet.confirm()
        await until { sheet.phase == .running(BrowseActionProgress(completed: 1, total: 4, cancellable: true)) }
        #expect(!sheet.canConfirm && sheet.canCancel)
        sheet.cancel()
        await until { model.commands.sheet == nil }
        #expect(session.heldAction.cancelled && session.actions.isEmpty, "a cancelled run writes nothing")
        // A run let through ends as usual.
        let next = try await open(.forget, model)
        next.confirm()
        await until { session.heldAction.started == 2 }
        session.heldAction.release()
        await until { model.commands.sheet == nil }
        #expect(session.actions.count == 1)
    }

    @Test func aFailedCommandKeepsTheSheetAndSaysNothingChanged() async throws {
        let (model, session, _) = try await browse()
        session.actionFails = true
        await select(model, [0])
        let sheet = try await open(.setDueDate, model)
        await until { if case .valid = sheet.due { true } else { false } }
        sheet.confirm()
        await until { sheet.failure != nil }
        #expect(sheet.failure == .failed && model.commands.sheet === sheet && sheet.canConfirm)
    }

    @Test func refusalsReadAsTheSheetSaysThem() {
        #expect(BrowseSheet.failure(BridgeError.invalidArgument("the selection cannot be resolved")) == .rowsChanged)
        #expect(BrowseSheet.failure(BridgeError.browse(.staleRevision(current: 2))) == .rowsChanged)
        #expect(BrowseSheet.failure(BridgeError.deckRefused(.filteredDeck)) == .deck(.filteredDeck))
        #expect(BrowseSheet.failure(BridgeError.schedOpRefused(.invalidDays(.malformed))) == .invalidDays(.malformed))
        #expect(BrowseSheet.failure(BridgeError.cancelled) == nil)
        #expect(BrowseSheet.failure(BridgeError.unavailable(reason: "x")) == .failed)
    }
}

/// The sheets' reads, scripted: decks 1 Default, 2 Lang, 3 Lang::Spanish,
/// 4 Cram (filtered) and 5 Zeta; the default deck holds no card.
@MainActor final class FakeSheetReads: BrowseSheetReads {
    var defaults = BrowseSheetDefaults(setDueDays: "0", restorePosition: true, resetCounts: false)
    /// `sheetDefaults()` waits for `releaseDefaults()`.
    var holdDefaults = false
    private var heldDefaults: [CheckedContinuation<Void, Never>] = []
    func releaseDefaults() {
        holdDefaults = false
        heldDefaults.forEach { $0.resume() }
        heldDefaults = []
    }
    /// Names `matchDeckName` finds a deck for (compared ignoring case).
    var existing: [String] = []
    private(set) var matched: [String] = []

    func deckNames() async throws -> DeckNameSnapshot {
        DeckNameSnapshot(generation: 1, decks: [
            DeckNameEntry(id: 1, fullName: "Default", filtered: false),
            DeckNameEntry(id: 2, fullName: "Lang", filtered: false),
            DeckNameEntry(id: 3, fullName: "Lang::Spanish", filtered: false),
            DeckNameEntry(id: 4, fullName: "Cram", filtered: true),
            DeckNameEntry(id: 5, fullName: "Zeta", filtered: false),
        ])
    }

    func matchDeckName(_ name: String) async throws -> DeckNameMatch {
        matched.append(name)
        let found = existing.first { $0.lowercased() == name.lowercased() }
        return DeckNameMatch(generation: 1, deck: found.map { DeckNameEntry(id: 3, fullName: $0, filtered: false) })
    }

    func defaultDeckHasCards() async throws -> Bool { false }
    var defaultsFail = false
    func sheetDefaults() async throws -> BrowseSheetDefaults {
        if defaultsFail { throw BridgeError.unavailable(reason: "sample") }
        if holdDefaults { await withCheckedContinuation { heldDefaults.append($0) } }
        return defaults
    }

    /// `sched/set-due-date` S2's grammar, enough for these tests.
    func parseDueDays(_ text: String) async throws -> DueDays {
        let reset = text.hasSuffix("!")
        let body = reset ? String(text.dropLast()) : text
        let parts = body.split(separator: "-", omittingEmptySubsequences: false)
        guard (1...2).contains(parts.count), parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy(\.isASCII) && $0.allSatisfy(\.isNumber) })
        else { throw BridgeError.schedOpRefused(.invalidDays(.malformed)) }
        let numbers = try parts.map { part -> UInt32 in
            guard let value = UInt32(part) else { throw BridgeError.schedOpRefused(.invalidDays(.tooLarge)) }
            return value
        }
        return DueDays(minimum: numbers.min() ?? 0, maximum: numbers.max() ?? 0, resetInterval: reset)
    }
}
