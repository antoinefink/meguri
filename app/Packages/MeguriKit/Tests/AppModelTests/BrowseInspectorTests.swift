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
    @Test func occlusionMaskAndMetadataSaveOnRowChangeAndReopenWithoutRewritingMasks() async throws {
        let fixture = try await Fixture()
        let type = OcclusionFieldFlowTests.type(tags: [3, 2, 1, 0, 4])
        fixture.backend.customNotetype = type
        let masks = OcclusionFieldFlowTests.first + " \t<br/>" + OcclusionFieldFlowTests.second
        for id in [Int64(10), 20] {
            fixture.backend.notes[id] = .init(id: id, notetypeId: type.id,
                fields: ["extra \(id)", "header \(id)", "<img src=\"missing.png\">", masks, ""], tags: ["kept"], guid: "g\(id)")
        }
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let first = try #require(fixture.inspector.note), runtime = try #require(fixture.runtime)
        #expect(first.draft.showsMaskEditor && first.draft.hostFields == [2, 3])
        #expect(first.emptyFields == [4])
        first.draft.acceptHostField(OcclusionFieldFlowTests.first, at: 3)
        runtime.typeWithoutCommit(1, "edited header")
        first.draft.setTags(["edited"])
        fixture.select([1])
        try await pollUntil { fixture.inspector.note?.noteID == 20 }
        #expect(fixture.backend.saves.count == 1)
        let saved = try #require(fixture.backend.saves.first)
        #expect(saved.id == 10 && saved.fields == [1: "edited header", 3: OcclusionFieldFlowTests.first] && saved.tags == ["edited"])
        // An obsolete source completion still holds only the departed draft.
        first.draft.acceptHostField("late old draft", at: 3)
        #expect(fixture.inspector.note?.draft.values[3] == masks)
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.noteID == 10 }
        let reopened = try #require(fixture.inspector.note)
        #expect(reopened.draft.values[3] == OcclusionFieldFlowTests.first && reopened.draft.values[1] == "edited header")
        #expect(reopened.draft.values[0] == "extra 10" && reopened.draft.values[2] == "<img src=\"missing.png\">" && reopened.draft.tags == ["edited"])
        fixture.select([])
        try await pollUntil { fixture.inspector.content == .noSelection }
        #expect(fixture.backend.saves.count == 1, "Untouched reopened masks have no patch")
    }
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

    @Test func anotherCardOfTheShownNoteWinsWhenItsCardsAreReadAgain() async throws {
        // Cards 1 and 2 belong to note 10; the first read of its cards knows only card 1.
        let fixture = try await Fixture(notesOf: [1: 10, 2: 10])
        fixture.backend.cards[10] = [NoteCard(id: 1, ord: 0, deckId: 1, originalDeckId: 0)]
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.card?.id == 1 }
        fixture.backend.cards[10] = [NoteCard(id: 1, ord: 0, deckId: 1, originalDeckId: 0),
                                     NoteCard(id: 2, ord: 1, deckId: 2, originalDeckId: 0)]
        fixture.select([1])
        try await pollUntil { fixture.inspector.note?.cards.count == 2 }
        await fixture.inspector.settled()
        #expect(fixture.inspector.note?.card?.id == 2 && fixture.inspector.note?.card?.deckId == 2,
                "the selected row's card, not the one shown before")
        #expect(fixture.page.opened == [10], "the same note: no reopen")
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

    @Test(arguments: [[UInt32(0), 4, 12], [UInt32(7), 15, 90]])
    func sparseStoredOrdinalsSaveTheLogicalPositionAndKeepTheRawFingerprint(_ ords: [UInt32]) async throws {
        let fixture = try await Fixture()
        fixture.backend.fieldOrds = ords
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "later field")
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        let patch = try #require(fixture.backend.saves.first)
        #expect(patch.fields == [1: "later field"] && patch.notetype.fields.map(\.ord) == ords)
        let raw = try #require(patch.raw)
        #expect(raw.fields.map(\.ord) == [1] && raw.fields.map(\.html) == ["later field"])
        #expect(raw.fieldLayout.map(\.ord) == ords)
        #expect(raw.fieldLayout.map(\.id) == [0, 1, 2] && raw.fieldLayout.map(\.name) == ["Front", "Back", "Extra"])
        #expect(runtime.draft.values == ["front 10", "later field", ""])
        // A foreign change still reloads by position without disturbing a
        // committed edit, even though there is no stored ordinal zero.
        fixture.backend.notes[10] = .init(id: 10, notetypeId: 42, fields: ["front 10", "later field", "external"], tags: [], guid: "g10")
        fixture.model.changesCommitted(Self.noteUpdated(10), seq: 900)
        try await pollUntil { fixture.page.reloads.contains([2]) }
        #expect(runtime.draft.values == ["front 10", "later field", "external"])
    }

    @Test func aStalePatchRemapsToTheNewLogicalPositionWithItsRawLayout() async throws {
        let fixture = try await Fixture()
        fixture.backend.fieldOrds = [7, 15, 90]
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(2, "edited extra")
        fixture.backend.failNext = .stale(table: .notetypes, key: "42")
        fixture.backend.onSave = { _ in
            fixture.backend.fieldOrder = [2, 0, 1]
            fixture.backend.fieldOrds = [9, 25, 61]
        }
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        let patch = try #require(fixture.backend.saves.first)
        #expect(fixture.backend.saveCalls == 2 && patch.fields == [0: "edited extra"])
        let raw = try #require(patch.raw)
        #expect(raw.fieldLayout.map(\.ord) == [9, 25, 61] && raw.fieldLayout.map(\.id) == [2, 0, 1])
        #expect(raw.fields.map(\.ord) == [0])
    }

    @Test(arguments: [BridgeError.commitCompleted(sequence: 101, message: "synthetic"),
                      BridgeError.mediaCommitPending(sequence: 101, message: "synthetic")])
    func remappedPostCommitAcknowledgesOnlyTheSentRevision(_ error: BridgeError) async throws {
        let fixture = try await Fixture()
        fixture.backend.fieldOrds = [7, 15, 90]
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(2, "first edit")
        fixture.backend.failNext = .unavailable(reason: "synthetic")
        let failed = await fixture.inspector.flushForBackground()
        #expect(!failed && fixture.inspector.note?.saveFailure == .notSaved)

        fixture.backend.failNext = .stale(table: .notetypes, key: "42")
        fixture.backend.onSave = { _ in
            guard fixture.backend.saveCalls == 2 else { return }
            fixture.backend.fieldOrder = [2, 0, 1]
            fixture.backend.fieldOrds = [9, 25, 61]
            fixture.backend.notes[10] = .init(id: 10, notetypeId: 42, fields: ["", "front 10", "back 10"], tags: [], guid: "g10")
        }
        fixture.backend.postCommitError = error
        var reply: CheckedContinuation<Void, Never>?
        defer { reply?.resume() }
        fixture.backend.afterSave = { _ in
            await withCheckedContinuation { reply = $0 }
        }
        let saving = Task { await fixture.inspector.flushForBackground() }
        try await pollUntil { reply != nil }
        // Input arriving after the durable write belongs to a later revision.
        try await runtime.type(2, "later edit")
        reply?.resume(); reply = nil
        let saved = await saving.value
        #expect(saved && fixture.inspector.note?.saveFailure == nil)
        #expect(fixture.backend.saveCalls == 3 && fixture.backend.saves.count == 1)
        #expect(fixture.backend.saves.first?.fields == [0: "first edit"])
        #expect(fixture.backend.saves.first?.notetype.fields.map(\.ord) == [9, 25, 61])
        #expect(runtime.draft.committedPatch()?.fields == [2: "later edit"])
        #expect(fixture.steps.isEmpty, "a post-commit error supplies no mutation summary to record")
        try await pollUntil { fixture.backend.noteReads[10, default: 0] >= 3 && !runtime.held }
        #expect(runtime.draft.values[2] == "later edit", "the requested reload preserves the later input")

        fixture.backend.afterSave = nil
        fixture.backend.onSave = nil
        fixture.backend.failNext = .stale(table: .notetypes, key: "42")
        let laterSaved = await fixture.inspector.flushForBackground()
        #expect(laterSaved && fixture.backend.saves.count == 2)
        #expect(fixture.backend.saves.last?.fields == [0: "later edit"])
        #expect(fixture.steps.count == 1, "the real result keeps normal undo bookkeeping")
        try await pollUntil { fixture.inspector.note?.draft.notetype.fields.first?.id == 2 }
        #expect(fixture.inspector.note?.draft.values == ["later edit", "front 10", "back 10"])
        let calls = fixture.backend.saveCalls
        let unchanged = await fixture.inspector.flushForBackground()
        #expect(unchanged && fixture.backend.saveCalls == calls, "acknowledged input is never resubmitted")
    }

    @Test(arguments: [BridgeError.commitCompleted(sequence: 101, message: "synthetic"),
                      BridgeError.mediaCommitPending(sequence: 101, message: "synthetic")])
    func initialPostCommitClearsAnEarlierSaveFailure(_ error: BridgeError) async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "durable edit")
        fixture.backend.failNext = .unavailable(reason: "synthetic")
        let failed = await fixture.inspector.flushForBackground()
        #expect(!failed && fixture.inspector.note?.saveFailure == .notSaved)
        fixture.backend.postCommitError = error
        let saved = await fixture.inspector.flushForBackground()
        #expect(saved && fixture.inspector.note?.saveFailure == nil)
        try await pollUntil { fixture.backend.noteReads[10, default: 0] >= 2 && !runtime.held }
        #expect(!runtime.draft.hasChanges && runtime.draft.values[1] == "durable edit")
        let calls = fixture.backend.saveCalls
        _ = await fixture.inspector.flushForBackground()
        #expect(fixture.backend.saveCalls == calls)
    }

    @Test func malformedCountsAndOrdinalsShowReadableDiagnosticsWithoutOpeningAFieldPage() async throws {
        for ords in [[UInt32(7), 7, 90], [UInt32(15), 7, 90]] {
            let fixture = try await Fixture()
            fixture.backend.fieldOrds = ords
            fixture.select([0])
            try await pollUntil { fixture.inspector.note != nil }
            #expect(fixture.inspector.note?.error == "This note type has duplicate or unordered field ordinals. Run Check Database before editing it.")
            #expect(fixture.page.opened.isEmpty && fixture.backend.saves.isEmpty)
        }
        for values in [["short"], ["too", "many", "stored", "values"]] {
            let fixture = try await Fixture()
            fixture.backend.fieldOrds = [7, 15, 90]
            fixture.backend.notes[10] = .init(id: 10, notetypeId: 42, fields: values, tags: [], guid: "g10")
            fixture.select([0])
            try await pollUntil { fixture.inspector.note != nil }
            #expect(fixture.inspector.note?.error == "The note’s field count (\(values.count)) does not match its note type (3). Run Check Database before editing it.")
            #expect(fixture.page.opened.isEmpty && fixture.backend.saves.isEmpty)
        }
    }

    @Test func aStalePatchCannotResendIntoImageOcclusionFields() async throws {
        let fixture = try await Fixture()
        fixture.backend.fieldOrds = [7, 15, 90]
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "keep my draft")
        fixture.backend.failNext = .stale(table: .notetypes, key: "42")
        fixture.backend.onSave = { _ in fixture.backend.originalStockKind = 6 }
        runtime.blur()
        try await pollUntil { fixture.inspector.note?.saveFailure == .notSaved }
        #expect(fixture.backend.saveCalls == 1 && fixture.backend.saves.isEmpty)
        #expect(runtime.draft.values[1] == "keep my draft")
    }

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

    @Test func typingBackTheOpenedTextAfterASaveIsSavedToo() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true }
        let runtime = try #require(fixture.runtime)
        try await runtime.type(1, "edited")
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 1 }
        // Back to the text the page opened with: the page says "unchanged", which is no longer what is stored.
        try await runtime.type(1, "back 10")
        runtime.blur()
        try await pollUntil { fixture.backend.saves.count == 2 }
        #expect(fixture.backend.saves.last?.fields == [1: "back 10"])
    }

    @Test func tagsTypedForOneNoteNeverReachAnother() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.noteID == 10 }
        let first = try #require(fixture.inspector.note)
        fixture.select([1])
        try await pollUntil { fixture.inspector.note?.noteID == 20 && fixture.inspector.note?.runtime?.editable == true }
        fixture.inspector.setTags(["late"], for: first)
        await fixture.inspector.settled()
        #expect(fixture.backend.saves.isEmpty && fixture.inspector.note?.draft.tags == [])
        fixture.inspector.setTags(["kept"], for: try #require(fixture.inspector.note))
        try await pollUntil { fixture.backend.saves.count == 1 }
        #expect(fixture.backend.saves.first?.id == 20 && fixture.backend.saves.first?.tags == ["kept"])
    }

    @Test func aCommandOnTheEditedRowSavesTheEditFirst() async throws {
        let fixture = try await Fixture()
        fixture.select([0])
        try await pollUntil { fixture.inspector.note?.runtime?.editable == true && fixture.model.commands.facts?.rows == 1 }
        fixture.runtime?.typeWithoutCommit(0, "before the flag")
        fixture.backend.onSave = { _ in #expect(fixture.session.actions.isEmpty, "the save comes before the command") }
        fixture.model.commands.perform(.flag(2))
        try await pollUntil { !fixture.session.actions.isEmpty }
        #expect(fixture.backend.saves.first?.fields == [0: "before the flag"])
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

        init(mode: BrowseMode = .cards, notesOf: [Int64: Int64] = [:]) async throws {
            let browse = FakeBrowseBackend()
            browse.searchSession = session
            session.notesOf = notesOf
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
    var afterSave: ((NoteFieldPatch) async -> Void)?
    var postCommitError: BridgeError?
    var fieldOrds: [UInt32] = [0, 1, 2]
    var fieldOrder = [0, 1, 2]
    var originalStockKind: Int32 = 0
    var customNotetype: NotetypeRecord?
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
        if let customNotetype { return customNotetype }
        let fields = fieldOrder.enumerated().map { position, index in
            NotetypeField(ord: fieldOrds[position], id: Int64(index), name: ["Front", "Back", "Extra"][index], sticky: false, rtl: false, fontName: "", fontSize: 20,
                          description: "", plainText: false, collapsed: false, excludeFromSearch: false)
        }
        return NotetypeRecord(id: id, name: "Three", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [],
                              originalStockKind: originalStockKind)
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
        if let stored = notes[patch.id] {
            var fields = stored.fields
            for (position, html) in patch.fields { fields[position] = html }
            notes[patch.id] = .init(id: stored.id, notetypeId: stored.notetypeId, fields: fields,
                                  tags: patch.tags ?? stored.tags, guid: stored.guid)
        }
        lastSeq += 1
        await afterSave?(patch)
        if let error = postCommitError { postCommitError = nil; throw error }
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
    /// What the page was opened with: its "unchanged" means equal to this.
    private var opened: [String]

    init(draft: NoteEditorDraft, page: FakePage, continuing: Bool = false) {
        self.draft = draft
        self.page = page
        opened = draft.values
        if !continuing { page.newPages += 1 }
    }

    func start() async throws {
        page.opened.append(draft.noteID ?? 0)
        editable = true
        held = false
    }
    func flush(hold: Bool) async throws {
        if hold { held = true }
        let commits = pending.map { NoteEditorDraft.Commit(ord: $0.key, html: $0.value, unchanged: $0.value == opened[$0.key]) }
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
    func reload(fields: [Int]) async throws {
        page.reloads.append(fields)
        for ord in fields { opened[ord] = draft.values[ord] }
    }
    func setHiddenFields(_ ords: Set<Int>) async throws {
        draft.hiddenFields = ords
        hiddenSet = ords
    }
    func focusField(_ ord: Int) async throws { focused = ord }

    /// Typing and its commit (the page's idle or blur commit).
    func type(_ ord: Int, _ html: String) async throws {
        draft.markDirty(ord)
        try await draft.accept([.init(ord: ord, html: html, unchanged: html == opened[ord])], ifCurrent: { true })
    }
    /// Typing whose commit waits for the next flush.
    func typeWithoutCommit(_ ord: Int, _ html: String) {
        draft.markDirty(ord)
        pending[ord] = html
    }
    func blur() { onFieldBlur?() }
}
