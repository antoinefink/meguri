@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// Browse's search field (U271): when the field changes the query
/// (`search/filter-tokens` T8), the token menus and keys (T10), accepting
/// suggestions (T12) and Save Search (T13), against a scripted core.
@MainActor @Suite struct BrowseSearchFieldTests {
    private static let spanish = FilterValue.deck(id: 10, native: "Languages\u{1F}Español", tree: [10, 11])
    private static let japanese = FilterValue.deck(id: 20, native: "Languages\u{1F}日本語", tree: [20])

    private func token(_ values: FilterValue..., negated: Bool = false) -> FilterToken {
        .value(values, negated: negated)
    }

    private func makeModel() async -> (BrowseModel, FakeBrowseBackend) {
        let core = FakeBrowseBackend()
        let model = BrowseModel(backend: core)
        model.appear()
        await until { !model.deckValues.isEmpty && !core.countCalls.isEmpty && core.openCalls == 1 }
        await until { !core.searchSession.starts.isEmpty }
        return (model, core)
    }

    private func started(_ core: FakeBrowseBackend) -> [String] { core.searchSession.starts.map(\.text) }

    /// Keystrokes: one character a change, after the text there is.
    private func type(_ model: BrowseModel, _ text: String) {
        for character in text { model.editText(model.query.text + String(character)) }
    }

    // MARK: T8

    @Test func everyEditOfTheTextRunsTheQueryWithoutReturn() async {
        let (model, core) = await makeModel()
        type(model, "plimb")
        await until { started(core).last == "run:0:plimb" }
        // A newer keystroke cancels the older run: the shorter texts never started.
        #expect(!started(core).contains("run:0:p") && !started(core).contains("run:0:pl"))
        #expect(model.query == FilterQuery(text: "plimb") && core.commits.isEmpty)
    }

    @Test func incompleteTextKeepsTheLastResults() async {
        let (model, core) = await makeModel()
        type(model, "plimb")
        await until { started(core).last == "run:0:plimb" }
        core.incomplete = ["plimb d", "plimb de", "plimb dec", "plimb deck", "plimb deck:"]
        type(model, " deck:")
        await until { core.incompleteChecks.contains("plimb deck:") }
        try? await Task.sleep(for: .milliseconds(150))
        #expect(started(core).last == "run:0:plimb")
        // The field still shows what was typed.
        #expect(model.query.text == "plimb deck:")
    }

    @Test func returnCommitsSyntaxIntoTokensAfterTheQuerysOwn() async {
        let (model, core) = await makeModel()
        model.activate(.deck(10))
        let typed = FilterQuery(tokens: [token(Self.spanish)], text: "deck:French plimb")
        core.committed[typed] = FilterQuery(tokens: [token(Self.spanish), .advanced("deck:French")], text: "plimb")
        type(model, "deck:French plimb")
        #expect(core.commits.isEmpty)
        model.commitText()
        await until { model.query.tokens.count == 2 }
        #expect(model.query == FilterQuery(tokens: [token(Self.spanish), .advanced("deck:French")], text: "plimb"))
        await until { started(core).last == "run:2:plimb" }
    }

    @Test func aPasteCommitsAndLosingTheFocusCommits() async {
        let (model, core) = await makeModel()
        core.committed[FilterQuery(text: "tag:brask")] = FilterQuery(tokens: [token(.tag("brask"))])
        // Two or more characters in one change are a paste.
        model.editText("tag:brask")
        await until { model.query == FilterQuery(tokens: [token(.tag("brask"))]) }
        core.committed[FilterQuery(tokens: [token(.tag("brask"))], text: "is:new")] =
            FilterQuery(tokens: [token(.tag("brask")), token(.state(.new))])
        model.setFieldFocused(true)
        type(model, "is:new")
        #expect(core.commits.count == 1)
        model.setFieldFocused(false)
        await until { model.query.tokens.count == 2 }
        #expect(model.query.tokens.last == token(.state(.new)) && model.query.text.isEmpty)
    }

    // MARK: T10

