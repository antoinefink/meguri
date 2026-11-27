import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

@MainActor
struct SyncCoordinatorTests {
    @Test func readyBeforeOpenReturnWaitsForRetainedHostThenStillReplacesAndReopens() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), reopened = SyncOwnerFake()
        let openReturn = SyncOwnerGate()
        var factories = 0, hostRetained = false
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            #expect(hostRetained, "Ready alone must never admit backend or SDK work")
            factories += 1
            switch factories {
            case 1: return original
            case 2:
                #expect(original.closed && original.closeCount == 1)
                return successor
            default:
                #expect(factories == 3 && successor.closed)
                return reopened
            }
        }, defaults: nil)
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation(), sync: sync)
        await sync.preparePlatform()
        // Hold the simulated open return, deliver Ready on the production
        // owner's lifecycle path, and fully join the input/replacement lanes.
        let completion = Task {
            await openReturn.hold()
            hostRetained = true
            await sync.completeOpenedHost(generation: 4)
        }
        await openReturn.waitUntilHeld()
        owner.receiveLifecycle(.opening)
        owner.receiveLifecycle(.ready(generation: 4))
        sync.collectionReplaced(4)
        await sync.waitForReplacement()
        #expect(owner.lifecycle == .ready(generation: 4) && owner.generation == 4)
        #expect(sync.generation == 4 && sync.setupState == .prepared && !sync.operationFailed)
        #expect(factories == 0 && original.events.isEmpty && original.accountReads == 0)
        openReturn.release()
        await completion.value
        await sync.prepareConnectionAuthority()
        #expect(factories == 1 && original.accountReads == 1)
        #expect(sync.setupState == .attached && !sync.operationFailed)
        owner.receiveLifecycle(.ready(generation: 4))
        await sync.waitForReplacement()
        #expect(factories == 1)

        owner.receiveLifecycle(.ready(generation: 5))
        sync.collectionReplaced(5)
        await sync.waitForReplacement(); await sync.prepareConnectionAuthority()
        #expect(factories == 2 && successor.accountReads == 1)
        #expect(sync.generation == 5 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())

        // A new open after a successful close needs retention admission again.
        hostRetained = false
        await sync.preparePlatform()
        owner.receiveLifecycle(.ready(generation: 6))
        await sync.waitForReplacement()
        #expect(factories == 2 && reopened.events.isEmpty && reopened.accountReads == 0)
        hostRetained = true
        await sync.completeOpenedHost(generation: 6)
        await sync.prepareConnectionAuthority()
        #expect(factories == 3 && reopened.accountReads == 1)
        #expect(sync.generation == 6 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())
    }

    @Test func visibleMediaReplaysOnRetriedBackendWithoutAnotherScreenUpdate() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake()
        var factories = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            factories += 1
            return factories == 1 ? original : successor
        }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 3)
        sync.visibleMedia(["current.png"], generation: 3)
        await sync.waitForInputs()
        #expect(original.visibleNames == [["current.png"]])
        original.onClose = { throw BridgeError.internal("synthetic close failure") }
        sync.collectionReplaced(4)
        sync.visibleMedia(["next.png"], generation: 4)
        await sync.waitForInputs(); await sync.waitForReplacement()
        #expect(sync.setupState == .failed && successor.visibleNames.isEmpty)
        original.onClose = nil
        // Retry the same collection generation, without another visible-set input.
        sync.collectionReplaced(4)
        await sync.waitForInputs(); await sync.waitForReplacement(); await sync.waitForInputs()
        #expect(successor.visibleNames == [["next.png"]])
        #expect(original.visibleNames == [["current.png"]])
        #expect(await sync.cancelAndDrain())
    }

    @Test func visibleMediaWaitsForBackendDeduplicatesAndReplaysAfterSuspend() async {
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        sync.visibleMedia(["question.png", "answer.mp3", "question.png"], generation: 4)
        await sync.waitForInputs()
        #expect(backend.visibleNames.isEmpty)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 4)
        await sync.waitForInputs()
        #expect(backend.visibleNames == [["answer.mp3", "question.png"]])
        sync.visibleMedia(["question.png", "answer.mp3"], generation: 4)
        await sync.waitForInputs()
        #expect(backend.visibleNames.count == 1)
        #expect(await sync.backgroundAfterLocalFlush(deadlineMilliseconds: nil))
        sync.visibleMedia(["next.png"], generation: 4)
        await sync.waitForInputs()
        #expect(backend.visibleNames.count == 1)
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        #expect(backend.visibleNames == [["answer.mp3", "question.png"], ["next.png"]])
        #expect(await sync.cancelAndDrain())
    }

    @Test func visibleMediaSurvivesFailedAttachmentButNeverCrossesCollectionGeneration() async {
        let backend = SyncOwnerFake(), successor = SyncOwnerFake()
        var attempts = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            attempts += 1
            if attempts == 1 { throw BridgeError.internal("synthetic attachment failure") }
            return attempts == 2 ? backend : successor
        }, defaults: nil)
        await sync.preparePlatform()
        sync.visibleMedia(["review.png"], generation: 7)
        await sync.completeOpenedHost(generation: 7)
        #expect(sync.setupState == .failed)
        await sync.completeOpenedHost(generation: 7)
        await sync.waitForInputs()
        #expect(backend.visibleNames == [["review.png"]])
        sync.collectionReplaced(8)
        await sync.waitForInputs(); await sync.waitForReplacement()
        await sync.waitForInputs()
        #expect(successor.visibleNames.isEmpty)
        sync.visibleMedia(["stale.png"], generation: 7)
        sync.visibleMedia(["fresh.png"], generation: 8)
        await sync.waitForInputs()
        #expect(successor.visibleNames == [["fresh.png"]])
        sync.visibleMedia([], generation: 8)
        await sync.waitForInputs()
        #expect(successor.visibleNames == [["fresh.png"], []])
        #expect(await sync.cancelAndDrain())
    }

    @Test func missingConfigurationTracksLocalGenerationsWithoutConstructingCloudOwner() async {
        let backend = SyncOwnerFake()
        var factories = 0
        let sync = SyncCoordinator(bootstrap: { throw SyncOwnerFailure.configurationUnavailable },
            makeBackend: { _ in factories += 1; return backend }, defaults: nil)
        await sync.preparePlatform()
        sync.receiveLifecycle(.ready(generation: 4))
        await sync.waitForInputs()
        await sync.waitForReplacement()
        await sync.completeOpenedHost(generation: 4)
        sync.collectionReplaced(5)
        sync.receiveLifecycle(.ready(generation: 5))
        sync.receive(.refresh(.local))
        sync.receive(.network(reachable: true))
        sync.syncNow(source: .command)
        await sync.waitForInputs()
        await sync.waitForReplacement()
        #expect(sync.generation == 5)
        #expect(sync.setupState == .configurationUnavailable && !sync.operationFailed)
        #expect(sync.status == nil && sync.preview == nil && sync.recovery == nil)
        #expect(sync.connectionService == nil)
        #expect(factories == 0 && backend.accountReads == 0 && backend.events.isEmpty)
        #expect(await sync.cancelAndDrain())
        #expect(backend.closeCount == 0)
    }

    @Test func bootstrapDoesNotReadAccountAndOwnerSurvivesLayouts() async throws {
        let backend = SyncOwnerFake()
        var order: [String] = []
        let sync = SyncCoordinator(bootstrap: { order.append("configure") },
            makeBackend: { _ in order.append("retained-host"); return backend }, defaults: nil)
        let navigation = AppNavigation()
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: navigation, sync: sync)
        await sync.preparePlatform()
        #expect(order == ["configure"])
        #expect(backend.accountReads == 0)
        await sync.completeOpenedHost(generation: 4)
        await backend.waitForAccount()
        navigation.adoptLaunchLayout(.compact)
        navigation.adoptLaunchLayout(.sidebar)
        #expect(owner.sync === sync)
        await sync.completeOpenedHost(generation: 4)
        await sync.waitForInputs()
        #expect(order == ["configure", "retained-host"])
        #expect(backend.accountReads == 1)
        #expect(await sync.cancelAndDrain())
    }

    @Test func suspendedAccountReadDoesNotBlockInputsOrReplacement() async {
        let backend = SyncOwnerFake(), successor = SyncOwnerFake()
        backend.holdAccount = true
        var attachments = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            attachments += 1
            if attachments == 1 { return backend }
            #expect(backend.closed && backend.closeCount == 1)
            return successor
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 8)
        await backend.waitForAccount()
        sync.receiveLifecycle(.ready(generation: 9))
        sync.receive(.network(reachable: false))
        sync.receive(.refresh(.local))
        await sync.waitForInputs()
        #expect(sync.generation == 9)
        await sync.waitForReplacement()
        sync.receive(.network(reachable: false))
        sync.receive(.refresh(.local))
        await sync.waitForInputs()
        #expect(successor.events.contains("offline"))
        #expect(backend.events.contains("refresh"))
        #expect(successor.events.contains("refresh"))
        #expect(attachments == 2)
        backend.releaseAccount()
        #expect(await sync.cancelAndDrain())
        #expect(backend.closed)
    }

    #if os(macOS)
    @Test func supersededAccountCancellationPreservesOutputsAndAcknowledgesBothReads() async throws {
        try await checkAccountReadFailure(superseded: true, error: CancellationError(), expectsFailure: false)
    }

    @Test func latestAccountCancellationStillReportsGenericFailure() async throws {
        try await checkAccountReadFailure(superseded: false, error: CancellationError(), expectsFailure: true)
    }

    @Test func supersededAccountGenuineFailureStillProcessesOutputsAndReportsFailure() async throws {
        try await checkAccountReadFailure(superseded: true,
            error: BridgeError.internal("synthetic-account-failure"), expectsFailure: true)
    }

    private func checkAccountReadFailure(superseded: Bool, error: any Error, expectsFailure: Bool) async throws {
        let backend = SyncOwnerFake()
        let firstGate = SyncOwnerGate(), secondGate = SyncOwnerGate()
        let firstHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let secondHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { firstHeld.continuation.finish(); secondHeld.continuation.finish() }
        var reads: [UInt64] = []
        let releaseState = SyncOwnerReleaseState()
        backend.resolveAccount = { read in
            reads.append(read)
            if read == 1 {
                firstHeld.continuation.yield(true)
                if !releaseState.releasing { await firstGate.hold() }
                return .init(outputs: [.syncNowUnavailable(.notConnected)], error: error, deferred: true)
            }
            secondHeld.continuation.yield(true)
            if !releaseState.releasing { await secondGate.hold() }
            return .init(outputs: [.syncNowUnavailable(.paused)], error: nil, deferred: false)
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 8)
        do {
            guard try await serviceAppSignal(firstHeld.stream, stage: "first account read held") else {
                throw CloudOwnTransportFailure.missingReply
            }
            let initiallyFailed = sync.operationFailed, initialRevision = sync.failureRevision
            if superseded { await sync.accountChanged() }
            // accountChanged joins the ordered beginAccountRead input, while A
            // is still held. B cannot resolve until A's completion is ACKed.
            let readsWhileFirstHeld = backend.accountReads
            sync.receive(.network(reachable: false))
            await sync.waitForInputs()
            let processedWhileFirstHeld = backend.events.contains("offline")
            firstGate.release()
            if superseded {
                guard try await serviceAppSignal(secondHeld.stream, stage: "second account read after first ACK") else {
                    throw CloudOwnTransportFailure.missingReply
                }
            } else { await sync.prepareConnectionAuthority() }
            let afterFirst = (failed: sync.operationFailed, revision: sync.failureRevision,
                unavailable: sync.syncNowUnavailable, deferred: sync.hasDeferredNativeWork)
            let refreshCount = backend.localRefreshCount
            sync.receive(.refresh(.local))
            await sync.waitForInputs()
            let processedAfterFirst = backend.localRefreshCount == refreshCount + 1
            releaseState.releasing = true
            secondGate.release()
            // Joins the original account task, including B's result ACK.
            await sync.prepareConnectionAuthority()
            let afterBoth = (failed: sync.operationFailed, revision: sync.failureRevision,
                unavailable: sync.syncNowUnavailable, deferred: sync.hasDeferredNativeWork)
            let drained = await sync.cancelAndDrain()
            // Every held continuation is released and original work joined
            // before assertions, including on the unfixed source's RED path.
            #expect(!initiallyFailed && initialRevision == 0)
            #expect(readsWhileFirstHeld == 1 && processedWhileFirstHeld)
            #expect(afterFirst.failed == expectsFailure)
            #expect(afterFirst.revision == (expectsFailure ? 1 : 0))
            #expect(afterFirst.unavailable == .notConnected && afterFirst.deferred)
            #expect(processedAfterFirst)
            #expect(reads == (superseded ? [1, 2] : [1]))
            #expect(afterBoth.failed == expectsFailure)
            #expect(afterBoth.revision == (expectsFailure ? 1 : 0))
            #expect(afterBoth.unavailable == (superseded ? .paused : .notConnected))
            #expect(afterBoth.deferred)
            #expect(drained && backend.quiesced && backend.closed && backend.closeCount == 1)
        } catch {
            releaseState.releasing = true
            firstGate.release(); secondGate.release()
            _ = await sync.cancelAndDrain()
            throw error
        }
    }
    #endif

    @Test func localFlushBoundaryAndExpiryAreOrderedAndDrainIsIdempotent() async {
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 1)
        await sync.waitForInputs()
        backend.events.removeAll()
        // The ReviewCollection lifecycle invokes these only after actual resume/flush.
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        await sync.backgroundAfterLocalFlush(deadlineMilliseconds: 123_000)
        sync.backgroundExpired()
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        await sync.waitForInputs()
        #expect(backend.events.firstIndex(of: "foreground")! < backend.events.firstIndex(of: "background")!)
        #expect(backend.events.firstIndex(of: "background")! < backend.events.firstIndex(of: "expired")!)
        #expect(await sync.cancelAndDrain())
        #expect(await sync.cancelAndDrain())
        #expect(backend.closeCount == 1)
    }


    @Test func operationSourcesKeepCloudAppliesApartFromLocalTriggers() {
        for source in [OpSource.userUndoable, .userNonUndoable, .syncIncoming, .localApi, .optimizer, .rollover, .maintenance] {
            #expect(SyncCoordinator.changeCause(source) == .local)
        }
        #expect(SyncCoordinator.changeCause(.cloudIncoming) == .cloud)
        #expect(SyncCoordinator.changeCause(.cloudRekey) == .cloud)
    }

    @Test func currentOwnerInputFailuresRemainReportedWithoutSuccessorProof() async {
        let errors: [any Error] = [BridgeError.internal("synthetic-input-failure"),
            BridgeError.cloudPolicy(.generationChanged), CloudOwnTransportFailure.unexpectedReply]
        for error in errors {
            let backend = SyncOwnerFake()
            var attachments = 0
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                attachments += 1
                return backend
            }, defaults: nil)
            await sync.preparePlatform()
            // No actual Core lifecycle can prove a successor to this fake
            // generation, regardless of other tests' process-wide Host state.
            await sync.completeOpenedHost(generation: .max)
            await sync.prepareConnectionAuthority()
            #expect(!sync.operationFailed)
            backend.inputError = error
            sync.receive(.refresh(.local))
            await sync.waitForInputs()
            await sync.waitForReplacement()
            #expect(sync.operationFailed)
            #expect(sync.generation == .max && sync.setupState == .attached)
            #expect(attachments == 1 && backend.closeCount == 0)
            #expect(await sync.cancelAndDrain())
            #expect(sync.operationFailed)
        }
    }

    @Test func secondReplacementDuringFactoryDrainsStaleCandidateBeforeLatestOwnerAttaches() async {
        let original = SyncOwnerFake(), stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate()
        var callbacks: [@Sendable () async -> Void] = []
        stale.onClose = { await staleDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
            callbacks.append(persisted)
            switch callbacks.count {
            case 1: return original
            case 2:
                #expect(original.closed)
                await factory.hold()
                return stale
            default:
                #expect(stale.closed && stale.closeCount == 1)
                return latest
            }
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 10)
        await sync.waitForInputs()
        sync.collectionReplaced(11)
        await factory.waitUntilHeld()
        sync.collectionReplaced(12)
        await sync.waitForInputs()
        factory.release()
        await staleDrain.waitUntilHeld()
        #expect(sync.generation == 12 && callbacks.count == 2)
        #expect(stale.events.isEmpty && latest.events.isEmpty)
        await callbacks[1]()
        await sync.waitForInputs()
        #expect(stale.persistedReads == 0)
        staleDrain.release()
        await sync.waitForReplacement()
        #expect(callbacks.count == 3 && sync.setupState == .attached)
        #expect(stale.closed && stale.accountReads == 0)
        await callbacks[0]()
        await callbacks[1]()
        await sync.waitForInputs()
        #expect(latest.persistedReads == 0)
        await callbacks[2]()
        await sync.waitForInputs()
        #expect(latest.persistedReads == 1)
        #expect(await sync.cancelAndDrain())
    }

    @Test func attachmentBeforeReplacementClosesStaleCandidateBeforeNextFactory() async {
        let stale = SyncOwnerFake(), successor = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate(), staleDrain = SyncOwnerGate(), finalDrain = SyncOwnerGate()
        var callbacks: [@Sendable () async -> Void] = []
        var observations: [SyncCoordinator.PlatformSignalStage] = []
        var constructing = 0, maximumConstructing = 0
        stale.onClose = { await staleDrain.hold() }
        latest.onClose = { await finalDrain.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
            constructing += 1
            maximumConstructing = max(maximumConstructing, constructing)
            defer { constructing -= 1 }
            callbacks.append(persisted)
            if callbacks.count == 1 {
                await factory.hold()
                return stale
            }
            #expect(stale.closed && stale.closeCount == 1)
            if callbacks.count == 2 { return successor }
            #expect(successor.closed && successor.closeCount == 1)
            return latest
        }, defaults: nil, observeSignals: true)
        sync.observePlatformSignalLifetime = { observations.append($0) }
        await sync.preparePlatform()
        let attachment = Task { await sync.completeOpenedHost(generation: 30) }
        await factory.waitUntilHeld()
        sync.receiveLifecycle(.ready(generation: 31))
        sync.collectionReplaced(32)
        sync.receiveLifecycle(.ready(generation: 32))
        await sync.waitForInputs()
        var repeated: Task<Void, Never>?
        await withCheckedContinuation { started in
            repeated = Task {
                started.resume()
                await sync.completeOpenedHost(generation: 30)
            }
        }
        #expect(callbacks.count == 1 && constructing == 1)
        factory.release()
        await staleDrain.waitUntilHeld()
        #expect(sync.generation == 32 && callbacks.count == 1)
        #expect(stale.events.isEmpty && stale.accountReads == 0)
        #expect(observations.isEmpty)
        await callbacks[0]()
        await sync.waitForInputs()
        #expect(stale.persistedReads == 0)
        staleDrain.release()
        await attachment.value
        await repeated?.value
        await sync.waitForReplacement()
        #expect(callbacks.count == 2 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 32 && sync.setupState == .attached && !sync.operationFailed)
        #expect(observations == [.started])
        sync.collectionReplaced(33)
        await sync.waitForReplacement()
        #expect(callbacks.count == 3 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 33 && sync.setupState == .attached && !sync.operationFailed)
        #expect(observations == [.started])
        var shutdownReturned = false
        let shutdown = Task {
            let result = await sync.cancelAndDrain()
            shutdownReturned = true
            return result
        }
        await finalDrain.waitUntilHeld()
        #expect(!shutdownReturned && !latest.closed)
        #expect(observations == [.started, .drained])
        finalDrain.release()
        #expect(await shutdown.value)
        #expect(stale.closed && successor.closed && latest.closed && latest.closeCount == 1)
        #expect(observations == [.started, .drained])
    }

    @Test func replacementBeforeCompleteOpenedHostJoinsOriginalFactory() async {
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), unexpected = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var factories = 0, constructing = 0, maximumConstructing = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            factories += 1
            constructing += 1
            maximumConstructing = max(maximumConstructing, constructing)
            defer { constructing -= 1 }
            if factories == 1 { return original }
            #expect(factories == 2)
            if factories > 2 { return unexpected }
            #expect(original.closed && original.closeCount == 1)
            await factory.hold()
            return successor
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 40)
        sync.collectionReplaced(41)
        await factory.waitUntilHeld()
        var completion: Task<Void, Never>?
        var completionReturned = false
        await withCheckedContinuation { started in
            completion = Task {
                started.resume()
                await sync.completeOpenedHost(generation: 41)
                completionReturned = true
            }
        }
        sync.receiveLifecycle(.ready(generation: 41))
        sync.collectionReplaced(41)
        await sync.waitForInputs()
        #expect(factories == 2 && constructing == 1 && !completionReturned)
        #expect(successor.events.isEmpty && sync.generation == 41)
        factory.release()
        await completion?.value
        await sync.waitForReplacement()
        // A late retention completion from the old Host cannot roll back or
        // construct another backend after the replacement has attached.
        await sync.completeOpenedHost(generation: 40)
        #expect(factories == 2 && maximumConstructing == 1 && constructing == 0)
        #expect(sync.generation == 41 && sync.setupState == .attached && !sync.operationFailed)
        #expect(await sync.cancelAndDrain())
        #expect(original.closed && successor.closed && successor.closeCount == 1)
    }

    @Test func cancelledAttachmentCallerAndShutdownJoinOriginalFactoryAndCandidateDrain() async {
        let candidate = SyncOwnerFake()
        let factoryGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
        var factories = 0, shutdownReturned = false, attachmentReturned = false
        candidate.onClose = { await closeGate.hold() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            factories += 1
            await factoryGate.hold()
            return candidate
        }, defaults: nil)
        await sync.preparePlatform()
        let attachment = Task {
            await sync.completeOpenedHost(generation: 50)
            attachmentReturned = true
        }
        await factoryGate.waitUntilHeld()
        attachment.cancel()
        var shutdown: Task<Bool, Never>?
        await withCheckedContinuation { started in
            shutdown = Task {
                started.resume()
                let result = await sync.cancelAndDrain()
                shutdownReturned = true
                return result
            }
        }
        #expect(!shutdownReturned && !attachmentReturned && factories == 1)
        factoryGate.release()
        await closeGate.waitUntilHeld()
        #expect(!shutdownReturned && !attachmentReturned && !candidate.closed)
        #expect(candidate.events.isEmpty && candidate.accountReads == 0)
        closeGate.release()
        await attachment.value
        #expect(await shutdown?.value == true)
        #expect(candidate.closed && candidate.closeCount == 1 && factories == 1)
        #expect(await sync.cancelAndDrain())
        #expect(candidate.closeCount == 1)
    }

    @Test func staleAttachmentCloseFailureRetainsOwnerUntilSuccessfulDrainBeforeNextFactory() async {
        let stale = SyncOwnerFake(), latest = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var factories = 0, closeAttempts = 0
        stale.onClose = {
            closeAttempts += 1
            throw CloudOwnTransportFailure.unexpectedReply
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            factories += 1
            if factories == 1 { await factory.hold(); return stale }
            #expect(stale.closed && stale.closeCount == 1)
            return latest
        }, defaults: nil)
        await sync.preparePlatform()
        let attachment = Task { await sync.completeOpenedHost(generation: 60) }
        await factory.waitUntilHeld()
        sync.collectionReplaced(61)
        await sync.waitForInputs()
        factory.release()
        await attachment.value
        await sync.waitForReplacement()
        #expect(closeAttempts == 2 && factories == 1 && !stale.closed)
        #expect(sync.operationFailed && sync.setupState == .failed && sync.generation == 61)
        #expect(stale.events.isEmpty && latest.events.isEmpty)
        stale.onClose = nil
        await sync.completeOpenedHost(generation: 61)
        await sync.waitForReplacement()
        #expect(stale.closed && factories == 2 && sync.setupState == .attached)
        #expect(sync.operationFailed && sync.generation == 61)
        #expect(await sync.cancelAndDrain())
        #expect(latest.closed && latest.closeCount == 1)
    }

    @Test func closeDuringSuccessorFactoryDrainsCandidateWithoutAttaching() async {
        let original = SyncOwnerFake(), candidate = SyncOwnerFake()
        let factory = SyncOwnerGate()
        var attachments = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            attachments += 1
            if attachments == 1 { return original }
            await factory.hold()
            return candidate
        }, defaults: nil)
        await sync.preparePlatform()
        await sync.completeOpenedHost(generation: 20)
        sync.collectionReplaced(21)
        await factory.waitUntilHeld()
        var close: Task<Bool, Never>?
        await withCheckedContinuation { started in
            close = Task {
                started.resume()
                return await sync.cancelAndDrain()
            }
        }
        factory.release()
        #expect(await close?.value == true)
        #expect(candidate.closed && candidate.closeCount == 1)
        #expect(candidate.events.isEmpty && candidate.accountReads == 0)
        #expect(attachments == 2)
    }

    @Test func deviceSettingsPersistWithoutEnablingOnPreviewOrCancel() async throws {
        let name = "SyncCoordinatorTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        sync.setDeviceSettings(paused: true, automatic: false)
        await sync.waitForInputs()
        await sync.cancelConnectionPreview()
        #expect(!sync.settings.enabled)
        let restored = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        // P25.1: a stored media off from an earlier build reads as on.
        #expect(restored.settings == .init(enabled: false, paused: true, automatic: false, media: true))
    }

    // sync/policy P25.5, U151's rows.

    private func round2Defaults(_ stored: [String: Bool]?) throws -> (UserDefaults, String) {
        let name = "SyncCoordinatorTests.round2.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: name))
        if let stored { defaults.set(stored, forKey: "meguri.cloud.deviceSettings") }
        return (defaults, name)
    }

    @Test func missingAutomaticReadsOnAndTheFirstInputCarriesIt() async throws {
        let (defaults, name) = try round2Defaults(nil)
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: defaults)
        #expect(sync.settings.automatic && sync.settings.media && !sync.settings.paused)
        #expect(sync.storedAutomatic && sync.automaticSwitchValue)
        var inputs: [CloudPolicyDeviceSettings] = []
        backend.routeInput = { event in
            if case .settings(let value) = event { inputs.append(value) }
            return .commands([])
        }
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        #expect(inputs.first?.automatic == true && inputs.first?.media == true)
        #expect(await sync.cancelAndDrain())
    }

    @Test func storedPauseBecomesAutomaticOffInOneWriteBeforeTheFirstInput() async throws {
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": true, "automatic": true, "media": false])
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        var inputs: [CloudPolicyDeviceSettings] = []
        backend.routeInput = { event in
            if case .settings(let value) = event { inputs.append(value) }
            return .commands([])
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: defaults)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        // No publish or adopt op: no hold. The first input is already migrated.
        #expect(inputs.first == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && !sync.updateHold)
        let stored = try #require(defaults.dictionary(forKey: "meguri.cloud.deviceSettings") as? [String: Bool])
        #expect(stored == ["enabled": true, "paused": false, "automatic": false, "media": true])
        #expect(await sync.cancelAndDrain())
        // It runs once: a later launch changes nothing, and a stored off survives.
        let later = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        #expect(later.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!later.storedAutomatic)
    }

    @Test func storedSettingsThatWereNotPausedKeepTheirValues() async throws {
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": false, "automatic": true, "media": false])
        defer { defaults.removePersistentDomain(forName: name) }
        let backend = SyncOwnerFake()
        var inputs: [CloudPolicyDeviceSettings] = []
        backend.routeInput = { event in
            if case .settings(let value) = event { inputs.append(value) }
            return .commands([])
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: defaults)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        // Only the removed media value changes; automatic and paused are untouched.
        #expect(inputs.first == .init(enabled: true, paused: false, automatic: true, media: true))
        #expect(sync.storedAutomatic)
        #expect(await sync.cancelAndDrain())
    }

    private func attach(_ stored: [String: Bool], operation: Result<Bool, any Error>?, defaults: UserDefaults)
        async -> (SyncCoordinator, [CloudPolicyDeviceSettings]) {
        defaults.set(stored, forKey: "meguri.cloud.deviceSettings")
        let backend = SyncOwnerFake()
        var inputs: [CloudPolicyDeviceSettings] = []
        backend.routeInput = { event in
            if case .settings(let value) = event { inputs.append(value) }
            return .commands([])
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: defaults)
        if let operation { sync.retainedOperationForMigration = { operation } }
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        await sync.waitForInputs()
        return (sync, inputs)
    }

    @Test func storedPauseStaysAsTheUpdateHoldOnlyWithAnOperation() async throws {
        // P25.2: a `publish` op at any step, or an `adopt` op. The op's kind and
        // step do not enter the decision: its presence alone keeps the pause.
        let (defaults, name) = try round2Defaults(nil)
        defer { defaults.removePersistentDomain(forName: name) }
        let (sync, inputs) = await attach(["enabled": true, "paused": true, "automatic": true, "media": true],
            operation: .success(true), defaults: defaults)
        #expect(inputs.first == .init(enabled: true, paused: true, automatic: false, media: true))
        #expect(sync.updateHold && !sync.storedAutomatic && !sync.automaticSwitchValue)
        #expect(defaults.bool(forKey: "meguri.sync.round2Migrated"))
        let stored = try #require(defaults.dictionary(forKey: "meguri.cloud.deviceSettings") as? [String: Bool])
        #expect(stored == ["enabled": true, "paused": true, "automatic": false, "media": true])
        #expect(await sync.cancelAndDrain())
        // It ran once. A later launch, with the op gone, does not reopen the question:
        // the hold stays until a clearing act, and Sync Automatically stays off.
        let later = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        later.retainedOperationForMigration = { .success(false) }
        await later.preparePlatform(); await later.completeOpenedHost(generation: 2); await later.waitForInputs()
        #expect(later.settings == .init(enabled: true, paused: true, automatic: false, media: true))
        #expect(await later.clearUpdateHold())
        #expect(later.settings == .init(enabled: true, paused: false, automatic: false, media: true) && !later.storedAutomatic)
        #expect(await later.cancelAndDrain())
    }


    @Test func anEarlierOffWithoutPauseIsCarriedToTheDeviceAndSurvivesAServiceChange() async throws {
        // P25.3: the earlier build kept Sync Automatically with the service's settings.
        let (defaults, name) = try round2Defaults(nil)
        defer { defaults.removePersistentDomain(forName: name) }
        let (sync, inputs) = await attach(["enabled": true, "paused": false, "automatic": false, "media": true],
            operation: nil, defaults: defaults)
        #expect(inputs.first == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
        #expect(await sync.cancelAndDrain())
        // Another service's settings arrive with their own default of on.
        // The device's value still reads off, and a setup commits it as off.
        let (other, _) = await attach(["enabled": false, "paused": false, "automatic": true, "media": true],
            operation: nil, defaults: defaults)
        #expect(!other.storedAutomatic && !other.automaticSwitchValue)
        #expect(await other.cancelAndDrain())
        // A device that never turned it off, and was not connected, stores nothing.
        let (fresh, freshName) = try round2Defaults(nil)
        defer { fresh.removePersistentDomain(forName: freshName) }
        let (untouched, _) = await attach(["enabled": false, "paused": false, "automatic": false, "media": true],
            operation: nil, defaults: fresh)
        #expect(untouched.storedAutomatic && fresh.object(forKey: "meguri.sync.automatic") == nil)
        #expect(await untouched.cancelAndDrain())
    }

    @Test func aClearingActBeforeTheMigrationCannotTurnSyncAutomaticallyOn() async throws {
        // The earlier build's pause with auto on, on a device whose migration has
        // not run (or whose write failed). Sync Now clears the hold first.
        let (defaults, name) = try round2Defaults(["enabled": true, "paused": true, "automatic": true, "media": true])
        defer { defaults.removePersistentDomain(forName: name) }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        #expect(sync.updateHold && sync.settings.automatic)
        #expect(await sync.clearUpdateHold())
        // The device's off is stored before the write, so it can never read on afterwards.
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(!sync.storedAutomatic && defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
        // The migration then finds nothing to change, and a later launch keeps off.
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2); await sync.waitForInputs()
        #expect(defaults.bool(forKey: "meguri.sync.round2Migrated"))
        #expect(sync.settings == .init(enabled: true, paused: false, automatic: false, media: true))
        #expect(await sync.cancelAndDrain())
        let later = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: defaults)
        #expect(!later.settings.automatic && !later.storedAutomatic && !later.updateHold)
    }

    @Test func resumeAndCancelOfAnOperationClearTheHoldFirst() async throws {
        // P25.2 lists `resume_recovery_op` and `cancel_recovery_op` among the clearing acts.
        for cancel in [false, true] {
            let backend = SyncOwnerFake()
            var timeline: [Bool] = []
            backend.routeInput = { event in
                if case .settings(let value) = event { timeline.append(value.paused) }
                return .commands([])
            }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
            sync.setDeviceSettings(paused: true, automatic: false)
            await sync.waitForInputs()
            #expect(sync.updateHold)
            if cancel { _ = await sync.prepareConnectionCancellation() } else { await sync.resumeRecoveryOperation() }
            await sync.waitForInputs()
            #expect(!sync.updateHold && timeline.last == false && !sync.settings.automatic)
            #expect(await sync.cancelAndDrain())
        }
    }


    @Test func offWithNoServiceSelectedOrNoICloudWritesNothing() async {
        for state in [SyncCoordinator.RenderState.off, .meguriConnected] {
            let sync = SyncCoordinator.renderFixture(state)
            let selection = sync.providerState?.selection
            #expect(!sync.serviceChangeRefusedForOperation)
            #expect(!(await sync.turnOffICloud()))
            #expect(sync.providerState?.selection == selection && sync.recovery == nil)
        }
    }

    @Test func clearingActsWritePausedOffBeforeTheirRequest() async throws {
        let backend = SyncOwnerFake()
        var timeline: [String] = []
        backend.routeInput = { event in
            switch event {
            case .settings(let value): timeline.append(value.paused ? "paused" : "clear")
            case .syncNowRequested: timeline.append("syncNow")
            default: break
            }
            return .commands([])
        }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 2)
        // The hold, as a stored pause that no control sets any more.
        sync.setDeviceSettings(paused: true, automatic: false)
        await sync.waitForInputs()
        #expect(sync.updateHold)
        timeline.removeAll()
        // A launch-like refresh, a foreground return and an account read do not clear it.
        await sync.foregroundAfterLocalResume()
        await sync.prepareConnectionAuthority()
        // An account event does not clear it either (P25.2).
        await sync.accountChanged()
        await sync.waitForInputs()
        #expect(sync.updateHold && !timeline.contains("clear"))
        // Sync Now clears it first, then forwards the request.
        sync.syncNow(source: .command)
        while sync.updateHold { await Task.yield() }
        await sync.waitForInputs()
        for _ in 0..<50 where !timeline.contains("syncNow") { await Task.yield(); await sync.waitForInputs() }
        let clear = try #require(timeline.firstIndex(of: "clear"))
        let request = try #require(timeline.firstIndex(of: "syncNow"))
        #expect(clear < request && !sync.settings.paused)
        // Auto stays as stored: off.
        #expect(!sync.settings.automatic)
        // A recovery call the person starts clears it through the same write.
        sync.setDeviceSettings(paused: true, automatic: false)
        await sync.waitForInputs()
        #expect(await sync.clearUpdateHold())
        #expect(!sync.updateHold)
        #expect(await sync.cancelAndDrain())
    }
}

