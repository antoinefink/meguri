import Foundation
import Testing
@testable import CoreBridge

private final class SearchClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class SearchDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

/// Every event of request `seq`, up to its terminal one.
private func request(_ seq: UInt64, _ events: inout AsyncStream<BrowseSearchEvent>.Iterator) async throws
    -> [BrowseSearchEvent] {
    var got: [BrowseSearchEvent] = []
    while true {
        let event = try #require(await events.next())
        #expect(event.seq == seq, "\(event)")
        got.append(event)
        if event.isTerminal { return got }
    }
}

/// A request that completes: its handle, first page and count.
private func completed(_ seq: UInt64, _ events: inout AsyncStream<BrowseSearchEvent>.Iterator) async throws
    -> (BrowseHandle, [Int64], UInt64) {
    let got = try await request(seq, &events)
    guard got.count == 2, case .firstPage(_, let handle, let ids) = got[0],
          case .complete(_, let done, let count, 0) = got[1], done == handle else {
        Issue.record("not a completed request: \(got)")
        throw BridgeError.internal("not completed")
    }
    return (handle, ids, count)
}

private func sortField(_ row: BrowseRowResult) -> String? {
    guard case .row(let row) = row, case .text(let text) = row.cells.first else { return nil }
    return text
}

extension EventTests {
    /// U264's session through CoreBridge on an opened collection
    /// (`browser/result-sets` H2 to H12): events in order on the session's
    /// stream, pages and Question cells, reverse and its stale revision,
    /// resort, the toggle, selection resolution and mapping, invalidation by
    /// a tag edit's change set, a search error's UTF-16 range, and close.
    @Test(.timeLimit(.minutes(2))) @MainActor func searchSessionsPageSortToggleAndMapThroughCoreBridge() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: SearchClock(), diskSpace: SearchDiskCapacity())
        let notes = CoreNoteService(), types = CoreNotetypeService(), tags = CoreTagService()
        do {
            try await service.open(location, randomSeed: 43)
            let added = try await types.addNotetype(.init(name: "Duo", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{FrontSide}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{FrontSide}}", browserFront: "", browserBack: "")],
                css: ""))
            var noteIDs: [Int64] = []
            var cards: [Int64] = []
            for fields in [["alpha", "one"], ["beta", "two"], ["gamma", "three"]] {
                let note = try await notes.addNote(.init(notetypeId: added.id, fields: fields, tags: [], deckId: 1, guid: nil,
                                                         updateAddDefaults: false))
                noteIDs.append(note.id)
                cards += note.cards.sorted()
            }

            let session = try await BrowseSearchSession.open()
            var events = session.events.makeAsyncIterator()
            let byField = BrowseSort(key: "noteFld", direction: .ascending)

            let seq = try await session.startSearch("", mode: .cards, sort: byField)
            let (first, page, count) = try await completed(seq, &events)
            #expect(page == cards && count == 6)
            #expect(try await session.state(of: first)
                == BrowseHandleState(mode: .cards, revision: 0, count: 6, isComplete: true, sort: byField))

            let rows = try await session.rows(first, revision: 0, range: 0..<6, columns: ["noteFld", "question"])
            #expect(rows.map(sortField) == ["alpha", "alpha", "beta", "beta", "gamma", "gamma"])
            guard case .row(let third) = rows[2] else { throw BridgeError.internal("not a row: \(rows[2])") }
            #expect(third.id == cards[2] && third.noteID == noteIDs[1] && third.cells[1] == .deferred)
            #expect(try await session.qaCells(first, revision: 0, range: 0..<2, which: .question)
                == [.cells(question: .text("alpha"), answer: nil), .cells(question: .text("one"), answer: nil)])

            #expect(try await session.reverse(first, revision: 0) == 1)
            #expect(try await session.rows(first, revision: 1, range: 0..<1, columns: ["noteFld"]).map(sortField) == ["gamma"])
            await #expect(throws: BridgeError.browse(.staleRevision(current: 1))) {
                _ = try await session.rows(first, revision: 0, range: 0..<1, columns: ["noteFld"])
            }

            let sortSeq = try await session.resort(first, sort: byField)
            let (cardsHandle, sortPage, _) = try await completed(sortSeq, &events)
            #expect(sortPage == cards)
            let toggled = try await session.toggle(cardsHandle, sort: byField)
            let (notesHandle, notesPage, notesCount) = try await completed(toggled, &events)
            #expect(notesPage == noteIDs && notesCount == 3)

            #expect(try await session.resolveSelection(cardsHandle, revision: 0, ranges: [0..<3], target: .notes)
                == Array(noteIDs.prefix(2)))
            let selection = try await session.selection(notesHandle, revision: 0, ranges: [0..<3])
            #expect(try await selection.resolve(.notes) == noteIDs)
            #expect(try await session.index(of: cards[3], kind: .cards, in: notesHandle, revision: 0) == 1)
            #expect(try await session.mapSelection(from: .handle(cardsHandle, revision: 0), selected: .ranges([0..<3]),
                                                   current: cards[2], to: notesHandle, targetRevision: 0)
                == BrowseMappedSelection(selected: [0..<2], current: 1))
            #expect(try await session.mapSelection(from: .mode(.notes), selected: .ids([noteIDs[1]]), current: noteIDs[1],
                                                   to: cardsHandle, targetRevision: 0)
                == BrowseMappedSelection(selected: [2..<4], current: 2))

            // H10: the change set of a tag edit crosses back as received.
            let tagged = try await tags.addNoteTags(notes: [noteIDs[0]], text: "picked").summary
            #expect(try await session.invalidatedRows(cardsHandle, revision: 0,
                                                      changes: [SequencedChangeSet(seq: tagged.seq, changes: tagged.changes)])
                == BrowseInvalidation(removed: [], refetch: .rows([0, 1]), revision: 0, isComplete: true, count: 6))

            // H4, H15: the range of a search error is in UTF-16 units.
            let failed = try await session.startSearch("\u{1D11E} is:bogus", mode: .cards, sort: byField)
            let failure = try await request(failed, &events)
            guard case .failed(_, .browse(.search(let error))) = failure.first else {
                throw BridgeError.internal("not a search error: \(failure)")
            }
            #expect(error.kind == "invalid_state" && error.provided == "bogus" && error.utf16Range == 3..<11)
            let unsortable = try await session.startSearch("", mode: .cards, sort: BrowseSort(key: "question", direction: .ascending))
            #expect(try await request(unsortable, &events) == [.failed(seq: unsortable, error: .browse(.unsortableColumn))])

            try await session.close()
            #expect(await events.next() == nil)
            await #expect(throws: BridgeError.browse(.unknownHandle)) { _ = try await session.state(of: cardsHandle) }
            await #expect(throws: BridgeError.browse(.unknownHandle)) { _ = try await selection.resolve(.notes) }
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
