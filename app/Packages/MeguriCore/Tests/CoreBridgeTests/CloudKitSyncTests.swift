import Foundation
import Testing
@testable import CoreBridge

// CoreCloudWorkflowService -> Rust provides every behavioral observation here.
// In the two ordering cases, pendingTransition (Runtime::pending_transition)
// supplies only synthetic pending-list input; it establishes no collection
// policy facts. The fake emits SDK events and supplies no core outcomes.
// These cases share serialization with the process-wide CoreHost suite.
private final class CloudTaskOriginalFailure: Error {}

enum CloudIdentityReadTrigger: CaseIterable, Sendable, Equatable {
    case initial, notification, network, foreground

    func read(_ adapter: CloudKitSyncAdapter) async throws {
        switch self {
        case .initial: try await adapter.start(automaticallySync: false)
        case .notification: try await adapter.platformAccountChanged()
        case .network: try await adapter.networkChanged(reachable: true)
        case .foreground: try await adapter.foregroundReturned()
        }
    }
}

extension EventTests {
    @Test func cancelledIdentityWaitReturnsBeforeIgnoringProviderAndLateReplyCannotRotateSuccessor() async throws {
        try await withCloudKitFixture { f in
            let gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.success(.init(status: .noAccount, identity: nil)), gate: gate)
            let returned = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let original = Task {
                defer { returned.continuation.yield(()) }
                do { try await f.adapter.foregroundReturned(); Issue.record("Cancelled original read succeeded") }
                catch is CancellationError { }
            }
            do {
                try await gate.waitForArrival()
                original.cancel()
                // This original completion must precede opening the provider.
                try await withThrowingTaskGroup(of: Void.self) { group in
                    group.addTask {
                        var iterator = returned.stream.makeAsyncIterator()
                        guard await iterator.next() != nil else { throw CloudTaskOriginalFailure() }
                    }
                    group.addTask {
                        try await Task.sleep(for: .seconds(10))
                        throw CloudTaskOriginalFailure()
                    }
                    defer { group.cancelAll() }
                    _ = try await group.next()
                }
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
                try await f.adapter.foregroundReturned()
                let retained = await f.adapter.retainedInstanceIDs()
                await gate.open()
                _ = await original.result
                #expect(await f.adapter.retainedInstanceIDs() == retained)
            } catch {
                await gate.open(); original.cancel(); _ = await original.result
                throw error
            }
        }
    }

    /// V3.4/V9.1: stop joins no paused resolver and revokes every read intent.
    @Test(arguments: CloudIdentityReadTrigger.allCases)
    func cloudKitStopDuringIdentityReadCannotCreateAnSDK(trigger: CloudIdentityReadTrigger) async throws {
        try await withCloudKitFixture { f in
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            if trigger == .initial { try await f.adapter.stop() }
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            let count = await f.factory.engines().count
            let gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))), gate: gate)
            try await withCloudTask(gates: [gate], operation: {
                do {
                    try await trigger.read(f.adapter)
                    Issue.record("Stopped identity read completed its run intent")
                } catch is CancellationError { }
            }, whileRunning: {
                try await gate.waitForArrival()
                try await f.adapter.stop()
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                #expect(await f.factory.engines().count == count)
                await gate.open()
            })
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.factory.engines().count == count)
            try await f.adapter.foregroundReturned()
            try await f.adapter.platformAccountChanged()
            try await f.adapter.networkChanged(reachable: false)
            try await f.adapter.networkChanged(reachable: true)
            #expect(await f.factory.engines().count == count)
        }
    }

    /// V3.4: a newer successful read owns its SDK while the old resolver returns.
    @Test func cloudKitSupersededInitialReadCannotStopTheNewOwner() async throws {
        try await withCloudKitFixture { f in
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            let count = await f.factory.engines().count
            let gate = CloudCallbackGate()
            let account = CloudResolvedAccount(status: .available, identity: Data("synthetic-account-A".utf8))
            await f.factory.appendAccountRead(.success(account), gate: gate)
            let replacementID = try await withCloudTask(gates: [gate], operation: {
                do {
                    try await f.adapter.start(automaticallySync: false)
                    Issue.record("Superseded initial read completed its run intent")
                } catch is CancellationError { }
            }, whileRunning: {
                try await gate.waitForArrival()
                await f.factory.appendAccountRead(.success(account))
                try await f.adapter.start(automaticallySync: true)
                let replacement = try await f.engine()
                #expect(await f.factory.engines().count == count + 1)
                await gate.open()
                return replacement.instanceID
            })
            #expect(await f.adapter.retainedInstanceIDs() == [replacementID])
            #expect(await f.factory.engines().count == count + 1)
            try await f.core.validateSDK(replacementID)
        }
    }

    /// V3.4: a cancelled resolver may return, but cannot publish its proof/SDK.
    @Test func cloudKitCancelledInitialIdentityReadCannotCreateAnSDK() async throws {
        try await withCloudKitFixture { f in
            try await f.adapter.stop()
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            let gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))), gate: gate)
            let count = await f.factory.engines().count
            let start = Task {
                do { try await f.adapter.start(automaticallySync: false); _ = await gate.operationSucceeded() }
                catch { await gate.abort(error); throw error }
            }
            do {
                try await gate.waitForArrival()
                start.cancel()
                await gate.open()
                do { try await start.value; Issue.record("Cancelled initial read started an SDK") }
                catch is CancellationError { }
                #expect(await f.factory.engines().count == count)
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                do { _ = try await f.core.registerSDK(automatic: false); Issue.record("Cancelled read verified the owner") }
                catch { }
            } catch {
                await gate.open()
                start.cancel()
                _ = await start.result
                throw error
            }
        }
    }

    /// V3.4: a creation superseded during makeEngine releases only its own SDK.
    @Test func cloudKitSupersededCreationReleasesItsOwnSDKBeforeReplacement() async throws {
        try await withCloudKitFixture { f in
            try await f.adapter.stop()
            let gate = CloudCallbackGate()
            await f.factory.pauseNextCreation(gate)
            try await withCloudTask(gates: [gate], operation: {
                do {
                    try await f.adapter.start(automaticallySync: false)
                    Issue.record("Superseded creation was published")
                } catch is CancellationError { }
            }, whileRunning: {
                try await gate.waitForArrival()
                let superseded = try await f.engine()
                try await f.adapter.stop()
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                try await withCloudTask(gates: [gate], expectedArrivals: [], operation: {
                    try await f.adapter.start(automaticallySync: true)
                }, whileRunning: {
                    await gate.open()
                })
                let replacement = try await f.engine()
                #expect(replacement.instanceID != superseded.instanceID)
                #expect(await superseded.recordedTrace().contains(.released))
                #expect(!(await replacement.recordedTrace()).contains(.released))
                #expect(await f.adapter.retainedInstanceIDs() == [replacement.instanceID])
                try await f.core.validateSDK(replacement.instanceID)
            })
        }
    }

    /// V9.1/K10.3: a mere notification lookup cannot discard records then save
    /// the following cursor. A later resolver failure preserves the live owner.
    @Test func cloudKitFailedNotificationLookupKeepsOwnerRecordsCursorAndPending() async throws {
        try await withCloudKitPendingFixture { f in
            let engine = try await f.engine()
            let guardBefore = try await f.core.currentGuard()
            let pending = await engine.pendingRecordZoneChanges()
            let failure = CloudTaskOriginalFailure(), gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.failure(failure), gate: gate)
            let cursor = Data([0x72, 0x34, 0x01])
            try await withCloudTask(gates: [gate], operation: {
                do { try await f.adapter.platformAccountChanged(); Issue.record("Resolver failure was lost") }
                catch { #expect((error as? CloudTaskOriginalFailure) === failure) }
            }, whileRunning: {
                try await gate.waitForArrival()
                try await f.core.validateSDK(engine.instanceID)
                await engine.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "recordsDuringNotificationLookup")
                try await expectAppliedCloudIncoming(f)
                await engine.emitRetained(.stateUpdate(cursor), label: "cursorDuringNotificationLookup")
                let advanced = try await f.core.durableSnapshot()
                #expect(advanced.serialization == cursor)
                try await expectAppliedCloudIncoming(f)
                await gate.open()
            })
            #expect(try await f.core.currentGuard() == guardBefore)
            #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(await engine.pendingRecordZoneChanges() == pending)
            try await f.adapter.requestSend(zones: [f.zone])
            let name = try #require(f.names.first)
            #expect(try await f.core.entityFacts(name: name)?.ackedRev == 3)
        }
    }

    /// V9.1/A12/P3: an SDK account event loses SDK state even when the identity
    /// resolver fails; it keeps the verified owner and restores usable pending.
    @Test func cloudKitFailedSDKLookupKeepsOwnerAndMarksActualStateLoss() async throws {
        try await withCloudKitPendingFixture { f in
            let engine = try await f.engine()
            let guardBefore = try await f.core.currentGuard()
            let pending = await engine.pendingRecordZoneChanges()
            let updates = try await f.core.emittedUpdateIDs()
            let name = try #require(f.names.first)
            let original = try #require(try await f.core.entityFacts(name: name))
            let call = try await f.core.beginFetch(sdk: engine.instanceID, zones: [f.zone])
            await engine.emitRetained(.willFetch(groupID: "before-account-event", reason: .manual), label: "fetchBeforeAccountEvent")
            await engine.emitRetained(.willFetchZone(zone: f.zone), label: "zoneBeforeAccountEvent")
            let failure = CloudTaskOriginalFailure(), gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.failure(failure), gate: gate)
            try await withCloudTask(gates: [gate], operation: {
                await engine.signInWithUnresolvedIdentity()
            }, whileRunning: {
                try await gate.waitForArrival()
                try await f.core.validateSDK(engine.instanceID)
                await gate.open()
            })
            #expect((await f.adapter.lastFailure as? CloudTaskOriginalFailure) === failure)
            #expect(try await f.core.currentGuard() == guardBefore)
            #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(await engine.pendingRecordZoneChanges() == pending)
            #expect(try await f.core.emittedUpdateIDs().count > updates.count)
            await engine.emitRetained(.didFetchZone(zone: f.zone, error: nil), label: "oldZoneAfterAccountEvent")
            await engine.emitRetained(.didFetch(groupID: "before-account-event", reason: .manual), label: "oldFetchAfterAccountEvent")
            #expect(!(try await f.core.finishFetch(sdk: engine.instanceID, id: call)))
            await engine.appendFetchScripts([.manual(fetchedZones: [], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: [f.zone])
            #expect(!(try await f.core.fetchDrains()).contains { $0.zone == f.zone && !$0.fetched })
            try await f.adapter.requestSend(zones: [f.zone])
            let blocked = try #require(try await f.core.entityFacts(name: name))
            #expect(blocked.ackedRev == original.ackedRev)
            #expect(blocked.localRev == original.localRev)
            #expect(blocked.inFlight == original.inFlight)
            #expect(blocked.mark == original.mark)
            #expect(await engine.pendingRecordZoneChanges() == pending)

            // A real current-point collection fetch discharges verification.
            let collectionDrainsBefore = try await f.core.fetchDrains()
            await engine.appendFetchScripts([.manual(fetchedZones: [f.zone], records: [], group: nil)])
            try await f.adapter.requestFetch(zones: [f.zone])
            try await f.adapter.waitForIdle()
            #expect((try await f.core.fetchDrains()).dropFirst(collectionDrainsBefore.count).contains {
                $0.zone == f.zone && $0.fetched
            })

            // R6 skips root confirmation while discharging that marker.
            let rootDrainsBefore = try await f.core.fetchDrains()
            await engine.appendFetchScripts([.manual(fetchedZones: ["meguri-root"], records: [f.root], group: nil)])
            try await f.adapter.requestFetch(zones: ["meguri-root"])
            try await f.adapter.waitForIdle()
            #expect((try await f.core.fetchDrains()).dropFirst(rootDrainsBefore.count).contains {
                $0.zone == "meguri-root" && $0.fetched
            })
            try await f.adapter.requestSend(zones: [f.zone])
            #expect(try await f.core.entityFacts(name: name)?.ackedRev == 3)
        }
    }

    /// V9.1: ReadFailed preserves no proof for a previously unverified owner.
    @Test func cloudKitInitialLookupFailureStaysBlockedUntilSuccessfulRecovery() async throws {
        try await withCloudKitFixture { f in
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            let count = await f.factory.engines().count
            let failure = CloudTaskOriginalFailure()
            await f.factory.appendAccountRead(.failure(failure))
            do { try await f.adapter.start(automaticallySync: false); Issue.record("Initial failure started an SDK") }
            catch { #expect((error as? CloudTaskOriginalFailure) === failure) }
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.factory.engines().count == count)
            do { _ = try await f.core.registerSDK(automatic: false); Issue.record("Initial failure verified the owner") }
            catch { }
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            try await f.adapter.platformAccountChanged()
            #expect(await f.factory.engines().count == count + 1)
            let recovered = try await f.engine()
            try await f.core.validateSDK(recovered.instanceID)
        }
    }

    /// V9.1/V3.4: actual sign-out/switch evidence revokes a pending lookup now.
    @Test(arguments: [true, false])
    func cloudKitActualAccountChangeDuringLookupCannotBeUndone(signedOut: Bool) async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let engine = try await f.engine()
            let before = try await f.core.currentGuard()
            let gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))), gate: gate)
            try await withCloudTask(gates: [gate], operation: {
                do { try await f.adapter.platformAccountChanged(); Issue.record("Stale lookup restored a changed account") }
                catch { }
            }, whileRunning: {
                try await gate.waitForArrival()
                await engine.emitRetained(.accountChanged(change: signedOut ? .signOut : .switchAccounts,
                                                         identity: nil), label: "actualAccountChange")
                try await f.adapter.waitForIdle()
                #expect(try await f.core.currentGuard() != before)
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
                await gate.open()
            })
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
        }
    }

    /// V3.4.
    @Test func cloudKitIdentityReadValidatesOriginAtomicallyAndAtCompletion() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let read = try await f.core.beginIdentityRead(transition: true, originSDK: old.instanceID)
            try await f.core.retireSDK(old.instanceID, policyStop: false)
            #expect(!(try await f.core.completeIdentityRead(id: read, status: .available, identity: Data("synthetic-account-B".utf8))))
            try await f.adapter.configureAutomatic(true)
            let replacement = try await f.engine()
            let fresh = try await f.core.beginIdentityRead(transition: true, originSDK: replacement.instanceID)
            let updates = try await f.core.emittedUpdateIDs()
            let guardBefore = try await f.core.currentGuard()
            do {
                _ = try await f.core.beginIdentityRead(transition: true, originSDK: old.instanceID)
                Issue.record("Stale origin captured the replacement's identity read")
            } catch { }
            #expect(try await f.core.currentGuard() == guardBefore)
            #expect(try await f.core.emittedUpdateIDs() == updates)
            #expect(try await f.core.completeIdentityRead(id: fresh, status: .available, identity: Data("synthetic-account-A".utf8)))
            #expect(!(try await f.core.completeIdentityRead(id: fresh, status: .available, identity: Data("synthetic-account-B".utf8))))
            try await f.core.validateSDK(replacement.instanceID)
        }
    }
}

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
        try await withCloudKitPendingFixture(a23: a23, keepAlive: !a23) { f in
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
            try await f.qualifyCompleteEpoch()
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
                try await Core.flushReviewForBackground()
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
        try await withCloudKitPendingFixture(a23: a23) { f in
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
            try await f.qualifyCompleteEpoch()
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
            try await f.qualifyCompleteEpoch()
            let engine = try await f.engine()
            let gate = CloudCallbackGate()
            await f.probe.pauseFetched(gate)
            let old = try await f.core.durableSnapshot()
            let state = Data([0xff, 0x00, 0x73, 0x64, 0x6b, 0x81])
            try await withCloudTask(gates: [gate], operation: {
                await engine.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "fetched")
            }, whileRunning: {
                try await gate.waitForArrival()
                try await expectAppliedCloudIncoming(f)
                try await withCloudTask(gates: [gate], operation: {
                    await engine.emitRetained(.stateUpdate(state), label: "state")
                }, whileRunning: {
                    let snapshot = try await f.core.durableSnapshot()
                    #expect(snapshot.serialization == old.serialization)
                    #expect(!(await engine.recordedTrace()).contains(.eventEntered("state")))
                    await gate.open()
                })
            })
            let committed = try await f.core.durableSnapshot()
            #expect(committed.serialization == state)
            try await expectAppliedCloudIncoming(f)
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
        try await withCloudKitFixture(keepAlive: true) { f in
            try await f.qualifyCompleteEpoch()
            try await f.armAdapterFaults(failInbox: failInbox, failState: !failInbox)
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            await engine.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "fetched")
            await engine.emitRetained(.stateUpdate(Data([0x6e, 0x65, 0x77])), label: "state")
            try await f.adapter.waitForIdle()
            let after = try await f.core.durableSnapshot()
            let originalFailure = try #require(await f.adapter.lastFailure as? BridgeError)
            #expect(originalFailure != .cloudSDKCallbackRejected)
            #expect(after.serialization == before.serialization)
            #expect(after.trackingStopped)
            #expect(!after.inboxNames.contains(f.incoming.name))
            if failInbox {
                #expect(try await f.core.entityFacts(name: f.incoming.name) == nil)
            } else {
                try await expectAppliedCloudIncoming(f)
            }
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
            #expect(await f.adapter.lastFailure as? BridgeError == originalFailure)
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
            let allFetched = drains.allSatisfy { $0.fetched }
            #expect(allFetched)
            let trace = await engine.recordedTrace()
            #expect(trace.filter { if case .fetchEntered = $0 { true } else { false } }.count == 2)
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    /// Q11 O13/P21 fetch steps: no-account short circuits on both calls end
    /// Sync Now waiting, after persistence, with no fabricated FetchDrained.
    @Test func cloudKitBothFetchesSkippedNeverClaimDrainOrSuccess() async throws {
        try await withCloudKitFixture { f in
            try await f.qualifyCompleteEpoch()
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

    /// Q11/P3/P12.3/K12.5/V26.1: a root will or did before signIn proves
    /// no post-point root fetch. The original record-ID read's unknownItem
    /// must isolate active_missing, with no root drain or absence completion.
    @Test(arguments: [false, true])
    func cloudKitManualSameIdentityNoStatePointRejectsEarlierRootFetch(afterRootDid: Bool) async throws {
        let transport = CloudKitMissingActiveTransport()
        try await withCloudKitFixture(assetReadTransport: transport) { f in
            // Sign-in carries its identity directly; the later missing Active
            // reply needs one actual provider proof before K12.5 isolation.
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-A".utf8))))
            let engine = try await f.engine()
            let before = try await f.core.fetchDrains()
            let guardBefore = try await f.core.currentGuard()
            #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
            let gate = CloudCallbackGate()
            let scope = ["meguri-root", f.zone]
            await engine.appendFetchScripts([
                .manualPaused(fetchedZones: scope, records: [], pauseZone: "meguri-root",
                              afterDid: afterRootDid, gate: gate),
                .shortCircuited,
            ])
            try await withCloudTask(gates: [gate], operation: {
                try await f.adapter.requestFetch(zones: scope)
            }, whileRunning: {
                try await gate.waitForArrival()
                let trace = await engine.recordedTrace()
                #expect(trace.contains(.eventReturned("willFetchZone")))
                #expect(trace.contains(.eventReturned("didFetchZone")) == afterRootDid)
                await engine.signInSameSyntheticIdentity()
                #expect(try await f.core.currentGuard() == guardBefore)
                try await f.core.validateSDK(engine.instanceID)
                #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                await gate.open()
            })
            try await f.adapter.waitForIdle()
            let active = CloudKitMissingActiveTransport.active
            try #require(transport.recordedTrace() == [
                .prepared(active), .enqueued(active), .unknownItem(active), .replied(.missing),
            ])
            let drains = Array((try await f.core.fetchDrains()).dropFirst(before.count))
            #expect(!drains.contains { $0.zone == "meguri-root" })
            #expect(!(try await f.core.status()).syncingCollection)
            let after = try await f.core.durableSnapshot()
            #expect(!after.absencePassCompleted)
            // adapter_snapshot_locked serializes the real stored binding as
            // the first tuple, with BindingFacts last; no fake status trace.
            let content = try #require(JSONSerialization.jsonObject(with: after.trackingContent) as? [Any])
            let binding = try #require(content.first as? [Any])
            let facts = try #require(binding.last as? [String: Any])
            #expect(facts["state"] as? [String: String] == ["Isolated": "ActiveMissing"])
            #expect(try await f.core.currentGuard() != guardBefore)
            await #expect(throws: (any Error).self) { try await f.core.validateSDK(engine.instanceID) }
            let trace = await engine.recordedTrace()
            let options = trace.compactMap { if case .fetchOptions(let value) = $0 { value } else { nil } }
            #expect(options.count == 1 && options.allSatisfy { $0.zones == scope })
            let signIn = try #require(trace.firstIndex(of: .eventReturned("sameIdentitySignIn")))
            let rootEvent = try #require(trace.firstIndex(of: .eventReturned(afterRootDid ? "didFetchZone" : "willFetchZone")))
            let returned = try #require(trace.firstIndex { if case .fetchReturned = $0 { true } else { false } })
            #expect(rootEvent < signIn && signIn < returned)
            let callbacks = trace.compactMap { entry -> String? in
                guard case .eventReturned(let name) = entry,
                      ["willFetch", "willFetchZone", "didFetchZone", "didFetch", "sameIdentitySignIn"].contains(name)
                else { return nil }
                return name
            }
            let rootCallbacks = afterRootDid
                ? ["willFetchZone", "didFetchZone", "sameIdentitySignIn"]
                : ["willFetchZone", "sameIdentitySignIn", "didFetchZone"]
            #expect(callbacks == ["willFetch"] + rootCallbacks + ["willFetchZone", "didFetchZone", "didFetch"])
            #expect(await f.factory.engines().count == 1)
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    /// Q11/P3/K17.4: a real no-state creation starts the absence pass, whereas
    /// a same-identity reset of a saved instance starts none. In either case a
    /// pre-point zone will followed by its original did completes no pass.
    @Test(arguments: [false, true])
    func cloudKitManualPrePointCollectionFetchCompletesNoAbsencePass(createdWithoutState: Bool) async throws {
        let savedState: Data? = createdWithoutState ? nil : Data([0x73, 0x64, 0x6b])
        try await withCloudKitFixture(savedState: savedState) { f in
            try await f.qualifyCompleteEpoch()
            let engine = try await f.engine()
            #expect(engine.restoredSerialization == savedState)
            let before = try await f.core.fetchDrains()
            let snapshotBefore = try await f.core.durableSnapshot()
            #expect(!snapshotBefore.absencePassCompleted)
            let guardBefore = try await f.core.currentGuard()
            let gate = CloudCallbackGate()
            await engine.appendFetchScripts([
                .manualPaused(fetchedZones: [f.zone], records: [f.incoming], pauseZone: f.zone,
                              afterDid: false, gate: gate),
                .shortCircuited,
            ])
            try await withCloudTask(gates: [gate], operation: {
                try await f.adapter.requestFetch(zones: [f.zone])
            }, whileRunning: {
                try await gate.waitForArrival()
                await engine.signInSameSyntheticIdentity()
                #expect(try await f.core.currentGuard() == guardBefore)
                #expect(!(try await f.core.durableSnapshot()).absencePassCompleted)
                try await f.core.validateSDK(engine.instanceID)
                await gate.open()
            })
            try await f.adapter.waitForIdle()
            #expect(try await f.core.fetchDrains() == before)
            let after = try await f.core.durableSnapshot()
            #expect(!after.absencePassCompleted)
            #expect(after.binding == snapshotBefore.binding)
            #expect(after.rootEnvelope == snapshotBefore.rootEnvelope)
            #expect(after.rootSystemFields == snapshotBefore.rootSystemFields)
            try await expectAppliedCloudIncoming(f)
            #expect(try await f.core.currentGuard() == guardBefore)
            #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
            let trace = await engine.recordedTrace()
            let will = try #require(trace.firstIndex(of: .eventReturned("willFetchZone")))
            let signIn = try #require(trace.firstIndex(of: .eventReturned("sameIdentitySignIn")))
            let records = try #require(trace.firstIndex(of: .eventReturned("fetched")))
            let did = try #require(trace.firstIndex(of: .eventReturned("didFetchZone")))
            let returned = try #require(trace.firstIndex { if case .fetchReturned = $0 { true } else { false } })
            #expect(will < signIn && signIn < records && records < did && did < returned)
            #expect(!trace.contains(.reentrantNetworkCall))
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
        try await withCloudKitPendingFixture(keepAlive: true) { f in
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

    /// Owner shutdown closes admission before retiring the original SDK.
    /// This fake exercises Rust authority and SDK callback ordering, not Apple CloudKit.
    @Test func cloudKitQuiescePreservesOriginalCallbackAuthorityUntilFinalStop() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let engine = try await f.engine()
            let originalGuard = try await f.core.currentGuard()
            let callback = CloudCallbackGate(), cancel = CloudCallbackGate()
            let cursor = Data([0x71, 0x75, 0x69, 0x65, 0x73, 0x63, 0x65])
            await engine.pauseNextCallback(callback)
            await engine.pauseCancellation(cancel)
            try await withCloudTask(gates: [callback, cancel], expectedArrivals: [callback], operation: {
                await engine.emitRetained(.stateUpdate(cursor), label: "originalClosingState")
            }, whileRunning: {
                try await callback.waitForArrival()
                try await withCloudTask(gates: [callback, cancel], expectedArrivals: [cancel], operation: {
                    // Concurrent callers join the same original transport drain.
                    async let first: Void = f.adapter.quiesce()
                    async let second: Void = f.adapter.quiesce()
                    try await first
                    try await second
                }, whileRunning: {
                    try await cancel.waitForArrival()
                    try await f.core.validateSDK(engine.instanceID)
                    #expect(try await f.core.currentGuard() == originalGuard)
                    #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                    #expect(!(await engine.recordedTrace()).contains(.released))
                    let calls = engine.acceptedNetworkCalls.value
                    do { try await f.adapter.requestSend(zones: [f.zone]); Issue.record("Quiesced send admitted") }
                    catch is CancellationError { }
                    do { try await f.adapter.requestFetch(zones: [f.zone]); Issue.record("Quiesced fetch admitted") }
                    catch is CancellationError { }
                    do { try await f.adapter.configureAutomatic(true); Issue.record("Unexpected SDK configuration") }
                    catch is CancellationError { }
                    #expect(engine.acceptedNetworkCalls.value == calls)
                    #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
                    await callback.open()
                    await cancel.open()
                })
            })
            try await f.adapter.quiesce()
            try await f.adapter.consumePendingUpdates()
            try await f.core.validateSDK(engine.instanceID)
            #expect(try await f.core.currentGuard() == originalGuard)
            #expect(try await f.core.durableSnapshot().serialization == cursor)
            #expect(!(await engine.recordedTrace()).contains(.released))
            try await f.adapter.stop()
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await engine.recordedTrace().contains(.released))
            #expect(try await f.core.currentGuard() != originalGuard)
            let retired = try await f.core.durableSnapshot()
            await engine.emitRetained(.stateUpdate(Data([0xff])), label: "retiredClosingState")
            #expect(try await f.core.durableSnapshot() == retired)
        }
    }

    /// Real Rust-issued StopEngine must return while quiesce and a required
    /// RecordExchange acknowledgement are held. The fake supplies SDK timing only.
    @Test(.timeLimit(.minutes(1)))
    func cloudKitIssuedStopDefersUntilOriginalRequiredFinalizationDrains() async throws {
        try await withNativePolicyFixture(keepAlive: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            defer { withExtendedLifetime((policy, executor)) {} }
            try await prepareCloudKitPendingFixture(f, policy: policy, executor: executor)
            let engine = try await f.engine()
            let originalGuard = try await f.core.currentGuard()
            let native = CloudCallbackGate(), cancel = CloudCallbackGate(), finalization = CloudCallbackGate()
            await engine.pauseNativeReturnedWork(native)
            await engine.pauseCancellation(cancel)
            let request = try await policy.input(.syncNowRequested(.command))
            let issued = try shutdownPolicyCommands(request)
            let exchange = try #require(issued.first { if case .runExchange = $0.action { true } else { false } })
            guard case .runExchange(let exchangeID, _) = exchange.action else { return }
            try await withCloudTask(gates: [native, cancel, finalization], expectedArrivals: [native, finalization], operation: {
                let result = await executor.execute(exchange)
                #expect(result.error != nil)
                let required = try result.outputs.flatMap(shutdownPolicyCommands)
                let record = try #require(required.first { if case .recordExchange = $0.action { true } else { false } })
                await finalization.enter()
                try await f.core.validateSDK(engine.instanceID)
                #expect(!(await engine.recordedTrace()).contains(.released))
                let acknowledged = await executor.execute(record)
                #expect(acknowledged.error == nil && !acknowledged.deferred)
                #expect(try await policy.status().lastAttempt?.exchange == exchangeID)
                try await f.core.validateSDK(engine.instanceID)
                #expect(!(await engine.recordedTrace()).contains(.released))
            }, whileRunning: {
                try await native.waitForArrival()
                try await withCloudTask(gates: [native, cancel], expectedArrivals: [cancel], operation: {
                    try await executor.quiesce()
                }, whileRunning: {
                    try await cancel.waitForArrival()
                    let output = try await policy.input(.trackingWriteFailed(noSpace: false))
                    try #require(try await f.core.currentGuard() == originalGuard)
                    let stop = try #require(try shutdownPolicyCommands(output).first { if case .stopEngine = $0.action { true } else { false } })
                    // This returns BEFORE either transport quiescence or the
                    // coordinator's required-effect drain can finish.
                    // Bound a broken implementation's wait and release its
                    // gates before joining it. Final stop here is failure-only
                    // recovery for an implementation awaiting its own close.
                    let returned = CloudPolicyReadinessDecision()
                    let watchdog = Task {
                        do { try await Task.sleep(for: .seconds(10)) } catch { return }
                        if returned.expire() {
                            await native.open(); await cancel.open(); await finalization.open()
                            try? await f.adapter.stop()
                        }
                    }
                    defer { watchdog.cancel() }
                    let result = await executor.execute(stop)
                    #expect(returned.observeReady(), "StopEngine awaited a later shutdown phase")
                    #expect(result.deferred && result.error == nil && result.outputs.isEmpty)
                    try await f.core.validateSDK(engine.instanceID)
                    #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                    #expect(!(await engine.recordedTrace()).contains(.released))
                    await native.open(); await cancel.open()
                })
                try await finalization.waitForArrival()
                try await f.core.validateSDK(engine.instanceID)
                #expect(!(await engine.recordedTrace()).contains(.released))
                let recovery = try await policy.input(.foreground)
                let probe = try #require(try shutdownPolicyCommands(recovery).first { if case .probeTracking = $0.action { true } else { false } })
                let recovered = await executor.execute(probe)
                #expect(recovered.error == nil && !recovered.deferred)
                try #require(try await f.core.currentGuard() == originalGuard)
                let updateOutput = try await policy.input(.refresh(.local))
                let update = try #require(try shutdownPolicyCommands(updateOutput).first { if case .updatePending = $0.action { true } else { false } })
                let before = Set(try await f.core.emittedUpdateIDs())
                let updated = await executor.execute(update)
                #expect(updated.error == nil && !updated.deferred)
                try await f.adapter.consumePendingUpdates()
                let emitted = Set(try await f.core.emittedUpdateIDs()).subtracting(before)
                #expect(!emitted.isEmpty)
                #expect(await engine.recordedTrace().contains { if case .pendingApplied(let id, _, _) = $0 { emitted.contains(id) } else { false } })
                try await f.core.validateSDK(engine.instanceID)
                #expect(await f.factory.engines().count == 1)
                #expect(!(await engine.recordedTrace()).contains(.released))
                await finalization.open()
            })
            // No command waited for final close. Only now may the owner retire
            // the exact deferred execution (policy stop must not bump Core's guard).
            async let first: Void = f.adapter.stop()
            async let second: Void = f.adapter.stop()
            try await first; try await second
            try await f.adapter.stop()
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            try #require(try await f.core.currentGuard() == originalGuard)
            #expect(await engine.recordedTrace().filter { $0 == .released }.count == 1)
            await #expect(throws: (any Error).self) { try await f.core.validateSDK(engine.instanceID) }
            let retired = try await f.core.durableSnapshot()
            await engine.emitRetained(.stateUpdate(Data([0xff])), label: "afterDeferredStop")
            #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            #expect(try await f.core.durableSnapshot() == retired)
        }
    }

    @Test func cloudKitIssuedStopOutsideQuiesceRetiresImmediately() async throws {
        try await withNativePolicyFixture(keepAlive: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            let engine = try await f.engine()
            let output = try await policy.input(.settings(.init(enabled: false, paused: false, automatic: false, media: false)))
            let originalGuard = try await f.core.currentGuard()
            let stop = try #require(try shutdownPolicyCommands(output).first { if case .stopEngine = $0.action { true } else { false } })
            let result = await executor.execute(stop)
            #expect(!result.deferred && result.error == nil && !result.outputs.isEmpty)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await engine.recordedTrace().filter { $0 == .released }.count == 1)
            try #require(try await f.core.currentGuard() == originalGuard)
        }
    }

    @Test(.timeLimit(.minutes(1)))
    func cloudKitIssuedStopMeetingQuiesceDuringDrainRetainsExactExecution() async throws {
        try await withNativePolicyFixture(keepAlive: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            let engine = try await f.engine()
            let originalGuard = try await f.core.currentGuard()
            let media = CloudCallbackGate(), cancel = CloudCallbackGate()
            await f.adapter.setOriginalMediaDrain { _ in await media.enter() }
            await engine.pauseCancellation(cancel)
            let output = try await policy.input(.trackingWriteFailed(noSpace: false))
            try #require(try await f.core.currentGuard() == originalGuard)
            let stop = try #require(try shutdownPolicyCommands(output).first { if case .stopEngine = $0.action { true } else { false } })
            try await withCloudTask(gates: [media, cancel], expectedArrivals: [media], operation: {
                let result = await executor.execute(stop)
                #expect(result.deferred && result.error == nil && result.outputs.isEmpty)
            }, whileRunning: {
                try await media.waitForArrival()
                try await withCloudTask(gates: [media, cancel], expectedArrivals: [cancel], operation: {
                    try await executor.quiesce()
                }, whileRunning: {
                    try await cancel.waitForArrival()
                    try await f.core.validateSDK(engine.instanceID)
                    #expect(!(await engine.recordedTrace()).contains(.released))
                    let cursor = Data([0x64, 0x72, 0x61, 0x69, 0x6e])
                    await engine.emitRetained(.stateUpdate(cursor), label: "duringTransferredStop")
                    #expect(try await f.core.durableSnapshot().serialization == cursor)
                    await media.open(); await cancel.open()
                })
            })
            try await f.core.validateSDK(engine.instanceID)
            #expect(!(await engine.recordedTrace()).contains(.released))
            try await f.adapter.stop()
            try #require(try await f.core.currentGuard() == originalGuard)
            #expect(await engine.recordedTrace().filter { $0 == .released }.count == 1)
        }
    }

    /// A real authority rejection must remain an error on explicit close and
    /// retry; neither may replace the retained command with an unguarded stop.
    @Test func cloudKitDeferredStopRetirementFailureRetainsOriginalForExplicitRetry() async throws {
        try await withNativePolicyFixture(keepAlive: true) { f in
            let (policy, executor) = try await nativePolicy(f)
            let engine = try await f.engine()
            try await executor.quiesce()
            let output = try await policy.input(.settings(.init(enabled: false, paused: false, automatic: false, media: false)))
            let stop = try #require(try shutdownPolicyCommands(output).first { if case .stopEngine = $0.action { true } else { false } })
            let deferred = await executor.execute(stop)
            #expect(deferred.deferred && deferred.error == nil && deferred.outputs.isEmpty)
            // Actual Core token transition revokes the issued stop. It cannot
            // be repaired by native inferring a new current SDK/control guard.
            let beforeRevocation = try await f.core.currentGuard()
            try await f.core.setPaused(false)
            let revoked = try await f.core.currentGuard()
            try #require(revoked != beforeRevocation)
            for _ in 0..<2 {
                do { try await f.adapter.stop(); Issue.record("Unexpected stop result") }
                catch BridgeError.cloudPolicy(.staleCommand) { }
                #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                #expect(!(await engine.recordedTrace()).contains(.released))
                #expect(try await f.core.currentGuard() == revoked)
                #expect(await f.adapter.lastFailure != nil)
            }
        }
    }

    @Test func cloudKitQuiescedCallbackFailureCannotRetireBeforeFinalStop() async throws {
        try await withCloudKitFixture(keepAlive: true, failState: true) { f in
            let engine = try await f.engine()
            try await f.adapter.quiesce()
            await engine.emitRetained(.stateUpdate(Data([0x01])), label: "failedQuiescedState")
            #expect(await f.adapter.lastFailure != nil)
            #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(!(await engine.recordedTrace()).contains(.released))
            try await f.adapter.stop()
            #expect(await engine.recordedTrace().filter { $0 == .released }.count == 1)
            #expect(await f.adapter.lastFailure != nil)
        }
    }

    /// Hold the delegate immediately after successful original validation,
    /// then install the quiesce fence before it can offer any new SDK work.
    @Test(arguments: [false, true])
    func cloudKitDelegateNewWorkRechecksQuiesceAfterValidation(batch: Bool) async throws {
        let validated = CloudCallbackGate()
        try await withCloudKitFixture(keepAlive: true, delegateWorkValidated: { await validated.enter() }) { f in
            let engine = try await f.engine()
            try await withCloudTask(gates: [validated], operation: {
                if batch { #expect(await engine.requestRetainedBatch(zone: f.zone) == nil) }
                else {
                    let options = await engine.scheduledFetchOptions(.init(zones: ["meguri-root", f.zone], priorityZones: [f.zone], groupID: "closing-options"))
                    #expect(options.zones.isEmpty && options.priorityZones.isEmpty && options.groupID == "closing-options")
                }
            }, whileRunning: {
                try await validated.waitForArrival()
                try await f.adapter.quiesce()
                try await f.core.validateSDK(engine.instanceID)
                await validated.open()
            })
            #expect(!(await engine.recordedTrace()).contains { if case .handed = $0 { true } else { false } })
            try await f.adapter.stop()
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
        try await withCloudKitPendingFixture(keepAlive: true) { f in
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

private func shutdownPolicyCommands(_ output: CloudPolicyOutput) throws -> [CloudPolicyCommand] {
    guard case .commands(let commands) = output else { throw BridgeError.internal("Unexpected shutdown policy output") }
    return commands
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

extension EventTests {
    /// R3/K2/K13.1: SDK deletion is persisted before any later cursor delivery.
    @Test func cloudKitDeletedActiveCannotBeReconfirmedBySuccessfulDrainOrReopen() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            // A fetched Active deletion requires a post-loss account read.
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-A".utf8))))
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            #expect(before.rootEnvelope != nil)
            await engine.emitRetained(.willFetch(groupID: "delete-root", reason: .scheduled), label: "willDeleteRoot")
            await engine.emitRetained(.willFetchZone(zone: "meguri-root"), label: "willDeleteRootZone")
            await engine.emitRetained(.fetched(zone: "meguri-root", records: [], deleted: ["active"]), label: "deletedActive")
            await engine.emitRetained(.didFetchZone(zone: "meguri-root", error: nil), label: "deletedRootZoneDrained")
            await engine.emitRetained(.didFetch(groupID: "delete-root", reason: .scheduled), label: "deletedRootDrained")
            await engine.emitRetained(.stateUpdate(Data([0xde, 0x1e])), label: "cursorAfterDeletion")
            try await f.adapter.waitForIdle()
            let deletionFailure = await f.adapter.lastFailure
            #expect(deletionFailure == nil, "Fetched Active deletion completion: \(String(describing: deletionFailure))")
            let deleted = try await f.core.durableSnapshot()
            #expect(deleted.rootEnvelope == nil && deleted.rootSystemFields == nil)
            #expect(deleted.serialization == before.serialization)
            #expect(deleted.trackingContent != before.trackingContent)
            #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("deletedActive")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            #expect(returned < cancelled)

            try await f.collection.close()
            let closed = try await f.core.durableSnapshot()
            let location = CoreBridge.CollectionLocation(
                collectionDirectory: f.directory.appending(path: "collection"),
                backupsDirectory: f.directory.appending(path: "backups"),
                appGroupSnapshot: f.directory.appending(path: "widget.snapshot"))
            try await f.collection.open(location, randomSeed: 115)
            let reopened = CoreCloudWorkflowService()
            let persisted = try await reopened.durableSnapshot()
            #expect(persisted.binding == deleted.binding)
            #expect(persisted.rootEnvelope == nil)
            do { _ = try await reopened.registerSDK(automatic: false); Issue.record("Deleted root admitted a new SDK") }
            catch { }
            await engine.emitRetained(.fetched(zone: "meguri-root", records: [f.root], deleted: []), label: "oldRootAfterReopen")
            #expect(try await f.core.durableSnapshot() == closed)
            #expect(try await reopened.durableSnapshot() == persisted)
        }
    }

    #if DEBUG
    /// K2/K12.5/P21/V23: an already validated drain crosses actual fresh
    /// account-proof acceptance, which retires its original SDK authority.
    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func cloudKitValidatedRootZoneDrainAfterFreshIdentityProofKeepsDeletionAuthoritative(wholeFetch: Bool) async throws {
        try await withCloudKitFixture(keepAlive: true, appOwnsAccountObservation: true) { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            try #require(before.rootEnvelope != nil)
            let originalGuard = try await f.core.currentGuard()
            let reads = await f.factory.resolvedAccountReadCount()
            await engine.emitRetained(.willFetch(groupID: "delete-root", reason: .scheduled), label: "willDeleteRoot")
            await engine.emitRetained(.willFetchZone(zone: "meguri-root"), label: "willDeleteRootZone")
            await engine.emitRetained(.fetched(zone: "meguri-root", records: [], deleted: ["active"]), label: "deletedActive")
            try #require(await f.adapter.lastFailure == nil, "Active deletion must stage without a setup failure")

            let validated = CloudCallbackGate()
            let lateEvent: CloudEngineEvent = wholeFetch
                ? .didFetch(groupID: "delete-root", reason: .scheduled)
                : .didFetchZone(zone: "meguri-root", error: nil)
            await f.adapter.setValidatedEventHook((event: lateEvent, action: { await validated.enter() }))
            do {
                try await withCloudTask(gates: [validated], operation: {
                    await engine.emitRetained(lateEvent, label: "validatedRootZoneDrained")
                }, whileRunning: {
                    try await validated.waitForArrival()
                    try #require(try await f.core.currentGuard() == originalGuard)
                    try #require(await f.factory.resolvedAccountReadCount() == reads,
                                 "App-owned loss handling must not start an automatic SDK account lookup")
                    try #require(await f.adapter.lastFailure == nil)
                    let heldTrace = await engine.recordedTrace()
                    try #require(heldTrace.contains(.eventReturned("deletedActive")))
                    try #require(!heldTrace.contains(.eventReturned("validatedRootZoneDrained")))

                    // Public adapter path resolves an actual SDK account result
                    // and completes Core's fresh read while the drain is held.
                    await f.factory.appendAccountRead(.success(.init(status: .available,
                        identity: Data("synthetic-account-A".utf8))))
                    try await f.adapter.refreshPlatformIdentity()
                    try #require(await f.factory.resolvedAccountReadCount() == reads + 1)
                    try #require(try await f.core.currentGuard() != originalGuard,
                                 "Actual fresh proof must retire the original SDK before the drain resumes")
                    let accepted = try await f.core.durableSnapshot()
                    try #require(accepted.rootEnvelope == nil && accepted.rootSystemFields == nil)
                    try #require(accepted.serialization == before.serialization)
                    await validated.open()
                })
            } catch {
                await f.adapter.setValidatedEventHook(nil)
                throw error
            }
            await f.adapter.setValidatedEventHook(nil)
            await engine.emitRetained(.didFetch(groupID: "delete-root", reason: .scheduled), label: "deletedRootDrained")
            await engine.emitRetained(.stateUpdate(Data([0xde, 0x1e])), label: "cursorAfterDeletion")
            try await f.adapter.waitForIdle()
            let deletionFailure = await f.adapter.lastFailure
            #expect(deletionFailure == nil, "Validated drain after fresh identity proof: \(String(describing: deletionFailure))")
            let deleted = try await f.core.durableSnapshot()
            #expect(deleted.rootEnvelope == nil && deleted.rootSystemFields == nil)
            #expect(deleted.serialization == before.serialization)
            #expect(deleted.trackingContent != before.trackingContent)
            #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.factory.engines().count == 1)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("deletedActive")))
            let drained = try #require(trace.firstIndex(of: .eventReturned("validatedRootZoneDrained")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            #expect(returned < drained && drained < cancelled)
            #expect(trace.contains(.cancelReturned) && trace.contains(.released))

            try await f.collection.close()
            let closed = try await f.core.durableSnapshot()
            let location = CoreBridge.CollectionLocation(
                collectionDirectory: f.directory.appending(path: "collection"),
                backupsDirectory: f.directory.appending(path: "backups"),
                appGroupSnapshot: f.directory.appending(path: "widget.snapshot"))
            try await f.collection.open(location, randomSeed: 115)
            let reopened = CoreCloudWorkflowService()
            let persisted = try await reopened.durableSnapshot()
            #expect(persisted.binding == deleted.binding)
            #expect(persisted.rootEnvelope == nil)
            do { _ = try await reopened.registerSDK(automatic: false); Issue.record("Deleted root admitted a new SDK") }
            catch { }
            await engine.emitRetained(.fetched(zone: "meguri-root", records: [f.root], deleted: []), label: "oldRootAfterReopen")
            #expect(try await f.core.durableSnapshot() == closed)
            #expect(try await reopened.durableSnapshot() == persisted)
        }
    }
    #endif

    /// R3/K10.3: equality includes same-name content and pending root facts.
    @Test func cloudKitSnapshotIncludesInboxContentPendingRootAndStaleDeliveries() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            try await f.qualifyCompleteEpoch()
            let old = try await f.engine()
            await old.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: []), label: "firstInbox")
            let first = try await f.core.durableSnapshot()
            let different = CloudRecord(name: f.incoming.name, envelope: f.incoming.envelope,
                                        systemFields: Data([0x72, 0x33, 0x01]))
            await old.emitRetained(.fetched(zone: f.zone, records: [different], deleted: []), label: "sameNameDifferentInbox")
            let second = try await f.core.durableSnapshot()
            #expect(first.inboxNames == second.inboxNames)
            // The already-applied erev is stale, including different system
            // fields. Redelivery cannot replace its acknowledged version.
            #expect(first == second)
            try await expectAppliedCloudIncoming(f)
            let root = CloudRecord(name: f.root.name, envelope: f.root.envelope, systemFields: Data([0x72, 0x33, 0x02]),
                                   recordType: f.root.recordType, zone: f.root.zone)
            await old.emitRetained(.fetched(zone: "meguri-root", records: [root], deleted: []), label: "pendingRoot")
            let third = try await f.core.durableSnapshot()
            #expect(second.rootEnvelope == third.rootEnvelope && second.rootSystemFields == third.rootSystemFields)
            #expect(second.trackingContent != third.trackingContent && second != third)
            try await f.adapter.configureAutomatic(true)
            let stable = try await f.core.durableSnapshot()
            await old.emitRetained(.fetched(zone: f.zone, records: [f.incoming], deleted: [f.incoming.name]), label: "staleInboxContent")
            await old.emitRetained(.fetched(zone: "meguri-root", records: [], deleted: ["active"]), label: "staleRootContent")
            #expect(try await f.core.durableSnapshot() == stable)
        }
    }

    /// R3/P12.1/.6/.7: real core joins and records a cancellation once.
    @Test func cloudKitCoreExchangeJoinAndCancellationKeepActualSelection() async throws {
        try await withCloudKitPendingFixture { f in
            let sdk = try await f.engine().instanceID
            let id = try await f.core.beginExchange(sdk: sdk)
            #expect(try await f.core.beginExchange(sdk: sdk) == id)
            let ended = try await f.core.completeExchange(sdk: sdk, id: id, fetched: false, cancelled: true)
            #expect(ended.collection == .cancelled)
            #expect(!ended.selectedNames.isEmpty)
            #expect(ended.ackedNames.isEmpty && ended.outstandingNames == ended.selectedNames)
            #expect(try await f.core.completeExchange(sdk: sdk, id: id, fetched: true) == ended)
            #expect(try await f.core.exchangeResult(id: id) == ended)
            #expect(!(try await f.core.status()).syncingCollection)
            let next = try await f.core.beginExchange(sdk: sdk)
            #expect(next != id)
            _ = try await f.core.completeExchange(sdk: sdk, id: next, fetched: false, cancelled: true)
        }
    }

    /// R3/V9.1/A19: stale completions cannot restore a transitioned owner.
    @Test func cloudKitIdentityTransitionRequiresFreshMatchingRead() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let oldRead = try await f.core.beginIdentityRead()
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            #expect(!(try await f.core.completeIdentityRead(id: oldRead, status: .available, identity: Data("synthetic-account-A".utf8))))
            do { try await f.adapter.start(automaticallySync: false); Issue.record("Old platform identity verified a recreated SDK") }
            catch { }
            #expect(await old.requestRetainedBatch(zone: f.zone) == nil)
            #expect(await f.factory.engines().count == 1)
            let fresh = try await f.core.beginIdentityRead()
            #expect(try await f.core.completeIdentityRead(id: fresh, status: .available, identity: Data("synthetic-account-A".utf8)))
            #expect(!(try await f.core.completeIdentityRead(id: fresh, status: .available, identity: Data("synthetic-account-A".utf8))))
            try await f.adapter.start(automaticallySync: false)
            #expect(await f.factory.engines().count == 2)
            let readBeforeClose = try await f.core.beginIdentityRead()
            try await f.adapter.stop()
            try await f.collection.close()
            let closed = try await f.core.durableSnapshot()
            #expect(!(try await f.core.completeIdentityRead(id: readBeforeClose, status: .available, identity: Data("synthetic-account-A".utf8))))
            #expect(try await f.core.durableSnapshot() == closed)
        }
    }
}

