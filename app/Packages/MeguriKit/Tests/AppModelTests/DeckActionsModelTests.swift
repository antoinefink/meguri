import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct DeckActionsModelTests {
    @Test func mainHostFullPathsBlankAndFilteredAdmission() async {
        let owner = await makeOwner(), model = owner.deckActions
        model.present(.create, host: .settings)
        #expect(owner.navigation.sheet == nil)
        model.present(.rename(2))
        #expect(model.name == "Child" && model.parentID == 1 && model.composedName == "Parent::Child")
        #expect(owner.navigation.sheetHost == .main)
        #expect(!model.canPresent(.create))
        // A layout rereads the same owner, rather than making a fresh draft.
        model.name = "Edited draft"
        owner.navigation.adoptLaunchLayout(.compact)
        #expect(owner.deckActions === model)
        #expect(model.name == "Edited draft" && model.composedName == "Parent::Edited draft")
        owner.navigation.sheetHost = .settings
        #expect(!model.canSubmit)
        owner.navigation.sheetHost = .main
        model.cancel()
        model.present(.subdeck(2))
        #expect(model.name.isEmpty && model.parentID == 2)
        model.name = " \n\t "
        #expect(!model.canSubmit)
        model.cancel()
        #expect(model.canPresent(.rename(3)))
        #expect(!model.canPresent(.subdeck(3)))
        #expect(!model.canPresent(.rename(999)))
        owner.entry.pickerPresented = true
        #expect(!model.canPresent(.create))
        owner.entry.pickerPresented = false
        owner.navigation.present(.collectionExport)
        #expect(!model.canPresent(.create))
    }

    @Test func duplicateSubmitIsReservedSynchronouslyAndRefusalRetainsDraft() async {
        let owner = await makeOwner(), model = owner.deckActions, gate = DeckNameGate()
        var calls = 0
        model.createDeck = { name in
            calls += 1
            #expect(name == "Parent::Child::Typed")
            await gate.hold()
            throw BridgeError.deckRefused(.filteredParent)
        }
        model.present(.subdeck(2)); model.name = "Typed"
        model.submit(); model.submit()
        #expect(model.busy)
        #expect(!owner.canStudy && !owner.exportAllowed && !owner.manualAllowed)
        await gate.entered()
        model.submit()
        #expect(calls == 1)
        gate.release(); await model.waitForCompletion()
        #expect(!model.busy)
        #expect(model.name == "Typed" && model.composedName == "Parent::Child::Typed")
        #expect(model.message == DeckActionsModel.explanation(.deckRefused(.filteredParent)))
        #expect(model.canSubmit)
    }

    @Test func staleGenerationAndWrongHostCannotPublishOrDispatch() async {
        let owner = await makeOwner(), model = owner.deckActions, gate = DeckNameGate()
        var calls = 0
        model.createDeck = { _ in calls += 1; await gate.hold(); throw BridgeError.internal("diagnostic") }
        model.present(.create); model.name = "Kept"
        model.submit(); await gate.entered()
        owner.receiveLifecycle(.ready(generation: 12))
        gate.release(); await model.waitForCompletion()
        #expect(model.message == nil && model.finalName == nil)
        #expect(owner.navigation.sheet == nil)
        #expect(calls == 1)
        model.present(.create); model.name = "Second"
        owner.navigation.sheetHost = .settings
        model.submit()
        #expect(!model.busy && calls == 1)
    }

    @Test func dispatchRechecksRouteAndTargetAfterSynchronousReservation() async {
        let owner = await makeOwner(), model = owner.deckActions
        var calls = 0
        model.createDeck = { _ in calls += 1; throw BridgeError.cancelled }
        model.present(.subdeck(2)); model.name = "New"
        model.submit()
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: []) }
        // A conflicting route wins even if the queued task has not run yet.
        owner.navigation.dismissSheet()
        await model.waitForCompletion()
        #expect(calls == 0)
        await owner.loadCounts()
        #expect(!model.canPresent(.subdeck(2)))
        owner.navigation.present(.deckName(.subdeck(2)))
        model.submit()
        #expect(!model.busy && calls == 0)
    }

    @Test(arguments: [false, true]) func anyOpenReviewBlocksNames(finished: Bool) async {
        let owner = await makeOwner(), model = owner.deckActions
        let fixture = ReviewCoordinatorTests.Fixture()
        if finished {
            fixture.client.currentHead = ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0),
                validUntil: Int64.max, terminalState: .finished)
        }
        await fixture.model.start()
        #expect(fixture.model.phase == (finished ? .finished : .question))
        model.present(.create); model.name = "Blocked"
        var calls = 0
        model.createDeck = { _ in calls += 1; throw BridgeError.cancelled }
        owner.presentReview(fixture.model)
        #expect(!model.canPresent(.create) && !model.canSubmit)
        model.submit()
        #expect(calls == 0)
        model.cancel()
        #expect(!model.canPresent(.create))
        _ = await fixture.model.end()
    }

    @Test func curatedErrorsNeverContainDiagnosticText() {
        for error in [BridgeError.internal("sensitive diagnostic"), .invalidArgument("sensitive diagnostic"),
                      .deckRefused(.underDescendant), .deckRefused(.tooDeep), .cancelled] {
            #expect(!DeckActionsModel.explanation(error).contains("sensitive"))
        }
    }

    private func makeOwner() async -> ReviewCollection {
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let counts = ReviewCounts(new: 0, learning: 0, review: 0)
        let child = CountedDeckNode(id: 2, name: "Child", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        let parent = CountedDeckNode(id: 1, name: "Parent", filtered: false, studyCollapsed: false,
            browserCollapsed: true, counts: counts, children: [child])
        let filtered = CountedDeckNode(id: 3, name: "Filtered", filtered: true, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: [parent, filtered]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}

@MainActor
private final class DeckNameGate {
    var layout: ShellLayout = .sidebar
    private var didEnter = false
    private var entry: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    func entered() async {
        if didEnter { return }
        await withCheckedContinuation { entry = $0 }
    }
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; didEnter = true; entry?.resume(); entry = nil
        }
    }
    func release() { continuation?.resume(); continuation = nil }
}

