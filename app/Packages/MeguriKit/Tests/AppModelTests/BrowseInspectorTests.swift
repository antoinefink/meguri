@testable import AppModel
@testable import CoreBridge
@testable import Editor
import Foundation
import Testing

/// Browse's inspector (U273; `browser/workspace` V7, V12, V17; N21.3;
/// `editor/edit-flows` W2, W4, W5): what it shows for no row, one and
/// several; that a quick run through the rows mounts only the last one and
/// never shows an empty pane between two notes; that a save writes only
/// what changed, to the note shown, as one "Edit Note" step, before
/// another note shows; that a failed save keeps the row and its text; the
/// width threshold, the empty fields and Edit Note. Over a scripted search
/// session, inspector reads and a field page with no WebKit.
@MainActor @Suite struct BrowseInspectorTests {
    // MARK: What it shows (V7)

    @Test func itShowsNoSelectionOneNoteOrTheSummaryOfSeveralRows() async throws {
        let fixture = try await Fixture()
        let inspector = fixture.inspector
        #expect(inspector.content == .noSelection && inspector.note == nil)
        fixture.select([1])
        try await pollUntil { inspector.content == .note && inspector.note?.noteID == 20 }
        let note = try #require(inspector.note)
        #expect(note.notetype.id == 42 && note.card?.id == 2 && note.draft.values == ["front 20", "back 20", ""])
        #expect(fixture.page.opened == [20])
        fixture.select([1, 2, 3])
        try await pollUntil { inspector.content == .several && inspector.summary != nil }
        let summary = try #require(inspector.summary)
        #expect(summary.rows == 3 && summary.mode == .cards)
        #expect(inspector.note == nil, "the note was left (and saved) before the summary showed")
        fixture.select([])
        try await pollUntil { inspector.content == .noSelection }
        #expect(fixture.backend.saves.isEmpty, "nothing was typed, so nothing was written (W4)")
    }

    @Test func notesModeShowsTheNotesCardWithTheLowestOrdinal() async throws {
        let fixture = try await Fixture(mode: .notes)
        fixture.backend.cards[30] = [NoteCard(id: 7, ord: 0, deckId: 2, originalDeckId: 0), NoteCard(id: 3, ord: 1, deckId: 1, originalDeckId: 0)]
        fixture.select([2])
        try await pollUntil { fixture.inspector.note?.noteID == 30 }
        #expect(fixture.inspector.note?.card?.id == 7 && fixture.inspector.note?.card?.deckId == 2)
    }

    // MARK: Snappy (V17)

    @Test func aQuickRunThroughTheRowsMountsOnlyTheLastAndNeverShowsAnEmptyPane() async throws {
        let fixture = try await Fixture()
        let inspector = fixture.inspector
        fixture.select([0])
        try await pollUntil { inspector.note?.noteID == 10 }
        // Reads now wait until released: the note shown stays meanwhile.
        fixture.backend.holdReads = true
        for row in 1...4 { fixture.select([row]); await Task.yield() }
        try await pollUntil { fixture.backend.heldReads > 0 }
        #expect(inspector.note?.noteID == 10 && inspector.content == .note, "the old note stays until the new one is read")
        fixture.backend.holdReads = false
        fixture.backend.releaseReads()
        try await pollUntil { inspector.note?.noteID == 50 }
        await inspector.settled()
        #expect(fixture.page.opened == [10, 50], "the rows passed on the way were never opened: \(fixture.page.opened)")
        #expect(fixture.page.newPages == 1, "the next note opened in the same page")
        #expect(!inspector.loading)
    }

    @Test func aSlowFirstReadSaysItIsLoadingOnlyAfterHalfASecond() async throws {
        let fixture = try await Fixture()
        fixture.inspector.loadingDelay = .milliseconds(50)
        fixture.backend.holdReads = true
        fixture.select([0])
        try await pollUntil { fixture.backend.heldReads > 0 }
        #expect(!fixture.inspector.loading)
        try await pollUntil { fixture.inspector.loading }
        fixture.backend.holdReads = false
        fixture.backend.releaseReads()
        try await pollUntil { fixture.inspector.note?.noteID == 10 }
        #expect(!fixture.inspector.loading)
    }

