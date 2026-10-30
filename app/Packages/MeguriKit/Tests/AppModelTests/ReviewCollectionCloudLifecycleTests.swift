import Foundation
import Testing
@testable import AppModel
import CoreBridge

extension ReviewCollectionStartupTests {
    @Test func missingCloudConfigurationKeepsRealLocalChangesAndReviewsUsable() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let sync = SyncCoordinator(directory: root, configuration: nil, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        do {
            await owner.start()
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            owner.entry.present(); owner.entry.inspect(source)
            await owner.entry.waitForCompletion()
            try #require(owner.entry.preview != nil)
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            try #require(owner.state == .ready)
            await sync.waitForInputs()
            await sync.waitForReplacement()
            #expect(sync.generation == owner.generation)
            #expect(sync.setupState == .configurationUnavailable && !sync.operationFailed)
            #expect(owner.syncNowAvailability() == .unavailable(.iCloudUnavailable))
            _ = try await CoreDeckService().addDeck(name: "Local without iCloud setup")
            if owner.policy != .ok { await owner.enableFsrs() }
            func flattened(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flattened($0.children) } }
            let deck = try #require(flattened(owner.decks).first {
                !$0.filtered && ($0.counts.new + $0.counts.review + $0.counts.learning) > 0
            })
            await owner.startStudy(deckID: deck.id)
            let review = try #require(owner.review)
            try #require(review.phase == .question)
            review.reveal(); await review.settled()
            try #require(review.phase == .answer)
            review.rate(.easy); await review.settled()
            try #require(review.record.answers.count == 1)
            await owner.finishReview()
            #expect(owner.previousRecord?.answers.count == 1)
            #expect(owner.previousRecord?.answers.allSatisfy(\.durable) == true)
            await sync.waitForInputs()
            #expect(sync.setupState == .configurationUnavailable && !sync.operationFailed)
            #expect(sync.status == nil && sync.connectionService == nil)
            #expect(owner.state == .ready && owner.error == nil)
            #expect(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }

    @Test func offlineOwnerResumesFlushesAndDrainsBeforeOriginalCoreClose() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        try files.createDirectory(at: root, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try files.copyItem(at: source, to: root.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        let backend = SyncOwnerFake()
        let service = CoreCollectionService()
        var configured = false
        let sync = SyncCoordinator(bootstrap: {
            let phase: CollectionLifecycle = try await service.lifecycle()
            #expect(phase == .noCollection)
            configured = true
        }, makeBackend: { _ in
            #expect(configured)
            guard case .ready = try await service.lifecycle() else {
                Issue.record("Cloud attached before the original Host was retained")
                return backend
            }
            return backend
        }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        var localFlushObserved = false
        owner.didFlushLocally = { localFlushObserved = true; backend.events.append("local-flush") }
        backend.onClose = {
            // This query still resolves the ORIGINAL Host at native drain.
            let phase = try await service.lifecycle()
            #expect(phase == .ready(generation: owner.generation))
            backend.events.append("native-drained")
        }
        await owner.start()
        do {
            try #require(owner.state == .ready)
            sync.receive(.network(reachable: false))
            await sync.waitForInputs()
            #expect(!owner.decks.isEmpty)
            let localCount = backend.localRefreshCount
            _ = try await CoreDeckService().addDeck(name: "Lifecycle local change")
            await backend.waitForLocalRefresh(after: localCount)
            #expect(backend.refreshCauses.contains(.local))
            let lease = try #require(owner.beginBackgroundTask(deadlineMilliseconds: 123_000))
            await owner.waitForLifecycle()
            #expect(localFlushObserved)
            let flushed = try #require(backend.events.firstIndex(of: "local-flush"))
            let cloud = try #require(backend.events.firstIndex(of: "background"))
            #expect(flushed < cloud)
            owner.backgroundTaskExpired(lease)
            await sync.waitForInputs()
            owner.setForeground(true)
            await owner.waitForLifecycle()
            #expect(backend.events.contains("foreground"))
            #expect(backend.events.contains("expired"))
            // A completed old assertion cannot expire the new foreground.
            let expiries = backend.events.filter { $0 == "expired" }.count
            owner.backgroundTaskExpired(lease)
            await sync.waitForInputs()
            #expect(backend.events.filter { $0 == "expired" }.count == expiries)
            #expect(await owner.close())
            #expect(backend.closed && backend.closeCount == 1)
            let phase: CollectionLifecycle = try await service.lifecycle()
            #expect(phase == .noCollection)
            #expect(await owner.close())
            #expect(backend.closeCount == 1)
        } catch { _ = await owner.close(); throw error }
    }

    @Test func entryPreparationConfiguresBeforeEmptyRetainedHostAndRelease() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        let backend = SyncOwnerFake()
        var configured = false
        let sync = SyncCoordinator(bootstrap: { configured = true }, makeBackend: { _ in backend }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        let location = try await owner.prepareEntry()
        #expect(configured)
        #expect(backend.accountReads == 0)
        #expect(sync.setupState == .prepared)
        // Inspection retains a real empty Host without touching frozen server
        // work. Cloud must stay unattached until completeOpenedHost sees Ready.
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        _ = try await CoreCollectionEntryService().inspect(source: source, location: location,
            operation: CollectionEntryOperation(), progress: { _ in })
        await owner.releaseEmptyManualHost()
        #expect(!backend.closed && backend.accountReads == 0)
        #expect(await owner.close())
    }
}
