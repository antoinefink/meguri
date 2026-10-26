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

extension CollectionSourceLifetimeTests {
    @Test func recoveryEvidenceSurvivesCancelledSourceWaiterUntilTerminal() async throws {
        let access = RecordingSourceAccess()
        let owner = CoordinatedSourceOwner(access: access)
        let gate = EntryGate()
        let (entered, continuation) = AsyncStream<Void>.makeStream()
        let failure = CollectionEntryFailure(issue: .replacement(.mediaRecoveryRequired(recoveryReference: "r6-held")),
            candidateActive: false, mediaRecoveryReference: "r6-held",
            media: .init(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1))
        let task = Task {
            try await owner.withReadablePackage(at: URL(filePath: "/selected.colpkg")) { _ -> Int in
                continuation.yield(())
                await gate.wait()
                #expect(access.active.withLock { $0 })
                throw failure
            }
        }
        for await _ in entered { break }
        task.cancel()
        #expect(access.active.withLock { $0 })
        await gate.release()
        do { _ = try await task.value; Issue.record("Expected typed recovery failure") }
        catch { #expect(error as? CollectionEntryFailure == failure) }
        #expect(access.visits.withLock { $0 } == ["begin", "release"])
        continuation.finish()
    }
}

extension EventTests {
    @Test func inspectionSeedProviderRunsInsideRealSourceOwnerAndRefusesBeforeDispatch() async throws {
        try await CoreEventStream.shared.start()
        let access = RecordingSourceAccess()
        let witness = Mutex<[UInt64]>([])
        let service = CoreCollectionEntryService(source: CoordinatedSourceOwner(access: access), inspectionSeed: {
            #expect(access.active.withLock { $0 })
            witness.withLock { $0.append(43) }
            throw BridgeError.invalidArgument("declared seed refused")
        })
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let location = CoreBridge.CollectionLocation(collectionDirectory: root, backupsDirectory: root.appending(path: "backups"),
            appGroupSnapshot: root.appending(path: "widget"))
        do {
            _ = try await service.inspect(source: URL(filePath: "/selected/package.colpkg"), location: location,
                operation: CollectionEntryOperation(), progress: { _ in })
            Issue.record("Seed refusal must stop before the real FFI call")
        } catch { #expect(error as? BridgeError == .invalidArgument("declared seed refused")) }
        #expect(witness.withLock { $0 } == [43])
        #expect(access.visits.withLock { $0 } == ["begin", "release"])
        #expect(!FileManager.default.fileExists(atPath: root.path))
    }

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
            candidateActive: true, generation: 17, backup: receipt, mediaRecoveryReference: nil, media: nil))
        #expect(failure.candidateActive && failure.generation == 17)
        #expect(failure.backup?.snapshotModifiedMilliseconds == 99)
        #expect(failure.issue == .safetyBackup(cause: .package(.insufficientSpace(needed: 10, available: 3)),
                                             publication: .durable(CoreBridge.ImportBackupReceipt(receipt))))
        #expect(CollectionEntryFailure(MeguriCoreBindings.ImportFailure(issue: .staleConfirmation, candidateActive: false,
            generation: nil, backup: nil, mediaRecoveryReference: nil, media: nil)).issue == .staleConfirmation)
    }
    @Test func inspectionAndActivationMapAllMediaFactsAndSynthesizedFailuresKeepThem() {
        let media = MeguriCoreBindings.CollectionImportMedia(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)
        let facts = CoreBridge.CollectionInspectionFacts(MeguriCoreBindings.CollectionInspectionFacts(
            notes: 6, cards: 11, reviews: 7, sourceSchema: 18, destination: "/private/not-for-presentation",
            replacingExisting: true, sourceSha256: Data([1, 2]), mediaCount: 5, mediaDecodedBytes: 123))
        #expect(facts.mediaCount == 5 && facts.mediaDecodedBytes == 123)
        #expect(facts.notes == 6 && facts.cards == 11 && facts.reviews == 7 && facts.sourceSHA256 == Data([1, 2]))
        let receipt = MeguriCoreBindings.ImportBackupReceipt(name: "safety.colpkg", sizeBytes: 80, instantMs: 101, snapshotModifiedMs: 99)
        let activation = CoreBridge.CollectionActivation(MeguriCoreBindings.CollectionActivation(
            generation: 19, backup: receipt, media: media, mediaRetentionReference: "r6-retained"))
        #expect(activation.media == CoreBridge.CollectionImportMedia(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1))
        #expect(activation.mediaRetentionReference == "r6-retained")
        for issue in [CoreBridge.ImportIssue.core(.internal("tracking")), .core(.unavailable(reason: "host setup"))] {
            let failure = CollectionEntryFailure(after: activation, issue: issue)
            #expect(failure.issue == issue && failure.candidateActive)
            #expect(failure.generation == activation.generation && failure.backup == activation.backup)
            #expect(failure.media == activation.media && failure.mediaRecoveryReference == activation.mediaRetentionReference)
            #expect(failure.completedActivation == activation)
            #expect(failure.requiresRecovery && !failure.requiresReopenRecovery)
        }
    }

    @Test(arguments: [false, true]) func recoveryRequiredKeepsBothCommitSidesAndUnknownGeneration(active: Bool) {
        let raw = MeguriCoreBindings.ImportFailure(
            issue: .replacement(failure: .mediaRecoveryRequired(recoveryReference: "r6-recovery")),
            candidateActive: active, generation: nil, backup: nil, mediaRecoveryReference: "r6-recovery",
            media: .init(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1))
        let failure = CollectionEntryFailure(raw)
        #expect(failure.issue == .replacement(.mediaRecoveryRequired(recoveryReference: "r6-recovery")))
        #expect(failure.candidateActive == active && failure.generation == nil)
        #expect(failure.mediaRecoveryReference == "r6-recovery" && failure.media?.trashed == 1)
        #expect(failure.requiresRecovery && failure.requiresReopenRecovery)
        #expect(failure.preparedMedia == failure.media && failure.completedActivation == nil)
    }

    @Test(arguments: [
        NativeActivationUncertainty.outcomeReadFailed(.internal("read failed")),
        .outcomePending
    ])
    func unavailableOutcomeKeepsOnlyPreparedEvidence(cause: NativeActivationUncertainty) {
        let facts = CoreBridge.CollectionImportMedia(
            incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)
        let result = CollectionEntryFailure(
            issue: .nativeActivationUncertain(cause), preparedMedia: facts)

        #expect(result.activationOutcomeUncertain)
        #expect(result.requiresRecovery)
        #expect(result.requiresReopenRecovery)
        #expect(!result.candidateActive)
        #expect(result.generation == nil)
        #expect(result.backup == nil)
        #expect(result.mediaRecoveryReference == nil)
        #expect(result.media == nil)
        #expect(result.completedActivation == nil)
        #expect(result.preparedMedia == facts)
    }

    @Test func typedPreparedFactsReachStreamBeforeTerminalDrain() async {
        let (stream, continuation) = AsyncStream<CoreProgress>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let relay = ProgressRelay(continuation)
        relay.onProgress(update: .init(jobId: 71, phase: .committing, etaMillis: nil, completed: 0, total: nil,
            collectionMediaPrepared: .init(incoming: 5, decodedBytes: 123, reused: 2, added: 3, trashed: 1)))
        let completion = JobCompletion(progress: continuation)
        completion.resolve(.succeeded)
        var updates: [CoreProgress] = []
        for await update in stream { updates.append(update) }
        #expect(updates.count == 1 && updates.first?.cancellable == false)
        #expect(updates.first?.collectionMediaPrepared?.added == 3)
        #expect(updates.first?.collectionMediaPrepared?.decodedBytes == 123)
        #expect(await completion.wait() == .succeeded)
    }

}
