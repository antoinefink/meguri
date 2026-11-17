import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

@MainActor extension ReviewCollectionStartupTests {
    @Test(arguments: ["absent", "invalidLegacy", "unknownVersion", "partialIntent"])
    func providerMetadataCannotDisableLocalOpenStudyOrDurableAnswers(input: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-provider-\(UUID().uuidString)")
        let directory = root.appending(path: "collection", directoryHint: .isDirectory)
        let installation = root.appending(path: "installation", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: installation, withIntermediateDirectories: true)
        let suite = "synthetic-provider-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite); try? FileManager.default.removeItem(at: root) }
        let schema = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: schema, to: directory.appending(path: "collection.anki2"))
        switch input {
        case "invalidLegacy": defaults.set(["enabled": true, "paused": false, "automatic": "invalid", "media": true], forKey: "meguri.cloud.deviceSettings")
        case "unknownVersion": try Data("{\"version\":999}".utf8).write(to: installation.appending(path: "provider.json"))
        case "partialIntent": try Data("{\"version\":1,\"handoff\":{\"old\":\"ICloud\"}}".utf8).write(to: installation.appending(path: "provider.json"))
        default: break
        }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        func makeOwner() -> ReviewCollection {
            let sync = SyncCoordinator(directory: directory, configuration: nil, defaults: defaults)
            let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync, providerDirectory: installation)
            owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
            return owner
        }
        var active = makeOwner()
        do {
            await active.start()
            try #require(active.state == .ready)
            let provider = try await CoreProviderService.attached()
            let state = try await provider.state()
            #expect(state.phase == (input == "absent" ? .localOnly : .blocked))
            let deck = try await CoreDeckService().addDeck(name: "Synthetic Provider Study")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic Provider QA", cloze: false,
                fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
            _ = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Synthetic question", "Synthetic answer"],
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            try await CoreCollectionService().flushForBackground()
            try #require(await active.close())
            active = makeOwner()
            await active.start()
            if active.policy != .ok { await active.enableFsrs() }
            try #require(active.policy == .ok)
            await active.startStudy(deckID: deck.id)
            let review = try #require(active.review)
            try #require(review.phase == .question)
            review.reveal(); await review.settled()
            review.rate(.easy); await review.settled()
            let answered = try #require(review.record.answers.first)
            #expect(review.record.answers.count == 1)
            try await CoreCollectionService().flushForBackground()
            await active.finishReview()
            try #require(await active.close())
            active = makeOwner()
            await active.start()
            try #require(active.state == .ready)
            #expect(active.previousRecord?.answers.contains(where: { $0.cardID == answered.cardID && $0.rating == answered.rating }) == true)
            await active.startStudy(deckID: deck.id)
            if let session = active.review?.sessionID {
                let head = try await Core.sessionHead(sessionID: session, count: 3)
                #expect(!head.items.contains(where: { $0.cardId == answered.cardID }))
            }
            await active.finishReview()
            try #require(await active.close())
        } catch { _ = await active.close(); throw error }
    }
}

