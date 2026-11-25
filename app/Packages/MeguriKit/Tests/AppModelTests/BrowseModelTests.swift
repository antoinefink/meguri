@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// Browse's section shell (U270): the sidebar's clicks (`search/filter-tokens`
/// T9), what the rows show selected, the counts (T12.4), the state kept while
/// the app runs (`browser/workspace` V1) and across launches (T14), and the
/// sidebar's filter (`browser/sidebar` S6), against a scripted core.
@MainActor @Suite struct BrowseModelTests {
    // MARK: T9 on each kind

    private static let spanish = FilterValue.deck(id: 10, native: "Languages\u{1F}Español", tree: [10, 11])
    private static let japanese = FilterValue.deck(id: 20, native: "Languages\u{1F}日本語", tree: [20])
    private static let vocabulary = FilterValue.notetype(id: 7, name: "Vocabulary")
    private static let basic = FilterValue.notetype(id: 8, name: "Basic")

    private func token(_ values: FilterValue..., negated: Bool = false) -> FilterToken {
        .value(values, negated: negated)
    }

    @Test func aClickMakesItsKindsTokenIsVAndKeepsEveryOtherTokenAndTheText() {
        let start = FilterQuery(tokens: [token(Self.spanish), token(.tag("brask"), .tag("zindro"), negated: true), .advanced("prop:ivl>3")],
                                text: "plimb")
        // Deck: replaced where it stood.
        #expect(start.clicking(Self.japanese).tokens == [token(Self.japanese), start.tokens[1], start.tokens[2]])
        // Tag: an is-not token with two values becomes is {v}.
        #expect(start.clicking(.tag("vimmet")).tokens == [start.tokens[0], token(.tag("vimmet")), start.tokens[2]])
        #expect(start.clicking(.untagged).tokens[1] == token(.untagged))
        // A kind with no token yet: is {v} after the others; the text stays.
        let state = start.clicking(.state(.suspended))
        #expect(state.tokens == start.tokens + [token(.state(.suspended))] && state.text == "plimb")
        #expect(start.clicking(Self.vocabulary).tokens.last == token(Self.vocabulary))
        #expect(start.clicking(.noFlag).tokens.last == token(.noFlag))
        #expect(start.clicking(.flag(3)).tokens.last == token(.flag(3)))
        // Never a second token of a kind (T2).
        let twice = start.clicking(.state(.new)).clicking(.state(.review))
        #expect(twice.tokens.filter { $0.kind == .state } == [token(.state(.review))])
    }

    @Test func aCommandClickAddsOrRemovesVInItsKindsTokenAndKeepsIsOrIsNot() {
        let start = FilterQuery(tokens: [token(Self.spanish), token(.tag("brask"), negated: true)])
        // Added, and the token's is not stays.
        #expect(start.commandClicking(.tag("zindro")).tokens[1] == token(.tag("brask"), .tag("zindro"), negated: true))
        #expect(start.commandClicking(Self.japanese).tokens[0] == token(Self.spanish, Self.japanese))
        // Present: removed; a deck is the same deck by its id, however it was read.
        let reread = FilterValue.deck(id: 10, native: "Languages\u{1F}Español", tree: [10])
        let removed = start.commandClicking(Self.japanese).commandClicking(reread)
        #expect(removed.tokens[0] == token(Self.japanese))
        // The last value removed takes the token with it.
        #expect(start.commandClicking(Self.spanish).tokens == [token(.tag("brask"), negated: true)])
        // No token of the kind: is {v} is made.
        #expect(start.commandClicking(.state(.buried)).tokens.last == token(.state(.buried)))
        #expect(start.commandClicking(Self.basic).commandClicking(Self.vocabulary).tokens.last == token(Self.basic, Self.vocabulary))
        #expect(start.commandClicking(.flag(1)).commandClicking(.noFlag).tokens.last == token(.flag(1), .noFlag))
    }

    @Test func aCommandClickOnNoFlagOrUntaggedIntoAnIsNotTokenIsAClick() {
        let flags = FilterQuery(tokens: [token(.flag(1), negated: true)])
        #expect(flags.commandClicking(.noFlag).tokens == [token(.noFlag)])
        let tags = FilterQuery(tokens: [token(.tag("brask"), negated: true)])
        #expect(tags.commandClicking(.untagged).tokens == [token(.untagged)])
        // Into an is token they add, as any value does.
        #expect(FilterQuery(tokens: [token(.flag(1))]).commandClicking(.noFlag).tokens == [token(.flag(1), .noFlag)])
    }

    @Test func anOptionClickMakesIsNotVWithTheFlagsNormalFormAndUntaggedAsAClick() {
        let start = FilterQuery(tokens: [token(Self.spanish, Self.japanese), token(.state(.new))], text: "x")
        #expect(start.optionClicking(Self.japanese).tokens[0] == token(Self.japanese, negated: true))
        #expect(start.optionClicking(.state(.suspended)).tokens[1] == token(.state(.suspended), negated: true))
        #expect(start.optionClicking(.tag("brask")).tokens.last == token(.tag("brask"), negated: true))
        #expect(start.optionClicking(Self.vocabulary).tokens.last == token(Self.vocabulary, negated: true))
        #expect(start.optionClicking(.flag(2)).tokens.last == token(.flag(2), negated: true))
        // No Flag ⇄ Any Flag (T10's normal form).
        #expect(start.optionClicking(.noFlag).tokens.last == token(.anyFlag))
        #expect(start.optionClicking(.anyFlag).tokens.last == token(.noFlag))
        // Untagged has no opposite: a click.
        let tagged = FilterQuery(tokens: [token(.tag("brask"), negated: true)])
        #expect(tagged.optionClicking(.untagged).tokens == [token(.untagged)])
        #expect(start.optionClicking(.state(.suspended)).text == "x")
    }

