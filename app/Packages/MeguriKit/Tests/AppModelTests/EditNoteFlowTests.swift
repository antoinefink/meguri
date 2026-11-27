import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// `review/edit-current` E2 and E3 at the model level (U184's
/// EditCurrentCoordinatorTests): the draft and its one patch, the discard
/// question, the foreign-change merge and the review's hold.
@MainActor
@Suite struct EditNoteDraftTests {
    static func type() -> NotetypeRecord {
        let fields = ["Prompt", "Meaning", "Source"].enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: false,
                rtl: false, fontName: "", fontSize: 20, description: "", plainText: true,
                collapsed: false, excludeFromSearch: false)
        }
        return .init(id: 42, name: "Three fields", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [], originalStockKind: 0)
    }
    static func draft(_ values: [String] = ["p", "m", "s"], tags: [String] = ["t"]) throws -> NoteEditorDraft {
        try NoteEditorDraft(editing: NoteRecord(id: 7, notetypeId: 42, fields: values, tags: tags, guid: "g"),
            notetype: type(), deck: DeckNameEntry(id: 3, fullName: "Languages::Filtered", filtered: true), generation: 1)
    }
    static func type(_ draft: NoteEditorDraft, _ ord: Int, _ html: String) async throws {
        draft.markDirty(ord)
        try await draft.accept([.init(ord: ord, html: html, unchanged: html == draft.fields[ord].baseline)], ifCurrent: { true })
    }

    @Test func donePatchesOnlyTheChangedFields() async throws {
        let draft = try Self.draft()
        #expect(draft.noteID == 7 && !draft.hasChanges && draft.patch() == nil)
        draft.markDirty(1)
        // Input not committed yet is a change, but only a flush makes it sendable.
        #expect(draft.hasChanges && draft.patch() == nil)
        try await draft.accept([.init(ord: 1, html: "meaning", unchanged: false)], ifCurrent: { true })
        let patch = try #require(draft.patch())
        #expect(patch.id == 7 && patch.notetype.id == 42 && patch.fields == [1: "meaning"] && patch.tags == nil)
    }

    @Test func aFieldEditedBackToItsStoredValueIsNoChange() async throws {
        let draft = try Self.draft()
        try await Self.type(draft, 0, "x")
        #expect(draft.hasChanges)
        try await Self.type(draft, 0, "p")
        #expect(!draft.hasChanges && draft.patch() == nil)
    }

    @Test func aForeignChangeReloadsOnlyTheFieldsWithoutAnEdit() async throws {
        let draft = try Self.draft()
        try await Self.type(draft, 0, "mine")
        draft.markDirty(2)
        let reloaded = draft.reloadUnchanged(stored: ["theirs 0", "theirs 1", "theirs 2"], tags: ["u"])
        #expect(reloaded == [1])
        #expect(draft.values == ["mine", "theirs 1", "s"] && draft.tags == ["u"])
        try await draft.accept([.init(ord: 2, html: "s", unchanged: true)], ifCurrent: { true })
        let patch = try #require(draft.patch())
        #expect(patch.fields == [0: "mine"] && patch.tags == nil)
    }

    @Test func onlyEditModeShowsAFilteredDeck() throws {
        #expect(throws: (any Error).self) {
            try NoteEditorDraft(generation: 1, notetype: Self.type(), deck: DeckNameEntry(id: 3, fullName: "F", filtered: true))
        }
        #expect(try Self.draft().deck.filtered)
        #expect(EditNoteFlow.lastComponent("Languages::Japanese::Verbs") == "Verbs")
    }
}

