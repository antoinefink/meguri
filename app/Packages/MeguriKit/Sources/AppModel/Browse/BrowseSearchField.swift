import CoreBridge
import Foundation

/// One token of the search field as the system's token field holds it
/// (`search/filter-tokens` T1, N21.2): the query's tokens in their order,
/// and a suggestion the field took as a token when it was accepted
/// (`searchCompletion`), which the model turns into its change at once.
public nonisolated enum BrowseFieldToken: Identifiable, Hashable, Sendable {
    case token(index: Int, FilterToken)
    case suggestion(FilterSuggestion)
    /// One of a Field suggestion's three tests (T12.1).
    case fieldTest(FilterFieldTest)

    public var id: String {
        switch self {
        case .token(let index, let token): "\(index):\(token.hashValue)"
        case .suggestion(let suggestion): "suggestion:\(suggestion.hashValue)"
        case .fieldTest(let test): "test:\(test.hashValue)"
        }
    }
}

/// A Field suggestion accepted (T12.1): its three tests show next, and
/// Contains… takes `word` as its text when the word is not the field's name.
public nonisolated struct SuggestedField: Hashable, Sendable {
    public let name: String
    public let word: String
    /// The text Contains… takes (T12.1), or nil when it asks for one.
    public var containsText: String? {
        word.isEmpty || word.lowercased() == name.lowercased() ? nil : word
    }
}

/// Save Search… asking for a name (T13), then, for a name already saved,
/// whether to replace it.
public nonisolated struct SavingSearch: Hashable, Sendable {
    public var name = ""
    /// Which presentation this is: a write lands only on the one that started it.
    public internal(set) var id: UInt64 = 0
    /// The name the question asks about ("A saved search named … already exists").
    public internal(set) var replacing: String?
    /// The write is on its way: the sheet stays until it lands.
    public internal(set) var saving = false
    /// The last write failed: the sheet keeps the name and says so.
    public internal(set) var failed = false

    mutating func markFailed() {
        saving = false
        failed = true
    }
}

/// Contains… asking for its text (T10): of the Field token at `token`, or
/// for a new Field value of `field` from the suggestions.
public nonisolated struct EditingContains: Hashable, Sendable {
    public let field: String
    public let token: Int?
    /// The Contains value it edits in place, when there is one.
    public let replacing: String?
    public var text: String
}

extension FilterKind {
    /// The suggestions' section whose value list holds this kind's values
    /// (T12.1), nil for a kind with no list (Duplicates, Studied On).
    public nonisolated var section: FilterSection? {
        switch self {
        case .deck: .decks
        case .tag: .tags
        case .notetype: .notetypes
        case .cardType: .cardTypes
        case .state: .cardState
        case .flag: .flag
        case .due: .due
        case .added: .added
        case .edited: .edited
        case .studied: .studied
        case .field: .fields
        case .duplicates, .studiedOn: nil
        }
    }
}

extension FilterValue {
    /// No Flag, Any Flag and Untagged, which never stand in an is-not token (T10).
    public nonisolated var isOneSided: Bool { self == .noFlag || self == .anyFlag || self == .untagged }
}

extension FilterToken {
    /// T10: Is Not is disabled while the token holds No Flag, Any Flag or Untagged.
    public nonisolated var canNegate: Bool {
        guard case .value(let values, _) = self else { return false }
        return !values.contains { $0.isOneSided }
    }
}

extension FilterQuery {
    /// T10, T12: `value` added to its kind's token (made is {v} when there
    /// is none); a value already there changes nothing. No Flag, Any Flag
    /// and Untagged into an is-not token make it is {v} (T10's normal form).
    public nonisolated func adding(_ value: FilterValue) -> FilterQuery {
        guard let index = tokens.firstIndex(where: { $0.kind == value.kind }),
              case .value(var values, let negated) = tokens[index] else { return clicking(value) }
        if negated, value.isOneSided { return clicking(value) }
        guard !values.contains(where: { $0.isSameValue(value) }) else { return self }
        values.append(value)
        var query = self
        query.tokens[index] = .value(values, negated: negated)
        return query
    }

