import CardRendering
@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// Warm pages and the preloaded first card (R1813; Antoine, 2026-11-21). The
/// warm-up only reads: a peek of the queue head and a render, never a start.
@MainActor
@Suite struct StudyWarmupTests {
    @Test func preloadReadsTheHeadAndPreparesItHiddenAndNothingElse() async throws {
        let fixture = try await Fixture()
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        #expect(fixture.peeks == [7])
        #expect(fixture.renders == [ReviewCoordinatorTests.head(1).items[0].cardId])
        let surface = try #require(fixture.surfaces.first)
        #expect(fixture.surfaces.count == 1)
        // Loaded and prepared beneath: never presented, never revealed, no audio.
        #expect(surface.calls == ["load", "prepareFirst"])
        #expect(surface.presented.isEmpty && surface.reveals == 0)
        #expect(fixture.warmup.prepared?.deckID == 7 && !fixture.warmup.stale)
        let taken = try #require(fixture.warmup.take(deckID: 7, resolver: fixture.resolver))
        #expect(taken.surface === surface && taken.preloaded)
        #expect(fixture.warmup.surface == nil)
    }

    @Test func aChangeHoldsThePreloadBackUntilTheHeadIsReadAgain() async throws {
        let fixture = try await Fixture()
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        fixture.warmup.invalidate()
        #expect(fixture.warmup.stale)
        // Study right after the change: the prepared card is not used.
        let taken = try #require(fixture.warmup.take(deckID: 7, resolver: fixture.resolver))
        #expect(!taken.preloaded)
        #expect(fixture.surfaces[0].calls.last == "discardFirst")
    }

    @Test func aChangedHeadIsPreparedAgainAndAnUnchangedOneKept() async throws {
        let fixture = try await Fixture()
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        // Same card, token and key: read again, kept, not rendered again.
        fixture.warmup.invalidate(); await fixture.warmup.settled()
        #expect(fixture.peeks.count == 2 && fixture.renders.count == 1 && !fixture.warmup.stale)
        // Another head: dropped, rendered and prepared again.
        fixture.head = ReviewCoordinatorTests.head(2).items[0]
        fixture.warmup.invalidate(); await fixture.warmup.settled()
        #expect(fixture.renders.count == 2 && fixture.warmup.prepared?.item.cardId == 2)
        #expect(Array(fixture.surfaces[0].calls.suffix(2)) == ["discardFirst", "prepareFirst"])
    }

    @Test func anotherDeckOrNoTargetIsNeverTheStudiedDecksPreload() async throws {
        let fixture = try await Fixture()
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        // The visible Study now targets another deck.
        fixture.warmup.setTarget(8, owner: fixture.owner)
        #expect(fixture.warmup.prepared == nil)
        let first = try #require(fixture.warmup.take(deckID: 7, resolver: fixture.resolver))
        #expect(!first.preloaded)
        // The screen disappeared.
        fixture.warmup.warm(); await fixture.warmup.settled()
        fixture.warmup.setTarget(7, owner: fixture.owner); await fixture.warmup.settled()
        #expect(fixture.warmup.prepared?.deckID == 7)
        fixture.warmup.clearTarget(fixture.owner)
        #expect(fixture.warmup.prepared == nil)
        let second = try #require(fixture.warmup.take(deckID: 7, resolver: fixture.resolver))
        #expect(!second.preloaded)
    }

    @Test func anEarlierScreenDisappearingAfterTheNextAppearedClearsNothing() async throws {
        let fixture = try await Fixture()
        let next = StudyWarmup.Owner(id: UUID())
        fixture.warmup.setTarget(7, owner: fixture.owner)
        fixture.warmup.setTarget(8, owner: next)
        fixture.warmup.clearTarget(fixture.owner)
        await fixture.warmup.settled()
        #expect(fixture.warmup.target == 8 && fixture.warmup.prepared?.deckID == 8)
    }

    @Test func releaseClosesThePagesAndAnotherBindingIsNotTaken() async throws {
        let fixture = try await Fixture()
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        fixture.warmup.release()
        #expect(fixture.surfaces[0].calls.last == "close" && fixture.warmup.surface == nil)
        #expect(fixture.warmup.take(deckID: 7, resolver: fixture.resolver) == nil)
        // Pages bound to another media root never serve a session.
        fixture.warmup.warm(); await fixture.warmup.settled()
        let other = try await MediaPathResolver.open(root: fixture.root, trackedNames: [])
        defer { Task { await other.close() } }
        #expect(fixture.warmup.take(deckID: 7, resolver: other) == nil)
        #expect(fixture.surfaces[1].calls.last == "close")
    }

    @Test func nothingWarmsWithoutAdmissionAndAnEmptyHeadPreparesNothing() async throws {
        let fixture = try await Fixture()
        fixture.admitted = false
        fixture.warmup.setTarget(7, owner: fixture.owner)
        await fixture.warmup.settled()
        #expect(fixture.surfaces.isEmpty && fixture.peeks.isEmpty)
        fixture.admitted = true
        fixture.empty = true
        fixture.warmup.warm(); await fixture.warmup.settled()
        #expect(fixture.peeks == [7] && fixture.renders.isEmpty && fixture.warmup.prepared == nil)
        #expect(fixture.surfaces[0].calls == ["load"])
    }

    // MARK: Support

    @MainActor final class Fixture {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let root: URL
        let resolver: MediaPathResolver
        let owner = StudyWarmup.Owner(id: UUID())
        var surfaces: [WarmSurface] = []
        var peeks: [Int64] = []
        var renders: [Int64] = []
        var head = ReviewCoordinatorTests.head(1).items[0]
        var empty = false
        var admitted = true
        var warmup: StudyWarmup!
        init() async throws {
            root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
            warmup = StudyWarmup { [unowned self] _ in
                let surface = WarmSurface(); surfaces.append(surface); return surface
            }
            warmup.settleDelay = .zero
            warmup.admitted = { [unowned self] in admitted }
            warmup.resolver = { [unowned self] in resolver }
            warmup.peek = { [unowned self] deck in
                peeks.append(deck)
                return empty ? .empty : .card(item: head)
            }
            warmup.render = { [unowned self] id in
                renders.append(id)
                return CardPayload(cardID: id, question: "Q\(id)", answer: "A\(id)")
            }
        }
        isolated deinit {
            warmup.release()
            try? FileManager.default.removeItem(at: directory)
        }
    }

    final class WarmSurface: ReviewCoordinatorTests.Surface {}
}