    @Test func theTokenMenuChecksValuesNegatesAndRemoves() async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        model.activate(.tag("brask"))
        // Checking a value adds it; unchecking the last removes the token.
        model.toggle(Self.japanese, in: 0)
        #expect(model.query.tokens[0] == token(Self.spanish, Self.japanese))
        model.toggle(Self.spanish, in: 0)
        model.toggle(Self.japanese, in: 0)
        #expect(model.query.tokens == [token(.tag("brask"))])
        // Is Not, then Is.
        model.setNegated(0, true)
        #expect(model.query.tokens == [token(.tag("brask"), negated: true)])
        model.setNegated(0, false)
        #expect(model.query.tokens == [token(.tag("brask"))])
        model.removeToken(0)
        #expect(model.query.isEmpty)
    }

    @Test func isNotIsRefusedWhileTheTokenHoldsNoFlagAnyFlagOrUntagged() async {
        let (model, core) = await makeModel()
        model.activate(.flag(0))
        #expect(!model.query.tokens[0].canNegate)
        model.setNegated(0, true)
        #expect(model.query.tokens == [token(.noFlag)])
        // An is-not token never takes them: the menu offers none.
        core.suggestionRows[.flag] = [.noFlag, .anyFlag, .flag(1), .flag(3)].map {
            FilterSuggestion(section: .flag, row: .value($0), key: nil)
        }
        model.activate(.flag(2), click: .option)
        await until { model.tokenOffers[.flag]?.count == 4 }
        #expect(model.offeredValues(0) == [.flag(1), .flag(3)])
        model.toggle(.noFlag, in: 0)
        #expect(model.query.tokens == [token(.flag(2), negated: true)])
    }

    @Test func theMenuOffersUpToEightOtherValuesAndAddAnotherScopesTheSuggestions() async {
        let (model, core) = await makeModel()
        core.suggestionRows[.tags] = (0..<12).map { FilterSuggestion(section: .tags, row: .value(.tag("t\($0)")), key: nil) }
        model.activate(.tag("t0"))
        // The menu keeps the token's own values and 8 others, no more.
        await until { model.tokenOffers[.tag]?.count == 9 }
        #expect(model.offeredValues(0) == (1...8).map { .tag("t\($0)") })
        let focus = model.focusRequests
        model.setFieldFocused(true)
        model.addAnother(to: 0)
        #expect(model.suggestionScope == .tags && model.focusRequests == focus &+ 1)
        await until { core.suggestionCalls.last?.2 == .tags }
    }

    @Test func anAdvancedTokensEditPutsItsTextBackInTheField() async {
        let (model, _) = await makeModel()
        model.setFieldTokensForTests(FilterQuery(tokens: [.advanced("prop:ivl>30"), token(.state(.new))], text: "plimb"))
        model.editAdvanced(0)
        #expect(model.query == FilterQuery(tokens: [token(.state(.new))], text: "prop:ivl>30 plimb"))
    }

    @Test func theSystemFieldRemovingATokenOrTheClearButtonChangeTheQuery() async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        model.activate(.state(.new))
        // ⌫ removed the first token.
        model.setFieldTokens(Array(model.fieldTokens.dropFirst()))
        #expect(model.query.tokens == [token(.state(.new))])
        type(model, "plimb")
        model.clearField()
        #expect(model.query.isEmpty)
    }

    // MARK: T12

    @Test func acceptingAValueAddsItToItsKindsTokenAndDropsTheWord() async {
        let (model, _) = await makeModel()
        model.activate(.deck(10))
        type(model, "plimb jap")
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.japanese), key: "deck:20"))
        #expect(model.query == FilterQuery(tokens: [token(Self.spanish, Self.japanese)], text: "plimb "))
        // A kind with no token yet gets is {v}.
        type(model, "new")
        model.accept(FilterSuggestion(section: .cardState, row: .value(.state(.new)), key: nil))
        #expect(model.query.tokens.last == token(.state(.new)) && model.query.text == "plimb ")
    }

    @Test func acceptingContainsCommitsAndASavedSearchReplacesTheQuery() async {
        let (model, core) = await makeModel()
        model.activate(.deck(10))
        type(model, "plimb")
        model.accept(FilterSuggestion(section: nil, row: .contains("plimb"), key: nil))
        await until { core.commits.last == FilterQuery(tokens: [token(Self.spanish)], text: "plimb") }
        model.accept(FilterSuggestion(section: .savedSearches, row: .builtIn(.leeches), key: nil))
        #expect(model.query == FilterQuery(tokens: [token(.state(.leech))]))
        core.reads["tag:x"] = FilterQuery(tokens: [token(.tag("x"))])
        model.accept(FilterSuggestion(section: .savedSearches, row: .savedSearch(name: "X", text: "tag:x"), key: nil))
        await until { model.query == FilterQuery(tokens: [token(.tag("x"))]) }
    }

    @Test func aPasteOverASelectionCommitsToo() {
        #expect(BrowseModel.inserted("tag:old", "tag:new") == 3)
        #expect(BrowseModel.inserted("plimb", "plimbs") == 1)
        #expect(BrowseModel.inserted("plimb x", "plimb deck:a x") == 7)
        #expect(BrowseModel.inserted("aa", "aaa") == 1)
        #expect(BrowseModel.inserted("abc", "") == 0)
    }

    @Test func aSyntaxCandidateThatDoesNotCompleteNeverCommits() async {
        let (model, core) = await makeModel()
        model.setFieldFocused(true)
        type(model, "de")
        core.suggestionRows[nil] = [FilterSuggestion(section: nil, row: .syntax(SyntaxCandidate(
            kind: .keyword, label: "deck:", detail: nil, insert: (0..<2, "deck:", 5), completes: false)), key: nil)]
        model.editText("de ")
        model.editText("de")
        await until { model.suggestions.count == 1 }
        model.accept(model.suggestions[0])
        #expect(model.query.text == "deck:")
        try? await Task.sleep(for: .milliseconds(100))
        #expect(core.commits.isEmpty)
    }

    @Test func filterByOpensAKindsValueListAndAddsWithoutDroppingText() async {
        let (model, core) = await makeModel()
        // Filter By shows with the caret after White_Space: no word to drop.
        type(model, "plimb ")
        model.setFieldFocused(true)
        model.accept(FilterSuggestion(section: .filterBy, row: .filterBy(.decks), key: nil))
        #expect(model.suggestionScope == .decks)
        await until { core.suggestionCalls.last?.2 == .decks }
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.spanish), key: nil))
        #expect(model.query == FilterQuery(tokens: [token(Self.spanish)], text: "plimb ") && model.suggestionScope == nil)
        // Show All from a typed word: the list is that word's, so the word goes.
        type(model, "jap")
        model.accept(FilterSuggestion(section: .decks, row: .showAll(.decks), key: nil))
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.japanese), key: nil))
        #expect(model.query.text == "plimb ")
    }

    @Test func isEmptyAndHasContentDropTheWordThatFoundTheField() async {
        let (model, _) = await makeModel()
        type(model, "plimb vor")
        model.accept(FilterSuggestion(section: .fields, row: .field(name: "Vorrel", notetypes: [8]), key: nil))
        model.acceptFieldTest(.empty)
        #expect(model.query == FilterQuery(tokens: [token(.field("Vorrel", .empty))], text: "plimb "))
    }

    @Test func containsAnswersEvenAfterTheFieldLostTheFocus() async {
        let (model, _) = await makeModel()
        model.setFieldFocused(true)
        type(model, "back")
        model.accept(FilterSuggestion(section: .fields, row: .field(name: "Back", notetypes: [8]), key: nil))
        model.acceptFieldTest(.contains(""))
        guard let editing = model.editingContains else {
            Issue.record("Contains… did not ask for its text")
            return
        }
        // The alert takes the focus, then clears its binding before its button runs.
        model.setFieldFocused(false)
        model.dismissContains()
        model.finishContains(editing, "pear")
        #expect(model.query.tokens == [token(.field("Back", .contains("pear")))] && model.query.text.isEmpty)
    }

    @Test func aFieldSuggestionsContainsTakesTheWordUnlessItIsTheFieldsName() async {
        let (model, _) = await makeModel()
        type(model, "apple")
        model.accept(FilterSuggestion(section: .fields, row: .field(name: "Back", notetypes: [8]), key: nil))
        #expect(model.suggestedField?.containsText == "apple")
        model.acceptFieldTest(.contains(""))
        #expect(model.query == FilterQuery(tokens: [token(.field("Back", .contains("apple")))], text: ""))
        // The word is the field's name: Contains… asks; Has Content takes the word with it.
        type(model, "back")
        model.accept(FilterSuggestion(section: .fields, row: .field(name: "Back", notetypes: [8]), key: nil))
        model.acceptFieldTest(.contains(""))
        #expect(model.editingContains?.field == "Back")
        model.finishContains("pear")
        #expect(model.query.tokens == [token(.field("Back", .contains("apple")), .field("Back", .contains("pear")))])
        #expect(model.query.text.isEmpty)
    }

    @Test func acceptedSuggestionsGoFirstInTheRecentList() async {
        let (model, core) = await makeModel()
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.spanish), key: "deck:10"))
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.japanese), key: "deck:20"))
        model.accept(FilterSuggestion(section: .decks, row: .value(Self.spanish), key: "deck:10"))
        model.setFieldFocused(true)
        await until { core.suggestionCalls.last?.3 == ["deck:10", "deck:20"] }
    }

    @Test func theSuggestionsOnlyValuesAreCountedAfterTheSidebars() async {
        let (model, core) = await makeModel()
        await until { model.valueCounts[.due(.overdue)] == 3 }
        #expect(core.suggestionCountCalls.count == 1 && model.count(of: .added(.today)) == 3)
        // The sidebar's own rows keep the sidebar's counts.
        #expect(model.count(of: Self.spanish) == 1)
    }

    @Test func theTextSurvivesTheFieldEmptyingItWhenItTakesASuggestion() async {
        let (model, core) = await makeModel()
        let contains = FilterSuggestion(section: nil, row: .contains("plimb"), key: nil)
        core.suggestionRows[nil] = [contains]
        model.setFieldFocused(true)
        // The field empties the text, then hands over the suggestion.
        type(model, "plimb")
        await until { model.suggestionsCurrent }
        model.editText("")
        model.setFieldTokens(model.fieldTokens + [.suggestion(contains)])
        #expect(model.query.text == "plimb")
        await until { core.commits.last?.text == "plimb" }
        // The other order: the suggestion first, then the emptied text.
        await until { !model.acceptedThisTurn && model.clearedThisTurn == nil }
        type(model, " zindro")
        await until { model.suggestionsCurrent }
        model.setFieldTokens(model.fieldTokens + [.suggestion(FilterSuggestion(section: nil, row: .contains("zindro"), key: nil))])
        model.editText("")
        #expect(model.query.text == "plimb zindro")
        // A clear of its own, later, still empties it.
        await until { !model.acceptedThisTurn }
        model.editText("")
        #expect(model.query.text.isEmpty)
    }

    @Test func rowsReadForAnotherScopeOrTextAreInertUntilTheNewOnesLand() async {
        let (model, core) = await makeModel()
        let old = FilterSuggestion(section: .decks, row: .value(Self.spanish), key: nil)
        core.suggestionRows[nil] = [old]
        model.setFieldFocused(true)
        type(model, "plimb sp")
        await until { model.suggestions == [old] && model.suggestionsCurrent }
        // Add Another tag… changes the scope; the deck row still shows until the
        // tags' rows land, and accepting it changes nothing.
        model.activate(.tag("brask"))
        model.addAnother(to: 0)
        #expect(model.suggestions == [old] && !model.suggestionsCurrent)
        let before = model.query
        model.setFieldTokens(model.fieldTokens + [.suggestion(old)])
        #expect(model.query == before)
        // Once the scope's rows land, a row of them is accepted.
        let tag = FilterSuggestion(section: .tags, row: .value(.tag("zindro")), key: nil)
        core.suggestionRows[.tags] = [tag]
        model.addAnother(to: 0)
        await until { model.suggestions == [tag] && model.suggestionsCurrent }
        model.setFieldTokens(model.fieldTokens + [.suggestion(tag)])
        #expect(model.query.tokens == [token(.tag("brask"), .tag("zindro"))])
    }

    // MARK: T13

    @Test func saveSearchStoresTheRunTextAndAsksBeforeReplacing() async {
        let (model, core) = await makeModel()
        model.startSavingSearch()
        #expect(model.savingSearch == nil)  // the empty query has nothing to save
        model.activate(.deck(10))
        model.startSavingSearch()
        model.saveSearch(named: "  Spanish  ")
        await until { core.savedSearchWrites.count == 1 }
        #expect(core.savedSearchWrites.first?.0 == "Spanish" && core.savedSearchWrites.first?.1 == "run:1")
        // "Hard Ones" exists: the question first; Cancel goes back to the name.
        model.startSavingSearch()
        model.saveSearch(named: "Hard Ones")
        #expect(model.savingSearch?.replacing == "Hard Ones" && core.savedSearchWrites.count == 1)
        model.keepNaming()
        #expect(model.savingSearch != nil && model.savingSearch?.replacing == nil)
        model.saveSearch(named: "Hard Ones")
        model.saveSearch(named: "Hard Ones")
        await until { core.savedSearchWrites.count == 2 && model.savingSearch == nil }
    }

    @Test func aFailedSaveKeepsTheSheetAndItsName() async {
        let (model, core) = await makeModel()
        model.activate(.deck(10))
        core.saveFails = true
        model.startSavingSearch()
        model.saveSearch(named: "Spanish")
        #expect(model.savingSearch?.saving == true)
        await until { model.savingSearch?.failed == true }
        #expect(model.savingSearch?.name == "Spanish" && core.savedSearchWrites.isEmpty)
        core.saveFails = false
        model.saveSearch(named: "Spanish")
        await until { model.savingSearch == nil && core.savedSearchWrites.count == 1 }
    }
}

extension BrowseModel {
    func setFieldTokensForTests(_ query: FilterQuery) { setQuery(query) }
}