    /// The text without the word at its end (T12.2's w, the caret at the end).
    nonisolated func droppingWord() -> FilterQuery {
        var query = self
        query.text = String(text.dropLast(Self.word(of: text).count))
        return query
    }

    /// T12.2: the run of characters after the last White_Space, the caret at the end.
    nonisolated static func word(of text: String) -> Substring {
        guard let space = text.lastIndex(where: \.isWhitespace) else { return text[...] }
        return text[text.index(after: space)...]
    }
}

/// The search field (U271; `search/filter-tokens` T8, T10, T12, T13;
/// N21.2): live search, commit, the token menus, the suggestions and Save
/// Search. Every change of the query here is direct: none asks anything,
/// none is an undo step (T10); only Save Search writes the collection.
extension BrowseModel {
    /// How long a keystroke waits before its search runs (T8: at most 60 ms);
    /// a newer keystroke cancels it.
    static let liveDelay: Duration = .milliseconds(40)
    /// T10: a token's menu offers up to this many values after its own.
    static let offeredValues = 8
    /// D6.5 step 4: the accepted rows kept, newest first.
    static let recentLimit = 30

    // MARK: The field

    /// The tokens the field shows (T1), in the query's order.
    public var fieldTokens: [BrowseFieldToken] {
        query.tokens.enumerated().map { .token(index: $0.offset, $0.element) }
    }

    /// The field's tokens after the system changed them: a token removed
    /// (⌫, the clear button), or a suggestion it took as a token, which is
    /// accepted here (T12) and never shows.
    public func setFieldTokens(_ next: [BrowseFieldToken]) {
        var kept: [FilterToken] = [], accepted: [FilterSuggestion] = [], tests: [FilterFieldTest] = []
        for token in next {
            switch token {
            case .token(_, let token): kept.append(token)
            case .suggestion(let suggestion): accepted.append(suggestion)
            case .fieldTest(let test): tests.append(test)
            }
        }
        if !accepted.isEmpty || !tests.isEmpty {
            // The field emptied the text first: it comes back before the row is accepted.
            if query.text.isEmpty, let cleared = clearedThisTurn {
                queryRevision &+= 1
                query.text = cleared
            }
            acceptedThisTurn = true
            Task { acceptedThisTurn = false }
        }
        // A row read for another text or scope stays on screen until the new
        // rows land (no flash), but it is inert.
        if !suggestionsCurrent { accepted = [] }
        if kept != query.tokens {
            pendingSearch = nil
            setQuery(FilterQuery(tokens: kept, text: query.text))
        }
        accepted.forEach(accept)
        tests.forEach(acceptFieldTest)
    }

    /// T8: every edit of the text runs the query at once, unless D5 finds
    /// the text incomplete at the caret (the last results stay). A paste
    /// commits. The field reports no caret: it is taken at the text's end.
    public func editText(_ text: String) {
        edit(text, pasteCommits: true)
    }

    /// An edit of the text; `pasteCommits` false for text the model puts
    /// in itself (a syntax candidate), which commits only when it says so.
    private func edit(_ text: String, pasteCommits: Bool) {
        guard text != query.text else { return }
        let before = query.text
        if text.isEmpty, !before.isEmpty {
            // The field emptying its text after it took a suggestion: the text stays.
            if acceptedThisTurn { return }
            clearedThisTurn = before
            Task { clearedThisTurn = nil }
        }
        pendingSearch = nil
        queryRevision &+= 1
        query.text = text
        // V10: any change of the query but the Duplicates token's removal drops the previous query.
        duplicates?.previous = nil
        suggestionScope = nil
        suggestedField = nil
        if fieldFocused { refreshSuggestions() }
        // A paste: two or more characters arrive in one change (T8), also
        // over a selection the paste replaces.
        if pasteCommits, Self.inserted(before, text) >= 2 {
            return commitText()
        }
        liveRun?.cancel()
        let revision = queryRevision, epoch = epoch, query = query, backend = backend
        let checks = !query.text.isEmpty && query.text != committedText
        liveRun = Task {
            try? await Task.sleep(for: Self.liveDelay)
            guard !Task.isCancelled, revision == queryRevision, self.epoch == epoch, ready() else { return }
            if checks, (try? await backend.isIncomplete(query.text, caret: query.text.utf16.count)) == true { return }
            guard !Task.isCancelled, revision == queryRevision, self.epoch == epoch else { return }
            keepRunText()
            showResults()
        }
    }

