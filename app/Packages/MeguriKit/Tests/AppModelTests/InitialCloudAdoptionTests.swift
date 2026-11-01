@testable import CoreBridge
import Foundation
@testable import Instrumentation
import func MeguriCoreBindings.testCloudPolicyExchangeObservation
import Synchronization
import Testing
@testable import AppModel

extension ReviewCollectionStartupTests {
    @Test func absentCollectionAutomaticallyAdoptsRealCloudRowsAndThenCommitsAReview() async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture)
        let probe = InitialCloudEvents()
        defer { harness.release(); harness.removeTemporaryStorage(); ChangeFanout.shared.unsubscribe(probe); probe.finish() }
        try await ChangeFanout.shared.subscribe(probe)
        do {
            await harness.owner.start()
            #expect(harness.owner.state == .noFile && harness.owner.lifecycle == .noCollection)
            try await harness.owner.prepareCloudConnectionHost()
            let original = try #require(harness.backends.first)
            let connection = try #require(original.connection)
            let executor = try #require(original.own)
            // Exercise the native executor itself: discovery grants no activation.
            guard case .ready(let handle, let summary) = try await executor.preview() else {
                throw CloudOwnTransportFailure.unexpectedReply
            }
            #expect(summary.initialAdoptionRequired && !summary.localCollectionPresent)
            #expect(!harness.sync.settings.enabled && harness.persistedEnabled == false)
            #expect(!harness.collectionExists && harness.owner.review == nil)
            try await connection.cancelPreview(handle)
            let before = harness.owner.generation
            var observedAcceptance = false
            harness.sync.observeInitialAdoptionPreview = { _ in
                #expect(!harness.sync.settings.enabled && harness.persistedEnabled == false)
            }
            harness.sync.observeInitialAdoptionAcceptance = {
                observedAcceptance = true
                #expect(harness.sync.settings.enabled && harness.persistedEnabled == true)
                #expect(!harness.collectionExists)
            }
            try await harness.beginAndWaitForPage()
            #expect(observedAcceptance)
            #expect(harness.sync.initialAdoptionActive && harness.sync.recovery?.step == .staging)
            #expect(harness.sync.preview == nil)
            #expect(harness.sync.settings.enabled && harness.persistedEnabled == true)
            #expect(!harness.sync.settings.automatic && harness.sync.settings.media)
            #expect(!harness.collectionExists && !harness.backupsExist)
            harness.page.finish(fixture.page)
            await harness.joinOriginal()
            let replacement = try await probe.next(stage: "collection-replaced") { if case .collectionReplaced = $0.kind { true } else { false } }
            await harness.sync.waitForReplacement()
            await harness.owner.waitForCloudActivation()
            await harness.sync.prepareConnectionAuthority()
            #expect(harness.owner.generation == replacement.generation && replacement.generation > before)
            #expect(harness.owner.state == .ready && harness.owner.lifecycle == .ready(generation: replacement.generation))
            #expect(harness.sync.generation == replacement.generation)
            #expect(harness.sync.preview == nil && harness.sync.recovery == nil)
            #expect(harness.sync.settings.enabled && harness.persistedEnabled == true)
            let reloaded = SyncCoordinator(bootstrap: {}, makeBackend: { _ in throw CloudOwnTransportFailure.unexpectedReply },
                defaults: UserDefaults(suiteName: harness.suite), clock: .system, directory: nil, observeSignals: false)
            #expect(reloaded.settings.enabled && !reloaded.settings.automatic && reloaded.settings.media && !reloaded.settings.paused)
            #expect(harness.backends.count == 2 && harness.backends[0].closeCount == 1)
            #expect(harness.backends[1].closeCount == 0 && !harness.backupsExist)
            try await harness.backends[1].waitForStart()
            #expect(harness.backends[1].starts.count == 1 && harness.backends[1].starts.first?.succeeded == true)
            #expect(harness.backends[1].starts.first?.automatic == false)
            #expect(harness.factory.created.count == 1 && harness.factory.maximumLive == 1)
            #expect(Set(harness.factory.created.map(\.instanceID)).count == harness.factory.created.count)
            let registration = try #require(harness.factory.created.first)
            #expect(!registration.automaticallySync && registration.serialization == nil)
            #expect(!harness.sync.settings.automatic)
            // P6: adoption starts a cursorless instance, but auto off waits for Sync Now.
            #expect(harness.factory.fetches.isEmpty)
            #expect(harness.backends[1].exchangeObservations.isEmpty)
            let workflow = harness.backends[1].workflow
            #expect(try await workflow.fetchDrains().isEmpty)
            harness.sync.receive(.network(reachable: true))
            harness.sync.syncNow()
            try await harness.backends[1].waitForExchange()
            let (initialBefore, initialAfter) = try #require(harness.backends[1].exchangeObservations.first)
            #expect(initialBefore.mapped == nil)
            let initialRuntimeID = try #require(initialAfter.mapped, "\(harness.backends[1].startDiagnostics)")
            #expect(initialAfter.policy == initialBefore.policy)
            #expect(initialAfter.captures == initialBefore.captures.merging([initialBefore.policy: initialRuntimeID]) { _, new in new })
            try await harness.factory.waitForFetch()
            let initialResult = try #require(try await workflow.exchangeResult(id: initialRuntimeID))
            #expect(initialAfter.completed.contains(initialRuntimeID) && initialAfter.active == nil)
            #expect(initialResult.id == initialRuntimeID)
            let fetch = try #require(harness.factory.fetches.first)
            #expect(fetch.instance == registration.instanceID)
            #expect(Set(fetch.options.zones) == Set(["meguri-root", registration.collectionZone]))
            #expect(!fetch.options.groupID.isEmpty)
            let drains = try await workflow.fetchDrains()
            #expect(Set(drains.filter(\.fetched).map(\.zone)).isSuperset(of: ["meguri-root", registration.collectionZone]))
            let snapshot = try await workflow.durableSnapshot()
            #expect(snapshot.rootEnvelope != nil && snapshot.inboxNames.isEmpty && snapshot.absencePassCompleted)
            #expect(Set(snapshot.entities.map(\.name)).isSuperset(of: fixture.entities.map(\.name)))
            let emitted = try await workflow.emittedUpdateIDs()
            let received = await harness.factory.engines[0].receivedUpdates()
            #expect(!received.isEmpty && Set(received).isSubset(of: Set(emitted)))
            await harness.sync.cancelConnectionPreview()
            await harness.sync.cancelConnectionOperation()
            harness.owner.navigation.dismissSheet()
            #expect(harness.owner.state == .ready && harness.owner.generation == replacement.generation)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: replacement.generation))
            #expect(harness.sync.settings.enabled && harness.backends[1].closeCount == 0)
            #expect(harness.owner.policy != nil)
            let names = try await CoreDeckService().allDeckNames()
            #expect(Set(names.decks.map(\.id)) == fixture.deckIDs)
            for note in fixture.notes {
                let actual = try await CoreNoteService().readNote(id: note.id)
                #expect(actual.fields == note.fields)
            }
            harness.owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
            if harness.owner.policy != .ok { await harness.owner.enableFsrs() }
            func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
            let deck = try #require(flatten(harness.owner.decks).first { $0.counts.new > 0 })
            await harness.owner.startStudy(deckID: deck.id)
            let review = try #require(harness.owner.review)
            try #require(review.phase == .question)
            review.reveal(); await review.settled()
            try #require(review.phase == .answer)
            let refreshBefore = harness.backends[1].localRefreshes
            review.rate(.easy); await review.settled()
            let answered = try #require(review.record.answers.first?.cardID)
            #expect(fixture.cardIDs.contains(answered))
            try await CoreCollectionService().flushForBackground()
            let committed = try await probe.next(after: replacement.seq, stage: "review-commit") {
                guard case .changes(let change) = $0.kind else { return false }
                return !change.revlogAdded.isEmpty && change.cards.contains { $0.id == answered }
            }
            await harness.sync.waitForInputs()
            #expect(committed.generation == replacement.generation)
            #expect(review.record.answers.first?.durable == true)
            #expect(harness.backends[1].localRefreshes > refreshBefore)
            let sendables = try await workflow.trackingSendables()
            #expect(!sendables.isEmpty) // The fake never confirms a server save.
            #expect(harness.factory.fetches.allSatisfy { $0.instance == registration.instanceID })
            #expect(zip(probe.events, probe.events.dropFirst()).allSatisfy { $0.0.seq < $0.1.seq })
            await harness.owner.finishReview()
            let exchangesBeforeReviewSync = harness.backends[1].exchangeObservations.count
            harness.sync.receive(.network(reachable: true))
            harness.sync.syncNow()
            try await harness.backends[1].waitForExchange()
            let (captureBefore, captureAfter) = try #require(harness.backends[1].exchangeObservations.dropFirst(exchangesBeforeReviewSync).first)
            #expect(captureBefore.mapped == nil)
            let runtimeID = try #require(captureAfter.mapped)
            #expect(captureAfter.policy == captureBefore.policy)
            #expect(captureAfter.captures == captureBefore.captures.merging([captureBefore.policy: runtimeID]) { _, new in new })
            #expect(captureBefore.policy != initialBefore.policy && runtimeID != initialRuntimeID)
            let stored = try #require(try await workflow.exchangeResult(id: runtimeID))
            #expect(captureAfter.completed.contains(runtimeID) && captureAfter.active == nil)
            #expect(stored.id == runtimeID)
            #expect(!harness.sync.settings.automatic)
            #expect(await harness.owner.close())
            #expect(harness.factory.live.isEmpty)
        } catch { await harness.cleanup(); throw error }
    }

    @Test func failedIssuedZoneDeletionDoesNotEnterCoordinatorFailureCycleOrChangeExchange() async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture, automatic: true)
        defer { harness.release(); harness.removeTemporaryStorage() }
        let boundary = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in boundary }, defaults: nil)
        do {
            try await harness.beginAndWaitForPage()
            harness.page.finish(fixture.page)
            await harness.joinOriginal()
            await harness.sync.waitForReplacement()
            await harness.owner.waitForCloudActivation()
            await harness.sync.prepareConnectionAuthority()
            let backend = try #require(harness.backends.last)
            try await backend.waitForStart()
            // Capture an actual drain-issued command from the existing app fixture.
            // The original workflow still executes every action, including RunCleanup.
            let command = try await backend.waitForZoneDeletion()
            let policy = try await backend.workflow.policy()
            let native = NativeCloudPolicyExecutor(policy: policy,
                adapter: CloudKitSyncAdapter(core: backend.workflow, engineFactory: harness.factory))
            var originalFailure: (any Error)?
            await sync.preparePlatform()
            await sync.completeOpenedHost(generation: harness.owner.generation)
            await sync.prepareConnectionAuthority()
            boundary.nextCommands = [try #require(backend.lastStatusCommand)]
            sync.receive(.refresh(.cloud))
            await boundary.waitForExecution()
            await sync.waitForInputs()
            boundary.executeCommand = { command in
                let result = await native.execute(command)
                if case .deleteZones = command.action { originalFailure = result.zoneDeletionError }
                return SyncCommandResult(result)
            }
            let before = try await policy.status()
            let appStatus = try #require(sync.status?.state)
            let revision = sync.failureRevision
            try #require(!sync.operationFailed)
            boundary.nextCommands = [command]
            sync.receive(.refresh(.cloud))
            await boundary.waitForExecution()
            await sync.waitForInputs()
            #expect(originalFailure as? BridgeError == .cloudPolicy(.coreCapabilityUnavailable))
            #expect(!sync.operationFailed && sync.status?.state == appStatus)
            let afterRevision = sync.failureRevision
            #expect(afterRevision == revision)
            let after = try await policy.status()
            #expect(after.state == before.state && after.asOfMilliseconds == before.asOfMilliseconds)
            #expect(after.lastAttempt?.exchange == before.lastAttempt?.exchange)
            #expect(after.lastCompleted?.exchange == before.lastCompleted?.exchange)
            #expect(after.lastAttempt?.endedMilliseconds == before.lastAttempt?.endedMilliseconds)
            #expect(after.lastCompleted?.endedMilliseconds == before.lastCompleted?.endedMilliseconds)
            #expect(CloudZoneDeletionDiagnostics.localFailures().contains {
                if case .localFailure(.capabilityUnavailable, let facts) = $0 { facts.code == nil } else { false }
            })
            #expect(await sync.cancelAndDrain())
            #expect(await harness.owner.close())
        } catch { _ = await sync.cancelAndDrain(); await harness.cleanup(); throw error }
    }

    @Test func automaticInitialAdoptionStartsCursorlessSDKAndFetchesRealCloudRows() async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture, automatic: true)
        defer { harness.release(); harness.removeTemporaryStorage() }
        do {
            try await harness.beginAndWaitForPage()
            let generation = harness.owner.generation
            #expect(harness.sync.settings.enabled && harness.sync.settings.automatic)
            #expect(!harness.collectionExists && harness.factory.created.isEmpty)
            harness.page.finish(fixture.page)
            await harness.joinOriginal()
            #expect(harness.page.replyReturned)
            await harness.sync.waitForReplacement()
            await harness.owner.waitForCloudActivation()
            await harness.sync.prepareConnectionAuthority()
            #expect(harness.backends.count == 2 && harness.backends[0].closeCount == 1)
            let backend = try #require(harness.backends.last)
            try await backend.waitForStart()
            try await harness.factory.waitForFetch()
            #expect(backend.starts.count == 1 && backend.starts.first?.succeeded == true)
            #expect(backend.starts.first?.automatic == true)
            #expect(harness.factory.created.count == 1 && harness.factory.maximumLive == 1)
            let registration = try #require(harness.factory.created.first)
            #expect(registration.automaticallySync && registration.serialization == nil)
            #expect(harness.sync.settings.enabled && harness.persistedEnabled == true && harness.sync.settings.automatic)
            #expect(harness.defaults.dictionary(forKey: "meguri.cloud.deviceSettings")?["automatic"] as? Bool == true)
            #expect(harness.owner.state == .ready && harness.owner.generation > generation)
            #expect(harness.sync.recovery == nil && !harness.backupsExist)
            #expect(backend.exchangeObservations.isEmpty) // No user Sync Now supplied this fetch.
            let fetch = try #require(harness.factory.fetches.first)
            #expect(fetch.instance == registration.instanceID)
            #expect(Set(fetch.options.zones) == Set(["meguri-root", registration.collectionZone]))
            #expect(!fetch.options.groupID.isEmpty)
            #expect(harness.factory.fetches.allSatisfy { $0.instance == registration.instanceID })
            let drains = try await backend.workflow.fetchDrains()
            #expect(Set(drains.filter(\.fetched).map(\.zone)).isSuperset(of: ["meguri-root", registration.collectionZone]))
            let snapshot = try await backend.workflow.durableSnapshot()
            #expect(snapshot.rootEnvelope != nil && snapshot.inboxNames.isEmpty && snapshot.absencePassCompleted)
            #expect(Set(snapshot.entities.map(\.name)).isSuperset(of: fixture.entities.map(\.name)))
            let emitted = try await backend.workflow.emittedUpdateIDs()
            let engine = try #require(harness.factory.engines.first)
            let received = await engine.receivedUpdates()
            #expect(!received.isEmpty && Set(received).isSubset(of: Set(emitted)))
            for note in fixture.notes {
                #expect(try await CoreNoteService().readNote(id: note.id).fields == note.fields)
            }
            // The fixture rejects sends, so this case checks startup and intake.
            #expect(await harness.owner.close())
            #expect(harness.factory.live.isEmpty)
        } catch { await harness.cleanup(); throw error }
    }

    @Test(arguments: [false, true])
    func initialAdoptionStagingCancelDrainsOriginalLatePageAndRestoresExactEnabled(earlier: Bool) async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture, enabled: earlier)
        defer { harness.release(); harness.removeTemporaryStorage() }
        var cancel: Task<Void, Never>?
        do {
            try await harness.beginAndWaitForPage()
            let originalGeneration = harness.owner.generation
            cancel = Task { await harness.sync.cancelConnectionOperation() }
            try await harness.page.waitForCancellation()
            #expect(harness.sync.connectionOperationActive && harness.sync.settings.enabled)
            #expect(harness.persistedEnabled == true && !harness.collectionExists)
            harness.page.finish(fixture.page) // ORIGINAL callback after cancel, not a new fetch.
            await cancel?.value
            await harness.joinOriginal()
            #expect(!harness.collectionExists && !harness.backupsExist)
            #expect(harness.owner.generation == originalGeneration && harness.owner.review == nil)
            #expect(harness.sync.preview == nil && harness.sync.recovery == nil)
            #expect(harness.sync.settings.enabled == earlier && harness.persistedEnabled == earlier)
            #expect(!harness.sync.initialAdoptionActive && !harness.sync.connectionOperationActive)
            #expect(!harness.sync.settings.automatic && harness.sync.settings.media && !harness.sync.settings.paused)
            #expect(harness.backends.count == 1)
            #expect(await harness.owner.close())
        } catch { harness.release(); await cancel?.value; await harness.cleanup(); throw error }
    }

    @Test(arguments: [false, true])
    func initialAdoptionCorruptTransportPageEndsWithoutBindingAndRestoresEnabled(earlier: Bool) async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture, enabled: earlier)
        defer { harness.release(); harness.removeTemporaryStorage() }
        do {
            try await harness.beginAndWaitForPage()
            let generation = harness.owner.generation
            let first = try #require(fixture.entities.first)
            // Corrupt only the actual transport ciphertext; Rust owns refusal.
            var bytes = try #require(first.record.encrypted)
            bytes[bytes.startIndex] ^= 1
            let corrupt = CloudStagingRecord(name: first.name, recordType: first.recordType,
                record: .init(encrypted: bytes, systemFields: first.record.systemFields))
            harness.page.finish(.page(records: [corrupt] + Array(fixture.entities.dropFirst()), deleted: [], next: nil))
            await harness.joinOriginal()
            await harness.sync.waitForInputs()
            #expect(harness.sync.operationFailed)
            #expect(harness.sync.settings.enabled == earlier && harness.persistedEnabled == earlier)
            #expect(!harness.sync.initialAdoptionActive && harness.sync.recovery == nil)
            #expect(harness.owner.generation == generation && !harness.collectionExists)
            #expect(await harness.owner.close())
        } catch { await harness.cleanup(); throw error }
    }

    @Test(arguments: [false, true])
    func initialAdoptionAcceptanceFailureRestoresExactEnabled(earlier: Bool) async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture, enabled: earlier)
        defer { harness.release(); harness.removeTemporaryStorage() }
        var observedAcceptance = false
        harness.sync.observeInitialAdoptionPreview = { handle in
            #expect(harness.sync.settings.enabled == earlier && harness.persistedEnabled == earlier)
            #expect(!harness.collectionExists)
            do { try await harness.backends[0].connection?.cancelPreview(handle) }
            catch { Issue.record("Could not invalidate the actual preview: \(String(describing: error))") }
        }
        harness.sync.observeInitialAdoptionAcceptance = {
            observedAcceptance = true
            #expect(harness.sync.settings.enabled && harness.persistedEnabled == true)
            #expect(!harness.collectionExists)
        }
        do {
            await harness.owner.start()
            try await harness.owner.prepareCloudConnectionPreview()
            await harness.sync.waitForConnectionPreview()
            #expect(observedAcceptance && harness.sync.operationFailed)
            #expect(harness.sync.settings.enabled == earlier && harness.persistedEnabled == earlier)
            #expect(!harness.sync.initialAdoptionActive && harness.sync.recovery == nil)
            #expect(!harness.collectionExists && harness.factory.created.isEmpty)
            #expect(await harness.owner.close())
        } catch { await harness.cleanup(); throw error }
    }

    @Test(arguments: [InitialCloudHold.pause, .background])
    func acceptedInitialAdoptionDrainsThenResumesAfterFreshProof(hold: InitialCloudHold) async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture)
        defer { harness.release(); harness.removeTemporaryStorage() }
        var transition: Task<Void, Never>?
        do {
            try await harness.beginAndWaitForPage()
            let generation = harness.owner.generation
            let requestsBefore = harness.requests.count
            if hold == .pause {
                harness.sync.setDeviceSettings(paused: true, automatic: false, media: true)
                transition = Task { await harness.sync.waitForInitialOwnDrain() }
            } else {
                transition = Task { harness.owner.setForeground(false); await harness.owner.waitForLifecycle() }
            }
            try await harness.page.waitForCancellation()
            #expect(harness.requests.count == requestsBefore && !harness.collectionExists)
            #expect(harness.sync.initialAdoptionActive && harness.persistedEnabled == true)
            #expect(!harness.page.replyReturned)
            harness.page.finish(fixture.page)
            await transition?.value
            await harness.joinOriginal()
            #expect(harness.page.replyReturned)
            #expect(harness.requests.count == requestsBefore)
            #expect(harness.owner.generation == generation && !harness.collectionExists)
            #expect(harness.sync.initialAdoptionActive && harness.sync.recovery != nil)
            #expect(harness.sync.settings.enabled && harness.persistedEnabled == true)
            #expect(harness.factory.created.isEmpty && harness.backends.count == 1)
            if hold == .pause {
                harness.sync.setDeviceSettings(paused: false, automatic: false, media: true)
                await harness.sync.waitForInitialOwnResume()
            } else {
                harness.owner.setForeground(true)
                await harness.owner.waitForLifecycle()
                await harness.sync.prepareConnectionAuthority()
                await harness.sync.waitForConnectionPreview()
            }
            await harness.sync.waitForReplacement()
            await harness.owner.waitForCloudActivation()
            await harness.sync.prepareConnectionAuthority()
            try await harness.backends[1].waitForStart()
            #expect(harness.backends[1].starts.count == 1 && harness.backends[1].starts.first?.succeeded == true)
            #expect(harness.backends[1].starts.first?.automatic == false)
            #expect(harness.owner.state == .ready && harness.owner.generation > generation)
            #expect(harness.sync.settings.enabled && !harness.sync.settings.automatic && !harness.sync.settings.paused)
            #expect(harness.backends.count == 2 && harness.backends[0].closeCount == 1)
            #expect(harness.factory.created.count == 1 && harness.factory.maximumLive == 1)
            let registration = try #require(harness.factory.created.first)
            #expect(!registration.automaticallySync && registration.serialization == nil)
            #expect(harness.factory.fetches.isEmpty)
            #expect(try await harness.backends[1].workflow.fetchDrains().isEmpty)
            #expect(await harness.owner.close())
        } catch { harness.release(); await transition?.value; await harness.cleanup(); throw error }
    }

    @Test func acceptedInitialAdoptionRetainsEnabledAfterTransientPageError() async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture)
        defer { harness.release(); harness.removeTemporaryStorage() }
        do {
            try await harness.beginAndWaitForPage()
            harness.page.fail()
            await harness.joinOriginal()
            #expect(harness.sync.operationFailed && harness.sync.initialAdoptionActive)
            #expect(harness.sync.recovery != nil && harness.sync.settings.enabled && harness.persistedEnabled == true)
            #expect(!harness.collectionExists && harness.factory.created.isEmpty)
            await harness.sync.accountChanged()
            await harness.sync.prepareConnectionAuthority()
            harness.sync.resumeConnection()
            await harness.sync.waitForConnectionPreview()
            await harness.sync.waitForReplacement()
            await harness.owner.waitForCloudActivation()
            await harness.sync.prepareConnectionAuthority()
            try await harness.backends[1].waitForStart()
            #expect(harness.backends[1].starts.count == 1 && harness.backends[1].starts.first?.succeeded == true)
            #expect(harness.backends[1].starts.first?.automatic == false)
            #expect(harness.owner.state == .ready && harness.sync.settings.enabled)
            #expect(!harness.sync.settings.automatic && harness.backends[0].closeCount == 1)
            #expect(harness.factory.created.count == 1 && harness.factory.maximumLive == 1)
            let registration = try #require(harness.factory.created.first)
            #expect(!registration.automaticallySync && registration.serialization == nil)
            #expect(harness.factory.fetches.isEmpty)
            #expect(try await harness.backends[1].workflow.fetchDrains().isEmpty)
            #expect(await harness.owner.close())
        } catch { await harness.cleanup(); throw error }
    }

    @Test(arguments: [InitialCloudInterruption.account, .close])
    func initialAdoptionOriginalCallbackDrainsAcrossLifecycle(interruption: InitialCloudInterruption) async throws {
        let fixture = try InitialCloudFixture.load()
        let harness = try InitialCloudHarness(fixture: fixture)
        defer { harness.release(); harness.removeTemporaryStorage() }
        var transition: Task<Void, Never>?
        do {
            try await harness.beginAndWaitForPage()
            let generation = harness.owner.generation
            switch interruption {
            case .account:
                // Real ordered Core account invalidation, with SDK resolution faked.
                harness.factory.account = .init(status: .available, identity: Data("synthetic-other-account".utf8))
                transition = Task { await harness.sync.accountChanged(); await harness.sync.prepareConnectionAuthority() }
                await transition?.value
            case .close:
                transition = Task { #expect(await harness.owner.close()) }
                try await harness.page.waitForCancellation()
            }
            #expect(!harness.collectionExists && harness.backends.count == 1)
            harness.page.finish(fixture.page)
            await transition?.value
            await harness.joinOriginal()
            #expect(!harness.collectionExists && !harness.backupsExist)
            #expect(harness.owner.generation == generation && harness.owner.review == nil)
            #expect(harness.sync.preview == nil && !harness.sync.connectionPreviewActive)
            #expect(!harness.sync.settings.enabled && harness.persistedEnabled == false)
            #expect(harness.backends.count == 1 && harness.factory.maximumLive <= 1)
            if interruption != .close { #expect(await harness.owner.close()) }
            #expect(harness.factory.live.isEmpty)
        } catch { harness.release(); await transition?.value; await harness.cleanup(); throw error }
    }
}

nonisolated enum InitialCloudHold: Sendable { case pause, background }
nonisolated enum InitialCloudInterruption: Sendable { case account, close }

nonisolated struct InitialCloudFixture: Sendable {
    struct Note: Sendable { let id: Int64; let fields: [String] }
    struct Record: Decodable, Sendable {
        let zone: String; let name: String; let recordType: String
        let encrypted: [UInt8]; let systemFields: [UInt8]
        var control: CloudControlRecord { .init(encrypted: Data(encrypted), systemFields: Data(systemFields)) }
    }
    let records: [Record]
    let notes: [Note]
    let cardIDs: Set<Int64>
    let deckIDs: Set<Int64>
    var entities: [CloudStagingRecord] {
        records.filter { $0.recordType == "MGEntity" }.map { .init(name: $0.name, recordType: $0.recordType, record: $0.control) }
    }
    var page: CloudOwnTransportReply { .page(records: entities, deleted: [], next: nil) }
    static func load() throws -> Self {
        let path = try #require(ProcessInfo.processInfo.environment["MEGURI_INITIAL_ADOPTION_FIXTURE"],
            "Export cloud_initial_app_fixture::export_initial_app_adoption_replies_from_real_publication through tools/mg first")
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        struct Envelope: Decodable { let records: [Record] }
        let envelope = try JSONDecoder().decode(Envelope.self, from: data)
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        let rows = try #require(object["sourceRows"] as? [[String: Any]])
        func ids(_ kind: String) -> Set<Int64> {
            Set(rows.compactMap { ($0[kind] as? [String: Any])?["id"] as? NSNumber }.map(\.int64Value))
        }
        let notes = try rows.compactMap { row -> Note? in
            guard let note = row["Note"] as? [String: Any] else { return nil }
            let id = try #require(note["id"] as? NSNumber).int64Value
            let bytes = try #require(note["flds"] as? [UInt8])
            return Note(id: id, fields: String(decoding: bytes, as: UTF8.self).components(separatedBy: "\u{1f}"))
        }
        try #require(notes.count == 2 && !ids("Card").isEmpty && !ids("Deck").isEmpty)
        return Self(records: envelope.records, notes: notes, cardIDs: ids("Card"), deckIDs: ids("Deck"))
    }
}

