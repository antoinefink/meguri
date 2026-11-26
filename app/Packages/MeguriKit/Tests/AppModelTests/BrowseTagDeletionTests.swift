@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

@MainActor @Suite struct BrowseTagDeletionTests {
    @Test func coreScopeIsCapturedForMenusKeyboardAndRetryIndependentlyOfUIEpoch() async {
        let backend = TagDeletionBackend(), model = BrowseModel(backend: backend)
        backend.coreGeneration = 41
        model.generation = { 9001 }
        await model.refreshTags()
        let keyboardOrMenu = model.tagDeletion.target(["brask"])
        backend.perform = { throw BridgeError.unavailable(reason: "synthetic") }
        model.tagDeletion.delete(keyboardOrMenu)
        await model.tagDeletion.waitForCompletion()
        #expect(model.tagDeletion.failed == keyboardOrMenu)
        #expect(backend.expectedGenerations == [41])
        backend.coreGeneration = 42
        await model.refreshTags() // Same names and UI epoch.
        #expect(!model.tagDeletion.canDelete(keyboardOrMenu))
        model.tagDeletion.delete(keyboardOrMenu)
        await model.tagDeletion.waitForCompletion()
        #expect(backend.expectedGenerations == [41])
        backend.perform = {}
        model.tagDeletion.delete(model.tagDeletion.target(["brask"]))
        await model.tagDeletion.waitForCompletion()
        #expect(backend.expectedGenerations == [41, 42])
        #expect(model.tagDeletion.announcement?.count == 2)
    }

