@testable import CoreBridge
import Instrumentation
import Synchronization
#if os(macOS)
import Darwin
#endif
import Foundation
import Testing
@testable import AppModel

// Owns the process's real collection; run this suite with a focused selector.
@MainActor
@Suite(.serialized) struct ReviewCollectionStartupTests {
    @Test func providerHandoffRefusesAnActiveRealReviewWithoutWaitingForNetwork() async throws {
        try await withStartupActivityOwner { fixture, _ in
            let suffix = UUID().uuidString
            let deck = try await CoreDeckService().addDeck(name: "Synthetic handoff review \(suffix)")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic handoff QA \(suffix)", cloze: false,
                fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
            let note = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Question", "Answer"],
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            try #require(note.cards.count == 1)
            let cardID = try #require(note.cards.first)
            fixture.owner.reviewSessionClient = CoreReviewSessionClient()
            await fixture.owner.startStudy(deckID: deck.id)
            let review = try #require(fixture.owner.review)
            try #require(review.phase == .question)
            try #require(review.item?.cardId == cardID)
            let session = try #require(review.sessionID)
            #expect(!(await fixture.owner.sync.changeProvider(to: .meguri)))
            #expect(review.sessionID == session && fixture.owner.reviewIsActive)
            #expect(try await CoreCollectionService().resume().activeSession == session)
            review.reveal(); await review.settled()
            #expect(review.phase == .answer)
            await fixture.owner.finishReview()
            #expect(try await CoreCollectionService().resume().activeSession == nil)
        }
    }
    @Test func reviewStartupActivityCoalescedRecoveryEnablesReveal() async throws {
        try await withStartupActivityOwner { fixture, deckID in
            let study = Task { await fixture.owner.startStudy(deckID: deckID) }
            await fixture.gate.requested()
            let model = try #require(fixture.owner.review)
            #expect(model.sessionID == nil)
            // No suspension between these setters: both queued tasks see active.
            fixture.owner.setSceneActive(false)
            fixture.owner.setSceneActive(true)
            await fixture.owner.waitForLifecycle()
            #expect(model.sessionID == nil && model.paused)
            #expect(fixture.surface.reveals == 0)
            fixture.gate.release()
            await study.value
            #expect(fixture.owner.review === model)
            #expect(fixture.owner.reviewIsActive)
            #expect(model.phase == .question)
            #expect(!model.paused && model.canReveal)
            model.reveal(); await model.settled()
            #expect(fixture.surface.reveals == 1)
            #expect(model.phase == .answer)
        }
    }

    @Test func reviewStartupActivityInactiveCompletionWaitsForLaterActivation() async throws {
        try await withStartupActivityOwner { fixture, deckID in
            let study = Task { await fixture.owner.startStudy(deckID: deckID) }
            await fixture.gate.requested()
            let model = try #require(fixture.owner.review)
            fixture.owner.setSceneActive(false)
            // Unlike recovery, this lifecycle task may wait for startup in pause.
            fixture.gate.release()
            await study.value
            await fixture.owner.waitForLifecycle()
            #expect(!fixture.owner.reviewIsActive)
            #expect(model.phase == .loading && model.paused && !model.canReveal)
            model.reveal(); await model.settled()
            #expect(fixture.surface.reveals == 0)
            fixture.owner.setSceneActive(true)
            await fixture.owner.waitForLifecycle()
            #expect(fixture.owner.reviewIsActive && !model.paused && model.canReveal)
            model.reveal(); await model.settled()
            #expect(fixture.surface.reveals == 1)
            #expect(model.phase == .answer)
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["scene", "foreground"])
    func pauseBeforeCoreAdmissionRetainsOriginalStart(activity: String) async throws {
        try await withStartupActivityOwner { fixture, deckID in
            let client = HeldCoreStart(gate: fixture.gate)
            fixture.owner.reviewSessionClient = client
            let study = Task { await fixture.owner.startStudy(deckID: deckID) }
            do {
                await fixture.gate.requested() // No Core start has been issued yet.
                let model = try #require(fixture.owner.review)
                #expect(model.sessionID == nil && client.starts == 0)
                if activity == "foreground" {
                    fixture.owner.setForeground(false)
                    fixture.owner.setForeground(true)
                } else {
                    fixture.owner.setSceneActive(false)
                    fixture.owner.setSceneActive(true)
                    await fixture.owner.waitForLifecycle()
                }
                #expect(model.paused && model.sessionID == nil)
                fixture.gate.release()
                await study.value
                await fixture.owner.waitForLifecycle()
                let id = try #require(model.sessionID)
                #expect(fixture.owner.review === model && !model.paused)
                #expect(client.starts == 1 && !client.startWasCancelled)
                // The ID and retained session are Core's actual terminal result.
                _ = try await Core.sessionHead(sessionID: id, count: 3)
                #expect(try await CoreCollectionService().resume().activeSession == id)
                await fixture.owner.finishReview()
                #expect(client.ends == 1 && model.sessionID == nil)
                #expect(try await CoreCollectionService().resume().activeSession == nil)
            } catch {
                fixture.gate.release()
                await study.value
                throw error
            }
        }
    }

    private final class HeldCoreStart: ReviewSessionClient {
        let gate: StartupResultGate
        let holdBeforeAdmission: Bool
        let core = CoreReviewSessionClient()
        var starts = 0, ends = 0
        var startWasCancelled = false
        init(gate: StartupResultGate, holdBeforeAdmission: Bool = true) {
            self.gate = gate; self.holdBeforeAdmission = holdBeforeAdmission
        }
        func start(deckID: Int64) async throws -> ReviewStartResult {
            if holdBeforeAdmission { await gate.hold() }
            startWasCancelled = Task.isCancelled
            try Task.checkCancellation()
            starts += 1
            return try await core.start(deckID: deckID)
        }
        func head(sessionID: UInt64) async throws -> ReviewHead { try await core.head(sessionID: sessionID) }
        func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
            try await core.answer(sessionID: sessionID, item: item, rating: rating, at: at, elapsed: elapsed)
        }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus { try await core.undoStatus(sessionID: sessionID, redo: redo) }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead { try await core.restore(sessionID: sessionID, redo: redo) }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState { try await core.studyState(deckID: deckID) }
        func end(sessionID: UInt64) async throws { try await core.end(sessionID: sessionID); ends += 1 }
    }

    @Test(.timeLimit(.minutes(1))) func cancelledCallerBeforeStartupExecutionStillJoinsCoreStart() async throws {
        try await withStartupActivityOwner { fixture, deckID in
            let client = HeldCoreStart(gate: fixture.gate, holdBeforeAdmission: false)
            let model = ReviewCoordinator(deckID: deckID, surface: fixture.surface,
                store: ReviewRecordStore(file: fixture.directory.appending(path: "cancelled-start.json")), client: client)
            let starting = Task {
                withUnsafeCurrentTask { $0?.cancel() }
                await model.start() // Cancellation handler runs before startOwned can execute.
            }
            await starting.value
            do {
                let id = try #require(model.sessionID)
                #expect(client.starts == 1 && !client.startWasCancelled)
                #expect(model.phase == .error && fixture.surface.presented.isEmpty)
                _ = try await Core.sessionHead(sessionID: id, count: 3)
                #expect(await model.end())
                #expect(client.ends == 1)
                #expect(try await CoreCollectionService().resume().activeSession == nil)
            } catch {
                _ = await model.end()
                throw error
            }
        }
    }

    @Test func reviewStartupActivityReplacedOwnerDoesNotResumeOldModel() async throws {
        try await withStartupActivityOwner { fixture, deckID in
            let study = Task { await fixture.owner.startStudy(deckID: deckID) }
            await fixture.gate.requested()
            let old = try #require(fixture.owner.review)
            fixture.owner.setSceneActive(false)
            fixture.owner.setSceneActive(true)
            await fixture.owner.waitForLifecycle()
            let replacement = ReviewCoordinatorTests.Fixture()
            await replacement.model.start()
            fixture.owner.presentReview(replacement.model)
            fixture.gate.release()
            await study.value
            #expect(fixture.owner.review === replacement.model)
            #expect(old.paused && !old.canReveal)
            #expect(!fixture.surface.calls.contains("resume"))
            #expect(replacement.model.canReveal)
            old.reveal(); await old.settled()
            #expect(fixture.surface.reveals == 0)
            #expect(await old.end())
            await fixture.owner.finishReview()
        }
    }

    private func withStartupActivityOwner(
        _ body: (StartupActivityFixture, Int64) async throws -> Void
    ) async throws {
        let fixture = try StartupActivityFixture()
        defer { try? FileManager.default.removeItem(at: fixture.directory) }
        do {
            await fixture.owner.start()
            try #require(fixture.owner.state == .ready)
            if fixture.owner.policy != .ok { await fixture.owner.enableFsrs() }
            try #require(fixture.owner.policy == .ok)
            let deck = try #require(fixture.owner.decks.first)
            try await body(fixture, deck.id)
            #expect(await fixture.owner.close())
        } catch {
            fixture.gate.release()
            _ = await fixture.owner.close()
            throw error
        }
    }

    private final class StartupActivityFixture {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let surface = ReviewCoordinatorTests.Surface()
        let client = ReviewCoordinatorTests.Client()
        let gate = StartupResultGate()
        let owner: ReviewCollection

