import CoreBridge
import Foundation
import Testing

@testable import Stats

/// S10.2/S10.5/S11/S17: exercise the production cache API and real synthetic files.
/// Save admission is observed before any disk work; gates are events, never yield polls.
@MainActor
@Suite struct StatsKeptResultsTests {
    let directory = URL.temporaryDirectory.appending(path: "stats-persistence-\(UUID().uuidString)", directoryHint: .isDirectory)

    private func opened() -> StatsKeptResults {
        let kept = StatsKeptResults(directory: directory)
        kept.open(collection: "collection-a")
        return kept
    }

    private func reload() async -> [StatsPartSet] {
        let reader = opened()
        reader.prepare(.allDecks) {}
        await reader.waitForLoads()
        return reader.sets(for: .allDecks)
    }

    @Test func identicalPendingAndSavedSnapshotsDoNoMoreWork() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        let gate = SaveGate()
        defer { gate.release() }
        var saves = 0
        kept.beforeSave = { saves += 1; if saves == 1 { await gate.hold() } }
        let set = Fixture.set([.cardCounts])
        kept.keep(set)
        kept.persist(.allDecks)
        await gate.arrival()
        for _ in 0..<4 { kept.keep(set); kept.persist(.allDecks) }
        #expect(saves == 1 && kept.pendingWriteCount == 1)
        gate.release()
        await kept.waitForWrites()
        for _ in 0..<4 { kept.keep(set); kept.persist(.allDecks) }
        await kept.waitForWrites()
        #expect(saves == 1 && kept.pendingWriteCount == 0)

