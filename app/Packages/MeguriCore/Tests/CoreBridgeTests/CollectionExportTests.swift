import Dispatch
import Foundation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private actor ExportGate {
    private var open = false
    private var waiting: [CheckedContinuation<Void, Never>] = []
    func wait() async { if !open { await withCheckedContinuation { waiting.append($0) } } }
    func release() { open = true; let pending = waiting; waiting = []; for waiter in pending { waiter.resume() } }
}
private final class ExportAccessWitness: CollectionExportDestinationAccess {
    let active = Mutex(false)
    let completed = Mutex(false)
    let actual = URL(fileURLWithPath: "/coordinated/actual.colpkg")
    let failAfterAccessor: Bool
    init(failAfterAccessor: Bool = false) { self.failAfterAccessor = failAfterAccessor }
    func write(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws {
        #expect(!Thread.isMainThread)
        active.withLock { $0 = true }
        defer { active.withLock { $0 = false }; completed.withLock { $0 = true } }
        accessor(actual)
        if failAfterAccessor { throw BridgeError.unavailable(reason: "synthetic provider completion failure") }
    }
}
@Suite struct CollectionExportMappingTests {
    @Test func boundedReceiptPreservesAdmissionAndSnapshotFacts() {
        let receipt = CoreBridge.CollectionExportMetadata(MeguriCoreBindings.CollectionExportMetadata(
            operationId: 42, sourceGeneration: 3, admissionSeq: 100, snapshotModifiedMs: 77,
            byteLength: 3_000_000_000, suggestedName: "Meguri-Collection.colpkg", sha256: Data(repeating: 7, count: 32)))
        #expect(receipt.operationID == 42 && receipt.sourceGeneration == 3 && receipt.admissionSequence == 100)
        #expect(receipt.snapshotModifiedMilliseconds == 77 && receipt.byteLength == 3_000_000_000)
        #expect(receipt.sha256.count == 32 && receipt.suggestedName.hasSuffix(".colpkg"))
        let failure = CoreBridge.CollectionExportFailure(MeguriCoreBindings.CollectionExportFailure(
            cause: .Cancelled, publicationUncertain: true, destination: "/synthetic/saved.colpkg"))
        #expect(failure.cause == .cancelled && failure.publicationUncertain)
    }
    @Test(arguments: [false, true])
    func cancelledSwiftTaskKeepsCoordinatedAccessUntilActualIOEnds(fails: Bool) async throws {
        let access = ExportAccessWitness()
        let owner = CoordinatedExportDestination(access: access)
        let gate = ExportGate()
        let (entered, signal) = AsyncStream<Void>.makeStream()
        let task = Task {
            try await owner.withDestination(URL(fileURLWithPath: "/selected/initial.colpkg")) { actual -> Int in
                #expect(actual == access.actual && access.active.withLock { $0 })
                signal.yield(())
                await gate.wait()
                #expect(access.active.withLock { $0 })
                if fails { throw BridgeError.cancelled }
                return 51
            }
        }
        for await _ in entered { break }
        task.cancel()
        #expect(access.active.withLock { $0 } && !access.completed.withLock { $0 })
        await gate.release()
        do { #expect(try await task.value == 51); #expect(!fails) }
        catch { #expect(fails && (error as? BridgeError) == .cancelled) }
        #expect(access.completed.withLock { $0 } && !access.active.withLock { $0 })
        signal.finish()
    }
    @Test func coordinationFailureAfterSuccessfulCopyIsUncertain() async throws {
        let access = ExportAccessWitness(failAfterAccessor: true)
        do {
            _ = try await CoordinatedExportDestination(access: access).withDestination(URL(fileURLWithPath: "/selected.colpkg")) { _ in 1 }
            Issue.record("Expected uncertain completion")
        } catch { #expect(error is ExportDestinationCompletionUncertain) }
    }
}
extension EventTests {
    @Test func exportTrackingFailureDrainsBeforeDestinationAccessIsReleased() async throws {
        try await CoreEventStream.shared.start()
        let access = ExportAccessWitness()
        do {
            _ = try await CoordinatedExportDestination(access: access).withDestination(URL(fileURLWithPath: "/synthetic.colpkg")) { _ -> Int in
                let handle = try await MaintenanceLane.shared.call("test.startJob") {
                    try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
                }
                do {
                    _ = try await Jobs.trackDraining(handle, setup: { _ in throw BridgeError.invalidArgument("export listener setup failed") })
                    Issue.record("Expected setup failure")
                } catch {
                    #expect(access.active.withLock { $0 })
                    #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
                    throw error
                }
                return 1
            }
            Issue.record("Expected drained setup failure")
        } catch { #expect(error as? BridgeError == .invalidArgument("export listener setup failed")) }
        #expect(access.completed.withLock { $0 })
    }
    @Test func exportCancellationBeforeAttachCancelsRealWorker() async throws {
        let operation = CollectionExportOperation()
        await operation.cancel()
        let job = try await Jobs.start("test.startJob") { try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil) }
        await operation.attach(job)
        #expect(await job.outcome() == .cancelled)
    }
}