    /// How many characters `after` has in place of the part of `before`
    /// they replaced (the part between their common start and end).
    nonisolated static func inserted(_ before: String, _ after: String) -> Int {
        let head = zip(after, before).prefix { $0 == $1 }.count
        let room = min(after.count, before.count) - head
        let tail = min(zip(after.reversed(), before.reversed()).prefix { $0 == $1 }.count, room)
        return after.count - head - tail
    }

    /// T8's commit (Return, a paste, accepting a suggestion, the field
    /// losing the focus): syntax in the text becomes tokens after the
    /// query's own; text that stays text stays as typed. It runs at once.
    public func commitText() {
        liveRun?.cancel()
        guard ready() else { return }
        let revision = queryRevision, epoch = epoch, query = query, backend = backend
        Task {
            let committed = (try? await backend.commit(query)) ?? query
            guard revision == queryRevision, self.epoch == epoch else { return }
            committedText = committed.text
            // Nothing to read into tokens and already shown: no second run.
            if committed == query, lastRun == query, !results.isDimmed, !results.failed { return }
            setQuery(committed)
        }
    }

    /// The field took or lost the focus: the suggestions show while it has
    /// it (T12); losing it commits (T8).
    public func setFieldFocused(_ focused: Bool) {
        guard focused != fieldFocused else { return }
        fieldFocused = focused
        if focused {
            refreshSuggestions()
        } else {
            // Contains… or Save Search took the focus: the Field suggestion
            // they finish stays.
            if editingContains == nil {
                suggestionScope = nil
                suggestedField = nil
            }
            if !query.text.isEmpty { commitText() }
        }
    }

    /// Find… (⌘F, N21.8): the field takes the focus (T10).
    public func focusField() {
        focusRequests &+= 1
    }

    /// The clear button, and Esc a second time (T10, R1092): the empty query.
    public func clearField() {
        pendingSearch = nil
        suggestionScope = nil
        suggestedField = nil
        committedText = nil
        setQuery(FilterQuery())
    }

    /// Esc while the suggestions show a value list or a field's tests: back
    /// to the suggestions of the text.
    public func leaveSuggestionList() -> Bool {
        guard suggestionScope != nil || suggestedField != nil else { return false }
        suggestionScope = nil
        suggestedField = nil
        refreshSuggestions()
        return true
    }

    // MARK: Suggestions (T12)

    /// The suggestions for the query and the caret (its text's end), or the
    /// open value list; one core call on the interactive lane, a newer one
    /// superseding it.
    func refreshSuggestions() {
        suggestionsToken &+= 1
        guard ready(), suggestedField == nil else {
            if suggestedField == nil { suggestions = [] }
            return
        }
        let token = suggestionsToken, epoch = epoch, query = query, scope = suggestionScope, backend = backend
        let recent = device.recentSuggestions ?? []
        Task {
            let rows = try? await backend.suggestions(for: query, caret: query.text.utf16.count, scope: scope, recent: recent)
            guard token == suggestionsToken, self.epoch == epoch else { return }
            // A failed read leaves no rows of an older text behind.
            suggestions = rows ?? []
            suggestionsText = rows == nil ? nil : query.text
            suggestionsReadScope = scope
        }
    }