/// SDK operation errors only. All selection, terminal results and later sends
/// still pass through the production bridge and Runtime.
private actor CloudThrowingFetchEngine: CloudKitEngine {
    nonisolated let instanceID: String
    let sdk: CloudKitEngineFake
    private var throwFetch = false
    private var fetchGate: CloudCallbackGate?
    private var activeFetchGates: [CloudCallbackGate] = []
    private let networkLifetime = CloudKitNetworkLifetime()
    func pauseNextFetch(_ gate: CloudCallbackGate) { fetchGate = gate }
    init(sdk: CloudKitEngineFake) { self.sdk = sdk; instanceID = sdk.instanceID }
    func failNextFetch() { throwFetch = true }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] { await sdk.pendingRecordZoneChanges() }
    func pendingDatabaseChanges() async -> Set<String> { await sdk.pendingDatabaseChanges() }
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws -> CloudPendingUpdate { await sdk.applyPendingUpdate(update) }
    func removePendingDatabaseChanges() async { await sdk.removePendingDatabaseChanges() }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue {
        try await sdk.prepareSend(zones: zones)
    }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        let lifetime = networkLifetime
        let generation = lifetime.authority()
        let prepared = try await sdk.prepareFetch(options: options)
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { await self.cancelFetchSDKCalls() }) {
                try await self.invokePreparedFetch(prepared, generation: generation, prepare: prepare, validate: validate)
            }
        }
    }
    private func invokePreparedFetch(_ prepared: CloudKitNetworkEnqueue, generation: UInt64,
                                     prepare: @escaping @Sendable () async throws -> Void,
                                     validate: @escaping @Sendable () throws -> Void) async throws {
        try await waitForFetchGate()
        let lifetime = networkLifetime
        if throwFetch {
            throwFetch = false
            try await prepare()
            try lifetime.enqueue(generation: generation, validate: validate, begin: {})
            throw CloudTaskOriginalFailure()
        }
        let operation = try prepared.enqueue(prepare: prepare, validate: {
            try lifetime.enqueue(generation: generation, validate: validate, begin: {})
        })
        try await operation.drain()
    }
    private func waitForFetchGate() async throws {
        if let gate = fetchGate {
            fetchGate = nil
            activeFetchGates.append(gate)
            await gate.enter()
            activeFetchGates.removeAll { $0 === gate }
            try Task.checkCancellation()
        }
    }
    func sendChanges(zones: [String]) async throws { try await sdk.sendChanges(zones: zones) }
    func fetchChanges(options: CloudFetchOptions) async throws {
        try await waitForFetchGate()
        if throwFetch { throwFetch = false; throw CloudTaskOriginalFailure() }
        try await sdk.fetchChanges(options: options)
    }
    private func cancelFetchSDKCalls() async {
        for gate in activeFetchGates { await gate.open() }
        await sdk.cancelOperations()
    }
    func cancelOperations() async {
        let operations = networkLifetime.beginCancellation()
        defer { networkLifetime.endCancellation() }
        await cancelFetchSDKCalls()
        for operation in operations { await operation.cancelAndDrain() }
    }
    func release() async {
        networkLifetime.retire()
        await cancelOperations()
        await sdk.release()
    }
}
private actor CloudThrowingFetchFactory: CloudKitEngineFactory {
    private var engine: CloudThrowingFetchEngine?
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async -> any CloudKitEngine {
        let result = CloudThrowingFetchEngine(sdk: CloudKitEngineFake(configuration: configuration, delegate: delegate, a23: true, keepAlive: false))
        engine = result
        return result
    }
    func made() -> CloudThrowingFetchEngine? { engine }
}
extension EventTests {
    @Test func cloudKitThrownFetchTerminatesDurablyAndNextRequestRuns() async throws {
        try await withCloudKitPendingFixture { f in
            try await f.adapter.stop()
            let factory = CloudThrowingFetchFactory()
            let adapter = CloudKitSyncAdapter(core: f.core, engineFactory: factory)
            do {
                try await adapter.start(automaticallySync: false)
                let sdk = try #require(await factory.made())
                await sdk.failNextFetch()
                let failedID = try await f.core.beginExchange(sdk: sdk.instanceID)
                do { _ = try await adapter.syncNow(); Issue.record("SDK fetch error was lost") }
                catch is CloudTaskOriginalFailure { }
                #expect(!(try await f.core.status()).syncingCollection)
                let failed = try #require(try await f.core.exchangeResult(id: failedID))
                #expect(failed.collection == .failed)
                #expect(!failed.selectedNames.isEmpty && failed.outstandingNames == failed.selectedNames && failed.ackedNames.isEmpty)
                let failedContent = try await f.core.durableSnapshot().trackingContent
                let completed = try await adapter.syncNow()
                #expect(completed.collection == .succeeded)
                #expect(completed.selectedNames == completed.ackedNames && completed.outstandingNames.isEmpty)
                #expect(try await f.core.exchangeResult(id: completed.id) == completed)
                #expect(try await f.core.exchangeResult(id: failedID) == failed)
                #expect(try await f.core.durableSnapshot().trackingContent != failedContent)
                try await adapter.stop()
            } catch { try? await adapter.stop(); throw error }
        }
    }
}

