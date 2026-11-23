import Foundation
import Testing
@testable import CoreBridge

private final class QueryClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class QueryDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension EventTests {
    /// U264a through CoreBridge on an opened collection: T7's reading and
    /// T4's run text, the commit, the incomplete text, the suggestions with
    /// a UTF-16 caret, the filter counts, selection counts, Find Duplicates
    /// and Find and Replace as jobs, and a column set change.
    @Test(.timeLimit(.minutes(2))) @MainActor func browseQueriesSuggestCountAndChangeColumnsThroughCoreBridge() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: QueryClock(), diskSpace: QueryDiskCapacity())
        let notes = CoreNoteService(), types = CoreNotetypeService()
        do {
            try await service.open(location, randomSeed: 47)
            let added = try await types.addNotetype(.init(name: "Duo", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{FrontSide}}", browserFront: "", browserBack: "")],
                css: ""))
            var firstIDs: [Int64] = []
            for (front, tag) in [("alpha", "\u{1D11E}zorb"), ("beta", "plain")] {
                let note = try await notes.addNote(.init(notetypeId: added.id, fields: [front, "x"], tags: [tag], deckId: 1,
                                                         guid: nil, updateAddDefaults: false))
                firstIDs.append(note.id)
            }

            let read = try await CoreBrowseQuery.read("-is:suspended tag:plain word")
            #expect(read == FilterQuery(tokens: [.value([.state(.suspended)], negated: true), .value([.tag("plain")], negated: false)],
                                        text: "word"))
            let run = try await CoreBrowseQuery.runText(read)
            #expect(try await CoreBrowseQuery.read(run) == read)
            #expect(try await CoreBrowseQuery.commit(FilterQuery(text: "tag:plain more"))
                == FilterQuery(tokens: [.value([.tag("plain")], negated: false)], text: "more"))
            #expect(try await CoreBrowseQuery.isIncomplete("deck:", caret: 5))
            #expect(try await CoreBrowseQuery.deckValue(1)?.deckName == "Default")

            // The caret counts UTF-16 units: the tag starts with a character
            // outside the BMP.
            let typed = "\u{1D11E}zo"
            let rows = try await CoreBrowseQuery.suggestions(for: FilterQuery(text: typed), caret: typed.utf16.count,
                                                             labels: .english)
            #expect(rows.first?.row == .contains(typed))
            #expect(rows.contains { $0.row == .value(.tag("\u{1D11E}zorb")) && $0.section == .tags && $0.key != nil })

            // A D6 syntax row after a character outside the BMP: its range
            // and caret are UTF-16 offsets of the field's text.
            let syntax = "\u{1D11E} tag:pl"
            let candidates = try await CoreBrowseQuery.suggestions(for: FilterQuery(text: syntax), caret: syntax.utf16.count,
                                                                   labels: .english)
            let insert = try #require(candidates.lazy.compactMap { row -> (Range<Int>, String, Int)? in
                guard case .syntax(let candidate) = row.row, candidate.label == "plain", let insert = candidate.insert else {
                    return nil
                }
                return (insert.range, insert.text, insert.caretAfter)
            }.first)
            #expect(insert.0.lowerBound >= 3 && insert.0.upperBound == syntax.utf16.count)
            var applied = syntax.utf16.map { $0 }
            applied.replaceSubrange(insert.0, with: insert.1.utf16)
            let written = String(decoding: applied, as: UTF16.self)
            #expect(written.contains("plain") && insert.2 <= written.utf16.count)

            let counts = try await CoreBrowseQuery.startFilterCounts(.notes, items: [.value(.tag("plain")), .saved("tag:*")])
            #expect(try await counts.value() == [1, 2])

            let shown = try await CoreBrowseQuery.columnSet(notetype: nil)
            #expect(shown.set == .standard && shown.columns(.cards).first == "noteFld")
            let change = try await CoreBrowseQuery.changeColumnSet(.standard, mode: .notes, change: .hide("noteCrt"))
            #expect(change.written && change.summary.undoName == nil)
            #expect(try await CoreBrowseQuery.columnSet(notetype: nil).notes.contains("noteCrt") == false)

            // T14: a new collection opens Browse on the whole collection, in Cards.
            #expect(try await CoreBrowseQuery.opening() == BrowseOpening(defaultSearch: nil, notesMode: false))

            let session = try await BrowseSearchSession.open()
            var events = session.events.makeAsyncIterator()
            let seq = try await session.startSearch("", mode: .notes, sort: BrowseSort(key: "noteFld", direction: .ascending))
            var handle: BrowseHandle?
            while handle == nil, let event = await events.next() {
                if case .complete(seq, let done, _, _) = event { handle = done }
            }
            let notesHandle = try #require(handle)
            let selection = try await session.selection(notesHandle, revision: 0, ranges: [0..<2])
            let selected = try await CoreBrowseQuery.startSelectionCounts(selection).value()
            #expect(selected == BrowseSelectionCounts(cards: 2, notes: 2, suspended: 0, buried: 0, decks: 1, deck: 1,
                                                      notetypes: [added.id]))

            // V10: a note whose Front differs from alpha's only in case,
            // markup and spaces. All Notes finds the pair; the selection's
            // two rows hold one note of each Front.
            let third = try await notes.addNote(.init(notetypeId: added.id, fields: ["<b>ALPHA</b> ", "y"], tags: [],
                                                      deckId: 1, guid: nil, updateAddDefaults: false))
            let everyNote = try await CoreBrowseQuery.startFindDuplicates(field: "Front", in: nil).value()
            let pair = [firstIDs[0], third.id].sorted()
            #expect(everyNote == FindDuplicatesResult(groups: [pair], noteIds: pair, considered: 3))
            let inFound = try await CoreBrowseQuery.startFindDuplicates(field: "Front", in: selection).value()
            #expect(inFound == FindDuplicatesResult(groups: [], noteIds: [], considered: 2))

            // F10, F11: the preview on the selection's notes (alpha, beta),
            // the run it confirms, and a typed refusal.
            let previews = try await CoreFindReplacePreviews.open()
            let request = FindReplaceRequest(find: "alpha", replace: "quint", scope: .field("Front"))
            let previewJob = try await previews.start(request, on: selection)
            #expect(previewJob.request == request)
            let previewed = try await previewJob.value()
            #expect(previewed == FindReplacePreview(changed: 1, considered: 2, entries: [
                FindReplacePreviewEntry(noteID: firstIDs[0], sortField: "alpha", field: "Front", spans: [
                    FindReplaceSpan(edit: .removed, text: "alpha"), FindReplaceSpan(edit: .inserted, text: "quint"),
                ]),
            ]))
            let done = try await CoreFindReplace.start(request, on: selection, confirmed: previewed.changed).value()
            #expect(done.affected == 1 && done.considered == 2 && done.summary.undoName == "Find and Replace")
            #expect(try await notes.readNote(id: firstIDs[0]).fields[0] == "quint")
            let spaced = FindReplaceRequest(find: "plain", replace: "two words", scope: .tags)
            await #expect(throws: BridgeError.findReplaceRefused(.separatorInReplacement)) {
                try await previews.start(spaced, on: nil).value()
            }

            // U275: card and note actions on the selection, resolved in the core (K1).
            let suspended = try await CoreBulkActions.start(.suspend, on: selection).value()
            #expect(suspended.affected == 2 && suspended.considered == 2 && suspended.summary.undoName == "Suspend")
            let unsuspended = try await CoreBulkActions.start(.unsuspend, on: selection).value()
            #expect(unsuspended.affected == 2 && unsuspended.summary.undoName == "Unbury/Unsuspend")
            #expect(try await CoreBulkActions.start(.setFlag(4), on: selection).value().affected == 2)
            // T13: Rename… (quotes dropped) and Delete of a saved search, each one undoable step.
            #expect(try await CoreBrowseQuery.saveSearch(name: "Mine", search: "tag:plain").written)
            let renamed = try await CoreBrowseQuery.renameSavedSearch("Mine", to: "Ou\"rs")
            #expect(renamed.written && renamed.summary.undoName != nil)
            #expect(try await CoreBrowseQuery.renameSavedSearch("Mine", to: "Other").written == false)
            #expect(try await CoreBrowseQuery.deleteSavedSearches(["Ours", "Nope"]).written)
            #expect(try await CoreBrowseQuery.deleteSavedSearches(["Ours"]).written == false)
            let deleted = try await CoreBulkActions.start(.deleteNotes, on: selection).value()
            #expect(deleted.affected == 2 && deleted.cardsRemoved == 2)
            try await session.close()
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
