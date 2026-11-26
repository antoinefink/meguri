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
    @Test func deckOptionsPreserveEveryFlagAcrossTheBridge() {
        let defaults = CoreBridge.DeckExportOptions()
        #expect(!defaults.raw.includeScheduling && !defaults.raw.includePresets && defaults.raw.includeMedia)
        let chosen = CoreBridge.DeckExportOptions(includeScheduling: true, includePresets: true, includeMedia: false)
        #expect(chosen.raw.includeScheduling && chosen.raw.includePresets && !chosen.raw.includeMedia)
    }
    @Test func sourceRefusalsSurviveTheExistingTypedFailureReceipt() {
        let cases: [(MeguriCoreBindings.CoreError, BridgeError)] = [
            (.ExportSourceChanged, .exportSourceChanged),
            (.ExportSourceInvalid, .exportSourceInvalid),
            (.ExportFormatLimit, .exportFormatLimit),
            (.Cancelled, .cancelled),
            (.InsufficientSpace(required: 20, available: 1), .insufficientSpace(required: 20, available: 1)),
            (.StaleGeneration(planned: 1, current: 2), .staleGeneration(planned: 1, current: 2)),
            (.InvalidArgument(message: "destination refused"), .invalidArgument("destination refused")),
            (.Internal(message: "generation failure"), .internal("generation failure"))
        ]
        for (raw, expected) in cases {
            let failure = CoreBridge.CollectionExportFailure(MeguriCoreBindings.CollectionExportFailure(
                cause: raw, publicationUncertain: false, destination: nil))
            #expect(failure.cause == expected && !failure.publicationUncertain && failure.destination == nil)
        }
    }
    @Test func boundedReceiptPreservesAdmissionAndSnapshotFacts() {
        let receipt = CoreBridge.CollectionExportMetadata(MeguriCoreBindings.CollectionExportMetadata(
            operationId: 42, sourceGeneration: 3, admissionSeq: 100, snapshotModifiedMs: 77,
            byteLength: 3_000_000_000, suggestedName: "Meguri-Collection.colpkg", sha256: Data(repeating: 7, count: 32), deckCounts: nil))
        #expect(receipt.operationID == 42 && receipt.sourceGeneration == 3 && receipt.admissionSequence == 100)
        #expect(receipt.snapshotModifiedMilliseconds == 77 && receipt.byteLength == 3_000_000_000)
        #expect(receipt.sha256.count == 32 && receipt.suggestedName.hasSuffix(".colpkg"))
        let failure = CoreBridge.CollectionExportFailure(MeguriCoreBindings.CollectionExportFailure(
            cause: .Cancelled, publicationUncertain: true, destination: "/synthetic/saved.colpkg"))
        #expect(failure.cause == .cancelled && failure.publicationUncertain)
    }
    @Test func deckCountsAndRefusalsSurviveGeneratedBindings() {
        let metadata = CoreBridge.CollectionExportMetadata(MeguriCoreBindings.CollectionExportMetadata(
            operationId: 1, sourceGeneration: 2, admissionSeq: 3, snapshotModifiedMs: 4,
            byteLength: 5, suggestedName: "Deck.apkg", sha256: Data(repeating: 0, count: 32),
            deckCounts: .init(notes: 0, cards: 2, media: 3)))
        #expect(metadata.deckCounts?.notes == 0 && metadata.deckCounts?.cards == 2 && metadata.deckCounts?.media == 3)
        let cases: [(MeguriCoreBindings.CoreError, CoreBridge.DeckExportRefusal)] = [
            (.NotFound(table: .decks, key: "42"), .missingDeck),
            (.ExportInvalidIds, .invalidIDs), (.ExportNeedsCheckDatabase, .needsCheckDatabase)
        ]
        for (cause, refusal) in cases {
            let receipt = CoreBridge.CollectionExportFailure(MeguriCoreBindings.CollectionExportFailure(
                cause: cause, publicationUncertain: false, destination: nil))
            #expect(receipt.deckRefusal == refusal && !receipt.publicationUncertain)
        }
    }
    @Test func collectionOnlyServiceRejectsDeckWithoutCallingCollectionPreparation() async {
        let service = CollectionOnlyExportService()
        do {
            _ = try await service.prepare(deck: DeckExportRequest(deckID: 42, options: .init(), expectedGeneration: 1),
                                          operation: CollectionExportOperation(), progress: { _ in })
            Issue.record("An unsupported deck request must be refused")
        } catch { #expect((error as? BridgeError) == .unsupported(feature: "Selected-deck export")) }
        #expect(service.calls.withLock { $0 } == 0)
        do {
            _ = try await service.prepare(deck: nil, operation: CollectionExportOperation(), progress: { _ in })
        } catch { #expect((error as? BridgeError) == .cancelled) }
        #expect(service.calls.withLock { $0 } == 1)
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

/// The original collection-only service surface remains a valid conformer.
private final class CollectionOnlyExportService: CollectionExportService {
    let calls = Mutex(0)
    func prepare(operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        calls.withLock { $0 += 1 }
        throw BridgeError.cancelled
    }
    func save(_ artifact: ExportedCollection, to: URL, operation: CollectionExportOperation) async throws {}
}