extension ReviewCoordinatorTests {
    @Test func editNoteHoldsInputAndTheTimerAndRendersOnlyWhenItCloses() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.now = 10
        fixture.model.reveal(); await fixture.model.settled()
        #expect(fixture.model.canEditNote)
        fixture.now = 12
        #expect(fixture.model.beginEditingNote())
        #expect(fixture.model.editingNote && !fixture.model.canRate && !fixture.model.canUndo && !fixture.model.canEditNote)
        #expect(!fixture.model.beginEditingNote())
        fixture.model.rate(.good)
        #expect(fixture.client.answers == 0)
        let reads = fixture.client.headReads, presented = fixture.surface.presented
        // A change report while the form is open is held, not rendered.
        fixture.model.requestRefresh()
        await fixture.model.settled()
        #expect(fixture.client.headReads == reads && fixture.surface.presented == presented)
        #expect(fixture.model.phase == .answer)
        fixture.now = 20
        fixture.model.endEditingNote(saved: false)
        await fixture.model.settled()
        #expect(fixture.client.headReads == reads + 1)
        #expect(fixture.surface.presented == presented && fixture.model.phase == .answer)
        fixture.now = 23
        fixture.model.rate(.good); await fixture.model.settled()
        // 12 s before the form and 3 s after it; the 8 s it was open do not count.
        #expect(fixture.client.elapsed == [15_000])
        _ = await fixture.model.end()
    }

    @Test func aSavedEditPresentsTheSameSideAgainAndRestartsTheTimer() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.now = 10
        fixture.model.reveal(); await fixture.model.settled()
        fixture.now = 12
        #expect(fixture.model.beginEditingNote())
        fixture.now = 20
        fixture.model.endEditingNote(saved: true)
        await fixture.model.settled()
        #expect(fixture.surface.presented == [1, 1] && fixture.surface.reveals == 2)
        #expect(fixture.model.phase == .answer && fixture.model.item?.cardId == 1 && fixture.model.record.answers.isEmpty)
        fixture.now = 23
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.elapsed == [3_000])
        _ = await fixture.model.end()
    }
}

/// A field page with no WebKit: typing is a commit into the draft.
@MainActor final class EditNoteRuntime: AddNoteFieldRuntime {
    let draft: NoteEditorDraft
    let isCurrent: @MainActor () -> Bool
    private(set) var reloads: [[Int]] = []
    private(set) var wasShutDown = false
    init(_ draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) { self.draft = draft; self.isCurrent = isCurrent }
    func start() async throws {}
    func flush(hold: Bool) async throws {}
    func release() async throws {}
    func setFocus(_ ord: Int) async throws { draft.setFocus(ord) }
    func shutDown() { wasShutDown = true }
    func reload(fields: [Int]) async throws { reloads.append(fields) }
    func type(_ ord: Int, _ html: String) async throws {
        draft.markDirty(ord)
        try await draft.accept([.init(ord: ord, html: html, unchanged: html == draft.fields[ord].baseline)], ifCurrent: isCurrent)
    }
}

