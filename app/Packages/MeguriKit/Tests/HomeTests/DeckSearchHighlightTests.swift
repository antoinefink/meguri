import CoreBridge
import Testing
@testable import Home

/// N20.1 and `decks/deck-list` H6.3: the sidebar search's rows, highlight and
/// arrows, and D18's create row.
@MainActor
@Suite struct DeckSearchHighlightTests {
    private var entries: [DeckNameEntry] {
        [DeckNameEntry(id: 11, fullName: "Languages", filtered: false),
         DeckNameEntry(id: 12, fullName: "Languages::日本語", filtered: false),
         DeckNameEntry(id: 13, fullName: "Languages::日本語::文法", filtered: false),
         DeckNameEntry(id: 14, fullName: "Languages::Español", filtered: false),
         DeckNameEntry(id: 15, fullName: "Science", filtered: false)]
    }
    private func search(_ query: String) -> DeckSearch { DeckSearch(entries: entries, listsDefault: true, query: query) }

    @Test func resultsReadAsPathsWithChevrons() {
        let found = search("文法")
        #expect(found.matches.map(\.id) == [13])
        #expect(found.matches.first?.ancestors == ["Languages", "日本語"])
        #expect(found.matches.first?.spokenPath == "Languages › 日本語 › 文法")
    }

    @Test func theFirstRowIsHighlightedOrTheCreateRowWhenNothingIsListed() {
        let listed = search("a").targets(offeringCreate: true)
        #expect(listed == [.deck(11), .deck(12), .deck(13), .deck(14), .create])
        #expect(DeckSearch.highlight(nil, in: listed) == .deck(11))
        let nothing = search("Korean").targets(offeringCreate: true)
        #expect(nothing == [.create])
        #expect(DeckSearch.highlight(nil, in: nothing) == .create)
        #expect(DeckSearch.highlight(nil, in: []) == nil)
        // A choice that is no longer listed falls back to the first row.
        #expect(DeckSearch.highlight(.deck(99), in: [.deck(2), .create]) == .deck(2))
        #expect(DeckSearch.highlight(.create, in: [.deck(2), .create]) == .create)
    }

    @Test func arrowsMoveOverAllRowsAndWrap() {
        let rows: [DeckSearchTarget] = [.deck(1), .deck(2), .create]
        #expect(DeckSearch.move(.deck(1), by: 1, in: rows) == .deck(2))
        #expect(DeckSearch.move(.deck(2), by: 1, in: rows) == .create)
        #expect(DeckSearch.move(.create, by: 1, in: rows) == .deck(1))
        #expect(DeckSearch.move(.deck(1), by: -1, in: rows) == .create)
        #expect(DeckSearch.move(nil, by: 1, in: rows) == .deck(1))
        #expect(DeckSearch.move(nil, by: -1, in: rows) == .create)
        #expect(DeckSearch.move(.deck(1), by: 1, in: []) == nil)
    }

    @Test func anEditHighlightsTheFirstRowAgain() {
        let search = DeckListSearch()
        search.text = "a"
        search.chosen = .deck(4)
        search.text = "ab"
        #expect(search.chosen == nil)
    }

    @Test func createIsOfferedOnlyAfterACompletedReadForThisTextAndGeneration() async {
        let search = DeckListSearch()
        search.text = "Korean"
        #expect(!search.offersCreate(generation: 11), "No read yet")
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
        #expect(search.offersCreate(generation: 11))
        #expect(!search.offersCreate(generation: 12), "Another generation needs its own read")
        search.text = "Korean "
        #expect(!search.offersCreate(generation: 11), "Another text needs its own read")

        // An exact match, a read of another generation and a failed read never offer it.
        search.text = "languages"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in
            DeckNameMatch(generation: 11, deck: DeckNameEntry(id: 1, fullName: "Languages", filtered: false))
        }
        #expect(search.nameCheck(generation: 11) == .exists && !search.offersCreate(generation: 11))
        search.text = "Korean"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 12, deck: nil) }
        #expect(search.nameCheck(generation: 11) == .failed && !search.offersCreate(generation: 11))
        search.retryNameCheck()
        #expect(search.nameCheck(generation: 11) == nil, "Try Again starts a new read")
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in throw BridgeError.cancelled }
        #expect(search.nameCheck(generation: 11) == .failed)
    }

    @Test func aBlankTextIsNeverOffered() async {
        let search = DeckListSearch()
        for blank in ["", "  ", "\u{3000}"] {
            search.text = blank
            await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
            #expect(!search.offersCreate(generation: 11))
        }
        search.text = "::"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
        #expect(search.offersCreate(generation: 11), "\"::\" is not blank (R58)")
    }

    @Test func aLateNameReadNeverOverwritesTheCurrentOne() async {
        let search = DeckListSearch()
        search.text = "Kor"
        let first = search.nameCheckKey(generation: 11)
        let gate = Gate()
        let late = Task { await search.checkName(first) { _ in await gate.wait(); return DeckNameMatch(generation: 11, deck: nil) } }
        await gate.entered()
        search.text = "Korean"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in
            DeckNameMatch(generation: 11, deck: DeckNameEntry(id: 20, fullName: "Korean", filtered: false))
        }
        gate.open(); await late.value
        #expect(search.nameCheck(generation: 11) == .exists, "The read for \"Kor\" ended last but belongs to an older text")
        #expect(!search.offersCreate(generation: 11))
    }

    @Test func aNewSearchHasItsOwnNameRead() async {
        let search = DeckListSearch()
        search.text = "Korean"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
        #expect(search.offersCreate(generation: 11))
        let before = search.presentation
        search.text = ""
        search.text = "Korean"
        #expect(search.presentation != before)
        #expect(!search.offersCreate(generation: 11), "A cleared and retyped field reads again")
    }

    @Test func theSnapshotIsReadPerGenerationAndTreeAndFollowsTheDefaultRule() async {
        let search = DeckListSearch()
        let key = search.snapshotKey(generation: 11, tree: 1)
        #expect(search.entries(generation: 11) == nil)
        await search.loadSnapshot(key, read: { DeckNameSnapshot(generation: 11, decks: self.entries) },
            defaultHasCards: { Issue.record("No id 1 in the snapshot: no read"); return true })
        #expect(search.entries(generation: 11)?.count == 5 && !search.listsDefault)
        #expect(search.entries(generation: 12) == nil)
        // A snapshot of another generation counts as a failed read.
        await search.loadSnapshot(search.snapshotKey(generation: 12, tree: 1),
            read: { DeckNameSnapshot(generation: 11, decks: self.entries) }, defaultHasCards: { false })
        #expect(search.snapshotFailed && search.entries(generation: 12) == nil)
        let withDefault = [DeckNameEntry(id: 1, fullName: "Default", filtered: false)] + entries
        await search.loadSnapshot(search.snapshotKey(generation: 12, tree: 2),
            read: { DeckNameSnapshot(generation: 12, decks: withDefault) }, defaultHasCards: { true })
        #expect(!search.snapshotFailed && search.listsDefault && search.entries(generation: 12)?.first?.id == 1)
    }
}