        // A second history does not rewrite the first; neither does OS cache eviction.
        kept.keep(Fixture.set([.cardCounts], history: .allHistory))
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 2)
        try FileManager.default.removeItem(at: directory)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 2)
        #expect(!FileManager.default.fileExists(atPath: directory.path))
    }

    @Test func acceptedPartsAndCompletionChangeTheSavedVersion() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        var saves = 0
        kept.beforeSave = { saves += 1 }
        var set = Fixture.set([.cardCounts])
        set.complete = false
        kept.keep(set)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 1)
        let header = try #require(set.header)
        let accepted = set.receive(StatsPartResult(part: .cardCounts, header: header,
                                                 outcome: .data(.cardCounts(Fixture.cardCounts(new: 20)))))
        #expect(accepted)
        kept.keep(set)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 2)
        set.complete = true // No accepted part here, but the saved completion flag changes.
        kept.keep(set)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        let wrong = Fixture.header(refresh: header.refresh + 1, history: set.history)
        let refused = set.receive(StatsPartResult(part: .cardCounts, header: wrong, outcome: .data(Fixture.value(.cardCounts))))
        #expect(!refused)
        kept.keep(set)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 3)
        let saved = await reload()
        #expect(saved.first?.complete == true)
        #expect(saved.first?.value(.cardCounts) == set.value(.cardCounts))
    }

    @Test func onlyTheFailedHistoryRetriesOnTheNextNormalPersist() async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        var saves = 0
        kept.beforeSave = { saves += 1 }
        // A directory at the destination refuses the atomic file write, after encoding.
        let blocked = directory.appending(path: "all-allHistory.plist", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: blocked, withIntermediateDirectories: true)
        kept.keep(Fixture.set([.cardCounts]))
        kept.keep(Fixture.set([.cardCounts], history: .allHistory))
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 2)
        #expect(kept.pendingWriteCount == 0)
        try FileManager.default.removeItem(at: blocked)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 3)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 3)
        #expect(await reload().count == 2)
    }

    @Test(arguments: [false, true])
    func distinctRequestsKeepOnlyTheLatestEvenAfterFailureAndEviction(firstFails: Bool) async throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        let gate = SaveGate()
        defer { gate.release() }
        var saves = 0
        kept.beforeSave = { saves += 1; if saves == 1 { await gate.hold() } }
        let blocked = directory.appending(path: "all-last12Months.plist", directoryHint: .isDirectory)
        if firstFails {
            try FileManager.default.createDirectory(at: blocked, withIntermediateDirectories: true)
            kept.afterSave = { if saves == 1 { try? FileManager.default.removeItem(at: blocked) } }
        }
        var set = Fixture.set([.cardCounts])
        kept.keep(set)
        kept.persist(.allDecks)
        await gate.arrival()
        let header = try #require(set.header)
        for value in 1...8 {
            set.receive(StatsPartResult(part: .cardCounts, header: header,
                                       outcome: .data(.cardCounts(Fixture.cardCounts(new: value)))))
            kept.keep(set)
            kept.persist(.allDecks)
        }
        #expect(saves == 1 && kept.pendingWriteCount == 1)
        kept.trim()
        #expect(kept.sets(for: .allDecks).isEmpty)
        gate.release()
        await kept.waitForWrites()
        #expect(saves == 2 && kept.pendingWriteCount == 0)
        #expect(kept.persistenceKeyCount == 0)
        #expect(await reload().first?.value(.cardCounts) == set.value(.cardCounts))
    }

    @Test(arguments: [false, true])
    func lastSuccessDoesNotCleanUnrequestedChangesOrWriteAnObsoleteReplacement(returnToActive: Bool) async {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        let gate = SaveGate()
        defer { gate.release() }
        var saves = 0
        kept.beforeSave = { saves += 1 }
        kept.afterSave = { await gate.hold() }
        var set = Fixture.set([.cardCounts])
        set.complete = false
        kept.keep(set)
        kept.persist(.allDecks)
        await gate.arrival() // A is written; its completion has not landed.
        kept.afterSave = nil
        set.complete = true
        kept.keep(set)
        if returnToActive {
            kept.persist(.allDecks) // Request B, then A again; B must never be written.
            set.complete = false
            kept.keep(set)
            kept.persist(.allDecks)
        }
        gate.release()
        await kept.waitForWrites()
        #expect(saves == 1 && kept.pendingWriteCount == 0)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == (returnToActive ? 1 : 2))
        #expect(kept.pendingWriteCount == 0)
        #expect(await reload().first?.complete == set.complete)
    }

    @Test(arguments: ["replacement", "A-B-A", "close"], [false, true])
    func droppedSavesCannotResurrectOrCleanTheNewEpoch(transition: String, alreadyWritten: Bool) async {
        defer { try? FileManager.default.removeItem(at: directory) }
        let kept = opened()
        var saves = 0
        kept.beforeSave = { saves += 1 }
        let gate = SaveGate()
        defer { gate.release() }
        var old = Fixture.set([.cardCounts])
        old.complete = false
        kept.keep(old)
        if alreadyWritten { kept.afterSave = { await gate.hold() } }
        else { kept.beforeSave = { saves += 1; if saves == 1 { await gate.hold() } } }
        kept.persist(.allDecks)
        await gate.arrival()
        kept.afterSave = nil
        old.complete = true
        kept.keep(old)
        kept.persist(.allDecks) // This queued old-epoch replacement must also be dropped.
        switch transition {
        case "replacement": kept.collectionReplaced()
        case "A-B-A": kept.open(collection: "collection-b"); kept.open(collection: "collection-a")
        default: kept.close(); kept.open(collection: "collection-a")
        }
        let fresh = Fixture.set([.cardCounts], values: [.cardCounts: .cardCounts(Fixture.cardCounts(new: 40))])
        kept.keep(fresh)
        kept.persist(.allDecks)
        gate.release()
        await kept.waitForWrites()
        #expect(kept.pendingWriteCount == 0)
        kept.persist(.allDecks)
        await kept.waitForWrites()
        #expect(saves == 2)
        #expect(await reload().first?.value(.cardCounts) == fresh.value(.cardCounts))
    }

    @Test func missingHistoryRetriesAndUnchangedWindowsLeavePeerSnapshotsAlone() async {
        defer { try? FileManager.default.removeItem(at: directory) }
        let reader = opened()
        var saves = 0
        reader.beforeSave = { saves += 1 }
        let original = Fixture.set([.cardCounts])
        reader.keep(original)
        reader.persist(.allDecks)
        await reader.waitForWrites()
        reader.prepare(.allDecks) {}
        await reader.waitForLoads() // allHistory is genuinely missing.
        let peer = opened()
        let newer = Fixture.set([.cardCounts], values: [.cardCounts: .cardCounts(Fixture.cardCounts(new: 40))])
        peer.keep(newer)
        peer.keep(Fixture.set([.cardCounts], history: .allHistory))
        peer.persist(.allDecks)
        await peer.waitForWrites()
        reader.prepare(.allDecks) {}
        await reader.waitForLoads() // A missing read was not negatively cached.
        #expect(reader.hasComplete(.allDecks, .allHistory))
        reader.persist(.allDecks)
        await reader.waitForWrites()
        #expect(saves == 1, "loaded histories are clean too")
        #expect(reader.sets(for: .allDecks).first { $0.history == .last12Months }?.id == original.id)
        reader.trim() // A genuinely needed fresh load must see the peer's later data.
        reader.prepare(.allDecks) {}
        await reader.waitForLoads()
        #expect(reader.sets(for: .allDecks).first { $0.history == .last12Months }?.id == newer.id)
        reader.persist(.allDecks)
        await reader.waitForWrites()
        #expect(saves == 1)
    }
}

/// One explicitly released suspension; arrival is an event, never a timing assumption.
@MainActor
private final class SaveGate {
    private var entered = false
    private var arrived: CheckedContinuation<Void, Never>?
    private var held: CheckedContinuation<Void, Never>?

    func hold() async {
        await withCheckedContinuation { continuation in
            held = continuation
            entered = true
            arrived?.resume()
            arrived = nil
        }
    }

    func arrival() async {
        if entered { return }
        await withCheckedContinuation { arrived = $0 }
    }

    func release() {
        held?.resume()
        held = nil
    }
}
