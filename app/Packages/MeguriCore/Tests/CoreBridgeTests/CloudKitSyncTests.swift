import Foundation
import Testing
@testable import CoreBridge

// Source-only first phase of U114. Enabled tests deliberately reference future
// production APIs. Compile RED is required until the real bridge and adapter
// implement them. No #if, skip, expected failure or fake core coverage.
// Main base: 218ab2e2. Coherent Cloud batch base:
// 1416a520542a8c5daa38a23bfe398da704c3d6c9. Runner Git base is synthetic.
//
// CoreCloudWorkflowService -> Rust provides every behavioral observation here.
// In the two ordering cases, pendingTransition (Runtime::pending_transition)
// supplies only synthetic pending-list input; it establishes no collection
// policy facts. The fake emits SDK events and supplies no core outcomes.
// These cases share serialization with the process-wide CoreHost suite.
private final class CloudTaskOriginalFailure: Error {}

extension EventTests {
    /// Helper-only regression: no adapter/core outcome is supplied here.
    /// Both terminal paths precede the expected gate, with another owned
    /// participant parked until cleanup releases it. No timeout is involved.
    @Test(arguments: [true, false])
    func cloudKitTaskTerminalBeforeArrivalDrainsGatesAndChildren(throwsOriginal: Bool) async throws {
        let original = CloudTaskOriginalFailure()
        let missing = CloudCallbackGate(), parked = CloudCallbackGate()
        let bodyReady = CloudCallbackGate()
        await bodyReady.open()
        try await withThrowingTaskGroup(of: Bool.self) { workers in
            workers.addTask { await parked.enter(); return true }
            try await parked.waitForArrival()
            do {
                try await withCloudTask(gates: [missing, parked, bodyReady], expectedArrivals: [missing], operation: {
                    try await bodyReady.waitForArrival()
                    if throwsOriginal { throw original }
                }, whileRunning: {
                    await bodyReady.enter()
                    try await missing.waitForArrival()
                    Issue.record("An operation that never entered the expected gate satisfied its arrival")
                })
                Issue.record("Terminal operation without the expected arrival did not throw")
            } catch {
                if throwsOriginal {
                    #expect((error as? CloudTaskOriginalFailure) === original)
                } else if case CloudCallbackGate.ArrivalError.operationCompletedBeforeArrival = error {
                    // The actual early-success result, rather than a timeout.
                } else {
                    Issue.record("Unexpected early-success error: \(error)")
                }
            }
            // This participant can return only after cleanup opens parked.
            #expect(try await workers.next() == true)
            for gate in [missing, parked, bodyReady] {
                #expect(await gate.pendingWaiterCount() == 0)
            }
        }
    }

