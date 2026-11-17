import Foundation
import Synchronization
import Testing
@testable import AppModel
@testable import CoreBridge

#if os(macOS)
extension ReviewCollectionStartupTests {
    @Test func legacyUpgradeKeepsTheRealRetainedICloudBindingPendingRowsAndSettings() async throws {
        try await withShutdownHarness { harness in
            _ = try await CoreDeckService().addDeck(name: "Synthetic pending upgrade deck")
            try await CoreCollectionService().flushForBackground()
            await harness.sync.waitForInputs()
            await harness.sync.drainOperations()
            let original = try harness.backend
            try #require(await harness.sync.cancelAndDrain())
            let before = try await original.workflow.durableSnapshot()
            let providerFile = harness.root.appending(path: "synthetic-installation/provider.json")
            let oldDocument = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: providerFile)) as? [String: Any])
            try #require(await harness.owner.close())
            try FileManager.default.removeItem(at: providerFile)
            harness.defaults.set(["enabled": true, "paused": true, "automatic": false, "media": false], forKey: "meguri.cloud.deviceSettings")
            let reopened = try ServiceAppHarness(root: harness.root, wire: harness.wire, defaults: harness.defaults)
            do {
                await reopened.owner.start()
                try #require(reopened.owner.state == .ready)
                let settings = reopened.sync.settings
                #expect(settings.enabled && settings.paused && !settings.automatic && !settings.media)
                #expect(reopened.sync.providerState?.selection == .iCloud)
                let after = try await reopened.backend.workflow.durableSnapshot()
                #expect(before.binding == after.binding)
                #expect(before.entities == after.entities)
                #expect(before.inboxNames == after.inboxNames)
                let newDocument = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: providerFile)) as? [String: Any])
                let oldBinding = try #require(oldDocument["binding"] as? NSDictionary)
                let newBinding = try #require(newDocument["binding"] as? NSDictionary)
                #expect(oldBinding.isEqual(newBinding))
                try #require(await reopened.owner.close())
            } catch { _ = await reopened.owner.close(); throw error }
        }
    }
    @Test func providerHandoffJoinsTheOriginalCoreCommandBeforeCommittingMeguriSetup() async throws {
        try await withShutdownHarness { harness in
            var originalCoordinator: SyncCoordinator? = harness.sync
            weak let originalLifetime = originalCoordinator
            let backend = try harness.backend
            await harness.sync.drainOperations()
            let gate = SyncOwnerGate()
            let held = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var originalReturned = false, handoffReturned = false, once = false
            backend.beforeExecution = { command in
                guard case .resumeMedia = command.action, !once else { return }
                once = true; held.continuation.yield(true)
                await gate.hold()
                #expect(Task.isCancelled)
                originalReturned = true
            }
            defer { backend.beforeExecution = nil; gate.release(); held.continuation.finish() }
            harness.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            try #require(try await serviceAppSignal(held.stream, stage: "provider original command"))
            let changing = Task {
                let success = await harness.sync.changeProvider(to: .meguri)
                handoffReturned = true
                return success
            }
            do {
                try await backend.waitForQuiesce()
                #expect(!originalReturned && !handoffReturned && !backend.closed)
                #expect(originalLifetime != nil, "Keep the original owner through its actual callback join")
                #expect(harness.sync.providerState?.selection == .iCloud)
                #expect(harness.sync.providerState?.phase == .handoffRecovery)
                gate.release()
                #expect(await changing.value)
                #expect(originalReturned && backend.closed && backend.closeReturned == 1)
                #expect(harness.sync.providerState?.selection == .meguri)
                #expect(harness.sync.providerState?.phase == .setupRequired)
                #expect(!harness.sync.settings.automatic)
                #expect(harness.sync !== originalCoordinator)
                #expect(harness.sync.setupState == .attached)
                #expect(try harness.backend !== backend)
                originalCoordinator?.beginConnectionPreview()
                await originalCoordinator?.waitForConnectionPreview()
                #expect(originalCoordinator?.preview == nil)
                originalCoordinator = nil
                #expect(originalLifetime == nil, "The attached successor must release its drained predecessor")
                weak let secondLifetime = harness.sync
                try #require(await harness.sync.changeProvider(to: .localOnly))
                #expect(secondLifetime == nil, "Repeated handoffs must not retain a coordinator chain")
                observeShutdownReturnPreview(harness, test: .providerHandoff, stage: .beforePrepare)
                try await harness.owner.prepareCloudConnectionPreview()
                observeShutdownReturnPreview(harness, test: .providerHandoff, stage: .afterPrepare)
                await harness.sync.waitForConnectionPreview()
                observeShutdownReturnPreview(harness, test: .providerHandoff, stage: .afterWait)
                try #require(harness.sync.preview != nil && harness.sync.connectionFailure == nil)
                await harness.sync.keepLocalOnly()
                #expect(harness.sync.providerState?.selection == .localOnly)
                #expect(harness.owner.state == .ready && harness.sync.setupState == .attached)
            } catch { gate.release(); _ = await changing.value; throw error }
        }
    }
    @Test(arguments: ["factory", "providerRefresh"])
    func failedSuccessorAttachmentRetriesOnceOnTheCurrentHost(failure: String) async throws {
        try await withShutdownHarness { app in
            let original = app.sync
            let originalBackend = try app.backend
            // Initial adoption really replaced the Host. This facade belongs
            // to that retired generation; it cannot refresh for this successor.
            let retiredProvider = try #require(app.backends.first?.providerService)
            do { _ = try await retiredProvider.refresh(); Issue.record("Expected a retired provider") }
            catch { /* The negative uses actual Core ownership, never a state DTO. */ }
            var attempts = 0
            app.beforeBackend = {
                attempts += 1
                if failure == "factory", attempts == 1 {
                    throw BridgeError.cloudConnection(.accountUnavailable)
                }
            }
            app.afterBackend = { backend in
                if failure == "providerRefresh", attempts == 1 {
                    backend.attachmentProvider = retiredProvider
                }
            }
            defer { app.beforeBackend = nil; app.afterBackend = nil }
            #expect(!(await original.changeProvider(to: .meguri)))
            let current = app.sync
            try #require(current !== original && current.setupState == .failed)
            #expect(originalBackend.closed && originalBackend.closeReturned == 1)
            #expect(attempts == 1 && current.operationFailed)
            #expect(current.providerState?.selection == .meguri)
            #expect(current.providerState?.phase == .setupRequired)
            if failure == "factory" { #expect(current.connectionFailure == .accountUnavailable) }
            if failure == "providerRefresh" {
                let failedBackend = try app.backend
                #expect(failedBackend.providerService === retiredProvider)
                #expect(!failedBackend.closed)
            }
            // Failure does not close the collection or require a study restart.
            if app.owner.policy != .ok { await app.owner.enableFsrs() }
            let deck = try await CoreDeckService().addDeck(name: "Synthetic successor failure study")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic successor QA", cloze: false,
                fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
            _ = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Question", "Answer"],
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            await app.owner.startStudy(deckID: deck.id)
            let review = try #require(app.owner.review)
            try #require(review.phase == .question)
            review.reveal(); await review.settled()
            review.rate(.easy); await review.settled()
            #expect(review.record.answers.count == 1)
            await app.owner.finishReview()
            #expect(attempts == 1, "Local work must not automatically retry attachment")
            await current.retryProviderAuthority()
            try #require(app.sync === current && current.setupState == .attached)
            #expect(attempts == 2 && current.connectionFailure == nil)
            let backend = try app.backend
            let provider = try #require(backend.providerService)
            #expect(provider !== retiredProvider && backend !== originalBackend)
            #expect(try await provider.state().selection == .meguri)
            let credentials = try await current.meguriConnection()
            try await credentials.configure(endpoint: "https://sync.invalid")
            let configured = try #require(try await provider.configuredMeguri())
            #expect(URL(string: configured.connection.endpoint)?.host == "sync.invalid")
            #expect(await credentials.phase == .signedOut)
            #expect(current.providerState?.phase == .setupRequired)
            await current.retryProviderAuthority()
            #expect(attempts == 2, "An attached owner refreshes instead of repeating its factory")
            try #require(await current.changeProvider(to: .localOnly))
            #expect(app.owner.state == .ready && app.sync.setupState == .attached)
            original.beginConnectionPreview()
            await original.waitForConnectionPreview()
            #expect(original.preview == nil && originalBackend.closeReturned == 1)
        }
    }
    @Test func actualCollectionReplacementRetiresProviderAndCredentialOwnersBeforeCurrentSettingsAndReturn() async throws {
        var oldProvider: CoreProviderService?
        var oldCredentials: MeguriProviderConnectionOwner?
        try await withShutdownHarness(beforeAdopt: { app in
            #if DEBUG
            app.observeRuntimePreparationRefusals = true
            #endif
            app.sync.observeInitialAdoptionPreview = { _ in
                oldProvider = try? await CoreProviderService.attached()
                oldCredentials = try? await app.sync.meguriConnection()
            }
        }) { app in
            let original = try #require(app.backends.first)
            try #require(app.backends.count >= 2 && original.closed && original.closeReturned == 1)
            let provider = try #require(oldProvider)
            let credentials = try #require(oldCredentials)
            do { _ = try await provider.state(); Issue.record("Replaced provider facade remained usable") }
            catch { /* The original Core-issued generation must remain rejected. */ }
            do { try await credentials.deleteCredentialsAfterDrain(); Issue.record("Replaced credential owner remained usable") }
            catch { /* Retired credentials cannot act for the successor. */ }
            app.sync.setDeviceSettings(paused: true, automatic: false, media: false)
            await app.sync.waitForInputs()
            #expect(app.sync.settings.paused && !app.sync.settings.automatic && !app.sync.settings.media)
            #expect(app.sync.providerState?.settings == app.sync.settings)
            #expect(!app.sync.operationFailed)
            app.sync.setDeviceSettings(paused: false, automatic: false, media: false)
            await app.sync.waitForInputs()
            try #require(await app.sync.changeProvider(to: .localOnly))
            observeShutdownReturnPreview(app, test: .collectionReplacement, stage: .beforePrepare)
            try await app.owner.prepareCloudConnectionPreview()
            observeShutdownReturnPreview(app, test: .collectionReplacement, stage: .afterPrepare)
            await app.sync.waitForConnectionPreview()
            observeShutdownReturnPreview(app, test: .collectionReplacement, stage: .afterWait)
            try #require(app.sync.preview != nil && app.sync.connectionFailure == nil)
            #expect(app.sync.providerState?.selection == .localOnly)
            await app.sync.keepLocalOnly()
            #expect(app.owner.state == .ready && !app.sync.operationFailed)
        }
    }
    @Test func realOwnerQuitCancelsAndJoinsMediaBeforeClosingItsCollection() async throws {
        try await withShutdownHarness { harness in
            let backend = try harness.backend
            await harness.sync.drainOperations()
            try #require(!harness.sync.operationFailed && !backend.quiesced)
            let mediaGate = SyncOwnerGate()
            let held = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var mediaHeld = false, mediaReturned = false, closeReturned = false
            backend.beforeExecution = { command in
                guard case .resumeMedia = command.action, !mediaHeld else { return }
                mediaHeld = true
                held.continuation.yield(true)
                await mediaGate.hold()
                #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                mediaReturned = true
            }
            defer { backend.beforeExecution = nil; mediaGate.release(); held.continuation.finish() }
            var quitting: Task<Bool, Never>?
            do {
                harness.sync.setDeviceSettings(paused: false, automatic: true, media: true)
                try #require(try await serviceAppSignal(held.stream, stage: "owner quit media held"))
                quitting = Task {
                    let result = await harness.owner.closeForQuit()
                    closeReturned = true
                    return result
                }
                try await backend.waitForQuiesce()
                #expect(!mediaReturned && !closeReturned && !backend.closed)
                mediaGate.release()
                let close = try #require(quitting)
                #expect(await close.value)
                #expect(mediaReturned && closeReturned && backend.closed && backend.closeReturned == 1)
                #expect(!harness.sync.operationFailed)
            } catch {
                mediaGate.release()
                if let quitting { _ = await quitting.value }
                throw error
            }
        }
    }

    @Test(arguments: [false, true], [false, true])
    func quitJoinsItsCommandWithoutWaitingForUnrelatedMedia(flush: Bool, fails: Bool) async throws {
        try await withShutdownCommands { media, _ in
            let policy = try await CoreCloudWorkflowService().policy()
            if flush { _ = try await policy.input(.syncNowRequested(.command)) }
            let output = try await policy.input(.quitRequested)
            guard case .commands(let commands) = output else {
                throw BridgeError.internal("Quit command fixture failed")
            }
            let quit = try #require(commands.first { command in
                if flush, case .quitFlush = command.action { return true }
                if !flush, case .quitNow = command.action { return true }
                return false
            })
            let backend = SyncOwnerFake(), mediaGate = SyncOwnerGate(), quitGate = SyncOwnerGate()
            let returned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var mediaReturned = false, mediaCancelled = false, allowed = false, quitACKs = 0
            backend.executeCommand = { command in
                if command.raw === media.raw {
                    await mediaGate.hold()
                    mediaCancelled = Task.isCancelled
                    mediaReturned = true
                    return .init(outputs: [], error: CancellationError(), deferred: false)
                }
                #expect(command.raw === quit.raw && !Task.isCancelled)
                await quitGate.hold()
                return .init(outputs: [.syncNowUnavailable(.paused)],
                    error: fails ? BridgeError.cloudPolicy(.staleCommand) : nil, deferred: false)
            }
            backend.onQuiesce = { mediaGate.release() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 90)
            await sync.prepareConnectionAuthority()
            sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command, command.raw === quit.raw {
                    quitACKs += 1
                }
            }
            defer { sync.observeCausalBoundary = nil; returned.continuation.finish() }
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            await mediaGate.waitUntilHeld()
            backend.routeInput = { event in
                if case .quitRequested = event { return .commands([quit]) }
                return .commands([])
            }
            let quitting = Task {
                await sync.quitAfterLocalFlush { allowed = true }
                returned.continuation.yield(true)
            }
            do {
                await quitGate.waitUntilHeld()
                #expect(!allowed && !mediaReturned && !backend.quiesced)
                quitGate.release()
                try #require(try await serviceAppSignal(returned.stream, stage: "quit command joined while media is held"))
                await quitting.value
                #expect(allowed == (flush || !fails))
                #expect(quitACKs == 1 && sync.syncNowUnavailable == .paused)
                #expect(!mediaReturned && !backend.quiesced && !backend.closed)
                #expect(await sync.cancelAndDrain())
                #expect(mediaReturned && mediaCancelled && backend.closed && backend.closeCount == 1)
                #expect(sync.operationFailed == fails)
            } catch {
                quitGate.release(); mediaGate.release()
                await quitting.value
                _ = await sync.cancelAndDrain()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func shutdownRequestsOriginalCancellationBeforeRevokingProofAndKeepsProviderFailure(earlierProviderFailure: Bool) async throws {
        try await withShutdownCommands { media, _ in
            let backend = SyncOwnerFake(), provider = SyncOwnerGate(), delivery = SyncOwnerGate()
            let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let proof = CloudOriginalCommandProofYield()
            let providerError = BridgeError.internal("synthetic-original-proof-provider")
            let cancellation = Mutex<(pending: Bool, shutdown: Bool)?>(nil)
            var executions = 0, returnedError: (any Error)?
            #if DEBUG
            var acknowledgements = 0
            #endif
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 201)
            await sync.prepareConnectionAuthority()
            backend.resolveAccount = { _ in
                entered.continuation.yield(true)
                await provider.hold()
                return .init(outputs: [], error: earlierProviderFailure ? providerError : nil,
                    deferred: false, accountVerified: !earlierProviderFailure)
            }
            backend.executeCommand = { command in
                #expect(command.raw === media.raw)
                executions += 1
                guard let admit = CloudOriginalCommandProofYield.admission,
                      let context = SyncCommandTaskContext.current else {
                    Issue.record("Missing original command proof admission")
                    return .init(outputs: [], error: providerError, deferred: false)
                }
                let intent = context.cancellationIntent
                // Model the inner proof bridge without automatically cancelling
                // its child: the original's handler must run before revocation.
                let child = Task { try await admit(proof) }
                let terminal = await withTaskCancellationHandler {
                    let terminal = await child.result
                    if earlierProviderFailure { await delivery.hold() }
                    return terminal
                } onCancel: {
                    let shutdown = intent.capture().request?.source == .shutdown
                    cancellation.withLock { $0 = (proof.isPending, shutdown) }
                }
                #expect(Task.isCancelled && context.shutdownCancelled)
                switch terminal {
                case .success:
                    Issue.record("Original proof unexpectedly succeeded")
                    return .init(outputs: [], error: nil, deferred: false)
                case .failure(let error):
                    returnedError = error
                    return .init(outputs: [], error: error, deferred: false)
                }
            }
            backend.onQuiesce = { provider.release(); delivery.release() }
            #if DEBUG
            sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command, command.raw === media.raw {
                    acknowledgements += 1
                }
            }
            #endif
            defer {
                provider.release(); delivery.release(); entered.continuation.finish()
                #if DEBUG
                sync.observeCausalBoundary = nil
                #endif
            }
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            do {
                try #require(try await serviceAppSignal(entered.stream, stage: "original proof resolver held before shutdown"))
                try #require(proof.isPending)
                if earlierProviderFailure {
                    provider.release()
                    await delivery.waitUntilHeld()
                    #expect(!proof.isPending && sync.operationFailed && sync.failureRevision == 1)
                }
                #expect(await sync.cancelAndDrain())
                let observation = try #require(cancellation.withLock { $0 })
                #expect(observation.shutdown && observation.pending == !earlierProviderFailure)
                #expect(executions == 1 && backend.closed && backend.closeCount == 1)
                #if DEBUG
                #expect(acknowledgements == 1)
                #endif
                #expect(sync.operationFailed == earlierProviderFailure)
                #expect(sync.failureRevision == (earlierProviderFailure ? 2 : 0))
                if earlierProviderFailure { #expect((returnedError as? BridgeError) == providerError) }
                else { #expect(returnedError is CancellationError) }
            } catch {
                provider.release(); delivery.release(); _ = await sync.cancelAndDrain()
                throw error
            }
        }
    }

    @Test func ownedShutdownCancellationAcknowledgesOriginalMediaAndReopensHealthy() async throws {
        try await withShutdownCommands { media, _ in
            let original = SyncOwnerFake(), successor = SyncOwnerFake()
            let commandGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var attachments = 0, returnedCancelled = false, executions = 0, closeReturned = false
            original.executeCommand = { _ in
                executions += 1
                #expect(!Task.isCancelled)
                await commandGate.hold()
                returnedCancelled = Task.isCancelled
                return .init(outputs: [.commands([media])], error: CancellationError(), deferred: false)
            }
            original.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                attachments += 1; return attachments == 1 ? original : successor
            }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 100)
            await sync.prepareConnectionAuthority()
            original.nextCommands = [media]
            sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
            await closeGate.waitUntilHeld()
            #expect(!closeReturned && !returnedCancelled)
            closeGate.release()
            await original.waitForQuiesce()
            #expect(original.quiesced && !original.closed && !closeReturned && !returnedCancelled)
            commandGate.release()
            #expect(await close.value)
            #expect(returnedCancelled && closeReturned && executions == 1)
            #expect(original.closed && original.closeCount == 1)
            #expect(!sync.operationFailed && sync.failureRevision == 0)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 101)
            await sync.prepareConnectionAuthority()
            #expect(attachments == 2 && sync.setupState == .attached && !sync.operationFailed)
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test func normalShutdownResultsRunAndAcknowledgeRequiredEffectsBeforeCloseFinishes() async throws {
        try await withShutdownCommands { media, update in
            for lateDelivery in [false, true] {
                for requiredFailure in [false, true] {
                    let backend = SyncOwnerFake()
                    let commandGate = SyncOwnerGate(), requiredGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
                    var executions = 0, closeReturned = false
                    backend.executeCommand = { command in
                        executions += 1
                        if case .resumeMedia = command.action {
                            #expect(executions == 1 && !Task.isCancelled)
                            await commandGate.hold()
                            #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                            return .init(outputs: [.commands([update])], error: CancellationError(), deferred: false)
                        }
                        guard case .updatePending = command.action else {
                            Issue.record("Expected required updatePending effect")
                            return .init(outputs: [], error: nil, deferred: false)
                        }
                        #expect(executions == 2 && !Task.isCancelled)
                        #expect(backend.quiesced == lateDelivery && !backend.closed)
                        await requiredGate.hold()
                        let error: (any Error)? = requiredFailure ? BridgeError.internal("synthetic-required-effect-failure") : nil
                        return .init(outputs: [.commands([media])], error: error, deferred: false)
                    }
                    backend.onQuiesce = { await closeGate.hold() }
                    let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 150)
                    await sync.prepareConnectionAuthority()
                    backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
                    await commandGate.waitUntilHeld()
                    let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
                    await closeGate.waitUntilHeld()
                    if lateDelivery { closeGate.release(); await backend.waitForQuiesce() }
                    commandGate.release()
                    await requiredGate.waitUntilHeld()
                    #expect(!closeReturned && executions == 2)
                    if !lateDelivery { closeGate.release(); await backend.waitForQuiesce() }
                    #expect(backend.quiesced && !backend.closed && !closeReturned)
                    requiredGate.release()
                    #expect(await close.value)
                    #expect(closeReturned && executions == 2 && backend.closeCount == 1)
                    #expect(sync.operationFailed == requiredFailure)
                    #expect(sync.failureRevision == (requiredFailure ? 1 : 0))
                }
            }
        }
    }

    #if DEBUG
    @Test(arguments: [false, true])
    func nativePreparationAndExpiryIntentAloneRemainUnownedAndSticky(alreadyCancelled: Bool) async throws {
        try await withShutdownHarness(mediaEnabled: false) { app in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: false)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let original = try app.backend
            let policy = try await original.workflow.policy()
            guard case .commands(let commands) = try await policy.input(.foreground) else {
                throw BridgeError.internal("Expected actual foreground commands")
            }
            let command = try #require(commands.first { if case .requestFetch = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            #expect(try await execution.originalSDK() != nil)
            let backend = SyncOwnerFake(), held = SyncOwnerGate()
            var returned = false
            backend.executeCommand = { issued in
                #expect(issued.raw === command.raw)
                if alreadyCancelled { withUnsafeCurrentTask { $0?.cancel() } }
                await held.hold()
                let result = SyncCommandResult(outputs: [], error: CancellationError(), deferred: false)
                #expect(result.nativeCancellation == nil)
                let context = SyncCommandTaskContext.current
                #expect(Task.isCancelled && context != nil)
                #expect(context?.ownsNativeCancellation(result, command: command, backend: backend,
                    generation: 173, revision: context?.revision ?? 0) == false)
                returned = true
                return result
            }
            backend.onQuiesce = { held.release() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 173)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [command]; sync.receive(.refresh(.cloud))
            await held.waitUntilHeld()
            sync.backgroundExpired()
            held.release()
            await backend.waitForExecution(); await sync.waitForInputs(); await sync.drainOperations()
            #expect(returned && sync.operationFailed && sync.failureRevision == 1)
            #expect(await sync.cancelAndDrain())
            #expect(sync.operationFailed && sync.failureRevision == 1)
        }
    }

    @Test(arguments: [false, true])
    func nativeOriginalFetchCancellationMatchesExpiryOrReplacementIntent(replacement: Bool) async throws {
        try await withShutdownHarness(mediaEnabled: false) { app in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: false)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let sdk = try #require(await app.factory.engines.last)
            await sdk.waitForAutomaticSend()
            try await sdk.signInSameServiceIdentity()
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            await sdk.omitNextZoneCallbacks()
            let original = try app.backend
            let policy = try await original.workflow.policy()
            let issued = try await policy.input(.foreground)
            guard case .commands(let commands) = issued else { throw BridgeError.internal("Expected actual foreground commands") }
            let mismatch = try #require(commands.first { if case .requestFetch = $0.action { true } else { false } })
            let native = SyncOwnerGate(), provider = SyncOwnerGate()
            let parked = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let proofWaiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let providerReturned = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var receipts = 0, acknowledgements = 0
            var originalCommands: [CloudPolicyCommand] = []
            var originalResults: [CloudPolicyCommand] = []
            var originalACKs: [CloudPolicyCommand] = []
            let capturingForeground = Mutex(false)
            original.beforeExecution = { command in
                if capturingForeground.withLock({ $0 }), case .requestFetch = command.action { originalCommands.append(command) }
            }
            var contexts: [SyncCommandTaskContext] = []
            app.sync.observeQuiescentDependency = { parked.continuation.yield($0) }
            app.sync.observeAccountPolicyDrain = { if case .waiting = $0 { proofWaiting.continuation.yield(true) } }
            app.sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command,
                   originalCommands.contains(where: { $0.raw === command.raw }) {
                    originalACKs.append(command); acknowledgements += 1
                }
            }
            original.afterExecution = { command, result in
                guard originalCommands.contains(where: { $0.raw === command.raw }) else { return }
                guard let context = SyncCommandTaskContext.current else { Issue.record("Missing retained original context"); return }
                #expect(result.error is CancellationError && result.nativeCancellation != nil)
                #expect(context.ownsNativeCancellation(result, command: command, backend: original,
                    generation: context.generation, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: mismatch, backend: original,
                    generation: context.generation, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: command, backend: SyncOwnerFake(),
                    generation: context.generation, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: command, backend: original,
                    generation: context.generation + 1, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: command, backend: original,
                    generation: context.generation, revision: context.revision + 1))
                let unowned = SyncCommandTaskContext(generation: context.generation, revision: context.revision, backend: original,
                                                     originalCommands: [command])
                unowned.activeCommandIndex = 0
                #expect(!unowned.ownsNativeCancellation(result, command: command, backend: original,
                    generation: context.generation, revision: context.revision))
                unowned.recordCancellation(.expiry, alreadyCancelled: true)
                #expect(!unowned.ownsNativeCancellation(result, command: command, backend: original,
                    generation: context.generation, revision: context.revision))
                #expect(result.acknowledgedOutputCount == result.outputs.count)
                originalResults.append(command)
                contexts.append(context); receipts += 1
            }
            defer {
                original.beforeExecution = nil; original.afterExecution = nil
                app.sync.observeQuiescentDependency = nil; app.sync.observeAccountPolicyDrain = nil
                app.sync.observeCausalBoundary = nil
                native.release(); provider.release()
                parked.continuation.finish(); proofWaiting.continuation.finish(); providerReturned.continuation.finish()
            }
            await sdk.holdNextFetchReturn { await native.hold() }
            capturingForeground.withLock { $0 = true }
            app.sync.receive(.foreground)
            await native.waitUntilHeld()
            capturingForeground.withLock { $0 = false }
            try #require(originalCommands.count == 1)
            capturingForeground.withLock { $0 = true }
            app.sync.receive(.foreground)
            try #require(try await serviceAppSignal(parked.stream, stage: "native dependent accepted"))
            capturingForeground.withLock { $0 = false }
            try #require(originalCommands.count == 2)
            #expect(originalCommands[0].raw !== originalCommands[1].raw)
            await app.sync.accountChanged()
            try #require(try await serviceAppSignal(proofWaiting.stream, stage: "account proof held by original fetch"))
            await app.factory.holdNextAccountRead { await provider.hold(); providerReturned.continuation.yield(true) }
            native.release()
            await provider.waitUntilHeld()
            if replacement {
                let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
                app.owner.entry.inspect(source); await app.owner.entry.waitForCompletion()
                try #require(app.owner.entry.preview != nil)
                app.owner.entry.confirm(); await app.owner.entry.waitForCompletion()
                await app.sync.waitForReplacement()
            } else {
                app.sync.backgroundExpired()
                try #require(await app.owner.close())
                try await app.reopen()
            }
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            #expect(receipts == 2 && acknowledgements == 2)
            for command in originalCommands {
                #expect(originalResults.filter { $0.raw === command.raw }.count == 1)
                #expect(originalACKs.filter { $0.raw === command.raw }.count == 1)
            }
            #expect(contexts.allSatisfy { !$0.shutdownCancellationRequested })
            #expect(original.closed && original.closeReturned == 1)
            #expect(!app.sync.operationFailed && app.sync.failureRevision == 0)
            let successor = try app.backend
            #expect(successor !== original)
            let snapshot = try await successor.workflow.durableSnapshot()
            let guardBefore = try await successor.workflow.currentGuard()
            provider.release()
            try #require(try await serviceAppSignal(providerReturned.stream, stage: "late original provider returned"))
            await app.sync.prepareConnectionAuthority(); await app.sync.drainOperations()
            #expect(try await successor.workflow.durableSnapshot() == snapshot)
            #expect(try await successor.workflow.currentGuard() == guardBefore)
            #expect(!app.sync.operationFailed && app.sync.failureRevision == 0)
        }
    }
    @Test(arguments: [false, true])
    func queuedOriginalMediaCancellationOwnsOnlyFrozenQueueAndPreservesEarlierProviderFailure(earlierFailure: Bool) async throws {
        try await withShutdownHarness { app in
            app.sync.setDeviceSettings(paused: false, automatic: true, media: true)
            await app.sync.waitForInputs(); await app.sync.drainOperations()
            let original = try app.backend
            let policy = try await original.workflow.policy()
            guard case .commands(let issued) = try await policy.input(.foreground),
                  case .commands(let later) = try await policy.input(.foreground) else {
                throw BridgeError.internal("Expected actual foreground command lists")
            }
            let active = try #require(issued.first { if case .requestFetch = $0.action { true } else { false } })
            let queued = try #require(issued.first { if case .resumeMedia = $0.action { true } else { false } })
            let unissued = try #require(later.first { if case .resumeMedia = $0.action { true } else { false } })
            let backend = SyncOwnerFake(), held = SyncOwnerGate()
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: app.sync.generation)
            await sync.prepareConnectionAuthority()
            var context: SyncCommandTaskContext?
            var results: [CloudPolicyCommand] = [], acknowledgements: [CloudPolicyCommand] = []
            var providerError: (any Error)?
            sync.observeCausalBoundary = { boundary, command in
                guard let command, command.raw === active.raw || command.raw === queued.raw else { return }
                if case .commandAcknowledgementJoined = boundary { acknowledgements.append(command) }
            }
            backend.executeCommand = { command in
                results.append(command)
                if command.raw === active.raw {
                    context = SyncCommandTaskContext.current
                    #expect(context != nil && !Task.isCancelled)
                    if earlierFailure {
                        await app.factory.resolveNextAccountRead(with: .failure(ServiceWorkflowFailure.wire))
                        do { _ = try await app.factory.resolveAccount(); Issue.record("Expected original provider failure") }
                        catch { providerError = error }
                    }
                    await held.hold()
                    #expect(Task.isCancelled && context?.shutdownCancellationRequested == false)
                    return .init(outputs: [], error: providerError, deferred: false)
                }
                #expect(CloudPolicyOutputAcknowledgement.current != nil)
                #expect(CloudPolicyOutputAcknowledgement.input != nil)
                guard command.raw === queued.raw, let context else {
                    let result = await original.production.execute(command)
                    #expect(result.error == nil)
                    return result
                }
                #expect(SyncCommandTaskContext.current === context && Task.isCancelled)
                let result = await original.production.execute(command)
                #expect(result.error is CancellationError && result.nativeCancellation?.matches(command) == true)
                #expect(context.ownsNativeCancellation(result, command: command, backend: backend,
                    generation: context.generation, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: active, backend: backend,
                    generation: context.generation, revision: context.revision))
                #expect(!context.ownsNativeCancellation(result, command: unissued, backend: backend,
                    generation: context.generation, revision: context.revision))
                checkFrozenQueueOwnership(result, command: command, other: active, unissued: unissued, backend: backend)
                #expect(result.acknowledgedOutputCount == result.outputs.count)
                return result
            }
            defer { held.release(); backend.executeCommand = nil; sync.observeCausalBoundary = nil }
            backend.nextCommands = [active, queued]; sync.receive(.refresh(.cloud))
            await held.waitUntilHeld()
            sync.backgroundExpired()
            held.release()
            await sync.drainOperations()
            for command in [active, queued] {
                #expect(results.filter { $0.raw === command.raw }.count == 1)
                #expect(acknowledgements.filter { $0.raw === command.raw }.count == 1)
            }
            #expect((providerError != nil) == earlierFailure)
            #expect(sync.operationFailed == earlierFailure && sync.failureRevision == (earlierFailure ? 1 : 0))
            #expect(context?.shutdownCancellationRequested == false)
            #expect(await sync.cancelAndDrain())
            #expect(context?.shutdownCancellationRequested == false)
            #expect(sync.operationFailed == earlierFailure && sync.failureRevision == (earlierFailure ? 1 : 0))
            #expect(!app.sync.operationFailed && !original.closed)
        }
    }
    #endif

    @Test func backgroundExpiryCancellationHeldThroughCloseRemainsUnownedAndSticky() async throws {
        try await withShutdownCommands { media, _ in
            let backend = SyncOwnerFake()
            let commandGate = SyncOwnerGate(), cancelledGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var closeReturned = false, returnedCancelled = false
            backend.executeCommand = { _ in
                #expect(!Task.isCancelled)
                let context = SyncCommandTaskContext.current
                #expect(context != nil)
                await commandGate.hold()
                #expect(Task.isCancelled && context?.shutdownCancellationRequested == false)
                await cancelledGate.hold()
                returnedCancelled = Task.isCancelled
                #expect(context?.shutdownCancellationRequested == false)
                return .init(outputs: [], error: CancellationError(), deferred: false)
            }
            backend.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 175)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            sync.backgroundExpired()
            commandGate.release()
            await cancelledGate.waitUntilHeld()
            let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
            await closeGate.waitUntilHeld()
            closeGate.release(); await backend.waitForQuiesce()
            #expect(backend.quiesced && !backend.closed && !closeReturned && !returnedCancelled)
            cancelledGate.release()
            #expect(await close.value)
            #expect(returnedCancelled && closeReturned && backend.closeCount == 1)
            #expect(sync.operationFailed && sync.failureRevision == 1)
        }
    }

    @Test func currentCommandFailuresStayStickyIncludingDuringShutdown() async throws {
        try await withShutdownCommands { media, _ in
            for duringClose in [false, true] {
                let errors: [any Error] = [CancellationError(), BridgeError.cloudPolicy(.staleCommand),
                                          BridgeError.internal("synthetic-command-failure")]
                for error in errors {
                    let backend = SyncOwnerFake(), gate = SyncOwnerGate()
                    var cancelled = false
                    backend.executeCommand = { _ in
                        await gate.hold(); cancelled = Task.isCancelled
                        return .init(outputs: [], error: error, deferred: false)
                    }
                    backend.onQuiesce = { gate.release() }
                    let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 200)
                    await sync.prepareConnectionAuthority()
                    backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
                    await gate.waitUntilHeld()
                    if !duringClose {
                        gate.release(); await backend.waitForExecution(); await sync.waitForInputs()
                        #expect(!cancelled && sync.operationFailed && sync.failureRevision == 1)
                    }
                    #expect(await sync.cancelAndDrain())
                    let expectedFailure = !duringClose || !(error is CancellationError)
                    #expect(cancelled == duringClose)
                    #expect(sync.operationFailed == expectedFailure)
                    #expect(sync.failureRevision == (expectedFailure ? 1 : 0))
                    #expect(backend.closed && backend.closeCount == 1)
                }
            }
        }
    }

    @Test func genuineFailureOutputsDuringCloseStillReportAndRunRequiredEffects() async throws {
        try await withShutdownCommands { media, update in
            let backend = SyncOwnerFake(), commandGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var executions = 0
            backend.executeCommand = { command in
                executions += 1
                if case .resumeMedia = command.action {
                    await commandGate.hold()
                    #expect(Task.isCancelled)
                    return .init(outputs: [.coreActionFailed(error: .cloudPolicy(.staleCommand), commands: [update]),
                                           .applyDeferred(error: .cancelled)],
                        error: CancellationError(), deferred: false)
                }
                #expect(!backend.closed)
                // Required follow-up work was issued after the shutdown cancel
                // pass: its CancellationError has no original-task cancellation.
                #expect(!Task.isCancelled)
                return .init(outputs: [], error: CancellationError(), deferred: false)
            }
            backend.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 250)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            let close = Task { await sync.cancelAndDrain() }
            await closeGate.waitUntilHeld()
            commandGate.release()
            await backend.waitForExecution(); await backend.waitForExecution()
            await sync.waitForInputs()
            #expect(executions == 2 && sync.operationFailed && sync.failureRevision == 3)
            closeGate.release()
            #expect(await close.value)
            #expect(sync.operationFailed && backend.closed)
        }
    }

}
#endif


extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func originalCompletedProjectionFailureStaysStickyWhenCloseFollows(cancellation: Bool) async {
        let backend = SyncOwnerFake()
        // This seam supplies an already completed original projection result;
        // it supplies no native prepared-read refusal witness.
        let error: any Error = cancellation ? CancellationError() : BridgeError.internal("synthetic-original-projection")
        backend.accountResult = .init(outputs: [], error: error, deferred: false)
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 301)
        await sync.prepareConnectionAuthority()
        #expect(sync.operationFailed && sync.failureRevision == 1)
        #expect(await sync.cancelAndDrain())
        #expect(sync.operationFailed && sync.failureRevision == 1 && backend.closeCount == 1)
    }

    @Test func unownedAccountCancellationDuringCloseRemainsFailure() async {
        let backend = SyncOwnerFake()
        backend.holdAccount = true
        backend.accountResult = .init(outputs: [], error: CancellationError(), deferred: false)
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 300)
        await backend.waitForAccount()
        #expect(await sync.cancelAndDrain())
        #expect(sync.operationFailed && sync.failureRevision == 1)
    }

    @Test func shutdownCloseFailureRetainsBackendAndFailureForExplicitDrainRetry() async {
        let backend = SyncOwnerFake()
        backend.onClose = { throw CancellationError() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 400)
        await sync.prepareConnectionAuthority()
        #expect(!(await sync.cancelAndDrain()))
        #expect(sync.operationFailed && !backend.closed && backend.closeCount == 0)
        backend.onClose = nil
        #expect(await sync.cancelAndDrain())
        #expect(sync.operationFailed && backend.closed && backend.closeCount == 1)
    }

}

