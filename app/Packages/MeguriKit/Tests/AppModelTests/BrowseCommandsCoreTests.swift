@testable import AppModel
import CoreBridge
import Foundation
import Testing

// U275 on the real core, through ReviewCollection: each command that needs no
// sheet runs on the rows chosen, the table's rows change in place, and
// Edit › Undo and Redo name the step after the command (`browser/workspace`
// V8.1 to V8.3). It owns the process's collection, so it joins the
// serialized real-host suite.
extension ReviewCollectionStartupTests {
    @Test func browseCommandsRunOnTheRealCoreWithUndoAndRedo() async throws {
        try await withBrowseOwner { owner, ids in
            let browse = owner.browse, results = browse.results, commands = browse.commands, undo = owner.collectionUndo
            @MainActor func row(_ index: Int) -> BrowseShownRow? { results.row(at: index) }
            @MainActor func pick(_ rows: Set<Int>, sourceLocation: SourceLocation = #_sourceLocation) async {
                results.select(rows)
                // The core's counts of these rows, not those of the rows chosen before.
                await waitFor({ commands.factsRead != nil && commands.factsRead == results.target() && !commands.busy },
                                 sourceLocation: sourceLocation)
            }
            @MainActor func undoTitle(_ title: String, sourceLocation: SourceLocation = #_sourceLocation) async {
                await waitFor({ undo.undoName == title && undo.canUndo }, sourceLocation: sourceLocation)
            }

            // Suspend Card, then Undo and Redo, each titled after the command.
            await pick([0])
            #expect(commands.state(.suspend).enabled && !commands.state(.unsuspend).shown)
            commands.perform(.suspend)
            await waitFor { row(0)?.state?.suspended == true }
            await undoTitle("Suspend Card")
            #expect(results.list?.count == 4 && results.selection == [0], "the row stays, selected (R1155)")
            undo.undo(); await undo.waitForCompletion()
            await waitFor { row(0)?.state?.suspended == false }
            await waitFor { undo.redoName == "Suspend Card" }
            undo.redo(); await undo.waitForCompletion()
            await waitFor { row(0)?.state?.suspended == true }
            await undoTitle("Suspend Card")

            // Unsuspend: only the suspended targets; its own title.
            await pick([0, 1])
            #expect(commands.state(.unsuspend).count == 1 && commands.state(.suspend).count == 2)
            commands.perform(.unsuspend)
            await waitFor { row(0)?.state?.suspended == false }
            await undoTitle("Unsuspend Card")
            undo.undo(); await undo.waitForCompletion()
            await waitFor { row(0)?.state?.suspended == true }
            await waitFor { undo.redoName == "Unsuspend Card" }
            undo.redo(); await undo.waitForCompletion()
            await waitFor { row(0)?.state?.suspended == false }

            // Bury Cards on two rows, Unbury Cards, Flag and No Flag.
            await pick([0, 1])
            commands.perform(.bury)
            await waitFor { row(0)?.state?.buried == true && row(1)?.state?.buried == true }
            await undoTitle("Bury Cards")
            await pick([0, 1])
            await waitFor { commands.state(.unbury).count == 2 }
            commands.perform(.unbury)
            await waitFor { row(0)?.state?.buried == false && row(1)?.state?.buried == false }
            await undoTitle("Unbury Cards")
            commands.perform(.flag(3))
            await waitFor { row(0)?.state?.flag == 3 && row(1)?.state?.flag == 3 }
            await undoTitle("Flag")
            undo.undo(); await undo.waitForCompletion()
            await waitFor { row(0)?.state?.flag == 0 }
            await waitFor { undo.redoName == "Flag" }

            // Select Sibling Cards: the other card of the two-card note.
            await pick([2])
            commands.selectSiblings()
            await waitFor { results.selection == [2, 3] }

            // Delete of one note with one card: no question; Undo brings its row back.
            await pick([0])
            commands.perform(.delete)
            await waitFor { results.list?.count == 3 }
            #expect(commands.deleteQuestion == nil)
            await undoTitle("Delete Note")
            undo.undo(); await undo.waitForCompletion()
            await waitFor { results.list?.count == 4 && row(0)?.id == ids.first }
            await waitFor { results.selection == [0] }

            // Delete of the two-card note asks first, with its cards; Cancel changes nothing.
            await pick([2])
            await waitFor { commands.state(.delete).asks }
            commands.perform(.delete)
            await waitFor { commands.deleteQuestion != nil }
            let question = try #require(commands.deleteQuestion)
            #expect(question.notes == 1 && question.cards == 2 && question.sortField == "plorvex c")
            commands.cancelDelete()
            #expect(results.list?.count == 4)
            commands.perform(.delete)
            await waitFor { commands.deleteQuestion != nil }
            commands.confirmDelete(try #require(commands.deleteQuestion))
            await waitFor { results.list?.count == 2 }
            await undoTitle("Delete Note")
        }
    }

    /// Waits, suspended, until `condition` holds (at most ten seconds: each
    /// command is a core job, then a change set and a read of its rows).
    private func waitFor(_ condition: () -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
        for _ in 0..<1_000 where !condition() {
            try? await Task.sleep(for: .milliseconds(10))
        }
        #expect(condition(), sourceLocation: sourceLocation)
    }

    /// A collection with three notes that only this test's search finds:
    /// "plorvex a" and "plorvex b" with one card each, "plorvex c" with two.
    private func withBrowseOwner(_ body: @MainActor (ReviewCollection, [Int64]) async throws -> Void) async throws {
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
            let single = try await types.addNotetype(.init(name: "QZ One \(UUID().uuidString)", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: "")], css: nil))
            let double = try await types.addNotetype(.init(name: "QZ Two \(UUID().uuidString)", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{Front}}", browserFront: "", browserBack: "")],
                css: nil))
            var cards: [Int64] = []
            for (type, front) in [(single.id, "plorvex a"), (single.id, "plorvex b"), (double.id, "plorvex c")] {
                let note = try await notes.addNote(.init(notetypeId: type, fields: [front, "x"], tags: [], deckId: 1, guid: nil,
                                                         updateAddDefaults: false))
                cards += note.cards
            }
            try #require(cards.count == 4)
            owner.navigation.select(.section(.browse))
            let browse = owner.browse
            browse.appear()
            await waitFor { browse.opened && browse.results.list != nil }
            browse.setQuery(FilterQuery(text: "plorvex"))
            await waitFor { browse.results.list?.count == 4 && browse.results.list?.isComplete == true && browse.results.row(at: 3) != nil }
            try #require(browse.results.list?.mode == .cards)
            try await body(owner, cards)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