#if os(macOS)
@MainActor extension ReviewCollectionStartupTests {
    @Test func localOnlyStartupAndStudyDoNotReadAccountButExplicitConnectJoinsFreshProof() async throws {
        try await withFreshProviderSetup { app in
            await app.owner.start()
            try #require(app.owner.state == .ready)
            await app.sync.prepareConnectionAuthority()
            let backend = try app.backend
            #expect(backend.accountReadCount == 0)
            var admittedReads: [UInt64] = []
            backend.beforeAdmittedAccountCompletion = { admittedReads.append($0) }
            defer { backend.beforeAdmittedAccountCompletion = nil }
            #expect(await app.factory.engines.isEmpty)
            #expect(app.sync.providerState?.selection == .localOnly)
            let gate = SyncOwnerGate()
            let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            await app.factory.holdNextAccountRead { entered.continuation.yield(true); await gate.hold() }
            defer { gate.release(); entered.continuation.finish() }
            try await app.owner.prepareCloudConnectionPreview()
            try #require(try await serviceAppSignal(entered.stream, stage: "explicit setup account read"))
            #expect(app.sync.preview == nil && app.sync.connectionFailure == nil)
            #expect(backend.accountReadCount == 1)
            #expect(admittedReads.isEmpty, "The held actual account read has not reached ordered admission")
            #expect(await app.factory.engines.isEmpty)
            // The real review consumer remains usable while account IO is held.
            if app.owner.policy != .ok { await app.owner.enableFsrs() }
            let deck = try await CoreDeckService().addDeck(name: "Synthetic setup study")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic setup QA", cloze: false,
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
            gate.release()
            await app.sync.waitForConnectionPreview()
            try #require(app.sync.preview != nil && app.sync.connectionFailure == nil)
            #expect(admittedReads.count == 1 && backend.accountReadCount == 1,
                "First Connect joins its actual ordered account read before preview")
            #expect(app.sync.providerState?.selection == .localOnly)
            #expect(await app.factory.engines.isEmpty)
            await app.sync.keepLocalOnly()
            #expect(app.owner.state == .ready && app.sync.providerState?.selection == .localOnly)
        }
    }

    @Test func firstPublicationQualifiesItsActualLocalBindingBeforeSDKStartup() async throws {
        try await withFreshProviderSetup { app in
            await app.owner.start()
            try #require(app.owner.state == .ready)
            try await app.owner.prepareCloudConnectionPreview()
            await app.sync.waitForConnectionPreview()
            guard case .ready(_, let summary) = app.sync.preview else {
                Issue.record("Expected first publication choice"); return
            }
            try #require(SyncStatusPresentation.permits(.publication, decision: summary.decision))
            let backend = try app.backend
            let before = try await backend.workflow.durableSnapshot()
            let gate = SyncOwnerGate()
            let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var startupObserved = false
            backend.beforeExecution = { command in
                guard case .startEngine = command.action else { return }
                startupObserved = true
                // This is the production runtime's post-Bind/WriteDecision
                // evidence, observed before the required SDK command executes.
                let after = try? await backend.workflow.durableSnapshot()
                #expect(after?.binding != nil && after?.binding != before.binding)
                #expect(app.sync.providerState?.selection == .iCloud)
                let operation = try? await backend.connection?.recoveryStatus()?.operation
                #expect(operation != nil)
                entered.continuation.yield(true)
                await gate.hold()
            }
            defer { backend.beforeExecution = nil; gate.release(); entered.continuation.finish() }
            await app.sync.confirmPublication()
            try #require(try await serviceAppSignal(entered.stream, stage: "first publication SDK command"))
            #expect(startupObserved && !app.sync.operationFailed)
            #expect(await app.factory.engines.isEmpty)
            gate.release()
            try await backend.waitForStart()
            await app.sync.drainOperations()
            #expect(!app.sync.operationFailed && app.sync.connectionFailure == nil)
        }
    }

    @Test func actualAdoptionReplacementRejectsOriginalProviderRecoveryAndPublicationRetry() async throws {
        try await withFreshProviderSetup { app in
            try await app.wire.seed(try InitialCloudFixture.load())
            await app.owner.start()
            try #require(app.owner.state == .ready)
            let original = try await CoreProviderService.attached()
            try await app.owner.prepareCloudConnectionPreview()
            await app.sync.waitForConnectionPreview()
            guard case .ready(_, let summary) = app.sync.preview else {
                Issue.record("Expected a real adoption choice"); return
            }
            try #require(SyncStatusPresentation.permits(.adoption, decision: summary.decision))
            await app.sync.confirmAdoption()
            await app.owner.waitForCloudActivation()
            await app.sync.waitForReplacement()
            await app.sync.prepareConnectionAuthority()
            let current = try await CoreProviderService.attached()
            let file = app.root.appending(path: "synthetic-installation/provider.json")
            let committed = try Data(contentsOf: file)
            await #expect(throws: (any Error).self) { _ = try await original.recoverHandoffToLocalOnly() }
            await #expect(throws: (any Error).self) { _ = try await original.retryFailedPublication() }
            #expect(try Data(contentsOf: file) == committed)
            #expect(try await current.state().selection == .iCloud)
            #expect(!(await app.sync.recoverProviderHandoffToLocalOnly()))
            #expect(app.owner.state == .ready && app.sync.setupState == .attached)
        }
    }

    @Test func relaunchedHandoffRecoversOnlyAfterExplicitCurrentOwnerJoin() async throws {
        try await withFreshProviderSetup { app in
            await app.owner.start()
            try #require(app.owner.state == .ready)
            let originalCoordinator = app.sync
            let originalProvider = try await CoreProviderService.attached()
            _ = try await originalProvider.beginHandoff(to: .meguri)
            let file = app.root.appending(path: "synthetic-installation/provider.json")
            let intent = try Data(contentsOf: file)
            try #require(await app.owner.close())
            let reopened = try ServiceAppHarness(root: app.root, wire: app.wire, defaults: app.defaults, cachedIdentityAvailable: false)
            do {
                await reopened.owner.start()
                try #require(reopened.owner.state == .ready)
                let current = reopened.sync
                let backend = try reopened.backend
                try #require(current.providerState?.phase == .handoffRecovery)
                await current.retryProviderAuthority()
                #expect(current.providerState?.phase == .handoffRecovery)
                #expect(try Data(contentsOf: file) == intent)
                #expect(await reopened.factory.engines.isEmpty)
                #expect(!(await originalCoordinator.recoverProviderHandoffToLocalOnly()))
                #expect(!(await originalCoordinator.retryFailedProviderPublication()))
                await originalCoordinator.retryProviderAuthority()
                await #expect(throws: (any Error).self) { _ = try await originalProvider.recoverHandoffToLocalOnly() }

                // Review remains local and usable. The concrete recovery action
                // honors the real collection owner's active-review guard.
                if reopened.owner.policy != .ok { await reopened.owner.enableFsrs() }
                let deck = try await CoreDeckService().addDeck(name: "Synthetic handoff recovery study")
                let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic recovery QA", cloze: false,
                    fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
                _ = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Question", "Answer"],
                    tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
                await reopened.owner.startStudy(deckID: deck.id)
                let review = try #require(reopened.owner.review)
                try #require(review.phase == .question)
                #expect(!(await current.recoverProviderHandoffToLocalOnly()))
                #expect(!backend.closed && current.providerState?.phase == .handoffRecovery)
                review.reveal(); await review.settled()
                review.rate(.easy); await review.settled()
                #expect(review.record.answers.count == 1)
                await reopened.owner.finishReview()

                let gate = SyncOwnerGate()
                let held = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
                backend.beforeQuiesce = { held.continuation.yield(true); await gate.hold() }
                defer { backend.beforeQuiesce = nil; gate.release(); held.continuation.finish() }
                let recovering = Task { await current.recoverProviderHandoffToLocalOnly() }
                do {
                    try #require(try await serviceAppSignal(held.stream, stage: "current provider recovery shutdown"))
                    #expect(!backend.closed && reopened.sync === current)
                    #expect(try Data(contentsOf: file) == intent)
                    #expect(!(await current.recoverProviderHandoffToLocalOnly()))
                    #expect(!(await current.retryFailedProviderPublication()))
                    #expect(!(await current.changeProvider(to: .meguri)))
                    await current.retryProviderAuthority()
                    #expect(try Data(contentsOf: file) == intent)
                    gate.release()
                    try #require(await recovering.value)
                    #expect(backend.closed && backend.closeReturned == 1)
                    #expect(reopened.sync !== current && reopened.sync.setupState == .attached)
                    #expect(reopened.sync.providerState?.selection == .localOnly)
                    #expect(reopened.sync.providerState?.phase == .localOnly)
                    #expect(await reopened.factory.engines.isEmpty)
                    let committed = try Data(contentsOf: file)
                    #expect(!(await current.recoverProviderHandoffToLocalOnly()))
                    #expect(!(await current.retryFailedProviderPublication()))
                    await current.retryProviderAuthority()
                    #expect(try Data(contentsOf: file) == committed)
                    weak let successorLifetime = reopened.sync
                    try #require(await reopened.sync.changeProvider(to: .meguri))
                    #expect(successorLifetime == nil, "Repeated recovery/handoff must release drained predecessors")
                    try #require(await reopened.sync.changeProvider(to: .localOnly))
                    #expect(reopened.sync.setupState == .attached && reopened.owner.state == .ready)
                } catch { gate.release(); _ = await recovering.value; throw error }
                try #require(await reopened.owner.close())
            } catch { _ = await reopened.owner.close(); throw error }
        }
    }

    @Test func relaunchedCredentialRemovalRecoveryStaysStoppedAndRetainsIntent() async throws {
        try await withFreshProviderSetup { app in
            await app.owner.start()
            try #require(app.owner.state == .ready)
            let original = try await CoreProviderService.attached()
            _ = try await original.beginHandoff(to: .localOnly, removeCredentials: true)
            let file = app.root.appending(path: "synthetic-installation/provider.json")
            let intent = try Data(contentsOf: file)
            try #require(await app.owner.close())
            let reopened = try ServiceAppHarness(root: app.root, wire: app.wire, defaults: app.defaults, cachedIdentityAvailable: false)
            do {
                await reopened.owner.start()
                try #require(reopened.owner.state == .ready)
                let stopped = reopened.sync
                let backend = try reopened.backend
                #expect(stopped.providerState?.removalPending == true)
                #expect(!(await stopped.recoverProviderHandoffToLocalOnly()))
                #expect(backend.closed && backend.closeReturned == 1 && reopened.sync === stopped)
                #expect(stopped.operationFailed && stopped.providerState?.removalPending == true)
                await stopped.retryProviderAuthority()
                #expect(!(await stopped.recoverProviderHandoffToLocalOnly()))
                #expect(backend.closeReturned == 1 && reopened.sync === stopped)
                #expect(try Data(contentsOf: file) == intent)
                #expect(await reopened.factory.engines.isEmpty)
                try #require(await reopened.owner.close())
            } catch { _ = await reopened.owner.close(); throw error }
        }
    }
}

// Uses the ordinary app composition with an unknown cached identity, a real
// local collection and an empty protocol service. No binding is fixture-issued.
@MainActor private func withFreshProviderSetup(_ body: (ServiceAppHarness) async throws -> Void) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-provider-setup-\(UUID().uuidString)")
    let directory = root.appending(path: "collection", directoryHint: .isDirectory)
    let serviceDirectory = root.appending(path: "service", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let schema = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
    try FileManager.default.copyItem(at: schema, to: directory.appending(path: "collection.anki2"))
    let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let app = try ServiceAppHarness(root: directory, wire: wire, cachedIdentityAvailable: false)
        active = app
        defer { app.defaults.removePersistentDomain(forName: app.suite) }
        try await body(app)
        try #require(await app.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty)
    } catch {
        _ = await active?.owner.close()
        _ = await process.cancel()
        throw error
    }
}
#endif
