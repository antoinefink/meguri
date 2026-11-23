@testable import AppModel
import CoreBridge
import Foundation
import Testing

// U273 on the real core, through ReviewCollection: the inspector reads the
// selected row's note and its cards, a save writes only the field changed,
// to that note, as one step Edit › Undo names "Edit Note" (V8.3), the row's
// cell changes in place and stays selected, and Undo and Redo bring the
// fields back into the page. A save with nothing changed writes nothing
// (W4). The field page is the test's own (no WebKit); the rest is the app's.
extension ReviewCollectionStartupTests {
    @Test func browseInspectorSavesAnEditAsOneEditNoteStepOnTheRealCore() async throws {
        try await withInspectorOwner { owner in
            let browse = owner.browse, results = browse.results, inspector = browse.inspector, undo = owner.collectionUndo
            let page = FakePage()
            inspector.makeRuntime = { draft, _ in FakeFieldRuntime(draft: draft, page: page) }
            @MainActor func sortField(_ index: Int) -> String? {
                guard let row = results.row(at: index), let column = results.list?.columns.firstIndex(of: "noteFld"),
                      row.cells.indices.contains(column), case .text(let text) = row.cells[column] else { return nil }
                return text
            }

            // The second card of the two-card note, in Cards mode: its note, both cards, this one shown.
            results.select([2])
            await waitUntil { inspector.note?.runtime?.editable == true }
            let note = try #require(inspector.note)
            #expect(note.draft.values == ["kerrow c", "x"] && note.cards.count == 2 && note.cards.map(\.ord) == [0, 1])
            let shown = try #require(results.row(at: 2)?.id)
            #expect(note.card?.id == shown)

            // Nothing typed: leaving the row writes nothing (W4).
            let before = try await CoreNoteService().readNote(id: note.noteID), stepBefore = undo.undoName
            results.select([0])
            await waitUntil { inspector.note?.noteID != note.noteID && inspector.note?.runtime?.editable == true }
            #expect(try await CoreNoteService().readNote(id: note.noteID) == before)
            #expect(undo.undoName == stepBefore, "no step was recorded")

            // A field edited, then the focus leaves: one "Edit Note" step; the row's cell follows, still selected.
            let first = try #require(inspector.note)
            let runtime = try #require(first.runtime as? FakeFieldRuntime)
            try await runtime.type(0, "kerrow a, edited")
            runtime.blur()
            await waitUntil { undo.undoName == "Edit Note" && undo.canUndo }
            await waitUntil { sortField(0) == "kerrow a, edited" }
            #expect(results.selection == [0] && inspector.note === first)
            #expect(try await CoreNoteService().readNote(id: first.noteID).fields == ["kerrow a, edited", "x"])

            // Undo puts the stored field back, and the page takes it (W5.2).
            undo.undo(); await undo.waitForCompletion()
            await waitUntil { first.draft.values[0] == "kerrow a" && sortField(0) == "kerrow a" }
            await waitUntil { undo.redoName == "Edit Note" }
            #expect(page.reloads.contains([0]))
            undo.redo(); await undo.waitForCompletion()
            await waitUntil { first.draft.values[0] == "kerrow a, edited" && undo.undoName == "Edit Note" }
        }
    }

    private func waitUntil(_ condition: () -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
        for _ in 0..<1_000 where !condition() {
            try? await Task.sleep(for: .milliseconds(10))
        }
        #expect(condition(), sourceLocation: sourceLocation)
    }

    /// Three notes that only this test's search finds: "kerrow a" and
    /// "kerrow b" with one card each, "kerrow c" with two.
    private func withInspectorOwner(_ body: @MainActor (ReviewCollection) async throws -> Void) async throws {
        let files = FileManager.default, root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        do {
            await owner.start()
            owner.entry.inspect(try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg")))
            await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            owner.navigation.dismissSheet()
            let types = CoreNotetypeService(), notes = CoreNoteService()
            let single = try await types.addNotetype(.init(name: "QI One \(UUID().uuidString)", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: "")], css: nil))
            let double = try await types.addNotetype(.init(name: "QI Two \(UUID().uuidString)", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{Front}}", browserFront: "", browserBack: "")],
                css: nil))
            for (type, front) in [(single.id, "kerrow a"), (single.id, "kerrow b"), (double.id, "kerrow c")] {
                _ = try await notes.addNote(.init(notetypeId: type, fields: [front, "x"], tags: [], deckId: 1, guid: nil,
                                                  updateAddDefaults: false))
            }
            owner.navigation.select(.section(.browse))
            let browse = owner.browse
            browse.appear()
            await waitUntil { browse.opened && browse.results.list != nil }
            browse.setQuery(FilterQuery(text: "kerrow"))
            await waitUntil { browse.results.list?.count == 4 && browse.results.list?.isComplete == true && browse.results.row(at: 3) != nil }
            try #require(browse.results.list?.mode == .cards)
            try await body(owner)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