    @Test func absentReadScopeNeverDispatches() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        deletion.available = { true }; deletion.contains = { _ in true }
        deletion.delete(deletion.target(["brask"]))
        await deletion.waitForCompletion()
        #expect(backend.texts.isEmpty)
    }

    @Test(arguments: [false, true])
    func successfulSaveRevalidatesAnInvalidatedModelTree(duringSave: Bool) async {
        let backend = TagDeletionBackend(), model = BrowseModel(backend: backend)
        backend.coreGeneration = 41
        await model.refreshTags()
        let target = model.tagDeletion.target(["brask::oddly"])
        model.tagDeletion.saveFirst = {
            if duringSave { Self.invalidateTagTree(model) }
            #expect(model.tagDeletion.expectedGeneration() == nil)
            #expect(!model.hasTag("brask::oddly"))
            return true
        }
        model.tagDeletion.delete(target)
        if !duringSave { Self.invalidateTagTree(model) }
        await model.tagDeletion.waitForCompletion()
        #expect(backend.texts == ["brask::oddly"] && backend.expectedGenerations == [41])
        #expect(model.tagDeletion.announcement?.count == 2 && model.tagDeletion.failed == nil)
        #expect(!model.tagDeletion.busy)
    }

    @Test(arguments: [["brask", "brask::oddly"], ["brask", "Brask"], ["brask", "brask::odd"], ["brask", "missing"]])
    func authoritativeRevalidationRequiresEveryExactMember(names: [String]) async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        var treeGeneration: UInt64? = 41
        backend.coreGeneration = 41
        deletion.expectedGeneration = { treeGeneration }
        deletion.available = { true }
        deletion.contains = { _ in treeGeneration != nil }
        deletion.saveFirst = { treeGeneration = nil; return true }
        let target = deletion.target(names)
        deletion.delete(target)
        await deletion.waitForCompletion()
        let exists = names == ["brask", "brask::oddly"]
        #expect(backend.texts == (exists ? ["brask brask::oddly"] : []))
        #expect(deletion.failed == (exists ? nil : target))
        #expect((deletion.announcement != nil) == exists && !deletion.busy)
    }

    @Test func authoritativeReadFailureIsReportedAndCanBeRetried() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        var treeGeneration: UInt64? = 41
        backend.coreGeneration = 41
        deletion.expectedGeneration = { treeGeneration }
        deletion.available = { true }
        deletion.contains = { _ in treeGeneration != nil }
        deletion.saveFirst = { treeGeneration = nil; return true }
        backend.readTree = { throw BridgeError.unavailable(reason: "synthetic") }
        let target = deletion.target(["brask"])
        deletion.delete(target)
        await deletion.waitForCompletion()
        #expect(backend.texts.isEmpty && deletion.failed == target && !deletion.busy)
        #expect(deletion.announcement == nil)
        treeGeneration = 41 // The same collection's cache reload enables Retry.
        backend.readTree = nil
        #expect(deletion.canDelete(target))
        deletion.delete(target)
        await deletion.waitForCompletion()
        #expect(backend.texts == ["brask"] && backend.expectedGenerations == [41])
        #expect(deletion.failed == nil && deletion.announcement?.count == 2)
    }

    @Test(arguments: [false, true])
    func replacementDuringAuthoritativeReadNeverDispatches(observedReplacement: Bool) async {
        let backend = TagDeletionBackend(), model = BrowseModel(backend: backend)
        backend.coreGeneration = 41
        await model.refreshTags()
        let target = model.tagDeletion.target(["brask"])
        model.tagDeletion.saveFirst = {
            Self.invalidateTagTree(model)
            return true
        }
        backend.readTree = {
            backend.coreGeneration = 42 // Successor deliberately has identical names.
            if observedReplacement { model.collectionReplaced() }
            return backend.reads.tagNodes
        }
        model.tagDeletion.delete(target)
        await model.tagDeletion.waitForCompletion()
        #expect(backend.texts.isEmpty && backend.expectedGenerations.isEmpty)
        #expect(model.tagDeletion.announcement == nil && !model.tagDeletion.busy)
        #expect(model.tagDeletion.failed == (observedReplacement ? nil : target))
    }

    @Test func completionSurvivesTreeInvalidationAcrossTheWrite() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        backend.coreGeneration = 41
        var treeGeneration: UInt64? = 41
        deletion.generation = { 9001 }
        deletion.expectedGeneration = { treeGeneration }
        deletion.available = { true }
        deletion.contains = { _ in treeGeneration != nil }
        var steps: [String] = [], refreshes: [String] = []
        deletion.stepRecorded = { seq, title in
            #expect(seq == 1)
            steps.append(title)
        }
        deletion.finished = { result, title in
            #expect(treeGeneration == nil && result.summary.generation == 41)
            refreshes.append(title)
        }
        var undoRefreshed = false
        deletion.refreshUndo = { undoRefreshed = true }
        backend.perform = { treeGeneration = nil } // Change event precedes job completion.
        let target = deletion.target(["brask"])
        deletion.delete(target)
        await deletion.waitForCompletion()
        #expect(backend.texts == ["brask"] && backend.expectedGenerations == [41])
        #expect(steps == ["Delete Tag"] && refreshes == ["Delete Tag"] && undoRefreshed)
        #expect(deletion.announcement?.count == 2 && deletion.failed == nil && !deletion.busy)
        #expect(!deletion.canDelete(target), "new dispatch still requires a current tree")
    }

    @Test(arguments: [false, true])
    func treeInvalidationDoesNotHideSameCollectionErrors(duringSave: Bool) async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        var treeGeneration: UInt64? = 0
        deletion.expectedGeneration = { treeGeneration }
        deletion.available = { true }
        deletion.contains = { _ in true }
        if duringSave {
            deletion.saveFirst = { treeGeneration = nil; return false }
        } else {
            backend.perform = {
                treeGeneration = nil
                throw BridgeError.unavailable(reason: "synthetic")
            }
        }
        let target = deletion.target(["brask"])
        deletion.delete(target)
        await deletion.waitForCompletion()
        #expect(deletion.failed == target && deletion.announcement == nil && !deletion.busy)
        #expect(backend.texts.count == (duringSave ? 0 : 1))
        #expect(!deletion.canDelete(target))
    }

    @Test func mismatchedReceiptGenerationCannotDeliverSuccessAfterTreeInvalidation() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        var treeGeneration: UInt64? = 41
        deletion.expectedGeneration = { treeGeneration }
        deletion.available = { true }
        deletion.contains = { _ in true }
        var delivered = false
        deletion.stepRecorded = { _, _ in delivered = true }
        deletion.finished = { _, _ in delivered = true }
        deletion.refreshUndo = { delivered = true }
        backend.perform = { treeGeneration = nil; backend.coreGeneration = 42 }
        deletion.delete(deletion.target(["brask"]))
        await deletion.waitForCompletion()
        #expect(backend.expectedGenerations == [41])
        #expect(!delivered && deletion.announcement == nil && deletion.failed == nil && !deletion.busy)
    }

    @Test func forwardsLiteralFullNamesAndReportsTheCoreCountIncludingZero() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        deletion.expectedGeneration = { 0 }
        deletion.available = { true }
        deletion.contains = { _ in true }
        var steps: [(CountMutation, String)] = []
        deletion.finished = { steps.append(($0, $1)) }
        let target = deletion.target(["Ink::pen", "ink*", "c++"])
        deletion.delete(target)
        // Admission reserves busy synchronously, so a double action runs one job.
        deletion.delete(target)
        await deletion.waitForCompletion()
        #expect(backend.texts == ["Ink::pen ink* c++"])
        #expect(deletion.announcement?.count == 2)
        #expect(steps.first?.1 == "Delete Tags")
        backend.count = 0
        deletion.delete(deletion.target(["registry::only"]))
        await deletion.waitForCompletion()
        #expect(deletion.announcement?.count == 0)
        #expect(steps.last?.1 == "Delete Tag")
        #expect(steps.last?.0.summary.undoName == "Remove Tag", "zero notes is still a registry undo step")
    }

    @Test func refusesUnavailableMissingAndOldCollectionTargets() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        deletion.expectedGeneration = { 0 }
        deletion.available = { true }
        deletion.contains = { $0 == "Ink" }
        let old = deletion.target(["Ink"])
        deletion.available = { false }
        deletion.delete(old)
        deletion.available = { true }
        deletion.delete(deletion.target(["missing"]))
        deletion.reset()
        deletion.delete(old)
        deletion.delete(deletion.target([]))
        await deletion.waitForCompletion()
        #expect(backend.texts.isEmpty)
    }

    @Test func savesFirstAndDropsSuccessAndFailureFromAnOldCollection() async {
        for failing in [false, true] {
            let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
            deletion.expectedGeneration = { 0 }
            deletion.available = { true }
            deletion.contains = { _ in true }
            var saved = false, landed = false
            deletion.saveFirst = { saved = true; return true }
            deletion.finished = { _, _ in landed = true }
            backend.perform = {
                #expect(saved)
                deletion.reset()
                if failing { throw BridgeError.unavailable(reason: "synthetic") }
            }
            deletion.delete(deletion.target(["Ink"]))
            await deletion.waitForCompletion()
            #expect(!landed && deletion.announcement == nil && deletion.failed == nil && !deletion.busy)
        }
    }

    @Test func replacementDuringInspectorFlushStartsNoTagJob() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        deletion.expectedGeneration = { 0 }
        deletion.available = { true }
        deletion.contains = { _ in true }
        deletion.saveFirst = { deletion.reset(); return true }
        deletion.delete(deletion.target(["Ink"]))
        await deletion.waitForCompletion()
        #expect(backend.texts.isEmpty && !deletion.busy && deletion.failed == nil)
    }

    @Test func failedSaveDoesNotDeleteAndCoreFailureCanBeRetried() async {
        let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
        deletion.expectedGeneration = { 0 }
        deletion.available = { true }
        deletion.contains = { _ in true }
        deletion.saveFirst = { false }
        let target = deletion.target(["Ink"])
        deletion.delete(target); await deletion.waitForCompletion()
        #expect(backend.texts.isEmpty && deletion.failed == target)
        deletion.saveFirst = { true }
        backend.perform = { throw BridgeError.unavailable(reason: "synthetic") }
        deletion.delete(target); await deletion.waitForCompletion()
        #expect(deletion.failed == target && deletion.announcement == nil && !deletion.busy)
        deletion.dismissFailure()
        backend.perform = {}
        deletion.delete(target); await deletion.waitForCompletion()
        #expect(deletion.failed == nil && deletion.announcement?.count == 2)
    }

    @Test func settingsLoadsAndDeletesWithoutEverOpeningBrowse() async {
        let backend = TagDeletionBackend(), model = BrowseModel(backend: backend)
        #expect(!model.opened && !model.tagsLoaded)
        await model.refreshTags()
        #expect(model.tagsLoaded && model.hasTag("brask::oddly"))
        #expect(!model.opened && !model.results.hasRun && backend.reads.openCalls == 0)
        backend.perform = { backend.reads.tagNodes = [] }
        model.tagDeletion.delete(model.tagDeletion.target(["brask"]))
        await model.tagDeletion.waitForCompletion()
        #expect(backend.texts == ["brask"] && model.tagsLoaded && model.tags.isEmpty)
        #expect(!model.opened && !model.results.hasRun && model.tagDeletion.failed == nil)
    }

    @Test func tagReadFailureAndOldReadNeverPublishAsAnEmptyReadyTree() async throws {
        let backend = TagDeletionBackend(), model = BrowseModel(backend: backend)
        backend.readTree = { throw BridgeError.unavailable(reason: "synthetic") }
        await model.refreshTags()
        #expect(model.tagsFailed && !model.tagsLoaded && !model.tagsReading)
        var held: CheckedContinuation<[TagNode], Never>?
        backend.readTree = { await withCheckedContinuation { held = $0 } }
        let oldRead = Task { await model.refreshTags() }
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while held == nil, ContinuousClock.now < deadline { await Task.yield() }
        let release = try #require(held)
        let oldTree = backend.reads.tagNodes
        model.collectionReplaced()
        backend.readTree = { [] }
        await model.refreshTags()
        release.resume(returning: oldTree)
        await oldRead.value
        #expect(model.tagsLoaded && model.tags.isEmpty && !model.tagsFailed && !model.tagsReading)
        #expect(!model.opened && !model.results.hasRun)
    }

    @Test func dispatchAndCompletionRecheckReadinessAndGeneration() async {
        for duringSave in [true, false] {
            let backend = TagDeletionBackend(), deletion = BrowseTagDeletion(backend: backend)
            deletion.expectedGeneration = { 0 }
            var ready = true, generation: UInt64 = 0
            deletion.available = { ready }
            deletion.generation = { generation }
            deletion.contains = { _ in true }
            let target = deletion.target(["brask"])
            var landed = false
            deletion.finished = { _, _ in landed = true }
            if duringSave { deletion.saveFirst = { ready = false; return true } }
            else { backend.perform = { generation += 1 } }
            deletion.delete(target)
            await deletion.waitForCompletion()
            #expect(!landed && deletion.announcement == nil && !deletion.busy)
            #expect(backend.texts.count == (duringSave ? 0 : 1))
            if duringSave { #expect(deletion.failed == target) }
            else { #expect(deletion.failed == nil && !deletion.canDelete(target)) }
        }
    }

    private static func invalidateTagTree(_ model: BrowseModel) {
        // Hold the cache at transient nil instead of racing its eager reload.
        let ready = model.ready
        model.ready = { false }
        model.changesCommitted(ChangeSet(wholeCollection: false, source: .userUndoable,
            cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [],
            tags: [.init(name: "brask", change: .updated)], revlogAdded: [], revlogRemoved: [],
            mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
            skippedIncomingDecks: [], collectionRow: false), seq: 1)
        model.ready = ready
    }

}

/// Only the tag job is scripted. Other Browse reads use the existing pure fake.
@MainActor private final class TagDeletionBackend: BrowseBackend {
    let reads = FakeBrowseBackend()
    var texts: [String] = []
    var count: UInt64 = 2
    var coreGeneration: UInt64 = 0
    var expectedGenerations: [UInt64] = []
    func scopedTagTree() async throws -> CollectionScopedRead<[TagNode]> {
        .init(value: try await tagTree(), generation: coreGeneration)
    }
    func deleteTags(text: String, expectedGeneration: UInt64) async throws -> CountMutation {
        expectedGenerations.append(expectedGeneration)
        return try await deleteTags(text: text)
    }
    var perform: @MainActor () async throws -> Void = {}
    func deleteTags(text: String) async throws -> CountMutation {
        texts.append(text)
        try await perform()
        return CountMutation(summary: MutationSummary(seq: UInt64(texts.count), generation: coreGeneration, undoName: "Remove Tag",
            changes: ChangeSet(wholeCollection: false, source: .userUndoable,
                cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [],
                revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [],
                skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false), pastCancelPoint: true), count: count)
    }
    func opening() async throws -> BrowseOpening { try await reads.opening() }
    func read(_ text: String) async throws -> FilterQuery { try await reads.read(text) }
    func runText(_ query: FilterQuery) async throws -> String { try await reads.runText(query) }
    func commit(_ query: FilterQuery) async throws -> FilterQuery { query }
    func isIncomplete(_ text: String, caret: Int) async throws -> Bool { false }
    func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection?, recent: [String]) async throws -> [FilterSuggestion] { [] }
    func saveSearch(name: String, text: String) async throws {}
    func renameSavedSearch(_ old: String, to name: String) async throws {}
    func deleteSavedSearch(_ name: String) async throws {}
    func values(_ section: FilterSection) async throws -> [FilterSuggestionRow] { [] }
    func notetypes() async throws -> [NotetypeName] { [] }
    var readTree: (@MainActor () async throws -> [TagNode])?
    func tagTree() async throws -> [TagNode] {
        if let readTree { return try await readTree() }
        return reads.tagNodes
    }
    func flagNames() async throws -> [String] { [] }
    func counts(_ mode: BrowseMode, _ items: [FilterCountItem]) async throws -> [UInt64?] { items.map { _ in 0 } }
    func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws {}
    func openSearchSession() async throws -> any BrowseSearching { try await reads.openSearchSession() }
    func columnSet(notetype: Int64?) async throws -> ColumnSetShown { try await reads.columnSet(notetype: notetype) }
    func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws {}
    func useStandardColumns(notetype: Int64) async throws {}
    func setNotesMode(_ notes: Bool) async throws {}
}