#if os(macOS)
extension ReviewCollectionStartupTests {
    @Test func replacedResultsAndSameGenerationRetiredCallbacksRemainFenced() async throws {
        try await withShutdownCommands { media, _ in
            // Reusing the local generation across close/reopen also exercises
            // hostRevision fencing, independently of the generation comparison.
            for replacement in [false, true] {
                let original = SyncOwnerFake(), successor = SyncOwnerFake(), gate = SyncOwnerGate()
                var attachments = 0, executions = 0
                var callbacks: [@Sendable () async -> Void] = []
                original.executeCommand = { _ in
                    executions += 1; await gate.hold()
                    return .init(outputs: [.commands([media])], error: BridgeError.cloudPolicy(.staleCommand), deferred: false)
                }
                original.onQuiesce = { gate.release() }
                let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
                    callbacks.append(persisted)
                    attachments += 1; return attachments == 1 ? original : successor
                }, defaults: nil)
                await sync.preparePlatform(); await sync.completeOpenedHost(generation: 500)
                await sync.prepareConnectionAuthority()
                original.nextCommands = [media]; sync.receive(.refresh(.cloud))
                await gate.waitUntilHeld()
                if replacement {
                    sync.collectionReplaced(501); await sync.waitForReplacement()
                    #expect(!sync.operationFailed && sync.failureRevision == 0)
                } else {
                    #expect(await sync.cancelAndDrain())
                    #expect(sync.operationFailed && sync.failureRevision == 1)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 500)
                    #expect(sync.operationFailed && sync.failureRevision == 1)
                }
                await sync.prepareConnectionAuthority()
                #expect(attachments == 2 && executions == 1 && sync.setupState == .attached)
                let reads = successor.persistedReads
                await callbacks[0](); await sync.waitForInputs()
                #expect(original.persistedReads == 0 && successor.persistedReads == reads)
                #expect(await sync.cancelAndDrain())
            }
        }
    }
}

