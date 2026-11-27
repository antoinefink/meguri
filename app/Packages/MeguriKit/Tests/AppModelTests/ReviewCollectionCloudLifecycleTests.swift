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
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        do {
            await owner.start()
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            owner.entry.present(); owner.entry.inspect(source)
            await owner.entry.waitForCompletion()
            try #require(owner.entry.preview != nil)
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
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
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
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

    @Test(arguments: ["derived-store", "thread registry"], [false, true])
    func finalSuspendDeadlineResumesWithoutReplacingUIError(phase: String, existingError: Bool) async throws {
        try await withBackgroundSuspendOwner { owner, _ in
            if existingError {
                owner.finalBackgroundSuspend = { (_: UInt32) async throws(BridgeError) -> SuspendOutcome in
                    throw .unavailable(reason: "Synthetic unrelated suspend failure")
                }
                owner.setForeground(false); await owner.waitForLifecycle()
                try #require(owner.error?.contains("Synthetic unrelated suspend failure") == true)
                owner.setForeground(true); await owner.waitForLifecycle()
            }
            let previousError = owner.error
            var calls = 0
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                // Exercise real teardown and recovery; inject only the final diagnostic.
                _ = try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
                throw .suspendDeadline(phase: phase)
            }
            owner.setForeground(false); await owner.waitForLifecycle()
            #expect(calls == 1)
            #expect(owner.error == previousError)
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == previousError)
            #expect(owner.foreground && !owner.resuming)
            let deck = try await CoreDeckService().addDeck(name: "After deadline \(phase)")
            #expect(deck.id > 0)
        }
    }

    enum BackgroundSuspendBudget: CaseIterable { case unknown, laterEstimate, exhaustedEstimate, remaining, clamped }

    @Test(arguments: BackgroundSuspendBudget.allCases)
    func finalSuspendUsesDefaultOrRemainingWallBudgetAfterFlush(budget: BackgroundSuspendBudget) async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            let deadline: Int64? = switch budget {
            case .unknown, .laterEstimate, .exhaustedEstimate: nil
            case .remaining: Int64(Date().timeIntervalSince1970 * 1_000) + 60_000
            case .clamped: Int64.max
            }
            var flushed = false
            owner.didFlushLocally = { flushed = true; backend.events.append("local-flush") }
            var calls = 0
            var samples = 0
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                #expect(flushed)
                #expect(backend.events.contains(deadline == nil ? "expired" : "background"))
                if let deadline {
                    if budget == .clamped { #expect(timeout == UInt32(Int32.max)) }
                    else {
                        let remaining = Double(deadline) - Date().timeIntervalSince1970 * 1_000
                        #expect(timeout > 1_500)
                        #expect(abs(Double(timeout) - remaining) < 250)
                        #expect(timeout <= 60_000)
                    }
                } else if budget == .laterEstimate {
                    #expect(timeout == 30_000)
                } else {
                    #expect(timeout == 1_500)
                }
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            let lease = owner.beginBackgroundTask(deadlineMilliseconds: deadline, currentBudgetMilliseconds: {
                samples += 1
                #expect(flushed)
                #expect(backend.events.contains(deadline == nil ? "expired" : "background"))
                switch budget {
                case .laterEstimate: return 30_000
                case .exhaustedEstimate: return 0
                default: return nil
                }
            })
            try #require(lease != nil)
            await owner.waitForLifecycle()
            #expect(samples == (deadline == nil ? 1 : 0))
            #expect(calls == (budget == .exhaustedEstimate ? 0 : 1) && owner.error == nil)
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil && !owner.resuming)
            #expect(try await CoreDeckService().addDeck(name: "After budgeted suspend").id > 0)
        }
    }

    enum BackgroundSuspendSkip: CaseIterable { case past, expired, foreground, superseded }

    @Test(arguments: BackgroundSuspendSkip.allCases)
    func finalSuspendSkipsInvalidLeaseAfterLocalFlush(reason: BackgroundSuspendSkip) async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            var calls = 0
            var flushes = 0
            var token: UInt64?
            let deadline = Int64(Date().timeIntervalSince1970 * 1_000) + 60_000
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                calls += 1
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            owner.didFlushLocally = {
                flushes += 1
                // Only the first transition changes activity; its successor has its own flush.
                owner.didFlushLocally = nil
                switch reason {
                case .expired: if let token { owner.backgroundTaskExpired(token) }
                case .foreground: owner.setForeground(true)
                case .superseded:
                    owner.setForeground(true)
                    _ = owner.beginBackgroundTask(deadlineMilliseconds: 1)
                case .past: break
                }
            }
            token = try #require(owner.beginBackgroundTask(deadlineMilliseconds:
                reason == .past ? 1 : deadline))
            // A callback may queue a foreground/successor transition. Join that too.
            await owner.waitForLifecycle(); await owner.waitForLifecycle()
            #expect(flushes == 1)
            #expect(calls == 0 && owner.error == nil)
            #expect(backend.events.contains("expired") || backend.events.contains("background"))
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil && !owner.resuming)
            #expect(try await CoreDeckService().addDeck(name: "After skipped suspend").id > 0)
        }
    }

    @Test func directBackgroundAfterUnknownExpiredLeaseUsesDefaultBudget() async throws {
        try await withBackgroundSuspendOwner { owner, backend in
            var timeouts: [UInt32] = []
            var token: UInt64?
            owner.didFlushLocally = {
                owner.didFlushLocally = nil
                if let token { owner.backgroundTaskExpired(token) }
            }
            owner.finalBackgroundSuspend = { (timeout: UInt32) async throws(BridgeError) -> SuspendOutcome in
                timeouts.append(timeout)
                return try await CoreCollectionService().suspend(timeoutMilliseconds: timeout)
            }
            token = try #require(owner.beginBackgroundTask(deadlineMilliseconds: nil))
            await owner.waitForLifecycle()
            #expect(timeouts.isEmpty)
            #expect(backend.events.contains("expired"))
            owner.setForeground(true); await owner.waitForLifecycle()
            owner.setForeground(false); await owner.waitForLifecycle()
            #expect(timeouts == [1_500])
            owner.setForeground(true); await owner.waitForLifecycle()
            #expect(owner.error == nil)
            #expect(try await CoreDeckService().addDeck(name: "After direct suspend").id > 0)
        }
    }

    /// These tests extend the serialized real-Core startup suite. Each owner is closed
    /// before its synthetic directory (and sibling backups) are removed, even on failure.
    private func withBackgroundSuspendOwner(
        _ body: (ReviewCollection, SyncOwnerFake) async throws -> Void
    ) async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let directory = root.appending(path: "collection")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try files.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        // Deliberately far from wall time: OS budgets must not use the study clock.
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        do {
            await owner.start()
            try #require(owner.state == .ready)
            await sync.waitForInputs()
            try await body(owner, backend)
            #expect(await owner.close())
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
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
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
