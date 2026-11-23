import CoreBridge
import Foundation
import Testing

@testable import Stats

/// G9.2's `homeIdle` (once Home has been idle for 2 s after each open) and G9.5's release on a
/// memory warning.
@MainActor
@Suite struct StatsCacheSignalsTests {
    let service = FakeStatsService()
    let timers = ManualTimers()

    var idleCalls: Int { service.state.withLock { $0.idle } }
    var releases: Int { service.state.withLock { $0.released } }

    @Test func homeIdleTwoSecondsAfterAnOpenOncePerOpen() async {
        let signals = StatsCacheSignals(service: service, timers: timers, watchesMemory: false)
        // Idle before any open: nothing.
        signals.homeIdle(true)
        timers.advance(5)
        await settle()
        #expect(idleCalls == 0)
        signals.collectionOpened()
        timers.advance(1.9)
        await settle()
        #expect(idleCalls == 0)
        timers.advance(0.1)
        await settle { idleCalls == 1 }
        #expect(idleCalls == 1)
        // Once per open.
        signals.homeIdle(false)
        signals.homeIdle(true)
        timers.advance(5)
        await settle()
        #expect(idleCalls == 1)
        // A busy Home restarts the two seconds.
        signals.collectionOpened()
        timers.advance(1)
        signals.homeIdle(false)
        timers.advance(5)
        signals.homeIdle(true)
        timers.advance(1)
        await settle()
        #expect(idleCalls == 1)
        timers.advance(1)
        await settle { idleCalls == 2 }
        #expect(idleCalls == 2)
        // Closed before the two seconds: nothing.
        signals.collectionOpened()
        signals.collectionClosed()
        timers.advance(5)
        await settle()
        #expect(idleCalls == 2)
    }

    @Test func aMemoryWarningReleasesTheCachesAndTrimsTheKeptResults() async {
        let signals = StatsCacheSignals(service: service, timers: timers, watchesMemory: false)
        var trimmed = 0
        signals.onMemoryWarning.append { trimmed += 1 }
        signals.memoryWarning()
        // A second warning while the release runs adds no second release.
        signals.memoryWarning()
        await settle { releases == 1 && !signals.releasing }
        #expect(releases == 1 && trimmed == 2)
        signals.memoryWarning()
        await settle { releases == 2 }
        #expect(releases == 2 && trimmed == 3)
    }

    @Test func aMemoryWarningKeepsOnlyTheShownScope() async throws {
        let kept = StatsKeptResults(directory: nil)
        kept.open(collection: "a")
        kept.keep(Fixture.set(scope: .allDecks))
        kept.keep(Fixture.set(scope: .deck(1)))
        kept.show(.deck(1), by: ObjectIdentifier(kept))
        kept.trim()
        #expect(kept.sets(for: .allDecks).isEmpty)
        #expect(kept.sets(for: .deck(1)).count == 1)
    }
}