extension ReviewCollectionStartupTests {
    #if DEBUG
    @Test func keepLocalOnlyJoinsTheActualStatusResultAcknowledgementBeforeRotatingItsGuard() async throws {
        try await withShutdownHarness(mediaEnabled: false) { harness in
            try #require(await harness.sync.changeProvider(to: .localOnly))
            try await harness.owner.prepareCloudConnectionPreview()
            await harness.sync.waitForConnectionPreview()
            guard case .ready = harness.sync.preview else {
                Issue.record("Expected the real ready return preview")
                return
            }
            // Finish discovery's account proof and trailing refresh. Enabling
            // media changes status while preserving the preview's account authority.
            await harness.sync.drainOperations()
            try #require(!harness.sync.settings.enabled && !harness.sync.settings.paused
                && !harness.sync.settings.automatic && !harness.sync.settings.media)
            let status = try #require(harness.sync.status)
            try #require(status.media.noSpace != true && status.mediaPaused)
            let originalCoordinator = harness.sync
            let backend = try harness.backend
            let originalGuard = try await backend.workflow.currentGuard()
            try #require(!harness.sync.operationFailed && harness.sync.connectionFailure == nil)
            let resultGate = SyncOwnerGate()
            let held = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let waiting = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var heldCommand: CloudPolicyCommand?
            var heldResult: SyncCommandResult?
            var acknowledgementJoined = false, localOnlyReturned = false
            backend.afterExecution = { command, result in
                guard case .statusChanged(let status) = command.action, heldCommand == nil else { return }
                heldCommand = command; heldResult = result
                #expect(!status.mediaPaused && status.media.noSpace != true)
                #expect(!Task.isCancelled && result.error == nil && !result.deferred)
                held.continuation.yield(true)
                await resultGate.hold()
                #expect(!Task.isCancelled && !acknowledgementJoined)
                do {
                    #expect(try await backend.workflow.currentGuard() == originalGuard)
                } catch { Issue.record("Original local-only guard read failed") }
            }
            originalCoordinator.observeCausalBoundary = { boundary, command in
                switch boundary {
                case .localOnlyPolicyWaiting: waiting.continuation.yield(true)
                case .commandAcknowledgementJoined:
                    if let command, command.raw === heldCommand?.raw { acknowledgementJoined = true }
                default: break
                }
            }
            defer {
                backend.afterExecution = nil; originalCoordinator.observeCausalBoundary = nil
                resultGate.release(); held.continuation.finish(); waiting.continuation.finish()
            }
            var choosing: Task<Void, Never>?
            do {
                originalCoordinator.setDeviceSettings(paused: false, automatic: false, media: true)
                // The status Core phase is held by this result: an input
                // barrier here would wait for the same ACK we have not released.
                try #require(try await serviceAppSignal(held.stream, stage: "local-only actual status result held"))
                let result = try #require(heldResult)
                try #require(heldCommand != nil)
                #expect(result.error == nil && !result.deferred)
                #expect(!originalCoordinator.settings.enabled && !originalCoordinator.settings.paused
                    && !originalCoordinator.settings.automatic && originalCoordinator.settings.media
                    && !acknowledgementJoined)
                let action = Task {
                    await originalCoordinator.keepLocalOnly()
                    localOnlyReturned = true
                }
                choosing = action
                try #require(try await serviceAppSignal(waiting.stream, stage: "local-only ordered admission waiting"))
                #expect(!localOnlyReturned && !acknowledgementJoined && !backend.closed)
                #expect(try await backend.workflow.currentGuard() == originalGuard)
                resultGate.release()
                await action.value
            } catch {
                resultGate.release()
                await choosing?.value
                await originalCoordinator.drainOperations()
                throw error
            }
            #expect(localOnlyReturned && acknowledgementJoined)
            #expect(originalCoordinator.preview == nil && originalCoordinator.connectionFailure == nil)
            #expect(!originalCoordinator.operationFailed && originalCoordinator.failureRevision == 0)
            #expect(backend.closed && backend.closeReturned == 1)
            #expect(harness.sync !== originalCoordinator && harness.sync.providerState?.selection == .localOnly)
            #expect(!harness.sync.settings.enabled && !harness.sync.settings.paused
                && !harness.sync.settings.automatic && harness.sync.settings.media)
            #expect(harness.owner.state == .ready && harness.sync.setupState == .attached)
            #expect(!harness.sync.operationFailed && harness.sync.connectionFailure == nil)
            #expect(try await harness.backend.workflow.currentGuard() != originalGuard)
            originalCoordinator.observeCausalBoundary = nil
        }
    }
    #endif
    @Test func genuineShutdownResultFinalizesOnOriginalAuthorityBeforeNativeClose() async throws {
        try await withShutdownHarness { harness in
            let backend = try harness.backend
            let engine = try #require(await harness.factory.engines.last)
            let originalGuard = try await backend.workflow.currentGuard()
            let resultGate = SyncOwnerGate(), updateGate = SyncOwnerGate()
            let exchangeHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let updateHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var updateContext: SyncCommandTaskContext?
            var finalizedUpdates = 0, heldUpdateFinalized = false
            backend.afterExecution = { command, result in
                if case .runExchange = command.action {
                    #expect(!Task.isCancelled && result.error == nil && !result.deferred)
                    exchangeHeld.continuation.yield(true)
                    await resultGate.hold()
                    #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                }
                if case .updatePending = command.action, backend.quiesced {
                    finalizedUpdates += 1
                    #expect(result.error == nil && !result.deferred)
                    if let updateContext, SyncCommandTaskContext.current === updateContext {
                        heldUpdateFinalized = true
                        #expect(Task.isCancelled && updateContext.shutdownCancelled)
                        #expect(updateContext.generation == harness.owner.generation)
                    } else {
                        #expect(!Task.isCancelled)
                    }
                    #expect(!backend.closed && backend.closeEntered == 0)
                    do {
                        try await backend.workflow.validateSDK(engine.instanceID)
                        #expect(try await backend.workflow.currentGuard() == originalGuard)
                    } catch { Issue.record("Original SDK validation failed") }
                }
            }
            defer {
                resultGate.release(); updateGate.release()
                backend.beforeExecution = nil; backend.afterExecution = nil
                exchangeHeld.continuation.finish(); updateHeld.continuation.finish()
            }
            var close: Task<Bool, Never>?
            do {
                harness.sync.receive(.network(reachable: true))
                harness.sync.syncNow()
                try #require(try await serviceAppSignal(exchangeHeld.stream, stage: "shutdown exchange result held"))
                backend.beforeExecution = { command in
                    if case .updatePending = command.action, updateContext == nil {
                        let context = SyncCommandTaskContext.current
                        #expect(context != nil && context?.generation == harness.owner.generation)
                        #expect(!Task.isCancelled)
                        updateContext = context
                        updateHeld.continuation.yield(true)
                        await updateGate.hold()
                        #expect(Task.isCancelled && context?.shutdownCancelled == true)
                        #expect(backend.quiesced && !backend.closed && backend.closeEntered == 0)
                    }
                }
                let fixture = try InitialCloudFixture.load()
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let note = try await CoreNoteService().readNote(id: seed.id)
                var fields = note.fields
                fields[0] += " Synthetic shutdown edit"
                let mutation = try await CoreNoteService().updateNote(.init(id: note.id, fields: fields, tags: note.tags))
                try #require(mutation.written && mutation.id == note.id)
                try await CoreCollectionService().flushForBackground()
                try await backend.workflow.awaitTracking(generation: mutation.summary.generation, seq: mutation.summary.seq)
                await harness.sync.waitForInputs()
                harness.sync.receive(.refresh(.local))
                await harness.sync.waitForInputs()
                try #require(try await serviceAppSignal(updateHeld.stream, stage: "shutdown pending update held"))
                close = Task { await harness.owner.close() }
                try await backend.waitForQuiesce()
                #expect(!backend.closed && backend.closeEntered == 0)
                try await backend.workflow.validateSDK(engine.instanceID)
                #expect(try await backend.workflow.currentGuard() == originalGuard)
            } catch {
                backend.beforeExecution = nil; backend.afterExecution = nil
                resultGate.release(); updateGate.release()
                if let close { _ = await close.value }
                else { _ = await harness.owner.close() }
                throw error
            }
            resultGate.release(); updateGate.release()
            let closing = try #require(close)
            #expect(await closing.value)
            #expect(finalizedUpdates > 0)
            #expect(heldUpdateFinalized)
            #expect(!harness.sync.operationFailed && harness.sync.failureRevision == 0)
            #expect(backend.closed && backend.closeEntered == 1 && backend.closeReturned == 1)
        }
    }
}

