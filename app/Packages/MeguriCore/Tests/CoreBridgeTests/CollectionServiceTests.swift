import Dispatch
import Foundation
import Synchronization
import Testing
import MeguriCoreBindings
@testable import CoreBridge

private final class ServiceClock: CollectionClock {
    let mainThreadCalls = Mutex<[Bool]>([])
    func nowMilliseconds() -> Int64 { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return 0 }
    func zoneIdentifier() -> String { mainThreadCalls.withLock { $0.append(Thread.isMainThread) }; return "UTC" }
}

@MaintenanceLane private func bootstrapCollection(_ location: CoreBridge.CollectionLocation) throws {
    try MaintenanceLane.shared.call("test.bootstrapCollection") {
        try testBootstrapCollection(location: location.raw)
    }
}

// Every test that changes this process's open collection joins the existing
// serialized listener suite, including the closed-review refusal test.
extension EventTests {
    @Test @MainActor func collectionAndDeckServicesOpenReadCloseAndReopenOffMain() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups, appGroupSnapshot: root.appending(path: "widget.snapshot"))
        try await bootstrapCollection(location)
        let clock = ServiceClock()
        let service = CoreCollectionService(clock: clock)
        let decks = CoreDeckService()
        do {
            let job = try await service.startOpen(location, randomSeed: 37)
            #expect(await job.outcome() == .succeeded)
            let first = try await service.waitAvailable(timeoutMilliseconds: 0)
            #expect(try await service.lifecycle() == .ready(generation: first))
            let tree = try await decks.treeWithCounts()
            #expect(tree.generation == first)
            #expect(tree.nodes.map(\.id) == [1])
            #expect(tree.nodes.first?.counts == CoreBridge.ReviewCounts(new: 0, learning: 0, review: 0))
            #expect(tree.nextChangeAt == 1_894_766_400)
            #expect(try await decks.studyState(deckID: 1).empty)
            #expect(try await decks.descriptionHTML(deckID: 1) == "")
            try await service.close()
            #expect(try await service.lifecycle() == .noCollection)
            try await service.open(location, randomSeed: 37)
            #expect(try await service.waitAvailable(timeoutMilliseconds: 0) == first + 1)
            try await service.close()
            // Drain through the final close before another serialized test can
            // replace the listener or consume this process-wide stream.
            for await event in CoreEventStream.shared.events {
                if event.generation == first + 1 && event.kind == .lifecycle(.noCollection) { break }
            }
            let calls = clock.mainThreadCalls.withLock { $0 }
            #expect(!calls.isEmpty && calls.allSatisfy { !$0 })
        } catch {
            try? await service.close()
            throw error
        }
    }

    @Test func aLongAvailabilityWaitLeavesControlFreeAndCancelsPromptly() async throws {
        let service = CoreCollectionService()
        #expect(try await service.lifecycle() == .noCollection)
        let (attempts, continuation) = AsyncStream<Void>.makeStream()
        let timer = ContinuousClock()
        let waiter = Task { () -> (Result<UInt64, BridgeError>, ContinuousClock.Instant) in
            let result: Result<UInt64, BridgeError>
            do throws(BridgeError) {
                result = .success(try await service.waitAvailable(timeoutMilliseconds: .max, using: {
                    #expect(String(cString: __dispatch_queue_get_label(nil)) == Lane.control.queueLabel)
                    continuation.yield(())
                    return try waitCollectionAvailable(timeoutMs: 0)
                }))
            } catch {
                result = .failure(error)
            }
            return (result, timer.now)
        }
        defer { waiter.cancel(); continuation.finish() }
        for await _ in attempts { break }
        let controlStarted = timer.now
        #expect(try await service.lifecycle() == .noCollection)
        try await service.close()
        #expect(timer.now - controlStarted < .milliseconds(100))
        let cancelledAt = timer.now
        waiter.cancel()
        let (result, returnedAt) = await waiter.value
        #expect(throws: BridgeError.cancelled) { try result.get() }
        #expect(returnedAt - cancelledAt < .milliseconds(100))
    }
}

