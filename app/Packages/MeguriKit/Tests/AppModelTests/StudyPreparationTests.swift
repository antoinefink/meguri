@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// `sched/fsrs-policy` A1 and `sched/app-coordination` F1, F3 to F5, against a
/// scripted core (T1).
@MainActor @Suite struct StudyPreparationTests {
    @MainActor final class Flag { var value = false }
    @MainActor final class FakeCore {
        var status: FsrsPolicyStatus = .needsEnable
        var statusFails = false
        var sessionOpen = false
        var facts = StudyPreparationSyncFacts.none
        var now: Int64 = 1_000_000
        var reads = 0
        var starts = 0
        var cancels = 0
        var commits = 0
        var computed: UInt64? = 3
        /// The result of the next jobs, oldest first; a missing entry commits.
        var results: [Result<Void, any Error>] = []
        /// When set, a started job ends only when `release()` is called.
        var holds = false
        var records: [FieldDiagnosticObservation] = []
        private var held: [CheckedContinuation<Void, Never>] = []

        func release() { let pending = held; held.removeAll(); for item in pending { item.resume() } }

        var environment: StudyPreparation.Environment {
            .init(
                status: { [unowned self] in
                    reads += 1
                    if statusFails { throw BridgeError.unavailable(reason: "synthetic") }
                    return status
                },
                start: { [unowned self] in
                    starts += 1
                    let result: Result<Void, any Error> = results.isEmpty ? .success(()) : results.removeFirst()
                    let cancelled = Flag()
                    return .init(cancel: { [unowned self] in cancels += 1; cancelled.value = true; release() }, finish: { [unowned self] in
                        if holds, !cancelled.value { await withCheckedContinuation { held.append($0) } }
                        if cancelled.value { throw BridgeError.cancelled }
                        try result.get()
                        status = .ok
                        return computed
                    })
                },
                sessionOpen: { [unowned self] in sessionOpen },
                syncFacts: { [unowned self] in facts },
                nowMilliseconds: { [unowned self] in now },
                record: { [unowned self] in records.append($0) },
                committed: { [unowned self] in commits += 1 })
        }
    }

    private func make(_ configure: (FakeCore) -> Void = { _ in }) -> (StudyPreparation, FakeCore) {
        let core = FakeCore()
        configure(core)
        let preparation = StudyPreparation()
        preparation.environment = core.environment
        return (preparation, core)
    }
    /// Lets a started job reach its first suspension.
    private func settle() async { for _ in 0..<12 { await Task.yield() } }
    private static let connected = StudyPreparationSyncFacts(exchangeExpected: true, activity: .waiting)
    private static let exchanging = StudyPreparationSyncFacts(exchangeExpected: true, activity: .exchanging)
    private static func settled(_ ended: Int64? = nil) -> StudyPreparationSyncFacts {
        .init(exchangeExpected: true, activity: .settled(endedMilliseconds: ended))
    }

    // MARK: At open (A1.2, F5.1)

