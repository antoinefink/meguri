import Foundation
import Synchronization
import Testing
import UniformTypeIdentifiers
import MeguriCoreBindings
@testable import CoreBridge

private final class ImportClock: CollectionClock {
    let now = Mutex<Int64>(1_894_701_600_000)
    func nowMilliseconds() -> Int64 { now.withLock { $0 } }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class ImportDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension MaintenanceLane {
    /// The open collection as a latest-format package: under a deck
    /// package's name, a deck package of every note.
    func writeDeckPackage(to file: URL) throws {
        try call("test.writeDeckPackage") { try testWriteDeckPackage(path: file.path) }
    }
}

private func location(_ root: URL) throws -> CoreBridge.CollectionLocation {
    let directory = root.appending(path: "collection")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
    return CoreBridge.CollectionLocation(collectionDirectory: directory,
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
        manualTransferDirectory: root.appending(path: "ManualTransfers"))
}

private func stagingDirectories(_ location: CoreBridge.CollectionLocation) throws -> Int {
    try FileManager.default.contentsOfDirectory(atPath: location.collectionDirectory.path)
        .count { $0.hasPrefix(".meguri-stage-") }
}

extension EventTests {
    /// A small generated package through the real core: the file's class,
    /// the facts, the stored options, the import's counts and log pages,
    /// the one undo step "Import", a second import that updates the edited
    /// note and adds no duplicate, and the staging that goes with a session.
    @Test @MainActor func deckPackageIsInspectedAppliedAndClosedThroughTheCore() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let sourceLocation = try location(root.appending(path: "source"))
        let targetLocation = try location(root.appending(path: "target"))
        let clock = ImportClock()
        let service = CoreCollectionService(clock: clock, diskSpace: ImportDiskCapacity())
        let notes = CoreNoteService(), types = CoreNotetypeService(), undo = CoreUndoService()
        let imports = CoreDeckPackageImportService()
        let package = root.appending(path: "deck.apkg"), later = root.appending(path: "later.apkg")
        do {
            // The source: two notes, then the first one edited later.
            try await MaintenanceLane.shared.bootstrapCollection(sourceLocation)
            try await service.open(sourceLocation, randomSeed: 41)
            let basic = try #require(try await types.notetypeList().first { $0.name == "Basic" }).id
            var ids: [Int64] = []
            for (guid, front) in [("g-first", "sun"), ("g-second", "moon")] {
                ids.append(try await notes.addNote(.init(notetypeId: basic, fields: [front, "back"], tags: [], deckId: 1,
                                                         guid: guid, updateAddDefaults: false)).id)
            }
            try await MaintenanceLane.shared.writeDeckPackage(to: package)
            clock.now.withLock { $0 += 90_000 }
            _ = try await notes.updateNote(.init(id: ids[0], fields: ["sun-zq2", "back"], tags: []))
            try await MaintenanceLane.shared.writeDeckPackage(to: later)
            try await service.close()

            // The target: a new collection with the same stock note types.
            try await MaintenanceLane.shared.bootstrapCollection(targetLocation)
            try await service.open(targetLocation, randomSeed: 41)

            // P18: whole-collection names go to the other flow.
            #expect(try await imports.classify(name: "deck.apkg") == .deckPackage)
            #expect(try await imports.classify(name: "Shared.ZIP") == .deckPackage)
            for whole in ["all.colpkg", "collection.apkg", "backup-2026-11-23.apkg"] {
                #expect(try await imports.classify(name: whole) == .collectionPackage)
            }
            #expect(try await imports.classify(name: "notes.txt") == nil)
            #expect(try await imports.classify(file: package) == .deckPackage)
            let renamed = root.appending(path: "renamed.bin")
            try FileManager.default.copyItem(at: package, to: renamed)
            #expect(try await imports.classify(file: renamed) == .deckPackage)
            let text = root.appending(path: "notes.txt")
            try Data("plain words".utf8).write(to: text)
            #expect(try await imports.classify(file: text) == .unsupported)
            await #expect(throws: DeckImportRefusal.unreadableFile) { try await imports.classify(file: root) }
            // The declared type resolves the extension.
            #expect(UTType.meguriDeckPackage.identifier == "com.antoinefink.meguri.deck-package")

            // Refusals are typed, and leave nothing behind.
            await #expect(throws: DeckImportRefusal.unsupportedFile) { _ = try await imports.inspect(file: text).value() }
            let whole = root.appending(path: "collection.apkg")
            try FileManager.default.copyItem(at: package, to: whole)
            await #expect(throws: DeckImportRefusal.collectionPackage) { _ = try await imports.inspect(file: whole).value() }
            #expect(try stagingDirectories(targetLocation) == 0)

            let defaults = try await imports.storedOptions()
            #expect(defaults == DeckImportOptions())

            // Inspect, with its stages as a stream.
            let inspecting = imports.inspect(file: package)
            let stages = Task { var seen: [DeckImportStage] = []; for await update in inspecting.progress { seen.append(update.stage) }; return seen }
            let session = try await inspecting.value()
            #expect((await stages.value).allSatisfy { $0 == .extracting || $0 == .analysing })
            #expect(session.fileName == "deck.apkg")
            #expect(session.facts.notes == 2 && session.facts.cards == 2 && session.facts.format == .latest)
            #expect(session.facts.decks == ["Default"] && session.facts.notetypes == ["Basic"])
            #expect(!session.facts.hasScheduling && session.facts.mediaFiles == 0)
            #expect(try stagingDirectories(targetLocation) == 1)
            #expect(try await imports.result(of: session) == nil)

            // Apply: two notes added, one undo step "Import".
            var options = defaults
            options.withScheduling = true
            guard case .imported(let result) = try await imports.apply(session, options: options, confirmation: nil,
                                                                        askStructure: false).value() else {
                Issue.record("the import commits"); return
            }
            #expect(result.counts.notesAdded == 2 && result.counts.cardsAdded == 2 && result.counts.notesUpdated == 0)
            #expect(result.log.foundNotes == 2 && result.log[.new] == 2 && result.log[.duplicate] == 0)
            #expect(result.undoStep == "Import" && !result.mediaPending && result.structure.reasons.isEmpty)
            #expect(try await imports.result(of: session) == result)
            #expect(try await imports.storedOptions() == options)
            let all = try await imports.logPage(session, category: .new, offset: 0, count: 10)
            #expect(all.map { $0.fields[0] } == ["sun", "moon"])
            #expect(try await imports.logPage(session, category: .new, offset: 1, count: 5) == Array(all[1...]))
            #expect(try await imports.logPage(session, category: .new, offset: 2, count: 5).isEmpty)
            guard case .available(let step) = try await undo.peekUndo(scope: .global) else {
                Issue.record("the import is the undo step"); return
            }
            #expect(step.name == "Import")
            // A session commits once, and its staging goes when it closes.
            guard case .refused(.sessionClosed) = try await imports.apply(session, options: options, confirmation: nil,
                                                                           askStructure: false).value() else {
                Issue.record("a second apply of one session is refused"); return
            }
            await imports.close(session)
            await imports.close(session)
            #expect(try stagingDirectories(targetLocation) == 0)
            #expect(try await imports.logPage(session, category: .new, offset: 0, count: 10).isEmpty)

            // The edited package: the first note is updated, none is added twice.
            clock.now.withLock { $0 += 200_000 }
            let second = try await imports.inspect(file: later).value()
            guard case .imported(let updated) = try await imports.apply(second, options: options, confirmation: nil,
                                                                         askStructure: false).value() else {
                Issue.record("the second import commits"); return
            }
            #expect(updated.counts.notesAdded == 0 && updated.counts.notesUpdated == 1)
            #expect(updated.log[.new] == 0 && updated.log[.updated] == 1 && updated.log[.duplicate] == 1)
            let entry = try #require(try await imports.logPage(second, category: .updated, offset: 0, count: 5).first)
            #expect(entry.fields[0] == "sun-zq2" && entry.noteId == all[0].noteId)
            // Undo takes that import back; the first one is the next step.
            _ = try await undo.startUndo(scope: .global).value()
            guard case .available(let earlier) = try await undo.peekUndo(scope: .global) else {
                Issue.record("the first import is still undoable"); return
            }
            #expect(earlier.name == "Import")

            // A cancelled inspect answers `cancelled` and leaves no staging.
            let cancelled = imports.inspect(file: package)
            cancelled.cancel()
            await #expect(throws: DeckImportRefusal.cancelled) { _ = try await cancelled.value() }
            // Closing the collection ends the session that is still open.
            #expect(try stagingDirectories(targetLocation) == 1)
            try await service.close()
            #expect(try stagingDirectories(targetLocation) == 0)
            #expect(try await imports.result(of: second) == nil)
        } catch {
            try? await service.close()
            throw error
        }
    }