extension EventTests {
    /// K2/K4.1: the fixture, SDK registration and real FFI builder agree.
    @Test func cloudKitRegisteredZoneUsesCanonicalEpochText() async throws {
        try await withCloudKitPendingFixture { f in
            #expect(f.zone == "c-ojzhe4tsojzhe4tsojzhe4tsoi")
            let sdk = try await f.engine()
            #expect(await sdk.requestRetainedBatch(zone: "c-72727272727272727272727272727272") == nil)
            let batch = try #require(await sdk.requestRetainedBatch(zone: "c-ojzhe4tsojzhe4tsojzhe4tsoi"))
            #expect(!batch.saves.isEmpty)
        }
    }
}

private actor CloudExchangeJoinProbe {
    private var count = 0
    let two = CloudCallbackGate()
    func didJoin() async { count += 1; if count == 2 { await two.open() } }
    func waitForTwo() async { await two.enter() }
}
extension EventTests {
    /// P12.1/.6/.7: both callers own the same task until a controlled SDK return.
    @Test func cloudKitConcurrentCallersJoinAndCancelOneOwnedExchange() async throws {
        try await withCloudKitPendingFixture { f in
            try await f.adapter.stop()
            let factory = CloudThrowingFetchFactory(), joins = CloudExchangeJoinProbe()
            let adapter = CloudKitSyncAdapter(core: f.core, engineFactory: factory,
                                             exchangeDidJoin: { await joins.didJoin() })
            try await adapter.start(automaticallySync: false)
            let sdk = try #require(await factory.made())
            let gate = CloudCallbackGate()
            await sdk.pauseNextFetch(gate)
            let first = Task {
                do {
                    let result = try await adapter.syncNow()
                    _ = await gate.operationSucceeded()
                    return result
                } catch { await gate.abort(error); throw error }
            }
            do {
                try await gate.waitForArrival()
                let id = try await f.core.beginExchange(sdk: sdk.instanceID)
                let second = Task { try await adapter.syncNow() }
                await joins.waitForTwo()
                await adapter.cancelSyncNow()
                await gate.open()
                do { _ = try await first.value; Issue.record("First joined caller lost cancellation") }
                catch is CancellationError { }
                do { _ = try await second.value; Issue.record("Second joined caller lost cancellation") }
                catch is CancellationError { }
                let ended = try #require(try await f.core.exchangeResult(id: id))
                #expect(ended.collection == .cancelled)
                #expect(!ended.selectedNames.isEmpty && ended.outstandingNames == ended.selectedNames)
                #expect(!(try await f.core.status()).syncingCollection)
                #expect(try await adapter.syncNow().collection == .succeeded)
                try await adapter.stop()
            } catch {
                await gate.open()
                first.cancel()
                _ = try? await first.value
                try? await adapter.stop()
                throw error
            }
        }
    }
}