extension ReviewCollectionStartupTests {
    @Test func visibleMediaNamesReachTheRealPolicyWithMediaAlwaysOn() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try files.createDirectory(at: root, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: root.appending(path: "collection.anki2"))
        let backend = SyncOwnerFake()
        var policyTimeline: [String] = []
        var issuedMediaSets: [[String]] = []
        let sync = SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-visible-media", environment: .development,
                status: .available, identity: Data("synthetic-visible-account".utf8))
        }, makeBackend: { _ in
            let policyDriver = try await CoreCloudWorkflowService().policy()
            backend.routeInput = { incoming in
                switch incoming {
                case .settings(let choice): policyTimeline.append(choice.media ? "media:on" : "media:off")
                case .visibleMedia(let references): policyTimeline.append("visible:\(references.joined(separator: ","))")
                default: break
                }
                let policyReply = try await policyDriver.input(incoming)
                if case .commands(let issuedBatch) = policyReply {
                    for issued in issuedBatch {
                        if case .prioritizeMedia(let references) = issued.action { issuedMediaSets.append(references) }
                    }
                } else { Issue.record("Expected a command batch from the policy input") }
                return policyReply
            }
            return backend
        }, defaults: nil)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        sync.setDeviceSettings(paused: false, automatic: true)
        do {
            await owner.start()
            try #require(owner.state == .ready && sync.setupState == .attached)
            await sync.waitForInputs()
            // P25.1: media is always on, so the names on screen are prioritised
            // as they change; no setting holds them back or needs to replay them.
            policyTimeline.removeAll()
            let generation = owner.generation
            try #require(generation > 0)
            sync.visibleMedia(["stale.png"], generation: generation)
            sync.visibleMedia(["head.png", "answer.mp3"], generation: generation)
            sync.visibleMedia(["old-collection.png"], generation: generation - 1)
            await sync.waitForInputs()
            #expect(policyTimeline.last == "visible:answer.mp3,head.png")
            #expect(issuedMediaSets.last == ["answer.mp3", "head.png"])
            #expect(!policyTimeline.contains { $0.contains("old-collection.png") })
            // No settings write can turn media off, whatever it changes.
            sync.setDeviceSettings(paused: false, automatic: false)
            sync.setDeviceSettings(paused: false, automatic: true)
            await sync.waitForInputs()
            #expect(!policyTimeline.contains("media:off") && sync.settings.media)
            #expect(issuedMediaSets.allSatisfy { $0 == ["answer.mp3", "head.png"] || $0 == ["stale.png"] })
            #expect(!sync.operationFailed)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }

    #if os(macOS)
    #endif
}

