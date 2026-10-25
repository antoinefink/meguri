import CardRendering
import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct ReviewMediaBindingTests {
    @Test func journalArrivalUpdatesResolverAndFailedOrStaleBindingClosesIt() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = CollectionLocation(directory: directory)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        owner.loadMediaIndex = { MediaNameSnapshot(generation: 0, root: root, names: []) }
        let resolver = try #require(await owner.mediaResolverForReview())
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/mixed.png") }
        try Data("asset".utf8).write(to: root.appending(path: "MiXeD.PNG"))
        owner.loadMediaIndex = {
            MediaNameSnapshot(generation: 0, root: root, names: [TrackedMediaName(name: "MiXeD.PNG", caseKey: "mixed.png")])
        }
        let refreshed = try #require(await owner.mediaResolverForReview())
        #expect(refreshed === resolver)
        let file = try await resolver.openURLPath("/mixed.png")
        #expect(try await file.read(offset: 0, count: 16) == Data("asset".utf8))
        await file.close()
        owner.loadMediaIndex = { MediaNameSnapshot(generation: 1, root: root, names: []) }
        #expect(await owner.mediaResolverForReview() == nil)
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/MiXeD.PNG") }
        owner.loadMediaIndex = { throw BridgeError.internal("journal read failed") }
        #expect(await owner.mediaResolverForReview() == nil)
        #expect(owner.error?.contains("journal read failed") == true)
    }

    @Test func bindingRequiresBothGenerationAndCollectionRoot() {
        let root = URL(fileURLWithPath: "/tmp/collection/collection.media")
        #expect(ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 4, root: root, names: []), generation: 4, root: root))
        #expect(!ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 3, root: root, names: []), generation: 4, root: root))
        #expect(!ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 4, root: root.deletingLastPathComponent(), names: []), generation: 4, root: root))
    }

    @Test func ownerArrivalReachesActiveSurfaceAfterIndexUpdateWithoutPresentation() async throws {
        let binding = try Binding()
        await binding.start()
        let resolver = try #require(await binding.owner.mediaResolverForReview())
        binding.review.model.reveal(); await binding.review.model.settled()
        let calls = binding.review.surface.calls
        try Data("generated media".utf8).write(to: binding.root.appending(path: "MiXeD.PNG"))
        let snapshot = binding.snapshot(["MiXeD.PNG"]), surface = binding.review.surface
        binding.owner.loadMediaIndex = { snapshot }
        binding.review.model.retryMedia = { names in
            // The case fallback can only succeed after the owner's index update.
            let file = try await resolver.openURLPath("/mixed.png")
            #expect(try await file.read(offset: 0, count: 32) == Data("generated media".utf8))
            await file.close()
            surface.arrivals.append(names)
            return true
        }
        binding.owner.scheduleMediaRefresh(names: ["MiXeD.PNG", "MiXeD.PNG"], generation: 0)
        _ = await binding.owner.mediaResolverForReview()
        await binding.review.model.settled()
        #expect(binding.review.surface.arrivals == [["MiXeD.PNG"]])
        #expect(binding.review.surface.calls == calls)
        #expect(binding.review.surface.presented == [1])
        #expect(binding.review.model.phase == .answer)
        #expect(binding.review.client.headReads == 0)
        #expect(await binding.review.model.end())
    }

    @Test func pausedArrivalsWaitForResumeAndPresentationReadiness() async throws {
        let binding = try Binding()
        await binding.start()
        try await binding.review.model.pause()
        binding.owner.scheduleMediaRefresh(names: ["late.png"], generation: 0)
        _ = await binding.owner.mediaResolverForReview()
        #expect(binding.review.surface.arrivals.isEmpty)
        var ready = false
        var attempts = 0
        let surface = binding.review.surface
        binding.review.model.retryMedia = { names in
            attempts += 1
            guard ready else { return false }
            surface.arrivals.append(names)
            return true
        }
        try await binding.review.model.resume()
        await binding.review.model.settled()
        #expect(attempts == 1 && binding.review.surface.arrivals.isEmpty)
        ready = true
        binding.review.model.mediaPresentationReady()
        await binding.review.model.settled()
        #expect(binding.review.surface.arrivals == [["late.png"]])
        #expect(binding.review.surface.presented == [1])
        #expect(await binding.review.model.end())
    }

    @Test func overlappingIndexRefreshCoalescesAllNamesAndRejectsOlderSnapshot() async throws {
        let binding = try Binding()
        await binding.start()
        let gate = Gate()
        var reads = 0
        let older = binding.snapshot([]), latest = binding.snapshot(["first.png", "second.css"])
        let surface = binding.review.surface
        binding.owner.loadMediaIndex = {
            reads += 1
            if reads == 1 {
                await gate.wait()
                return older
            }
            #expect(surface.arrivals.isEmpty)
            return latest
        }
        binding.owner.scheduleMediaRefresh(names: ["first.png"], generation: 0)
        let refreshing = Task { await binding.owner.mediaResolverForReview() }
        await gate.requested()
        binding.owner.scheduleMediaRefresh(names: ["second.css", "first.png"], generation: 0)
        gate.release()
        _ = await refreshing.value
        await binding.review.model.settled()
        #expect(reads == 2)
        #expect(binding.review.surface.arrivals == [["first.png", "second.css"]])
        #expect(binding.review.surface.presented == [1])
        #expect(await binding.review.model.end())
    }

    @Test func replacedGenerationDropsPendingNamesAndOldSnapshot() async throws {
        let binding = try Binding()
        await binding.start()
        let oldResolver = try #require(await binding.owner.mediaResolverForReview())
        let gate = Gate()
        var reads = 0
        let older = binding.snapshot(["old.png"]), latest = binding.snapshot([], generation: 1)
        binding.owner.loadMediaIndex = {
            reads += 1
            if reads == 1 { await gate.wait(); return older }
            return latest
        }
        binding.owner.scheduleMediaRefresh(names: ["old.png"], generation: 0)
        let refreshing = Task { await binding.owner.mediaResolverForReview() }
        await gate.requested()
        binding.owner.receiveLifecycle(.ready(generation: 1))
        binding.review.model.generationEnded()
        let next = ReviewCoordinatorTests.Fixture()
        binding.owner.presentReview(next.model)
        await next.model.start()
        // A delayed event from the old collection cannot target this review.
        binding.owner.scheduleMediaRefresh(names: ["stale.png"], generation: 0)
        gate.release()
        _ = await refreshing.value
        await next.model.settled()
        #expect(binding.review.surface.arrivals.isEmpty && next.surface.arrivals.isEmpty)
        await #expect(throws: (any Error).self) { try await oldResolver.openURLPath("/old.png") }
        #expect(await next.model.end())
    }

    @Test func closeDuringIndexRefreshCannotDeliverOrReopenResolver() async throws {
        let binding = try Binding()
        await binding.start()
        let gate = Gate()
        let snapshot = binding.snapshot(["late.png"])
        binding.owner.loadMediaIndex = { await gate.wait(); return snapshot }
        binding.owner.scheduleMediaRefresh(names: ["late.png"], generation: 0)
        let refreshing = Task { await binding.owner.mediaResolverForReview() }
        await gate.requested()
        // No core host is open in this focused owner fixture.
        #expect(await binding.owner.close())
        gate.release()
        #expect(await refreshing.value == nil)
        #expect(binding.review.surface.arrivals.isEmpty)
        #expect(binding.review.model.phase == .ended)
    }

    @Test func retryFailureUsesExistingErrorRecoveryWithoutRawDiagnostic() async throws {
        let fixture = ReviewCoordinatorTests.Fixture()
        await fixture.model.start()
        var attempts = 0
        fixture.model.retryMedia = { _ in
            attempts += 1
            throw BridgeError.internal("private synthetic path must not appear")
        }
        fixture.model.mediaArrived(["late.png"])
        await fixture.model.settled()
        #expect(attempts == 1)
        #expect(fixture.model.phase == .error)
        #expect(fixture.model.notice == "Couldn’t refresh card media. Try again, or return to Home.")
        #expect(fixture.surface.presented == [1])
        fixture.model.retry(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && attempts == 1)
        #expect(await fixture.model.end())
    }

    @Test func pauseDuringRetryRetainsNamesUntilResume() async throws {
        let fixture = ReviewCoordinatorTests.Fixture()
        await fixture.model.start()
        let gate = Gate(), surface = fixture.surface
        var attempts = 0
        fixture.model.retryMedia = { names in
            attempts += 1
            if attempts == 1 { await gate.wait(); throw CardPage.Failure.superseded }
            surface.arrivals.append(names)
            return true
        }
        fixture.model.mediaArrived(["first.png"])
        await gate.requested()
        fixture.model.beginPause()
        fixture.model.mediaArrived(["second.css", "first.png"])
        gate.release()
        await fixture.model.settled()
        #expect(fixture.model.paused && fixture.model.notice == nil && surface.arrivals.isEmpty)
        try await fixture.model.pause()
        try await fixture.model.resume()
        await fixture.model.settled()
        #expect(surface.arrivals == [["first.png", "second.css"]])
        #expect(attempts == 2 && surface.presented == [1])
        #expect(await fixture.model.end())
    }

    private final class Binding {
        let review = ReviewCoordinatorTests.Fixture()
        let directory: URL
        let root: URL
        let owner: ReviewCollection
        init() throws {
            directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = CollectionLocation(directory: directory)
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
            let root = root
            owner.loadMediaIndex = { MediaNameSnapshot(generation: 0, root: root, names: []) }
        }
        func snapshot(_ names: [String], generation: UInt64 = 0) -> MediaNameSnapshot {
            MediaNameSnapshot(generation: generation, root: root,
                names: names.map { TrackedMediaName(name: $0, caseKey: $0.lowercased()) })
        }
        func start() async {
            owner.presentReview(review.model)
            await review.model.start()
        }
        isolated deinit { try? FileManager.default.removeItem(at: directory) }
    }

    private final class Gate {
        private var pending: CheckedContinuation<Void, Never>?
        private var observer: CheckedContinuation<Void, Never>?
        private var waiting = false
        func wait() async {
            await withCheckedContinuation {
                pending = $0; waiting = true
                observer?.resume(); observer = nil
            }
        }
        func requested() async {
            if waiting { return }
            await withCheckedContinuation { observer = $0 }
        }
        func release() { pending?.resume(); pending = nil }
    }
}
