import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct ReviewDayMonitorTests {
    @Test func offsetBoundaryAndLegacyBoundaryUseNextWholeSecond() throws {
        let zone = try #require(TimeZone(identifier: "Europe/Paris"))
        #expect(ReviewDayMonitor.deadline(nextDayAt: 1_901_156_400,
            now: Date(timeIntervalSince1970: 1_901_098_800), zone: zone) == Date(timeIntervalSince1970: 1_901_149_200))
        #expect(ReviewDayMonitor.deadline(nextDayAt: 100,
            now: Date(timeIntervalSince1970: 100.8), zone: TimeZone(secondsFromGMT: 0)!) == Date(timeIntervalSince1970: 101))
    }
    @Test func newestEventWinsOverCheckAndResumeAndCloseCancels() async {
        var scheduled: [Date] = []
        var cancelled = 0
        let monitor = ReviewDayMonitor(clock: .fixed(Date(timeIntervalSince1970: 100)), zone: { TimeZone(secondsFromGMT: 0)! },
            check: { DayClockReport(today: 1, nextDayAt: 200, rolledOverFrom: nil) },
            schedule: { date, _ in scheduled.append(date); return { cancelled += 1 } })
        let callBegan = monitor.beginResult()
        monitor.event(nextDayAt: 150)
        monitor.result(nextDayAt: 200, beganAt: callBegan)
        #expect(scheduled == [Date(timeIntervalSince1970: 150)])
        monitor.event(nextDayAt: 160)
        #expect(cancelled == 1)
        monitor.close()
        monitor.event(nextDayAt: 170)
        #expect(cancelled == 2)
        #expect(monitor.deadline == nil)
    }
    @Test func unavailableCheckArmsNothingUntilReadyTrigger() async {
        var fails = true
        var scheduled = 0
        let monitor = ReviewDayMonitor(clock: .fixed(Date(timeIntervalSince1970: 100)), zone: { TimeZone(secondsFromGMT: 0)! },
            check: {
                if fails { throw BridgeError.unavailable(reason: "exclusive phase") }
                return DayClockReport(today: 1, nextDayAt: 200, rolledOverFrom: nil)
            }, schedule: { _, _ in scheduled += 1; return {} })
        await monitor.checkNow()
        #expect(scheduled == 0)
        fails = false
        await monitor.checkNow()
        #expect(scheduled == 1)
    }
    @Test func recordsRejectOlderRecoveryAndPreviousSessionWrites() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ReviewRecordStore(file: directory.appending(path: "record.json"))
        var first = ReviewRecord(deckID: 1, startedAt: Date(timeIntervalSince1970: 100))
        try await store.begin(first)
        let old = first
        first.finished = true
        try await store.save(first, version: 2)
        try await store.save(old, version: 1)
        #expect(try await store.load()?.finished == true)
        let second = ReviewRecord(deckID: 2, startedAt: Date(timeIntervalSince1970: 200))
        try await store.begin(second)
        try await store.save(first, version: 3)
        let reopened = ReviewRecordStore(file: directory.appending(path: "record.json"))
        #expect(try await reopened.load()?.deckID == 2)
    }
}