// The existing serialized owner suite isolates the process-wide real core.
extension ReviewCollectionStartupTests {
    @Test func browseTagDeletionUsesTheCoreMatchingRegistryUndoAndActiveQuery() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var config = LaunchConfiguration()
        config.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        config.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        config.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: config, navigation: AppNavigation(),
                                     providerDirectory: root.appending(path: "synthetic-installation"))
        do {
            await owner.start()
            owner.entry.inspect(try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg")))
            await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            owner.navigation.dismissSheet()
            let notes = CoreNoteService(), tags = CoreTagService()
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic Tag Deletion", cloze: false,
                fields: ["Q", "A"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}", browserFront: "", browserBack: "")], css: nil))
            let first = try await notes.addNote(.init(notetypeId: type.id, fields: ["tag deletion first", "answer"],
                tags: ["Ink", "Ink::pen", "Ink::pen::nib", "inky", "other::Ink"], deckId: 1, guid: nil, updateAddDefaults: false))
            let second = try await notes.addNote(.init(notetypeId: type.id, fields: ["tag deletion second", "answer"],
                tags: ["Ink::pencil", "Ink:", "inky"], deckId: 1, guid: nil, updateAddDefaults: false))
            _ = try await tags.addNoteTags(notes: [], text: "Ink::zz Registry::only")
            let before = try await notes.readNote(id: first.id)
            let sibling = try await notes.readNote(id: second.id)
            let cards = try await notes.noteCards(noteId: first.id)
            let otherCards = try await notes.noteCards(noteId: second.id)
            let browse = owner.browse, undo = owner.collectionUndo
            // Settings is the first tag surface: no opening defaults, result
            // session or sidebar read has populated the model.
            await browse.refreshTags()
            await tagWait { browse.hasTag("Registry::only") }
            #expect(!browse.opened && !browse.results.hasRun && browse.hasTag("Registry::only"))
            let registry = browse.tagDeletion.target(["Registry"])
            browse.tagDeletion.delete(registry)
            #expect(browse.tagDeletion.busy && !undo.canUndo)
            undo.undo() // A pending deletion must not undo the preceding note edit.
            await browse.tagDeletion.waitForCompletion()
            await tagWait { browse.tagsLoaded && !browse.hasTag("Registry") && undo.canUndo && undo.undoName == "Delete Tag" }
            #expect(browse.tagDeletion.announcement?.count == 0 && browse.tagDeletion.failed == nil)
            undo.undo(); await undo.waitForCompletion()
            await tagWait { browse.hasTag("Registry::only") }
            #expect(!undo.failed && !browse.opened && !browse.results.hasRun)
            #expect(try await notes.readNote(id: first.id) == before)
            #expect(try await notes.noteCards(noteId: first.id) == cards)

            owner.navigation.select(.section(.browse)); browse.appear()
            await tagWait { browse.opened && browse.hasTag("Ink::zz") }
            let query = FilterQuery(tokens: [.value([.tag("Ink")], negated: false)])
            browse.setQuery(query)
            await tagWait { browse.results.list?.count == 2 }

            // Child deletion preserves its parent, equally-prefixed sibling,
            // unrelated tag path and every note and card.
            browse.tagDeletion.delete(browse.tagDeletion.target(["Ink::pen"]))
            await browse.tagDeletion.waitForCompletion()
            #expect(browse.tagDeletion.announcement?.count == 1)
            await tagWait { browse.tagsLoaded && !browse.hasTag("Ink::pen") && undo.undoName == "Delete Tag" && undo.canUndo }
            #expect(browse.hasTag("Ink") && browse.hasTag("Ink::pencil") && browse.hasTag("inky"))
            #expect(try await notes.readNote(id: first.id).tags == ["Ink", "inky", "other::Ink"])
            #expect(try await notes.readNote(id: second.id) == sibling)
            #expect(try await notes.noteCards(noteId: first.id) == cards)
            undo.undo(); await undo.waitForCompletion()
            await tagWait { browse.hasTag("Ink::pen::nib") }
            #expect(try await notes.readNote(id: first.id) == before)

            browse.tagDeletion.delete(browse.tagDeletion.target(["Ink"]))
            await browse.tagDeletion.waitForCompletion()
            #expect(browse.tagDeletion.announcement?.count == 2)
            await tagWait { browse.tagsLoaded && !browse.hasTag("Ink") && browse.results.list?.count == 0 && undo.canUndo && undo.undoName == "Delete Tag" }
            #expect(browse.query == query, "the active tag filter survives; its search now finds nothing")
            #expect(browse.hasTag("inky") && !browse.hasTag("Ink::zz"))
            #expect(try await notes.readNote(id: first.id).tags == ["inky", "other::Ink"])
            #expect(try await notes.readNote(id: second.id).tags == ["Ink:", "inky"])
            #expect(try await notes.noteCards(noteId: first.id) == cards)
            #expect(try await notes.noteCards(noteId: second.id) == otherCards)
            undo.undo(); await undo.waitForCompletion()
            await tagWait { browse.hasTag("Ink::zz") && browse.results.list?.count == 2 }
            #expect(try await notes.readNote(id: first.id) == before)
            #expect(try await notes.readNote(id: second.id) == sibling)

            browse.tagDeletion.delete(browse.tagDeletion.target(["Registry"]))
            await browse.tagDeletion.waitForCompletion()
            #expect(browse.tagDeletion.announcement?.count == 0)
            await tagWait { browse.tagsLoaded && !browse.hasTag("Registry") && undo.undoName == "Delete Tag" && undo.canUndo }
            undo.undo(); await undo.waitForCompletion()
            await tagWait { browse.hasTag("Registry::only") }
            #expect(try await notes.noteCards(noteId: first.id) == cards)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }

    private func tagWait(_ condition: @MainActor () -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
        let end = ContinuousClock.now.advanced(by: .seconds(10))
        while !condition(), ContinuousClock.now < end { try? await Task.sleep(for: .milliseconds(10)) }
        #expect(condition(), sourceLocation: sourceLocation)
    }
}