extension DeckSearchHighlightTests {
    @Test func aCancelledReadLetsTheNextViewReadTheSameKey() async {
        let search = DeckListSearch(), gate = Gate()
        let key = search.snapshotKey(generation: 11, tree: 1)
        var reads = 0
        let first = Task {
            await search.loadSnapshot(key, read: { reads += 1; await gate.wait(); return DeckNameSnapshot(generation: 11, decks: []) },
                defaultHasCards: { false })
        }
        await gate.entered()
        first.cancel(); gate.open(); await first.value
        #expect(search.entries(generation: 11) == nil, "A cancelled read publishes nothing")
        await search.loadSnapshot(key, read: { reads += 1; return DeckNameSnapshot(generation: 11, decks: []) },
            defaultHasCards: { false })
        #expect(reads == 2 && search.entries(generation: 11) != nil)
    }

    @Test func aFailedRefreshShowsTheErrorAndARetryReplacesTheResults() async {
        let search = DeckListSearch()
        let old = [DeckNameEntry(id: 11, fullName: "QZ67", filtered: false)]
        let new = [DeckNameEntry(id: 11, fullName: "QZ68", filtered: false)]
        await search.loadSnapshot(search.snapshotKey(generation: 11, tree: 1),
            read: { DeckNameSnapshot(generation: 11, decks: old) }, defaultHasCards: { false })
        await search.loadSnapshot(search.snapshotKey(generation: 11, tree: 2),
            read: { throw BridgeError.cancelled }, defaultHasCards: { false })
        #expect(search.snapshotFailed && search.entries(generation: 11) == old, "The error shows with the older results")
        search.retrySnapshot()
        await search.loadSnapshot(search.snapshotKey(generation: 11, tree: 2),
            read: { DeckNameSnapshot(generation: 11, decks: new) }, defaultHasCards: { false })
        #expect(!search.snapshotFailed && search.entries(generation: 11) == new)
    }

    #if os(macOS)
    @Test func theFieldsKeysLeaveAnInputMethodsCompositionAlone() {
        #expect(DeckSearchFieldKey(keyCode: 125, modifiers: [], repeated: false, composing: false) == .down)
        #expect(DeckSearchFieldKey(keyCode: 126, modifiers: [], repeated: true, composing: false) == .up)
        #expect(DeckSearchFieldKey(keyCode: 36, modifiers: [], repeated: false, composing: false) == .open)
        #expect(DeckSearchFieldKey(keyCode: 76, modifiers: [], repeated: false, composing: false) == .open)
        #expect(DeckSearchFieldKey(keyCode: 36, modifiers: [], repeated: true, composing: false) == nil, "A held Return opens once")
        #expect(DeckSearchFieldKey(keyCode: 36, modifiers: .shift, repeated: false, composing: false) == nil)
        for code: UInt16 in [36, 76, 125, 126] {
            #expect(DeckSearchFieldKey(keyCode: code, modifiers: [], repeated: false, composing: true) == nil,
                "Return and the arrows belong to the input method while it composes")
        }
    }
    #endif
}

@MainActor
private final class Gate {
    private var waiting: CheckedContinuation<Void, Never>?
    private var arrival: CheckedContinuation<Void, Never>?
    private var arrived = false
    func wait() async { await withCheckedContinuation { waiting = $0; arrived = true; arrival?.resume(); arrival = nil } }
    func entered() async { if arrived { return }; await withCheckedContinuation { arrival = $0 } }
    func open() { waiting?.resume(); waiting = nil }
}
