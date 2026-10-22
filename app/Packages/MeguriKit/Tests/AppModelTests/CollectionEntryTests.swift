import CardRendering
import CoreBridge
import Foundation
import Testing
@testable import AppModel

// These tests share the production host with the existing serialized startup
// suite. No launch hook creates the destination; it is absent at start().
extension ReviewCollectionStartupTests {
    @Test func missingDirectoryImportRetainsHostReviewsSavesAndReopens() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let directory = root.appending(path: "Application Support/Collection")
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        let original = try Data(contentsOf: source)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let display = EntryReviewDisplay()
        owner.makeReviewSurface = { _ in display }
        #expect(!files.fileExists(atPath: directory.path))
        await owner.start()
        try #require(owner.state == .noFile)
        #expect(files.fileExists(atPath: directory.path))
        #expect(!files.fileExists(atPath: configuration.collectionLocation.backupsDirectory.path))
        let entry = owner.entry
        entry.present(); entry.inspect(source)
        await entry.waitForCompletion()
        let facts = try #require(entry.preview?.facts, "\(entry.message ?? "No preview")")
        #expect(facts.sourceSchema == 18 && facts.notes == 6 && facts.cards == 11 && facts.reviews == 6)
        #expect(!facts.replacingExisting)
        // Changing the shell layout retains this pending inspection and its owner.
        let preview = entry.preview
        owner.navigation.adoptLaunchLayout(.compact)
        owner.navigation.adoptLaunchLayout(.sidebar)
        #expect(owner.entry === entry && entry.preview === preview)
        do {
            entry.confirm(); await entry.waitForCompletion()
            try #require(owner.state == .ready, "\(owner.error ?? entry.message ?? "Host not ready")")
            #expect(owner.error == nil)
            #expect(!owner.decks.isEmpty && entry.preview == nil)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: owner.generation))
            if owner.policy != .ok { await owner.enableFsrs() }
            func flattened(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flattened($0.children) } }
            let deck = try #require(flattened(owner.decks).first { !$0.filtered && ($0.counts.new + $0.counts.review + $0.counts.learning) > 0 })
            await owner.startStudy(deckID: deck.id)
            let review = try #require(owner.review, "\(owner.error ?? "No real review")")
            try #require(review.phase == .question, "\(review.notice ?? String(describing: review.phase))")
            let current = try #require(review.item)
            let rendered = try #require(display.rendered.first { $0.cardID == current.cardId })
            #expect(rendered.mode == .review && rendered.outcome == .rendered)
            #expect(rendered.question?.isEmpty == false && rendered.answer?.isEmpty == false)
            #expect(display.presented.last == current.cardId)
            #expect(!entry.canPresent)
            review.reveal(); await review.settled()
            try #require(review.phase == .answer)
            #expect(display.revealed == [current.cardId])
            review.rate(.easy); await review.settled()
            try #require(review.record.answers.count == 1)
            let answered = try #require(review.record.answers.first?.cardID)
            owner.setForeground(false); await owner.waitForLifecycle()
            owner.setForeground(true); await owner.waitForLifecycle()
            await owner.finishReview()
            #expect(owner.previousRecord?.answers.map(\.cardID) == [answered])
            #expect(await owner.close())
            let restarted = ReviewCollection(configuration: configuration, navigation: AppNavigation())
            await restarted.start()
            try #require(restarted.state == .ready, "\(restarted.error ?? "Reopen failed")")
            #expect(restarted.previousRecord?.answers.map(\.cardID) == [answered])
            #expect(restarted.previousRecord?.answers.allSatisfy(\.durable) == true)
            #expect(restarted.error == nil)
            // Replace the reviewed collection through the same real service.
            // The actual safety package must retain the committed answer.
            restarted.entry.inspect(source); await restarted.entry.waitForCompletion()
            try #require(restarted.entry.preview?.facts.replacingExisting == true)
            restarted.entry.confirm(); await restarted.entry.waitForCompletion()
            try #require(restarted.state == .ready)
            #expect(restarted.previousRecord == nil)
            let receipt = try #require(restarted.entry.lastActivation?.backup)
            let backup = configuration.collectionLocation.backupsDirectory.appending(path: receipt.name)
            #expect(files.fileExists(atPath: backup.path))
            let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
                backupsDirectory: configuration.collectionLocation.backupsDirectory,
                appGroupSnapshot: directory.appending(path: "meguri-widget-unavailable.json"))
            let protected = try await CoreCollectionEntryService().inspect(source: backup, location: location,
                operation: CollectionEntryOperation(), progress: { _ in })
            #expect(protected.facts.reviews == 7, "The pre-import backup must include the one newly committed answer")
            try await protected.discard()
            #expect(await restarted.close())
            #expect(try Data(contentsOf: source) == original)
        } catch { _ = await owner.close(); try? await CoreCollectionService().close(); throw error }
    }

    @Test func cancelledMalformedAndStaleEntriesKeepRealOldCollection() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let directory = root.appending(path: "Collection")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        try files.copyItem(at: #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2")),
                           to: directory.appending(path: "collection.anki2"))
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        try #require(owner.state == .ready)
        let generation = owner.generation
        do {
            let bad = root.appending(path: "malformed.colpkg")
            try Data("public malformed package".utf8).write(to: bad)
            owner.entry.inspect(bad); await owner.entry.waitForCompletion()
            #expect(owner.entry.stage == .refused && owner.entry.preview == nil)
            #expect(owner.state == .ready && owner.generation == generation)
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            #expect(owner.entry.preview?.facts.replacingExisting == true)
            await owner.entry.dismiss()
            #expect(owner.entry.preview == nil && owner.generation == generation)
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            try #require(owner.entry.preview != nil)
            _ = try await CoreDeckService().addDeck(name: "Committed after preview")
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            #expect(owner.entry.failure?.issue == .staleConfirmation)
            #expect(owner.entry.preview == nil && owner.entry.needsReinspection)
            #expect(owner.state == .ready && owner.generation == generation)
            let tree = try await CoreDeckService().treeWithCounts()
            #expect(tree.nodes.contains { $0.name == "Committed after preview" })
            if owner.policy != .ok { await owner.enableFsrs() }
            let deck = try #require(tree.nodes.first)
            // Start a real backend session without populating the UI advisory
            // owner, so the exclusive-entry guard itself must refuse import.
            guard case .started(let session, _, _, _) = try await Core.startSession(deckID: deck.id) else {
                Issue.record("Expected a real session for authoritative refusal"); _ = await owner.close(); return
            }
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            try #require(owner.entry.preview != nil)
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            #expect(owner.entry.failure?.issue == .core(.sessionActive))
            #expect(owner.generation == generation)
            try await Core.endSession(sessionID: session)
            #expect(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }
    @Test func zeroCapacityRefusesCopyAndDoesNotCreateBackupParents() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            entryService: CoreCollectionEntryService(diskSpace: EntryZeroCapacity()))
        await owner.start()
        try #require(owner.state == .noFile)
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        owner.entry.inspect(source); await owner.entry.waitForCompletion()
        guard let failure = owner.entry.failure, case .package(.insufficientSpace) = failure.issue else {
            Issue.record("Expected typed combined-preflight refusal: \(owner.entry.message ?? "missing error")")
            _ = await owner.close(); return
        }
        #expect(owner.state == .noFile && owner.entry.preview == nil)
        #expect(!files.fileExists(atPath: configuration.collectionLocation.collectionFile.path))
        #expect(!files.fileExists(atPath: configuration.collectionLocation.backupsDirectory.path))
        #expect(try files.contentsOfDirectory(atPath: configuration.collectionLocation.directory.path).isEmpty)
        #expect(await owner.close())
    }

}

