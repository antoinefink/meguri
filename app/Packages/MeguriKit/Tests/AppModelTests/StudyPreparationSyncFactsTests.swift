@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// What study preparation reads from sync (`sched/app-coordination` F5.1,
/// `sched/fsrs-policy` A1.2): `.none` only for a state that is known.
@MainActor @Suite struct StudyPreparationSyncFactsTests {
    private typealias Facts = StudyPreparationSyncFacts
    private static let waiting = Facts(exchangeExpected: true, activity: .waiting)
    private static let exchanging = Facts(exchangeExpected: true, activity: .exchanging)
    private static func settled(_ ended: Int64?) -> Facts {
        .init(exchangeExpected: true, activity: .settled(endedMilliseconds: ended))
    }
    private static let routines: [MeguriRoutineState?] = [nil, .unreported, .exchanging, .idle, .pending, .waiting,
        .mediaBlocked, .authentication, .acceptanceUnknown, .fullSyncRequired, .network, .server, .cancelled, .failed]
    private static let phases: [MeguriProviderConnectionPhase] = [.unconfigured, .signedOut, .authenticating,
        .setupRequired, .cancelled, .blocked]


    /// A connected Meguri service with nothing in the way.
    private static func meguri(_ change: (inout StudyPreparationSyncInput) -> Void = { _ in }) -> StudyPreparationSyncInput {
        var input = StudyPreparationSyncInput(selection: .meguri, meguriPhase: .setupRequired, meguriReady: true,
            meguriSettled: true, meguriRoutine: .idle, meguriLastExchange: 77)
        change(&input)
        return input
    }
    /// Every combination of selection, readiness, load, phase, routine, last
    /// exchange state, without the switches that turn sync off.
    private static var everyConnection: [StudyPreparationSyncInput] {
        var all: [StudyPreparationSyncInput] = [.init(selection: nil), .init(selection: .localOnly)]
        for ready in [false, true] { for settled in [false, true] { for phase in phases { for routine in routines {
            for last in [Int64?.none, 77] {
                all.append(.init(selection: .meguri, meguriPhase: phase, meguriReady: ready, meguriSettled: settled,
                    meguriRoutine: routine, meguriLastExchange: last))
            }
        } } } }
        return all
    }

    // MARK: States in which no exchange comes by itself

    @Test func offAutomaticOffHoldAndNeedsUserExpectNoExchangeWhateverTheConnection() {
        for base in Self.everyConnection {
            for (needsUser, enabled, automatic, paused) in [(true, true, true, false), (false, false, true, false),
                (false, true, false, false), (false, true, true, true), (true, false, false, true)] {
                var input = base
                input.needsUser = needsUser; input.enabled = enabled; input.automatic = automatic; input.paused = paused
                #expect(Facts.mapping(input) == .none, "\(input)")
            }
        }
    }

    @Test func noServiceExpectsNoExchange() {
        #expect(Facts.mapping(.init(selection: nil)) == .none)
        #expect(Facts.mapping(.init(selection: .localOnly)) == .none)
    }

    // MARK: Meguri Sync

    @Test func meguriConnectionLoadInFlightIsWaitedForInEveryPhase() {
        // The cold launch: the phase still reads `unconfigured` while the
        // first load runs. A reload: readiness is cleared before the result.
        for phase in Self.phases { for routine in Self.routines { for last in [Int64?.none, 77] {
            let input = Self.meguri {
                $0.meguriReady = false; $0.meguriSettled = false
                $0.meguriPhase = phase; $0.meguriRoutine = routine; $0.meguriLastExchange = last
            }
            #expect(Facts.mapping(input) == Self.waiting, "\(input)")
        } } }
    }

    @Test func meguriNotReadyAfterTheLoadDependsOnThePhase() {
        for routine in Self.routines {
            for phase in [MeguriProviderConnectionPhase.unconfigured, .signedOut, .cancelled, .blocked] {
                // Not set up, signed out, a cancelled sign-in, a blocked connection.
                let input = Self.meguri { $0.meguriReady = false; $0.meguriPhase = phase; $0.meguriRoutine = routine }
                #expect(Facts.mapping(input) == .none, "\(input)")
            }
            for phase in [MeguriProviderConnectionPhase.authenticating, .setupRequired] {
                // A sign-in under way, or connected and not ready yet.
                let input = Self.meguri { $0.meguriReady = false; $0.meguriPhase = phase; $0.meguriRoutine = routine }
                #expect(Facts.mapping(input) == Self.waiting, "\(input)")
            }
        }
    }

    @Test func meguriReadyFollowsTheRoutineAndItsLastExchange() {
        for settled in [false, true] { for last in [Int64?.none, 77] {
            for routine in Self.routines {
                let input = Self.meguri { $0.meguriSettled = settled; $0.meguriRoutine = routine; $0.meguriLastExchange = last }
                let expected: Facts
                switch routine {
                case .exchanging: expected = Self.exchanging
                // A cancelled round (the app left the foreground mid-round) is still waiting.
                case nil, .unreported, .network, .waiting, .cancelled: expected = Self.waiting
                default: expected = Self.settled(last)
                }
                #expect(Facts.mapping(input) == expected, "\(input)")
            }
        } }
    }






}