    /// Q11/P24 K12.3 Saved, K12.4/A23; P21 in-flight; K22.2 rev3/rev4.
    /// Both variants must acknowledge the actual built revision 3, retain 4,
    /// and hand 4 back without a foreground/local-change/full-update trigger.
    @Test(arguments: [true, false])
    func cloudKitSavedRevision3CannotEraseRevision4(a23: Bool) async throws {
        try await withCloudKitFixture(a23: a23, keepAlive: !a23) { f in
            let engine = try await f.engine()
            let name = try #require(f.names.first)
            let deck = try #require(f.decks.first)
            let built = CloudCallbackGate(), persisted = CloudCallbackGate()
            let returned = CloudCallbackGate(), lateRemoval = CloudCallbackGate()
            await engine.pauseAfterBatch(built)
            await engine.pauseAfterSentHandler(returned)
            if !a23 { await engine.pauseLateSavedRemoval(lateRemoval) }
            await f.probe.pauseSent(persisted)
            let original = try #require(try await f.core.entityFacts(name: name))
            #expect(original.localRev == 3 && original.ackedRev < 3 && original.mark != nil)

            try await withCloudTask(gates: [built, persisted, returned, lateRemoval],
                                    expectedArrivals: [built, persisted, returned] + (a23 ? [] : [lateRemoval]), operation: {
                try await f.adapter.requestSend(zones: [f.zone])
            }, whileRunning: {
                try await built.waitForArrival()
                let flight = try #require(try await f.core.entityFacts(name: name))
                #expect(flight.inFlight.contains { $0.localRev == 3 })
                let edit = try await CoreDeckService().renameDeck(id: deck, name: "Revision four")
                try await f.core.awaitTracking(generation: edit.summary.generation, seq: edit.summary.seq)
                #expect(try await f.core.entityFacts(name: name)?.localRev == 4)
                await built.open()
                try await persisted.waitForArrival()
                let afterSaved = try #require(try await f.core.entityFacts(name: name))
                #expect(afterSaved.ackedRev == 3)
                #expect(afterSaved.localRev == 4 && afterSaved.mark != nil)
                #expect(afterSaved.payloadDigest != afterSaved.ackedPayloadDigest)
                let emitted = try await f.core.emittedUpdateIDs()
                await persisted.open()
                try await returned.waitForArrival()
                await returned.open()
                try await f.adapter.waitForDeferredUpdates()
                if !a23 {
                    try await lateRemoval.waitForArrival()
                    // The deferred add has already happened; SDK then erases
                    // it. Only comparison AFTER send returns can repair this.
                    #expect(await engine.pendingRecordZoneChanges()[name] == .save)
                    await lateRemoval.open()
                }
                #expect(!emitted.isEmpty)
            })
            try await f.adapter.waitForIdle()
            #expect(await engine.pendingRecordZoneChanges()[name] == .save)
            #expect(await engine.pendingRecordZoneChanges() == (try await f.core.trackingSendables()))
            let firstTrace = await engine.recordedTrace()
            #expect(firstTrace.filter { if case .sendEntered = $0 { true } else { false } }.count == 1)
            #expect(!firstTrace.contains(.reentrantNetworkCall))
            #expect(!firstTrace.contains { if case .fetchEntered = $0 { true } else { false } })
            let removal = try #require(firstTrace.firstIndex { if case .savedRemoved = $0 { true } else { false } })
            let handler = try #require(firstTrace.firstIndex(of: .eventReturned("sent")))
            #expect(handler < removal)
            if !a23 {
                let deferredAdd = try #require(firstTrace[..<removal].lastIndex {
                    if case .pendingApplied(_, _, let adds) = $0 { adds.contains(name) } else { false }
                })
                #expect(handler < deferredAdd && deferredAdd < removal)
                let sendReturn = try #require(firstTrace.firstIndex(of: .sendReturned(1)))
                let repair = try #require(firstTrace[removal...].firstIndex {
                    if case .pendingApplied(_, _, let adds) = $0 { adds.contains(name) } else { false }
                })
                #expect(sendReturn < repair)
            }
            // A second actual call proves the retained mark is usable.
            try await f.adapter.requestSend(zones: [f.zone])
            try await f.adapter.waitForIdle()
            let done = try #require(try await f.core.entityFacts(name: name))
            #expect(done.localRev == 4 && done.ackedRev == 4 && done.mark == nil)
        }
    }

    /// Q11 K12.4; P21 hold-back/deferrals; actual ReviewService SessionEnded.
    @Test func cloudKitHeldEditWaitsForRealSessionEndedFullUpdate() async throws {
        try await withCloudKitFixture { f in
            if try await Core.fsrsPolicyStatus() != .ok {
                let job = try await Core.enableFsrs()
                try await job.value()
            }
            let admission = try await Core.startSession(deckID: f.reviewDeck)
            guard case .started(let session, _, let head, _) = admission else {
                Issue.record("Synthetic review baseline did not admit a real session"); return
            }
            do {
                let card = try #require(head.items.first)
                #expect(card.cardId == f.reviewCard)
                let outcome = try await Core.answerSession(
                    sessionID: session, cardID: card.cardId, rating: .good,
                    stateToken: card.stateToken, answeredAtMs: cloudFixtureOriginMilliseconds, elapsedMs: 100
                )
                guard case .accepted = outcome else {
                    Issue.record("Real answer was not accepted"); try await Core.endSession(sessionID: session); return
                }
                try await f.core.pollReview()
                try await f.adapter.consumePendingUpdates()
                let held = try #require(try await f.core.entityFacts(name: f.reviewName))
                #expect(held.mark != nil)
                let engine = try await f.engine()
                try await f.adapter.requestSend(zones: [f.zone])
                try await f.adapter.waitForIdle()
                #expect(!(await engine.recordedTrace()).contains {
                    if case .handed(_, let names) = $0 { names.contains(f.reviewName) } else { false }
                })
                let before = try await f.core.emittedUpdateIDs()
                try await Core.endSession(sessionID: session)
                try await f.core.pollReview()
                try await f.adapter.consumePendingUpdates()
                try await f.adapter.waitForIdle()
                let observations = try await f.core.observations()
                let end = try #require(observations.firstIndex(of: .sessionEnded(session: session)))
                #expect(observations[end...].contains {
                    if case .corePendingEmitted(let kind, let id) = $0 {
                        kind == .full && !before.contains(id)
                    } else { false }
                })
                #expect(await engine.pendingRecordZoneChanges()[f.reviewName] == .save)
                try await f.adapter.requestSend(zones: [f.zone])
                try await f.adapter.waitForIdle()
                let sent = try #require(try await f.core.entityFacts(name: f.reviewName))
                #expect(sent.mark == nil && sent.ackedRev == sent.localRev)
            } catch {
                try? await Core.endSession(sessionID: session)
                throw error
            }
        }
    }

    /// Q11/P24: preserve ordering for complete deferred updates and removals.
    /// This input exercises routing; entity and media policy remain outside its scope.
    @Test(arguments: [true, false])
    func cloudKitDeferredAddThenFullRemovalRemainsAbsent(a23: Bool) async throws {
        try await cloudKitOrderedPending(a23: a23, replacement: nil)
    }

    /// Q11/P24: SDK deduplicates save/delete by name; a deferred blob save
    /// cannot overtake a later deletion add. Routing, not blob policy.
    @Test(arguments: [true, false])
    func cloudKitDeferredBlobSaveThenDeleteKeepsDelete(a23: Bool) async throws {
        try await cloudKitOrderedPending(a23: a23, replacement: .delete)
    }

    private func cloudKitOrderedPending(a23: Bool, replacement: CloudPendingChange?) async throws {
        try await withCloudKitFixture(a23: a23) { f in
            let engine = try await f.engine()
            let inside = CloudCallbackGate()
            await f.probe.pauseSent(inside)
            let name = "synthetic-pending-X"
            let ids = try await withCloudTask(gates: [inside], operation: {
                try await f.adapter.requestSend(zones: [f.zone])
            }, whileRunning: {
                try await inside.waitForArrival()
                let instance = try await f.core.instance()
                let add = try await f.core.pendingTransition(instance: instance, kind: .changes,
                                                            snapshot: [name: .save])
                let later = try await f.core.pendingTransition(
                    instance: instance, kind: replacement == nil ? .full : .changes,
                    snapshot: replacement.map { [name: $0] } ?? [:]
                )
                let ids = [add.id, later.id]
                try await f.adapter.consumePendingUpdates()
                #expect(!(await engine.recordedTrace()).contains {
                    if case .pendingApplied(let id, _, _) = $0 { ids.contains(id) } else { false }
                })
                await inside.open()
                return ids
            })
            try await f.adapter.waitForIdle()
            let trace = await engine.recordedTrace()
            let applied = trace.compactMap { event -> UInt64? in
                if case .pendingApplied(let id, _, _) = event { id } else { nil }
            }
            #expect(applied.filter { ids.contains($0) } == ids)
            #expect(applied == (try await f.core.emittedUpdateIDs()))
            let returned = try #require(trace.firstIndex(of: .eventReturned("sent")))
            for id in ids {
                let index = try #require(trace.firstIndex {
                    if case .pendingApplied(let actual, _, _) = $0 { actual == id } else { false }
                })
                #expect(returned < index)
            }
            #expect(await engine.pendingRecordZoneChanges()[name] == replacement)
            #expect(await engine.pendingRecordZoneChanges() == (try await f.core.trackingSendables()))
            if replacement == nil {
                #expect(trace.contains {
                    if case .pendingApplied(let id, let removes, _) = $0 {
                        id == ids.last && removes.contains(name)
                    } else { false }
                })
            }
        }
    }

    /// Q11/P24 K12.4/P12.3 step4. Actual send calls and durable selected
    /// acknowledgements are observed AFTER every handler/deferred list/send
    /// returns. No helper loop makes sends or manufactures success.
    @Test func cloudKitAutoOff400SelectedLimitExceededFinishesWithinEightCalls() async throws {
        try await withCloudKitFixture(count: 400) { f in
            let engine = try await f.engine()
            #expect(!engine.automaticallySync)
            await engine.appendSendReplies([.failed(.limitExceeded)])
            let result = try await f.adapter.syncNow()
            try await f.adapter.waitForIdle()
            #expect(result.collection == .succeeded)
            #expect(result.selectedNames == Set(f.names))
            #expect(result.ackedNames == result.selectedNames)
            #expect(result.outstandingNames.isEmpty)
            let trace = await engine.recordedTrace()
            let entries = trace.compactMap { if case .sendEntered(let id) = $0 { id } else { nil } }
            let returns = trace.compactMap { if case .sendReturned(let id) = $0 { id } else { nil } }
            #expect((2...8).contains(entries.count))
            #expect(returns == entries)
            let handed = trace.compactMap { event -> [String]? in
                if case .handed(_, let names) = event { names } else { nil }
            }
            #expect(handed.first?.count == 250)
            #expect(handed.dropFirst().allSatisfy { $0.count <= 125 })
            #expect(Set(handed.dropFirst().flatMap { $0 }) == Set(f.names))
            for name in f.names {
                let fact = try #require(try await f.core.entityFacts(name: name))
                #expect(fact.ackedRev == 3 && fact.localRev == 3 && fact.mark == nil)
                #expect(fact.inFlight.isEmpty)
                #expect(!fact.ackedSystemFields.isEmpty)
            }
            let durable = try #require(try await f.core.exchangeResult(id: result.id))
            #expect(durable == result)
            #expect(await engine.pendingRecordZoneChanges().isEmpty)
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    /// P21/V23.2/V3.1: serial delivery cannot advance the opaque state while
    /// the corresponding durable inbox event's handler is still suspended.
    @Test func cloudKitInboxCommitPrecedesOpaqueSDKState() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let gate = CloudCallbackGate()
            await f.probe.pauseFetched(gate)
            let old = try await f.core.durableSnapshot()
            let state = Data([0xff, 0x00, 0x73, 0x64, 0x6b, 0x81])
            try await withCloudTask(gates: [gate], operation: {
                await engine.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "fetched")
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect(try await f.core.durableSnapshot().inboxNames.contains(f.incoming.name))
                try await withCloudTask(gates: [gate], operation: {
                    await engine.emitRetained(.stateUpdate(state), label: "state")
                }, whileRunning: {
                    #expect(try await f.core.durableSnapshot().serialization == old.serialization)
                    #expect(!(await engine.recordedTrace()).contains(.eventEntered("state")))
                    await gate.open()
                })
            })
            let committed = try await f.core.durableSnapshot()
            #expect(committed.serialization == state)
            #expect(committed.inboxNames.contains(f.incoming.name))
            let trace = await engine.recordedTrace()
            let inboxReturn = try #require(trace.firstIndex(of: .eventReturned("fetched")))
            let stateEnter = try #require(trace.firstIndex(of: .eventEntered("state")))
            #expect(inboxReturn < stateEnter)
            try await f.adapter.configureAutomatic(true)
            try await f.adapter.waitForIdle()
            let replacement = try await f.engine()
            #expect(replacement.restoredSerialization == state)
        }
    }

    /// P21 tracking failure, V23.2 durable fetch failure, A6/A7. Faults are
    /// injected into real Rust persistence, including opaque-state commits.
    @Test(arguments: [true, false])
    func cloudKitPersistenceFailureStopsAndDrainsOldInstance(failInbox: Bool) async throws {
        try await withCloudKitFixture(keepAlive: true, failInbox: failInbox, failState: !failInbox) { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            await engine.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "fetched")
            await engine.emitRetained(.stateUpdate(Data([0x6e, 0x65, 0x77])), label: "state")
            try await f.adapter.waitForIdle()
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization)
            #expect(after.trackingStopped)
            #expect(after.inboxNames.contains(f.incoming.name) == !failInbox)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.factory.engines().count == 1)
            let trace = await engine.recordedTrace()
            #expect(trace.contains(.cancelEntered) && trace.contains(.cancelReturned) && trace.contains(.released))
            let failedHandler = try #require(trace.firstIndex(of: .eventReturned(failInbox ? "fetched" : "state")))
            let cancelEntered = try #require(trace.firstIndex(of: .cancelEntered))
            let cancelReturned = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(failedHandler < cancelEntered && cancelEntered < cancelReturned && cancelReturned < released)
            let stable = try await f.core.durableSnapshot()
            await engine.emitRetained(.stateUpdate(Data([0x7f])), label: "lateState")
            #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            #expect(try await f.core.durableSnapshot() == stable)
        }
    }

    /// Q11/P21 fetch steps: joined has no own will; a skip has no events.
    /// Exactly one repeat; neither successful API return alone invents drains.
    @Test(arguments: [true, false])
    func cloudKitJoinedOrShortCircuitedFetchRepeatsOnce(joined: Bool) async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let old = try await f.core.fetchDrains()
            await engine.appendFetchScripts([
                joined ? .joined(group: "earlier-group") : .shortCircuited,
                .manual(fetchedZones: ["meguri-root", f.zone], records: [], group: nil),
            ])
            try await f.adapter.requestFetch(zones: ["meguri-root", f.zone])
            try await f.adapter.waitForIdle()
            let drains = Array((try await f.core.fetchDrains()).dropFirst(old.count))
            #expect(drains.map(\.zone) == ["meguri-root", f.zone])
            #expect(drains.allSatisfy(\.fetched))
            let trace = await engine.recordedTrace()
            #expect(trace.filter { if case .fetchEntered = $0 { true } else { false } }.count == 2)
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    /// Q11 O13/P21 fetch steps: no-account short circuits on both calls end
    /// Sync Now waiting, after persistence, with no fabricated FetchDrained.
    @Test func cloudKitBothFetchesSkippedNeverClaimDrainOrSuccess() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.fetchDrains()
            await engine.appendFetchScripts([.shortCircuited, .shortCircuited])
            let result = try await f.adapter.syncNow()
            try await f.adapter.waitForIdle()
            #expect(result.collection == .waiting(.noAccount))
            #expect(try await f.core.fetchDrains() == before)
            #expect(!(try await f.core.status()).syncingCollection)
            let trace = await engine.recordedTrace()
            #expect(trace.filter { if case .fetchEntered = $0 { true } else { false } }.count == 2)
            #expect(!trace.contains { if case .sendEntered = $0 { true } else { false } })
        }
    }

    /// Q3.3/P21: saved state with no zone changes drains false; manual root
    /// scope stays root through the delegate options callback. This baseline
    /// has saved state and a recorded MGActive; it makes no no-state inference.
    @Test func cloudKitSavedStateUnchangedZonesAndManualRootScope() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.fetchDrains()
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: ["meguri-root"])
            try await f.adapter.waitForIdle()
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == ["meguri-root"])
            #expect(drains.map(\.fetched) == [false])
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
        }
    }

    /// Q3.3/P3 no-state constraint: a zone fetch begun before same-identity
    /// signIn is not a fetch after that point and completes no absence pass.
    /// This covers the event fence, not V26.1's subsequent record-ID reads.
    @Test func cloudKitSameIdentityNoStatePointRejectsEarlierZoneFetch() async throws {
        try await withCloudKitFixture(savedState: nil) { f in
            let engine = try await f.engine()
            #expect(engine.restoredSerialization == nil)
            let before = try await f.core.fetchDrains()
            let guardBefore = try await f.core.currentGuard()
            await engine.emitRetained(.willFetch(groupID: "scheduled-before-signin", reason: .scheduled), label: "willFetch")
            await engine.emitRetained(.willFetchZone(zone: f.zone), label: "willCollectionBeforeSignIn")
            await engine.signInSameSyntheticIdentity()
            #expect(try await f.core.currentGuard() == guardBefore)
            await engine.emitRetained(.didFetchZone(zone: f.zone, error: nil), label: "didCollectionAfterSignIn")
            await engine.emitRetained(.didFetch(groupID: "scheduled-before-signin", reason: .scheduled), label: "didFetch")
            #expect(try await f.core.fetchDrains() == before)
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
        }
    }

    /// Q11 O13/P3: when did is absent, close its boundary at the following will
    /// or return. SDK replacement groups qualify unless owned by another call.
    @Test func cloudKitMissingDidAndReplacedOperationGroupDoNotWedgeFetch() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.fetchDrains()
            await engine.appendFetchScripts([
                .missingDid(group: "sdk-owned-group"),
                .manual(fetchedZones: ["meguri-root", f.zone], records: [], group: "replacement-group"),
            ])
            try await f.adapter.requestFetch(zones: ["meguri-root", f.zone])
            try await f.adapter.waitForIdle()
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == ["meguri-root", f.zone])
            #expect(try await f.core.diagnostics().missingDidFetchEvents == 1)
        }
    }

    /// Q3.3/P21: scheduled events report fetched zones only; failed zone
    /// events never drain, and scheduled scope is restricted by the delegate.
    @Test func cloudKitScheduledFetchZoneFailureAndScope() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.fetchDrains()
            let scoped = await engine.scheduledFetchOptions(
                .init(zones: ["meguri-root", f.zone, "meguri-media", "c-other"],
                      priorityZones: [], groupID: "scheduled-scope")
            )
            #expect(scoped.zones == ["meguri-root", f.zone])
            await engine.emitRetained(.willFetch(groupID: "scheduled", reason: .scheduled), label: "willFetch")
            await engine.emitRetained(.willFetchZone(zone: "meguri-root"), label: "willRoot")
            await engine.emitRetained(.didFetchZone(zone: "meguri-root", error: nil), label: "didRoot")
            await engine.emitRetained(.willFetchZone(zone: f.zone), label: "willCollection")
            await engine.emitRetained(.didFetchZone(zone: f.zone, error: .networkFailure), label: "didCollectionFailed")
            await engine.emitRetained(.didFetch(groupID: "scheduled", reason: .scheduled), label: "didFetch")
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(drains.map(\.zone) == ["meguri-root"])
            #expect(drains.map(\.fetched) == [true])
        }
    }

    /// P21; V23.2.
    @Test(arguments: CloudFenceTransition.allCases)
    func cloudKitOldSavedStateFetchedRootAndBatchChangeNothing(transition: CloudFenceTransition) async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let guardBefore = try await f.core.currentGuard()
            let batch = try #require(await old.requestRetainedBatch(zone: f.zone))
            let saved = batch.saves.map {
                CloudRecord(name: $0.name, envelope: $0.envelope, systemFields: Data([0x6f, 0x6c, 0x64]))
            }
            try await transition.apply(to: f)
            try await f.adapter.waitForIdle()
            if transition == .recreate {
                #expect(try await f.core.currentGuard() == guardBefore)
                #expect(try await f.engine().instanceID != old.instanceID)
            } else {
                #expect(try await f.core.currentGuard() != guardBefore)
            }
            let before = try await f.core.durableSnapshot()
            await old.emitRetained(.sent(batchID: batch.id, saved: saved, failed: [], deleted: []), label: "oldSaved")
            await old.emitRetained(.stateUpdate(Data([0x6f, 0x6c, 0x64])), label: "oldState")
            await old.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "oldFetched")
            await old.emitRetained(.fetched(zone: "meguri-root", records: [f.root], deleted: []), label: "oldRoot")
            #expect(await old.requestRetainedBatch(zone: f.zone) == nil)
            #expect(try await f.core.durableSnapshot() == before)
            #expect(!(await old.recordedTrace()).contains { if case .handed = $0 { true } else { false } })
        }
    }

    /// P4/V3.4: origin is captured before suspension; resumed old callbacks
    /// cannot acquire the current guard/instance. No second SDK before cancel
    /// completion, even with keep-alive after release.
    @Test func cloudKitSuspendedOldCallbackAndCancelCompletionFenceReplacement() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let callback = CloudCallbackGate(), cancel = CloudCallbackGate()
            await old.pauseNextCallback(callback)
            await old.pauseCancellation(cancel)
            try await withCloudTask(gates: [callback, cancel], expectedArrivals: [callback], operation: {
                await old.emitRetained(.stateUpdate(Data([0x6f, 0x6c, 0x64])), label: "suspendedOldState")
            }, whileRunning: {
                try await callback.waitForArrival()
                try await withCloudTask(gates: [callback, cancel], operation: {
                    try await f.adapter.configureAutomatic(true)
                }, whileRunning: {
                    // Stop first invalidates origin, then waits for the active
                    // handler, then cancel. It cannot wait for cancel here.
                    try await f.adapter.waitForStopping(instanceID: old.instanceID)
                    #expect(await f.factory.engines().count == 1)
                    await callback.open()
                    try await cancel.waitForArrival()
                    #expect(await f.factory.engines().count == 1)
                    await cancel.open()
                })
            })
            try await f.adapter.waitForIdle()
            #expect(await f.factory.engines().count == 2)
            #expect(try await f.core.durableSnapshot().serialization != Data([0x6f, 0x6c, 0x64]))
            let stable = try await f.core.durableSnapshot()
            await old.emitRetained(.stateUpdate(Data([0x61])), label: "keepAliveState")
            #expect(await old.requestRetainedBatch(zone: f.zone) == nil)
            #expect(try await f.core.durableSnapshot() == stable)
        }
    }

    /// P4/P21: the saved handler already passed its original guard and
    /// durably acked when it suspends. A token change during that suspension
    /// must fence its deferred pending actions, even though the ack was valid.
    @Test func cloudKitGuardCapturedBeforeSuspensionFencesDeferredActions() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let handler = CloudCallbackGate()
            await f.probe.pauseSent(handler)
            try await withCloudTask(gates: [handler], operation: {
                try await f.adapter.requestSend(zones: [f.zone])
            }, whileRunning: {
                try await handler.waitForArrival()
                let originalGuard = try await f.core.currentGuard()
                let instance = try await f.core.instance()
                let deferred = try await f.core.pendingTransition(
                    instance: instance, kind: .changes, snapshot: ["synthetic-stale-add": .save]
                )
                try await f.adapter.consumePendingUpdates()
                // Real policy input changes the token before stop waits for
                // this handler. Do not await the stop while still paused.
                try await f.core.setPaused(true)
                #expect(try await f.core.currentGuard() != originalGuard)
                try await withCloudTask(gates: [handler], operation: {
                    try await f.adapter.executePendingActions()
                }, whileRunning: {
                    try await f.adapter.waitForStopping(instanceID: old.instanceID)
                    await handler.open()
                })
                #expect(!(await old.recordedTrace()).contains {
                    if case .pendingApplied(let id, _, _) = $0 { id == deferred.id } else { false }
                })
            })
            try await f.adapter.waitForIdle()
            #expect(await old.pendingRecordZoneChanges()["synthetic-stale-add"] == nil)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
        }
    }
}

enum CloudFenceTransition: CaseIterable, Sendable {
    case signOut, accountSwitch, stop, recreate, generation, binding, token

    // Every path is a real policy/lifecycle input; no test mutates the guard.
    func apply(to f: CloudKitBridgeFixture) async throws {
        switch self {
        case .signOut:
            try await f.adapter.accountChanged(status: .noAccount, identity: nil)
        case .accountSwitch:
            try await f.adapter.accountChanged(status: .available, identity: Data("synthetic-account-B".utf8))
        case .stop:
            try await f.adapter.stop()
        case .recreate:
            try await f.adapter.configureAutomatic(true)
        case .generation:
            try await f.collection.close()
        case .binding:
            try await f.core.stopSyncing()
            try await f.adapter.executePendingActions()
        case .token:
            try await f.core.setPaused(true)
            try await f.adapter.executePendingActions()
        }
    }
}
