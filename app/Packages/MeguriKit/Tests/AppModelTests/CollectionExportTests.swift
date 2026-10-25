@testable import CoreBridge
import Foundation
import Testing
import Synchronization
@testable import AppModel

private struct RefusingExportService: CollectionExportService {
    let cause: BridgeError
    var prepareReal = false
    var uncertain = false
    func prepare(operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        if prepareReal { return try await CoreCollectionExportService().prepare(operation: operation, progress: progress) }
        if uncertain { throw ExportDestinationCompletionUncertain() }
        throw CollectionExportFailure(cause: cause, destination: URL(fileURLWithPath: "/synthetic/do-not-display"))
    }
    func save(_ artifact: ExportedCollection, to: URL, operation: CollectionExportOperation) async throws {
        if uncertain { throw ExportDestinationCompletionUncertain() }
        throw CollectionExportFailure(cause: cause, destination: to)
    }
}

extension ReviewCollectionStartupTests {
    @Test func exportWithoutStudyAdmissionRetriesDestinationAndReimportsCapturedReview() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion()
        try #require(owner.state == .ready)
        let coordinator = owner.collectionExport
        do {
            // Export availability is separate from FSRS policy and study admission.
            #expect(coordinator.canPrepare)
            coordinator.present(); coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready, "\(coordinator.message ?? "missing Ready")")
            let artifact = try #require(coordinator.artifact)
            #expect(artifact.metadata.sourceGeneration == owner.generation && artifact.metadata.byteLength > 0)
            owner.navigation.adoptLaunchLayout(.compact); owner.navigation.adoptLaunchLayout(.sidebar)
            #expect(owner.collectionExport === coordinator)
            await coordinator.dismiss()
            #expect(coordinator.artifact === artifact)
            coordinator.present(); coordinator.chooseDestination(); await coordinator.waitForCompletion()
            try #require(coordinator.providerLease != nil && coordinator.destinationPresented)
            // Dismissal cannot prove the provider stopped reading.
            await coordinator.dismiss(); await coordinator.discard()
            #expect(coordinator.providerLease != nil && coordinator.artifact === artifact)
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .ready && coordinator.providerLease == nil)
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            coordinator.save(to: root.appending(path: "missing-parent/out.colpkg")); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .failed && coordinator.artifact === artifact && coordinator.canSave)
            let saved = root.appending(path: "saved.colpkg")
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            coordinator.save(to: saved); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .saved && files.fileExists(atPath: saved.path))
            let location = CoreBridge.CollectionLocation(collectionDirectory: configuration.collectionLocation.directory,
                backupsDirectory: configuration.collectionLocation.backupsDirectory,
                appGroupSnapshot: root.appending(path: "widget"))
            let receipt = try await CoreCollectionEntryService().inspect(source: saved, location: location,
                operation: CollectionEntryOperation(), progress: { _ in })
            #expect(receipt.facts.notes == 6 && receipt.facts.cards == 11 && receipt.facts.reviews == 6)
            try await receipt.discard()
            // Reuse the real RF2 replacement, with its safety backup.
            owner.entry.inspect(saved); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            try #require(owner.state == .ready)
            #expect(owner.generation != artifact.metadata.sourceGeneration)
            #expect(coordinator.artifact === artifact)
            #expect(coordinator.stage == .saved && !coordinator.canSave)
            await coordinator.discard()
            #expect(coordinator.artifact == nil && files.fileExists(atPath: saved.path))
            if owner.policy != .ok { await owner.enableFsrs() }
            let tree = try await CoreDeckService().treeWithCounts()
            func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
            let deck = try #require(flatten(tree.nodes).first { !$0.filtered && $0.counts.new + $0.counts.review + $0.counts.learning > 0 })
            guard case .started(let session, _, _, _) = try await Core.startSession(deckID: deck.id) else {
                Issue.record("Exported package must remain reviewable"); _ = await owner.close(); return
            }
            let head = try await Core.sessionHead(sessionID: session, count: 1)
            let item = try #require(head.items.first)
            _ = try await Core.answerSession(sessionID: session, cardID: item.cardId, rating: .easy,
                stateToken: item.stateToken, answeredAtMs: 1_894_701_600_000, elapsedMs: 123)
            try await Core.flushReviewForBackground()
            try await Core.endSession(sessionID: session)
            #expect(await owner.close())
        } catch { await coordinator.cancelAndDrain(); await coordinator.discard(); _ = await owner.close(); throw error }
    }
    @Test func filesPickerPreventsReplacementBeforeConfirmationAndPreservesLateSavedCompletion() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion()
        let coordinator = owner.collectionExport
        do {
            try #require(owner.state == .ready)
            coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready)
            let artifact = try #require(coordinator.artifact)
            let generation = owner.generation
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            let lease = try #require(coordinator.providerLease)
            try #require(coordinator.destinationPresented)
            let entryStage = owner.entry.stage
            #expect(!owner.entry.canPresent)
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            #expect(owner.entry.stage == entryStage)
            #expect(owner.generation == generation)
            #expect(coordinator.destinationPresented && coordinator.providerLease === lease)
            #expect(!(await owner.close()))
            // A delayed native copy callback still owns its source after dismissal.
            await coordinator.dismiss(); await coordinator.discard()
            #expect(coordinator.providerLease === lease)
            let saved = root.appending(path: "files-copy.colpkg")
            try files.copyItem(at: lease.url, to: saved)
            // Deterministic late notification after publication, before delegate delivery.
            coordinator.generationChanged()
            coordinator.nativeSaved(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .saved && coordinator.providerLease == nil)
            #expect(files.fileExists(atPath: saved.path) && coordinator.artifact === artifact)
            coordinator.generationChanged()
            #expect(coordinator.stage == .saved, "replacement cannot retrospectively relabel a completed copy")
            await coordinator.discard()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    @Test(arguments: [0, 1, 2])
    func nativeCleanupFailureRetainsRetryAndSavedPublication(completion: Int) async throws {
        let saved = completion != 0
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion()
        let failNext = Mutex(true)
        let coordinator = CollectionExportCoordinator(owner: owner, service: CoreCollectionExportService(), finishLease: { lease in
            if failNext.withLock({ fail in let previous = fail; fail = false; return previous }) {
                throw BridgeError.unavailable(reason: "synthetic completion interruption")
            }
            try await lease.finish()
        })
        do {
            try #require(owner.state == .ready)
            coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready)
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            let lease = try #require(coordinator.providerLease)
            let destination = root.appending(path: "saved.colpkg")
            if completion == 1 {
                // The app-controlled save publishes bytes before cleanup is attempted.
                coordinator.save(to: destination)
            } else if completion == 2 {
                try files.copyItem(at: lease.url, to: destination)
                coordinator.nativeSaved()
            } else {
                coordinator.nativeCancelled()
            }
            await coordinator.waitForCompletion()
            #expect(coordinator.providerLease === lease && coordinator.canRetryCompletion)
            #expect(!coordinator.canSave && !coordinator.canPrepare)
            #expect(coordinator.message?.contains("Retry cleanup") == true)
            #expect(coordinator.stage == (saved ? .saved : .ready))
            #expect(files.fileExists(atPath: destination.path) == saved)
            coordinator.chooseDestination()
            #expect(!coordinator.destinationPresented && coordinator.providerLease === lease)
            await coordinator.discard()
            #expect(coordinator.artifact != nil && coordinator.providerLease === lease)
            coordinator.retryCompletion(); await coordinator.waitForCompletion()
            #expect(coordinator.providerLease == nil && !coordinator.canRetryCompletion)
            #expect(coordinator.stage == (saved ? .saved : .ready) && coordinator.message == nil)
            #expect(files.fileExists(atPath: destination.path) == saved)
            await coordinator.discard()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            coordinator.retryCompletion(); await coordinator.waitForCompletion()
            await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    @Test func exportReportGivesBoundedSourceActionsAndPreservesOtherFailures() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion()
        try #require(owner.state == .ready)
        let generic = "The collection could not be exported. Check available space and collection access, then try again. Unused prepared output from an earlier launch is recovered when you prepare again."
        let cases: [(BridgeError, String)] = [
            (.exportSourceChanged, "The media source changed during export. Start a fresh export after changes finish."),
            (.exportSourceInvalid, "A media item is unsafe or unreadable. Correct its name, type or read access, then start a fresh export."),
            (.exportFormatLimit, "The collection or media exceeds the package format limits. Reduce the size or number of media files, then start a fresh export."),
            (.cancelled, generic), (.insufficientSpace(required: 20, available: 1), generic),
            (.staleGeneration(planned: 1, current: 2), generic),
            (.internal("private path /synthetic/do-not-display and OS diagnostic"), generic),
            (.invalidArgument("destination diagnostic"), generic)
        ]
        for (cause, expected) in cases {
            let coordinator = CollectionExportCoordinator(owner: owner, service: RefusingExportService(cause: cause))
            coordinator.prepare(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .failed && coordinator.message == expected)
            #expect(coordinator.artifact == nil && !coordinator.publicationUncertain)
        }
        // Actual report() precedence: destination and uncertain publication stay
        // actionable as destination problems even when a source cause is present.
        for uncertain in [false, true] {
            let coordinator = CollectionExportCoordinator(owner: owner, service: RefusingExportService(
                cause: .exportSourceInvalid, prepareReal: true, uncertain: uncertain))
            do {
                coordinator.prepare(); await coordinator.waitForCompletion()
                try #require(coordinator.stage == .ready)
                coordinator.chooseDestination(); await coordinator.waitForCompletion()
                coordinator.save(to: root.appending(path: "destination.colpkg")); await coordinator.waitForCompletion()
                #expect(coordinator.stage == .failed && coordinator.artifact != nil)
                #expect(coordinator.publicationUncertain == uncertain)
                #expect(coordinator.message == (uncertain
                    ? "The new file may already be saved. Check the destination before trying again. The completed export is retained."
                    : "The file could not be saved. The completed export is retained; choose another destination and try again."))
                await coordinator.discard()
            } catch {
                coordinator.nativeCancelled(); await coordinator.waitForCompletion()
                await coordinator.discard(); _ = await owner.close(); throw error
            }
        }
        #expect(await owner.close())
    }
    @Test func noCollectionExplainsExportWithoutStartingWork() async throws {
        var configuration = LaunchConfiguration()
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        let coordinator = owner.collectionExport
        #expect(!coordinator.canPrepare && coordinator.availabilityMessage != nil)
        coordinator.prepare(); #expect(coordinator.stage == .idle)
        coordinator.generationChanged()
        #expect(coordinator.stage == .idle && coordinator.artifact == nil)
        #expect(await owner.close())
    }
}