        init() throws {
            let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
            configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
            configuration.timeZone = TimeZone(secondsFromGMT: 0)
            let sync = SyncCoordinator(directory: directory, configuration: nil, defaults: nil)
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
            owner.makeReviewSurface = { [surface] _ in surface }
            owner.reviewSessionClient = client
            client.onStartResult = { [gate] in await gate.hold() }
        }
    }

    private final class StartupResultGate {
        private var held: CheckedContinuation<Void, Never>?
        private var admission: CheckedContinuation<Void, Never>?
        private var admitted = false
        func hold() async {
            await withCheckedContinuation {
                held = $0; admitted = true
                admission?.resume(); admission = nil
            }
        }
        func requested() async {
            if admitted { return }
            await withCheckedContinuation { admission = $0 }
        }
        func release() { held?.resume(); held = nil }
    }

    @Test func realChangesSurviveOwnerCloseAndReopenInOneEngine() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let cloudA = SyncOwnerFake(), cloudB = SyncOwnerFake()
        let syncA = SyncCoordinator(bootstrap: {}, makeBackend: { _ in cloudA }, defaults: nil)
        let syncB = SyncCoordinator(bootstrap: {}, makeBackend: { _ in cloudB }, defaults: nil)
        let ownerA = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: syncA,
            providerDirectory: configuration.collectionLocation.directory.appending(path: "synthetic-installation"))
        let ownerB = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: syncB,
            providerDirectory: configuration.collectionLocation.directory.appending(path: "synthetic-installation"))
        ownerA.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        ownerB.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        let service = CoreCollectionService()
        let fanout = ChangeFanout.shared
        let probe = OwnerFanoutProbe()
        defer { fanout.unsubscribe(probe); probe.finish() }
        var secondStarted = false
        do {
            // Also exercise the successful no-file close branch. It must not
            // cancel the process consumer, including while it has no sinks.
            await ownerA.start()
            try #require(ownerA.state == .noFile)
            #expect(fanout.isRegistered(ownerA))
            let emptyClosed = await ownerA.close()
            try #require(emptyClosed)
            #expect(!fanout.isRegistered(ownerA))
            try await fanout.subscribe(probe)

            await ownerA.start()
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            ownerA.entry.present(); ownerA.entry.inspect(source)
            await ownerA.entry.waitForCompletion()
            try #require(ownerA.entry.preview != nil)
            ownerA.entry.confirm(); await ownerA.entry.waitForCompletion()
            try #require(ownerA.state == .ready, "\(ownerA.error ?? "Real import did not open")")
            let generationA = ownerA.generation
            let replacement = try await probe.next { event in
                if case .collectionReplaced(generationA) = event.kind { return true }
                return false
            }
            #expect(replacement.generation == generationA)
            let durableA = try await recordRealAnswer(ownerA, cloud: cloudA, probe: probe)
            #expect(durableA.generation == generationA && durableA.seq > replacement.seq)

            cloudA.onClose = { throw OwnerFanoutFailure.closeRefused }
            let refused = await ownerA.close()
            try #require(!refused)
            #expect(fanout.isRegistered(ownerA))
            let retainedPhase: CollectionLifecycle = try await service.lifecycle()
            #expect(retainedPhase == .ready(generation: generationA))
            cloudA.onClose = nil
            let closedA = await ownerA.close()
            try #require(closedA)
            #expect(!fanout.isRegistered(ownerA))
            let closedEvent = try await probe.next(after: durableA.seq) { event in
                if case .lifecycle(.noCollection) = event.kind { return true }
                return false
            }
            let closedDecks = ownerA.decks, closedInputs = cloudA.events

            // Same singleton, engine and collection file; startOpen still
            // resolves through the real JobRegistry after owner A detached.
            secondStarted = true
            await ownerB.start()
            try #require(ownerB.state == .ready, "\(ownerB.error ?? "Real reopen did not complete")")
            try #require(ownerB.generation > generationA)
            #expect(fanout.isRegistered(ownerB))
            let generationB = ownerB.generation
            let reopened = try await probe.next(after: closedEvent.seq) { event in
                if case .lifecycle(.ready(generationB)) = event.kind { return true }
                return false
            }
            #expect(reopened.generation == generationB)
            await syncB.waitForInputs()
            let localBeforeDeck = cloudB.localRefreshCount
            let added = try await CoreDeckService().addDeck(name: "Fanout after owner reopen")
            let deckChange = try await probe.next(after: reopened.seq) { $0.seq == added.summary.seq }
            #expect(deckChange.generation == generationB)
            #expect(deckChange.kind == .changes(added.summary.changes))
            await syncB.waitForInputs()
            #expect(cloudB.localRefreshCount > localBeforeDeck)
            #expect(cloudB.refreshCauses.contains(.local))

            // A second real job proves raw completion AND app delivery keep
            // working; neither is inferred from the other.
            let rename = try await CoreDeckService().startRenameDeck(id: added.id, name: "Fanout renamed")
            let renamed = try await rename.value()
            #expect(renamed.id == added.id && renamed.summary.generation == generationB)
            let jobID = rename.job.id
            let finished = try await probe.next(after: deckChange.seq) { event in
                if case .jobFinished(jobID, .succeeded) = event.kind { return true }
                return false
            }
            #expect(finished.generation == generationB)
            let durableB = try await recordRealAnswer(ownerB, cloud: cloudB, probe: probe)
            #expect(durableB.generation == generationB && durableB.seq > durableA.seq)
            #expect(syncB.generation == generationB)
            #expect(ownerA.generation == generationA && ownerA.lifecycle == .noCollection)
            #expect(ownerA.state == .unopened && ownerA.review == nil)
            #expect(ownerA.decks == closedDecks && cloudA.events == closedInputs)
            #expect(!fanout.isRegistered(ownerA))
            #expect(zip(probe.received, probe.received.dropFirst()).allSatisfy { $0.0.seq < $0.1.seq })
            let closedB = await ownerB.close()
            try #require(closedB)
            #expect(!fanout.isRegistered(ownerB))
            let finalPhase: CollectionLifecycle = try await service.lifecycle()
            #expect(finalPhase == .noCollection)
        } catch {
            // No held fake inputs or detached producer tasks: all real jobs
            // above are awaited, and each bounded probe read joins its race.
            cloudA.onClose = nil
            if secondStarted { _ = await ownerB.close() }
            else { _ = await ownerA.close() }
            try? await service.flushForBackground()
            try? await service.close()
            throw error
        }
    }

    private func recordRealAnswer(_ owner: ReviewCollection, cloud: SyncOwnerFake, probe: OwnerFanoutProbe) async throws -> CoreEvent {
        if owner.policy != .ok { await owner.enableFsrs() }
        func flattened(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] {
            nodes.flatMap { [$0] + flattened($0.children) }
        }
        let deck = try #require(flattened(owner.decks).first {
            !$0.filtered && ($0.counts.new + $0.counts.review + $0.counts.learning) > 0
        })
        await owner.startStudy(deckID: deck.id)
        let review = try #require(owner.review)
        try #require(review.phase == .question)
        review.reveal(); await review.settled()
        try #require(review.phase == .answer)
        let beforeAnswer = probe.received.last?.seq ?? 0
        await owner.sync.waitForInputs()
        let localBeforeAnswer = cloud.localRefreshCount
        review.rate(.easy); await review.settled()
        try #require(review.record.answers.count == 1)
        let cardID = try #require(review.record.answers.first?.cardID)
        // Flush the actual accepted overlay while its owner is still attached.
        // Only the real Changes event may make the record durable.
        try await CoreCollectionService().flushForBackground()
        let committed = try await probe.next(after: beforeAnswer) { event in
            guard case .changes(let changes) = event.kind else { return false }
            return !changes.revlogAdded.isEmpty && changes.cards.contains { $0.id == cardID }
        }
        #expect(review.record.answers.first?.durable == true)
        await owner.sync.waitForInputs()
        #expect(cloud.localRefreshCount > localBeforeAnswer)
        await owner.finishReview()
        #expect(owner.previousRecord?.answers.count == 1)
        #expect(owner.previousRecord?.answers.allSatisfy(\.durable) == true)
        return committed
    }

    @Test func freshInstallOpensExistingCollectionWithoutBackups() async throws {
        let files = FileManager.default
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        let original = try Data(contentsOf: source)
        let root = files.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        defer { try? files.removeItem(at: root) }
        let support = root.appending(path: "Application Support", directoryHint: .isDirectory)
        let directory = support.appending(path: "Collection", directoryHint: .isDirectory)
        let backups = support.appending(path: "Backups", directoryHint: .isDirectory)
        let exported = root.appending(path: "export.anki2")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: exported)
        let database = directory.appending(path: "collection.anki2")
        try files.copyItem(at: exported, to: database)
        #expect(try Data(contentsOf: database) == original)
        try #require(!files.fileExists(atPath: backups.path))

        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: configuration.collectionLocation.directory.appending(path: "synthetic-installation"))
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        do {
            // No fake open or fixture-global override: this uses startOpen/CoreHost.
            await owner.start()
            try #require(owner.state == .ready, "Real open failed: \(owner.error ?? String(describing: owner.state))")
            #expect(try await service.lifecycle() == .ready(generation: owner.generation))
            let backupsExist = files.fileExists(atPath: backups.path)
            #expect(!backupsExist, "Only the real backup writer creates backup directories after capacity preflight")
            #expect(!owner.decks.isEmpty)
            // Owner close flushes before releasing the real core collection.
            let closed = await owner.close()
            try #require(closed, "Close failed: \(owner.error ?? "unknown error")")
            #expect(try await service.lifecycle() == .noCollection)
            #expect(files.fileExists(atPath: database.path))
            #expect(try Data(contentsOf: source) == original)
            #expect(try Data(contentsOf: exported) == original)
        } catch {
            _ = await owner.close()
            try? await service.flushForBackground()
            try? await service.close()
            throw error
        }
    }

    @Test func nonReadyStartupHostCompletionRecordsUnavailableWithoutSuccess() async throws {
        var configuration = LaunchConfiguration()
        var observations: [FieldDiagnosticObservation] = []
        configuration.recordDiagnostic = { observations.append($0) }
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: configuration.collectionLocation.directory.appending(path: "synthetic-installation"))
        // Exercise the actual nonthrowing lifecycle exit, with no fake ready flag.
        try await owner.completeStartupHost()
        #expect(owner.state == .unavailable && owner.lifecycle == .noCollection)
        #expect(observations.map(\.outcome) == [.unknown])
        #expect(observations.first?.failure == .unavailable)
        #expect(await owner.close())
    }

    @Test func existingInvalidCollectionFailsWithoutChangingOriginalBytes() async throws {
        let files = FileManager.default
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        let root = files.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        defer { try? files.removeItem(at: root) }
        let recovery = root.appending(path: "recovery.anki2")
        try files.createDirectory(at: root, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: recovery)
        let directory = root.appending(path: "Collection", directoryHint: .isDirectory)
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        let database = directory.appending(path: "collection.anki2")
        let original = Data("Synthetic invalid collection for the startup regression.".utf8)
        try original.write(to: database)
        var configuration = LaunchConfiguration()
        var observations: [FieldDiagnosticObservation] = []
        configuration.recordDiagnostic = { observations.append($0) }
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: configuration.collectionLocation.directory.appending(path: "synthetic-installation"))
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        func release() async throws {
            _ = await owner.close()
            try await service.close()
            let archive = root.appending(path: "invalid-original.anki2")
            try files.moveItem(at: database, to: archive)
            observations.removeAll()
            await owner.start()
            #expect(owner.state == .noFile)
            #expect(!observations.contains { $0.component == .startup && $0.outcome == .succeeded })
            #expect(observations.contains { $0.component == .startup && $0.stage == .admission && $0.outcome == .refused })
            let closed = await owner.close()
            try #require(closed)
            #if DEBUG
            #expect(owner.openFailureDiagnosticValue == nil, "A new no-file attempt clears the previous failure")
            #endif

            try files.moveItem(at: recovery, to: database)
            var recoveryFailure: (any Error)?
            do {
                await owner.start()
                #expect(observations.filter { $0.component == .startup && $0.stage == .activation }.map(\.outcome) == [.succeeded])
                try #require(owner.state == .ready, "Recovery open failed: \(owner.error ?? String(describing: owner.state))")
                #if DEBUG
                #expect(owner.openFailureDiagnosticValue == nil)
                #endif
                #expect(try await service.lifecycle() == .ready(generation: owner.generation))
                #expect(try Data(contentsOf: archive) == original)
            } catch { recoveryFailure = error }
            let recoveryClosed = await owner.close()
            if !recoveryClosed {
                Issue.record("Recovery close failed: \(owner.error ?? "unknown error")")
                try await service.close()
                try files.moveItem(at: database, to: root.appending(path: "recovery-closed.anki2"))
                await owner.start()
                let cancelled = await owner.close()
                try #require(cancelled)
            }
            if let recoveryFailure { throw recoveryFailure }
        }
        var failure: (any Error)?
        do {
            await owner.start()
            if case .failed(let message) = owner.state {
                #expect(message.contains("collection"))
                #expect(message.contains("Settings") || message.contains("try again"))
                #expect(!message.contains("detail:"))
                #expect(!message.contains("not a database"))
                #expect(!message.contains(root.path))
                #if DEBUG
                let diagnostic = try #require(owner.openFailureDiagnosticValue)
                #expect(diagnostic.hasPrefix("phase:coreResult;bridge:open;lifecycle:failed.open."))
                #expect(diagnostic.hasSuffix(";step:none;reason:none"))
                #expect(!diagnostic.contains(root.path))
                #endif
            } else {
                Issue.record("An existing invalid collection must present an open failure")
            }
            #expect(try Data(contentsOf: database) == original)
            #expect(owner.decks.isEmpty)
            #expect(observations.filter { $0.component == .startup }.map(\.outcome) == [.observed, .failed])
            #expect(observations.last?.failure == .other)
        } catch { failure = error }
        do {
            try await release()
            #expect(try await service.lifecycle() == .noCollection)
        } catch {
            if let failure { Issue.record(failure) }
            throw error
        }
        if let failure { throw failure }
    }

    #if DEBUG
    @Test func openFailureDiagnosticUsesOnlyFixedCategories() {
        let detail = "synthetic-private-detail;phase:injected;step:injected"
        let bridges: [(BridgeError, String)] = [
            (.cloudPolicy(.closed), "cloudPolicy"), (.cloudConnection(.closed), "cloudConnection"),
            (.maintenance(.io(detail)), "maintenance"), (.exportSourceChanged, "exportSourceChanged"),
            (.exportSourceInvalid, "exportSourceInvalid"), (.exportFormatLimit, "exportFormatLimit"),
            (.deckOptionsInvalid([]), "deckOptionsInvalid"), (.stale(table: .col, key: detail), "stale"),
            (.staleGeneration(planned: 123, current: 456), "staleGeneration"),
            (.undoUnavailable(reason: .none), "undoUnavailable"),
            (.notetypeRefused(.template(kind: .unknownField, template: 123, side: .front, name: detail, earlier: 456)), "notetypeRefused"),
            (.deckRefused(.filteredParent), "deckRefused"), (.noteNotFound(id: 123), "noteNotFound"),
            (.notFound(table: .notes, key: detail), "notFound"),
            (.insufficientSpace(required: 123, available: 456), "insufficientSpace"),
            (.open(.io(detail: detail)), "open"), (.suspendDeadline(phase: detail), "suspendDeadline"),
            (.schedOpRefused(.presetUnsupported(presetIDs: [123])), "schedOpRefused"),
            (.sessionActive, "sessionActive"), (.invalidTypeAnswerReplacement, "invalidTypeAnswerReplacement"),
            (.cancelled, "cancelled"), (.busy(runningOperation: detail), "busy"),
            (.unavailable(reason: detail), "unavailable"), (.unsupported(feature: detail), "unsupported"),
            (.invalidArgument(detail), "invalidArgument"), (.internal(detail), "internal"),
            (.mediaCommitPending(sequence: 123, message: detail), "mediaCommitPending"),
            (.commitCompleted(sequence: 456, message: detail), "commitCompleted")
        ]
        for (error, expected) in bridges {
            #expect(OpenFailureDiagnostic.bridgeCategory(error) == expected)
            let reason = expected == "invalidArgument" ? "review_service_other" : "none"
            for phase in OpenFailureDiagnostic.Phase.allCases {
                #expect(OpenFailureDiagnostic.value(phase: phase, error: error,
                    lifecycle: .failed(.openStep(step: detail, detail: detail))) ==
                    "phase:\(phase.rawValue);bridge:\(expected);lifecycle:failed.openStep;step:other;reason:\(reason)")
            }
        }
        let markers = ["pending_answer_identity_mismatch", "pending_answer_journal_corruption",
            "pending_answer_identity_migration_io", "pending_answer_identity_mismatch_path",
            "pending_answer_identity_mismatch_native", "pending_answer_identity_mismatch_path_native"]
        for marker in markers {
            let message = "review journal: \(marker)"
            let cases: [(any Error, String, String)] = [
                (BridgeError.invalidArgument(message), "invalidArgument", marker),
                (BridgeError.invalidArgument("\(message): \(detail)"), "invalidArgument", "review_journal_other"),
                (BridgeError.invalidArgument("\(message)_unknown"), "invalidArgument", "review_journal_other"),
                (BridgeError.invalidArgument("\(message)\n\(detail)"), "invalidArgument", "review_journal_other"),
                (BridgeError.invalidArgument("review journal: \(detail);\(marker)"), "invalidArgument", "review_journal_other"),
                (BridgeError.invalidArgument(marker), "invalidArgument", "review_service_other"),
                (BridgeError.invalidArgument("\(detail);\(message)"), "invalidArgument", "review_service_other"),
                (BridgeError.invalidArgument("review journal:\(marker)"), "invalidArgument", "review_service_other"),
                (BridgeError.invalidArgument("Review journal: \(marker)"), "invalidArgument", "review_service_other"),
                (BridgeError.internal(message), "internal", "none"),
                (BridgeError.open(.io(detail: message)), "open", "none"),
                (NSError(domain: detail, code: 123,
                    userInfo: [NSUnderlyingErrorKey: BridgeError.invalidArgument(message)]), "other", "other")
            ]
            for (error, bridge, reason) in cases {
                #expect(OpenFailureDiagnostic.value(phase: .coreResult, error: error,
                    lifecycle: .failed(.openStep(step: "review-session", detail: message))) ==
                    "phase:coreResult;bridge:\(bridge);lifecycle:failed.openStep;step:review-session;reason:\(reason)")
            }
        }
        for message in ["review journal: ", "review journal: unknown", "review journal: \(detail)"] {
            #expect(OpenFailureDiagnostic.value(phase: .coreResult, error: BridgeError.invalidArgument(message),
                lifecycle: .failed(.openStep(step: "review-session", detail: detail))) ==
                "phase:coreResult;bridge:invalidArgument;lifecycle:failed.openStep;step:review-session;reason:review_journal_other")
        }
        let states: [(CollectionLifecycle?, String)] = [
            (nil, "queryFailed"), (.noCollection, "noCollection"), (.opening, "opening"),
            (.upgrading(from: 123, step: 456), "upgrading"), (.ready(generation: 123), "ready"),
            (.exclusiveBusy(op: .restore, phase: .working, cancellable: true), "exclusiveBusy"),
            (.failed(.replace(detail: detail)), "failed.replace"),
            (.failed(.corrupt(detail: detail)), "failed.corrupt")
        ]
        for (state, expected) in states {
            #expect(OpenFailureDiagnostic.value(phase: .coreResult,
                error: NSError(domain: detail, code: 123), lifecycle: state) ==
                "phase:coreResult;bridge:other;lifecycle:\(expected);step:none;reason:other")
        }
        let failures: [(CollectionOpenFailure, String)] = [
            (.openInAnotherProgram, "openInAnotherProgram"), (.tooOld(version: 123), "tooOld"),
            (.tooNew(version: 456), "tooNew"), (.v1Scheduler, "v1Scheduler"),
            (.notACollection(missingTables: [detail]), "notACollection"),
            (.upgradeFailed(version: 123, stage: .step(456), detail: detail), "upgradeFailed"),
            (.upgradeFailed(version: 123, stage: .copy, detail: detail), "upgradeFailed"),
            (.corrupt(detail: detail), "corrupt"), (.connectionInit(detail: detail), "connectionInit"),
            (.notFound, "notFound"), (.io(detail: detail), "io")
        ]
        for (failure, expected) in failures {
            #expect(OpenFailureDiagnostic.value(phase: .coreResult, error: BridgeError.open(failure),
                lifecycle: .failed(.open(failure))) ==
                "phase:coreResult;bridge:open;lifecycle:failed.open.\(expected);step:none;reason:none")
        }
        let knownSteps = ["open", "ownership", "open_evidence", "writer", "fsrs_policy", "day_clock",
            "derived-store", "media", "review-session", "cloud-note-capture"]
        for step in knownSteps + [detail, "", "media/\(detail)", "dayclock", " media", "MEDIA"] {
            let expected = knownSteps.contains(step) ? step : "other"
            #expect(OpenFailureDiagnostic.value(phase: .coreResult, error: BridgeError.internal(detail),
                lifecycle: .failed(.openStep(step: step, detail: detail))) ==
                "phase:coreResult;bridge:internal;lifecycle:failed.openStep;step:\(expected);reason:none")
        }
    }
    #endif

    @Test func openFailurePresentationDoesNotEchoDiagnosticDetails() {
        let diagnostic = "synthetic-diagnostic-must-stay-private"
        let failures: [CollectionOpenFailure] = [
            .openInAnotherProgram, .tooOld(version: -1), .tooNew(version: 999), .v1Scheduler,
            .notACollection(missingTables: [diagnostic]),
            .upgradeFailed(version: 18, stage: .step(1), detail: diagnostic),
            .corrupt(detail: diagnostic), .connectionInit(detail: diagnostic),
            .notFound, .io(detail: diagnostic)
        ]
        for failure in failures {
            let message = ReviewCollection.openExplanation(BridgeError.open(failure))
            #expect(!message.isEmpty)
            #expect(!message.contains(diagnostic))
            // Job completion and lifecycle delivery must explain the same refusal.
            #expect(message == ReviewCollection.explanation(CollectionFailedReason.open(failure)))
        }
        #expect(ReviewCollection.explanation(CollectionOpenFailure.openInAnotherProgram).contains("Close it there"))
        #expect(ReviewCollection.explanation(CollectionOpenFailure.tooNew(version: 999)).contains("update"))
        let reasons: [CollectionFailedReason] = [
            .openStep(step: diagnostic, detail: diagnostic), .replace(detail: diagnostic), .corrupt(detail: diagnostic)
        ]
        for reason in reasons { #expect(!ReviewCollection.explanation(reason).contains(diagnostic)) }
        #expect(!ReviewCollection.openExplanation(BridgeError.internal(diagnostic)).contains(diagnostic))
        #expect(!ReviewCollection.openExplanation(NSError(domain: diagnostic, code: 1)).contains(diagnostic))
    }
}

