import Dispatch
import Foundation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

final class MaintenanceOccupancy: Sendable {
    let release = DispatchSemaphore(value: 0)
    let entered: AsyncStream<Void>
    let signal: AsyncStream<Void>.Continuation
    let watchdogFired = Mutex(false)
    init() { (entered, signal) = AsyncStream.makeStream() }
}
extension MaintenanceLane {
    func occupy(_ gate: MaintenanceOccupancy) throws {
        try call("test.occupyMaintenance") {
            gate.signal.yield(())
            defer { gate.signal.finish() }
            if gate.release.wait(timeout: .now() + 15) == .timedOut {
                gate.watchdogFired.withLock { $0 = true }
            }
        }
    }
}
private final class ControlProofClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}
extension EventTests {
    @Test @MainActor func realControlServicesFinishWhileMaintenanceIsOccupied() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups,
                                                     appGroupSnapshot: root.appending(path: "widget.snapshot"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: ControlProofClock())
        try await service.open(location, randomSeed: 37)
        do {
            let job = try await Jobs.start("test.startJob") {
                try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
            }
            // A delivered report proves the real engine worker entered this job.
            var progress = job.progress.makeAsyncIterator()
            #expect(await progress.next() != nil)
            let gate = MaintenanceOccupancy()
            let occupying = Task { try await MaintenanceLane.shared.occupy(gate) }
            defer { gate.release.signal() }
            for await _ in gate.entered { break }
            // The actor call is now positively occupied until explicit release
            // or its bounded watchdog. A routing regression fails, not deadlocks.
            let timer = ContinuousClock()
            let flushStart = timer.now
            try await service.flushForBackground()
            let flushElapsed = timer.now - flushStart
            print("U37 real flush while maintenance occupied: \(flushElapsed); engine flush has no timeout input")
            let suspendStart = timer.now
            #expect(try await service.suspend(timeoutMilliseconds: 1_000) == .suspended)
            #expect(timer.now - suspendStart < .seconds(1))
            let resumeStart = timer.now
            _ = try await service.resume()
            #expect(timer.now - resumeStart < .seconds(5))
            #expect(!gate.watchdogFired.withLock { $0 }, "control calls waited for the maintenance watchdog")
            gate.release.signal()
            try await occupying.value
            #expect(await job.outcome() == .cancelled)
            let generation = try await service.waitAvailable(timeoutMilliseconds: 0)
            try await service.close()
            for await event in CoreEventStream.shared.events {
                if event.generation == generation && event.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await service.close()
            throw error
        }
    }
}