extension ReviewCollectionStartupTests {
    #if DEBUG
    @Test func acceptedAccountOutputsAndFollowupsAreACKedBeforePreparationInput() async throws {
        try await withP4OrderingCommands { commands in
            let backend = SyncOwnerFake(), held = SyncOwnerGate(), followup = SyncOwnerGate()
            var order: [String] = []
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 704)
            await sync.prepareConnectionAuthority()
            sync.observeCausalBoundary = { boundary, command in
                guard case .commandAcknowledgementJoined = boundary, let command else { return }
                order.append(command.raw === commands.status.raw ? "account.ack" : "followup.ack")
            }
            backend.executeCommand = { command in
                if command.raw === commands.status.raw {
                    order.append("account.core"); await held.hold()
                    return .init(outputs: [.commands([commands.record])], error: nil, deferred: false)
                }
                #expect(command.raw === commands.record.raw)
                order.append("followup.core"); await followup.hold()
                return .init(outputs: [], error: nil, deferred: false)
            }
            backend.accountResult = .init(outputs: [.commands([commands.status])], error: nil, deferred: false,
                accountVerified: true, afterOutputs: { admitted in
                    #expect(admitted)
                    order.append("preparation.input")
                    return .init(outputs: [], error: nil, deferred: false, accountVerified: true)
                })
            await sync.accountChanged()
            await held.waitUntilHeld()
            #expect(order == ["account.core"])
            held.release(); await followup.waitUntilHeld()
            #expect(order == ["account.core", "account.ack", "followup.core"])
            followup.release()
            await sync.prepareConnectionAuthority(); await sync.drainOperations()
            #expect(order == ["account.core", "account.ack", "followup.core", "followup.ack", "preparation.input"])
            #expect(!sync.operationFailed && sync.failureRevision == 0)
            sync.observeCausalBoundary = nil
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test func corePhaseJoinsOriginalResultsAndFollowupsBeforeNextPolicyInput() async throws {
        try await withP4OrderingCommands { commands in
            let backend = SyncOwnerFake(), first = SyncOwnerGate(), followup = SyncOwnerGate()
            let originalError = NSError(domain: "synthetic-p4-core-result", code: 37)
            var order: [String] = [], returned: SyncCommandResult?
            backend.executeCommand = { command in
                if command.raw === commands.status.raw {
                    await first.hold()
                    let result = SyncCommandResult(outputs: [.commands([commands.record]), .syncNowUnavailable(.paused)],
                        error: originalError, deferred: true)
                    returned = result
                    return result
                }
                #expect(command.raw === commands.record.raw)
                order.append("followup.begin")
                await followup.hold()
                return .init(outputs: [], error: nil, deferred: false)
            }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 701)
            await sync.prepareConnectionAuthority()
            sync.observeCausalBoundary = { boundary, command in
                guard let command else { return }
                let label = command.raw === commands.status.raw ? "first" : "followup"
                switch boundary {
                case .commandResultEnqueue: order.append("\(label).enqueue")
                case .commandAcknowledgementJoined: order.append("\(label).ack")
                default: break
                }
            }
            backend.routeInput = { event in
                if case .network(false) = event { order.append("next.input") }
                if case .refresh = event { return .commands([commands.status]) }
                return .commands([])
            }
            sync.receive(.refresh(.cloud))
            await first.waitUntilHeld()
            sync.receive(.network(reachable: false))
            // The result must cross the real queued ACK path, including its
            // required core follow-up, before the already queued next input.
            first.release()
            await followup.waitUntilHeld()
            #expect(!order.contains("next.input"))
            followup.release()
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(order == ["first.enqueue", "first.ack", "followup.begin", "followup.enqueue", "followup.ack", "next.input"])
            #expect(sync.operationFailed && sync.failureRevision == 1 && sync.hasDeferredNativeWork)
            #expect(sync.syncNowUnavailable == .paused)
            let actual = try #require(returned)
            #expect((actual.error as? NSError) === originalError && actual.outputs.count == 2)
            if case .commands(let values) = actual.outputs[0] {
                #expect(values.count == 1 && values.first?.raw === commands.record.raw)
            } else { Issue.record("Original core follow-up lost") }
            sync.observeCausalBoundary = nil
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test func longFetchPreservesListOrderWhileLaterCoreAndImmediateStopProgress() async throws {
        try await withP4OrderingCommands { commands in
            let backend = SyncOwnerFake(), fetch = SyncOwnerGate(), terminal = SyncOwnerGate()
            var order: [String] = [], stopRequested = false
            let cancelled = SyncCommandResult(outputs: [.syncNowUnavailable(.paused)], error: CancellationError(), deferred: false)
            backend.executeCommand = { command in
                if command.raw === commands.fetch.raw {
                    order.append("fetch.begin")
                    await fetch.hold()
                    #expect(stopRequested)
                    order.append("fetch.cancelled")
                    // Keep the original cancelled callback outstanding after
                    // Stop has returned; no successor can overtake that return.
                    await terminal.hold()
                    order.append("fetch.return")
                    return cancelled
                } else if command.raw === commands.update.raw { order.append("list.successor") }
                else if command.raw === commands.status.raw { order.append("later.core") }
                else if command.raw === commands.stop.raw {
                    order.append("stop.begin"); stopRequested = true; fetch.release()
                }
                else { Issue.record("Unexpected command identity") }
                return .init(outputs: [], error: nil, deferred: false)
            }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 702)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [commands.fetch, commands.update]
            sync.receive(.refresh(.cloud))
            await fetch.waitUntilHeld()
            backend.nextCommands = [commands.status]
            sync.receive(.refresh(.cloud))
            await sync.waitForInputs()
            #expect(order == ["fetch.begin", "later.core"])
            let stopped = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command, command.raw === commands.stop.raw {
                    stopped.continuation.yield(true)
                }
            }
            backend.nextCommands = [commands.stop]
            sync.receive(.network(reachable: false))
            do {
                try #require(try await serviceAppSignal(stopped.stream, stage: "P4 immediate stop"))
            } catch {
                fetch.release(); terminal.release()
                _ = await sync.cancelAndDrain()
                stopped.continuation.finish(); sync.observeCausalBoundary = nil
                throw error
            }
            await terminal.waitUntilHeld()
            #expect(order.contains("stop.begin") && !order.contains("fetch.return") && !order.contains("list.successor"))
            terminal.release()
            await sync.drainOperations()
            let stoppedAt = try #require(order.firstIndex(of: "stop.begin"))
            let returnedAt = try #require(order.firstIndex(of: "fetch.return"))
            let successorAt = try #require(order.firstIndex(of: "list.successor"))
            #expect(stoppedAt < returnedAt && returnedAt < successorAt)
            stopped.continuation.finish(); sync.observeCausalBoundary = nil
            #expect(cancelled.error is CancellationError && cancelled.outputs.count == 1)
            // The scheduler preserves this unqualified original failure; it
            // does not infer a successful result from the Stop action.
            #expect(sync.operationFailed && sync.failureRevision == 1 && sync.syncNowUnavailable == .paused)
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test func cleanupCorePreparationReleasesInputsButRetainsOriginalInventoryResultAndDrain() async throws {
        try await withP4OrderingCommands { commands in
            let backend = SyncOwnerFake(), inventory = SyncOwnerGate()
            var order: [String] = []
            backend.executeCoreCommand = { command, corePrepared in
                #expect(command.raw === commands.cleanup.raw)
                order.append("cleanup.local.return")
                await corePrepared()
                await inventory.hold()
                order.append("inventory.return")
                return .init(outputs: [.syncNowUnavailable(.notConnected)], error: nil, deferred: false)
            }
            backend.routeInput = { event in
                if case .network(false) = event { order.append("later.input") }
                if case .refresh = event { return .commands([commands.cleanup]) }
                return .commands([])
            }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            // Install routing after startup, so only the controlled input issues work.
            let route = backend.routeInput; backend.routeInput = nil
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 703)
            await sync.prepareConnectionAuthority(); backend.routeInput = route
            sync.observeCausalBoundary = { boundary, command in
                if case .commandAcknowledgementJoined = boundary, let command, command.raw === commands.cleanup.raw {
                    order.append("cleanup.final.ack")
                }
            }
            sync.receive(.refresh(.cloud))
            await inventory.waitUntilHeld()
            sync.receive(.network(reachable: false)); await sync.waitForInputs()
            #expect(order == ["cleanup.local.return", "later.input"])
            backend.onClose = { order.append("close") }
            let close = Task { await sync.cancelAndDrain() }
            await backend.waitForQuiesce()
            #expect(!backend.closed && !order.contains("inventory.return"))
            inventory.release()
            #expect(await close.value)
            #expect(order == ["cleanup.local.return", "later.input", "inventory.return", "cleanup.final.ack", "close"])
            #expect(sync.syncNowUnavailable == .notConnected && !sync.operationFailed)
            sync.observeCausalBoundary = nil
        }
    }
    #endif

    @Test(arguments: [false, true])
    func nativeMediaCancellationQualifierMappingPreservesOriginalResult(qualified: Bool) throws {
        let original = NSError(domain: "synthetic-media-result", code: 20)
        let native = CloudPolicyNativeResult(outputs: [.syncNowUnavailable(.paused), .commands([])],
            error: original, deferred: true, mediaCancellation: qualified)
        let mapped = SyncCommandResult(native)
        #expect(mapped.mediaCancellation == qualified)
        #expect((mapped.error as? NSError) === original)
        #expect(mapped.deferred && !mapped.accountVerified)
        try #require(mapped.outputs.count == 2)
        if case .syncNowUnavailable(.paused) = mapped.outputs[0] {} else { Issue.record("Original unavailable output lost") }
        if case .commands(let commands) = mapped.outputs[1] { #expect(commands.isEmpty) }
        else { Issue.record("Original command output lost") }
        #expect(!CloudPolicyNativeResult(outputs: [], error: nil, deferred: false).mediaCancellation)
        #expect(!SyncCommandResult(outputs: [], error: nil, deferred: false).mediaCancellation)
    }

    #if DEBUG
    @Test(arguments: ["qualifiedCancellation", "unqualifiedCancellation", "genuineError",
                      "qualifiedGenuineError", "sdkWrapper20", "qualifiedSDKWrapper20"])
    func mediaCancellationPresentationPreservesOutputsAndOriginalAcknowledgements(tag: String) async throws {
        try await withShutdownCommands { media, update in
            let backend = SyncOwnerFake()
            let original = NSError(domain: "synthetic-media-presentation", code: 20)
            let qualified = tag == "qualifiedCancellation" || tag == "qualifiedGenuineError" || tag == "qualifiedSDKWrapper20"
            let cancellation = tag == "qualifiedCancellation" || tag == "unqualifiedCancellation"
            let wrapper = tag == "sdkWrapper20" || tag == "qualifiedSDKWrapper20"
            let error: any Error
            if cancellation {
                error = CancellationError()
            } else if wrapper {
                error = CloudKitSDKOperationFailure(underlying: original, code: 20, retryAfterMilliseconds: nil)
            } else {
                error = original
            }
            let expectsFailure = tag != "qualifiedCancellation"
            let acknowledgements = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var mediaExecutions = 0, updateExecutions = 0, mediaACKs = 0, updateACKs = 0
            var exactExecutions = true, exactACKs = true, originalResult: SyncCommandResult?
            backend.executeCommand = { command in
                #expect(!Task.isCancelled && !backend.closed)
                if command.raw === media.raw {
                    mediaExecutions += 1
                    let result = SyncCommandResult(outputs: [.syncNowUnavailable(.paused), .commands([update])],
                        error: error, deferred: true, mediaCancellation: qualified)
                    originalResult = result
                    return result
                }
                if command.raw === update.raw {
                    updateExecutions += 1
                    return .init(outputs: [.syncNowUnavailable(.notConnected)], error: nil, deferred: false)
                }
                exactExecutions = false
                return .init(outputs: [], error: nil, deferred: false)
            }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            sync.observeCausalBoundary = { boundary, command in
                guard case .commandAcknowledgementJoined = boundary else { return }
                if let command, command.raw === media.raw { mediaACKs += 1 }
                else if let command, command.raw === update.raw { updateACKs += 1 }
                else { exactACKs = false }
                if mediaACKs == 1 && updateACKs == 1 { acknowledgements.continuation.yield(true) }
            }
            defer { sync.observeCausalBoundary = nil; acknowledgements.continuation.finish() }
            do {
                await sync.preparePlatform(); await sync.completeOpenedHost(generation: 600)
                await sync.prepareConnectionAuthority()
                #expect(!sync.operationFailed && sync.failureRevision == 0)
                backend.nextCommands = [media]
                sync.receive(.refresh(.cloud))
                try #require(try await serviceAppSignal(acknowledgements.stream, stage: "media presentation original ACKs"))
                await sync.waitForInputs()
                let presented = (failed: sync.operationFailed, revision: sync.failureRevision,
                    unavailable: sync.syncNowUnavailable, deferred: sync.hasDeferredNativeWork)
                let drained = await sync.cancelAndDrain()
                #expect(presented.failed == expectsFailure)
                #expect(presented.revision == (expectsFailure ? 1 : 0))
                #expect(presented.unavailable == .notConnected && presented.deferred)
                #expect(exactExecutions && exactACKs)
                #expect(mediaExecutions == 1 && updateExecutions == 1 && mediaACKs == 1 && updateACKs == 1)
                #expect(drained && backend.quiesced && backend.closed && backend.closeCount == 1)
                let result = try #require(originalResult)
                #expect(result.mediaCancellation == qualified && result.deferred)
                if cancellation { #expect(result.error is CancellationError) }
                else if wrapper {
                    let preserved = try #require(result.error as? CloudKitSDKOperationFailure)
                    #expect(preserved.code == 20 && preserved.retryAfterMilliseconds == nil)
                    #expect((preserved.underlying as NSError) === original)
                } else { #expect((result.error as? NSError) === original) }
                try #require(result.outputs.count == 2)
                if case .syncNowUnavailable(.paused) = result.outputs[0] {} else { Issue.record("Media unavailable output lost") }
                if case .commands(let commands) = result.outputs[1] {
                    let exactFollowup = commands.count == 1 && commands.first?.raw === update.raw
                    #expect(exactFollowup)
                } else { Issue.record("Required media follow-up lost") }
            } catch {
                _ = await sync.cancelAndDrain()
                throw error
            }
        }
    }
    #endif
}

private enum ShutdownReturnPreviewTest: String { case providerHandoff, collectionReplacement }
private enum ShutdownReturnPreviewStage: String { case beforePrepare, afterPrepare, afterWait }

@MainActor private func observeShutdownReturnPreview(
    _ harness: ServiceAppHarness, test: ShutdownReturnPreviewTest, stage: ShutdownReturnPreviewStage
) {
    let sync = harness.sync
    let backend = harness.backends.last
    let accountReads = backend.map { String($0.accountReadCount) } ?? "absent"
    let label = "shutdownReturnPreview test=\(test.rawValue) stage=\(stage.rawValue)"
    print("\(label) attached=\(sync.setupState == .attached) paused=\(sync.settings.paused)"
        + " providerlocalOnly=\(sync.providerState?.selection == .localOnly) providerblocked=\(sync.providerState?.phase == .blocked)"
        + " previewPresent=\(sync.preview != nil) previewActive=\(sync.connectionPreviewActive)"
        + " operationActive=\(sync.connectionOperationActive) initialAdoptionActive=\(sync.initialAdoptionActive)"
        + " hasConnectionService=\(sync.connectionService != nil) hasOwn=\(backend?.own != nil)"
        + " hasFailure=\(sync.connectionFailure != nil) operationFailed=\(sync.operationFailed) accountReadCount=\(accountReads)")
    #if DEBUG
    // Process-global cross-check only; this does not attribute the latest failure to this attempt.
    print("\(label) globalLatestCrossCheck=\(DebugCloudDiagnostics.latestValue ?? "absent")")
    #endif
}

// The real harness retains its host; only copied, observed opaque handles reach
// SyncOwnerFake. Its scheduler assertions do not prove native SDK acceptance.
@MainActor private func withShutdownCommands(
    _ body: (CloudPolicyCommand, CloudPolicyCommand) async throws -> Void
) async throws {
    try await withShutdownHarness { harness in
        let backend = try harness.backend
        let observed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { backend.onCommands = nil; observed.continuation.finish() }
        var media: CloudPolicyCommand?, update: CloudPolicyCommand?
        backend.onCommands = { commands in
            for command in commands {
                switch command.action {
                case .resumeMedia where media == nil: media = command
                case .updatePending where update == nil: update = command
                default: break
                }
            }
            if media != nil && update != nil {
                observed.continuation.yield(true)
                observed.continuation.finish()
            }
        }
        try await harness.exchange(phase: .aBaseline)
        // RecordExchange can issue ResumeMedia after exchange() returns.
        try #require(try await serviceAppSignal(observed.stream, stage: "shutdown fixture issued commands"))
        backend.onCommands = nil
        try await body(try #require(media), try #require(update))
    }
}

@MainActor private func withShutdownHarness(
    mediaEnabled: Bool = true,
    beforeAdopt: (@MainActor (ServiceAppHarness) async throws -> Void)? = nil,
    _ body: (ServiceAppHarness) async throws -> Void
) async throws {
    let fixture = try InitialCloudFixture.load()
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-shutdown-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? FileManager.default.removeItem(at: root) }
    let serviceDirectory = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let harness = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
        active = harness
        if !mediaEnabled {
            harness.defaults.set(["enabled": false, "paused": false, "automatic": false, "media": false],
                forKey: "meguri.cloud.deviceSettings")
        }
        defer { harness.defaults.removePersistentDomain(forName: harness.suite) }
        try await wire.seed(fixture)
        try await beforeAdopt?(harness)
        try await harness.adopt()
        try await body(harness)
        try #require(await harness.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else {
            let exit = await process.cancel()
            #expect(exit.joined, "Shutdown fixture must join its owned Service process")
        }
        throw error
    }
}

#if DEBUG
// Every row reuses the real media executor's sealed result and original intent
// channel above, then independently checks frozen queue/active-command guards.
@MainActor private func checkFrozenQueueOwnership(
    _ result: SyncCommandResult, command: CloudPolicyCommand, other: CloudPolicyCommand,
    unissued: CloudPolicyCommand, backend: any SyncCoordinatorBackend
) {
    let originalIntent = SyncCommandTaskContext.current?.cancellationIntent ?? CloudPolicyCancellationContext()
    func make(_ commands: [CloudPolicyCommand]) -> SyncCommandTaskContext {
        SyncCommandTaskContext(generation: 179, revision: 3, backend: backend, originalCommands: commands,
                               cancellationIntent: originalIntent)
    }
    func owns(_ context: SyncCommandTaskContext, _ actual: CloudPolicyCommand) -> Bool {
        context.ownsNativeCancellation(result, command: actual, backend: backend, generation: 179, revision: 3)
    }
    for intent in [SyncCommandTaskContext.Cancellation.expiry, .replacement] {
        let queued = make([other, command])
        queued.activeCommandIndex = 0
        queued.recordCancellation(intent, alreadyCancelled: false)
        #expect(!owns(queued, command)) // Still executing the earlier original.
        queued.activeCommandIndex = 1
        #expect(owns(queued, command))
        #expect(!owns(queued, other) && !owns(queued, unissued))
        queued.recordCancellation(.shutdown, alreadyCancelled: true)
        #expect(owns(queued, command) && !queued.shutdownCancellationRequested)

        let notStarted = make([other, command])
        notStarted.recordCancellation(intent, alreadyCancelled: false)
        notStarted.activeCommandIndex = 1
        #expect(owns(notStarted, command))

        let completed = make([command, other])
        completed.activeCommandIndex = 1
        completed.recordCancellation(intent, alreadyCancelled: false)
        completed.activeCommandIndex = 0
        completed.recordCancellation(.shutdown, alreadyCancelled: true)
        completed.recordCancellation(intent, alreadyCancelled: false)
        #expect(!owns(completed, command) && !completed.shutdownCancellationRequested)

        for index in [-1, 2] {
            let inconsistent = make([other, command])
            inconsistent.activeCommandIndex = index
            inconsistent.recordCancellation(intent, alreadyCancelled: false)
            inconsistent.activeCommandIndex = 1
            inconsistent.recordCancellation(intent, alreadyCancelled: false)
            #expect(!owns(inconsistent, command))
        }
        var originalList = [other]
        let unknown = make(originalList)
        unknown.recordCancellation(intent, alreadyCancelled: false)
        originalList.append(command)
        unknown.activeCommandIndex = 1
        unknown.recordCancellation(.shutdown, alreadyCancelled: true)
        #expect(!owns(unknown, command))
        let laterList = make(originalList)
        laterList.activeCommandIndex = 1
        #expect(!owns(laterList, command))
        let different = make([unissued])
        different.activeCommandIndex = 0
        different.recordCancellation(intent, alreadyCancelled: false)
        #expect(!owns(different, command) && !owns(different, unissued))
    }
    for origin in [SyncCommandTaskContext.Cancellation.other, .shutdown, .expiry] {
        let unknownOrigin = make([command])
        unknownOrigin.activeCommandIndex = 0
        unknownOrigin.recordCancellation(origin, alreadyCancelled: origin == .expiry)
        unknownOrigin.recordCancellation(.replacement, alreadyCancelled: false)
        unknownOrigin.recordCancellation(.expiry, alreadyCancelled: false)
        #expect(!owns(unknownOrigin, command))
    }
}

private struct P4OrderingCommands {
    let status: CloudPolicyCommand, record: CloudPolicyCommand, cleanup: CloudPolicyCommand
    let fetch: CloudPolicyCommand, update: CloudPolicyCommand, stop: CloudPolicyCommand
}

// Actual issued handles, retained by the live harness. The fake replaces only
// execution to control causal order; these tests make no native-admission claim.
@MainActor private func withP4OrderingCommands(_ body: (P4OrderingCommands) async throws -> Void) async throws {
    try await withShutdownHarness(mediaEnabled: false) { harness in
        let backend = try harness.backend
        var observed: [CloudPolicyCommand] = []
        backend.onCommands = { observed.append(contentsOf: $0) }
        defer { backend.onCommands = nil }
        try await harness.exchange(phase: .aBaseline)
        await harness.sync.drainOperations()
        try #require(!harness.sync.operationFailed && harness.sync.failureRevision == 0)
        // Core issues the remaining real handles. Execution is replaced only in
        // the fake bodies below; do not run ConfigureAutomatic's replacement
        // SDK or media actions merely to obtain scheduler test capabilities.
        let policy = try await backend.workflow.policy()
        for output in [
            try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))),
            try await policy.input(.settings(.init(enabled: true, paused: true, automatic: true, media: false)))
        ] {
            guard case .commands(let issued) = output else {
                throw BridgeError.internal("P4 command acquisition failed")
            }
            observed.append(contentsOf: issued)
        }
        backend.onCommands = nil
        let commands = P4OrderingCommands(
            status: try #require(observed.first { if case .statusChanged = $0.action { true } else { false } }),
            record: try #require(observed.first { if case .recordExchange = $0.action { true } else { false } }),
            cleanup: try #require(observed.first { if case .runCleanup = $0.action { true } else { false } }),
            fetch: try #require(observed.first { if case .requestFetch = $0.action { true } else { false } }),
            update: try #require(observed.first { if case .updatePending = $0.action { true } else { false } }),
            stop: try #require(observed.first { if case .stopEngine = $0.action { true } else { false } }))
        try await body(commands)
    }
}
#endif
#endif