private enum OwnerFanoutFailure: Error {
    case closeRefused, eventStreamEnded, eventTimedOut
}

/// Observes the production fanout only; it cannot inject events or refreshes.
@MainActor
private final class OwnerFanoutProbe: ChangeFanoutSink {
    private let stream: AsyncStream<CoreEvent>
    private let continuation: AsyncStream<CoreEvent>.Continuation
    private(set) var received: [CoreEvent] = []

    init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }
    func receive(_ event: CoreEvent) { received.append(event); continuation.yield(event) }
    func finish() { continuation.finish() }

    func next(after seq: UInt64 = 0, matching predicate: @escaping @Sendable (CoreEvent) -> Bool) async throws -> CoreEvent {
        if let event = received.first(where: { $0.seq > seq && predicate($0) }) { return event }
        let stream = stream
        return try await withThrowingTaskGroup(of: CoreEvent.self) { group in
            group.addTask {
                for await event in stream where event.seq > seq && predicate(event) { return event }
                throw OwnerFanoutFailure.eventStreamEnded
            }
            group.addTask {
                try await Task.sleep(for: .seconds(10))
                throw OwnerFanoutFailure.eventTimedOut
            }
            defer { group.cancelAll() }
            guard let event = try await group.next() else { throw OwnerFanoutFailure.eventStreamEnded }
            return event
        }
    }
}