    // MARK: Saving (V12, W2, W4)

    @Test func losingTheFocusSavesTheChangedFieldsAsOneEditNoteStep() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "new back")
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        let patch = try #require(fixture.backend.saves.first)
        #expect(patch.id == 10 && patch.fields == [1: "new back"] && patch.tags == nil)
        #expect(fixture.steps.map(\.title) == ["Edit Note"] && fixture.steps.first?.seq == fixture.backend.lastSeq)
        // A second blur with nothing new writes nothing.
        runtime.blur()
        await fixture.inspector.settled()
        #expect(fixture.backend.saves.count == 1)
        #expect(fixture.inspector.note?.draft.hasChanges == false)
    }

    @Test func anEditIsSavedToItsOwnNoteBeforeAnotherRowsNoteShows() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        // Typed, not committed yet: the flush before the row change commits it.
        runtime.typeWithoutCommit(0, "typed front")
        fixture.backend.onSave = { patch in
            // The next row's note is not shown while the save runs.
            #expect(fixture.inspector.note?.noteID == 10, "W2: the save comes first")
            #expect(patch.id == 10)
        }
        fixture.select([3])
        try await pollUntil { fixture.inspector.note?.noteID == 40 }
        #expect(fixture.backend.saves.map(\.id) == [10] && fixture.backend.saves.first?.fields == [0: "typed front"])
        #expect(fixture.page.opened == [10, 40])
    }

    @Test func aFailedSaveKeepsTheRowTheNoteAndItsText() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(2, "extra text")
        fixture.backend.failNext = .unavailable(reason: "sample")
        fixture.select([4])
        try await pollUntil { fixture.backend.saveCalls == 1 }
        await fixture.inspector.settled()
        try await pollUntil { fixture.model.results.selection == [0] }
        #expect(fixture.inspector.note?.noteID == 10 && fixture.inspector.note?.saveFailure == .notSaved)
        #expect(fixture.inspector.note?.draft.values[2] == "extra text" && fixture.page.opened == [10])
        #expect(runtime.held == false, "the page takes input again")
        // The next try saves.
        fixture.select([4])
        try await pollUntil { fixture.inspector.note?.noteID == 50 }
        #expect(fixture.backend.saves.map(\.id) == [10])
    }

    @Test func aChangeFromElsewhereReloadsTheUneditedFieldsAndKeepsTheEditedOnes() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(0, "mine")
        fixture.backend.notes[10] = NoteRecord(id: 10, notetypeId: 42, fields: ["theirs 0", "theirs 1", ""], tags: ["t"], guid: "g")
        fixture.model.changesCommitted(Self.noteUpdated(10), seq: 900)
        try await pollUntil { fixture.page.reloads == [[1]] }
        let draft = try #require(fixture.inspector.note?.draft)
        #expect(draft.values == ["mine", "theirs 1", ""] && draft.tags == ["t"])
        // The change to the note's tags came with it; the user's field saves on blur.
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        #expect(fixture.backend.saves.first?.fields == [0: "mine"] && fixture.backend.saves.first?.tags == nil)
    }

    @Test func itsOwnSaveIsNotTakenForAChangeFromElsewhere() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "b")
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        fixture.model.changesCommitted(Self.noteUpdated(10), seq: fixture.backend.lastSeq)
        await fixture.inspector.settled()
        try await Task.sleep(for: .milliseconds(20))
        #expect(fixture.page.reloads.isEmpty && fixture.backend.noteReads[10] == 1)
    }

    @Test func aDeletedNoteClosesTheEditorAndSaysSoWhenTextWasDropped() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        try await fixture.runtime?.type(0, "lost")
        var removal = Self.noteUpdated(10)
        removal = ChangeSet(wholeCollection: false, source: .syncIncoming, cards: [], notes: [ChangeSet.Row(id: 10, change: .removed)],
                            notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [],
                            mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                            skippedIncomingDecks: [], collectionRow: false)
        fixture.model.changesCommitted(removal, seq: 901)
        try await pollUntil { fixture.inspector.note == nil }
        #expect(fixture.inspector.notice != nil && fixture.backend.saves.isEmpty)
    }

    // MARK: The pane (N21.3)

    @Test func underNineHundredPointsThePaneHidesAndReturnShowsItOverTheTable() async throws {
        let fixture = try await Fixture()
        let inspector = fixture.inspector
        inspector.widthChanged(1_400)
        #expect(inspector.shown && !inspector.narrow)
        inspector.widthChanged(880)
        #expect(inspector.narrow && !inspector.shown && inspector.presented, "it hides itself; the person's choice stays")
        inspector.show()
        #expect(inspector.shown && inspector.overlay)
        inspector.toggle()
        #expect(!inspector.shown)
        inspector.widthChanged(1_000)
        #expect(inspector.shown && !inspector.overlay)
        inspector.toggle()
        #expect(!inspector.presented && !inspector.shown)
        inspector.widthChanged(700)
        inspector.widthChanged(1_200)
        #expect(!inspector.shown, "hidden by the person, it stays hidden")
    }

    @Test func hidingThePaneSavesTheNote() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        try await fixture.runtime?.type(1, "before hiding")
        fixture.inspector.toggle()
        try await pollUntil { fixture.backend.saves.count == 1 }
        #expect(fixture.backend.saves.first?.fields == [1: "before hiding"])
    }

    // MARK: Fields and Edit Note (V12, N21.8)

    @Test func emptyFieldsStayBehindTheirRowUntilItOpensAndItStaysOpenForTheNoteType() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let note = try #require(fixture.inspector.note)
        #expect(note.emptyFields == [2] && note.draft.hiddenFields == [2])
        fixture.inspector.setEmptyFieldsOpen(true)
        try await pollUntil { fixture.runtime?.hiddenSet == [] }
        #expect(fixture.inspector.emptyFieldsOpen(42))
        fixture.select([1])
        try await pollUntil { fixture.inspector.note?.noteID == 20 }
        #expect(fixture.inspector.note?.draft.hiddenFields == [], "open for that note type")
    }

    @Test func editNoteIsEnabledForOneRowAndFocusesTheFirstShownField() async throws {
        let fixture = try await Fixture()
        let commands = fixture.model.commands
        fixture.select([0, 1])
        try await pollUntil { commands.facts?.rows == 2 }
        #expect(!commands.state(.editNote).enabled)
        fixture.select([0])
        try await pollUntil { commands.facts?.rows == 1 && fixture.inspector.canEditNote }
        #expect(commands.state(.editNote).enabled)
        fixture.inspector.toggle()
        #expect(!fixture.inspector.shown)
        commands.perform(.editNote)
        try await pollUntil { fixture.runtime?.focused == 0 }
        #expect(fixture.inspector.shown, "Edit Note shows the pane")
    }

    @Test func theDeckRowRunsChangeDeckOnTheShownRow() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        try await pollUntil { fixture.inspector.decks.count == 2 }
        fixture.inspector.chooseDeck(2)
        try await pollUntil { !fixture.session.actions.isEmpty }
        #expect(fixture.session.actions.first?.card == .changeDeck(deck: 2))
        #expect(fixture.session.actions.first?.target.ranges == [0..<1])
    }

    // MARK: Pure rules

    @Test func fieldEmptinessIsE1() {
        for empty in ["", " \t\n", "<br>", "<BR/>", "<br />", "</br />", "<Br >", "<div/>", "<div />", "</div>", "<DIV>", "<br/> <div></div>\n"] {
            #expect(FieldEmptiness.isEmpty(empty), "\(empty.debugDescription)")
        }
        for full in ["x", "&nbsp;", "\u{00A0}", "\u{3000}", "<span></span>", "<p></p>", "<!-- -->", "<br  >", "<br\t>", "< br>",
                     "<br/ >", "<br //>", "<div class=\"x\">"] {
            #expect(!FieldEmptiness.isEmpty(full), "\(full.debugDescription)")
        }
    }

    @Test func theClozeNumberIsTheHighestMarkerOfEveryField() {
        #expect(FieldEditorHost.highestCloze(["no markers"]) == 0)
        #expect(FieldEditorHost.highestCloze(["a", "{{c2::x}}"]) == 2)
        #expect(FieldEditorHost.highestCloze(["{{c0::x}}"]) == 0)
        #expect(FieldEditorHost.highestCloze(["{{c01::a}}"]) == 1)
        #expect(FieldEditorHost.highestCloze(["{{c1::a {{c4::b}}}}"]) == 4)
        #expect(FieldEditorHost.highestCloze(["{{c3::open"]) == 3)
        #expect(FieldEditorHost.highestCloze(["<img alt=\"{{c5::\">"]) == 5)
        #expect(FieldEditorHost.highestCloze(["{{c1,3::x}}", "{{C2::x}}"]) == 0)
        #expect(FieldEditorHost.highestCloze(["{{c99999999999999999999::x}}"]) == 9_007_199_254_740_991)
    }

    // MARK: Fixture

    static func noteUpdated(_ id: Int64) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .syncIncoming, cards: [], notes: [ChangeSet.Row(id: id, change: .updated)],
                  notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [],
                  mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                  skippedIncomingDecks: [], collectionRow: false)
    }

    @MainActor final class Fixture {
        let session = FakeSearchSession()
        let backend = FakeInspectorBackend()
        let page = FakePage()
        let model: BrowseModel
        var steps: [(seq: UInt64, title: String)] = []
        var inspector: BrowseInspector { model.inspector }
        var runtime: FakeFieldRuntime? { inspector.note?.runtime as? FakeFieldRuntime }

        init(mode: BrowseMode = .cards) async throws {
            let browse = FakeBrowseBackend()
            browse.searchSession = session
            session.results[""] = [1, 2, 3, 4, 5]
            model = BrowseModel(backend: browse, inspectorBackend: backend)
            model.results.progressDelay = .seconds(60)
            let page = page
            inspector.makeRuntime = { draft, _ in FakeFieldRuntime(draft: draft, page: page) }
            inspector.generation = { 1 }
            inspector.stepRecorded = { [weak self] seq, title in self?.steps.append((seq, title)) }
            model.results.run(FilterQuery(), mode: mode)
            for _ in 0..<500 where model.results.list?.count != 5 { try await Task.sleep(for: .milliseconds(2)) }
            try #require(model.results.list?.count == 5)
            for _ in 0..<500 where model.results.row(at: 4) == nil { try await Task.sleep(for: .milliseconds(2)) }
        }

        func select(_ rows: Set<Int>) { model.results.select(rows) }
    }
}