@MainActor private final class InitialCloudHarness {
    let root: URL
    let defaults: UserDefaults
    let suite: String
    let page = InitialCloudReplyGate()
    let requests = InitialCloudRequests()
    let factory: InitialCloudSDKFactory
    let fixture: InitialCloudFixture
    var backends: [InitialCloudBackend] = []
    private(set) var original: Task<Void, Never>?
    lazy var sync = makeSync()

    private func makeSync() -> SyncCoordinator {
        SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-initial-app", environment: .development,
                status: .available, identity: Data("synthetic-initial-account".utf8))
        }, makeBackend: { [self] persisted in
            #expect(backends.allSatisfy { $0.closeCount == 1 })
            let backend = try await InitialCloudBackend.make(transport: InitialCloudTransport(fixture: fixture, page: page, requests: requests),
                mediaTransport: InitialCloudMediaTransport(), factory: factory, persisted: persisted)
            backends.append(backend)
            return backend
        }, defaults: defaults, clock: .system, directory: nil, observeSignals: false)
    }
    lazy var owner: ReviewCollection = {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_735_689_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        return ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
    }()
    init(fixture: InitialCloudFixture, enabled: Bool = false, automatic: Bool = false) throws {
        self.fixture = fixture
        factory = InitialCloudSDKFactory(fixture: fixture)
        root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        suite = "synthetic-initial-adoption-\(UUID().uuidString)"
        defaults = try #require(UserDefaults(suiteName: suite))
        defaults.set(["enabled": enabled, "paused": false, "automatic": automatic, "media": true], forKey: "meguri.cloud.deviceSettings")
    }
    var persistedEnabled: Bool? { defaults.dictionary(forKey: "meguri.cloud.deviceSettings")?["enabled"] as? Bool }
    var collectionExists: Bool { FileManager.default.fileExists(atPath: root.appending(path: "collection.anki2").path) }
    var backupsExist: Bool { FileManager.default.fileExists(atPath: owner.configuration.collectionLocation.backupsDirectory.path) }
    func beginAndWaitForPage() async throws {
        if owner.state == .unopened { await owner.start() }
        try await owner.prepareCloudConnectionPreview()
        original = Task { [self] in await sync.waitForConnectionPreview(); page.finishedBeforeAdmission() }
        try await page.waitForAdmission()
    }
    func joinOriginal() async { await original?.value; await sync.waitForInputs() }
    func release() { page.finish(fixture.page) }
    func cleanup() async { release(); await joinOriginal(); _ = await owner.close() }
    func removeTemporaryStorage() {
        sync.observeInitialAdoptionPreview = nil
        sync.observeInitialAdoptionAcceptance = nil
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: root)
    }
}