    @Test func aRowShowsSelectedExactlyWhenItsValueIsInAnIsToken() async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        model.activate(.state(.suspended))
        model.activate(.tag("brask"), click: .option)
        model.activate(.flag(0))
        #expect(model.selectedRows == [.deck(10), .state(.suspended), .flag(0)])
        // The list's own selection is the Browse row alone: the rows draw the query's.
        #expect(model.listSelection == [.section(.browse)])
        #expect(model.query.selects(Self.spanish) && !model.query.selects(.tag("brask")))
        // Removing a token deselects its rows.
        model.activate(.deck(10), click: .command)
        #expect(!model.selectedRows.contains(.deck(10)))
        // ⌥-click on No Flag: Any Flag, which no row shows.
        model.activate(.flag(0), click: .option)
        #expect(!model.selectedRows.contains(.flag(0)))
    }

    @Test func theListsSelectionIsReadAsTheClickThatMadeIt() async {
        let (model, _) = await makeModel()
        // A plain click: the one row the list selected.
        #expect(model.listSelectionChanged(to: [.state(.new)], click: .plain) == nil)
        #expect(model.query.tokens == [token(.state(.new))])
        // A click on another kind keeps the first token; both rows then show selected.
        _ = model.listSelectionChanged(to: [.deck(10)], click: .plain)
        #expect(model.selectedRows == [.state(.new), .deck(10)])
        // ⌘-click: a row is added to its kind when absent and removed when present.
        _ = model.listSelectionChanged(to: model.listSelection.union([.deck(20)]), click: .command)
        #expect(model.query.tokens[1] == token(Self.spanish, Self.japanese))
        _ = model.listSelectionChanged(to: model.listSelection.union([.deck(10)]), click: .command)
        #expect(model.query.tokens[1] == token(Self.japanese))
        // ⌥-click.
        _ = model.listSelectionChanged(to: [.state(.new)], click: .option)
        #expect(model.query.tokens[0] == token(.state(.new), negated: true))
        // A ⇧ range or an emptied selection changes nothing, and the list is drawn again.
        let before = model.query, echo = model.selectionEcho
        _ = model.listSelectionChanged(to: [.state(.learning), .state(.review), .state(.suspended)], click: .plain)
        _ = model.listSelectionChanged(to: [], click: .plain)
        #expect(model.query == before && model.selectionEcho == echo &+ 2)
        // A ⌘-click changes the clicked row only: the other kinds' rows stay.
        _ = model.listSelectionChanged(to: [.state(.learning)], click: .plain)
        _ = model.listSelectionChanged(to: model.listSelection.union([.state(.review)]), click: .command)
        #expect(model.selectedRows == [.state(.learning), .state(.review), .deck(20)])
        // A section row navigates; the Browse row itself does nothing.
        #expect(model.listSelectionChanged(to: [.section(.home)], click: .plain) == .home)
        let after = model.query
        #expect(model.listSelectionChanged(to: [.section(.browse)], click: .plain) == nil)
        #expect(model.query == after)
    }

    @Test(arguments: [BrowseModel.Click.plain, .command, .option], [false, true])
    func sectionNavigationDoesNotChangeFilters(_ click: BrowseModel.Click, mixed: Bool) async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        model.activate(.tag("brask::oddly"))
        let query = model.query, selection = model.listSelection, echo = model.selectionEcho
        let rows: Set<BrowseSidebarTag> = mixed
            ? [.section(.home), .section(.browse), .deck(20), .tag("brask"), .showAllTags]
            : [.section(.home)]
        #expect(model.listSelectionChanged(to: rows, click: click) == .home)
        #expect(model.query == query && model.listSelection == selection)
        #expect(!model.showsAllTags && model.selectionEcho == echo &+ 1)
    }

    @Test(arguments: [BrowseModel.Click.plain, .option])
    func aSingleFilterClickBesideTheBrowseRowIsAClickOnIt(_ click: BrowseModel.Click) async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        #expect(model.listSelectionChanged(to: [.section(.browse), .tag("brask")], click: click) == nil)
        #expect(model.query.tokens == [token(Self.spanish), token(.tag("brask"), negated: click == .option)])
    }

    /// N21.1 (2026-11-24): the list's selection is the Browse row alone, so a
    /// ⌘-click reports the clicked row only, and a selected row the list
    /// does not draw (filtered out, under a collapsed parent) stays selected.
    @Test func aCommandClickKeepsHiddenDeckAndTagSelections() async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        model.activate(.tag("brask"))
        model.activate(.tag("brask::oddly"), click: .command)
        model.activate(.state(.new))
        // ⌘-clicks on deck 20 (not selected) and on the parent tag (selected).
        #expect(model.listSelectionChanged(to: [.section(.browse), .deck(20)], click: .command) == nil)
        #expect(model.listSelectionChanged(to: [.section(.browse), .tag("brask")], click: .command) == nil)
        #expect(model.selectedRows == [.deck(10), .deck(20), .tag("brask::oddly"), .state(.new)])
    }

    /// Decided 2026-11-23 (T9): a click on the row that is its kind's whole
    /// token removes it; on a saved search the query holds, it clears it.
    @Test func aSecondClickOnASelectedRowRemovesItsFilter() async {
        let (model, core) = await makeModel()
        model.activate(.state(.new))
        model.activate(.deck(10))
        #expect(model.selectedRows == [.state(.new), .deck(10)])
        // The list's selection after a plain click on a selected row is that row alone.
        _ = model.listSelectionChanged(to: [.state(.new)], click: .plain)
        #expect(model.query.tokens == [token(Self.spanish)] && model.selectedRows == [.deck(10)])
        // Every kind: decks, note types, tags, flags.
        model.activate(.deck(10))
        model.activate(.notetype(7)); model.activate(.notetype(7))
        model.activate(.tag("brask")); model.activate(.tag("brask"))
        model.activate(.flag(2)); model.activate(.flag(2))
        model.activate(.flag(0)); model.activate(.flag(0))
        #expect(model.query.tokens.isEmpty)
        // Return or Space on the list (T9): the one selected filter row, when that removes it.
        model.activate(.state(.new))
        #expect(!model.activateByKey([.section(.browse), .state(.new), .deck(10)]))
        #expect(!model.activateByKey([.section(.browse)]))
        #expect(model.activateByKey([.section(.browse), .state(.new)]) && model.query.tokens.isEmpty)
        // VoiceOver's hint says a click removes the filter only where it does.
        model.activate(.deck(10)); model.activate(.deck(20), click: .command)
        #expect(!model.activationRemoves(.deck(10)) && !model.activationRemoves(.state(.new)))
        model.activate(.deck(20), click: .command)
        #expect(model.activationRemoves(.deck(10)))
        model.activate(.deck(10))
        // A token holding two values, or an is-not token: a click makes it is {v}.
        model.activate(.deck(10)); model.activate(.deck(20), click: .command)
        model.activate(.deck(10))
        #expect(model.query.tokens == [token(Self.spanish)])
        model.activate(.state(.new), click: .option)
        model.activate(.state(.new))
        #expect(model.query.tokens.last == token(.state(.new)))
        // Saved searches: the second click empties the query they set.
        model.activate(.builtIn(.dueToday))
        #expect(model.selectedRows == [.builtIn(.dueToday)])
        model.activate(.builtIn(.dueToday))
        #expect(model.query == FilterQuery())
        core.reads["is:due -tag:x"] = FilterQuery(tokens: [.advanced("is:due"), token(.tag("x"), negated: true)])
        model.activate(.saved("Hard Ones"))
        await until { model.selectedRows == [.saved("Hard Ones")] }
        model.activate(.saved("Hard Ones"))
        #expect(model.query == FilterQuery() && model.selectedRows.isEmpty)
    }

    /// The rows follow the field both ways: a token edited or removed in
    /// the field deselects at once; a saved search shows selected only while
    /// the query equals it.
    @Test func theSidebarFollowsTheTokensInTheField() async {
        let (model, core) = await makeModel()
        core.reads["is:due -tag:x"] = FilterQuery(tokens: [.advanced("is:due"), token(.tag("x"), negated: true)])
        model.activate(.saved("Hard Ones"))
        await until { model.selectedRows.contains(.saved("Hard Ones")) }
        // The query is the saved search's, so the is-not tag selects no Tags row.
        #expect(model.selectedRows == [.saved("Hard Ones")])
        model.setQuery(model.query.clicking(.state(.review)))
        #expect(model.selectedRows == [.state(.review)])
        model.setQuery(FilterQuery(tokens: [token(.due(.today))]))
        #expect(model.selectedRows == [.builtIn(.dueToday)])
        model.setQuery(FilterQuery(tokens: [token(.due(.today))], text: "plimb"))
        #expect(model.selectedRows.isEmpty)
        model.setQuery(FilterQuery(tokens: [token(Self.spanish, Self.japanese)]))
        #expect(model.selectedRows == [.deck(10), .deck(20)])
    }

    @Test func savedSearchesReplaceTheWholeQuery() async {
        let (model, core) = await makeModel()
        core.reads["is:due -tag:x"] = FilterQuery(tokens: [.advanced("is:due"), token(.tag("x"), negated: true)])
        model.activate(.deck(10))
        model.activate(.builtIn(.dueToday))
        #expect(model.query == FilterQuery(tokens: [token(.due(.today))]))
        model.activate(.builtIn(.leeches))
        #expect(model.query == FilterQuery(tokens: [token(.state(.leech))]))
        // Leech is a Card State value, but no Card State row shows it: the
        // saved search that set it does.
        #expect(model.selectedRows == [.builtIn(.leeches)])
        model.activate(.saved("Hard Ones"))
        await until { model.query.tokens.first == .advanced("is:due") }
        #expect(model.query == core.reads["is:due -tag:x"])
        // A newer change wins over a reading still on its way (the query first
        // differs from the saved search's, else the click would clear it).
        model.activate(.deck(10))
        core.holdReads = true
        model.activate(.saved("Hard Ones"))
        await until { core.heldReadCount == 1 }
        model.activate(.state(.new))
        core.releaseReads()
        await until { core.readTexts.count == 2 && core.heldReadCount == 0 }
        try? await Task.sleep(for: .milliseconds(20))
        #expect(model.query.tokens.last == token(.state(.new)))
    }

    // MARK: Counts (T12.4)

    @Test func rowsShowAtOnceAndTheirCountsFillInForTheCurrentMode() async {
        let (model, core) = await makeModel(holdCounts: true)
        // The names are there while the counts are still being read.
        #expect(model.deckValues.keys.sorted() == [10, 20] && model.counts.isEmpty)
        guard let call = core.countCalls.first else { Issue.record("no counts read"); return }
        #expect(call.0 == .cards)
        let items = call.1
        #expect(items.contains(.value(.due(.today))) && items.contains(.saved("is:due -tag:x")))
        #expect(items.contains(.value(.state(.buried))) && items.contains(.value(.noFlag)) && items.contains(.value(.flag(7))))
        #expect(items.contains(.value(Self.spanish)) && items.contains(.value(Self.vocabulary)))
        #expect(items.contains(.value(.tag("brask"))) && items.contains(.value(.tag("brask::oddly"))))
        core.resumeCounts(0) { item in item == .value(Self.spanish) ? 1500 : (item == .value(.noFlag) ? nil : 3) }
        await until { !model.counts.isEmpty }
        #expect(model.counts[.deck(10)] == 1500 && model.counts[.tag("brask::oddly")] == 3)
        // A value that could not be counted shows none.
        #expect(model.counts[.flag(0)] == nil)
    }

    @Test func aNewerRefreshSupersedesTheOlderAndCancelsIt() async {
        let (model, core) = await makeModel(holdCounts: true)
        #expect(core.countCalls.count == 1)
        model.changesCommitted(Self.changes(tags: ["brask"]), seq: 1)
        await until { core.countCalls.count == 2 }
        await until { core.cancelledCounts == 1 }
        // The older counts arrive late: ignored.
        core.resumeCounts(0) { _ in 99 }
        core.resumeCounts(1) { _ in 5 }
        await until { !model.counts.isEmpty }
        #expect(model.counts[.deck(10)] == 5)
        // The counts shown stay while the next ones are read (no blank flash).
        model.changesCommitted(Self.changes(tags: ["brask"]), seq: 1)
        await until { core.countCalls.count == 3 }
        #expect(model.counts[.deck(10)] == 5)
        // Leaving Browse cancels the counts on their way.
        model.disappear()
        await until { core.cancelledCounts == 2 }
        // Coming back reads the counts left unfinished again.
        model.appear()
        await until { core.countCalls.count == 4 }
        core.resumeCounts(3) { _ in 6 }
        await until { model.counts[.deck(10)] == 6 }
        // Changes no shown row depends on do not count again.
        model.changesCommitted(Self.changes(tags: []), seq: 1)
        try? await Task.sleep(for: .milliseconds(20))
        #expect(core.countCalls.count == 4)
        core.resumeCounts(2) { _ in 0 }
    }

    @Test func countsLeftUnfinishedWhenBrowseHidesAreReadAgainWhenItShows() async {
        let (model, core) = await makeModel(holdCounts: true)
        model.disappear()
        await until { core.cancelledCounts == 1 }
        model.appear()
        await until { core.countCalls.count == 2 }
        core.resumeCounts(1) { _ in 7 }
        await until { model.counts[.deck(10)] == 7 }
        core.resumeCounts(0) { _ in 0 }
        // Finished counts are not read again on the next appear.
        model.disappear()
        model.appear()
        try? await Task.sleep(for: .milliseconds(20))
        #expect(core.countCalls.count == 2)
    }

    @Test func tagNoteCountsLeftUnfinishedAreReadAgainWhenBrowseShows() async {
        let names = (1...14).map { String(format: "t%02d", $0) }
        let (model, core) = await makeModel(holdCounts: true,
                                            tags: names.map { TagNode(name: $0, fullName: $0, collapsed: false, implicit: false, children: []) })
        core.resumeCounts(0) { _ in 3 }
        await until { core.countCalls.count == 2 }
        model.disappear()
        await until { core.cancelledCounts == 1 }
        model.appear()
        await until { core.countCalls.count == 3 }
        core.resumeCounts(2) { _ in 4 }
        await until { core.countCalls.count == 4 }
        #expect(core.countCalls[3].0 == .notes)
        core.resumeCounts(3) { _ in 5 }
        core.resumeCounts(1) { _ in 0 }
    }

    @Test func withMoreThanTwelveTopLevelTagsTagsShowsTheEightWithTheMostNotes() async {
        let names = (1...14).map { String(format: "t%02d", $0) }
        let (model, core) = await makeModel(holdCounts: true,
                                            tags: names.map { TagNode(name: $0, fullName: $0, collapsed: false, implicit: false, children: []) })
        // Before the note counts: the first eight, in tree order.
        #expect(model.shownTopLevelTags.tags.map(\.fullName) == Array(names.prefix(8)))
        core.resumeCounts(0) { _ in 3 }
        await until { core.countCalls.count == 2 }
        // Cards mode: a second read counts the top-level tags' notes.
        #expect(core.countCalls[1].0 == .notes && core.countCalls[1].1.count == 14)
        let notes: [String: UInt64] = ["t14": 90, "t02": 80, "t09": 80, "t05": 70, "t13": 60, "t01": 50, "t07": 40, "t11": 30, "t03": 20]
        core.resumeCounts(1) { item in
            if case .value(.tag(let name)) = item { notes[name] ?? 1 } else { nil }
        }
        await until { model.shownTopLevelTags.tags.map(\.fullName) != Array(names.prefix(8)) }
        #expect(model.shownTopLevelTags.tags.map(\.fullName) == ["t01", "t02", "t05", "t07", "t09", "t11", "t13", "t14"])
        #expect(model.shownTopLevelTags.showAll == 14)
        #expect(model.listSelectionChanged(to: [.showAllTags], click: .plain) == nil)
        #expect(model.shownTopLevelTags.tags.count == 14 && model.shownTopLevelTags.showAll == nil)
    }

    @Test func withMoreThanTwelveNoteTypesTheSectionShowsTheEightWithTheMostRowsAndTheQuerysOwn() async {
        let core = FakeBrowseBackend()
        core.holdCounts = true
        core.notetypeList = (1...14).map { NotetypeName(id: Int64($0), name: String(format: "n%02d", $0)) }
        let model = BrowseModel(backend: core)
        model.appear()
        await until { model.notetypes.count == 14 && !core.countCalls.isEmpty }
        // Before the counts: the first eight, in name order.
        #expect(model.shownNotetypes.rows.map(\.id) == Array(1...8) && model.shownNotetypes.showAll == 14)
        let rows: [Int64: UInt64] = [14: 90, 2: 80, 9: 80, 5: 70, 13: 60, 1: 50, 7: 40, 11: 30, 3: 20]
        core.resumeCounts(0) { item in
            if case .value(.notetype(let id, _)) = item { rows[id] ?? 1 } else { 3 }
        }
        await until { model.shownNotetypes.rows.map(\.id) == [1, 2, 5, 7, 9, 11, 13, 14] }
        // A note type the query selects shows too, in its place.
        model.activate(.notetype(4))
        #expect(model.shownNotetypes.rows.map(\.id) == [1, 2, 4, 5, 7, 9, 11, 13, 14] && model.shownNotetypes.showAll == 14)
        model.activate(.showAllNotetypes)
        #expect(model.shownNotetypes.rows.count == 14 && model.shownNotetypes.showAll == nil && model.showsAllNotetypes)
    }

    // MARK: Kept state (V1) and opening (T14)

    @Test func leavingBrowseAndComingBackKeepsTheQueryAndModeAndReadsNoDefaultAgain() async {
        let (model, core) = await makeModel(opening: BrowseOpening(defaultSearch: "deck:x", notesMode: true),
                                            reads: ["deck:x": FilterQuery(tokens: [token(Self.spanish)])])
        await until { model.query == core.reads["deck:x"] }
        #expect(model.mode == .notes)
        model.activate(.state(.suspended))
        let kept = model.query
        model.disappear()
        model.appear()
        model.disappear()
        model.appear()
        #expect(model.query == kept && model.mode == .notes && core.openCalls == 1)
    }

    @Test func aFirstOpenReadsTheDefaultSearchAndARelaunchReadsTheKeptRunText() async throws {
        let scratch = try ScratchDefaults()
        let store = BrowseDeviceStore(defaults: scratch.defaults)
        // No default search: the whole collection.
        let (empty, _) = await makeModel(store: store)
        #expect(empty.query.isEmpty)
        await until { store.load().runText != nil }

        // A first open with a default search (another device, or this one replaced).
        let scratch2 = try ScratchDefaults()
        let store2 = BrowseDeviceStore(defaults: scratch2.defaults)
        let (first, core) = await makeModel(opening: BrowseOpening(defaultSearch: "  deck:x ", notesMode: false), store: store2,
                                            reads: ["  deck:x ": FilterQuery(tokens: [token(Self.spanish)])])
        await until { first.query == core.reads["  deck:x "] }
        first.activate(.state(.new))
        await until { store2.load().runText == "run:2" }

        // A relaunch: the kept run text, read back by T7; the default is not read again.
        let (relaunched, again) = await makeModel(opening: BrowseOpening(defaultSearch: "  deck:x ", notesMode: false), store: store2,
                                                  reads: ["run:2": FilterQuery(tokens: [token(Self.spanish), token(.state(.new))])])
        await until { !relaunched.query.isEmpty }
        #expect(relaunched.query == FilterQuery(tokens: [token(Self.spanish), token(.state(.new))]))
        #expect(!again.readTexts.contains("  deck:x "))

        // The collection replaced: the next open is a first open again.
        relaunched.collectionReplaced()
        #expect(relaunched.query.isEmpty && !relaunched.opened && store2.load().runText == nil)
        again.reads["  deck:x "] = FilterQuery(tokens: [token(Self.japanese)])
        relaunched.collectionReady()
        relaunched.appear()
        await until { relaunched.query == FilterQuery(tokens: [token(Self.japanese)]) }
    }

    @Test func aFailedOpeningKeepsNothingAndTheNextAppearOpensAgain() async throws {
        let scratch = try ScratchDefaults()
        let store = BrowseDeviceStore(defaults: scratch.defaults)
        let core = FakeBrowseBackend()
        core.openingFails = true
        let model = BrowseModel(backend: core, store: store)
        model.appear()
        await until { core.openCalls == 1 && !model.opened }
        #expect(store.load().runText == nil)
        core.openingFails = false
        core.openingValue = BrowseOpening(defaultSearch: "deck:x", notesMode: false)
        core.reads["deck:x"] = FilterQuery(tokens: [token(Self.spanish)])
        model.disappear()
        model.appear()
        await until { model.query == FilterQuery(tokens: [token(Self.spanish)]) }
    }

    @Test func aFailedFirstReadingOpensAgainOnTheNextAppear() async {
        let core = FakeBrowseBackend()
        core.openingValue = BrowseOpening(defaultSearch: "deck:x", notesMode: false)
        core.readFails = ["deck:x"]
        let model = BrowseModel(backend: core)
        model.appear()
        await until { core.readTexts == ["deck:x"] && !model.opened }
        core.readFails = []
        core.reads["deck:x"] = FilterQuery(tokens: [token(Self.spanish)])
        model.disappear()
        model.appear()
        await until { model.query == FilterQuery(tokens: [token(Self.spanish)]) }
    }

    @Test func aHandedSearchReadAcrossAReadyCollectionIsReadAgain() async {
        let (model, core) = await makeModel()
        core.reads["deck:Japanese"] = FilterQuery(tokens: [token(Self.japanese)])
        let handed = Handed()
        model.takeEntrySearch = { defer { handed.search = nil }; return handed.search }
        core.holdReads = true
        model.disappear()
        model.appear()
        await until { core.heldReadCount == 1 }
        // The collection turns ready again while the reading is on its way: it is discarded.
        model.collectionReady()
        core.releaseReads()
        await until { model.query == FilterQuery(tokens: [token(Self.japanese)]) }
    }

    @Test func aFailedTagWriteUndoesOnlyItsOwnToggle() async {
        let (model, core) = await makeModel()
        let tag = TagNode(name: "brask", fullName: "brask", collapsed: true, implicit: false, children: [])
        core.holdTagWrites = true
        model.setTagExpanded(tag, true)   // A
        model.setTagExpanded(tag, false)  // B
        model.setTagExpanded(tag, true)   // C
        await until { core.tagWriteCount == 3 }
        core.endTagWrite(0, failing: true)
        try? await Task.sleep(for: .milliseconds(20))
        #expect(model.tagExpanded(tag))
        core.endTagWrite(1, failing: false)
        core.endTagWrite(2, failing: true)
        await until { !model.tagExpanded(tag) }
    }

    @Test func theModeIsKeptOnTheDeviceAcrossARelaunch() async throws {
        let scratch = try ScratchDefaults()
        let store = BrowseDeviceStore(defaults: scratch.defaults)
        let (first, _) = await makeModel(opening: BrowseOpening(defaultSearch: nil, notesMode: true), store: store)
        await until { first.mode == .notes && store.load().notesMode == true }
        // Another device turned the synced key off: this device keeps its own mode.
        let (relaunched, _) = await makeModel(opening: BrowseOpening(defaultSearch: nil, notesMode: false), store: store)
        try? await Task.sleep(for: .milliseconds(20))
        #expect(relaunched.mode == .notes)
    }

    @Test func aSearchHandedToBrowseReplacesTheQuery() async {
        let (model, core) = await makeModel()
        core.reads["deck:Japanese"] = FilterQuery(tokens: [token(Self.japanese)])
        model.activate(.state(.new))
        let handed = Handed()
        model.takeEntrySearch = { defer { handed.search = nil }; return handed.search }
        model.disappear()
        model.appear()
        await until { model.query == FilterQuery(tokens: [token(Self.japanese)]) }
        model.disappear()
        model.appear()
        #expect(model.query == FilterQuery(tokens: [token(Self.japanese)]))
    }

    @Test func foldedSectionsAndShowAllAreKeptOnTheDevice() async throws {
        let scratch = try ScratchDefaults()
        let store = BrowseDeviceStore(defaults: scratch.defaults)
        #expect(store.load().folded == [.flags, .tags])
        let model = BrowseModel(backend: FakeBrowseBackend(), store: store)
        #expect(model.folded == [.flags, .tags] && !model.showsAllTags)
        model.folded.remove(.tags)
        model.folded.insert(.decks)
        model.showsAllTags = true
        let again = BrowseModel(backend: FakeBrowseBackend(), store: store)
        #expect(again.folded == [.flags, .decks] && again.showsAllTags)
        // A replaced collection drops the kept query and mode, not the folding.
        var kept = store.load()
        kept.runText = "deck:x"
        kept.notesMode = true
        store.save(kept)
        store.forgetCollection()
        #expect(store.load() == BrowseDeviceState(runText: nil, notesMode: nil, folded: [.flags, .decks], showsAllTags: true))
    }

    @Test func nothingIsReadBeforeTheCollectionIsReady() async {
        let core = FakeBrowseBackend()
        let model = BrowseModel(backend: core)
        let ready = Flag()
        model.ready = { ready.value }
        model.appear()
        try? await Task.sleep(for: .milliseconds(20))
        #expect(core.openCalls == 0 && core.countCalls.isEmpty)
        ready.value = true
        model.collectionReady()
        await until { core.openCalls == 1 && !core.countCalls.isEmpty }
    }

    // MARK: The sidebar's filter (S6)

    @Test func theFilterShowsMatchesAndTheirAncestorsExpanded() throws {
        struct Item { let name: String; var children: [Item] = [] }
        let tree = [
            Item(name: "Languages", children: [Item(name: "Español", children: [Item(name: "Vocabulary")]), Item(name: "日本語")]),
            Item(name: "Science", children: [Item(name: "Chemistry")]),
            Item(name: "Music Theory"),
        ]
        let filter = BrowseSidebarFilter("voc")
        let shown = filter.tree(tree, label: \.name, children: \.children, rootMatched: false)
        #expect(shown.map(\.item.name) == ["Languages"])
        let languages = try #require(shown.first)
        #expect(languages.expanded && languages.children.map(\.item.name) == ["Español"])
        let español = try #require(languages.children.first)
        let vocabulary = try #require(español.children.first)
        #expect(español.children.map(\.item.name) == ["Vocabulary"] && vocabulary.matched)
        // Leading and trailing spaces count: " mam" does not match "mammal".
        #expect(!BrowseSidebarFilter(" voc").matches("Vocabulary") && BrowseSidebarFilter("OCAB").matches("Vocabulary"))
        // A matching section root shows every top-level item, folded unless a descendant matches.
        let root = BrowseSidebarFilter("chem").tree(tree, label: \.name, children: \.children, rootMatched: true)
        #expect(root.map(\.item.name) == ["Languages", "Science", "Music Theory"])
        try #require(root.count == 3)
        #expect(!root[0].expanded && root[1].expanded)
        // White_Space only is off.
        #expect(!BrowseSidebarFilter("  ").isOn && BrowseSidebarFilter("  ").tree(tree, label: \.name, children: \.children, rootMatched: false).isEmpty)
        #expect(BrowseSidebarFilter("x").rows(["Due Today", "Leeches"], label: { $0 }, rootMatched: false).isEmpty)
        // Off: every row shows.
        #expect(BrowseSidebarFilter(" ").rows(["Due Today", "Leeches"], label: { $0 }, rootMatched: false) == ["Due Today", "Leeches"])
        #expect(BrowseSidebarFilter("ee").rows(["Due Today", "Leeches"], label: { $0 }, rootMatched: false) == ["Leeches"])
    }

    // MARK: Fixtures

    @MainActor private final class Flag { var value = false }
    @MainActor private final class Handed { var search: String? = "deck:Japanese" }

    private final class ScratchDefaults {
        let name = "meguri.tests.browse.\(UUID().uuidString)"
        let defaults: UserDefaults
        init() throws { defaults = try #require(UserDefaults(suiteName: name)) }
        deinit { UserDefaults.standard.removePersistentDomain(forName: name) }
    }

    private static func changes(tags: [String]) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [],
            decks: [], deckConfigs: [], configKeys: [], tags: tags.map { .init(name: $0, change: .updated) },
            revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [],
            skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)
    }

    private func makeModel(opening: BrowseOpening = BrowseOpening(defaultSearch: nil, notesMode: false),
                           store: BrowseDeviceStore = BrowseDeviceStore(defaults: nil), holdCounts: Bool = false,
                           tags: [TagNode]? = nil, reads: [String: FilterQuery] = [:]) async -> (BrowseModel, FakeBrowseBackend) {
        let core = FakeBrowseBackend()
        core.openingValue = opening
        core.holdCounts = holdCounts
        core.reads.merge(reads) { $1 }
        if let tags { core.tagNodes = tags }
        let model = BrowseModel(backend: core, store: store)
        model.appear()
        await until { !model.deckValues.isEmpty && !core.countCalls.isEmpty && core.openCalls == 1 }
        return (model, core)
    }
}

