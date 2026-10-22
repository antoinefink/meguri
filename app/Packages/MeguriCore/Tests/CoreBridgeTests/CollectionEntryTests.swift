import Dispatch
import Foundation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private actor EntryGate {
    private var open = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    func wait() async {
        if open { return }
        await withCheckedContinuation { waiters.append($0) }
    }
    func release() { open = true; let pending = waiters; waiters = []; for waiter in pending { waiter.resume() } }
}
private final class EntryTokenWitness: Sendable { let value = Mutex<CoreBridge.CancelToken?>(nil) }
private final class RecordingSourceAccess: CoordinatedSourceAccess {
    let active = Mutex(false)
    let visits = Mutex<[String]>([])
    let coordinated = URL(filePath: "/coordinated/actual.colpkg")
    func read(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws {
        #expect(!Thread.isMainThread)
        active.withLock { $0 = true }; visits.withLock { $0.append("begin") }
        defer { active.withLock { $0 = false }; visits.withLock { $0.append("release") } }
        accessor(coordinated)
    }
}

private final class EntryScopeWitness: SourceSecurityScope {
    let acquired: Bool
    let visits = Mutex<[String]>([])
    init(acquired: Bool) { self.acquired = acquired }
    func start(_ url: URL) -> Bool { #expect(!Thread.isMainThread); visits.withLock { $0.append("start") }; return acquired }
    func stop(_ url: URL) { #expect(!Thread.isMainThread); visits.withLock { $0.append("stop") } }
}

@Suite struct CollectionSourceLifetimeTests {
    @Test(arguments: [false, true], [false, true])
    func foundationBalancesOnlyAcquiredScopeAfterCoordinatedTerminal(acquired: Bool, fails: Bool) async throws {
        let files = FileManager.default
        let url = files.temporaryDirectory.appending(path: "public-source-\(UUID().uuidString).colpkg")
        try Data("public source scope test".utf8).write(to: url)
        defer { try? files.removeItem(at: url) }
        let scope = EntryScopeWitness(acquired: acquired)
        let owner = CoordinatedSourceOwner(access: FoundationSourceAccess(scope: scope))
        do {
            _ = try await owner.withReadablePackage(at: url) { coordinatedURL in
                #expect(coordinatedURL.standardizedFileURL == url.standardizedFileURL)
                #expect(scope.visits.withLock { $0 } == ["start"])
                if fails { throw BridgeError.cancelled }
                return 1
            }
            #expect(!fails)
        } catch { #expect(fails && (error as? BridgeError) == .cancelled) }
        #expect(scope.visits.withLock { $0 } == (acquired ? ["start", "stop"] : ["start"]))
    }

    @Test(arguments: [false, true])
    @MainActor func workerHoldsAccessorUntilAsyncSuccessOrFailureAndUsesCoordinatedURL(fails: Bool) async throws {
        let access = RecordingSourceAccess()
        let owner = CoordinatedSourceOwner(access: access)
        let gate = EntryGate()
        let (entered, continuation) = AsyncStream<Void>.makeStream()
        let task = Task {
            try await owner.withReadablePackage(at: URL(filePath: "/selected/original.colpkg")) { url in
                #expect(url == access.coordinated)
                #expect(access.active.withLock { $0 })
                continuation.yield(())
                await gate.wait()
                #expect(access.active.withLock { $0 })
                if fails { throw BridgeError.invalidArgument("synthetic failure") }
                return 37
            }
        }
        for await _ in entered { break }
        // A cancelled Swift waiter must not release a synchronous accessor.
        task.cancel()
        #expect(access.active.withLock { $0 })
        #expect(access.visits.withLock { $0 } == ["begin"])
        await gate.release()
        if fails {
            do { _ = try await task.value; Issue.record("Expected source failure") }
            catch { #expect(error as? BridgeError == .invalidArgument("synthetic failure")) }
        } else { #expect(try await task.value == 37) }
        #expect(!access.active.withLock { $0 })
        #expect(access.visits.withLock { $0 } == ["begin", "release"])
        continuation.finish()
    }
}

extension EventTests {
    @Test func trackingFailureCancelsAndDrainsStartedRustJobBeforeReleasingSource() async throws {
        try await CoreEventStream.shared.start()
        let access = RecordingSourceAccess()
        let source = CoordinatedSourceOwner(access: access)
        let seen = EntryTokenWitness()
        do {
            _ = try await source.withReadablePackage(at: URL(filePath: "/selected.colpkg")) { _ -> Int in
                let handle = try await MaintenanceLane.shared.call("test.startJob") {
                    try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
                }
                seen.value.withLock { $0 = CoreBridge.CancelToken(raw: handle.cancel) }
                do {
                    _ = try await Jobs.trackDraining(handle, setup: { _ in throw BridgeError.invalidArgument("listener setup failed") })
                    Issue.record("Expected setup failure")
                } catch {
                    #expect(access.active.withLock { $0 })
                    // Terminal was consumed by the fallback registry waiter;
                    // no registered work remains before scope is released.
                    let token = seen.value.withLock { $0 }
                    #expect(try await token?.isCancelled() == true)
                    throw error
                }
                return 0
            }
            Issue.record("Expected setup failure")
        } catch { #expect(error as? BridgeError == .invalidArgument("listener setup failed")) }
        #expect(!access.active.withLock { $0 })
    }

    @Test func explicitCancellationIsRememberedBeforeAttachAndDrainedInsideAccessor() async throws {
        try await CoreEventStream.shared.start()
        let operation = CollectionEntryOperation()
        await operation.cancel()
        let access = RecordingSourceAccess()
        let source = CoordinatedSourceOwner(access: access)
        let result = try await source.withReadablePackage(at: URL(filePath: "/selected.colpkg")) { _ in
            let job = try await Jobs.start("test.startJob") {
                try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
            }
            await operation.attach(job)
            let terminal = await job.outcome()
            #expect(access.active.withLock { $0 })
            return terminal
        }
        #expect(result == .cancelled)
        #expect(!access.active.withLock { $0 })
    }
}

@Suite struct CollectionEntryMappingTests {
    @Test func failureKeepsActivationBackupAndTypedCause() {
        let receipt = MeguriCoreBindings.ImportBackupReceipt(name: "safety.colpkg", sizeBytes: 80, instantMs: 101, snapshotModifiedMs: 99)
        let failure = CollectionEntryFailure(MeguriCoreBindings.ImportFailure(
            issue: .safetyBackup(cause: .package(error: .insufficientSpace(needed: 10, available: 3)),
                                 publication: .durable(receipt: receipt)),
            candidateActive: true, generation: 17, backup: receipt))
        #expect(failure.candidateActive && failure.generation == 17)
        #expect(failure.backup?.snapshotModifiedMilliseconds == 99)
        #expect(failure.issue == .safetyBackup(cause: .package(.insufficientSpace(needed: 10, available: 3)),
                                             publication: .durable(CoreBridge.ImportBackupReceipt(receipt))))
        #expect(CollectionEntryFailure(MeguriCoreBindings.ImportFailure(issue: .staleConfirmation, candidateActive: false,
            generation: nil, backup: nil)).issue == .staleConfirmation)
    }
}