    @Test(arguments: [FsrsPolicyStatus.needsEnable, .needsReEnable])
    func openWithNoExchangeExpectedStartsAtOnceAndAsksNothing(cause: FsrsPolicyStatus) async {
        let (preparation, core) = make { $0.status = cause; $0.holds = true }
        await preparation.collectionOpened()
        await settle()
        // The open returned while the job still runs: opening never waits for it.
        #expect(preparation.phase == .running && core.starts == 1)
        core.release()
        await preparation.waitForJob()
        #expect(preparation.status == .ok && preparation.phase == .idle && core.commits == 1)
        #expect(core.records == [.init(component: .scheduling, operation: cause == .needsReEnable ? .recover : .activate,
            stage: .activation, outcome: .succeeded, count: 3)])
    }

    @Test func openWithStatusOkRunsNoJobAndWritesNoRecord() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(core.reads == 1 && core.starts == 0 && core.records.isEmpty && preparation.phase == .idle)
        #expect(await preparation.prepare(for: .study))
        #expect(core.reads == 1 && core.starts == 0)
    }

    @Test(arguments: [StudyPreparationSyncFacts.Activity.waiting, .exchanging, .settled(endedMilliseconds: nil),
                      .settled(endedMilliseconds: 999_000)])
    func openWithConnectedServiceWaits(activity: StudyPreparationSyncFacts.Activity) async {
        // Syncing, offline, waiting for a retry, or settled on an exchange that ended before the open.
        let (preparation, core) = make { $0.facts = .init(exchangeExpected: true, activity: activity) }
        await preparation.collectionOpened()
        await preparation.syncChanged()
        #expect(preparation.phase == .waitingForExchange && core.starts == 0 && preparation.status == .needsEnable)
    }

    @Test func waitEndsWhenASuccessfulExchangeEnds() async {
        let (preparation, core) = make { $0.facts = Self.connected }
        await preparation.collectionOpened()
        core.facts = Self.exchanging
        await preparation.syncChanged()
        #expect(core.starts == 0)
        core.facts = Self.settled(1_000_500)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok)
        #expect(core.records.first?.stage == .activation)
    }

    @Test func waitEndsWhenAFailedExchangeEnds() async {
        // A failed exchange reports no new end instant; the exchange was seen running.
        let (preparation, core) = make { $0.facts = Self.exchanging }
        await preparation.collectionOpened()
        core.facts = Self.settled(nil)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok)
    }

    @Test func waitEndsOnANewerEndInstantEvenWhenTheExchangeWasNotSeenRunning() async {
        let (preparation, core) = make { $0.facts = Self.settled(999_000) }
        await preparation.collectionOpened()
        core.facts = Self.settled(1_000_001)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 1)
    }

    // The host opens some steps before the status is read (`hostOpened()`).

    @Test func exchangeThatEndedBetweenTheHostOpenAndTheStatusReadCounts() async {
        let (preparation, core) = make { $0.facts = Self.connected }
        preparation.hostOpened()
        // The whole round ran while the open finished its other steps.
        core.now = 1_000_400
        core.facts = Self.settled(1_000_300)
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok && core.records.first?.stage == .activation)
    }

    @Test func failedExchangeSeenBetweenTheHostOpenAndTheStatusReadCounts() async {
        // It reports no end instant: it counts because it was seen running.
        let (preparation, core) = make { $0.facts = Self.connected }
        preparation.hostOpened()
        core.facts = Self.exchanging
        await preparation.syncChanged()
        core.facts = Self.settled(nil)
        await preparation.syncChanged()
        #expect(core.reads == 0 && core.starts == 0)
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok)
    }

    @Test func exchangeStillRunningAtTheStatusReadIsWaitedFor() async {
        let (preparation, core) = make { $0.facts = Self.connected }
        preparation.hostOpened()
        core.facts = Self.exchanging
        await preparation.syncChanged()
        await preparation.collectionOpened()
        #expect(preparation.phase == .waitingForExchange && core.starts == 0)
        core.facts = Self.settled(nil)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 1)
    }

    @Test func exchangeThatEndedBeforeTheHostOpenedIsNoEvidence() async {
        let (preparation, core) = make { $0.facts = Self.settled(999_000) }
        preparation.hostOpened()
        core.now = 1_000_400
        await preparation.collectionOpened()
        await preparation.syncChanged()
        #expect(preparation.phase == .waitingForExchange && core.starts == 0)
    }

    @Test func hostOpenOfOneCollectionIsNoEvidenceForTheNext() async {
        let (preparation, core) = make { $0.facts = Self.connected }
        preparation.hostOpened()
        core.facts = Self.settled(1_000_300)
        await preparation.syncChanged()
        preparation.collectionEnded()
        core.now = 1_000_900
        await preparation.collectionOpened()
        #expect(preparation.phase == .waitingForExchange && core.starts == 0)
    }

    @Test func exchangeThatBroughtTheSwitchOnStartsNoJob() async {
        let (preparation, core) = make { $0.facts = Self.exchanging }
        await preparation.collectionOpened()
        core.status = .ok
        core.facts = Self.settled(1_000_500)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 0 && preparation.status == .ok && preparation.phase == .idle && core.records.isEmpty)
    }

    @Test func changeToNeedsTheUserStartsTheJobAndChangeToOfflineDoesNot() async {
        let (preparation, core) = make { $0.facts = Self.exchanging }
        await preparation.collectionOpened()
        // Offline: the exchange is still to come.
        core.facts = Self.connected
        await preparation.syncChanged()
        #expect(core.starts == 0 && preparation.phase == .waitingForExchange)
        // Settling after an offline stop is not the end of an exchange.
        core.facts = Self.settled(nil)
        await preparation.syncChanged()
        #expect(core.starts == 0 && preparation.phase == .waitingForExchange)
        // Off, Sync Automatically off, or a state that needs the person.
        core.facts = .none
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok)
    }

    @Test func studyEntryPointDuringTheWaitStartsAtOnceAndGoesOnAtCommit() async {
        let (preparation, core) = make { $0.facts = Self.connected; $0.holds = true }
        await preparation.collectionOpened()
        #expect(preparation.phase == .waitingForExchange)
        async let prepared = preparation.prepare(for: .study)
        await settle()
        // Running without any change from the service: study never waits for the network.
        #expect(preparation.phase == .running && core.starts == 1)
        core.release()
        #expect(await prepared)
        #expect(core.records.map(\.stage) == [.activation])
    }

    @Test func incomingSwitchChangeDuringTheWaitDoesNotEndIt() async {
        let (preparation, core) = make { $0.facts = Self.exchanging }
        await preparation.collectionOpened()
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        #expect(core.starts == 0 && preparation.phase == .waitingForExchange)
    }

    // MARK: While open (A1.3, F3, F5.2)

    @Test func laterSyncThatTurnsTheSwitchOffStartsOneBackgroundJob() async {
        let (preparation, core) = make { $0.status = .ok; $0.facts = Self.exchanging; $0.holds = true }
        await preparation.collectionOpened()
        core.status = .needsReEnable
        core.facts = Self.settled(1_000_500)
        await preparation.syncChanged()
        await settle()
        #expect(preparation.phase == .running && core.starts == 1)
        // A second read during the job starts none.
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        core.facts = Self.exchanging
        await preparation.syncChanged()
        core.facts = Self.settled(1_000_900)
        await preparation.syncChanged()
        await settle()
        #expect(core.starts == 1)
        core.release()
        await preparation.waitForJob()
        #expect(core.records == [.init(component: .scheduling, operation: .recover, stage: .statusObserved,
            outcome: .succeeded, count: 3)])
    }

    @Test func changeSetsOtherThanAnIncomingSwitchChangeReadNothing() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        await preparation.changesCommitted(Self.changes(source: .userUndoable, keys: ["fsrs"]))
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["rollover"]))
        #expect(core.reads == 1)
        await preparation.changesCommitted(Self.changes(source: .syncIncoming, keys: ["fsrs"]))
        #expect(core.reads == 2)
    }

    @Test func openReviewSessionDefersTheJobUntilItEnds() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        core.sessionOpen = true
        core.status = .needsReEnable
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        #expect(core.starts == 0 && preparation.phase == .idle && preparation.status == .needsReEnable)
        // The core's event can arrive before the session is gone: still deferred.
        await preparation.sessionEnded()
        #expect(core.starts == 0)
        core.sessionOpen = false
        await preparation.sessionEnded()
        await preparation.waitForJob()
        #expect(core.starts == 1 && preparation.status == .ok)
        // Nothing is pending after it.
        await preparation.sessionEnded()
        #expect(core.starts == 1)
    }

    @Test func sessionThatStartedFirstRefusesTheJobWithoutAFailure() async {
        let (preparation, core) = make { $0.results = [.failure(BridgeError.sessionActive)] }
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(preparation.phase == .idle && preparation.status == .needsEnable)
        #expect(core.records.map(\.outcome) == [.refused])
        await preparation.sessionEnded()
        await preparation.waitForJob()
        #expect(core.starts == 2 && preparation.status == .ok)
    }

    @Test func everyReEnableIsRecorded() async {
        let (preparation, core) = make { $0.status = .ok; $0.facts = Self.settled(nil) }
        await preparation.collectionOpened()
        for round in 1...3 {
            core.status = .needsReEnable
            await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
            await preparation.waitForJob()
            #expect(core.starts == round && preparation.status == .ok)
        }
        #expect(core.records.count == 3)
        #expect(core.records.allSatisfy { $0.operation == .recover && $0.stage == .statusObserved && $0.outcome == .succeeded })
    }

    // MARK: Failure (A1.4, F5.3)

    @Test func failureAtOpenRefusesStudyUntilTryAgainSucceeds() async {
        let (preparation, core) = make { $0.results = [.failure(BridgeError.internal("synthetic")), .failure(BridgeError.internal("synthetic"))] }
        await preparation.collectionOpened()
        await preparation.waitForJob()
        // The collection stays open: the coordinator only refuses study.
        #expect(preparation.phase == .failed && preparation.status == .needsEnable && core.commits == 0)
        #expect(core.records == [.init(component: .scheduling, operation: .activate, stage: .activation, outcome: .failed, failure: .other)])
        // A Study control is refused without a new run.
        #expect(await !preparation.prepare(for: .study))
        #expect(core.starts == 1)
        // Try Again runs the job again; a second failure refuses again.
        #expect(await !preparation.prepare(for: .tryAgain))
        #expect(core.starts == 2 && preparation.phase == .failed)
        #expect(await preparation.prepare(for: .tryAgain))
        #expect(core.starts == 3 && preparation.phase == .idle && preparation.status == .ok && core.commits == 1)
        #expect(await preparation.prepare(for: .study))
    }

    @Test func failureInTheBackgroundBehavesAsAFailureAtOpen() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        core.status = .needsReEnable
        core.results = [.failure(BridgeError.internal("synthetic"))]
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        await preparation.waitForJob()
        #expect(preparation.phase == .failed)
        #expect(core.records.last == .init(component: .scheduling, operation: .recover, stage: .statusObserved,
            outcome: .failed, failure: .other))
        #expect(await !preparation.prepare(for: .study))
        #expect(await preparation.prepare(for: .tryAgain))
    }

    private static let contention: [BridgeError] = [.busy(runningOperation: "synthetic"), .unavailable(reason: "synthetic"),
        .stale(table: .config, key: "fsrs"), .staleGeneration(planned: 1, current: 2), .cancelled]

    @Test(arguments: 0..<5)
    func otherWorkHoldingTheCollectionIsNotAFailureAndTheNextStudyRunsTheJob(index: Int) async {
        // An export, an import, a replacement, a suspension: the core refuses or stops the job.
        let (preparation, core) = make { $0.results = [.failure(Self.contention[index])] }
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(preparation.phase == .idle && preparation.status == .needsEnable && core.starts == 1 && core.commits == 0)
        #expect(core.records.count == 1 && core.records.first?.outcome != .succeeded)
        // A Study control is not refused with the failure sentence: it runs the job and goes on.
        #expect(await preparation.prepare(for: .study))
        #expect(core.starts == 2 && preparation.status == .ok && preparation.phase == .idle)
    }

    @Test func otherWorkHoldingTheCollectionLetsTheNextReadRunTheJob() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        core.status = .needsReEnable
        core.results = [.failure(BridgeError.busy(runningOperation: "synthetic"))]
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        await preparation.waitForJob()
        #expect(preparation.phase == .idle && core.starts == 1)
        #expect(core.records.last == .init(component: .scheduling, operation: .recover, stage: .statusObserved,
            outcome: .failed, failure: .unavailable))
        core.facts = Self.exchanging
        await preparation.syncChanged()
        core.facts = Self.settled(1_000_700)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 2 && preparation.status == .ok)
    }

    @Test func studyThatMeetsOtherWorkIsRefusedOnceAndNotLeftFailed() async {
        let (preparation, core) = make { $0.facts = Self.connected; $0.results = [.failure(BridgeError.busy(runningOperation: "synthetic"))] }
        await preparation.collectionOpened()
        #expect(await !preparation.prepare(for: .study))
        #expect(preparation.phase == .idle && core.starts == 1)
        #expect(await preparation.prepare(for: .study))
        #expect(core.starts == 2)
    }

    @Test func laterReadAfterAFailureRunsTheJobAgain() async {
        let (preparation, core) = make { $0.results = [.failure(BridgeError.internal("synthetic"))]; $0.facts = Self.settled(nil) }
        core.facts = .none
        await preparation.collectionOpened()
        await preparation.waitForJob()
        #expect(preparation.phase == .failed)
        core.facts = Self.exchanging
        await preparation.syncChanged()
        core.facts = Self.settled(1_000_700)
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 2 && preparation.status == .ok && preparation.phase == .idle)
    }

    @Test func unreadableStatusRefusesStudyAndIsReadAgainAtTheNextEntryPoint() async {
        let (preparation, core) = make { $0.statusFails = true }
        await preparation.collectionOpened()
        #expect(preparation.status == nil && core.starts == 0)
        #expect(await !preparation.prepare(for: .study))
        core.statusFails = false
        #expect(await preparation.prepare(for: .study))
        #expect(core.starts == 1 && preparation.status == .ok)
    }

    // MARK: Study entry points (F4)

    @Test func refusalByTheCoreReadsAgainRunsTheJobAndGoesOn() async {
        let (preparation, core) = make { $0.status = .ok }
        await preparation.collectionOpened()
        // Another client turned the switch off and no read has seen it yet.
        core.status = .needsReEnable
        #expect(await preparation.prepare(for: .study))
        #expect(core.starts == 0)
        #expect(await preparation.prepare(for: .refusedByCore))
        #expect(core.starts == 1 && preparation.status == .ok)
        #expect(core.records.map(\.operation) == [.recover])
    }

    @Test func racingTriggersRunOneJob() async {
        let (preparation, core) = make { $0.facts = Self.exchanging; $0.holds = true }
        await preparation.collectionOpened()
        core.facts = Self.settled(1_000_500)
        async let first = preparation.prepare(for: .study)
        async let second = preparation.prepare(for: .study)
        async let sync: Void = preparation.syncChanged()
        async let incoming: Void = preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        await settle()
        core.release()
        let results = await (first, second)
        _ = await (sync, incoming)
        await preparation.waitForJob()
        #expect(results == (true, true))
        #expect(core.starts == 1 && core.records.count == 1)
    }

    // MARK: Replacement and close (F5.2)

    @Test func collectionReplacedEndsTheJobAndTheNewCollectionIsTreatedAsAnOpen() async {
        let (preparation, core) = make { $0.holds = true }
        await preparation.collectionOpened()
        await settle()
        #expect(preparation.phase == .running && core.starts == 1)
        preparation.collectionEnded()
        #expect(preparation.status == nil && preparation.phase == .idle)
        #expect(await !preparation.prepare(for: .study))
        await preparation.drain()
        #expect(core.cancels == 1 && core.commits == 0)
        #expect(core.records.map(\.outcome) == [.cancelled])
        // The new collection has the switch off and a connected service: F5.1 applies afresh.
        core.status = .needsEnable; core.holds = false
        core.facts = Self.connected
        await preparation.collectionOpened()
        #expect(preparation.phase == .waitingForExchange && core.starts == 1)
        core.facts = .none
        await preparation.syncChanged()
        await preparation.waitForJob()
        #expect(core.starts == 2 && preparation.status == .ok && core.commits == 1)
    }

    @Test func resultOfAReplacedCollectionsJobNeverAppliesToTheNextOne() async {
        let (preparation, core) = make { $0.holds = true }
        await preparation.collectionOpened()
        preparation.collectionEnded()
        core.status = .needsEnable; core.facts = Self.connected
        await preparation.collectionOpened()
        await preparation.drain()
        #expect(preparation.phase == .waitingForExchange && preparation.status == .needsEnable && core.commits == 0)
    }

    @Test func nothingRunsBeforeAnOpenOrAfterAClose() async {
        let (preparation, core) = make()
        await preparation.syncChanged()
        await preparation.sessionEnded()
        await preparation.changesCommitted(Self.changes(source: .cloudIncoming, keys: ["fsrs"]))
        #expect(await !preparation.prepare(for: .tryAgain))
        #expect(core.reads == 0 && core.starts == 0)
    }

    @Test func scriptedFixturesRunTheJobFromAStudyEntryPointOnly() async {
        let (preparation, core) = make()
        preparation.runsUnprompted = false
        await preparation.collectionOpened()
        await settle()
        #expect(core.starts == 0 && preparation.status == .needsEnable && preparation.available)
        #expect(await preparation.prepare(for: .study))
        #expect(core.starts == 1 && preparation.status == .ok)
    }

    // MARK: Source checks (T1)

    @Test func coordinatorHoldsNoTimerAndTheSheetIsGone() throws {
        let package = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let source = try String(contentsOf: package.appending(path: "Sources/AppModel/Collection/StudyPreparation.swift"), encoding: .utf8)
        for word in ["Timer", "sleep", "DispatchQueue", "URLSession", "asyncAfter"] { #expect(!source.contains(word), "\(word)") }
        let catalog = try String(contentsOf: package.appending(path: "Sources/Features/Home/Resources/Localizable.xcstrings"), encoding: .utf8)
        #expect(catalog.contains("\"study.prepare.failed\"") && catalog.contains("\"study.prepare.tryAgain\""))
        #expect(catalog.contains("Meguri couldn’t prepare this collection for study."))
        var swift: [URL] = []
        let sources = package.appending(path: "Sources")
        if let files = FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil) {
            for case let file as URL in files where ["swift", "xcstrings"].contains(file.pathExtension) { swift.append(file) }
        }
        #expect(swift.count > 50)
        for file in swift {
            let text = try String(contentsOf: file, encoding: .utf8)
            for removed in ["home.policy.", "FsrsAdmissionView", "FsrsEnabledSheet", "fsrsAdmission", "Enable FSRS", "Study with FSRS"] {
                #expect(!text.contains(removed), "\(removed) in \(file.lastPathComponent)")
            }
        }
    }

    private static func changes(source: OpSource, keys: [String]) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: source, cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [],
            configKeys: keys.map { .init(name: $0, change: .updated) }, tags: [], revlogAdded: [], revlogRemoved: [],
            mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [],
            collectionRow: false)
    }
}