/// Waits, suspended, until `condition` holds (at most a second).
@MainActor func until(_ condition: () -> Bool) async {
    for _ in 0..<200 where !condition() {
        try? await Task.sleep(for: .milliseconds(5))
    }
    #expect(condition())
}

/// A core over a small made-up collection: Languages::Español (10) and
/// Languages::日本語 (20), two note types, a tag tree, the seven flags and
/// one saved search. Counts can be held until the test resumes them.
@MainActor final class FakeBrowseBackend: BrowseBackend {
    var openingValue = BrowseOpening(defaultSearch: nil, notesMode: false)
    var openCalls = 0
    var reads: [String: FilterQuery] = [:]
    var readTexts: [String] = []
    var readCalls = 0
    var holdReads = false
    private var heldReads: [CheckedContinuation<Void, Never>] = []
    var heldReadCount: Int { heldReads.count }
    var runTexts = 0
    var tagNodes = [TagNode(name: "brask", fullName: "brask", collapsed: false, implicit: false, children: [
        TagNode(name: "oddly", fullName: "brask::oddly", collapsed: false, implicit: false, children: []),
    ])]
    var holdCounts = false
    var countCalls: [(BrowseMode, [FilterCountItem])] = []
    private var heldCounts: [Int: CheckedContinuation<[UInt64?], any Error>] = [:]
    var cancelledCounts = 0