    /// T12: what accepting a suggestion does.
    public func accept(_ suggestion: FilterSuggestion) {
        pendingSearch = nil
        remember(suggestion.key)
        switch suggestion.row {
        case .contains:
            commitText()
        case .builtIn(let search):
            leaveLists()
            setQuery(search.query)
        case .savedSearch(_, let text):
            leaveLists()
            replace(byReading: text)
        case .filterBy(let section), .showAll(let section):
            suggestedField = nil
            suggestionScope = section
            refreshSuggestions()
        case .value(let value), .cardType(let value, _):
            add(value)
        case .field(let name, _):
            // A read still on its way must not fill the list under the tests.
            suggestionsToken &+= 1
            suggestionScope = nil
            suggestedField = SuggestedField(name: name, word: String(FilterQuery.word(of: query.text)))
            suggestions = []
        case .saveSearch:
            startSavingSearch()
        case .syntax(let candidate):
            acceptSyntax(candidate)
        }
    }

    /// A value from the suggestions or a value list: added to its kind's
    /// token (T8: never a second token of a kind), the typed word gone.
    /// The typed word goes in a value list too: the core filters that list
    /// by it. Accepting is a commit (T8): syntax left in the text reads now.
    private func add(_ value: FilterValue) {
        leaveLists()
        setQuery(query.droppingWord().adding(value))
        if !query.text.isEmpty { commitText() }
    }

    private func leaveLists() {
        suggestionScope = nil
        suggestedField = nil
    }

    /// A Field suggestion's test (T12.1): Contains takes the word, or asks.
    public func acceptFieldTest(_ test: FilterFieldTest) {
        guard let field = suggestedField else { return }
        if case .contains = test {
            guard let text = field.containsText else {
                editingContains = EditingContains(field: field.name, token: nil, replacing: nil, text: "")
                return
            }
            suggestedField = nil
            setQuery(query.droppingWord().adding(.field(field.name, .contains(text))))
            return
        }
        // The word found the field; only Contains… takes it as its text (T12.1).
        suggestedField = nil
        setQuery(query.droppingWord().adding(.field(field.name, test)))
    }

    /// The suggestions shown were read for the text and the scope the field
    /// holds now.
    var suggestionsCurrent: Bool {
        suggestionsText == query.text && suggestionsReadScope == suggestionScope
    }

    /// D6: a syntax candidate's text put in place; one that completes commits.
    private func acceptSyntax(_ candidate: SyntaxCandidate) {
        // Its range is an offset into the text it was made for; once the text
        // changed, the row is not applied.
        guard suggestionsCurrent else { return }
        guard let insert = candidate.insert else {
            if candidate.kind == .asTyped { commitText() }
            return
        }
        let utf16 = Array(query.text.utf16)
        guard insert.range.lowerBound >= 0, insert.range.upperBound <= utf16.count else { return }
        let text = String(decoding: utf16[..<insert.range.lowerBound], as: UTF16.self) + insert.text
            + String(decoding: utf16[insert.range.upperBound...], as: UTF16.self)
        edit(text, pasteCommits: false)
        if candidate.completes { commitText() }
    }

    /// D6.5 step 4: the row accepted goes first in the device's list.
    private func remember(_ key: String?) {
        guard let key else { return }
        var recent = device.recentSuggestions ?? []
        recent.removeAll { $0 == key }
        recent.insert(key, at: 0)
        device.recentSuggestions = Array(recent.prefix(Self.recentLimit))
        saveDevice()
    }

    /// A suggestion's or a menu value's count in the current mode (T12.3
    /// step 4): the sidebar's, or the suggestions' own; nil until known.
    public func count(of value: FilterValue) -> UInt64? {
        switch value {
        case .deck(let id, _, _): counts[.deck(id)]
        case .tag(let name): counts[.tag(name)]
        case .notetype(let id, _): counts[.notetype(id)]
        case .state(.leech): counts[.builtIn(.leeches)]
        case .state(let state): counts[.state(state)]
        case .flag(let flag): counts[.flag(flag)]
        case .noFlag: counts[.flag(0)]
        default: valueCounts[value]
        }
    }

    public func count(of search: FilterBuiltIn) -> UInt64? { counts[.builtIn(search)] }
    public func count(ofSaved name: String) -> UInt64? { counts[.saved(name)] }

    // MARK: Token menus (T10)