    /// The fake follows the same contract: the file's class, a confirmation
    /// first, the scripted result, one commit per session, and a cancel.
    @Test func fakeDeckPackageImportFollowsTheSessionContract() async throws {
        let confirmation = DeckImportConfirmation(
            preview: .init(removedNotetypes: [.init(notetypeId: 7, name: "Pair+", movedTo: 5, movedToName: "Pair", notes: 3)]),
            reasons: [.notetypesMerged], fakeToken: 9)
        let result = DeckImportResult(sequence: 4, generation: 1, undoStep: "Import",
                                      log: .init(foundNotes: 3, entries: [.new: 2, .conflicting: 1]))
        var script = FakeDeckPackageImportService.Script(
            inspect: .success(.init(notes: 3, cards: 3, decks: ["Default"], notetypes: ["Pair"])),
            applies: [.confirmationRequired(confirmation), .imported(result)])
        script.log = [.new: [.init(noteId: 1, fields: ["a"]), .init(noteId: 2, fields: ["b"])]]
        let fake = FakeDeckPackageImportService(script)
        #expect(try await fake.classify(file: URL(filePath: "/x/collection.apkg")) == .collectionPackage)
        await #expect(throws: DeckImportRefusal.collectionPackage) {
            _ = try await fake.inspect(file: URL(filePath: "/x/backup-1.apkg")).value()
        }
        let session = try await fake.inspect(file: URL(filePath: "/x/deck.apkg")).value()
        #expect(session.facts.notes == 3 && fake.openSessions == 1)
        guard case .confirmationRequired(let asked) = try await fake.apply(session, options: .init(), confirmation: nil,
                                                                            askStructure: true).value() else {
            Issue.record("the fake asks first"); return
        }
        #expect(asked.reasons == [.notetypesMerged] && !asked.preview.isEmpty)
        #expect(try await fake.logPage(session, category: .new, offset: 0, count: 5).isEmpty)
        guard case .imported(let imported) = try await fake.apply(session, options: .init(), confirmation: asked,
                                                                   askStructure: false).value() else {
            Issue.record("the fake imports"); return
        }
        #expect(imported == result)
        #expect(fake.applied.map(\.confirmation) == [nil, 9] && fake.applied.map(\.askStructure) == [true, false])
        #expect(try await fake.logPage(session, category: .new, offset: 1, count: 5) == [.init(noteId: 2, fields: ["b"])])
        guard case .refused(.sessionClosed) = try await fake.apply(session, options: .init(), confirmation: nil,
                                                                    askStructure: false).value() else {
            Issue.record("one commit per session"); return
        }
        await fake.close(session)
        #expect(fake.openSessions == 0)
        #expect(try await fake.result(of: session) == nil)
    }
}