@MainActor @Suite struct CollectionEntryExplanationTests {
    @Test(arguments: [ImportIssue.staleConfirmation, .core(.sessionActive), .package(.unsupportedMedia),
                      .package(.corrupt("bad")), .package(.insufficientSpace(needed: 20, available: 1)),
                      .safetyBackup(cause: .core(.unavailable(reason: "provider")), publication: .unpublished)])
    func errorsAreActionableAndActiveFailureNeverPromisesRollback(issue: ImportIssue) {
        let before = CollectionEntryCoordinator.explanation(.init(issue: issue), fallback: "")
        #expect(!before.isEmpty)
        let after = CollectionEntryCoordinator.explanation(.init(issue: issue, candidateActive: true), fallback: "")
        #expect(after.contains("new collection remains active"))
        #expect(after.contains("recovery"))
        #expect(!after.contains("previous collection is still"))
    }
    @Test func customLocationsKeepBackupPathsIsolatedAndProductionUsesPlatformPolicy() {
        let root = URL(filePath: "/tmp/entry-location")
        let custom = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        #expect(custom.backupsDirectory == root.appending(path: "Backups", directoryHint: .isDirectory))
        #if os(iOS)
        #expect(AppModel.CollectionLocation.standard.backupsDirectory == URL.documentsDirectory.appending(path: "Backups", directoryHint: .isDirectory))
        #else
        #expect(AppModel.CollectionLocation.standard.backupsDirectory == URL.applicationSupportDirectory.appending(path: "Backups", directoryHint: .isDirectory))
        #endif
    }
}

private nonisolated struct EntryZeroCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { 0 }
}

// Model coverage needs core-rendered content, without an unhosted WebKit page.
// CollectionEntryJourneyTests exercises the actual display in the VM.
private final class EntryReviewDisplay: ReviewCardSurface {
    var pair: CardPagePair? { nil }
    var onAction: ((CardBridgeAction) -> Void)?
    var onRecovery: (() -> Void)?
    var editing: Bool { false }
    private(set) var rendered: [RenderedCard] = []
    private(set) var presented: [Int64] = []
    private(set) var revealed: [Int64] = []
    private var cards: [ReviewRenderKey: RenderedCard] = [:]
    private var visible: RenderedCard?

    func load() async throws {}
    func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws {
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        let values = try await Core.renderSessionHead(sessionID: sessionID, keys: items.map(\.renderKey), platform: platform)
        guard values.count == items.count else { throw BridgeError.internal("Entry display received an incomplete render") }
        for (item, card) in zip(items, values) { cards[item.renderKey] = card }
        rendered.append(contentsOf: values)
    }
    func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {
        guard let card = cards[item.renderKey], card.cardID == item.cardId else {
            throw BridgeError.internal("Entry display has no matching core render")
        }
        visible = card
        presented.append(card.cardID)
    }
    func preload(item: ReviewHeadItem) async throws {}
    func reveal(inputTime: TimeInterval) async throws {
        guard let visible else { throw BridgeError.internal("Entry display has no presented card") }
        revealed.append(visible.cardID)
    }
    func suspend() async {}
    func resume() async {}
    func close() { visible = nil; cards.removeAll() }
}