extension ReviewCollectionStartupTests {
    @Test func editNoteSavesOnePatchAtDoneAndKeepsTheSession() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, defaults: nil),
            providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
        let surface = ReviewCoordinatorTests.Surface()
        owner.makeReviewSurface = { _ in surface }
        do {
            await owner.start()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            let suffix = UUID().uuidString
            let deck = try await CoreDeckService().addDeck(name: "Languages::Edit \(suffix)")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Edit QA \(suffix)", cloze: false,
                fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
            let notes = CoreNoteService()
            let note = try await notes.addNote(.init(notetypeId: type.id, fields: ["Question", "Answer"],
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            await owner.startStudy(deckID: deck.id)
            let review = try #require(owner.review)
            try #require(review.phase == .question && review.item?.noteId == note.id)
            let card = try #require(review.item?.cardId)
            let flow = owner.editNoteFlow
            var page: EditNoteRuntime?
            func open() async throws -> EditNoteRuntime {
                page = nil
                try #require(await flow.open { draft, current in
                    let runtime = EditNoteRuntime(draft, isCurrent: current); page = runtime; return runtime
                })
                return try #require(page)
            }
            func stored() async throws -> [String] { try await notes.readNote(id: note.id).fields }
            func undoName() async throws -> String? {
                if case .available(let step) = try await CoreUndoService().peekUndo(scope: .global) { return step.name }
                return nil
            }
            func waitUntil(line: Int = #line, _ condition: () -> Bool) async throws {
                var tries = 0
                while !condition(), tries < 500 { tries += 1; try await Task.sleep(for: .milliseconds(10)) }
                try #require(condition(), "waited at line \(line)")
            }

            // E1: opening writes nothing and holds the review.
            #expect(flow.canOpen)
            let first = try await open()
            #expect(flow.isPresented && review.editingNote && !review.canReveal && !flow.canOpen)
            #expect(flow.subtitle == .init(deck: "Edit \(suffix)", notetype: "Edit QA \(suffix)") && flow.cardID == card)
            // Cancel with nothing changed closes without asking.
            #expect(await flow.requestCancel())
            #expect(flow.discardConfirmation == nil && !flow.isPresented && !review.editingNote && first.wasShutDown)

            // Cancel with a change asks; Keep Editing keeps the draft, Discard drops it.
            let second = try await open()
            try await second.type(1, "Dropped")
            let kept = Task { await flow.requestCancel() }
            try await waitUntil { flow.discardConfirmation != nil }
            flow.answerDiscard(try #require(flow.discardConfirmation), discard: false)
            #expect(await kept.value == false && flow.isPresented && flow.draft?.values == ["Question", "Dropped"])
            let dropped = Task { await flow.requestCancel() }
            try await waitUntil { flow.discardConfirmation != nil }
            flow.answerDiscard(try #require(flow.discardConfirmation), discard: true)
            #expect(await dropped.value && !flow.isPresented)
            #expect(try await stored() == ["Question", "Answer"])
            let held = try #require(surface.presented.last)
            #expect(held == card)

            // E7: a navigation that drops the review while the form holds a change
            // presents the review again, so its question can show; Keep Editing stays.
            let fourth = try await open()
            try await fourth.type(1, "Kept")
            owner.navigation.endReview(); owner.navigationChanged()
            try await waitUntil { flow.discardConfirmation != nil }
            #expect(owner.navigation.review != nil && owner.review === review)
            flow.answerDiscard(try #require(flow.discardConfirmation), discard: false)
            try await waitUntil { flow.discardConfirmation == nil && !flow.guardingClose }
            #expect(owner.navigation.review != nil && owner.review === review && flow.isPresented)
            let discarded = Task { await flow.requestCancel() }
            try await waitUntil { flow.discardConfirmation != nil }
            flow.answerDiscard(try #require(flow.discardConfirmation), discard: true)
            #expect(await discarded.value && !flow.isPresented)

            // Done with no change writes nothing.
            let undoBefore = try await undoName()
            _ = try await open()
            #expect(await flow.save())
            #expect(!flow.isPresented)
            #expect(try await stored() == ["Question", "Answer"])
            #expect(try await undoName() == undoBefore)

            // E2: a change made elsewhere reloads the untouched field; the edited one keeps the draft.
            let third = try await open()
            try await third.type(0, "Mine")
            _ = try await notes.updateNote(.init(id: note.id, fields: ["Theirs", "Their answer"], tags: []))
            try await waitUntil { flow.draft?.fields[1].baseline == "Their answer" }
            await flow.reloadCompletion?.value
            #expect(flow.draft?.values == ["Mine", "Their answer"] && third.reloads == [[1]])

            // Done sends one patch of the edited field: one "Update Note" step, and
            // the session keeps its card, its side and its place.
            #expect(await flow.save())
            #expect(!flow.isPresented && !review.editingNote)
            #expect(try await stored() == ["Mine", "Their answer"])
            #expect(try await undoName() == "Update Note")
            await review.settled()
            #expect(review.item?.cardId == card && review.phase == .question && review.record.answers.isEmpty)
            #expect(owner.review === review && review.canReveal)
            // N20.8: the review's own Undo covers the saved edit ("Undo Edit Note") and restores the note.
            if case .available(let step) = review.undoStatus {
                #expect(ReviewStepNames.display(step.name) == "Edit Note")
            } else { Issue.record("The review's Undo offers the saved edit: \(review.undoStatus)") }
            review.restore(); await review.settled()
            // The other writer's update made during the session merged into the
            // same "Update Note" step (engine/undo A3), so Undo restores both.
            #expect(try await stored() == ["Question", "Answer"], "Undo restores the note")
            #expect(review.item?.cardId == card && review.record.answers.isEmpty)

            await owner.finishReview()
            #expect(await owner.close())
        } catch {
            // A failure with a changed draft open would make close() wait for a
            // Discard answer nobody gives: end the test's form first.
            owner.editNoteFlow.generationEnded()
            _ = await owner.close(); throw error
        }
    }
}