    /// The values a kind's token menu offers after the token's own, in the
    /// suggestion order (T12), read once per change of the tokens or names.
    func refreshTokenOffers() {
        offersToken &+= 1
        let kinds = Set(query.tokens.compactMap { $0.kind }.filter { $0.section != nil && !$0.isField })
        guard ready(), !kinds.isEmpty else { return }
        let token = offersToken, epoch = epoch, backend = backend, recent = device.recentSuggestions ?? []
        // A menu shows the token's own values and 8 others: no more are kept.
        let needed = Dictionary(query.tokens.compactMap { token -> (FilterKind, Int)? in
            guard case .value(let values, _) = token, let kind = token.kind else { return nil }
            return (kind, values.count + Self.offeredValues)
        }, uniquingKeysWith: max)
        Task {
            var offers: [FilterKind: [FilterValue]] = [:]
            for kind in kinds {
                guard let section = kind.section,
                      let rows = try? await backend.suggestions(for: FilterQuery(), caret: 0, scope: section, recent: recent) else { continue }
                offers[kind] = Array(rows.lazy.compactMap { row -> FilterValue? in
                    switch row.row {
                    case .value(let value), .cardType(let value, _): value
                    default: nil
                    }
                }.prefix(needed[kind] ?? Self.offeredValues))
            }
            guard token == offersToken, self.epoch == epoch else { return }
            tokenOffers = offers
        }
    }

    /// The values token `index`'s menu lists unchecked: the kind's others,
    /// up to 8; never No Flag, Any Flag or Untagged in an is-not token.
    public func offeredValues(_ index: Int) -> [FilterValue] {
        guard query.tokens.indices.contains(index), case .value(let values, let negated) = query.tokens[index],
              let kind = query.tokens[index].kind else { return [] }
        let others = (tokenOffers[kind] ?? []).filter { offer in
            !values.contains { $0.isSameValue(offer) } && !(negated && offer.isOneSided)
        }
        return Array(others.prefix(Self.offeredValues))
    }

    /// Is or Is Not (T10).
    public func setNegated(_ index: Int, _ negated: Bool) {
        guard query.tokens.indices.contains(index), case .value(let values, let was) = query.tokens[index], was != negated,
              !negated || query.tokens[index].canNegate else { return }
        var next = query
        next.tokens[index] = .value(values, negated: negated)
        setQuery(next)
    }

    /// A value checked or unchecked in its token's menu (T10): unchecking
    /// the last removes the token.
    public func toggle(_ value: FilterValue, in index: Int) {
        guard query.tokens.indices.contains(index), case .value(var values, let negated) = query.tokens[index] else { return }
        var next = query
        if let present = values.firstIndex(where: { $0.isSameValue(value) }) {
            values.remove(at: present)
            if values.isEmpty { next.tokens.remove(at: index) } else { next.tokens[index] = .value(values, negated: negated) }
        } else {
            guard value.kind == query.tokens[index].kind, !(negated && value.isOneSided) else { return }
            values.append(value)
            next.tokens[index] = .value(values, negated: negated)
        }
        setQuery(next)
    }

    /// Add Another <kind>… (T10): the suggestions scoped to that kind.
    public func addAnother(to index: Int) {
        guard query.tokens.indices.contains(index), let section = query.tokens[index].kind?.section else { return }
        suggestedField = nil
        suggestionScope = section
        focusField()
        refreshSuggestions()
    }

    /// Remove Filter (⌫).
    public func removeToken(_ index: Int) {
        guard query.tokens.indices.contains(index) else { return }
        var next = query
        next.tokens.remove(at: index)
        setQuery(next)
    }

    /// An advanced token's Edit (T6): its text goes back into the field in
    /// place of the token, for a commit to read again.
    public func editAdvanced(_ index: Int) {
        guard query.tokens.indices.contains(index), case .advanced(let text) = query.tokens[index] else { return }
        var next = query
        next.tokens.remove(at: index)
        next.text = query.text.isEmpty ? text : text + " " + query.text
        committedText = nil
        setQuery(next)
        focusField()
    }

    /// A Field token's Is Empty or Has Content (T10): checked or unchecked as a value.
    public func toggleFieldTest(_ test: FilterFieldTest, in index: Int) {
        guard query.tokens.indices.contains(index), case .field(let field)? = query.tokens[index].kind else { return }
        toggle(.field(field, test), in: index)
    }