private nonisolated final class InitialCloudReplyGate: CloudPreparedOwnOperation, Sendable {
    private let admissions: AsyncStream<Void>; private let admitted: AsyncStream<Void>.Continuation
    private let cancellations: AsyncStream<Void>; private let cancelled: AsyncStream<Void>.Continuation
    private struct State {
        var queued = false
        var returned = false
        var result: Result<CloudOwnTransportReply, InitialCloudPageFailure>?
        var waiter: CheckedContinuation<CloudOwnTransportReply, any Error>?
    }
    private let state = Mutex(State())
    var wasQueued: Bool { state.withLock { $0.queued } }
    var replyReturned: Bool { state.withLock { $0.returned } }
    init() {
        (admissions, admitted) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
        (cancellations, cancelled) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func enqueue() -> Bool {
        let accepted = state.withLock { s in if s.queued { return false }; s.queued = true; return true }
        if accepted { admitted.yield(()) }
        return accepted
    }
    func cancel() { cancelled.yield(()) }
    func finish(_ reply: CloudOwnTransportReply) { finishResult(.success(reply)) }
    func fail() { finishResult(.failure(InitialCloudPageFailure())) }
    private func finishResult(_ reply: Result<CloudOwnTransportReply, InitialCloudPageFailure>) {
        let waiter = state.withLock { s in
            guard s.result == nil else { return Optional<CheckedContinuation<CloudOwnTransportReply, any Error>>.none }
            s.result = reply
            let waiter = s.waiter; s.waiter = nil
            return waiter
        }
        switch reply {
        case .success(let value): waiter?.resume(returning: value)
        case .failure(let error): waiter?.resume(throwing: error)
        }
    }
    func finishedBeforeAdmission() { admitted.finish(); cancelled.finish() }
    func waitForAdmission() async throws { try await initialCloudSignal(admissions, stage: "page-admission") }
    func waitForCancellation() async throws { try await initialCloudSignal(cancellations, stage: "page-cancel") }
    func reply() async throws -> CloudOwnTransportReply {
        defer { state.withLock { $0.returned = true } }
        return try await withCheckedThrowingContinuation { continuation in
            let result: Result<CloudOwnTransportReply, InitialCloudPageFailure>? = state.withLock { s in
                if let result = s.result { return result }
                s.waiter = continuation
                return nil
            }
            switch result {
            case .success(let value): continuation.resume(returning: value)
            case .failure(let error): continuation.resume(throwing: error)
            case nil: break // Only the original callback ends this wait.
            }
        }
    }
}
private nonisolated func initialCloudSignal(_ stream: AsyncStream<Void>, stage: String,
    observations: @escaping @Sendable () async -> String = { "" }) async throws {
    try await withThrowingTaskGroup(of: Void.self) { group in
        group.addTask {
            for await _ in stream { return }
            throw CloudOwnTransportFailure.missingReply
        }
        group.addTask {
            try await Task.sleep(for: .seconds(10))
            throw InitialCloudTimeout(stage: stage, observations: await observations())
        }
        defer { group.cancelAll() }
        _ = try await group.next()
    }
}
private nonisolated struct InitialCloudTimeout: Error, CustomStringConvertible {
    let stage: String
    var observations: String = ""
    var description: String { "InitialCloudTimeout(stage: \(stage), observations: \(observations))" }
}
private nonisolated struct InitialCloudPageFailure: Error {}
private nonisolated final class InitialCloudRequests: Sendable {
    private let commands = Mutex<[CloudOwnTransportCommand]>([])
    var count: Int { commands.withLock { $0.count } }
    func record(_ command: CloudOwnTransportCommand) { commands.withLock { $0.append(command) } }
}

private nonisolated final class InitialCloudTransport: CloudOwnTransport, Sendable {
    let records: [InitialCloudFixture.Record]
    let page: InitialCloudReplyGate
    let requests: InitialCloudRequests
    init(fixture: InitialCloudFixture, page: InitialCloudReplyGate, requests: InitialCloudRequests) {
        records = fixture.records; self.page = page; self.requests = requests
    }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        requests.record(command)
        switch command {
        case .read(let target):
            let record = records.first { $0.zone == target.zone && $0.name == target.name && $0.recordType == target.recordType }
            return InitialCloudImmediateReply(.read(record.map { .found($0.control) } ?? .missing))
        case .adoptionPage(let zone, _):
            if !page.wasQueued { return page }
            let entities = records.filter { $0.zone == zone && $0.recordType == "MGEntity" }
                .map { CloudStagingRecord(name: $0.name, recordType: $0.recordType, record: $0.control) }
            return InitialCloudImmediateReply(.page(records: entities, deleted: [], next: nil))
        case .zoneDeletionActiveRead(let target):
            let record = records.first { $0.zone == target.zone && $0.name == target.name && $0.recordType == target.recordType }
            return InitialCloudImmediateReply(.zoneDeletionActiveRead(.init(read: record.map { .found($0.control) } ?? .missing)))
        case .deleteZones: throw CloudOwnTransportFailure.unexpectedReply
        case .assetRead: throw CloudOwnTransportFailure.unexpectedReply
        case .publication: throw CloudOwnTransportFailure.unexpectedReply
        }
    }
}
private nonisolated final class InitialCloudImmediateReply: CloudPreparedOwnOperation, Sendable {
    let value: CloudOwnTransportReply
    init(_ value: CloudOwnTransportReply) { self.value = value }
    func enqueue() -> Bool { true }
    func cancel() {}
    func reply() async throws -> CloudOwnTransportReply { value }
}

