#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Instrumentation
import func MeguriCoreBindings.testCloudPolicyExchangeObservation
import Synchronization
import Testing

// The existing suite serializes the process-global Core owner.
extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func serviceAppPauseAndResumeWithMediaRetiresOriginalSDKWithoutOperationFailure(automatic: Bool) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            app.sync.setDeviceSettings(paused: false, automatic: automatic, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend()
            await app.sync.drainOperations()
            let backend = try app.backend
            let generation = app.owner.generation
            let before = try await backend.workflow.durableSnapshot()
            let collection = try await CoreCollectionService().localPublicationInformation(generation: generation)
            #expect(!app.sync.operationFailed)
            #expect(backend.firstFailureDiagnostic == nil)
            #expect(await sdk.automaticSchedulingEnabled == automatic)
            let cancelled = Mutex(false)
            await sdk.inspectNextCancellation { cancelled.withLock { $0 = true } }

            app.sync.setDeviceSettings(paused: true, automatic: automatic, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            #expect(app.sync.settings.paused && app.sync.settings.media)
            #expect(!app.sync.operationFailed)
            #expect(backend.firstFailureDiagnostic == nil)
            #expect(cancelled.withLock { $0 })
            #expect(try await backend.workflow.durableSnapshot().entities == before.entities)
            do {
                _ = try await sdk.prepareSend(zones: [])
                Issue.record("Pause did not release the original SDK")
            } catch { }
            do {
                try await backend.workflow.validateSDK(sdk.instanceID)
                Issue.record("Pause retained original SDK authority")
            } catch { }

            app.sync.setDeviceSettings(paused: false, automatic: automatic, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let successor = try #require(await app.factory.engines.last)
            #expect(successor !== sdk && successor.instanceID != sdk.instanceID)
            #expect(await successor.automaticSchedulingEnabled == automatic)
            #expect(!app.sync.settings.paused && app.sync.settings.media)
            #expect(!app.sync.operationFailed)
            #expect(backend.firstFailureDiagnostic == nil)
            try await backend.workflow.validateSDK(successor.instanceID)
            let fetches = await successor.manualFetchGroups.count
            try await app.exchange()
            await app.sync.drainOperations()
            await successor.waitForAutomaticSend()
            await app.sync.drainOperations()
            #expect(await successor.manualFetchGroups.count > fetches)
            #expect(!app.sync.operationFailed)
            #expect(backend.firstFailureDiagnostic == nil)
            do {
                try await backend.workflow.validateSDK(sdk.instanceID)
                Issue.record("Resume revived original SDK authority")
            } catch { }
            #expect(app.owner.state == .ready && app.owner.generation == generation)
            #expect(try app.backend === backend)
            #expect(try await backend.workflow.durableSnapshot().entities == before.entities)
            let after = try await CoreCollectionService().localPublicationInformation(generation: generation)
            #expect(after.localCounts == collection.localCounts)
            #expect(after.lastReviewMilliseconds == collection.lastReviewMilliseconds)
        }
    }

    @Test(arguments: [false, true])
    func serviceAppPauseQualifiesOnlyOriginalSupersededCommand(resumeMedia: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            let backend = try app.backend
            // Media off -> on with automatic scheduling issues the real ResumeMedia.
            // A normal local edit supplies UpdatePending in the other row.
            app.sync.setDeviceSettings(paused: false, automatic: resumeMedia, media: false)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend(); await app.sync.drainOperations()
            let generation = app.owner.generation
            let failureRevision = app.sync.failureRevision
            let held = ServiceAutomaticGate(), returned = ServiceAutomaticGate()
            let stopped = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let cancelled = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let originalID = Mutex<ObjectIdentifier?>(nil)
            let calls = Mutex<[String]>([])
            var original: CloudPolicyCommand?
            var observed: SyncCommandResult?
            var acknowledgements = 0
            backend.beforeExecution = { command in
                let matches: Bool
                if resumeMedia { if case .resumeMedia = command.action { matches = true } else { matches = false } }
                else { if case .updatePending = command.action { matches = true } else { matches = false } }
                if matches, original == nil {
                    original = command; originalID.withLock { $0 = ObjectIdentifier(command.raw) }
                    await held.hold()
                }
            }
            backend.afterExecution = { command, result in
                if let original, command.raw === original.raw {
                    observed = result
                    await returned.hold()
                }
                if case .stopEngine = command.action { stopped.continuation.yield(result.error == nil && !result.deferred) }
            }
            #if DEBUG
            backend.observeCoreCall = { command, call in
                guard originalID.withLock({ $0 == ObjectIdentifier(command.raw) }) else { return }
                calls.withLock { $0.append(call.description) }
            }
            #endif
            app.sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command, let original, command.raw === original.raw {
                    acknowledgements += 1
                }
            }
            await sdk.inspectNextCancellation { cancelled.continuation.yield(true) }
            defer {
                backend.beforeExecution = nil; backend.afterExecution = nil
                app.sync.observeCausalBoundary = nil
                #if DEBUG
                backend.observeCoreCall = nil
                #endif
                stopped.continuation.finish(); cancelled.continuation.finish()
            }
            do {
                if resumeMedia { app.sync.setDeviceSettings(paused: false, automatic: true, media: true) }
                else {
                    let seed = try #require(fixture.notes.first)
                    try await automaticServiceEdit(app, id: seed.id, suffix: " held original UpdatePending")
                }
                try #require(try await serviceAppSignal(held.entered, stage: "actual original command held before execution"))
                try #require(original != nil)
                let collection = try await CoreCollectionService().localPublicationInformation(generation: generation)
                app.sync.setDeviceSettings(paused: true, automatic: resumeMedia, media: resumeMedia)
                await app.sync.waitForInputs()
                // Join actual Pause observations, never drain with the original held.
                try #require(try await serviceAppSignal(cancelled.stream, stage: "Pause cancels original SDK"))
                try #require(try await serviceAppSignal(stopped.stream, stage: "Pause StopEngine returned"))
                #expect(app.sync.settings.paused && acknowledgements == 0)
                let before = try await backend.workflow.durableSnapshot()
                let emitted = try await backend.workflow.emittedUpdateIDs()
                let pending = await sdk.pendingRecordZoneChanges()
                let additions = await sdk.pendingAdditionUpdates
                let batches = await sdk.batchRequests
                let fetches = await sdk.manualFetchGroups
                let receipts = await wire.receipts.count
                let media = try await serviceMediaProof(generation: generation)
                await held.release()
                try #require(try await serviceAppSignal(returned.entered, stage: "original Core refusal stored through native result"))
                let result = try #require(observed)
                #expect(result.error as? BridgeError == .cloudPolicy(.staleCommand))
                #expect(result.originalGuardRefusal != nil && !result.deferred)
                #if DEBUG
                let originalCalls = calls.withLock { $0 }
                #expect(originalCalls.contains(resumeMedia ? "CloudPolicyExecutionResult.detailed_result" : "CloudPolicyOutputResult.detailed_result"))
                #expect(!originalCalls.contains("CloudPolicyExecution.start_begin_media"))
                #expect(!originalCalls.contains("CloudPolicyExecution.start_begin_fetch"))
                #expect(!originalCalls.contains("CloudPolicyExecution.start_begin_send"))
                #expect(!originalCalls.contains("CloudWorkflow.take_pending_updates"))
                #endif
                #expect(acknowledgements == 0)
                #expect(try await backend.workflow.emittedUpdateIDs() == emitted)
                #expect(await sdk.pendingRecordZoneChanges() == pending)
                #expect(await sdk.pendingAdditionUpdates == additions)
                #expect(await sdk.batchRequests == batches)
                #expect(await sdk.manualFetchGroups == fetches)
                #expect(await wire.receipts.count == receipts)
                let after = try await backend.workflow.durableSnapshot()
                #expect(after.entities == before.entities && after.inboxNames == before.inboxNames)
                #expect(after.trackingContent == before.trackingContent)
                let afterMedia = try await serviceMediaProof(generation: generation)
                #expect(afterMedia.names == media.names && afterMedia.bytes == media.bytes)
                await returned.release()
                await app.sync.drainOperations()
                #expect(acknowledgements == (resumeMedia ? 1 : 2))
                #expect(!app.sync.operationFailed && app.sync.failureRevision == failureRevision)
                #expect(backend.firstFailureDiagnostic == nil)
                #expect(backend.diagnostics.contains { $0.contains("qualifiedOriginalRefusal") })
                app.sync.setDeviceSettings(paused: false, automatic: resumeMedia, media: resumeMedia)
                await app.sync.waitForInputs(); await app.sync.drainOperations()
                let successor = try #require(await app.factory.engines.last)
                #expect(successor !== sdk && successor.instanceID != sdk.instanceID)
                try await backend.workflow.validateSDK(successor.instanceID)
                let successorFetches = await successor.manualFetchGroups.count
                try await app.exchange(); await app.sync.drainOperations()
                await successor.waitForAutomaticSend(); await app.sync.drainOperations()
                #expect(await successor.manualFetchGroups.count > successorFetches)
                #expect(!app.sync.operationFailed && app.sync.failureRevision == failureRevision)
                #expect(backend.firstFailureDiagnostic == nil)
                #expect(app.owner.state == .ready && app.owner.generation == generation)
                #expect(try app.backend === backend)
                let finalEntities = try await backend.workflow.durableSnapshot().entities
                #expect(finalEntities.map(\.name) == before.entities.map(\.name))
                #expect(finalEntities.map(\.localRev) == before.entities.map(\.localRev))
                #expect(finalEntities.map(\.payloadDigest) == before.entities.map(\.payloadDigest))
                let final = try await CoreCollectionService().localPublicationInformation(generation: generation)
                #expect(final.localCounts == collection.localCounts && final.lastReviewMilliseconds == collection.lastReviewMilliseconds)
            } catch {
                await held.release(); await returned.release()
                await app.sync.drainOperations()
                throw error
            }
        }
    }

    @Test(arguments: [0, 1, 2, 3, 4])
    func serviceAppOriginalProofProjectsActualProviderOutcomesWithoutLosingPendingWork(outcome: Int) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let gate = ServiceAutomaticGate()
            let result: Result<CloudResolvedAccount?, any Error>
            switch outcome {
            case 0: result = .success(.init(status: .available, identity: Data("synthetic-initial-account".utf8)))
            case 1: result = .success(.init(status: .available, identity: Data("synthetic-other-account".utf8)))
            case 2: result = .success(.init(status: .noAccount, identity: nil))
            case 3: result = .success(.init(status: .temporarilyUnavailable, identity: nil))
            default: result = .failure(ServiceWorkflowFailure.wire)
            }
            await app.factory.resolveNextAccountRead(with: result)
            await app.factory.holdNextAccountRead { await gate.hold() }
            app.sync.syncNow()
            do {
                try #require(try await serviceAppSignal(gate.entered, stage: "original command fresh proof provider held"))
                let workflow = try app.backend.workflow
                let before = try await workflow.durableSnapshot()
                let pending = try await workflow.trackingSendables()
                await gate.release()
                await app.sync.drainOperations(); await app.sync.prepareConnectionAuthority()
                let after = try await workflow.durableSnapshot()
                #expect(after.entities == before.entities)
                #expect(try await workflow.trackingSendables() == pending)
                if outcome == 0 { #expect(!app.sync.operationFailed) }
                else if outcome == 1 || outcome == 2 {
                    let content = try #require(JSONSerialization.jsonObject(with: after.trackingContent) as? [Any])
                    let binding = try #require(content.first as? [Any])
                    let facts = try #require(binding.last as? [String: Any])
                    #expect(facts["state"] as? [String: String] == ["Isolated": outcome == 1 ? "AccountChanged" : "SignedOut"])
                    do { try await workflow.validateSDK(sdk.instanceID); Issue.record("Original SDK survived account transition") }
                    catch { }
                } else {
                    #expect(app.sync.operationFailed)
                }
            } catch { await gate.release(); throw error }
        }
    }

    @Test(arguments: [false, true])
    func serviceAppCloseReleasesIgnoringProofProviderBeforeLateResultAndReopen(switchLate: Bool) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let provider = ServiceAutomaticGate()
            let providerReturned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            await app.factory.resolveNextAccountRead(with: .success(.init(status: switchLate ? .available : .noAccount,
                identity: switchLate ? Data("synthetic-other-account".utf8) : nil)))
            await app.factory.holdNextAccountRead {
                await provider.hold()
                providerReturned.continuation.yield(true)
            }
            app.sync.syncNow()
            var close: Task<Bool, Never>?
            do {
                try #require(try await serviceAppSignal(provider.entered, stage: "ignoring original proof provider"))
                let oldBackend = try app.backend
                let closed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                close = Task {
                    let result = await app.owner.close()
                    closed.continuation.yield(result)
                    return result
                }
                // The provider remains held while every original command and
                // native callback/counter drains and the backend closes.
                try #require(try await serviceAppSignal(closed.stream, stage: "close before ignoring provider returned"))
                #expect(oldBackend.closed && oldBackend.closeReturned == 1)
                try await app.reopen()
                await app.sync.drainOperations()
                let successor = try app.backend
                #expect(successor !== oldBackend)
                let before = try await successor.workflow.durableSnapshot()
                let guardBefore = try await successor.workflow.currentGuard()
                await provider.release()
                try #require(try await serviceAppSignal(providerReturned.stream, stage: "original late provider return"))
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(try await successor.workflow.durableSnapshot() == before)
                #expect(try await successor.workflow.currentGuard() == guardBefore)
                #expect(!app.sync.operationFailed)
            } catch {
                await provider.release(); _ = await close?.value
                throw error
            }
        }
    }

    #if DEBUG
    @Test(arguments: [0, 1, 2, 3, 4])
    func serviceAppTwoFetchesKeepDependentTrackedAcrossProofAndRetirement(outcome: Int) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            app.startCausalTrace()
            let causalTrace = app.causalTrace
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(causalTrace.presence(await app.factory.engines.last))
            await sdk.waitForAutomaticSend()
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let native = ServiceAutomaticGate(), proof = ServiceAutomaticGate()
            let parked = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let resolvedI = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let commands = try app.backend
            var fetchACKs = 0
            app.sync.observeCausalBoundary = { boundary, command in
                ServiceAppHarness.recordCausal(boundary, in: causalTrace)
                if case .commandAcknowledgementJoined = boundary, let command, case .requestFetch = command.action { fetchACKs += 1 }
            }
            app.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { resolvedI.continuation.yield(true) }
            }
            #if DEBUG
            app.sync.observeQuiescentDependency = { parked.continuation.yield($0) }
            #endif
            defer {
                app.restoreCausalTraceObserver(); app.sync.observeAccountPolicyDrain = nil
                #if DEBUG
                app.sync.observeQuiescentDependency = nil
                #endif
                parked.continuation.finish(); resolvedI.continuation.finish()
            }
            let reads = await app.factory.accountReadCount
            await sdk.holdNextFetchReturn { await native.hold() }
            // Two independently issued Foreground RequestFetch commands. B must
            // remain tracked while A owns its original native result and proof.
            app.sync.receive(.foreground)
            do {
                try #require(causalTrace.expectation(try await serviceAppSignal(native.entered, stage: "fetch A original Service terminal held", trace: causalTrace)))
                app.sync.receive(.foreground)
                try #require(causalTrace.expectation(try await serviceAppSignal(parked.stream, stage: "fetch B parked before Core fetch admission", trace: causalTrace)))
                await app.sync.accountChanged()
                try #require(causalTrace.expectation(try await serviceAppSignal(resolvedI.stream, stage: "resolved app read I retained behind fetch A", trace: causalTrace)))
                #expect(causalTrace.expectation(await app.factory.accountReadCount == reads + 1))
                if outcome == 1 {
                    await app.factory.resolveNextAccountRead(with: .success(.init(status: .noAccount, identity: nil)))
                } else if outcome == 2 {
                    await app.factory.resolveNextAccountRead(with: .success(.init(status: .available, identity: Data("synthetic-other-account".utf8))))
                }
                let providerReturned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                defer { providerReturned.continuation.finish() }
                await app.factory.holdNextAccountRead {
                    await proof.hold(); providerReturned.continuation.yield(true)
                }
                await native.release()
                try #require(causalTrace.expectation(try await serviceAppSignal(proof.entered, stage: "A fresh proof while B stays tracked and unadmitted", trace: causalTrace)))
                #expect(causalTrace.expectation(fetchACKs == 0))
                if outcome >= 3 {
                    // Retire the real old owner while A's provider ignores
                    // cancellation and B is still parked, then test its late
                    // return against the actual successor.
                    let retired = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    let ownerShutdownTask = Task { @MainActor () throws -> Void in
                        if outcome == 3 {
                            causalTrace.append("intentional.expiry.enter")
                            app.sync.backgroundExpired()
                            causalTrace.append("intentional.close.enter")
                            let closed = await app.owner.close()
                            retired.continuation.yield(closed)
                        } else {
                            let source = try #require(causalTrace.presence(Bundle.module.url(forResource: "review", withExtension: "colpkg")))
                            app.owner.entry.inspect(source); await app.owner.entry.waitForCompletion()
                            try #require(causalTrace.expectation(app.owner.entry.preview != nil))
                            app.owner.entry.confirm(); await app.owner.entry.waitForCompletion()
                            await app.sync.waitForReplacement()
                            retired.continuation.yield(app.owner.entry.lastActivation != nil)
                        }
                        retired.continuation.finish()
                    }
                    do {
                        try #require(causalTrace.expectation(try await serviceAppSignal(retired.stream, stage: "original A/B cancellation and native/backend join before provider release", trace: causalTrace)))
                        try await ownerShutdownTask.value
                        #expect(causalTrace.expectation(commands.closed && commands.closeReturned == 1))
                        if outcome == 3 { try await app.reopen() }
                        await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                        let successor = try app.backend
                        #expect(causalTrace.expectation(successor !== commands))
                        let snapshot = try await successor.workflow.durableSnapshot()
                        let guardBefore = try await successor.workflow.currentGuard()
                        await proof.release()
                        try #require(causalTrace.expectation(try await serviceAppSignal(providerReturned.stream, stage: "late old A provider after successor admission", trace: causalTrace)))
                        await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                        #expect(causalTrace.expectation(try await successor.workflow.durableSnapshot() == snapshot))
                        #expect(causalTrace.expectation(try await successor.workflow.currentGuard() == guardBefore))
                        return
                    } catch {
                        await proof.release(); _ = await ownerShutdownTask.result
                        throw error
                    }
                }
                await proof.release()
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(causalTrace.expectation(fetchACKs == 2)) // each original command returns through its normal executor
                if outcome == 0 { #expect(causalTrace.expectation(!app.sync.operationFailed)) }
                else {
                    do { try await commands.workflow.validateSDK(sdk.instanceID); causalTrace.expectation(false); Issue.record("Retired original SDK admitted dependent fetch") }
                    catch { }
                }
            } catch {
                await native.release(); await proof.release()
                throw error
            }
        }
    }

    // Three actual policy triggers, each with normal completion and owner
    // cancellation/reopen. No clock/deadline or command result is substituted.
    @Test(arguments: [0, 1, 2], [false, true])
    func serviceAppMixedAndFlushDependenciesJoinOriginalProof(trigger: Int, retire: Bool) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            app.startCausalTrace()
            let causalTrace = app.causalTrace
            if trigger == 0 {
                app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
                await app.sync.waitForInputs(); await app.sync.drainOperations()
            }
            let sdk = try #require(causalTrace.presence(await app.factory.engines.last))
            await sdk.waitForAutomaticSend()
            try #require(causalTrace.expectation(await sdk.automaticSchedulingEnabled == (trigger == 0)))
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let commands = try app.backend
            let native = ServiceAutomaticGate(), proof = ServiceAutomaticGate()
            let parked = AsyncStream<Bool>.makeStream(bufferingPolicy: .unbounded)
            let providerReturned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var primary: CloudPolicyCommand?, dependent: CloudPolicyCommand?
            var primaryStarts = 0, dependentStarts = 0
            var primaryResults = 0, dependentResults = 0
            var primaryACKs = 0, dependentACKs = 0
            var isParked = false, parks = 0, revocations = 0
            let cancelled = Mutex(false), parkedAtNative = Mutex(false), dependentEnteredNative = Mutex(false)
            await sdk.inspectNextCancellation { cancelled.withLock { $0 = true } }
            commands.beforeExecution = { command in
                switch command.action {
                case .requestFetch where trigger == 0:
                    primary = command; primaryStarts += 1
                case .runExchange:
                    if trigger == 0 { dependent = command; dependentStarts += 1 }
                    else { primary = command; primaryStarts += 1 }
                case .backgroundFlush where trigger == 1, .quitFlush where trigger == 2:
                    dependent = command; dependentStarts += 1
                default: break
                }
            }
            commands.afterExecution = { command, result in
                if let primary, command.raw === primary.raw { primaryResults += 1 }
                if let dependent, command.raw === dependent.raw {
                    #expect(causalTrace.expectation(!isParked), "Dependency revocation joins before original result delivery")
                    dependentResults += 1
                }
                if !retire, (primary.map { command.raw === $0.raw } == true || dependent.map { command.raw === $0.raw } == true) {
                    #expect(causalTrace.expectation(result.error == nil && !result.deferred))
                }
            }
            app.sync.observeQuiescentDependency = { value in
                isParked = value
                parkedAtNative.withLock { $0 = value }
                if value { parks += 1 } else { revocations += 1 }
                parked.continuation.yield(value)
            }
            app.sync.observeCausalBoundary = { boundary, command in
                ServiceAppHarness.recordCausal(boundary, in: causalTrace)
                guard case .commandAcknowledgementJoined = boundary, let command else { return }
                if let primary, command.raw === primary.raw { primaryACKs += 1 }
                if let dependent, command.raw === dependent.raw { dependentACKs += 1 }
            }
            defer {
                commands.beforeExecution = nil; commands.afterExecution = nil
                app.sync.observeQuiescentDependency = nil; app.restoreCausalTraceObserver()
                parked.continuation.finish(); providerReturned.continuation.finish()
            }
            let fetches = await sdk.manualFetchGroups.count
            let engines = await app.factory.engines.count
            let reads = await app.factory.accountReadCount
            let before = try await commands.workflow.durableSnapshot()
            let pending = try await commands.workflow.trackingSendables()
            await sdk.holdNextFetchReturn { await native.hold() }
            if trigger == 0 { app.sync.receive(.foreground) }
            else { app.sync.syncNow() }
            do {
                try #require(causalTrace.expectation(try await serviceAppSignal(native.entered, stage: "primary actual Service fetch return held", trace: causalTrace)))
                try #require(causalTrace.expectation(primaryStarts == 1 && primary != nil))
                #expect(causalTrace.expectation(await sdk.manualFetchGroups.count == fetches + 1))
                if trigger == 0 { app.sync.syncNow() }
                else if trigger == 1 {
                    app.sync.receive(.background(deadlineMilliseconds: Int64(Date().timeIntervalSince1970 * 1_000) + 30_000))
                } else { app.sync.receive(.quitRequested) }
                try #require(causalTrace.expectation(try await serviceAppSignal(parked.stream, stage: "actual dependent exchange or flush parked", trace: causalTrace)))
                try #require(causalTrace.expectation(dependentStarts == 1 && dependent != nil))
                #expect(causalTrace.expectation(primaryResults == 0 && dependentResults == 0 && primaryACKs == 0 && dependentACKs == 0))
                #expect(causalTrace.expectation(await sdk.manualFetchGroups.count == fetches + 1))
                await app.factory.holdNextAccountRead {
                    await proof.hold(); providerReturned.continuation.yield(true)
                }
                await native.release()
                try #require(causalTrace.expectation(try await serviceAppSignal(proof.entered, stage: "primary fresh account proof admitted with dependent still tracked", trace: causalTrace)))
                #expect(causalTrace.expectation(isParked && primaryResults == 0 && dependentResults == 0))
                #expect(causalTrace.expectation(await app.factory.accountReadCount == reads + 1))
                #expect(causalTrace.expectation(!cancelled.withLock { $0 }))
                if retire {
                    let closed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    defer { closed.continuation.finish() }
                    let closing = Task {
                        causalTrace.append("intentional.close.enter")
                        _ = closed.continuation.yield(await app.owner.close())
                    }
                    do {
                        try #require(causalTrace.expectation(try await serviceAppSignal(closed.stream, stage: "original proof and parked dependent cancel before late provider", trace: causalTrace)))
                        await closing.value
                    } catch { await proof.release(); await closing.value; throw error }
                    #expect(causalTrace.expectation(commands.closed && commands.closeReturned == 1 && cancelled.withLock { $0 }))
                    #expect(causalTrace.expectation(primaryACKs == 1 && dependentACKs == 1))
                    #expect(causalTrace.expectation(!isParked && revocations == parks))
                    try await app.reopen()
                    await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                    let successor = try app.backend
                    let successorSDK = try #require(causalTrace.presence(await app.factory.engines.last))
                    #expect(causalTrace.expectation(successor !== commands && successorSDK.instanceID != sdk.instanceID))
                    try await successor.workflow.validateSDK(successorSDK.instanceID)
                    let snapshot = try await successor.workflow.durableSnapshot()
                    let guardBefore = try await successor.workflow.currentGuard()
                    await proof.release()
                    try #require(causalTrace.expectation(try await serviceAppSignal(providerReturned.stream, stage: "late original provider returns after successor admission", trace: causalTrace)))
                    await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                    #expect(causalTrace.expectation(try await successor.workflow.durableSnapshot() == snapshot))
                    #expect(causalTrace.expectation(try await successor.workflow.currentGuard() == guardBefore))
                } else {
                    if trigger == 0 {
                        await sdk.inspectNextManualFetch {
                            #expect(causalTrace.expectation(!parkedAtNative.withLock { $0 }), "Exact dependent revocation precedes its actual SDK entry")
                            dependentEnteredNative.withLock { $0 = true }
                        }
                    }
                    await proof.release()
                    await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                    #expect(causalTrace.expectation(primaryStarts == 1 && dependentStarts == 1))
                    if trigger == 0 { #expect(causalTrace.expectation(dependentEnteredNative.withLock { $0 })) }
                    #expect(causalTrace.expectation(primaryResults == 1 && dependentResults == 1 && primaryACKs == 1 && dependentACKs == 1))
                    #expect(causalTrace.expectation(parks > 0 && revocations == parks && !isParked))
                    #expect(causalTrace.expectation(!cancelled.withLock { $0 } && !app.sync.operationFailed))
                    #expect(causalTrace.expectation(await app.factory.engines.count == engines))
                    let groups = await sdk.manualFetchGroups
                    #expect(causalTrace.expectation(groups.count == fetches + (trigger == 0 ? 3 : 2)))
                    #expect(causalTrace.expectation(Set(groups).count == groups.count))
                    try await commands.workflow.validateSDK(sdk.instanceID)
                    #expect(causalTrace.expectation(try await commands.workflow.durableSnapshot().entities == before.entities))
                    #expect(causalTrace.expectation(try await commands.workflow.trackingSendables() == pending))
                }
            } catch { await native.release(); await proof.release(); throw error }
        }
    }

    // The real QuitFlush keeps P11.4's original now+10s deadline. The other
    // row uses the existing app expiry/task-cancellation path; both join the
    // same cancelFlush and retain the ignoring provider until after results.
    @Test(arguments: [false, true])
    func serviceAppParkedFlushRevocationACKPrecedesNativeAndCoreCancellation(deadline: Bool) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let native = ServiceAutomaticGate(), proof = ServiceAutomaticGate()
            let parked = AsyncStream<Bool>.makeStream(bufferingPolicy: .unbounded)
            let cancelled = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let returned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let state = Mutex((parked: false, everParked: false, joined: false, native: 0, core: 0))
            let backend = try app.backend
            var flush: CloudPolicyCommand?, exchange: CloudPolicyCommand?
            var flushResults = 0, exchangeResults = 0, flushACKs = 0, exchangeACKs = 0
            app.sync.observeQuiescentDependency = { value in
                state.withLock { $0.parked = value; $0.everParked = $0.everParked || value }
                parked.continuation.yield(value)
            }
            app.sync.observeCausalBoundary = { boundary, command in
                guard let command else { return }
                switch boundary {
                case .dependencyRevocationJoined:
                    if case .quitFlush = command.action { state.withLock { $0.joined = true } }
                case .commandAcknowledgementJoined:
                    if let flush, command.raw === flush.raw { flushACKs += 1 }
                    if let exchange, command.raw === exchange.raw { exchangeACKs += 1 }
                default: break
                }
            }
            backend.beforeExecution = { command in
                if case .quitFlush = command.action { flush = command }
                if case .runExchange = command.action { exchange = command }
            }
            backend.afterExecution = { command, result in
                if let flush, command.raw === flush.raw {
                    #expect(result.error is CancellationError && !result.deferred)
                    #expect(result.nativeCancellation?.matches(command) == true)
                    if let exchange { #expect(result.nativeCancellation?.matches(exchange) == false) }
                    if let context = SyncCommandTaskContext.current {
                        #expect(context.ownsNativeCancellation(result, command: command, backend: backend,
                            generation: context.generation, revision: context.revision))
                    } else { Issue.record("Missing original flush context") }
                    flushResults += 1
                }
                if let exchange, command.raw === exchange.raw {
                    #expect(result.nativeCancellation?.matches(command) == true)
                    if let flush { #expect(result.nativeCancellation?.matches(flush) == false) }
                    if let context = SyncCommandTaskContext.current {
                        #expect(context.ownsNativeCancellation(result, command: command, backend: backend,
                            generation: context.generation, revision: context.revision))
                    } else { Issue.record("Missing original exchange context") }
                    exchangeResults += 1
                }
            }
            backend.observeCoreCall = { command, call in
                guard case .quitFlush = command.action,
                      call == .cloudPolicyExecutionStartValidateControl || call == .cloudPolicyExecutionOriginalSdk else { return }
                state.withLock {
                    if $0.everParked {
                        #expect($0.joined && !$0.parked, "Actual Core cancellation follows ordered revocation ACK")
                        $0.core += 1
                    }
                }
            }
            await sdk.inspectNextCancellation {
                state.withLock {
                    #expect($0.joined && !$0.parked, "Actual SDK cancellation follows ordered revocation ACK")
                    $0.native += 1
                }
                cancelled.continuation.yield(true)
            }
            defer {
                backend.beforeExecution = nil; backend.afterExecution = nil; backend.observeCoreCall = nil
                app.sync.observeQuiescentDependency = nil; app.sync.observeCausalBoundary = nil
                parked.continuation.finish(); cancelled.continuation.finish(); returned.continuation.finish()
            }
            await sdk.holdNextFetchReturn { await native.hold() }
            app.sync.syncNow()
            do {
                try #require(try await serviceAppSignal(native.entered, stage: "original exchange native terminal before flush"))
                app.sync.receive(.quitRequested)
                try #require(try await serviceAppSignal(parked.stream, stage: "real quit flush parked on original exchange"))
                try #require(flush != nil && exchange != nil)
                await app.factory.holdNextAccountRead { await proof.hold(); returned.continuation.yield(true) }
                await native.release()
                try #require(try await serviceAppSignal(proof.entered, stage: "original fresh proof pending at flush cancellation"))
                #expect(flushResults == 0 && exchangeResults == 0 && flushACKs == 0 && exchangeACKs == 0)
                if !deadline { app.sync.backgroundExpired() }
                try #require(try await serviceAppSignal(cancelled.stream, stage: "real cancellation at original absolute deadline or task expiry"))
                await app.sync.drainOperations()
                #expect(flushResults == 1 && exchangeResults == 1 && flushACKs == 1 && exchangeACKs == 1)
                #expect(state.withLock { $0.joined && !$0.parked && $0.native > 0 && $0.core > 0 })
                // The provider still has not returned. Close/reopen and then
                // deliver its late original reply against a real successor.
                try #require(await app.owner.close())
                try await app.reopen(); await app.sync.drainOperations()
                let successor = try app.backend
                #expect(successor !== backend)
                let snapshot = try await successor.workflow.durableSnapshot()
                let guardBefore = try await successor.workflow.currentGuard()
                await proof.release()
                try #require(try await serviceAppSignal(returned.stream, stage: "late provider after cancelled flush and successor"))
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(try await successor.workflow.durableSnapshot() == snapshot)
                #expect(try await successor.workflow.currentGuard() == guardBefore)
            } catch { await native.release(); await proof.release(); throw error }
        }
    }

    @Test(arguments: ServiceParkedFailureCase.allCases)
    func serviceAppParkedFlushPreservesOriginalFailureAcrossLaterExpiry(mode: ServiceParkedFailureCase) async throws {
        let original = ServiceParkedOriginalFailure()
        let sdkFailure = CloudKitSDKOperationFailure(underlying: original, code: 4, retryAfterMilliseconds: 123)
        let fault = ServiceParkedReadFault()
        try await withAutomaticServiceApp(prepare: { app in
            app.ownTransport = ServiceParkedReadTransport(original: ServiceOwnTransport(wire: app.wire), fault: fault)
        }) { app, _, _ in
            let backend = try app.backend
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let native = ServiceAutomaticGate(), provider = ServiceAutomaticGate()
            let exchangeDelivery = ServiceAutomaticGate(), flushDelivery = ServiceAutomaticGate()
            let parked = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let results = AsyncStream<Bool>.makeStream(bufferingPolicy: .unbounded)
            let lateProvider = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            // One terminal subscription per stage; cancelling an earlier stage's
            // iterator must not terminate a later stage's failure observation.
            let nativeFailure = AsyncStream<any Error>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let parkFailure = AsyncStream<any Error>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let providerFailure = AsyncStream<any Error>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var exchange: CloudPolicyCommand?, flush: CloudPolicyCommand?
            var exchangeResult: SyncCommandResult?, flushResult: SyncCommandResult?
            var acknowledgements = 0, inspected = 0
            let discards = Mutex(0)
            let verify: @MainActor (SyncCommandResult) -> Void = { result in
                #expect(result.error != nil && !result.deferred)
                #expect(result.nativeCancellation == nil)
                if mode.failsBeforeProof {
                    #expect(result.fetchProofOutcome == nil)
                } else {
                    let outcome = result.fetchProofOutcome
                    #expect(outcome != nil && outcome?.preservesFailure == true)
                }
                #expect(result.acknowledgedOutputCount == result.outputs.count)
                switch mode {
                case .providerCancellation, .cleanup: #expect(result.error is CancellationError)
                case .providerFailure, .readFailure: #expect((result.error as? ServiceParkedOriginalFailure) === original)
                case .sdkFailure:
                    let failure = result.error as? CloudKitSDKOperationFailure
                    #expect((failure?.underlying as? ServiceParkedOriginalFailure) === original)
                    #expect(failure?.code == 4 && failure?.retryAfterMilliseconds == 123)
                }
                if mode == .cleanup {
                    #expect((result.fetchProofOutcome?.debugOriginalCompletionFailure as? ServiceParkedOriginalFailure) === original)
                }
            }
            backend.beforeExecution = { command in
                let selected: Bool
                switch command.action {
                case .runExchange:
                    exchange = command; selected = true
                    if mode == .cleanup {
                        backend.originalDiscardFaults[ObjectIdentifier(command.raw)] = {
                            discards.withLock { $0 += 1 }; throw original
                        }
                    }
                case .quitFlush: flush = command; selected = true
                default: selected = false
                }
                if selected {
                    backend.expectedCommandFailures[ObjectIdentifier(command.raw)] = { result in verify(result); inspected += 1 }
                }
            }
            backend.afterExecution = { command, result in
                let delivery: ServiceAutomaticGate
                if let exchange, command.raw === exchange.raw { exchangeResult = result; delivery = exchangeDelivery }
                else if let flush, command.raw === flush.raw { flushResult = result; delivery = flushDelivery }
                else { return }
                // Always signal a terminal, including an unexpectedly early
                // error, before holding the actual result's delivery.
                results.continuation.yield(result.error != nil)
                if let failure = result.error {
                    nativeFailure.continuation.yield(failure)
                    parkFailure.continuation.yield(failure)
                    providerFailure.continuation.yield(failure)
                }
                await delivery.hold()
            }
            app.sync.observeQuiescentDependency = { if $0 { parked.continuation.yield(true) } }
            app.sync.observeCausalBoundary = { boundary, command in
                guard case .commandAcknowledgementJoined = boundary, let command else { return }
                if command.raw === exchange?.raw || command.raw === flush?.raw { acknowledgements += 1 }
            }
            defer {
                backend.beforeExecution = nil; backend.afterExecution = nil
                app.sync.observeQuiescentDependency = nil; app.sync.observeCausalBoundary = nil
                parked.continuation.finish(); results.continuation.finish(); lateProvider.continuation.finish()
                nativeFailure.continuation.finish(); parkFailure.continuation.finish(); providerFailure.continuation.finish()
            }
            await sdk.holdNextFetchReturn { await native.hold() }
            app.sync.syncNow()
            do {
                try #require(try await serviceAppSignal(native.entered, stage: "negative original native terminal", failure: nativeFailure.stream))
                app.sync.receive(.quitRequested)
                try #require(try await serviceAppSignal(parked.stream, stage: "negative real flush park", failure: parkFailure.stream))
                try #require(exchange != nil && flush != nil)
                switch mode {
                case .providerCancellation: await app.factory.resolveNextAccountRead(with: .failure(CancellationError()))
                case .providerFailure: await app.factory.resolveNextAccountRead(with: .failure(original))
                case .readFailure: fault.arm(original)
                case .sdkFailure: fault.arm(sdkFailure)
                case .cleanup: break
                }
                // The raw transport faults occur before any proof read. The
                // provider/cleanup rows instead retain the actual proof outcome.
                if !mode.failsBeforeProof {
                    await app.factory.holdNextAccountRead { await provider.hold(); lateProvider.continuation.yield(true) }
                }
                await native.release()
                if !mode.failsBeforeProof {
                    try #require(try await serviceAppSignal(provider.entered, stage: "negative original proof provider", failure: providerFailure.stream))
                    if mode == .cleanup { app.sync.backgroundExpired() }
                    else { await provider.release() }
                }
                try #require(try await serviceAppSignal(results.stream, stage: "negative first original result"))
                try #require(try await serviceAppSignal(results.stream, stage: "negative second original result"))
                let a = try #require(exchangeResult), b = try #require(flushResult)
                verify(a); verify(b)
                #expect(acknowledgements == 0)
                // Both actual native outcomes already exist. Later expiry and
                // held result delivery cannot turn either into owned cancel.
                app.sync.backgroundExpired()
                await exchangeDelivery.release(); await flushDelivery.release(); await app.sync.drainOperations()
                #expect(inspected == 2 && backend.expectedCommandFailures.isEmpty)
                #expect(acknowledgements == 2 && app.sync.operationFailed)
                #expect(discards.withLock { $0 } == (mode == .cleanup ? 1 : 0))
                await provider.release()
                if !mode.failsBeforeProof {
                    try #require(try await serviceAppSignal(lateProvider.stream, stage: "negative original provider joined"))
                }
            } catch {
                await native.release(); await provider.release(); await exchangeDelivery.release(); await flushDelivery.release()
                throw error
            }
        }
    }

    @Test
    func serviceAppPredecessorCorePhaseRetainsRealResultACKBeforeAccountProof() async throws {
        try await withAutomaticServiceApp { app, _, _ in
            // AutomaticOff masks B's completion status transition; enabling automatic mode rotates the SDK.
            app.sync.setDeviceSettings(paused: false, automatic: true, media: app.sync.settings.media)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.waitForAutomaticSend(); await app.sync.drainOperations()
            let backend = try app.backend
            let native = ServiceAutomaticGate(), provider = ServiceAutomaticGate()
            let firstResultGate = ServiceAutomaticGate(), resultB = ServiceAutomaticGate()
            let waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var a: CloudPolicyCommand?, b: CloudPolicyCommand?
            var aACK = false, bACK = false, bEntered = false, bReleased = false, projected = false
            backend.beforeExecution = { command in
                if case .recordExchange = command.action, a == nil { a = command }
                if let b, command.raw === b.raw {
                    #expect(aACK && bReleased, "B enters actual Core execution only after A's result ACK and phase release")
                    bEntered = true
                }
            }
            backend.afterExecution = { command, result in
                if let a, command.raw === a.raw {
                    #expect(result.error == nil && !result.deferred)
                    await firstResultGate.hold()
                }
                if let b, command.raw === b.raw {
                    #expect(result.error == nil && !result.deferred)
                    await resultB.hold()
                }
            }
            app.sync.observeCausalBoundary = { boundary, command in
                switch boundary {
                case .corePredecessorWaiting(let predecessor):
                    if case .recordExchange = predecessor.action, let command {
                        b = command
                        waiting.continuation.yield(true)
                    }
                case .corePredecessorReleased(let predecessor):
                    if let a, predecessor.raw === a.raw { bReleased = true }
                case .commandAcknowledgementJoined:
                    if let command, let a, command.raw === a.raw { aACK = true }
                    if let command, let b, command.raw === b.raw { bACK = true }
                case .proofResultReduced:
                    #expect(aACK && bACK, "Account projection cannot pass either real unfinished Core/result prefix")
                    projected = true
                default: break
                }
            }
            app.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { completion.continuation.yield(true) }
            }
            defer {
                backend.beforeExecution = nil; backend.afterExecution = nil
                app.sync.observeCausalBoundary = nil; app.sync.observeAccountPolicyDrain = nil
                waiting.continuation.finish(); completion.continuation.finish()
            }
            await sdk.holdNextFetchReturn { await native.hold() }
            app.sync.syncNow()
            do {
                try #require(try await serviceAppSignal(native.entered, stage: "actual exchange native prefix held"))
                await app.factory.holdNextAccountRead { await provider.hold() }
                await app.sync.accountChanged()
                try #require(try await serviceAppSignal(provider.entered, stage: "real account proof provider before predecessor phases"))
                await provider.release()
                try #require(try await serviceAppSignal(completion.stream, stage: "resolved real account completion held behind actual native prefix"))
                #expect(!projected)
                // Normal SDK callbacks/drains are retained: no synthetic phase,
                // result, ACK, or no-callback inference is introduced here.
                await native.release()
                try #require(try await serviceAppSignal(firstResultGate.entered, stage: "first exchange result paused pending acknowledgement"))
                try #require(try await serviceAppSignal(waiting.stream, stage: "tracked command B waiting actual predecessor phase A"))
                // The same resolved account completion remains held behind
                // the actual Core/result prefix; B is not a causal proof wait.
                #expect(!aACK && !bEntered && !projected)
                await firstResultGate.release()
                try #require(try await serviceAppSignal(resultB.entered, stage: "B released by actual A result ACK, actual B Core result held"))
                #expect(aACK && bEntered && bReleased && !bACK && !projected)
                await resultB.release()
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(aACK && bACK && projected && !app.sync.operationFailed)
                try await backend.workflow.validateSDK(sdk.instanceID)
                // An actual successor owner remains usable after all phases.
                try #require(await app.owner.close())
                try await app.reopen(); await app.sync.drainOperations()
                let successor = try app.backend
                #expect(successor !== backend)
                let successorSDK = try #require(await app.factory.engines.last)
                try await successor.workflow.validateSDK(successorSDK.instanceID)
            } catch {
                await native.release(); await provider.release(); await firstResultGate.release(); await resultB.release()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func servicePublisherOriginalOwnResultCommitsBeforeResolvedAccountProjection(signOut: Bool) async throws {
        let gate = ServiceAutomaticGate()
        let committed = Mutex(false)
        try await withServicePublisher(prepare: { app in
            app.ownTransport = HeldServicePublicationTransport(original: ServiceOwnTransport(wire: app.wire), gate: gate)
            app.observeOwn = { observation in
                if case .createZones = observation.metadata.kind, case .committed = observation.phase { committed.withLock { $0 = true } }
            }
        }) { app in
            let originalBackend = try app.backend
            let readResolved = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var projected = false
            app.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { readResolved.continuation.yield(true) }
            }
            #if DEBUG
            app.sync.observeCausalBoundary = { boundary, _ in
                if case .proofResultReduced = boundary {
                    #expect(committed.withLock { $0 }, "Original own result bookkeeping precedes account projection")
                    projected = true
                }
            }
            #endif
            defer {
                app.sync.observeAccountPolicyDrain = nil
                #if DEBUG
                app.sync.observeCausalBoundary = nil
                #endif
                readResolved.continuation.finish()
            }
            let publication = Task { await app.sync.confirmPublication() }
            do {
                try #require(try await serviceAppSignal(gate.entered, stage: "original Service CreateZones result held"))
                #expect(app.sync.connectionOperationActive && !committed.withLock { $0 })
                await app.factory.resolveNextAccountRead(with: .success(signOut
                    ? .init(status: .noAccount, identity: nil)
                    : .init(status: .available, identity: Data("synthetic-initial-account".utf8))))
                await app.sync.accountChanged()
                try #require(try await serviceAppSignal(readResolved.stream, stage: "account I completion held by actual own operation"))
                #expect(!projected && !committed.withLock { $0 })
                await gate.release()
                await publication.value
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(committed.withLock { $0 })
                #if DEBUG
                #expect(projected)
                #endif
                let backend = try app.backend
                #expect(backend === originalBackend)
                if !signOut { #expect(!app.sync.operationFailed) }
            } catch {
                await gate.release(); await publication.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceResumeOwnProofFinishesWithoutWaitingForItsOwnTask(priorAccount: Bool) async throws {
        try await withAutomaticServiceApp { app, _, _ in
            app.startCausalTrace()
            let causalTrace = app.causalTrace
            let sdk = try #require(causalTrace.presence(await app.factory.engines.last))
            await app.factory.resolveNextAccountRead(with: .success(.init(status: .noAccount, identity: nil)))
            await app.sync.accountChanged(); await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await app.factory.resolveNextAccountRead(with: .success(.init(status: .available, identity: Data("synthetic-initial-account".utf8))))
            await app.sync.accountChanged(); await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            try await app.owner.prepareCloudConnectionPreview(); await app.sync.waitForConnectionPreview()
            guard case .ready(_, let summary) = app.sync.preview else { throw ServiceWorkflowFailure.wire }
            try #require(causalTrace.expectation(summary.decision == .confirmResume))
            let proof = ServiceAutomaticGate(), preceding = ServiceAutomaticGate()
            let pending = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            app.sync.observeResumePolicyDrain = { stage in
                if case .waiting = stage { pending.continuation.yield(true) }
            }
            defer { app.sync.observeResumePolicyDrain = nil; pending.continuation.finish() }
            if priorAccount {
                await app.factory.holdNextAccountRead { await preceding.hold() }
                await app.sync.accountChanged()
                try #require(causalTrace.expectation(try await serviceAppSignal(preceding.entered, stage: "account I provider before unadmitted Resume", trace: causalTrace)))
            } else { await app.factory.holdNextAccountRead { await proof.hold() } }
            let resumed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let original = Task {
                await app.sync.confirmResume()
                causalTrace.append("confirmResume.return")
                resumed.continuation.yield(true); resumed.continuation.finish()
            }
            do {
                if priorAccount {
                    try #require(causalTrace.expectation(try await serviceAppSignal(pending.stream, stage: "unadmitted Resume waiting for actual earlier account I", trace: causalTrace)))
                    await app.factory.holdNextAccountRead { await proof.hold() }
                    await preceding.release()
                }
                try #require(causalTrace.expectation(try await serviceAppSignal(proof.entered, stage: "Resume owner's exact account proof", trace: causalTrace)))
                #expect(causalTrace.expectation(app.sync.connectionOperationActive))
                await proof.release()
                try #require(causalTrace.expectation(try await serviceAppSignal(resumed.stream, stage: "Resume proof ACK releases its own retained task", trace: causalTrace)))
                await original.value; await app.sync.drainOperations()
                #expect(causalTrace.expectation(!app.sync.connectionOperationActive && !app.sync.operationFailed))
                let successor = try #require(causalTrace.presence(await app.factory.engines.last))
                #expect(causalTrace.expectation(successor.instanceID != sdk.instanceID))
                try await app.backend.workflow.validateSDK(successor.instanceID)
            } catch { await preceding.release(); await proof.release(); await original.value; throw error }
        }
    }

    #endif

    @Test func serviceAppResolvedAccountReadKeepsItsProjectionBeforeCommandFreshProof() async throws {
        try await withAutomaticServiceApp { app, _, _ in
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let native = ServiceAutomaticGate(), fresh = ServiceAutomaticGate()
            let waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var proofProjections = 0
            app.sync.observeAccountPolicyDrain = { stage in
                if case .waiting = stage { waiting.continuation.yield(true) }
            }
            #if DEBUG
            app.sync.observeCausalBoundary = { boundary, _ in
                if case .proofAcknowledgementSending = boundary { proofProjections += 1 }
            }
            #endif
            let before = await app.factory.accountReadCount
            await sdk.holdNextFetchReturn { await native.hold() }
            let original = Task { try await app.exchange() }
            do {
                try #require(try await serviceAppSignal(native.entered, stage: "original native fetch result before command return"))
                await app.sync.accountChanged()
                try #require(try await serviceAppSignal(waiting.stream, stage: "resolved app read I held by original command"))
                #expect(await app.factory.accountReadCount == before + 1)
                await app.factory.holdNextAccountRead { await fresh.hold() }
                await native.release()
                try #require(try await serviceAppSignal(fresh.entered, stage: "ordered fresh read after I projection"))
                #if DEBUG
                #expect(proofProjections >= 1)
                #endif
                #expect(!app.sync.operationFailed)
                await fresh.release()
                try await original.value
                await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
                #expect(!app.sync.operationFailed)
            } catch {
                await native.release(); await fresh.release()
                original.cancel(); _ = await original.result
                app.sync.observeAccountPolicyDrain = nil
                #if DEBUG
                app.sync.observeCausalBoundary = nil
                #endif
                throw error
            }
            app.sync.observeAccountPolicyDrain = nil
            #if DEBUG
            app.sync.observeCausalBoundary = nil
            #endif
        }
    }

    @Test
    func serviceSDKSavedRemovalRequiresOriginalHandlerReturnAndActualBookkeeping() async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            let sdk = try #require(await app.factory.engines.last)
            let zone = try #require(fixture.records.first?.zone)
            let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
            let handler = ServiceAutomaticGate()
            var original: CloudKitNetworkOperation?
            do {
                await sdk.holdNextSentHandlerReturn { await handler.hold() }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Saved removal boundary")
                original = try await sdk.prepareSend(zones: [zone]).enqueue(validate: {})
                try #require(try await serviceAppSignal(handler.entered, stage: "original Saved handler held before SDK removal"))
                let request = try #require(await sdk.heldSentBatchID)
                let names = await sdk.heldSavedNames
                try #require(!names.isEmpty)
                let workflow = try app.backend.workflow
                let before = try await workflow.observations()
                let receipts = await wire.receipts
                for name in names {
                    let ack = try #require(try await workflow.entityFacts(name: name))
                    let actual = try #require(receipts.last { $0.zone == zone && $0.record.name == name })
                    try #require(ack.ackedRev == ack.localRev && ack.ackedPayloadDigest == ack.payloadDigest)
                    try #require(ack.ackedSystemFields == actual.record.systemFields)
                    try #require(await sdk.pendingRecordZoneChanges()[name] == .save)
                    try #require(!before.contains(.savedPendingRemoved(name: name, requestID: request)))
                }
                try #require(!before.contains(.handlerReturned(requestID: request)))
                await handler.release()
                try await original?.drain()
                try await sdk.waitForEventBookkeeping()
                let after = try await workflow.observations()
                let returned = try #require(after.firstIndex(of: .handlerReturned(requestID: request)))
                for name in names {
                    let removed = try #require(after.firstIndex(of: .savedPendingRemoved(name: name, requestID: request)))
                    try #require(returned < removed)
                    try #require(await sdk.pendingRecordZoneChanges()[name] == nil)
                }
            } catch {
                await handler.release()
                await original?.cancelAndDrain()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKAutomaticSendUsesAdditionalOriginalBatchesForHandedBackRevision(automatic: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
            let first = ServiceAutomaticGate(), second = ServiceAutomaticGate()
            var manual: CloudKitNetworkOperation?
            var joined: Task<Void, Never>?
            do {
                if automatic {
                    app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
                    await app.sync.waitForInputs(); await app.sync.drainOperations()
                }
                let sdk = try #require(await app.factory.engines.last)
                await sdk.waitForAutomaticSend()
                let runsBefore = await sdk.automaticRuns
                await sdk.holdNextSentResult(zone: nil) { await first.hold() }
                try await automaticServiceEdit(app, id: seed.id, suffix: " First original revision")
                if !automatic {
                    let zone = try #require(fixture.records.first?.zone)
                    let prepared = try await sdk.prepareSend(zones: [zone])
                    manual = try prepared.enqueue(validate: {})
                }
                try #require(try await serviceAppSignal(first.entered, stage: "original Service reply held"))
                try #require(await sdk.automaticSchedulingEnabled == automatic)
                let firstBatch = try #require(await sdk.heldSentBatchID)
                let zone = try #require(await sdk.heldSentZone)
                let names = await sdk.heldSavedNames
                let firstIssued = await sdk.heldSentPendingRevisions
                let bits = UInt64(bitPattern: seed.id)
                let prefix: [UInt8] = [8, 1, 16, 1, 26, 8] + (0..<8).reversed().map { UInt8(truncatingIfNeeded: bits >> ($0 * 8)) }
                let noteName = try #require(fixture.records.first { $0.encrypted.starts(with: prefix) }?.name)
                try #require(names.contains(noteName))
                let backend = try app.backend
                var firstRevisions: [String: UInt64] = [:]
                for name in [noteName] {
                    let facts = try #require(try await backend.workflow.entityFacts(name: name))
                    firstRevisions[name] = facts.localRev
                    try #require(facts.inFlight.contains { $0.localRev == facts.localRev })
                }
                // From here, only an ordinary durable local edit and its
                // TrackingChanged update may drive work. No second SDK call,
                // app RequestSend, Sync Now, or fabricated batch/ACK follows.
                var appNetworkRequests = 0
                backend.onCommands = { commands in
                    for command in commands {
                        switch command.action {
                        case .requestSend, .runExchange: appNetworkRequests += 1
                        default: break
                        }
                    }
                }
                defer { backend.onCommands = nil }
                let added = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                await sdk.inspectNextPendingAddition { added.continuation.yield(true) }
                defer { added.continuation.finish() }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Later dirty revision")
                try #require(try await serviceAppSignal(added.stream, stage: "newer edited-name pending addition applied before original result"))
                let originalIssuedRevision = try #require(firstIssued[noteName])
                let newerPendingRevision = try #require(await sdk.pendingRecordZoneChangeRevisions()[noteName])
                try #require(newerPendingRevision > originalIssuedRevision)
                try #require(await sdk.pendingRecordZoneChanges()[noteName] == .save)
                let edited = try #require(try await backend.workflow.entityFacts(name: noteName))
                let firstLocalRevision = try #require(firstRevisions[noteName])
                try #require(edited.localRev > firstLocalRevision && edited.mark != nil)
                var secondBatch: UInt64?
                var secondNames: [String] = []
                var secondIssued: [String: UInt64] = [:]
                if automatic {
                    await sdk.holdNextSentResult(zone: zone) { await second.hold() }
                }
                await first.release()
                if automatic {
                    try #require(try await serviceAppSignal(second.entered, stage: "automatic additional real Core batch and Service reply"))
                    let nextBatch = try #require(await sdk.heldSentBatchID)
                    try #require(nextBatch != firstBatch)
                    let nextNames = await sdk.heldSavedNames
                    secondBatch = nextBatch
                    secondNames = nextNames
                    secondIssued = await sdk.heldSentPendingRevisions
                    let nextIssuedRevision = try #require(secondIssued[noteName])
                    try #require(nextIssuedRevision >= newerPendingRevision && nextIssuedRevision > originalIssuedRevision)
                    let repeated = Set(names).intersection(nextNames)
                    try #require(repeated.contains(noteName))
                    for name in [noteName] {
                        let facts = try #require(try await backend.workflow.entityFacts(name: name))
                        try #require(facts.ackedRev == firstRevisions[name] && facts.localRev > facts.ackedRev)
                        try #require(facts.inFlight.contains { $0.localRev == facts.localRev })
                    }
                    await second.release()
                } else {
                    try await manual?.drain()
                }
                // Bounded wait on the original owned run. An empty-batch spin
                // cannot satisfy it; failure cleanup cancels and joins that run.
                let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                joined = Task {
                    await sdk.waitForAutomaticSend()
                    completion.continuation.yield(true); completion.continuation.finish()
                }
                try #require(try await serviceAppSignal(completion.stream, stage: "automatic SDK original run joined without empty-batch spin"))
                await joined?.value
                await app.sync.waitForInputs()
                try #require(!app.sync.operationFailed)
                let removalObservations = try await backend.workflow.observations()
                // The newer applied revision prevents removal of the original
                // complete set. A real ACK alone cannot supply that receipt.
                for name in names {
                    try #require(!removalObservations.contains(.savedPendingRemoved(name: name, requestID: firstBatch)))
                }
                if automatic {
                    let request = try #require(secondBatch)
                    try #require(!secondNames.isEmpty)
                    let atRemoval = try #require(await sdk.sentRemovalPendingRevisions[request])
                    let returned = try #require(removalObservations.firstIndex(of: .handlerReturned(requestID: request)))
                    // Compare every actual issued revision at the SDK removal
                    // boundary, after the handler. Concurrent app updates may
                    // supersede this set too; its proof must then stay Missing.
                    let originalSetUnchanged = secondNames.allSatisfy { name in
                        guard let issued = secondIssued[name] else { return false }
                        return atRemoval[name] == issued
                    }
                    for name in secondNames {
                        let removed = removalObservations.firstIndex(of: .savedPendingRemoved(name: name, requestID: request))
                        if originalSetUnchanged {
                            let removal = try #require(removed)
                            try #require(returned < removal)
                        } else {
                            try #require(removed == nil, "Missing: second original set was superseded before SDK removal")
                        }
                    }
                }
                let delivered = await sdk.deliveredSentBatches.filter { $0.zone == zone }
                try #require(delivered.contains { $0.id == firstBatch })
                let receipts = await wire.receipts
                for name in [noteName] {
                    let ack = try #require(try await backend.workflow.entityFacts(name: name))
                    let saved = receipts.filter { $0.zone == zone && $0.record.name == name }
                    if automatic {
                        try #require(delivered.count >= 2 && saved.count >= 2)
                        try #require(saved.suffix(2).first?.record.envelope != saved.last?.record.envelope)
                        try #require(ack.mark == nil && ack.inFlight.isEmpty && ack.ackedRev == ack.localRev)
                        try #require(ack.ackedSystemFields == saved.last?.record.systemFields)
                    } else {
                        try #require(delivered.count == 1)
                        try #require(ack.ackedRev == firstRevisions[name] && ack.localRev > ack.ackedRev)
                        try #require(ack.mark != nil && ack.inFlight.isEmpty)
                        try #require(await sdk.pendingRecordZoneChanges()[name] == .save)
                    }
                }
                try #require(appNetworkRequests == 0)
                try #require(await sdk.automaticRuns == runsBefore + (automatic ? 1 : 0))
                let requests = await sdk.batchRequests, empty = await sdk.emptyBatchRequests
                await sdk.waitForAutomaticSend()
                try await sdk.waitForEventBookkeeping()
                try #require(await sdk.batchRequests == requests)
                try #require(await sdk.emptyBatchRequests == empty)
                try #require(empty <= 4, "One terminal empty pass is bounded by the two admitted zones")
            } catch {
                await first.release(); await second.release()
                _ = await app.sync.cancelAndDrain()
                await manual?.cancelAndDrain()
                await joined?.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKTerminalEmptyPassRetainsOnlyNewRealPendingAddition(addition: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, wire in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend()
            let terminal = ServiceAutomaticGate(), sent = ServiceAutomaticGate()
            var joined: Task<Void, Never>?
            var originalEmpty: CloudKitNetworkOperation?
            do {
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let backend = try app.backend
                let zone = try #require(fixture.records.first?.zone)
                await sdk.holdNextEmptyTerminalReturn { await terminal.hold() }
                let prepared = try await sdk.prepareSend(zones: [zone, "meguri-media"])
                originalEmpty = try prepared.enqueue(validate: {})
                try #require(try await serviceAppSignal(terminal.entered, stage: "original last nil batch terminal callback held"))
                let before = await sdk.deliveredSentBatches
                let requests = await sdk.batchRequests, empty = await sdk.emptyBatchRequests
                let runs = await sdk.automaticRuns, updates = await sdk.pendingAdditionUpdates
                let receipts = await wire.receipts.count
                var appNetworkRequests = 0
                backend.onCommands = { commands in
                    for command in commands {
                        switch command.action {
                        case .requestSend, .runExchange: appNetworkRequests += 1
                        default: break
                        }
                    }
                }
                defer { backend.onCommands = nil }
                if addition {
                    await sdk.holdNextSentResult(zone: zone) { await sent.hold() }
                    let added = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    await sdk.inspectNextPendingAddition { added.continuation.yield(true) }
                    try await automaticServiceEdit(app, id: seed.id, suffix: " New addition during terminal callback")
                    try #require(try await serviceAppSignal(added.stream, stage: "normal pending addition applied during terminal callback"))
                    added.continuation.finish()
                    // Prove normal TrackingChanged reached this SDK before
                    // releasing nil's terminal bookkeeping, not merely Core.
                    try #require(await sdk.pendingAdditionUpdates > updates)
                }
                await terminal.release()
                if addition {
                    try #require(try await serviceAppSignal(sent.entered, stage: "new wake's actual Core batch and Service response"))
                    let actual = try #require(await sdk.heldSentBatchID)
                    try #require(!before.contains { $0.zone == zone && $0.id == actual })
                    try #require(await sdk.automaticRuns == runs + 1)
                    await sent.release()
                }
                let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                let emptyOperation = originalEmpty
                joined = Task {
                    do {
                        try await emptyOperation?.drain()
                        await sdk.waitForAutomaticSend()
                        completion.continuation.yield(true)
                    } catch { Issue.record(error); completion.continuation.yield(false) }
                    completion.continuation.finish()
                }
                try #require(try await serviceAppSignal(completion.stream, stage: "terminal empty automatic runs joined without spin"))
                await joined?.value
                await app.sync.waitForInputs()
                let delivered = await sdk.deliveredSentBatches
                try #require(delivered.count == before.count + (addition ? 1 : 0))
                try #require(await sdk.automaticRuns == runs + (addition ? 1 : 0))
                if addition {
                    let saved = await wire.receipts.dropFirst(receipts)
                    try #require(!saved.isEmpty)
                    for receipt in saved {
                        let ack = try #require(try await backend.workflow.entityFacts(name: receipt.record.name))
                        try #require(ack.mark == nil && ack.inFlight.isEmpty && ack.ackedRev == ack.localRev)
                        try #require(ack.ackedSystemFields == receipt.record.systemFields)
                    }
                    try #require(await sdk.emptyBatchRequests <= empty + 3)
                } else {
                    try #require(await sdk.batchRequests == requests)
                    try #require(await sdk.emptyBatchRequests == empty)
                }
                try #require(appNetworkRequests == 0 && !app.sync.operationFailed)
                let finalRequests = await sdk.batchRequests, finalEmpty = await sdk.emptyBatchRequests
                await sdk.waitForAutomaticSend(); try await sdk.waitForEventBookkeeping()
                try #require(await sdk.batchRequests == finalRequests)
                try #require(await sdk.emptyBatchRequests == finalEmpty)
            } catch {
                await terminal.release(); await sent.release()
                _ = await app.sync.cancelAndDrain()
                await joined?.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func serviceSDKCancellationAndReleaseJoinOriginalAutomaticCallbackAndFenceAdmission(release: Bool) async throws {
        try await withAutomaticServiceApp { app, fixture, _ in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            try #require(await sdk.automaticSchedulingEnabled)
            await sdk.waitForAutomaticSend()
            let zone = try #require(fixture.records.first?.zone)
            let stale = try await sdk.prepareSend(zones: [zone])
            let handler = ServiceAutomaticGate()
            let cancellation = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let stopped = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let boundary = ServiceAutomaticGate()
            let backend = try app.backend
            await sdk.holdNextSentHandlerReturn { await handler.hold() }
            var stop: Task<Bool, Never>?
            var freshOperation: CloudKitNetworkOperation?
            var freshJoin: Task<Void, Never>?
            var returned = false
            backend.beforeQuiesce = {
                // cancelAndDrain has fenced app admission and qualified any
                // original command cancellation before reaching this hook.
                await sdk.inspectNextCancellation { cancellation.continuation.yield(true) }
                if release { await sdk.release() } else { await sdk.cancelOperations() }
                returned = true
                stopped.continuation.yield(true)
                await boundary.hold()
            }
            defer { backend.beforeQuiesce = nil; cancellation.continuation.finish(); stopped.continuation.finish() }
            do {
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let deliveredBefore = await sdk.deliveredSentBatches.count
                try await automaticServiceEdit(app, id: seed.id, suffix: " Original automatic callback")
                try #require(try await serviceAppSignal(handler.entered, stage: "original automatic sent handler held"))
                stop = Task { @MainActor in await app.sync.cancelAndDrain() }
                try #require(try await serviceAppSignal(cancellation.stream, stage: "original automatic cancellation fence"))
                try #require(!returned)
                // A newer real revision can arrive during drain, but may not
                // create a fresh automatic operation on the cancelled SDK.
                try await automaticServiceEdit(app, id: seed.id, suffix: " Dirty while original callback drains")
                await handler.release()
                try #require(try await serviceAppSignal(stopped.stream, stage: "original SDK cancellation or release returned"))
                try #require(returned)
                await sdk.waitForAutomaticSend()
                try #require(await sdk.deliveredSentBatches.count == deliveredBefore + 1)
                let requests = await sdk.batchRequests
                do {
                    let refused = try stale.enqueue(validate: {})
                    try await refused.drain()
                    Issue.record("Pre-cancellation SDK generation admitted a later network call")
                } catch {}
                if release {
                    do {
                        _ = try await sdk.prepareSend(zones: [zone])
                        Issue.record("Released SDK accepted new preparation")
                    } catch { /* retired original SDK */ }
                }
                try await automaticServiceEdit(app, id: seed.id, suffix: " Later update after drain")
                await sdk.waitForAutomaticSend()
                try #require(await sdk.batchRequests == requests)
                try #require(await sdk.deliveredSentBatches.count == deliveredBefore + 1)
                if !release {
                    let fresh = try await sdk.prepareSend(zones: [zone])
                    let operation = try fresh.enqueue(validate: {})
                    freshOperation = operation
                    let completion = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                    freshJoin = Task {
                        do {
                            try await operation.drain()
                            await sdk.waitForAutomaticSend()
                            completion.continuation.yield(true)
                        } catch { Issue.record(error); completion.continuation.yield(false) }
                        completion.continuation.finish()
                    }
                    try #require(try await serviceAppSignal(completion.stream, stage: "fresh post-cancellation SDK authority joined"))
                    await freshJoin?.value
                    try #require(await sdk.batchRequests > requests)
                    let freshRequests = await sdk.batchRequests
                    do {
                        try await stale.enqueue(validate: {}).drain()
                        Issue.record("Fresh authority revived a pre-cancellation SDK preparation")
                    } catch {}
                    try #require(await sdk.batchRequests == freshRequests)
                }
                try #require(!app.sync.operationFailed)
                await boundary.release()
                try #require(await stop?.value == true)
                try #require(!app.sync.operationFailed)
            } catch {
                await handler.release(); await boundary.release()
                if let stop { _ = await stop.value }
                else { _ = await app.sync.cancelAndDrain() }
                await freshOperation?.cancelAndDrain()
                await freshJoin?.value
                throw error
            }
        }
    }
}


// P3/P4, approved P24(d), Q11's in-force no-state row. This uses the
// production app backend (appOwnsAccountObservation=true), original Service
// database/read operations and the real Copy Debug Info producer.
extension ReviewCollectionStartupTests {
    // Named Q11 prerequisite: same-ID signIn after C1's actual root fetch,
    // before C1 returns, must read the now-deleted root and emit no root drain.
    @Test func q11ServiceAppSignInAfterRootFetchReadsDeletedActiveWithoutDrain() async throws {
        try await withAutomaticServiceApp { app, _, wire in
            let sdk = try #require(await app.factory.engines.last)
            let backend = try app.backend
            let workflow = backend.workflow
            var original: CloudPolicyCommand?
            backend.beforeExecution = { command in
                if case .runExchange = command.action { original = command }
            }
            defer { backend.beforeExecution = nil }
            let before = try await workflow.durableSnapshot()
            let drains = try await workflow.fetchDrains()
            let returned = ServiceAutomaticGate()
            await sdk.holdNextFetchReturn { await returned.hold() }
            app.sync.syncNow()
            do {
                try #require(try await serviceAppSignal(returned.entered, stage: "Q11 C1 root zone fetch completed"))
                let command = try #require(original)
                let raw = command.raw
                let mappedRuntimeID = try await MaintenanceLane.shared.call(.testCloudPolicyExchangeObservation) { @Sendable in
                    try testCloudPolicyExchangeObservation(command: raw).mappedRuntimeId
                }
                let runtimeID = try #require(mappedRuntimeID)
                let batches = await sdk.batchRequests
                let emptyBatches = await sdk.emptyBatchRequests
                let deleted = try await wire.call(.object(["DeleteZone": .object([
                    "zone": .string("meguri-root"), "cause": .string("Deleted")])]))
                #expect(try deleted.ok().text == "Done")
                try await sdk.signInSameServiceIdentity()
                await returned.release()
                await app.sync.drainOperations(); await app.sync.prepareConnectionAuthority()
                let after = try await workflow.durableSnapshot()
                let content = try #require(JSONSerialization.jsonObject(with: after.trackingContent) as? [Any])
                let binding = try #require(content.first as? [Any])
                let facts = try #require(binding.last as? [String: Any])
                #expect(facts["state"] as? [String: String] == ["Isolated": "ActiveMissing"])
                #expect(after.entities == before.entities)
                #expect(!(try await workflow.fetchDrains()).dropFirst(drains.count).contains { $0.zone == "meguri-root" })
                #expect(!after.absencePassCompleted)
                let reads = try await workflow.diagnostics().rootReads
                #expect(reads.contains { $0.active && $0.accepted && $0.outcome == "zoneNotFound" })
                let result = try #require(try await workflow.exchangeResult(id: runtimeID))
                #expect(result.id == runtimeID && result.collection == .failed)
                #expect(await sdk.batchRequests == batches)
                #expect(await sdk.emptyBatchRequests == emptyBatches)
            } catch { await returned.release(); throw error }
        }
    }

    @Test(arguments: [false, true])
    func serviceAppNoStateProofReturnsThroughCoordinatorAndCopyDebugInfo(scheduledFirst: Bool) async throws {
        try await withAutomaticServiceApp(prepare: { app in
            await app.factory.omitFirstZoneCallbacksOnNextInstance(scheduledFirst: scheduledFirst)
        }) { app, _, _ in
            let workflow = try app.backend.workflow
            let initial = try await workflow.diagnostics().rootReads
            let epoch = try #require(initial.first { !$0.active && $0.outcome == "found" && $0.accepted })
            #expect(epoch.a7Failure)
            let producer = FieldDiagnosticsStore(directory: app.root.appending(path: "diagnostics"),
                rootReadDiagnostics: { (try? await workflow.diagnostics().rootReads) ?? [] })
            let bytes = try await producer.debugInfo()
            let info = try #require(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
            let exported = try #require(info["rootReads"] as? [[String: Any]])
            #expect(exported.count == initial.count)
            #expect(exported.compactMap { $0["a7Failure"] as? Bool } == initial.map(\.a7Failure))
            let sdk = try #require(await app.factory.engines.last)
            if scheduledFirst { #expect(await sdk.joinedScheduledFetches >= 1) }
            // C1 really fetched its zones before the sign-in no-state point.
            // Hold its native return, then deliver the actual same-ID event.
            let c1 = ServiceAutomaticGate()
            await sdk.holdNextFetchReturn { await c1.hold() }
            let before = try await workflow.diagnostics().rootReads.count
            let original = Task { try await app.exchange() }
            do {
                try #require(try await serviceAppSignal(c1.entered, stage: "C1 fetched zones before same-ID signIn"))
                try await sdk.signInSameServiceIdentity()
                await c1.release()
                try await original.value
            } catch {
                await c1.release(); original.cancel(); _ = await original.result
                throw error
            }
            await app.sync.prepareConnectionAuthority()
            await app.sync.drainOperations()
            let later = try await workflow.diagnostics().rootReads
            let found = later.dropFirst(before).filter { !$0.active && $0.outcome == "found" && $0.accepted }
            #expect(!found.isEmpty && found.allSatisfy { !$0.a7Failure })
            let latestBytes = try await producer.debugInfo()
            let latestInfo = try #require(JSONSerialization.jsonObject(with: latestBytes) as? [String: Any])
            let latest = try #require(latestInfo["rootReads"] as? [[String: Any]])
            #expect(latest.compactMap { $0["a7Failure"] as? Bool } == later.map(\.a7Failure))
            #expect(!app.sync.operationFailed)
            try #require(await app.owner.close())
            await app.factory.inspectCreation { configuration in
                try #require(configuration.serialization != nil)
            }
            try await app.reopen()
            let reopenedWorkflow = try app.backend.workflow
            let reopenedProducer = FieldDiagnosticsStore(directory: app.root.appending(path: "diagnostics"),
                rootReadDiagnostics: { (try? await reopenedWorkflow.diagnostics().rootReads) ?? [] })
            let savedSDK = try #require(await app.factory.engines.last)
            try await savedSDK.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority()
            await savedSDK.omitNextZoneCallbacks()
            let prior = try await reopenedWorkflow.diagnostics().rootReads.count
            try await app.exchange()
            await app.sync.drainOperations()
            let savedReads = try await reopenedWorkflow.diagnostics().rootReads
            let savedFound = savedReads.dropFirst(prior).filter { !$0.active && $0.outcome == "found" && $0.accepted }
            #expect(!savedFound.isEmpty && savedFound.allSatisfy { !$0.a7Failure })
            let savedBytes = try await reopenedProducer.debugInfo()
            let savedInfo = try #require(JSONSerialization.jsonObject(with: savedBytes) as? [String: Any])
            let savedText = try #require(savedInfo["rootReads"] as? [[String: Any]])
            #expect(savedText.compactMap { $0["a7Failure"] as? Bool } == savedReads.map(\.a7Failure))
        }
    }

    @Test func serviceAppHeldOriginalFreshReadFinishesWithoutWaitingOnItsCommandACK() async throws {
        try await withAutomaticServiceApp { app, _, _ in
            let sdk = try #require(await app.factory.engines.last)
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority()
            await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let gate = ServiceAutomaticGate()
            await app.factory.holdNextAccountRead { await gate.hold() }
            let exchange = Task { try await app.exchange() }
            do {
                try #require(try await serviceAppSignal(gate.entered, stage: "original in-command account resolver"))
                await gate.release()
                try await exchange.value
                await app.sync.drainOperations()
                #expect(!app.sync.operationFailed)
            } catch {
                await gate.release()
                exchange.cancel(); _ = await exchange.result
                throw error
            }
        }
    }
}

@MainActor private func withAutomaticServiceApp(prepare: (@MainActor (ServiceAppHarness) async throws -> Void)? = nil, _ body: @MainActor (ServiceAppHarness, InitialCloudFixture, ServiceWorkflowWire) async throws -> Void) async throws {
    let fixture = try InitialCloudFixture.load()
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-sdk-automatic-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? FileManager.default.removeItem(at: root) }
    let serviceHome = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceHome, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceHome)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire, clock: .system)
        active = app
        defer { app.causalTrace.emit(onlyIfFailed: true) }
        defer { app.defaults.removePersistentDomain(forName: app.suite) }
        try await wire.seed(fixture)
        try await prepare?(app)
        try await app.adopt(); try await app.exchange()
        // Join the original exchange's follow-up commands before a settings
        // change retires that SDK. No preexisting request belongs to our holds.
        await app.sync.waitForInputs(); await app.sync.drainOperations()
        try #require(!app.sync.operationFailed)
        try #require(!app.sync.settings.automatic)
        try await body(app, fixture, wire)
        app.causalTrace.append("harness.close.enter")
        try #require(app.causalTrace.expectation(await app.owner.close()))
        let exit = await process.close()
        try #require(app.causalTrace.expectation(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed))
        try #require(app.causalTrace.expectation(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded))
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else { _ = await process.cancel() }
        throw error
    }
}

// Every reply below is produced by the existing Service operation. Only its
// original owner's receipt is held; enqueue/cancel and read evidence stay real.
private nonisolated struct HeldServicePublicationTransport: CloudOwnTransport {
    let original: ServiceOwnTransport
    let gate: ServiceAutomaticGate
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let operation = try original.prepare(command)
        if case .publication(.createZones) = command { return HeldServicePublicationReply(original: operation, gate: gate) }
        return operation
    }
}
private nonisolated struct HeldServicePublicationReply: CloudPreparedOwnOperation {
    let original: any CloudPreparedOwnOperation
    let gate: ServiceAutomaticGate
    func enqueue() -> Bool { original.enqueue() }
    func cancel() { original.cancel() }
    func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
    func reply() async throws -> CloudOwnTransportReply {
        let reply = try await original.reply()
        await gate.hold()
        return reply
    }
}

@MainActor private func withServicePublisher(prepare: @MainActor (ServiceAppHarness) async throws -> Void,
                                            _ body: @MainActor (ServiceAppHarness) async throws -> Void) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-own-account-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
    let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
    let bytes = try Data(contentsOf: source)
    let serviceHome = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceHome, withIntermediateDirectories: false)
    let process = try await CloudServiceProcess.open(stateDirectory: serviceHome)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let app = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire, clock: .system)
        active = app
        defer { app.defaults.removePersistentDomain(forName: app.suite) }
        try FileManager.default.createDirectory(at: app.root, withIntermediateDirectories: false)
        try bytes.write(to: app.root.appending(path: "collection.anki2"))
        try await prepare(app)
        await app.owner.start(); await app.sync.prepareConnectionAuthority()
        try await app.owner.prepareCloudConnectionPreview()
        await app.sync.waitForConnectionPreview(); await app.sync.drainOperations()
        guard case .ready(_, let summary) = app.sync.preview else { throw ServiceWorkflowFailure.wire }
        try #require(summary.decision == .confirmUpload)
        try await body(app)
        try #require(await app.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() } else { _ = await process.cancel() }
        throw error
    }
}

@MainActor private func automaticServiceEdit(_ app: ServiceAppHarness, id: Int64, suffix: String) async throws {
    let note = try await CoreNoteService().readNote(id: id)
    var fields = note.fields; fields[0] += suffix
    let mutation = try await CoreNoteService().updateNote(.init(id: id, fields: fields, tags: note.tags))
    try #require(mutation.written && mutation.id == id)
    try await CoreCollectionService().flushForBackground()
    try await app.backend.workflow.awaitTracking(generation: mutation.summary.generation, seq: mutation.summary.seq)
    await app.sync.waitForInputs()
}

#if DEBUG
nonisolated enum ServiceParkedFailureCase: CaseIterable, Sendable, Equatable {
    case providerCancellation, providerFailure, readFailure, sdkFailure, cleanup
    var failsBeforeProof: Bool { self == .readFailure || self == .sdkFailure }
}
private nonisolated final class ServiceParkedOriginalFailure: Error {}
private nonisolated final class ServiceParkedReadFault: Sendable {
    private let error = Mutex<(any Error)?>(nil)
    func arm(_ value: any Error) { error.withLock { $0 = value } }
    func take() -> (any Error)? { error.withLock { let value = $0; $0 = nil; return value } }
}
private nonisolated struct ServiceParkedReadTransport: CloudOwnTransport {
    let original: ServiceOwnTransport
    let fault: ServiceParkedReadFault
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let operation = try original.prepare(command)
        guard case .read(let target) = command, target.name == "active" else { return operation }
        return ServiceParkedReadReply(original: operation, fault: fault)
    }
}
private nonisolated struct ServiceParkedReadReply: CloudPreparedOwnOperation {
    let original: any CloudPreparedOwnOperation
    let fault: ServiceParkedReadFault
    func enqueue() -> Bool { original.enqueue() }
    func cancel() { original.cancel() }
    func controlReadEvidence() -> CloudZoneDeletionRead? { original.controlReadEvidence() }
    func reply() async throws -> CloudOwnTransportReply {
        let reply = try await original.reply()
        if let error = fault.take() { throw error }
        return reply
    }
}
#endif

private actor ServiceAutomaticGate {
    nonisolated let entered: AsyncStream<Bool>
    private let continuation: AsyncStream<Bool>.Continuation
    private var waiter: CheckedContinuation<Void, Never>?
    private var released = false
    init() { (entered, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1)) }
    func hold() async {
        continuation.yield(true)
        if !released { await withCheckedContinuation { waiter = $0 } }
    }
    func release() { released = true; waiter?.resume(); waiter = nil; continuation.finish() }
}
#endif