    var openingFails = false
    func opening() async throws -> BrowseOpening {
        openCalls += 1
        if openingFails { throw BridgeError.unavailable(reason: "sample") }
        return openingValue
    }
    func read(_ text: String) async throws -> FilterQuery {
        readCalls += 1
        readTexts.append(text)
        if holdReads { await withCheckedContinuation { heldReads.append($0) } }
        if readFails.contains(text) { throw BridgeError.unavailable(reason: "sample") }
        return reads[text] ?? FilterQuery(text: text)
    }
    func releaseReads() {
        holdReads = false
        heldReads.forEach { $0.resume() }
        heldReads = []
    }
    func runText(_ query: FilterQuery) async throws -> String {
        runTexts += 1
        if let runTextDelay { try? await Task.sleep(for: runTextDelay) }
        if query.isEmpty { return "" }
        return query.text.isEmpty ? "run:\(query.tokens.count)" : "run:\(query.tokens.count):\(query.text)"
    }
    // The search field (U271).
    var commits: [FilterQuery] = []
    var committed: [FilterQuery: FilterQuery] = [:]
    func commit(_ query: FilterQuery) async throws -> FilterQuery {
        commits.append(query)
        return committed[query] ?? query
    }
    var incomplete: Set<String> = []
    var incompleteChecks: [String] = []
    func isIncomplete(_ text: String, caret: Int) async throws -> Bool {
        incompleteChecks.append(text)
        return incomplete.contains(text)
    }
    var suggestionCalls: [(FilterQuery, Int, FilterSection?, [String])] = []
    var suggestionRows: [FilterSection?: [FilterSuggestion]] = [:]
    func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection?, recent: [String]) async throws
        -> [FilterSuggestion] {
        suggestionCalls.append((query, caret, scope, recent))
        return suggestionRows[scope] ?? []
    }
    var savedSearchWrites: [(String, String)] = []
    var saveFails = false
    var holdSaves = false
    private var heldSaves: [CheckedContinuation<Void, Never>] = []
    func releaseSaves() { heldSaves.forEach { $0.resume() }; heldSaves = [] }
    var heldSaveCount: Int { heldSaves.count }
    func releaseFirstSave() { if !heldSaves.isEmpty { heldSaves.removeFirst().resume() } }
    func saveSearch(name: String, text: String) async throws {
        if holdSaves { await withCheckedContinuation { heldSaves.append($0) } }
        if saveFails { throw BridgeError.unavailable(reason: "sample") }
        savedSearchWrites.append((name, text))
    }
    var extraSaved: [FilterSuggestionRow] = []
    var renames: [(String, String)] = []
    func renameSavedSearch(_ old: String, to name: String) async throws { renames.append((old, name)) }
    var deletedSearches: [String] = []
    func deleteSavedSearch(_ name: String) async throws { deletedSearches.append(name) }
    func values(_ section: FilterSection) async throws -> [FilterSuggestionRow] {
        switch section {
        case .savedSearches:
            FilterBuiltIn.allCases.map { .builtIn($0) } + [.savedSearch(name: "Hard Ones", text: "is:due -tag:x")] + extraSaved
        case .decks:
            [.value(.deck(id: 10, native: "Languages\u{1F}Español", tree: [10, 11])),
             .value(.deck(id: 20, native: "Languages\u{1F}日本語", tree: [20]))]
        default: []
        }
    }
    var notetypeList = [NotetypeName(id: 8, name: "Basic"), NotetypeName(id: 7, name: "Vocabulary")]
    func notetypes() async throws -> [NotetypeName] { notetypeList }
    func tagTree() async throws -> [TagNode] { tagNodes }
    func flagNames() async throws -> [String] { ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"] }
    /// The second read of a refresh, the values only the suggestions show
    /// (U271): answered at once and kept apart from the sidebar's calls.
    var suggestionCountCalls: [(BrowseMode, [FilterCountItem])] = []
    func counts(_ mode: BrowseMode, _ items: [FilterCountItem]) async throws -> [UInt64?] {
        if items == BrowseModel.suggestedValues.map({ .value($0) }) {
            suggestionCountCalls.append((mode, items))
            return items.map { _ in 3 }
        }
        let call = countCalls.count
        countCalls.append((mode, items))
        guard holdCounts else { return items.map { _ in 1 } }
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { heldCounts[call] = $0 }
        } onCancel: {
            Task { @MainActor in self.cancelledCounts += 1 }
        }
    }
    /// Ends counts call `call` with `value` for each of its items.
    func resumeCounts(_ call: Int, _ value: (FilterCountItem) -> UInt64?) {
        guard let continuation = heldCounts.removeValue(forKey: call) else { return }
        continuation.resume(returning: countCalls[call].1.map(value))
    }
    var holdTagWrites = false
    private var heldTagWrites: [CheckedContinuation<Void, any Error>] = []
    var tagWriteCount: Int { heldTagWrites.count }
    func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws {
        guard holdTagWrites else { return }
        try await withCheckedThrowingContinuation { heldTagWrites.append($0) }
    }
    /// Ends held tag write `index`, failing it or not.
    func endTagWrite(_ index: Int, failing: Bool) {
        if failing { heldTagWrites[index].resume(throwing: BridgeError.unavailable(reason: "sample")) }
        else { heldTagWrites[index].resume() }
    }
    var readFails: Set<String> = []

    // The results table (U270a).
    var searchSession = FakeSearchSession()
    static let standard = ColumnSetShown(
        set: .standard, own: false, cards: ["noteFld", "template", "deck", "cardDue", "cardMod"],
        notes: ["noteFld", "note", "template", "noteTags", "noteCrt"],
        cardsSort: BrowseSort(key: "noteFld", direction: .ascending), notesSort: BrowseSort(key: "noteFld", direction: .ascending))
    var columnChanges: [ColumnSetChange] = []
    var columnChangeSets: [ColumnSetID] = []
    var notesModeWrites: [Bool] = []
    /// Note types with an own set (V2.2).
    var ownSets: [Int64: ColumnSetShown] = [:]
    var runTextDelay: Duration?
    func openSearchSession() async throws -> any BrowseSearching { searchSession }
    func columnSet(notetype: Int64?) async throws -> ColumnSetShown {
        notetype.flatMap { ownSets[$0] } ?? storedStandard ?? Self.standard
    }
    var columnModes: [BrowseMode] = []
    var useStandardCalls: [Int64] = []
    /// The standard set's changes are kept, as the collection keeps them (U272).
    var persistsColumns = false
    var storedStandard: ColumnSetShown?
    var columnChangeDelay: Duration?
    func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws {
        if let delay = columnChangeDelay { try? await Task.sleep(for: delay) }
        columnChanges.append(change)
        columnChangeSets.append(set)
        columnModes.append(mode)
        guard persistsColumns, set == .standard else { return }
        let stored = storedStandard ?? Self.standard
        var columns = stored.columns(mode)
        switch change {
        case .show(let key): columns.append(key)
        case .hide(let key): columns.removeAll { $0 == key }
        case .reorder(let keys): columns = keys
        case .sortBy, .setSort: break
        }
        storedStandard = stored.with(columns, mode: mode)
    }
    func useStandardColumns(notetype: Int64) async throws { useStandardCalls.append(notetype) }
    func setNotesMode(_ notes: Bool) async throws { notesModeWrites.append(notes) }
}
