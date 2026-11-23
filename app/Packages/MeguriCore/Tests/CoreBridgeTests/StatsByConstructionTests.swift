import Foundation
import Testing
@testable import CoreBridge
import MeguriCoreBindings

// The generated bindings use the same names: these are CoreBridge's.
private typealias StatsPart = CoreBridge.StatsPart
private typealias StatsPageRequest = CoreBridge.StatsPageRequest
private typealias StatsPageEnd = CoreBridge.StatsPageEnd

private final class ConstructionClock: CollectionClock {
    let now: Int64
    init(now: Int64) { self.now = now }
    func nowMilliseconds() -> Int64 { now }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class ConstructionDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension EventTests {
    /// `stats/statistics-page` S20's by-construction check (plan U299, Cards Over Time beside
    /// Card Counts): on one result, Cards Over Time's last day is Card Counts' `including_inactive`
    /// without `suspended_by_state`, state by state (`stats/history-reads` H2.2), so the lead chart
    /// ends where the table stands. On a copy of the schema-18 fixture, or of the collection
    /// `MEGURI_STATS_FIXTURE` names (the plan's 150k fixture: `tools/mg fixtures 150k`'s
    /// `collection.anki2`, its clock from the manifest beside it).
    @Test(.timeLimit(.minutes(10))) func cardsOverTimeEndsWhereCardCountsStand() async throws {
        let given = ProcessInfo.processInfo.environment["MEGURI_STATS_FIXTURE"].map { URL(filePath: $0) }
        let source = given ?? URL(filePath: #filePath).deletingLastPathComponent().appending(
            path: "../../../../../conformance/fixtures/storage/schema-18.anki2")
        var now: Int64 = 1_894_701_600_000
        if let given, let manifest = try? Data(contentsOf: given.deletingLastPathComponent().appending(path: "manifest.json")),
           let object = try? JSONSerialization.jsonObject(with: manifest) as? [String: Any],
           let value = object["now_ms"] as? NSNumber {
            now = value.int64Value
        }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        let service = CoreCollectionService(clock: ConstructionClock(now: now), diskSpace: ConstructionDiskCapacity())
        let stats = CoreStatsService()
        do {
            try await service.open(location, randomSeed: 11)
            let stream = try await stats.page(StatsPageRequest(scope: .allDecks, period: .all, refresh: 1))
            var counts: StatsCardCounts?
            var states: StatsCardStatesOverTime?
            var ends: [StatsPageEnd] = []
            for await message in stream {
                switch message {
                case let .part(result):
                    switch result.outcome {
                    case let .data(.cardCounts(value)): counts = value
                    case let .data(.cardStatesOverTime(value)): states = value
                    default: break
                    }
                case .progress: break
                case let .end(end): ends.append(end)
                }
            }
            #expect(ends == [.complete])
            let all = try #require(counts).includingInactive
            let suspended = try #require(counts).suspendedByState
            let today = try #require(states).counts(on: 0)
            #expect(today.new == all.new - suspended.new)
            #expect(today.learning == all.learning - suspended.learning)
            #expect(today.relearning == all.relearning - suspended.relearning)
            #expect(today.young == all.young - suspended.young)
            #expect(today.mature == all.mature - suspended.mature)
            #expect(try #require(states).leftOut == suspended.total)
            if given != nil {
                // The record of the run: the cards it checked.
                let cards = all.new + all.learning + all.relearning + all.young + all.mature + all.suspended + all.buried
                print("by construction: \(cards) cards, \(today.total) in Cards Over Time's last day, \(suspended.total) suspended")
                #expect(cards > 0)
            }
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
