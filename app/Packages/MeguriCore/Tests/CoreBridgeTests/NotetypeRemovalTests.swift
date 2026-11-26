import Foundation
import Dispatch
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private final class RemovalClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}
private final class RemovalDisk: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

/// Deliver the real terminal before failing setup. This forces the commit to
/// beat fallback cancellation, without sleeps or changing Rust's execution.
private final class RemovalTrackingFailure: Sendable {
    let terminal = Mutex<CoreBridge.JobOutcome?>(nil)

    func setup(_ handle: MeguriCoreBindings.JobHandle) throws -> CoreJob {
        let delivered = DispatchSemaphore(value: 0)
        Task.detached {
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, let outcome) = event.kind, id.rawValue == handle.id {
                    self.terminal.withLock { $0 = outcome }
                    delivered.signal()
                    break
                }
            }
        }
        guard delivered.wait(timeout: .now() + 20) == .success else {
            Issue.record("Removal terminal did not arrive before listener setup failed")
            throw BridgeError.internal("Removal terminal timeout")
        }
        throw BridgeError.invalidArgument("removal listener setup")
    }
}

extension EventTests {
    @Test @MainActor func removalTrackingFailureCancelsAndDrainsUncommittedWork() async throws {
        try await CoreEventStream.shared.start()
        let handle = try await MaintenanceLane.shared.call("test.startJob") { @Sendable in
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        let failure = BridgeError.invalidArgument("removal listener setup")
        let job = await Jobs.trackMutationDraining(handle, setup: { _ in throw failure })
        #expect(try await job.cancelToken.isCancelled())
        #expect(await job.outcome() == .failed(failure))
    }

    /// Both deletion facades, scoped and legacy, must retain the real result/Undo
    /// when listener setup fails after commit, and the note type's typed refusal otherwise.
    @Test(arguments: [false, true], [false, true]) @MainActor
    func removalTrackingFailurePreservesCommittedResultAndUndo(deleteTags: Bool, scoped: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget.snapshot"), manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let collection = CoreCollectionService(clock: RemovalClock(), diskSpace: RemovalDisk())
        let types = CoreNotetypeService(), notes = CoreNoteService()
        do {
            try await collection.open(location, randomSeed: 41)
            let generation = try await collection.waitAvailable(timeoutMilliseconds: 1_000)
            let expectedGeneration: UInt64? = scoped ? generation : nil
            let added = try await types.addStockNotetype(kind: .basic, name: "Tracking Removal")
            let note = try await notes.addNote(.init(notetypeId: added.id, fields: ["front", "back"],
                tags: ["remove::child", "keep"], deckId: 1, guid: nil, updateAddDefaults: false))
            let before = try await notes.readNote(id: note.id)
            if scoped {
                await #expect(throws: BridgeError.staleGeneration(planned: 0, current: generation)) {
                    if deleteTags {
                        _ = try await CoreTagService().startDeleteTags(text: "remove", expectedGeneration: 0,
                            trackingSetup: { _ in
                                Issue.record("Stale tag deletion reached tracking setup")
                                throw BridgeError.internal("Unexpected tracking setup")
                            })
                    } else {
                        _ = try await types.startNotetypeRemoval(
                            .init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 1, expectedGeneration: 0),
                            trackingSetup: { _ in
                                Issue.record("Stale note type deletion reached tracking setup")
                                throw BridgeError.internal("Unexpected tracking setup")
                            })
                    }
                }
                #expect(try await notes.readNote(id: note.id) == before)
            }
            let failure = RemovalTrackingFailure()
            let summary: CoreBridge.MutationSummary
            if deleteTags {
                let deletion = try await CoreTagService().startDeleteTags(text: "remove", expectedGeneration: expectedGeneration,
                    trackingSetup: { try failure.setup($0) })
                let result = try await deletion.value()
                #expect(try await deletion.job.cancelToken.isCancelled())
                #expect(result.count == 1)
                #expect(try await notes.readNote(id: note.id).tags == ["keep"])
                summary = result.summary
            } else {
                let refusal = RemovalTrackingFailure()
                let refused = try await types.startNotetypeRemoval(
                    .init(notetypeId: added.id, confirmedNotes: 0, confirmedCards: 0, expectedGeneration: expectedGeneration),
                    trackingSetup: { try refusal.setup($0) })
                await #expect(throws: BridgeError.notetypeStructureRefused(.removalLossesChanged(notes: 1, cards: 1))) {
                    try await refused.value()
                }
                #expect(try await notes.readNote(id: note.id) == before)
                let deletion = try await types.startNotetypeRemoval(
                    .init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 1, expectedGeneration: expectedGeneration),
                    trackingSetup: { try failure.setup($0) })
                let result = try await deletion.value()
                #expect(try await deletion.job.cancelToken.isCancelled())
                #expect(result.removed && result.id == added.id && result.notesRemoved == 1 && result.cardsRemoved == 1)
                #expect(!((try await types.notetypeList()).contains { $0.id == added.id }))
                summary = result.summary
            }
            #expect(failure.terminal.withLock { $0 } == .succeeded)
            #expect(summary.undoName != nil)
            // Use the exact returned sequence, as Browse's Undo action does.
            _ = try await CoreUndoService().startUndo(step: summary.seq).value()
            #expect(try await notes.readNote(id: note.id) == before)
            #expect(try await types.startNotetypeRemovalCounts(notetypeId: added.id).value() == .init(notes: 1, cards: 1))
            try await collection.close()
        } catch {
            try? await collection.close()
            throw error
        }
    }

    /// The published removal bridge must carry both preconditions, forward
    /// typed refusals, return the real result and expose the core's Undo.
    @Test @MainActor func notetypeRemovalRecountsDeletesAndUndoesThroughTheBridge() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget.snapshot"), manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let collection = CoreCollectionService(clock: RemovalClock(), diskSpace: RemovalDisk())
        let types = CoreNotetypeService(), notes = CoreNoteService()
        do {
            try await collection.open(location, randomSeed: 41)
            let added = try await types.addNotetype(.init(name: "Removal Sample", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{Front}}", browserFront: "", browserBack: "")], css: ""))
            let note = try await notes.addNote(.init(notetypeId: added.id, fields: ["front", "back"], tags: [], deckId: 1,
                guid: nil, updateAddDefaults: false))
            let before = try await types.readNotetype(id: added.id)
            let counts = try await types.startNotetypeRemovalCounts(notetypeId: added.id).value()
            #expect(counts == .init(notes: 1, cards: 2))
            let generation = try await collection.waitAvailable(timeoutMilliseconds: 1_000)
            await #expect(throws: BridgeError.staleGeneration(planned: 0, current: generation)) {
                try await types.startNotetypeRemoval(.init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 2,
                    expectedGeneration: 0))
            }
            #expect(try await types.readNotetype(id: added.id) == before)
            // A lower count in either category must refuse without writing.
            for request in [CoreBridge.NotetypeRemoval(notetypeId: added.id, confirmedNotes: 0, confirmedCards: 2),
                            CoreBridge.NotetypeRemoval(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 1)] {
                let job = try await types.startNotetypeRemoval(request)
                await #expect(throws: BridgeError.notetypeStructureRefused(.removalLossesChanged(notes: 1, cards: 2))) {
                    try await job.value()
                }
                #expect(try await types.readNotetype(id: added.id) == before)
            }
            let removed = try await types.startNotetypeRemoval(.init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 2,
                expectedGeneration: generation)).value()
            #expect(removed.removed && removed.id == added.id && removed.notesRemoved == 1 && removed.cardsRemoved == 2)
            #expect(!((try await types.notetypeList()).contains { $0.id == added.id }))
            #expect(removed.summary.undoName == "Remove Note Type")
            _ = try await CoreUndoService().startUndo(scope: .global).value()
            #expect(try await types.readNotetype(id: added.id) == before)
            #expect(try await notes.readNote(id: note.id).fields == ["front", "back"])
            #expect(try await types.startNotetypeRemovalCounts(notetypeId: added.id).value() == counts)
            // S12: removing a missing type is an acknowledged no-op.
            let missing = try await types.startNotetypeRemoval(.init(notetypeId: 9_999, confirmedNotes: 0, confirmedCards: 0)).value()
            #expect(!missing.removed)
            try await collection.close()
        } catch {
            try? await collection.close()
            throw error
        }
    }
}
