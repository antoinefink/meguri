@testable import CoreBridge
import Foundation
import Testing
import Synchronization
@testable import AppModel

private struct RefusingExportService: CollectionExportService {
    let cause: BridgeError
    var prepareReal = false
    var uncertain = false
    func prepare(deck: DeckExportRequest?, operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        if prepareReal { return try await CoreCollectionExportService().prepare(deck: deck, operation: operation, progress: progress) }
        if uncertain { throw ExportDestinationCompletionUncertain() }
        throw CollectionExportFailure(cause: cause, destination: URL(fileURLWithPath: "/synthetic/do-not-display"))
    }
    func save(_ artifact: ExportedCollection, to: URL, operation: CollectionExportOperation) async throws {
        if uncertain { throw ExportDestinationCompletionUncertain() }
        throw CollectionExportFailure(cause: cause, destination: to)
    }
}

/// The real service, keeping the last preparation error for a failed expectation's text.
private final class ObservedExportService: CollectionExportService {
    let lastError = Mutex<String?>(nil)
    func prepare(deck: DeckExportRequest?, operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        do { return try await CoreCollectionExportService().prepare(deck: deck, operation: operation, progress: progress) }
        catch { lastError.withLock { $0 = String(describing: error) }; throw error }
    }
    func save(_ artifact: ExportedCollection, to: URL, operation: CollectionExportOperation) async throws {
        try await CoreCollectionExportService().save(artifact, to: to, operation: operation)
    }
}