#if os(macOS)
@MainActor extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1))) func meguriLocalConnectionCommandsNeverStartCollectionOrMediaWork() async throws {
        try await withNativeMeguriSetupOwner { app in
            let core = try await CoreProviderService.attached()
            let store = NativeMeguriSetupCredentials()
            let owner = MeguriProviderConnectionOwner(service: core, store: store)
            let calls = Mutex<[BridgeCall]>([])
            do {
                try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                    try await owner.load()
                    #expect(await owner.phase == .unconfigured)
                    try await owner.configure(endpoint: "https://native-setup.synthetic.example/prefix/")
                    try await owner.load()
                    #expect(await owner.phase == .signedOut)
                    let facade = try await owner.bootstrapService()
                    guard case .notSelected = try await facade.readiness() else {
                        Issue.record("Local-only readiness must not grant setup or routine admission"); return
                    }
                    #expect(calls.withLock { !($0.contains(.providerStartLogin)) })
                    // Core validates this deliberately invalid username before
                    // constructing a transport. No real account is contacted.
                    do {
                        try await owner.signIn(username: "?", password: "synthetic-unused-password")
                        Issue.record("Invalid username unexpectedly authenticated")
                    } catch let error as MeguriProviderAuthenticationFailure {
                        guard case .username = error else { throw error }
                    }
                    #expect(await store.savedCount == 0)
                    #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 1)
                    #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                    #expect(app.sync.providerState?.selection == .localOnly)
                }
                await owner.cancelAndDrain()
            } catch { await owner.cancelAndDrain(); throw error }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriNativeSelectionAndSetupRoutingCannotAdmitARoutineRound() async throws {
        try await withNativeMeguriSetupOwner { app in
            let original = app.sync
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                try #require(await original.changeProvider(to: .meguri))
                try #require(app.sync !== original)
                await app.sync.waitForMeguriAction()
                #expect(app.sync.providerState?.selection == .meguri)
                #expect(app.sync.providerState?.phase == .setupRequired)
                #expect(app.sync.meguriPhase == .unconfigured)
                #expect(app.sync.meguriRoutine == nil && app.sync.meguriRoundReport == nil)
                let core = try await CoreProviderService.attached()
                let selected = try await core.state()
                #expect(selected.selection == .meguri && selected.phase == .setupRequired)
                let settings = try #require(selected.settings)
                #expect(settings.enabled && !settings.automatic)
                #expect(app.sync.settings == settings)
                #expect(app.sync.meguriPreview == nil && app.sync.meguriLastCommand == nil)
                #expect(app.sync.meguriStatus.route == .account("sync.meguri.setup.continue"))
                // No sheet asks (N18.1): the step runs by itself and study joins it.
                try #require(app.owner.navigation.sheet == nil)
                await app.owner.enableFsrs()
                try #require(try await Core.fsrsPolicyStatus() == .ok)
                try #require(app.owner.policy == .ok && app.owner.navigation.sheet == nil)
                #if os(macOS)
                #expect(app.owner.syncPresentation.settingsRequest == nil)
                #endif
                #expect(app.owner.requestSyncNow())
                #if os(macOS)
                #expect(app.owner.syncPresentation.settingsRequest != nil)
                #endif
                app.sync.syncNow() // Direct calls have the same Ready guard.
                await app.sync.waitForInputs()
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                #expect(calls.withLock { !($0.contains(.providerStartLogin)) })
                #expect(app.sync.meguriPreview == nil && app.sync.meguriLastCommand == nil)
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriColdLookupUsesCurrentFacadeAndUnboundCoreRefusesRoutineWork() async throws {
        try await withNativeMeguriSetupOwner { app in
            try #require(await app.sync.changeProvider(to: .meguri))
            await app.sync.waitForMeguriAction()
            let core = try await CoreProviderService.attached()
            let store = NativeMeguriSetupCredentials()
            let original = MeguriProviderConnectionOwner(service: core, store: store)
            try await original.configure(endpoint: "https://native-setup.synthetic.example/")
            let configured = try #require(try await core.configuredMeguri())
            await store.save(.init(connection: configured.connection, username: "synthetic-user", hostKey: "synthetic-key"))
            try await original.load()
            let facade = try await original.bootstrapService()
            let round = try await original.roundService()
            do {
                guard case .unbound(operation: nil) = try await facade.readiness() else {
                    Issue.record("Configured credentials must not manufacture a ready binding"); await original.cancelAndDrain(); return
                }
                #expect(try await facade.pendingPreparation() == nil)
                let report = try await round.fullRound()
                guard case .refused(.notReady) = report.collection.outcome else {
                    Issue.record("Unbound collection must refuse the actual native round"); await original.cancelAndDrain(); return
                }
                #expect(report.media == nil && !report.complete)
                await original.cancelAndDrain()
                await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await facade.readiness() }
                let current = MeguriProviderConnectionOwner(service: try await CoreProviderService.attached(), store: store)
                do {
                    try await current.load()
                    let fresh = try await current.bootstrapService()
                    #expect(fresh !== facade)
                    #expect(try await fresh.pendingPreparation() == nil)
                    // Retirement does not replace the original terminal result.
                    guard case .refused(.notReady) = report.collection.outcome else {
                        Issue.record("Original refusal was lost"); await current.cancelAndDrain(); return
                    }
                    await current.cancelAndDrain()
                } catch { await current.cancelAndDrain(); throw error }
            } catch { await original.cancelAndDrain(); throw error }
        }
    }

    private func withNativeMeguriSetupOwner(meguriCredentialStore: (any MeguriProviderCredentialStore)? = nil,
                                            _ body: (ServiceAppHarness) async throws -> Void) async throws {
        let files = FileManager.default
        let temporary = try #require(realpath(files.temporaryDirectory.path, nil))
        defer { free(temporary) }
        let root = URL(filePath: String(cString: temporary)).appending(path: "synthetic-native-meguri-\(UUID().uuidString)")
        let collection = root.appending(path: "collection")
        let installation = collection.appending(path: "synthetic-installation")
        let backups = root.appending(path: "Backups", directoryHint: .isDirectory)
        let service = root.appending(path: "service")
        for directory in [collection, installation, service] { try files.createDirectory(at: directory, withIntermediateDirectories: true) }
        defer { try? files.removeItem(at: root) }
        let schema = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try files.copyItem(at: schema, to: collection.appending(path: "collection.anki2"))
        let process = try await CloudServiceProcess.open(stateDirectory: service, transport: .binaryAssetsV1)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let app = try ServiceAppHarness(root: collection, wire: wire, cachedIdentityAvailable: false,
                meguriCredentialStore: meguriCredentialStore, clock: .system)
            active = app
            defer { app.defaults.removePersistentDomain(forName: app.suite) }
            #expect(app.owner.configuration.collectionLocation.backupsDirectory == backups)
            await app.owner.start()
            try #require(app.owner.state == .ready)
            await app.sync.waitForInputs()
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
}

@MainActor extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1)))
    func startEmptyRefusalKeepsNoFileAndTrackingFailureKeepsReady() async throws {
        try await withNativeMeguriNoFileOwner { app in
            let originalSync = app.sync
            let generation = app.owner.generation
            let location = app.owner.configuration.collectionLocation
            // Test-owned retained media refuses creation before publication.
            let obstruction = location.directory.appending(path: "collection.media")
            try FileManager.default.createDirectory(at: obstruction, withIntermediateDirectories: false)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                try #require(app.owner.canStartEmpty)
                await app.owner.startEmpty()
                #expect(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                #expect(try await CoreCollectionService().lifecycle() == .noCollection)
                #expect(app.owner.generation == generation && app.owner.sync === originalSync)
                #expect(app.owner.error == "Meguri couldn’t start an empty collection while the collection is busy or unavailable.")
                #expect(!FileManager.default.fileExists(atPath: location.collectionFile.path))
                try #require(app.owner.canStartEmpty)

                try FileManager.default.removeItem(at: obstruction)
                await app.owner.startEmpty() // A later explicit action, never generic Retry.
                try #require(app.owner.state == .ready)
                let readyGeneration = app.owner.generation
                #expect(readyGeneration > generation && app.owner.error == nil)
                #expect(FileManager.default.fileExists(atPath: location.collectionFile.path))

                // Reconcile a drained listener failure with a real Ready Host.
                try app.owner.beginEntryActivation()
                try await app.owner.drainEntryActivation()
                await app.owner.reconcileEmptyCreationFailure(BridgeError.internal("synthetic diagnostic detail"))
                #expect(app.owner.state == .ready)
                #expect(app.owner.lifecycle == .ready(generation: readyGeneration))
                #expect(try await CoreCollectionService().lifecycle() == .ready(generation: readyGeneration))
                #expect(app.owner.generation == readyGeneration && app.owner.sync === originalSync)
                #expect(app.owner.error == "Meguri couldn’t finish starting an empty collection.")
                #expect(!app.owner.canStartEmpty)
                await app.owner.startEmpty()
                #expect(calls.withLock { $0.filter { $0 == .startCreateEmpty }.count } == 2)
                #expect(calls.withLock { !$0.contains(.startOpen) && !$0.contains(.providerStartLogin) })
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
            }
        }
    }

    @Test(arguments: [false, true])
    func startEmptyOpensRealDefaultDeckWithoutProviderTransfer(coldSelected: Bool) async throws {
        try await withNativeMeguriNoFileOwner(coldSelected: coldSelected) { app in
            try #require(app.owner.canStartEmpty)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                async let first: Void = app.owner.startEmpty()
                async let repeated: Void = app.owner.startEmpty()
                _ = await (first, repeated)
                try #require(app.owner.state == .ready, "\(app.owner.error ?? "Creation did not become ready")")
                #expect(app.owner.lifecycle == .ready(generation: app.owner.generation))
                #expect(app.owner.generation > 0)
                #expect(app.owner.decks.count == 1 && app.owner.decks.first?.name == "Default")
                #expect(!app.owner.canStartEmpty)
                #expect(calls.withLock { $0.filter { $0 == .startCreateEmpty }.count } == 1)
                #expect(calls.withLock { !$0.contains(.startOpen) && !$0.contains(.providerStartLogin) })
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                let generation = app.owner.generation
                await app.owner.startEmpty()
                #expect(app.owner.generation == generation)
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func coldMeguriStoredPauseBecomesAutomaticOffAtTheAttachment() async throws {
        // sync/policy P25.2: no Resume exists. A pause that an earlier build stored
        // is turned into Sync Automatically off, in one provider write, when the
        // person's first sync action attaches the service.
        try await withNativeMeguriNoFileOwner(coldSelected: true, coldPaused: true) { app in
            let sync = app.sync
            try #require(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
            try #require(sync.providerState?.selection == .meguri && sync.settings.paused && sync.updateHold)
            try #require(app.backends.isEmpty && !sync.meguriSignInAttached)
            app.defaults.removeObject(forKey: "meguri.sync.round2Migrated")
            app.defaults.removeObject(forKey: "meguri.sync.automatic")
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                // Choosing or signing in to the service clears the hold first, then attaches.
                #expect(await sync.clearUpdateHold())
                #expect(!sync.storedAutomatic)
                try await app.owner.prepareSyncServiceSelection()
                await sync.waitForInputs()
                await sync.waitForMeguriAction()
                try #require(app.sync === sync && sync.meguriSignInAttached && app.backends.count == 1)
                #expect(!app.owner.meguriSignInPreparing && !sync.deviceSettingsSettling)
                #expect(!sync.settings.paused && !sync.settings.automatic && sync.settings.media && !sync.updateHold)
                let provider = try await CoreProviderService.attached()
                let committed = try #require(try await provider.refresh().settings)
                #expect(!committed.paused && !committed.automatic)
                #expect(app.defaults.bool(forKey: "meguri.sync.round2Migrated"))
                sync.loadMeguriConnection()
                await sync.waitForMeguriAction()
                let reloaded = try #require(try await provider.refresh().settings)
                #expect(!sync.settings.paused && reloaded == committed)
                #expect(try await provider.configuredMeguri() == nil)
                #expect(try await CoreCollectionService().lifecycle() == .noCollection)
                #expect(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
                #expect(calls.withLock { $0.filter { $0 == .providerSaveSettings }.count } == 1)
                #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) && !$0.contains(.providerStartLogin) })
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                #expect(sync.meguriPreview == nil && sync.meguriLastCommand == nil && sync.meguriRoundReport == nil)
            }
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [true, false])
    func coldMeguriHoldClearedBeforeARelaunchCommitsAutomaticOffAtTheLaterAttachment(clearsAgain: Bool) async throws {
        // sync/policy P25.2, P25.3: with no provider attached, a clearing act changes
        // only this device's own values, and the app may quit before the service
        // attaches. The earlier build left the device paused and stored no value of
        // its own for Sync Automatically, which then reads on (P25.3). The later
        // attachment must commit the pause as off, with or without a second act, and
        // the device's own value must stay off so a completed setup cannot turn it on.
        try await withNativeMeguriNoFileOwner(coldSelected: true, coldPaused: true) { app in
            let sync = app.sync
            try #require(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
            try #require(sync.providerState?.selection == .meguri && sync.settings.paused && sync.updateHold)
            try #require(app.backends.isEmpty && !sync.meguriSignInAttached)
            app.defaults.removeObject(forKey: "meguri.sync.round2Migrated")
            app.defaults.removeObject(forKey: "meguri.sync.automatic")
            try #require(sync.storedAutomatic)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                #expect(await sync.clearUpdateHold())
                #expect(!sync.storedAutomatic && !sync.settings.paused && !sync.settings.automatic)
                // Nothing reached the provider: no Host was prepared for this owner.
                #expect(app.backends.isEmpty && calls.withLock { !$0.contains(.providerSaveSettings) })
                #expect(!app.defaults.bool(forKey: "meguri.sync.round2Migrated"))
                #expect(app.defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
                try #require(await app.owner.close())
                // The next launch: the same installation and the same stored values.
                let relaunched = try ServiceAppHarness(root: app.root, wire: app.wire, defaults: app.defaults,
                    cachedIdentityAvailable: false, clock: .system)
                do {
                    await relaunched.owner.start()
                    let later = relaunched.sync
                    try #require(relaunched.owner.state == .noFile && later.providerState?.selection == .meguri)
                    try #require(relaunched.backends.isEmpty && !later.meguriSignInAttached)
                    // The service's own record was never written, yet the switch reads off.
                    #expect(!later.storedAutomatic && !later.automaticSwitchValue)
                    if clearsAgain { #expect(await later.clearUpdateHold()) }
                    try await relaunched.owner.prepareSyncServiceSelection()
                    await later.waitForInputs()
                    await later.waitForMeguriAction()
                    try #require(relaunched.sync === later && later.meguriSignInAttached && relaunched.backends.count == 1)
                    #expect(!later.settings.paused && !later.settings.automatic && later.settings.media && !later.updateHold)
                    #expect(!later.storedAutomatic && !later.automaticSwitchValue && !later.deviceSettingsSettling)
                    let provider = try await CoreProviderService.attached()
                    let committed = try #require(try await provider.refresh().settings)
                    #expect(!committed.paused && !committed.automatic)
                    #expect(app.defaults.bool(forKey: "meguri.sync.round2Migrated"))
                    #expect(app.defaults.object(forKey: "meguri.sync.automatic") as? Bool == false)
                    #expect(calls.withLock { $0.filter { $0 == .providerSaveSettings }.count } == 1)
                    #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) && !$0.contains(.providerStartLogin) })
                    try #require(await relaunched.owner.close())
                } catch { _ = await relaunched.owner.close(); throw error }
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func coldMeguriNoCollectionSignInAttachesBeforeAuthenticating() async throws {
        let store = NativeMeguriSetupCredentials()
        try await withNativeMeguriNoFileOwner(coldSelected: true, meguriCredentialStore: store) { app in
            let sync = app.sync
            try #require(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
            try #require(sync.providerState?.selection == .meguri)
            try #require(app.backends.isEmpty && sync.connectionService == nil && !sync.meguriSignInAttached)
            let endpoint = "https://cold-sign-in.synthetic.example/"
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                // Reproduce the installed form's former direct call before
                // exercising its replacement. No provider/configuration exists.
                sync.signInMeguri(endpoint: endpoint, username: "?", password: "synthetic-unused-password")
                await sync.waitForMeguriAction()
                #expect(sync.meguriSignInErrorKey == "sync.meguri.error.other")
                guard let localError = sync.meguriLastError as? BridgeError, case .unavailable = localError else {
                    Issue.record("Expected the actual unattached local refusal"); return
                }
                #expect(!sync.meguriSignInAttached && app.backends.isEmpty)
                #expect(calls.withLock { !$0.contains(.prepareCloudConnectionHost) })
                #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) && !$0.contains(.providerStartLogin) })

                app.owner.signInMeguri(endpoint: endpoint, username: "?", password: "synthetic-unused-password")
                #expect(app.owner.meguriSignInPreparing)
                // A second synchronous submit owns no second task or draft.
                app.owner.signInMeguri(endpoint: "https://ignored.synthetic.example/", username: "?", password: "synthetic-unused-password")
                await app.owner.waitForMeguriSignInPreparation()
                await sync.waitForMeguriAction()
                #expect(app.sync === sync && !app.owner.meguriSignInPreparing && !sync.meguriBusy)
                #expect(sync.meguriSignInAttached && sync.connectionService != nil && app.backends.count == 1)
                let provider = try await CoreProviderService.attached()
                let configured = try #require(try await provider.configuredMeguri())
                #expect(configured.connection.endpoint == endpoint)
                guard let failure = sync.meguriLastError as? MeguriProviderAuthenticationFailure,
                      case .username = failure else {
                    Issue.record("Expected real configured login's no-network username refusal"); return
                }
                let key = try #require(sync.meguriSignInErrorKey)
                #expect(key == "sync.meguri.error.credentials")
                #expect(SyncText.value(key) == "The username or password isn’t correct.")
                #expect(try await CoreCollectionService().lifecycle() == .noCollection)
                #expect(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
                #expect(await store.savedCount == 0)
                #expect(calls.withLock { $0.filter { $0 == .prepareCloudConnectionHost }.count } == 1)
                #expect(calls.withLock { $0.filter { $0 == .providerConfigureMeguri }.count } == 1)
                #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 1)
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                #expect(sync.meguriPreview == nil && sync.meguriLastCommand == nil && sync.meguriRoundReport == nil)
            }
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["background", "address", "attachment", "busy"])
    func coldMeguriSignInFencesSuspendedPreparation(change: String) async throws {
        try await withNativeMeguriNoFileOwner(coldSelected: true, meguriCredentialStore: NativeMeguriSetupCredentials()) { app in
            let gate = StartupResultGate()
            app.beforeBackend = {
                await gate.hold()
                if change == "attachment" { throw BridgeError.unavailable(reason: "synthetic local setup refusal") }
                if change == "busy" { throw BridgeError.busy(runningOperation: "synthetic local operation") }
            }
            let calls = Mutex<[BridgeCall]>([])
            await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                app.owner.signInMeguri(endpoint: "https://fenced-sign-in.synthetic.example/", username: "?", password: "synthetic-unused-password")
                await gate.requested()
                #expect(app.owner.meguriSignInPreparing && !app.sync.serviceChangeAvailable)
                switch change {
                case "background": app.owner.setForeground(false)
                case "address": app.sync.meguriAddressEdited()
                default: break
                }
                gate.release()
                await app.owner.waitForMeguriSignInPreparation()
                await app.sync.waitForMeguriAction()
                await app.owner.waitForLifecycle()
                #expect(!app.owner.meguriSignInPreparing)
                if change == "attachment" || change == "busy" {
                    let key = app.sync.meguriSignInErrorKey
                    #expect(key == (change == "busy" ? "sync.meguri.error.busy" : "sync.meguri.error.localSetup"))
                    #expect(key.map { SyncText.value($0) != $0 } == true)
                }
                #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) && !$0.contains(.providerStartLogin) })
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
                #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriUnchangedAddressAuthenticationRetryKeepsExactConnection() async throws {
        try await withNativeMeguriSetupOwner { app in
            try #require(await app.sync.changeProvider(to: .meguri))
            await app.sync.waitForMeguriAction()
            let core = try await CoreProviderService.attached()
            let configured = try await core.configureMeguri(endpoint: "https://native-auth.synthetic.example/prefix/")
            app.sync.loadMeguriConnection()
            await app.sync.waitForMeguriAction()
            let original = try await app.sync.meguriConnection()
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                // Reloading the configured spelling is programmatic, not an edit.
                app.sync.loadMeguriConnection()
                await app.sync.waitForMeguriAction()
                for _ in 0..<2 {
                    app.sync.signInMeguri(endpoint: configured.connection.endpoint, username: "?", password: "synthetic-unused-password")
                    await app.sync.waitForMeguriAction()
                    let currentConnection = try await app.sync.meguriConnection()
                    #expect(currentConnection === original)
                    let currentConfiguredConnection = try await core.configuredMeguri()?.connection
                    #expect(currentConfiguredConnection == configured.connection)
                    // Requires the normal native credential-read capability;
                    // a storage refusal is not an authentication pass.
                    guard let failure = app.sync.meguriLastError as? MeguriProviderAuthenticationFailure,
                          case .username = failure else {
                        Issue.record("Expected the actual no-network username refusal"); return
                    }
                    #expect(app.sync.meguriSignInErrorKey == "sync.meguri.error.credentials")
                }
                #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) })
                #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 2)
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriExplicitAddressEditSurvivesLocalReloadAndStillConfigures() async throws {
        let store = NativeMeguriSetupCredentials()
        try await withNativeMeguriSetupOwner(meguriCredentialStore: store) { app in
            try #require(await app.sync.changeProvider(to: .meguri))
            await app.sync.waitForMeguriAction()
            let core = try await CoreProviderService.attached()
            let configured = try await core.configureMeguri(endpoint: "https://native-edit.synthetic.example/")
            app.sync.loadMeguriConnection()
            await app.sync.waitForMeguriAction()
            let original = try await app.sync.meguriConnection()
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                // Even editing back to the same text is a deliberate edit.
                app.sync.meguriAddressEdited()
                app.sync.loadMeguriConnection()
                await app.sync.waitForMeguriAction()
                func phaseTag(_ phase: MeguriProviderConnectionPhase) -> String {
                    switch phase {
                    case .unconfigured: "unconfigured"
                    case .signedOut: "signedOut"
                    case .authenticating: "authenticating"
                    case .setupRequired: "setupRequired"
                    case .cancelled: "cancelled"
                    case .blocked: "blocked"
                    }
                }
                func errorKeyTag(_ key: String?) -> String {
                    switch key {
                    case nil: "none"
                    case "sync.meguri.error.address": "sync.meguri.error.address"
                    case "sync.meguri.error.credentials": "sync.meguri.error.credentials"
                    case "sync.meguri.error.redirect": "sync.meguri.error.redirect"
                    case "sync.meguri.error.network": "sync.meguri.error.network"
                    case "sync.meguri.error.other": "sync.meguri.error.other"
                    default: "unknown"
                    }
                }
                let beforeError = Self.nativeMeguriErrorCategory(app.sync.meguriLastError)
                let beforePhase = phaseTag(app.sync.meguriPhase)
                let beforeKey = errorKeyTag(app.sync.meguriSignInErrorKey)
                app.sync.signInMeguri(endpoint: configured.connection.endpoint, username: "?", password: "synthetic-unused-password")
                let reserved = app.sync.meguriBusy
                #expect(reserved)
                await app.sync.waitForMeguriAction()
                let afterError = Self.nativeMeguriErrorCategory(app.sync.meguriLastError)
                let afterPhase = phaseTag(app.sync.meguriPhase)
                let afterKey = errorKeyTag(app.sync.meguriSignInErrorKey)
                let afterBusy = app.sync.meguriBusy
                print("nativeMeguriAddressEdit before.error=\(beforeError) before.phase=\(beforePhase) before.key=\(beforeKey) reserved=\(reserved) after.error=\(afterError) after.phase=\(afterPhase) after.key=\(afterKey) after.busy=\(afterBusy)")
                #expect(!afterBusy)
                let actualUsernameRefusal: Bool
                if let failure = app.sync.meguriLastError as? MeguriProviderAuthenticationFailure, case .username = failure {
                    actualUsernameRefusal = true
                } else { actualUsernameRefusal = false }
                try #require(actualUsernameRefusal, "Expected the actual no-network username refusal")
                #expect(try await app.sync.meguriConnection() === original)
                let current = try #require(try await core.configuredMeguri())
                #expect(current.connection != configured.connection)
                #expect(current.connection.revision != configured.connection.revision)
                #expect(current.connection.endpoint == configured.connection.endpoint)
                let deletedOriginalConnection = await store.deleteAttempts == [configured.connection]
                #expect(deletedOriginalConnection)
                #expect(await store.savedCount == 0)
                #expect(calls.withLock { $0.filter { $0 == .providerConfigureMeguri }.count } == 1)
                #expect(calls.withLock { $0.filter { $0 == .providerStartLogin }.count } == 1)
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriExplicitAddressEditRetainsCredentialDeletionFailure() async throws {
        // Simulated store failure, independent of signed device Keychain capability.
        let deleteFailure = MeguriProviderCredentialFailure.access(-12345)
        let store = NativeMeguriSetupCredentials(deleteFailure: deleteFailure)
        try await withNativeMeguriSetupOwner(meguriCredentialStore: store) { app in
            try #require(await app.sync.changeProvider(to: .meguri))
            await app.sync.waitForMeguriAction()
            let core = try await CoreProviderService.attached()
            let configured = try await core.configureMeguri(endpoint: "https://native-edit.synthetic.example/")
            app.sync.loadMeguriConnection()
            await app.sync.waitForMeguriAction()
            let original = try await app.sync.meguriConnection()
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                app.sync.meguriAddressEdited()
                app.sync.loadMeguriConnection()
                await app.sync.waitForMeguriAction()
                app.sync.signInMeguri(endpoint: configured.connection.endpoint, username: "?", password: "synthetic-unused-password")
                #expect(app.sync.meguriBusy)
                await app.sync.waitForMeguriAction()
                #expect(!app.sync.meguriBusy)
                let retainedDeleteFailure = (app.sync.meguriLastError as? MeguriProviderCredentialFailure) == deleteFailure
                try #require(retainedDeleteFailure)
                #expect(try await app.sync.meguriConnection() === original)
                let current = try #require(try await core.configuredMeguri())
                let originalConfigurationRetained = current.connection == configured.connection
                #expect(originalConfigurationRetained)
                let deletedOriginalConnection = await store.deleteAttempts == [configured.connection]
                #expect(deletedOriginalConnection)
                #expect(await store.savedCount == 0)
                #expect(calls.withLock { !$0.contains(.providerConfigureMeguri) })
                #expect(calls.withLock { !$0.contains(.providerStartLogin) })
                #expect(calls.withLock { Set($0).isDisjoint(with: nativeMeguriTransferCalls) })
            }
        }
    }

    private static func nativeMeguriErrorCategory(_ error: (any Error)?) -> String {
        guard let error else { return "none" }
        if error is CancellationError { return "CancellationError" }
        if let failure = error as? MeguriProviderCredentialFailure {
            return switch failure {
            case .access(let code): "credential.access(\(code))"
            case .malformedPayload: "credential.malformedPayload"
            case .wrongConnection: "credential.wrongConnection"
            }
        }
        if let failure = error as? MeguriProviderAuthenticationFailure {
            return switch failure {
            case .connectionChanged: "authentication.connectionChanged"
            case .username: "authentication.username"
            case .credentials: "authentication.credentials"
            case .redirectRefused: "authentication.redirectRefused"
            case .network: "authentication.network"
            }
        }
        if let failure = error as? BridgeError {
            return switch failure {
            case .cloudPolicy: "bridge.cloudPolicy"
            case .cloudConnection: "bridge.cloudConnection"
            case .cloudSDKCallbackRejected: "bridge.cloudSDKCallbackRejected"
            case .maintenance: "bridge.maintenance"
            case .exportSourceChanged: "bridge.exportSourceChanged"
            case .exportSourceInvalid: "bridge.exportSourceInvalid"
            case .exportFormatLimit: "bridge.exportFormatLimit"
            case .deckOptionsInvalid: "bridge.deckOptionsInvalid"
            case .stale: "bridge.stale"
            case .staleGeneration: "bridge.staleGeneration"
            case .undoUnavailable: "bridge.undoUnavailable"
            case .notetypeRefused: "bridge.notetypeRefused"
            case .deckRefused: "bridge.deckRefused"
            case .noteNotFound: "bridge.noteNotFound"
            case .notFound: "bridge.notFound"
            case .insufficientSpace: "bridge.insufficientSpace"
            case .open: "bridge.open"
            case .suspendDeadline: "bridge.suspendDeadline"
            case .schedOpRefused: "bridge.schedOpRefused"
            case .sessionActive: "bridge.sessionActive"
            case .invalidTypeAnswerReplacement: "bridge.invalidTypeAnswerReplacement"
            case .cancelled: "bridge.cancelled"
            case .busy: "bridge.busy"
            case .unavailable: "bridge.unavailable"
            case .unsupported: "bridge.unsupported"
            case .invalidArgument: "bridge.invalidArgument"
            case .internal: "bridge.internal"
            case .mediaCommitPending: "bridge.mediaCommitPending"
            case .commitCompleted: "bridge.commitCompleted"
            }
        }
        return "unknown"
    }

    @Test(.timeLimit(.minutes(1))) func meguriNoFileNonactivationRetainsValidatedAttachmentAndOriginalRefusal() async throws {
        try await withNativeMeguriNoFileOwner { app in
            let sync = app.sync
            let core = try await CoreProviderService.attached()
            let store = NativeMeguriSetupCredentials()
            let owner = MeguriProviderConnectionOwner(service: core, store: store)
            do {
                try await owner.configure(endpoint: "https://native-no-file.synthetic.example/")
                let configured = try #require(try await core.configuredMeguri())
                await store.save(.init(connection: configured.connection, username: "?", hostKey: "synthetic-unused-key"))
                try await owner.load()
                let facade = try await owner.bootstrapService()
                let calls = Mutex<[BridgeCall]>([])
                try await MaintenanceCallObservation.$capture.withValue({ value in calls.withLock { $0.append(value) } }) {
                    // Actual local refusal, not a fabricated preview or receipt.
                    // A positive confirm/network failure needs the own-server fixture.
                    let result = try await facade.prepare(.useServerCollection)
                    sync.retainMeguriCollectionResult(result)
                    let begin = try #require(sync.beginMeguriActivation)
                    let finish = try #require(sync.finishMeguriActivation)
                    let prepare = try #require(sync.prepareEditorReplacement)
                    let release = try #require(sync.finishEditorReplacement)
                    let decision = ReviewCollection.EditorReplacementDecision()
                    let admission = try #require(await prepare("Meguri Sync", decision, { app.sync === sync }))
                    defer { release(admission.id) }
                    try #require(admission.accepted)
                    try begin(admission.id)
                    let current = await finish(false)
                    await owner.cancelAndDrain()
                    #expect(current === sync && app.sync === sync)
                    #expect(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
                    #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
                    #expect(sync.meguriStage == .failed)
                    #expect(sync.meguriFailureKey == "sync.meguri.setup.error.identityChanged")
                    guard case .refused(.username, _) = result.outcome,
                          let retained = sync.meguriCollectionCommand,
                          case .refused(.username, _) = retained.outcome else {
                        Issue.record("Original actual username refusal was lost"); return
                    }
                    #expect(sync.meguriCollectionError == nil && sync.meguriPreview == nil)
                    #expect(!sync.meguriCanFinish && !sync.meguriCanRetryMedia)
                    #expect(calls.withLock { $0.filter { $0 == .providerBootstrapPrepare }.count } == 1)
                    #expect(calls.withLock { $0.contains(.providerRefresh) })
                    #expect(calls.withLock { Set($0).intersection(nativeMeguriTransferCalls) } == [.providerBootstrapPrepare])
                }
            } catch { await owner.cancelAndDrain(); throw error }
        }
    }

    @Test(.timeLimit(.minutes(1))) func meguriNoFileNonactivationCannotReturnADrainedAttachment() async throws {
        try await withNativeMeguriNoFileOwner { app in
            let original = app.sync
            let begin = try #require(original.beginMeguriActivation)
            let finish = try #require(original.finishMeguriActivation)
            let prepare = try #require(original.prepareEditorReplacement)
            let release = try #require(original.finishEditorReplacement)
            let decision = ReviewCollection.EditorReplacementDecision()
            let admission = try #require(await prepare("Meguri Sync", decision, { app.sync === original }))
            defer { release(admission.id) }
            try #require(admission.accepted)
            try begin(admission.id)
            let drained = await original.cancelAndDrain()
            let current = await finish(false)
            #expect(drained)
            #expect(current == nil)
            #expect(app.owner.state == .noFile && app.owner.lifecycle == .noCollection)
            #expect(!FileManager.default.fileExists(atPath: app.owner.configuration.collectionLocation.collectionFile.path))
        }
    }

    /// `sched/app-coordination` F5.1 on a cold launch with Meguri Sync selected,
    /// with the preparation running unprompted and the real coordinator. The
    /// core refuses Sync Automatically for Meguri without a binding, and this
    /// harness has no signed-in Meguri server: the setting stays off here, so
    /// the job runs at once (F5.1's first item). What the coordinator publishes
    /// while a connection load is in flight is read as it is, and the mapping
    /// is asked what it means with Sync Automatically on: not a known state.
    @Test(.timeLimit(.minutes(1))) func coldMeguriLaunchPreparesUnpromptedAndALoadInFlightIsNotAKnownState() async throws {
        try await withNativeMeguriSetupOwner { first in
            try #require(await first.sync.changeProvider(to: .meguri))
            await first.sync.waitForMeguriAction()
            let provider = try await CoreProviderService.attached()
            let selected = try await provider.refresh()
            try #require(selected.selection == .meguri && selected.settings?.automatic == false)
            // The scripted first owner never ran the step: the switch is still off.
            try #require(try await Core.fsrsPolicyStatus() == .needsEnable)
            try #require(await first.owner.close())

            let app = try ServiceAppHarness(root: first.root, wire: first.wire, cachedIdentityAvailable: false, clock: .system)
            defer { app.defaults.removePersistentDomain(forName: app.suite) }
            let owner = app.owner
            owner.studyPreparation.runsUnprompted = true
            var environment = try #require(owner.studyPreparation.environment)
            let start = environment.start
            let waiting = StudyPreparationSyncFacts(exchangeExpected: true, activity: .waiting)
            /// The published state as it is, read as if Sync Automatically were on.
            @MainActor func automatic(_ sync: SyncCoordinator) -> StudyPreparationSyncFacts {
                var input = sync.studyPreparationInput
                input.automatic = true
                return .mapping(input)
            }
            var factsAtStart: [StudyPreparationSyncFacts] = [], consistentAtStart: [Bool] = []
            environment.start = {
                let sync = owner.sync
                factsAtStart.append(sync.studyPreparationFacts)
                // Whenever the job starts, a load still in flight reads as waiting.
                consistentAtStart.append(automatic(sync) == (sync.meguriLocalActionAvailable ? .none : waiting))
                return try await start()
            }
            owner.studyPreparation.environment = environment
            do {
                await owner.start()
                try #require(owner.state == .ready)
                let sync = owner.sync
                try #require(sync.providerState?.selection == .meguri && !sync.settings.automatic && !sync.settings.paused)
                await sync.waitForMeguriAction()
                await owner.studyPreparation.waitForJob()
                // One job, at once: with Sync Automatically off no exchange comes by itself.
                #expect(factsAtStart == [.none])
                #expect(consistentAtStart == [true])
                #expect(sync.meguriPhase == .unconfigured && owner.policy == .ok)
                #expect(try await Core.fsrsPolicyStatus() == .ok)
                #expect(automatic(sync) == .none)
                // A reload (the retry control of the setup view) is a load in
                // flight again: an exchange may follow, so nothing is "known".
                sync.loadMeguriConnection()
                #expect(sync.meguriBusy && !sync.studyPreparationInput.meguriSettled && !sync.studyPreparationInput.meguriReady)
                #expect(automatic(sync) == waiting)
                // With the setting off, as it is here, the state is known all along.
                #expect(sync.studyPreparationFacts == .none)
                await sync.waitForMeguriAction()
                #expect(sync.studyPreparationInput.meguriSettled && automatic(sync) == .none)
                #expect(factsAtStart.count == 1)
                try #require(await owner.close())
            } catch {
                _ = await owner.close()
                throw error
            }
        }
    }

    private func withNativeMeguriNoFileOwner(coldSelected: Bool = false, coldPaused: Bool = false,
                                            meguriCredentialStore: (any MeguriProviderCredentialStore)? = nil,
                                            _ body: (ServiceAppHarness) async throws -> Void) async throws {
        let files = FileManager.default
        let temporary = try #require(realpath(files.temporaryDirectory.path, nil))
        defer { free(temporary) }
        let root = URL(filePath: String(cString: temporary)).appending(path: "synthetic-native-meguri-no-file-\(UUID().uuidString)")
        let collection = root.appending(path: "collection")
        let installation = collection.appending(path: "synthetic-installation")
        let service = root.appending(path: "service")
        for directory in [collection, installation, service] { try files.createDirectory(at: directory, withIntermediateDirectories: true) }
        defer { try? files.removeItem(at: root) }
        let process = try await CloudServiceProcess.open(stateDirectory: service, transport: .binaryAssetsV1)
        var active: ServiceAppHarness?
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let prepared = try ServiceAppHarness(root: collection, wire: wire, cachedIdentityAvailable: false,
                meguriCredentialStore: meguriCredentialStore, clock: .system)
            var app = prepared
            active = app
            defer { prepared.defaults.removePersistentDomain(forName: prepared.suite) }
            await app.owner.start()
            try #require(app.owner.state == .noFile)
            try await app.owner.prepareSyncServiceSelection()
            try #require(await app.sync.changeProvider(to: .meguri))
            await app.sync.waitForMeguriAction()
            if coldSelected {
                let provider = try await CoreProviderService.attached()
                try #require(try await provider.configuredMeguri() == nil)
                if coldPaused {
                    let settings = try #require(try await provider.refresh().settings)
                    let paused = try await provider.saveSettings(.init(enabled: settings.enabled, paused: true,
                        automatic: settings.automatic, media: settings.media))
                    try #require(paused.settings?.paused == true)
                }
                try #require(await app.owner.close())
                // A fresh owner starts from the durable selection alone. Its
                // start() must not prepare a Host or create a collection file.
                app = try ServiceAppHarness(root: collection, wire: wire, cachedIdentityAvailable: false,
                    meguriCredentialStore: meguriCredentialStore, clock: .system)
                active = app
                await app.owner.start()
            }
            defer { if coldSelected { app.defaults.removePersistentDomain(forName: app.suite) } }
            try await body(app)
            let closed = await app.owner.close()
            let exit = await process.close()
            try #require(closed)
            try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            try #require(exit.diagnostics.isEmpty)
        } catch {
            _ = await active?.owner.close()
            _ = await process.cancel()
            throw error
        }
    }
}

private let nativeMeguriTransferCalls: Set<BridgeCall> = [
    .providerBootstrapPrepare, .providerBootstrapConfirm, .providerBootstrapRecovery,
    .providerBootstrapMediaPrepare, .providerBootstrapMedia, .providerBootstrapComplete,
    .providerStartCollectionRound, .providerStartMediaRound,
]
private actor NativeMeguriSetupCredentials: MeguriProviderCredentialStore {
    private var value: MeguriProviderCredentials?
    private let deleteFailure: MeguriProviderCredentialFailure?
    private(set) var savedCount = 0
    private(set) var deleteAttempts: [MeguriProviderConnection] = []
    init(deleteFailure: MeguriProviderCredentialFailure? = nil) { self.deleteFailure = deleteFailure }
    func load(connection: MeguriProviderConnection) -> MeguriProviderCredentials? { value }
    func save(_ credentials: MeguriProviderCredentials) { value = credentials; savedCount += 1 }
    func delete(connection: MeguriProviderConnection) throws {
        deleteAttempts.append(connection)
        if let deleteFailure { throw deleteFailure }
        value = nil
    }
}
#endif