// Real U28 API coverage shares the existing serialized owner of the global stream.
extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func hiddenSelectedDefaultAdmitsRenameAndSubdeckWithFreshStoredPaths(subdeck: Bool) async throws {
        try await withHiddenDefaultOwner { owner in
            let service = CoreDeckService(), model = owner.deckActions
            let treeBefore = owner.decks
            #expect(owner.fullDeckName(1) == "Default")
            #expect(model.canPresent(.rename(1)) && model.canPresent(.subdeck(1)))
            #expect(!model.canPresent(.rename(999)) && !model.canPresent(.subdeck(999)))
            model.present(subdeck ? .subdeck(1) : .rename(1))
            #expect(model.name == (subdeck ? "" : "Default") && model.parentID == (subdeck ? 1 : nil))
            #expect(owner.decks == treeBefore && owner.deck(1) == nil)
            model.name = subdeck ? "QZ81" : "QZ82::QZ83"
            let saved = model.composedName
            #expect(saved == (subdeck ? "Default::QZ81" : "QZ82::QZ83"))
            model.submit(); await model.waitForCompletion()
            #expect(model.message == nil && !model.needsRefresh)
            let selectedID = try #require(owner.navigation.homeDeckPage?.rawValue)
            #expect(subdeck ? selectedID != 1 : selectedID == 1)
            #expect(owner.fullDeckName(selectedID) == saved)
            let node = try #require(owner.deck(selectedID))
            #expect(owner.deckPage(selectedID)?.name == node.name)
            #expect(owner.deckPage(selectedID)?.counts == node.counts)
            let tree = try await service.treeWithCounts()
            #expect(owner.decks == tree.nodes)
            let selected = try await service.selectDeck(id: selectedID)
            #expect(selected.changes.configKeys.isEmpty)
            model.cancel()
            if !subdeck {
                // Returning the same ID to its hidden name replaces the visible
                // page detail; later drafts must use the new authoritative name.
                model.present(.rename(1))
                #expect(model.name == "QZ83" && model.parent?.fullName == "QZ82")
                model.parentID = nil; model.name = "Default"; model.submit(); await model.waitForCompletion()
                #expect(model.message == nil && !model.needsRefresh)
                #expect(owner.navigation.homeDeckPage == DeckID(1))
                #expect(owner.deck(1) == nil && owner.deckPage(1)?.name == "Default")
                #expect(owner.fullDeckName(1) == "Default")
                let refreshed = try await service.treeWithCounts()
                #expect(owner.decks == refreshed.nodes)
                model.cancel(); model.present(.subdeck(1))
                #expect(model.name.isEmpty && model.parentID == 1 && model.parent?.fullName == "Default")
                model.name = "QZ84"
                #expect(model.composedName == "Default::QZ84" && model.canSubmit)
                model.cancel()
            }
        }
    }

    @Test(arguments: [false, true])
    func hiddenDetailCannotBeReusedAfterNavigationOrCollectionClose(closing: Bool) async throws {
        try await withHiddenDefaultOwner { owner in
            let model = owner.deckActions
            if closing {
                #expect(await owner.close())
                await owner.start()
            } else {
                owner.navigation.openHome()
            }
            // A navigation ID alone cannot restore authority from an old detail.
            owner.navigation.openDeckPage(DeckID(1))
            #expect(owner.deckPage(1) == nil && owner.fullDeckName(1) == nil)
            #expect(!model.canPresent(.rename(1)) && !model.canPresent(.subdeck(1)))
            try #require(await owner.openDeckPage(deckID: 1).value)
            #expect(owner.deckPage(1) == nil)
            #expect(!model.canPresent(.rename(1)) && !model.canPresent(.subdeck(1)))
        }
    }

    @Test(arguments: [false, true])
    func caseInsensitiveCreateOpensHiddenEmptyDefaultAndRetriesSelectionOnly(failsFirst: Bool) async throws {
        try await withDeckNameOwner { owner in
            let service = CoreDeckService(), model = owner.deckActions
            // Make the fixture's Default empty through the public deletion API.
            let deletion = try await service.startDeleteDecks(ids: [1], confirmed: try await service.deckDeletionSummary(ids: [1]))
            _ = try await deletion.value()
            let other = try await service.addDeck(name: "Other Root")
            await owner.loadCounts()
            try #require(owner.deck(1) == nil)
            try #require(try await service.studyState(deckID: 1).empty)
            try #require(await owner.openDeckPage(deckID: other.id).value)
            let previousPage = owner.deckPage(other.id), treeBefore = owner.decks
            var mutations: [DeckMutation] = [], selections: [Int64] = []
            model.createDeck = { name in
                let result = try await service.addDeck(name: name)
                mutations.append(result)
                return result
            }
            owner.saveSelectedDeck = { id in
                selections.append(id)
                if failsFirst && selections.count == 1 { throw BridgeError.internal("selection diagnostic") }
                return try await service.selectDeck(id: id).generation
            }
            model.present(.create); model.name = "dEfAuLt"
            model.submit(); await model.waitForCompletion()
            #expect(mutations.count == 1)
            #expect(mutations.first?.id == 1)
            #expect(model.finalName == "Default")
            if failsFirst {
                #expect(owner.navigation.homeDeckPage == DeckID(other.id))
                #expect(owner.deckPage(other.id) == previousPage)
                #expect(model.needsRefresh && model.needsOpen && !model.canSubmit)
                #expect(model.message == "The deck was saved. Couldn’t open it. Try again.")
                // Recovery must not issue even another tree query, let alone a write.
                owner.loadDeckTree = { throw BridgeError.internal("unexpected retry query") }
                model.submit(); model.resumeResult(); model.resumeResult()
                await model.waitForCompletion()
                #expect(selections == [1, 1])
                #expect(mutations.count == 1)
            } else {
                #expect(selections == [1])
            }
            #expect(owner.navigation.homeDeckPage == DeckID(1))
            #expect(owner.deckPage(1)?.id == 1)
            #expect(owner.deckPage(1)?.name == "Default")
            #expect(owner.deckPage(1)?.counts == ReviewCounts(new: 0, learning: 0, review: 0))
            #expect(owner.decks == treeBefore)
            #expect(owner.deck(1) == nil)
            #expect(model.message == nil && !model.needsRefresh && !model.needsOpen)
            let selected = try await service.selectDeck(id: 1)
            #expect(selected.changes.configKeys.isEmpty)
            // Closing the sheet must leave the hidden ID's page renderable.
            model.cancel()
            #expect(owner.deckPage(1)?.id == 1)
        }
    }

    @Test(arguments: [false, true])
    func committedCreateOrSubdeckRetriesFailedSelectionWithoutRepeatingMutation(subdeck: Bool) async throws {
        try await withDeckNameOwner { owner in
            let service = CoreDeckService(), model = owner.deckActions
            let parent = try await service.addDeck(name: "Selection Parent")
            await owner.loadCounts()
            try #require(await owner.openDeckPage(deckID: parent.id).value)
            let previousPage = owner.deckPage(parent.id)
            var mutations: [DeckMutation] = [], selections: [Int64] = []
            model.createDeck = { name in
                let result = try await service.addDeck(name: name)
                mutations.append(result)
                return result
            }
            owner.saveSelectedDeck = { id in
                selections.append(id)
                if selections.count == 1 { throw BridgeError.internal("selection diagnostic") }
                return try await service.selectDeck(id: id).generation
            }
            model.present(subdeck ? .subdeck(parent.id) : .create)
            model.name = subdeck ? "Child" : "Selection Target"
            model.submit(); await model.waitForCompletion()
            let committed = try #require(mutations.first)
            #expect(mutations.count == 1 && selections == [committed.id])
            #expect(owner.navigation.homeDeckPage == DeckID(parent.id))
            #expect(owner.deckPage(parent.id) == previousPage)
            #expect(model.finalName == committed.name)
            #expect(model.message == "The deck was saved. Couldn’t open it. Try again.")
            #expect(model.needsRefresh && model.needsOpen && !model.canSubmit)
            owner.loadDeckTree = { throw BridgeError.internal("unexpected retry query") }
            model.submit(); model.resumeResult(); model.resumeResult()
            await model.waitForCompletion()
            #expect(mutations.count == 1 && selections == [committed.id, committed.id])
            #expect(owner.navigation.homeDeckPage == DeckID(committed.id))
            #expect(owner.deckPage(committed.id)?.id == committed.id)
            #expect(model.message == nil && !model.needsRefresh && !model.needsOpen)
        }
    }

    @Test func nativeDeckNamesUseRealReturnedIdentityPathsCollisionsAndRefusals() async throws {
        try await withDeckNameOwner { owner in
            let model = owner.deckActions
            model.present(.create); model.name = "Native Parent::Child"
            model.submit(); model.submit(); await model.waitForCompletion()
            let childID = try #require(owner.navigation.homeDeckPage?.rawValue)
            #expect(owner.fullDeckName(childID) == "Native Parent::Child")
            #expect(model.finalName == "Native Parent::Child")
            model.cancel()
            model.present(.create); model.name = "native parent::child"
            model.submit(); await model.waitForCompletion()
            #expect(owner.navigation.homeDeckPage?.rawValue == childID)
            #expect(model.finalName == "Native Parent::Child")
            model.cancel()
            let parentID = try #require(owner.decks.first { $0.name == "Native Parent" }?.id)
            model.present(.rename(parentID)); model.name = "Native Renamed"
            model.submit(); await model.waitForCompletion()
            // N20.3: Rename shows the renamed deck's page.
            #expect(owner.navigation.homeDeckPage?.rawValue == parentID)
            #expect(owner.fullDeckName(childID) == "Native Renamed::Child")
            model.cancel()
            model.present(.subdeck(parentID))
            #expect(model.name.isEmpty && model.parentID == parentID)
            model.name = "Sibling"; model.submit(); await model.waitForCompletion(); model.cancel()
            model.present(.rename(childID))
            #expect(model.name == "Child" && model.parentID == parentID && model.footer == .inside("Native Renamed"))
            model.name = "Sibling"
            model.submit(); await model.waitForCompletion()
            #expect(model.finalName == "Native Renamed::Sibling+")
            #expect(owner.fullDeckName(childID) == model.finalName)
            model.cancel()
            model.present(.rename(parentID)); model.name = "Native Renamed::Sibling+::Inside"
            model.submit(); await model.waitForCompletion()
            #expect(model.finalName == nil)
            #expect(model.message == DeckActionsModel.explanation(.deckRefused(.underDescendant)))
            #expect(model.name == "Native Renamed::Sibling+::Inside")
        }
    }

    /// D19.7 with the core: a Parent Deck changed to None moves the subtree in
    /// one Rename Deck step, and a trailing space writes nothing.
    @Test func parentDeckNoneMovesTheSubtreeInOneStepAndATrailingSpaceWritesNothing() async throws {
        try await withDeckNameOwner { owner in
            let service = CoreDeckService(), model = owner.deckActions, undo = CoreUndoService()
            let first = try await service.addDeck(name: "Languages::日本語::QZ61")
            let second = try await service.addDeck(name: "Languages::日本語::QZ62")
            await owner.loadCounts()
            let languages = try #require(owner.decks.first { $0.name == "Languages" })
            let japanese = try #require(languages.children.first { $0.name == "日本語" })
            model.present(.rename(japanese.id))
            #expect(model.name == "日本語" && model.parentID == languages.id && model.footer == .subdecksMove(2))
            model.parentID = nil
            #expect(model.composedName == "日本語" && model.canSubmit)
            model.submit(); await model.waitForCompletion()
            #expect(model.message == nil && owner.navigation.sheet == nil)
            #expect(owner.fullDeckName(first.id) == "日本語::QZ61" && owner.fullDeckName(second.id) == "日本語::QZ62")
            let step = try await undo.peekUndo(scope: .global)
            #expect(CollectionUndoModel.stepName(step) == "Rename Deck")
            _ = try await undo.startUndo(scope: .global).value()
            await owner.loadCounts()
            #expect(owner.fullDeckName(first.id) == "Languages::日本語::QZ61", "One step restores the whole subtree")
            #expect(owner.fullDeckName(second.id) == "Languages::日本語::QZ62")

            let plain = try await service.addDeck(name: "QZ63")
            await owner.loadCounts()
            model.present(.rename(plain.id))
            model.name = "QZ63 "
            #expect(model.canSubmit, "A difference the UI sees; D6 decides")
            model.submit(); await model.waitForCompletion()
            #expect(model.message == nil && owner.navigation.sheet == nil, "The form closes as after a rename")
            #expect(owner.fullDeckName(plain.id) == "QZ63")
            #expect(CollectionUndoModel.stepName(try await undo.peekUndo(scope: .global)) == "Add Deck",
                "D6 step 3 wrote nothing and recorded no step")
        }
    }

    /// N20.3: Rename shows the renamed deck's page, unless the person went
    /// elsewhere while it ran; the committed rename stays either way.
    @Test(arguments: [false, true])
    func renameOpensTheRenamedDeckOnlyIfThePersonStayed(navigates: Bool) async throws {
        try await withDeckNameOwner { owner in
            let model = owner.deckActions, gate = DeckNameGate(), service = CoreDeckService()
            let shown = try await service.addDeck(name: "QZ64").id
            let renamed = try await service.addDeck(name: "QZ65").id
            await owner.loadCounts()
            try #require(await owner.openDeckPage(deckID: shown).value)
            model.startRename = { id, name in
                let job = try await CoreDeckService().startRenameDeck(id: id, name: name)
                _ = try await job.value()
                await gate.hold()
                return job
            }
            model.present(.rename(renamed)); model.name = "QZ66"; model.submit()
            await gate.entered()
            if navigates { owner.navigation.openHome() }
            gate.release(); await model.waitForCompletion()
            #expect(owner.fullDeckName(renamed) == "QZ66")
            #expect(owner.navigation.homeDeckPage == (navigates ? nil : DeckID(renamed)))
            #expect(owner.navigation.sheet == nil && model.message == nil)
        }
    }

    @Test(arguments: [false, true])
    func admittedRenameDrainsBeforeBackgroundOrCloseAndLateCancelKeepsCommit(closing: Bool) async throws {
        try await withDeckNameOwner { owner in
            let model = owner.deckActions, gate = DeckNameGate()
            let id = try await CoreDeckService().addDeck(name: "Drain Before").id
            await owner.loadCounts()
            model.startRename = { id, name in
                let job = try await CoreDeckService().startRenameDeck(id: id, name: name)
                // Hold delivery of an actual committed job, not a fake success.
                _ = try await job.value()
                await gate.hold()
                return job
            }
            model.present(.rename(id)); model.name = "Drain After"; model.submit()
            await gate.entered()
            var drained = false
            let lifecycle = Task {
                if closing { #expect(await owner.close()) }
                else { owner.setForeground(false); await owner.waitForLifecycle() }
                drained = true
            }
            await Task.yield()
            #expect(!drained && model.busy)
            gate.release(); await lifecycle.value
            #expect(drained && !model.busy)
            if closing {
                #expect(model.finalName == nil && owner.navigation.sheet == nil)
                await owner.start()
            } else {
                #expect(model.finalName == nil)
                owner.setForeground(true); await owner.waitForLifecycle(); await model.waitForCompletion()
                #expect(model.finalName == "Drain After")
                #expect(model.message == nil)
            }
            #expect(owner.fullDeckName(id) == "Drain After")
        }
    }

    private func withHiddenDefaultOwner(_ body: (ReviewCollection) async throws -> Void) async throws {
        try await withDeckNameOwner { owner in
            let service = CoreDeckService(), model = owner.deckActions
            let deletion = try await service.startDeleteDecks(ids: [1], confirmed: try await service.deckDeletionSummary(ids: [1]))
            _ = try await deletion.value()
            _ = try await service.addDeck(name: "QZ80")
            await owner.loadCounts()
            try #require(owner.deck(1) == nil)
            model.present(.create); model.name = "dEfAuLt"
            model.submit(); await model.waitForCompletion(); model.cancel()
            try #require(owner.navigation.homeDeckPage == DeckID(1))
            try #require(owner.deckPage(1)?.name == "Default" && owner.deck(1) == nil)
            try await body(owner)
        }
    }

    private func withDeckNameOwner(_ body: (ReviewCollection) async throws -> Void) async throws {
        let files = FileManager.default, root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"))
        do {
            await owner.start()
            owner.entry.inspect(try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg")))
            await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            owner.navigation.dismissSheet()
            try await body(owner)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