    /// A Field token's Contains…: asks for the text, in place of the one it has.
    public func editContains(in index: Int) {
        guard query.tokens.indices.contains(index), case .value(let values, _) = query.tokens[index],
              case .field(let field)? = query.tokens[index].kind else { return }
        let current = values.lazy.compactMap { value -> String? in
            if case .field(_, .contains(let text)) = value { text } else { nil }
        }.first
        editingContains = EditingContains(field: field, token: index, replacing: current, text: current ?? "")
    }

    /// Contains…'s text given: the Field value made or edited in place. An
    /// empty text changes nothing (T3.1 refuses it).
    public func finishContains(_ text: String?) {
        guard let editing = editingContains else { return }
        finishContains(editing, text)
    }

    /// Contains… dismissed without an answer.
    public func dismissContains() {
        editingContains = nil
    }

    /// Contains…'s answer for the question `editing` asked (the alert hands
    /// it back, whatever order it clears its binding in).
    public func finishContains(_ editing: EditingContains, _ text: String?) {
        editingContains = nil
        guard let text, !text.isEmpty else { return }
        let value = FilterValue.field(editing.field, .contains(text))
        guard let index = editing.token else {
            let base = suggestedFieldWordTyped ? query.droppingWord() : query
            suggestedField = nil
            return setQuery(base.adding(value))
        }
        guard query.tokens.indices.contains(index), case .value(var values, let negated) = query.tokens[index],
              query.tokens[index].kind == .field(editing.field) else { return }
        if let replacing = editing.replacing,
           let at = values.firstIndex(of: .field(editing.field, .contains(replacing))) {
            values[at] = value
        } else if !values.contains(value) {
            values.append(value)
        }
        var next = query
        next.tokens[index] = .value(values, negated: negated)
        setQuery(next)
    }

    /// The word typed matched the field's name, so it goes with the token.
    private var suggestedFieldWordTyped: Bool {
        guard let field = suggestedField else { return false }
        return !field.word.isEmpty
    }

    // MARK: Save Search (T13)

    /// Save Search…: asks for a name.
    public func startSavingSearch() {
        guard !query.isEmpty else { return }
        savingPresentations &+= 1
        savingSearch = SavingSearch(id: savingPresentations)
    }

    /// The name given: a name already saved asks whether to replace it
    /// first; otherwise the query's run text is saved under it.
    public func saveSearch(named name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard savingSearch?.saving != true else { return }
        guard !name.isEmpty else {
            savingSearch = nil
            return
        }
        let presentation = savingSearch?.id ?? 0
        if savedSearches.contains(where: { $0.name == name }), savingSearch?.replacing != name {
            savingSearch = SavingSearch(name: name, id: presentation, replacing: name)
            return
        }
        // The sheet stays, with its name, until the write lands; a failed
        // write says so and keeps the name for another try.
        savingSearch = SavingSearch(name: name, id: presentation, replacing: nil, saving: true, failed: false)
        let query = query, backend = backend, epoch = epoch
        // Lands only on the presentation that started it, while it still waits.
        func current() -> Bool { self.epoch == epoch && savingSearch?.id == presentation && savingSearch?.saving == true }
        Task {
            do {
                let text = try await backend.runText(query)
                try await backend.saveSearch(name: name, text: text)
                guard current() else { return }
                savingSearch = nil
            } catch {
                guard current() else { return }
                savingSearch?.markFailed()
            }
        }
    }

    public func cancelSavingSearch() {
        savingSearch = nil
    }

    /// The replace question's Replace: `name` saved over the one there.
    public func replaceSavedSearch(named name: String) {
        savingSearch = SavingSearch(name: name, id: savingSearch?.id ?? 0, replacing: name)
        saveSearch(named: name)
    }

    /// The replace question's Cancel: back to the name, to choose another.
    public func keepNaming() {
        savingSearch?.replacing = nil
    }
}

extension FilterKind {
    nonisolated var isField: Bool {
        if case .field = self { true } else { false }
    }
}