@Suite struct CollectionServiceTests {
    @Test func availabilityUsesOneDeadlineAndReturnsTheCoreRefusal() async throws {
        let service = CoreCollectionService()
        let probes = Counter()
        let refusal = BridgeError.unavailable(reason: "synthetic unavailable state")
        let timer = ContinuousClock()
        let started = timer.now
        await #expect(throws: refusal) {
            try await service.waitAvailable(timeoutMilliseconds: 30, using: {
                probes.add(1)
                throw refusal
            })
        }
        let elapsed = timer.now - started
        #expect(probes.add(0) > 1)
        #expect(elapsed >= .milliseconds(30))
        #expect(elapsed < .milliseconds(100))
    }

    @Test func zeroTimeoutProbesOnceAndOtherErrorsPropagateImmediately() async throws {
        let service = CoreCollectionService()
        for refusal in [BridgeError.unavailable(reason: "closed"), .busy(runningOperation: "restore"), .internal("synthetic failure"), .cancelled] {
            let probes = Counter()
            await #expect(throws: refusal) {
                try await service.waitAvailable(timeoutMilliseconds: refusal == .unavailable(reason: "closed") ? 0 : .max, using: {
                    probes.add(1)
                    throw refusal
                })
            }
            #expect(probes.add(0) == 1)
        }
        #expect(try await service.waitAvailable(timeoutMilliseconds: 0, using: { 37 }) == 37)
    }

    @Test func availabilityReturnsTheGenerationWhenAProbeBecomesReady() async throws {
        let service = CoreCollectionService()
        let probes = Counter()
        let generation = try await service.waitAvailable(timeoutMilliseconds: 1_000, using: {
            if probes.add(1) == 1 { throw BridgeError.unavailable(reason: "opening") }
            return 37
        })
        #expect(generation == 37)
        #expect(probes.add(0) == 2)
    }

    @Test func scriptedFakeRecordsRequestsAndReturnsErrorsWithoutCore() async throws {
        let location = CoreBridge.CollectionLocation(collectionDirectory: URL(fileURLWithPath: "/preview/collection"), backupsDirectory: URL(fileURLWithPath: "/preview/backups"), appGroupSnapshot: URL(fileURLWithPath: "/preview/widget"))
        let fake = FakeCollectionService(opens: [.success(())], closes: [.success(())], states: [.success(.opening), .success(.ready(generation: 7))], waits: [.failure(.unavailable(reason: "deadline"))])
        let service: any CollectionService = fake
        try await service.open(location, randomSeed: 19)
        #expect(try await service.lifecycle() == .opening)
        #expect(try await service.lifecycle() == .ready(generation: 7))
        await #expect(throws: BridgeError.unavailable(reason: "deadline")) { try await service.waitAvailable(timeoutMilliseconds: 3) }
        try await service.close()
        #expect(await fake.calls == [.open(location, seed: 19), .lifecycle, .lifecycle, .waitAvailable(3), .close])
    }
    @Test func systemClockUsesTheOffsetAtTheRequestedInstant() {
        let clock = SystemCollectionClock()
        for instant in [Int64(0), 1_735_689_600_000, 1_751_328_000_000] {
            #expect(clock.utcOffsetSeconds(atMilliseconds: instant) == Int32(TimeZone.current.secondsFromGMT(for: Date(timeIntervalSince1970: Double(instant) / 1_000))))
        }
        #expect(clock.zoneIdentifier() == TimeZone.current.identifier)
        #expect(abs(clock.nowMilliseconds() - Int64(Date().timeIntervalSince1970 * 1_000)) < 1_000)
    }
    @Test func foreignClockRelayForwardsTheRequestedInstant() {
        struct Clock: CollectionClock {
            func nowMilliseconds() -> Int64 { 123 }
            func utcOffsetSeconds(atMilliseconds instant: Int64) -> Int32 { instant == 456 ? 3_600 : -3_600 }
            func zoneIdentifier() -> String { "synthetic-zone" }
        }
        let relay = CollectionClockRelay(Clock())
        #expect(relay.nowMs() == 123)
        #expect(relay.offsetFromUtc(instantMs: 456) == 3_600)
        #expect(relay.offsetFromUtc(instantMs: 789) == -3_600)
        #expect(relay.zoneId() == "synthetic-zone")
    }
    @Test func importantUsageCapacityFailureIsNotUnlimitedSpace() throws {
        struct Failure: DiskSpaceProviding {
            func availableBytes(at directory: URL) throws -> UInt64 { throw BridgeError.unavailable(reason: "volume gone") }
        }
        guard case .failed(let detail) = DiskSpaceRelay(Failure()).availableBytes(directory: "/missing") else {
            Issue.record("Capacity callback must report failure"); return
        }
        #expect(detail.contains("volume gone"))
        let missing = URL(fileURLWithPath: "/does-not-exist/meguri-capacity-\(UUID().uuidString)")
        #expect(throws: (any Error).self) { try DiskSpaceProvider().availableBytes(at: missing) }
    }
}

