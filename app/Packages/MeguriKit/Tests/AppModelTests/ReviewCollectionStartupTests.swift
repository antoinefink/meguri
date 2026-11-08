import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Owns the process's real collection; run this suite with a focused selector.
@MainActor
@Suite(.serialized) struct ReviewCollectionStartupTests {
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
            #expect(model.phase == .question && model.paused && !model.canReveal)
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
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
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
        let ownerA = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: syncA)
        let ownerB = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: syncB)
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
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
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
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        func release() async throws {
            _ = await owner.close()
            try await service.close()
            let archive = root.appending(path: "invalid-original.anki2")
            try files.moveItem(at: database, to: archive)
            await owner.start()
            let closed = await owner.close()
            try #require(closed)
            #if DEBUG
            #expect(owner.openFailureDiagnosticValue == nil, "A new no-file attempt clears the previous failure")
            #endif

            try files.moveItem(at: recovery, to: database)
            var recoveryFailure: (any Error)?
            do {
                await owner.start()
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
