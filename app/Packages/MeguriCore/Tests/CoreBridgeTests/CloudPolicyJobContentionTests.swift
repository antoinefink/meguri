import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

// Holds the real Driver at project_and_step's clock read. The timeout only
// releases a broken implementation so the tests can report and join it.
private final class PolicyProjectionClock: CollectionClock {
    private let armed = Mutex(false)
    let watchdog = Mutex(false)
    let release = DispatchSemaphore(value: 0)
    let entered: AsyncStream<Void>
    private let signal: AsyncStream<Void>.Continuation
    init() { (entered, signal) = AsyncStream.makeStream() }
    func arm() { armed.withLock { $0 = true } }
    func nowMilliseconds() -> Int64 {
        let hold = armed.withLock { value in let old = value; value = false; return old }
        if hold {
            signal.yield(())
            if release.wait(timeout: .now() + 10) == .timedOut { watchdog.withLock { $0 = true } }
        }
        return 1_894_701_600_000
    }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private func withPolicyProjectionClock(_ clock: PolicyProjectionClock,
                                       _ body: (CoreCollectionService, CoreCloudWorkflowService) async throws -> Void) async throws {
    let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    let data = directory.appending(path: "collection"), backups = directory.appending(path: "backups")
    try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let location = CoreBridge.CollectionLocation(collectionDirectory: data, backupsDirectory: backups,
                                                 appGroupSnapshot: directory.appending(path: "widget"))
    let collection = CoreCollectionService(clock: clock)
    do {
        try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-u114", environment: .development,
            status: .available, identity: Data("synthetic-account-A".utf8))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        try await collection.open(location, randomSeed: 114)
        try await body(collection, CoreCloudWorkflowService())
        try await collection.close()
    } catch {
        clock.release.signal()
        try? await collection.close()
        throw error
    }
}

extension EventTests {
    /// Tests-only RED recipe: apply only this file to the original tree and
    /// select this test. It uses only APIs present before the migration.
    /// The watchdog releases the original blocking Maintenance call; the
    /// before-release assertion then fails instead of falsely claiming a pass.
    @Test func policyFacadeProjectionAllowsUnrelatedReviewStartAdmission() async throws {
        let clock = PolicyProjectionClock()
        try await withPolicyProjectionClock(clock) { _, workflow in
            let policy = try await workflow.policy()
            clock.arm()
            let inputTask = Task { try await policy.input(.foreground) }
            defer { clock.release.signal() }
            for await _ in clock.entered { break }
            do {
                let review = try await Core.startSessionJob(deckID: Int64.max)
                let admittedWhileHeld = !clock.watchdog.withLock { $0 }
                clock.release.signal()
                _ = try await inputTask.value
                // This is START admission, with no completion guarantee while
                // other jobs occupy the bounded pool.
                _ = await review.job.outcome()
                #expect(admittedWhileHeld, "review START waited behind policy projection on Maintenance")
            } catch {
                clock.release.signal()
                _ = try? await inputTask.value
                throw error
            }
        }
    }
}