// This receive-only fixture has no media service. A request must fail rather
// than supplying empty data that could masquerade as a successful sync.
private nonisolated struct InitialCloudMediaTransport: CloudMediaTransport {
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        throw InitialCloudMediaFailure.unsupported
    }
}
private nonisolated enum InitialCloudMediaFailure: Error { case unsupported }

private nonisolated struct InitialCloudExchangeObservation: Sendable {
    let policy: UInt64
    let mapped: UInt64?
    let captures: [UInt64: UInt64]
    let completed: Set<UInt64>
    let active: UInt64?
    static func read(_ command: CoreBridge.CloudPolicyCommand) async throws -> Self {
        let raw = command.raw
        return try await MaintenanceLane.shared.call(BridgeCall("testCloudPolicyExchangeObservation")) {
            let value = try MeguriCoreBindings.testCloudPolicyExchangeObservation(command: raw)
            return Self(policy: value.policyExchange, mapped: value.mappedRuntimeId, captures: value.captures,
                completed: Set(value.completedIds), active: value.activeId)
        }
    }
}

// Diagnostics wrap the same composition used by the app; only transports differ.
@MainActor private final class InitialCloudBackend: SyncCoordinatorBackend {
    private let backend: NativeCollectionCloudBackend
    var connection: CoreCloudConnectionService? { backend.connection }
    var own: CloudOwnExecutor? { backend.own }
    let workflow: CoreCloudWorkflowService
    private(set) var closeCount = 0
    private(set) var localRefreshes = 0
    private let startup: AsyncStream<Void>
    private let startupFinished: AsyncStream<Void>.Continuation
    private(set) var starts: [(automatic: Bool, succeeded: Bool)] = []
    private(set) var lastStatusCommand: CloudPolicyCommand?
    private var lastPolicyStatus: CloudPolicyStatus?
    private var lastRead = "not-called"
    private var lastReadError = "none"
    private var lastExecute = "not-called"
    private var lastExecuteError = "none"
    fileprivate var startDiagnostics: String {
        let state = lastPolicyStatus.map { String(describing: $0.state) } ?? "unobserved"
        let controls = lastPolicyStatus.map { String(describing: $0.controls) } ?? "unobserved"
        let matches = lastPolicyStatus.map { String($0.accountMatchesBinding) } ?? "unobserved"
        return "startCount=\(starts.count); policyState=\(state); controls=\(controls); accountMatchesBinding=\(matches); "
            + "lastRead=\(lastRead); lastReadError=\(lastReadError); lastExecute=\(lastExecute); lastExecuteError=\(lastExecuteError)"
    }
    // Observe only statuses already returned by the real backend calls. Never dump
    // a command, account, or whole status: those can contain identifiers.
    private func observe(_ outputs: [CloudPolicyOutput]) {
        for output in outputs {
            if case .coreActionFailed(let error, _) = output {
                lastExecuteError = "coreActionFailed: \(String(describing: error))"
            }
            let commands: [CloudPolicyCommand]
            switch output {
            case .commands(let issued), .coreActionFailed(_, let issued): commands = issued
            case .applyDeferred, .syncNowUnavailable: commands = []
            }
            for command in commands {
                if case .statusChanged(let status) = command.action { lastPolicyStatus = status }
            }
        }
    }
    func waitForStart() async throws {
        try await initialCloudSignal(startup, stage: "backend-start") { [self] in await startDiagnostics }
        try #require(starts.last?.succeeded == true, "\(startDiagnostics)")
    }
    private let zoneDeletions = AsyncStream<Void>.makeStream()
    private var lastZoneDeletion: CloudPolicyCommand?
    func waitForZoneDeletion() async throws -> CloudPolicyCommand {
        try await initialCloudSignal(zoneDeletions.stream, stage: "issued-zone-deletion")
        return try #require(lastZoneDeletion)
    }
    private let exchanges: AsyncStream<Void>
    private let exchangeFinished: AsyncStream<Void>.Continuation
    private(set) var exchangeObservations: [(InitialCloudExchangeObservation, InitialCloudExchangeObservation)] = []
    func waitForExchange() async throws { try await initialCloudSignal(exchanges, stage: "exchange") }
    static func make(transport: any CloudOwnTransport, mediaTransport: any CloudMediaTransport, factory: InitialCloudSDKFactory,
                     persisted: @escaping @Sendable () async -> Void) async throws -> InitialCloudBackend {
        let workflow = CoreCloudWorkflowService()
        let backend = try await NativeCollectionCloudBackend.make(workflow: workflow, factory: factory,
            ownTransport: transport, mediaTransport: mediaTransport, persisted: persisted)
        return InitialCloudBackend(workflow: workflow, backend: backend)
    }
    private init(workflow: CoreCloudWorkflowService, backend: NativeCollectionCloudBackend) {
        self.workflow = workflow; self.backend = backend
        (exchanges, exchangeFinished) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (startup, startupFinished) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        let output = try await backend.input(event)
        if case .refresh(.local) = event { localRefreshes += 1 }
        observe([output])
        return output
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        let stage: String
        switch command.action {
        case .startEngine: stage = "startEngine"
        case .runExchange: stage = "runExchange"
        default: stage = "other-policy-command"
        }
        lastExecute = "\(stage): executing"
        var before: InitialCloudExchangeObservation?
        if case .runExchange = command.action {
            do { before = try await InitialCloudExchangeObservation.read(command) }
            catch { Issue.record("Original exchange observation failed: \(String(describing: error))") }
        }
        let result = await backend.execute(command)
        if case .deleteZones = command.action { lastZoneDeletion = command; zoneDeletions.continuation.yield(()) }
        if case .statusChanged = command.action { lastStatusCommand = command }
        lastExecute = "\(stage): returned; deferred=\(result.deferred); failed=\(result.error != nil)"
        if let error = result.error { lastExecuteError = String(describing: error) }
        observe(result.outputs)
        if case .startEngine(let automatic) = command.action {
            starts.append((automatic, result.error == nil))
            startupFinished.yield(())
        }
        if let before {
            do { exchangeObservations.append((before, try await InitialCloudExchangeObservation.read(command))) }
            catch { Issue.record("Original exchange observation failed: \(String(describing: error))") }
            exchangeFinished.yield(())
        }
        return result
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { try await backend.beginAccountRead(transition: transition) }
    func readAccount(_ id: UInt64) async -> SyncCommandResult {
        lastRead = "account-read: awaiting"
        let result = await backend.readAccount(id)
        lastRead = "account-read: returned; verified=\(result.accountVerified); failed=\(result.error != nil)"
        if let error = result.error { lastReadError = String(describing: error) }
        observe(result.outputs)
        return result
    }
    func takeAccountReadRequest() async -> Bool { await backend.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await backend.freezeForSuspend() }
    func resumeAfterCoreResume() async { await backend.resumeAfterCoreResume() }
    func close() async throws {
        try await backend.close()
        zoneDeletions.continuation.finish()
        closeCount = 1
    }
}

@MainActor private final class InitialCloudSDKFactory: CloudKitEngineFactory {
    var account = CloudResolvedAccount(status: .available, identity: Data("synthetic-initial-account".utf8))
    let fixture: InitialCloudFixture
    private let completions: AsyncStream<Void>
    private let completed: AsyncStream<Void>.Continuation
    private(set) var created: [CloudEngineConfiguration] = []
    private(set) var engines: [InitialCloudSDK] = []
    private(set) var fetches: [(instance: String, options: CloudFetchOptions)] = []
    private(set) var live: Set<String> = []
    private(set) var maximumLive = 0
    init(fixture: InitialCloudFixture) {
        self.fixture = fixture
        (completions, completed) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }
    func resolveAccount() async throws -> CloudResolvedAccount? { account }
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        #expect(live.isEmpty)
        created.append(configuration); live.insert(configuration.instanceID)
        maximumLive = max(maximumLive, live.count)
        let engine = InitialCloudSDK(instanceID: configuration.instanceID, delegate: delegate, fixture: fixture,
            fetched: { [weak self] id, options in
                self?.fetches.append((id, options)); self?.completed.yield(())
            }, released: { [weak self] id in _ = self?.live.remove(id) })
        engines.append(engine)
        return engine
    }
    func waitForFetch() async throws {
        if !fetches.isEmpty { return }
        try await initialCloudSignal(completions, stage: "factory-fetch")
    }
}
private actor InitialCloudSDK: CloudKitEngine {
    nonisolated let instanceID: String
    let delegate: any CloudKitEngineDelegate
    let fixture: InitialCloudFixture
    let fetched: @MainActor @Sendable (String, CloudFetchOptions) -> Void
    let released: @MainActor @Sendable (String) -> Void
    private var pending: [String: CloudPendingChange] = [:]
    private var updates: [UInt64] = []
    init(instanceID: String, delegate: any CloudKitEngineDelegate, fixture: InitialCloudFixture,
         fetched: @escaping @MainActor @Sendable (String, CloudFetchOptions) -> Void,
         released: @escaping @MainActor @Sendable (String) -> Void) {
        self.instanceID = instanceID; self.delegate = delegate; self.fixture = fixture
        self.fetched = fetched; self.released = released
    }
    func receivedUpdates() -> [UInt64] { updates }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] { pending }
    func pendingDatabaseChanges() async -> Set<String> { [] }
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws {
        updates.append(update.id)
        for name in update.removes { pending.removeValue(forKey: name) }
        pending.merge(update.adds) { _, next in next }
    }
    func removePendingDatabaseChanges() async {}
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue { throw CloudOwnTransportFailure.unexpectedReply }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        CloudKitNetworkEnqueue { validate in
            let original = Task { try validate(); try await self.fetchChanges(options: options) }
            return CloudKitNetworkOperation(completion: { try await original.value })
        }
    }
    func sendChanges(zones: [String]) async throws { throw CloudOwnTransportFailure.unexpectedReply }
    func fetchChanges(options: CloudFetchOptions) async throws {
        let scoped = await delegate.nextFetchChangesOptions(options, engine: self)
        #expect(scoped.groupID == options.groupID)
        await deliver(.willFetch(groupID: scoped.groupID, reason: .manual))
        for zone in scoped.zones {
            await deliver(.willFetchZone(zone: zone))
            let records = fixture.records.filter { $0.zone == zone }.map {
                CloudRecord(name: $0.name, envelope: Data($0.encrypted), systemFields: Data($0.systemFields), recordType: $0.recordType, zone: zone)
            }
            await deliver(.fetched(zone: zone, records: records, deleted: []))
            await deliver(.didFetchZone(zone: zone, error: nil))
        }
        await deliver(.didFetch(groupID: scoped.groupID, reason: .manual))
        await fetched(instanceID, scoped)
    }
    private func deliver(_ event: CloudEngineEvent) async {
        await delegate.eventWillDeliver(engine: self)
        await delegate.handleEvent(event, engine: self)
        await delegate.eventHandlerReturned(event, engine: self)
    }
    func cancelOperations() async {}
    func release() async { await released(instanceID) }
}

@MainActor final class InitialCloudEvents: ChangeFanoutSink {
    private let stream: AsyncStream<CoreEvent>
    private let continuation: AsyncStream<CoreEvent>.Continuation
    private(set) var events: [CoreEvent] = []
    init() { (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .unbounded) }
    func receive(_ event: CoreEvent) { events.append(event); continuation.yield(event) }
    func finish() { continuation.finish() }
    func next(after seq: UInt64 = 0, stage: String, matching predicate: @escaping @Sendable (CoreEvent) -> Bool) async throws -> CoreEvent {
        if let event = events.first(where: { $0.seq > seq && predicate($0) }) { return event }
        let stream = stream
        return try await withThrowingTaskGroup(of: CoreEvent.self) { group in
            group.addTask {
                for await event in stream where event.seq > seq && predicate(event) { return event }
                throw CloudOwnTransportFailure.missingReply
            }
            group.addTask { try await Task.sleep(for: .seconds(10)); throw InitialCloudTimeout(stage: stage) }
            defer { group.cancelAll() }
            guard let event = try await group.next() else { throw CloudOwnTransportFailure.missingReply }
            return event
        }
    }
}