@Suite struct CollectionLifecycleTests {
    @Test func lifecycleEventsPreserveStructuredFailuresAndExclusiveReasons() {
        let failures: [(MeguriCoreBindings.CollectionOpenFailure, CoreBridge.CollectionOpenFailure)] = [
            (.openInAnotherProgram, .openInAnotherProgram), (.tooOld(version: 10), .tooOld(version: 10)),
            (.tooNew(version: 19), .tooNew(version: 19)), (.v1Scheduler, .v1Scheduler),
            (.notACollection(missingTables: ["cards", "notes"]), .notACollection(missingTables: ["cards", "notes"])),
            (.upgradeFailed(version: 11, stage: .copy, detail: "space"), .upgradeFailed(version: 11, stage: .copy, detail: "space")),
            (.upgradeFailed(version: 11, stage: .step(step: 14), detail: "decode"), .upgradeFailed(version: 11, stage: .step(14), detail: "decode")),
            (.corrupt(detail: "page"), .corrupt(detail: "page")), (.connectionInit(detail: "collation"), .connectionInit(detail: "collation")),
            (.notFound, .notFound), (.io(detail: "volume"), .io(detail: "volume")),
        ]
        for (raw, expected) in failures {
            let event = CoreBridge.CoreEvent(.init(seq: 91, generation: 7, causeSeq: nil, kind: .lifecycle(state: .failed(reason: .open(error: raw)))))
            #expect(event.seq == 91 && event.generation == 7 && event.causeSeq == nil)
            #expect(event.kind == .lifecycle(.failed(.open(expected))))
        }
        let reasons: [(MeguriCoreBindings.CollectionFailedReason, CoreBridge.CollectionFailedReason)] = [
            (.openStep(step: "media", detail: "recovery"), .openStep(step: "media", detail: "recovery")),
            (.replace(detail: "reopen"), .replace(detail: "reopen")), (.corrupt(detail: "page"), .corrupt(detail: "page")),
        ]
        for (raw, expected) in reasons {
            #expect(CoreBridge.CollectionLifecycle(.failed(reason: raw)) == .failed(expected))
        }
        let operations: [(MeguriCoreBindings.CollectionLongOp, CoreBridge.CollectionLongOp)] = [
            (.oneWaySync, .oneWaySync), (.restore, .restore), (.colpkgImport, .colpkgImport), (.apkgApply, .apkgApply), (.checkDatabase, .checkDatabase),
        ]
        let phases: [(MeguriCoreBindings.CollectionExclusivePhase, CoreBridge.CollectionExclusivePhase)] = [
            (.draining, .draining), (.working, .working), (.replacing, .replacing),
        ]
        for (op, mappedOp) in operations {
            for (phase, mappedPhase) in phases {
                for cancellable in [true, false] {
                    #expect(CoreBridge.CollectionLifecycle(.exclusiveBusy(op: op, phase: phase, cancellable: cancellable)) == .exclusiveBusy(op: mappedOp, phase: mappedPhase, cancellable: cancellable))
                }
            }
        }
        #expect(CoreBridge.CollectionLifecycle(.noCollection) == .noCollection)
        #expect(CoreBridge.CollectionLifecycle(.opening) == .opening)
        #expect(CoreBridge.CollectionLifecycle(.upgrading(from: 11, step: 16)) == .upgrading(from: 11, step: 16))
        #expect(CoreBridge.CollectionLifecycle(.ready(generation: 23)) == .ready(generation: 23))
        #expect(CoreBridge.CoreEvent(.init(seq: 92, generation: 7, causeSeq: nil, kind: .available)).kind == .available)
    }
}