/// Waits, suspended, until `condition` holds (at most five seconds).
@MainActor func pollUntil(_ condition: () -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async throws {
    let clock = ContinuousClock(), end = clock.now + .seconds(5)
    while !condition() {
        guard clock.now < end else {
            Issue.record("timed out", sourceLocation: sourceLocation)
            throw BrowseResultsTests.TimedOut()
        }
        try await Task.sleep(for: .milliseconds(2))
    }
}

/// The inspector's reads and save, scripted: note n × 10 for card n, of the
/// note type 42 (Front, Back, Extra), one card each, Extra empty.
@MainActor final class FakeInspectorBackend: BrowseInspectorBackend {
    var notes: [Int64: NoteRecord] = Dictionary(uniqueKeysWithValues: (1...5).map { card in
        let id = Int64(card) * 10
        return (id, NoteRecord(id: id, notetypeId: 42, fields: ["front \(id)", "back \(id)", ""], tags: [], guid: "g\(id)"))
    })
    var cards: [Int64: [NoteCard]] = [:]
    var noteReads: [Int64: Int] = [:]
    var holdReads = false
    private var held: [CheckedContinuation<Void, Never>] = []
    var heldReads: Int { held.count }
    var saves: [NoteFieldPatch] = []
    var saveCalls = 0
    var failNext: BridgeError?
    var onSave: ((NoteFieldPatch) -> Void)?
    private(set) var lastSeq: UInt64 = 100

    func releaseReads() {
        let waiting = held
        held = []
        waiting.forEach { $0.resume() }
    }

    func note(_ id: Int64) async throws -> NoteRecord {
        noteReads[id, default: 0] += 1
        if holdReads { await withCheckedContinuation { held.append($0) } }
        guard let note = notes[id] else { throw BridgeError.noteNotFound(id: id) }
        return note
    }
    func notetype(_ id: Int64) async throws -> NotetypeRecord {
        let fields = ["Front", "Back", "Extra"].enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: false, rtl: false, fontName: "", fontSize: 20,
                          description: "", plainText: false, collapsed: false, excludeFromSearch: false)
        }
        return NotetypeRecord(id: id, name: "Three", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [])
    }
    func cards(ofNote id: Int64) async throws -> [NoteCard] {
        cards[id] ?? [NoteCard(id: id / 10, ord: 0, deckId: 1, originalDeckId: 0)]
    }
    func deckNames() async throws -> [DeckNameEntry] {
        [DeckNameEntry(id: 1, fullName: "Default", filtered: false), DeckNameEntry(id: 2, fullName: "Vorn::Brisk", filtered: false)]
    }
    func save(_ patch: NoteFieldPatch) async throws -> NoteMutation {
        saveCalls += 1
        onSave?(patch)
        if let error = failNext { failNext = nil; throw error }
        saves.append(patch)
        lastSeq += 1
        return NoteMutation(summary: MutationSummary(seq: lastSeq, generation: 1, undoName: "Update Note",
                                                     changes: BrowseInspectorTests.noteUpdated(patch.id), pastCancelPoint: false),
                            id: patch.id, cards: [], written: true)
    }
}