extension ReviewCollectionStartupTests {
    @Test func exportWithoutStudyAdmissionRetriesDestinationAndReimportsCapturedReview() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        var observations: [FieldDiagnosticObservation] = []
        configuration.recordDiagnostic = { observations.append($0) }
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        try #require(owner.state == .ready)
        let coordinator = owner.collectionExport
        do {
            // Export availability is separate from FSRS policy and study admission.
            #expect(coordinator.canPrepare)
            coordinator.present(); coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready, "\(coordinator.message ?? "missing Ready")")
            let artifact = try #require(coordinator.artifact)
            #expect(artifact.metadata.sourceGeneration == owner.generation && artifact.metadata.byteLength > 0)
            owner.navigation.adoptLaunchLayout(.compact); owner.navigation.adoptLaunchLayout(.sidebar)
            #expect(owner.collectionExport === coordinator)
            await coordinator.dismiss()
            #expect(coordinator.artifact === artifact)
            coordinator.present(); coordinator.chooseDestination(); await coordinator.waitForCompletion()
            try #require(coordinator.providerLease != nil && coordinator.destinationPresented)
            // Dismissal cannot prove the provider stopped reading.
            await coordinator.dismiss(); await coordinator.discard()
            #expect(coordinator.providerLease != nil && coordinator.artifact === artifact)
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .ready && coordinator.providerLease == nil)
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            coordinator.save(to: root.appending(path: "missing-parent/out.colpkg")); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .failed && coordinator.artifact === artifact && coordinator.canSave)
            let saved = root.appending(path: "saved.colpkg")
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            coordinator.save(to: saved); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .saved && files.fileExists(atPath: saved.path))
            #expect(observations.contains { $0.component == .collectionExport && $0.stage == .preparation && $0.outcome == .succeeded })
            #expect(observations.contains { $0.component == .collectionExport && $0.stage == .publication && $0.outcome == .succeeded })
            let location = CoreBridge.CollectionLocation(collectionDirectory: configuration.collectionLocation.directory,
                backupsDirectory: configuration.collectionLocation.backupsDirectory,
                appGroupSnapshot: root.appending(path: "widget"),
                manualTransferDirectory: root.appending(path: "synthetic-installation"))
            let receipt = try await CoreCollectionEntryService().inspect(source: saved, location: location,
                operation: CollectionEntryOperation(), progress: { _ in })
            #expect(receipt.facts.notes == 6 && receipt.facts.cards == 11 && receipt.facts.reviews == 6)
            try await receipt.discard()
            // Reuse the real RF2 replacement, with its safety backup.
            owner.entry.inspect(saved); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready)
            #expect(owner.generation != artifact.metadata.sourceGeneration)
            #expect(coordinator.artifact === artifact)
            #expect(coordinator.stage == .saved && !coordinator.canSave)
            await coordinator.discard()
            #expect(coordinator.artifact == nil && files.fileExists(atPath: saved.path))
            if owner.policy != .ok { await owner.enableFsrs() }
            let tree = try await CoreDeckService().treeWithCounts()
            func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
            let deck = try #require(flatten(tree.nodes).first { !$0.filtered && $0.counts.new + $0.counts.review + $0.counts.learning > 0 })
            guard case .started(let session, _, _, _) = try await Core.startSession(deckID: deck.id) else {
                Issue.record("Exported package must remain reviewable"); _ = await owner.close(); return
            }
            let head = try await Core.sessionHead(sessionID: session, count: 1)
            let item = try #require(head.items.first)
            _ = try await Core.answerSession(sessionID: session, cardID: item.cardId, rating: .easy,
                stateToken: item.stateToken, answeredAtMs: 1_894_701_600_000, elapsedMs: 123)
            try await Core.flushReviewForBackground()
            try await Core.endSession(sessionID: session)
            #expect(await owner.close())
        } catch { await coordinator.cancelAndDrain(); await coordinator.discard(); _ = await owner.close(); throw error }
    }
    @Test func filesPickerPreventsReplacementBeforeConfirmationAndPreservesLateSavedCompletion() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        let coordinator = owner.collectionExport
        do {
            try #require(owner.state == .ready)
            coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready)
            let artifact = try #require(coordinator.artifact)
            let generation = owner.generation
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            let lease = try #require(coordinator.providerLease)
            try #require(coordinator.destinationPresented)
            let entryStage = owner.entry.stage
            #expect(!owner.entry.canPresent)
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            #expect(owner.entry.stage == entryStage)
            #expect(owner.generation == generation)
            #expect(coordinator.destinationPresented && coordinator.providerLease === lease)
            #expect(!(await owner.close()))
            // A delayed native copy callback still owns its source after dismissal.
            await coordinator.dismiss(); await coordinator.discard()
            #expect(coordinator.providerLease === lease)
            let saved = root.appending(path: "files-copy.colpkg")
            try files.copyItem(at: lease.url, to: saved)
            // Deterministic late notification after publication, before delegate delivery.
            coordinator.generationChanged()
            coordinator.nativeSaved(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .saved && coordinator.providerLease == nil)
            #expect(files.fileExists(atPath: saved.path) && coordinator.artifact === artifact)
            coordinator.generationChanged()
            #expect(coordinator.stage == .saved, "replacement cannot retrospectively relabel a completed copy")
            await coordinator.discard()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    @Test(arguments: [0, 1, 2])
    func nativeCleanupFailureRetainsRetryAndSavedPublication(completion: Int) async throws {
        let saved = completion != 0
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        var observations: [FieldDiagnosticObservation] = []
        configuration.recordDiagnostic = { observations.append($0) }
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        let failNext = Mutex(true)
        let coordinator = CollectionExportCoordinator(owner: owner, service: CoreCollectionExportService(), recordDiagnostic: configuration.recordDiagnostic, finishLease: { lease in
            if failNext.withLock({ fail in let previous = fail; fail = false; return previous }) {
                throw BridgeError.unavailable(reason: "synthetic completion interruption")
            }
            try await lease.finish()
        })
        do {
            try #require(owner.state == .ready)
            coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready)
            coordinator.chooseDestination(); await coordinator.waitForCompletion()
            let lease = try #require(coordinator.providerLease)
            let destination = root.appending(path: "saved.colpkg")
            if completion == 1 {
                // The app-controlled save publishes bytes before cleanup is attempted.
                coordinator.save(to: destination)
            } else if completion == 2 {
                try files.copyItem(at: lease.url, to: destination)
                coordinator.nativeSaved()
            } else {
                coordinator.nativeCancelled()
            }
            await coordinator.waitForCompletion()
            #expect(coordinator.providerLease === lease && coordinator.canRetryCompletion)
            #expect(!coordinator.canSave && !coordinator.canPrepare)
            #expect(coordinator.message?.contains("Retry cleanup") == true)
            #expect(coordinator.stage == (saved ? .saved : .ready))
            #expect(files.fileExists(atPath: destination.path) == saved)
            let exportObservations = observations.filter { $0.component == .collectionExport }
            #expect(exportObservations.map(\.stage) == [.preparation, .publication, .cleanup])
            #expect(exportObservations.map(\.outcome) == [.succeeded, saved ? .succeeded : .cancelled, .cleanupPending])
            coordinator.chooseDestination()
            #expect(!coordinator.destinationPresented && coordinator.providerLease === lease)
            await coordinator.discard()
            #expect(coordinator.artifact != nil && coordinator.providerLease === lease)
            coordinator.retryCompletion(); await coordinator.waitForCompletion()
            #expect(coordinator.providerLease == nil && !coordinator.canRetryCompletion)
            #expect(coordinator.stage == (saved ? .saved : .ready) && coordinator.message == nil)
            #expect(files.fileExists(atPath: destination.path) == saved)
            await coordinator.discard()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            coordinator.retryCompletion(); await coordinator.waitForCompletion()
            await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    @Test func exportReportGivesBoundedSourceActionsAndPreservesOtherFailures() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        try #require(owner.state == .ready)
        let generic = "The collection could not be exported. Check available space and collection access, then try again. Unused prepared output from an earlier launch is recovered when you prepare again."
        let cases: [(BridgeError, String)] = [
            (.exportSourceChanged, "The media source changed during export. Start a fresh export after changes finish."),
            (.exportSourceInvalid, "A media item is unsafe or unreadable. Correct its name, type or read access, then start a fresh export."),
            (.exportFormatLimit, "The collection or media exceeds the package format limits. Reduce the size or number of media files, then start a fresh export."),
            (.cancelled, generic), (.insufficientSpace(required: 20, available: 1), generic),
            (.staleGeneration(planned: 1, current: 2), generic),
            (.internal("private path /synthetic/do-not-display and OS diagnostic"), generic),
            (.invalidArgument("destination diagnostic"), generic)
        ]
        for (cause, expected) in cases {
            let coordinator = CollectionExportCoordinator(owner: owner, service: RefusingExportService(cause: cause))
            coordinator.prepare(); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .failed && coordinator.message == expected)
            #expect(coordinator.artifact == nil && !coordinator.publicationUncertain)
        }
        // Actual report() precedence: destination and uncertain publication stay
        // actionable as destination problems even when a source cause is present.
        for uncertain in [false, true] {
            let coordinator = CollectionExportCoordinator(owner: owner, service: RefusingExportService(
                cause: .exportSourceInvalid, prepareReal: true, uncertain: uncertain))
            do {
                coordinator.prepare(); await coordinator.waitForCompletion()
                try #require(coordinator.stage == .ready)
                coordinator.chooseDestination(); await coordinator.waitForCompletion()
                coordinator.save(to: root.appending(path: "destination.colpkg")); await coordinator.waitForCompletion()
                #expect(coordinator.stage == .failed && coordinator.artifact != nil)
                #expect(coordinator.publicationUncertain == uncertain)
                #expect(coordinator.message == (uncertain
                    ? "The new file may already be saved. Check the destination before trying again. The completed export is retained."
                    : "The file could not be saved. The completed export is retained; choose another destination and try again."))
                await coordinator.discard()
            } catch {
                coordinator.nativeCancelled(); await coordinator.waitForCompletion()
                await coordinator.discard(); _ = await owner.close(); throw error
            }
        }
        #expect(await owner.close())
    }
    /// N17.3: one continuous export; a cancel or a finished save discards the prepared file.
    @Test func continuousExportOpensTheSaveDialogAndLeavesNoPreparedFile() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        try #require(owner.state == .ready)
        let coordinator = owner.collectionExport
        // Preparation, then the hand-off that acquires the provider's lease.
        func reachSaveDialog() async throws {
            coordinator.start()
            #expect(owner.navigation.sheet == .collectionExport && coordinator.continuous)
            await coordinator.waitForCompletion(); await coordinator.waitForCompletion()
            try #require(coordinator.destinationPresented && coordinator.providerLease != nil, "\(coordinator.message ?? "no save dialog")")
            #expect(coordinator.plainProgressTitle == "Saving…")
        }
        func expectFinished() {
            #expect(coordinator.stage == .idle && coordinator.artifact == nil && coordinator.providerLease == nil)
            #expect(!coordinator.continuous && owner.navigation.sheet == nil && coordinator.message == nil)
        }
        do {
            // Cancelling the save dialog discards the file and closes the sheet.
            try await reachSaveDialog()
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            expectFinished()
            // A finished save does the same.
            try await reachSaveDialog()
            let saved = root.appending(path: "saved.colpkg")
            coordinator.save(to: saved); await coordinator.waitForCompletion()
            #expect(files.fileExists(atPath: saved.path))
            expectFinished()
            // A failed save keeps the sheet, its message and the file for another try.
            try await reachSaveDialog()
            coordinator.save(to: root.appending(path: "missing-parent/out.colpkg")); await coordinator.waitForCompletion()
            #expect(coordinator.stage == .failed && coordinator.artifact != nil && coordinator.canChooseAgain)
            #expect(owner.navigation.sheet == .collectionExport && coordinator.message != nil)
            await coordinator.cancel()
            expectFinished()
            // Cancel during preparation opens no save dialog.
            coordinator.start()
            #expect(coordinator.plainProgressTitle == "Preparing Export…")
            await coordinator.cancel()
            #expect(!coordinator.destinationPresented)
            expectFinished()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            await coordinator.cancel(); await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    /// N17.3: no route leaves the provider's lease or the prepared file behind a closed sheet.
    @Test func continuousExportEndsCleanlyWhenCancelledOrItsSheetCloses() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        try #require(owner.state == .ready)
        let service = ObservedExportService()
        let coordinator = CollectionExportCoordinator(owner: owner, service: service)
        func expectReleased(_ name: String) {
            let route = Comment(rawValue: "\(name): \(coordinator.stage) \(service.lastError.withLock { $0 } ?? "no error")")
            #expect(coordinator.stage == .idle && coordinator.artifact == nil && coordinator.providerLease == nil, route)
            #expect(!coordinator.busy && !coordinator.destinationPresented && !coordinator.continuous, route)
            #expect(owner.navigation.sheet == nil && coordinator.canPresent, route)
        }
        do {
            // Cancel while the lease is being acquired, after preparation.
            coordinator.start(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .saving && !coordinator.destinationPresented, "\(coordinator.message ?? "not acquiring")")
            await coordinator.cancel()
            expectReleased("cancel during acquisition")
            // The sheet closes during preparation: no dialog opens without its host.
            coordinator.start()
            owner.navigation.dismissSheet()
            await coordinator.waitForCompletion(); await coordinator.waitForCompletion()
            expectReleased("sheet closed during preparation")
            // The sheet closes while the lease is being acquired. Core can still be
            // retiring the export it just discarded and refuse a new one as busy.
            for _ in 0..<50 {
                coordinator.start(); await coordinator.waitForCompletion()
                if coordinator.stage == .saving { break }
                await coordinator.cancel()
                try await Task.sleep(for: .milliseconds(100))
            }
            try #require(coordinator.stage == .saving && !coordinator.destinationPresented)
            owner.navigation.dismissSheet()
            await coordinator.waitForCompletion()
            expectReleased("sheet closed during acquisition")
            // A file chosen after another sheet took the window is not read.
            owner.navigation.present(.accountSync)
            owner.entry.selection(.success([source]))
            #expect(!owner.entry.busy && owner.entry.preview == nil && owner.navigation.sheet == .accountSync)
            owner.navigation.dismissSheet()
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            await coordinator.cancel(); await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
    @Test func noCollectionExplainsExportWithoutStartingWork() async throws {
        var configuration = LaunchConfiguration()
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let coordinator = owner.collectionExport
        #expect(!coordinator.canPrepare && coordinator.availabilityMessage != nil)
        #expect(!coordinator.canPresentDeck(1))
        coordinator.presentDeck(1)
        #expect(owner.navigation.sheet == nil)
        coordinator.prepare(); #expect(coordinator.stage == .idle)
        coordinator.generationChanged()
        #expect(coordinator.stage == .idle && coordinator.artifact == nil)
        #expect(await owner.close())
    }
}


extension ReviewCollectionStartupTests {
    @Test func selectedDeckExportSavesAPKGRemembersChoicesAndLeavesCollectionExportUnchanged() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let suite = "DeckExportTests-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        var configuration = LaunchConfiguration()
        configuration.deviceDefaults = defaults
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600)); configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
        try #require(owner.state == .ready)
        let coordinator = owner.collectionExport
        do {
            coordinator.presentDeck(1)
            #expect(coordinator.showsDeckOptions && coordinator.title == "Export Deck")
            #expect(coordinator.deckOptions == DeckExportOptions())
            coordinator.deckOptions = DeckExportOptions(includeScheduling: true, includePresets: true, includeMedia: false)
            coordinator.exportDeck()
            await coordinator.waitForCompletion(); await coordinator.waitForCompletion()
            try #require(coordinator.destinationPresented && coordinator.providerLease != nil, "\(coordinator.message ?? "missing deck save dialog")")
            let artifact = try #require(coordinator.artifact)
            #expect(artifact.metadata.suggestedName == "Default.apkg")
            let saved = root.appending(path: "selected.apkg")
            coordinator.save(to: saved); await coordinator.waitForCompletion()
            #expect(files.fileExists(atPath: saved.path))
            #expect(coordinator.stage == .idle && coordinator.artifact == nil && coordinator.providerLease == nil)
            #expect(owner.navigation.sheet == nil)
            coordinator.presentDeck(1)
            #expect(coordinator.deckOptions == DeckExportOptions(includeScheduling: true, includePresets: true, includeMedia: false))
            await coordinator.cancel()
            // The ordinary prepare entry must clear the closed deck options,
            // just as the continuous File/Settings command does below.
            coordinator.present(); coordinator.prepare(); await coordinator.waitForCompletion()
            try #require(coordinator.stage == .ready)
            #expect(!coordinator.isDeckExport)
            #expect(coordinator.artifact?.metadata.suggestedName == "Meguri-Collection.colpkg")
            await coordinator.cancel()
            coordinator.presentDeck(1); await coordinator.cancel()
            coordinator.start()
            await coordinator.waitForCompletion(); await coordinator.waitForCompletion()
            try #require(coordinator.destinationPresented)
            #expect(!coordinator.isDeckExport)
            #expect(coordinator.artifact?.metadata.suggestedName == "Meguri-Collection.colpkg")
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            #expect(owner.navigation.sheet == nil && coordinator.providerLease == nil)
            // A delayed bridge request retains the originating generation,
            // even when the replacement has exactly the same deck ID.
            let delayed = DeckExportRequest(deckID: 1, options: coordinator.deckOptions,
                                            expectedGeneration: owner.generation)
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready && owner.generation != delayed.expectedGeneration)
            try #require(owner.fullDeckName(1) == "Default")
            do {
                let unexpected = try await CoreCollectionExportService().prepare(deck: delayed,
                    operation: CollectionExportOperation(), progress: { _ in })
                try await unexpected.discard()
                Issue.record("An old deck action must refuse a replacement with the same deck ID")
            } catch let error as BridgeError {
                #expect(error == .staleGeneration(planned: delayed.expectedGeneration, current: owner.generation))
            }
            #expect(await owner.close())
        } catch {
            coordinator.nativeCancelled(); await coordinator.waitForCompletion()
            await coordinator.cancel(); await coordinator.discard(); _ = await owner.close(); throw error
        }
    }
}

@Suite @MainActor struct DeckExportPreferencesTests {
    @Test func firstUseAndRememberedChoicesAreDeviceLocal() throws {
        let suite = "DeckExportPreferencesTests-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let preferences = DeckExportPreferences(defaults: defaults)
        #expect(preferences.read() == DeckExportOptions())
        let changed = DeckExportOptions(includeScheduling: true, includePresets: true, includeMedia: false)
        preferences.write(changed)
        #expect(DeckExportPreferences(defaults: defaults).read() == changed)
        #expect(DeckExportPreferences(defaults: nil).read() == DeckExportOptions())
    }
}