@MainActor
final class SyncOwnerFake: SyncCoordinatorBackend {
    var events: [String] = []
    var visibleNames: [[String]] = []
    var refreshCauses: [CloudPolicyChangeCause] = []
    private var localRefreshWaiter: CheckedContinuation<Void, Never>?
    var localRefreshCount = 0
    var accountReads = 0
    var holdAccount = false
    var accountResult: SyncCommandResult?
    var resolveAccount: (@MainActor (UInt64) async -> SyncCommandResult)?
    var quiesced = false
    var closed = false
    var closeCount = 0
    var persistedReads = 0
    var inputError: (any Error)?
    var routeInput: (@MainActor (CloudPolicyEvent) async throws -> CloudPolicyOutput)?
    var nextCommands: [CloudPolicyCommand] = []
    var executeCommand: (@MainActor (CloudPolicyCommand) async -> SyncCommandResult)?
    var executeCoreCommand: (@MainActor (CloudPolicyCommand, @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult)?
    private let executions = AsyncStream<Void>.makeStream()
    func waitForExecution() async { for await _ in executions.stream { return } }
    private let closes = AsyncStream<Void>.makeStream()
    func waitForClose() async { for await _ in closes.stream { return } }
    private let quiescences = AsyncStream<Void>.makeStream()
    func waitForQuiesce() async { for await _ in quiescences.stream { return } }
    var onQuiesce: (@MainActor () async throws -> Void)?
    var onClose: (@MainActor () async throws -> Void)?
    private var account: CheckedContinuation<Void, Never>?
    private var accountStarted: CheckedContinuation<Void, Never>?
    var connection: CoreCloudConnectionService? { nil }
    var own: CloudOwnExecutor? { nil }
    func takeAccountReadRequest() async -> Bool { persistedReads += 1; return false }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        if let inputError { throw inputError }
        if let routeInput { return try await routeInput(event) }
        switch event {
        case .visibleMedia(let names): visibleNames.append(names)
        case .foreground: events.append("foreground")
        case .background: events.append("background")
        case .backgroundExpiring: events.append("expired")
        case .network(false): events.append("offline")
        case .refresh(let cause):
            events.append("refresh"); refreshCauses.append(cause)
            if cause == .local { localRefreshCount += 1; localRefreshWaiter?.resume(); localRefreshWaiter = nil }
        default: break
        }
        let commands = nextCommands
        nextCommands = []
        return .commands(commands)
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        let result = await executeCommand?(command) ?? .init(outputs: [], error: nil, deferred: false)
        executions.continuation.yield(())
        return result
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult {
        if let executeCoreCommand { return await executeCoreCommand(command, corePrepared) }
        return await execute(command)
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { UInt64(accountReads + 1) }
    func readAccount(_ read: UInt64) async -> SyncCommandResult {
        accountReads += 1
        accountStarted?.resume(); accountStarted = nil
        if holdAccount {
            await withCheckedContinuation { continuation in
                account = continuation
            }
        }
        if let resolveAccount { return await resolveAccount(read) }
        return accountResult ?? .init(outputs: [.commands([])], error: nil, deferred: false, accountVerified: true)
    }
    func waitForLocalRefresh(after count: Int) async {
        if localRefreshCount > count { return }
        await withCheckedContinuation { localRefreshWaiter = $0 }
    }
    func waitForAccount() async {
        if accountReads > 0 { return }
        await withCheckedContinuation { accountStarted = $0 }
    }
    func releaseAccount() { account?.resume(); account = nil }
    func quiesce() async throws {
        guard !quiesced else { return }
        try await onQuiesce?(); quiesced = true; releaseAccount()
        quiescences.continuation.yield(())
    }
    func close() async throws {
        try await quiesce()
        try await onClose?(); closed = true; closeCount += 1; releaseAccount()
        closes.continuation.yield(())
    }
}

@MainActor
private final class SyncOwnerReleaseState {
    var releasing = false
}

@MainActor final class SyncOwnerGate {
    private var held = false
    private var continuation: CheckedContinuation<Void, Never>?
    private var observer: CheckedContinuation<Void, Never>?
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; held = true
            observer?.resume(); observer = nil
        }
    }
    func waitUntilHeld() async {
        if held { return }
        await withCheckedContinuation { observer = $0 }
    }
    func release() { continuation?.resume(); continuation = nil }
}