extension EventTests {
    /// K12.5/V2.2/V3.1: original callback returns while its actual account read
    /// is held; no following opaque state can pass the unaccepted loss.
    @Test func cloudKitKnownRootLossReturnsBeforeFreshAccountReadAndFencesCursor() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let accountReadsBeforeLoss = await f.factory.resolvedAccountReadCount()
            let gate = CloudCallbackGate()
            await f.factory.appendAccountRead(.success(.init(status: .available,
                identity: Data("synthetic-account-A".utf8))), gate: gate)
            await engine.emitRetained(.fetchedZoneLosses([.init(zone: "meguri-root", cause: .purged)]), label: "knownRootLoss")
            try await gate.waitForArrival()
            await engine.emitRetained(.stateUpdate(Data("must-not-pass-loss".utf8)), label: "cursorBehindLoss")
            #expect(try await f.core.durableSnapshot().serialization == before.serialization)
            await gate.open()
            try await f.adapter.waitForIdle()
            let lossFailure = await f.adapter.lastFailure
            #expect(lossFailure == nil, "Known-root-loss completion: \(String(describing: lossFailure))")
            #expect(await f.factory.resolvedAccountReadCount() == accountReadsBeforeLoss + 1,
                "The following opaque cursor supplies no new loss requiring another provider read")
            do { try await f.core.validateSDK(engine.instanceID); Issue.record("Known root loss retained SDK authority") }
            catch { }
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("knownRootLoss")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            let cancelReturned = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(returned < cancelled && cancelled < cancelReturned && cancelReturned < released)
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization)
            #expect(after.entities == before.entities)
        }
    }

    /// K12.5/V9.1: a second actual loss staged after read A began needs B's
    /// fresh proof; coalescing bookkeeping must not reuse the held old read.
    @Test func cloudKitNewRootLossWhileAccountReadHeldRequiresFreshProof() async throws {
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let pending = await engine.pendingRecordZoneChanges()
            let readsBefore = await f.factory.resolvedAccountReadCount()
            let first = CloudCallbackGate(), second = CloudCallbackGate()
            let account = CloudResolvedAccount(status: .available, identity: Data("synthetic-account-A".utf8))
            await f.factory.appendAccountRead(.success(account), gate: first)
            await f.factory.appendAccountRead(.success(account), gate: second)
            try await withCloudTask(gates: [first, second], operation: {
                await engine.emitRetained(.fetchedZoneLosses([.init(zone: "meguri-root", cause: .deleted)]), label: "firstRootLoss")
                try await first.waitForArrival()
                try await f.adapter.waitForIdle()
            }, whileRunning: {
                try await first.waitForArrival()
                await engine.emitRetained(.fetchedZoneLosses([.init(zone: "meguri-root", cause: .purged)]), label: "newRootLoss")
                try await second.waitForArrival()
                #expect(await f.factory.resolvedAccountReadCount() == readsBefore + 2)
                try await f.core.validateSDK(engine.instanceID)
                #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                #expect(await engine.pendingRecordZoneChanges() == pending)
                await engine.emitRetained(.stateUpdate(Data("cursor-behind-new-loss".utf8)), label: "cursorBehindNewLoss")
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                // A's cancelled provider can return while B still owns proof.
                await first.open()
                try await f.core.validateSDK(engine.instanceID)
                #expect(await engine.pendingRecordZoneChanges() == pending)
                await second.open()
            })
            let failure = await f.adapter.lastFailure
            #expect(failure == nil, "Fresh loss proof completion: \(String(describing: failure))")
            #expect(await f.factory.resolvedAccountReadCount() == readsBefore + 2)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("newRootLoss")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            let cancelReturned = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(returned < cancelled && cancelled < cancelReturned && cancelReturned < released)
            do { try await f.core.validateSDK(engine.instanceID); Issue.record("Fresh root loss retained SDK authority") }
            catch { }
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization)
            #expect(after.entities == before.entities)
        }
    }
}