/// What the field pages did, across the pages that continue one another.
@MainActor final class FakePage {
    var opened: [Int64] = []
    var newPages = 0
    var reloads: [[Int]] = []
}

/// A field page with no WebKit: typing commits into the draft, or waits
/// for the next flush; the inspector's own calls are recorded.
@MainActor final class FakeFieldRuntime: BrowseFieldRuntime {
    let draft: NoteEditorDraft
    let page: FakePage
    var onFieldBlur: (@MainActor () -> Void)?
    private(set) var editable = false
    private(set) var held = false
    private(set) var focused: Int?
    private(set) var hiddenSet: Set<Int>?
    private var pending: [Int: String] = [:]

    init(draft: NoteEditorDraft, page: FakePage, continuing: Bool = false) {
        self.draft = draft
        self.page = page
        if !continuing { page.newPages += 1 }
    }

    func start() async throws {
        page.opened.append(draft.noteID ?? 0)
        editable = true
        held = false
    }
    func flush(hold: Bool) async throws {
        if hold { held = true }
        let commits = pending.map { NoteEditorDraft.Commit(ord: $0.key, html: $0.value, unchanged: $0.value == draft.fields[$0.key].baseline) }
        pending = [:]
        try await draft.accept(commits, ifCurrent: { true })
    }
    func release() async throws { held = false }
    func setFocus(_ ord: Int) async throws { focused = ord }
    func shutDown() { editable = false }
    func successor(for draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) -> (any AddNoteFieldRuntime)? {
        guard editable else { return nil }
        editable = false
        return FakeFieldRuntime(draft: draft, page: page, continuing: true)
    }
    func reload(fields: [Int]) async throws { page.reloads.append(fields) }
    func setHiddenFields(_ ords: Set<Int>) async throws {
        draft.hiddenFields = ords
        hiddenSet = ords
    }
    func focusField(_ ord: Int) async throws { focused = ord }

    /// Typing and its commit (the page's idle or blur commit).
    func type(_ ord: Int, _ html: String) async throws {
        draft.markDirty(ord)
        try await draft.accept([.init(ord: ord, html: html, unchanged: html == draft.fields[ord].baseline)], ifCurrent: { true })
    }
    /// Typing whose commit waits for the next flush.
    func typeWithoutCommit(_ ord: Int, _ html: String) {
        draft.markDirty(ord)
        pending[ord] = html
    }
    func blur() { onFieldBlur?() }
}