#if DEBUG
/// Fresh provider proof, rather than a native validity override, ends the
/// staged root-loss original. This path deliberately does not reconcile SDKs.
private func acceptPendingRootLossProof(_ f: CloudKitBridgeFixture, engine: CloudKitEngineFake,
                                        originalGuard: CloudGuard) async throws {
    let reads = await f.factory.resolvedAccountReadCount()
    try #require(try await f.core.currentGuard() == originalGuard)
    await f.factory.appendAccountRead(.success(.init(status: .available,
        identity: Data("synthetic-account-A".utf8))))
    try await f.adapter.refreshPlatformIdentity()
    try #require(await f.factory.resolvedAccountReadCount() == reads + 1)
    try #require(try await f.core.currentGuard() != originalGuard,
                 "Actual fresh provider proof must invalidate the held original")
    do { try await f.core.validateSDK(engine.instanceID); Issue.record("Fresh root-loss proof retained SDK authority") }
    catch { }
    let accepted = try await f.core.durableSnapshot()
    try #require(accepted.rootEnvelope == nil && accepted.rootSystemFields == nil)
    try #require(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
    try #require(!(await engine.recordedTrace()).contains(.cancelEntered),
                 "Proof acceptance itself must not retire the test's native original")
}

extension EventTests {
    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func cloudKitRejectedPendingDrainAutomaticallyRetiresOriginal(afterSDKMutation: Bool) async throws {
        try await withCloudKitFixture(keepAlive: true, appOwnsAccountObservation: true) { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let originalGuard = try await f.core.currentGuard()
            await engine.emitRetained(.willFetch(groupID: "pending-loss", reason: .scheduled), label: "pendingLossWillFetch")
            await engine.emitRetained(.willFetchZone(zone: "meguri-root"), label: "pendingLossWillFetchZone")
            await engine.emitRetained(.fetched(zone: "meguri-root", records: [], deleted: ["active"]), label: "pendingLossDeletedActive")
            try #require(await f.adapter.lastFailure == nil)
            let name = try #require(f.names.first)
            let update = try await f.core.pendingTransition(instance: engine.instanceID, kind: .changes,
                                                          snapshot: [name: .save])
            try #require(update.adds[name] == .save)
            let validated = CloudCallbackGate()
            await f.adapter.setPendingUpdateValidatedHook((afterSDKMutation: afterSDKMutation, action: { await validated.enter() }))
            do {
                try await withCloudTask(gates: [validated], operation: {
                    try await f.adapter.consumePendingUpdates()
                }, whileRunning: {
                    try await validated.waitForArrival()
                    let applied = await engine.recordedTrace().filter {
                        if case .pendingApplied(let id, _, _) = $0 { id == update.id } else { false }
                    }
                    try #require(applied.count == (afterSDKMutation ? 1 : 0),
                                 "ACK race must hold after the actual SDK mutation; take race before it")
                    try await acceptPendingRootLossProof(f, engine: engine, originalGuard: originalGuard)
                    await validated.open()
                })
            } catch {
                await f.adapter.setPendingUpdateValidatedHook(nil)
                throw error
            }
            await f.adapter.setPendingUpdateValidatedHook(nil)
            // This observation cannot initiate shutdown. On the unfixed source
            // it returns false immediately, exposing the missing stop task.
            #expect(try await f.adapter.joinOriginalStopTask(instanceID: engine.instanceID))
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.adapter.lastFailure == nil)
            #expect(await f.factory.engines().count == 1)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("pendingLossDeletedActive")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            let cancelReturned = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(returned < cancelled && cancelled < cancelReturned && cancelReturned < released)
            #expect(trace.filter { $0 == .released }.count == 1)
            #expect(trace.filter {
                if case .pendingApplied(let id, _, _) = $0 { id == update.id } else { false }
            }.count == (afterSDKMutation ? 1 : 0))
            #expect(!(try await f.core.observations()).contains(.pendingAdded(name: name, actionID: update.id)))
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization && after.entities == before.entities)
            #expect(after.rootEnvelope == nil && after.rootSystemFields == nil)
        }
    }

    /// P2: a real batch delegate held after validation must retire its captured
    /// original on Core rejection without another callback or waitForIdle().
    @Test(.timeLimit(.minutes(1)))
    func cloudKitRejectedNextBatchAutomaticallyRetiresOriginal() async throws {
        let validated = CloudCallbackGate()
        try await withCloudKitFixture(keepAlive: true, delegateWorkValidated: { await validated.enter() },
                                     appOwnsAccountObservation: true) { f in
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot()
            let originalGuard = try await f.core.currentGuard()
            try await withCloudTask(gates: [validated], operation: {
                #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            }, whileRunning: {
                try await validated.waitForArrival()
                await engine.emitRetained(.willFetch(groupID: "batch-loss", reason: .scheduled), label: "batchLossWillFetch")
                await engine.emitRetained(.willFetchZone(zone: "meguri-root"), label: "batchLossWillFetchZone")
                await engine.emitRetained(.fetched(zone: "meguri-root", records: [], deleted: ["active"]), label: "batchLossDeletedActive")
                try #require(await f.adapter.lastFailure == nil)
                try await acceptPendingRootLossProof(f, engine: engine, originalGuard: originalGuard)
                await validated.open()
            })
            #expect(try await f.adapter.joinOriginalStopTask(instanceID: engine.instanceID))
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.adapter.lastFailure == nil)
            #expect(await f.factory.engines().count == 1)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("batchLossDeletedActive")))
            let cancelled = try #require(trace.firstIndex(of: .cancelEntered))
            let cancelReturned = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(returned < cancelled && cancelled < cancelReturned && cancelReturned < released)
            #expect(trace.filter { $0 == .released }.count == 1)
            #expect(!trace.contains { if case .handed = $0 { true } else { false } })
            #expect(!(try await f.core.observations()).contains { if case .batchBuilt = $0 { true } else { false } })
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization && after.entities == before.entities)
            #expect(after.rootEnvelope == nil && after.rootSystemFields == nil)
        }
    }
}
#endif
