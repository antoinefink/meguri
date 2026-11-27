import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct SavedDeckStateTests {
    @Test func pendingDeckFeedbackIsSynchronousAndDoesNotChangeRestoration() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        owner.navigation.select(.section(.browse))
        let saved = owner.navigation.uiState
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        // No yield: both native layouts can select and name the requested deck now.
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        #expect(owner.navigation.sidebarSelection == .deck(DeckID(2)))
        #expect(owner.navigation.compactTab == .decks)
        #expect(owner.navigation.compactPath(.decks) == [.deckPage(DeckID(2))])
        owner.navigation.setCompactPath(owner.navigation.compactPath(.decks), for: .decks)
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        #expect(owner.navigation.uiState == saved)
        await gate.waitUntilEntered()
        gate.release()
        #expect(await opening.value)
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
    }

    @Test func rapidABACompletionCannotEraseNewerPendingFeedbackOrReturnAfterLeaving() async {
        let (owner, store) = await makeOwner()
        let first = DeckStateGate(), last = DeckStateGate()
        store.selectionGates = [first, last]
        let a = owner.openDeckPage(deckID: 2)
        await first.waitUntilEntered()
        let b = owner.openDeckPage(deckID: 3)
        let again = owner.openDeckPage(deckID: 2)
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        first.release()
        #expect(!(await a.value))
        #expect(!(await b.value))
        await last.waitUntilEntered()
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        owner.navigation.select(.section(.stats))
        #expect(owner.navigation.pendingDeckPage == nil)
        last.release()
        #expect(!(await again.value))
        #expect(owner.navigation.section == .stats)
        #expect(owner.navigation.homeDeckPage == nil)
    }

    @Test func loadedDeckRemainsVisibleWhenReturningFromAPendingSelection() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(2))
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let b = owner.openDeckPage(deckID: 3)
        await gate.waitUntilEntered()
        let a = owner.openDeckPage(deckID: 2)
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.navigation.sidebarSelection == .deck(DeckID(2)))
        gate.release()
        #expect(!(await b.value))
        #expect(await a.value)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
    }

    @Test func treePublicationVersionIgnoresNavigationAndIncludesDisclosureWrites() async {
        let (owner, _) = await makeOwner()
        let version = owner.deckTreeVersion
        owner.navigation.select(.section(.stats))
        owner.navigation.select(.section(.home))
        #expect(owner.deckTreeVersion == version)
        await owner.setStudyExpanded(deckID: 2, expanded: false).value
        #expect(owner.deckTreeVersion > version)
    }

    @Test func deckNavigationAwaitsSaveAndSameDeckStillUsesCoreNoOp() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        #expect(owner.navigation.homeDeckPage == DeckID(4))
        #expect(store.selected == 1)
        gate.release()
        #expect(await opening.value)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        #expect(store.selected == 2)
        owner.navigation.push(.noteTypes)
        #expect(await owner.openDeckPage(deckID: 2).value)
        #expect(owner.navigation.path(.home) == [.noteTypes])
        #expect(store.selectionAttempts == [2, 2])
        #expect(store.selectionWrites == 1)
    }

    @Test func rapidSelectionPublishesOnlyLatestAndSkipsSupersededQueuedIntent() async {
        let (owner, store) = await makeOwner()
        let first = DeckStateGate(), last = DeckStateGate()
        store.selectionGates = [first, last]
        let a = owner.openDeckPage(deckID: 1)
        await first.waitUntilEntered()
        let b = owner.openDeckPage(deckID: 2)
        let c = owner.openDeckPage(deckID: 3)
        first.release()
        await last.waitUntilEntered()
        #expect(owner.navigation.homeDeckPage == nil)
        #expect(store.selectionAttempts == [1, 3])
        #expect(!(await a.value))
        #expect(!(await b.value))
        last.release()
        #expect(await c.value)
        #expect(owner.navigation.homeDeckPage == DeckID(3))
        #expect(store.selected == 3)
    }

    @Test func returningHomeCancelsPendingDeckNavigationEvenWhenHomeAlreadySelected() async {
        let (owner, store) = await makeOwner()
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.navigation.select(.section(.home))
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.homeDeckPage == nil)
        #expect(owner.navigation.section == .home)
    }

    @Test func selectionFailureKeepsPageAndDoesNotExposeDiagnosticOrOverwriteNewerIntent() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        store.selectionFailures = [2]
        #expect(!(await owner.openDeckPage(deckID: 2).value))
        #expect(owner.navigation.homeDeckPage == DeckID(4))
        #expect(owner.error == "Couldn’t select this deck. Try again.")

        let (fresh, nextStore) = await makeOwner()
        let gate = DeckStateGate()
        nextStore.selectionGates = [gate]
        nextStore.selectionFailures = [2]
        let old = fresh.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        let next = fresh.openDeckPage(deckID: 3)
        gate.release()
        #expect(!(await old.value))
        #expect(await next.value)
        #expect(fresh.navigation.homeDeckPage == DeckID(3))
        #expect(fresh.error == nil)
    }

    @Test(arguments: [false, true])
    func selectionFailureSurvivesBackgroundRefreshFailureAndRetry(refreshFinishesFirst: Bool) async throws {
        let (owner, store) = await makeOwner()
        #expect(await owner.openDeckPage(deckID: 1).value)
        let selection = DeckStateGate(), refresh = DeckStateGate()
        store.selectionGates = [selection]
        store.selectionFailures = [2]
        owner.loadDeckTree = {
            await refresh.hold()
            throw BridgeError.internal("Synthetic background refresh failure")
        }
        let refreshing = Task { await owner.loadCounts() }
        await refresh.waitUntilEntered()
        let opening = owner.openDeckPage(deckID: 2)
        await selection.waitUntilEntered()
        if refreshFinishesFirst {
            refresh.release()
            await refreshing.value
            #expect(owner.error == "Couldn’t refresh decks. Try again.")
        }
        selection.release()
        #expect(!(await opening.value))
        if !refreshFinishesFirst {
            refresh.release()
            await refreshing.value
        }
        #expect(owner.navigation.homeDeckPage == DeckID(1))
        #expect(owner.navigation.sidebarSelection == .deck(DeckID(1)))
        #expect(store.selected == 1)
        #expect(owner.error == "Couldn’t select this deck. Try again.")

        // Restoring A mounts its page again, starting a fresh detail refresh.
        owner.loadFixtureDeckEmptiness = { _ in throw BridgeError.internal("Synthetic detail failure") }
        let request = try #require(owner.deckPageRefreshRequest)
        await owner.refreshDeckPage(request)
        #expect(owner.error == "Couldn’t select this deck. Try again.")
        #expect(owner.navigation.homeDeckPage == DeckID(1))
        store.selectionFailures = []
        #expect(await owner.openDeckPage(deckID: 2).value)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        #expect(store.selected == 2)
        #expect(store.selectionAttempts == [1, 2, 2])
        #expect(owner.error == nil)
    }

    @Test func syntheticDeckDetailsPublishEmptinessAndReportReadFailure() async throws {
        let (owner, _) = await makeOwner()
        #expect(await owner.openDeckPage(deckID: 1).value)
        let request = try #require(owner.deckPageRefreshRequest)
        owner.loadFixtureDeckEmptiness = { id in
            #expect(id == 1)
            return true
        }
        await owner.refreshDeckPage(request)
        #expect(owner.emptyDeckPageID == 1)
        #expect(owner.error == nil)
        owner.loadFixtureDeckEmptiness = { _ in throw BridgeError.internal("Synthetic detail failure") }
        await owner.refreshDeckPage(request)
        #expect(owner.emptyDeckPageID == nil)
        #expect(owner.error == "Couldn’t refresh the deck. Try again.")
    }

    /// N14.3, H5.4: the page offers Add Note only once a read found no card; any other
    /// regular deck with nothing waiting is finished. A failed or running read keeps the last answer.
    @Test func deckPageEmptinessSeparatesEmptyFromFinished() async throws {
        let (owner, _) = await makeOwner()
        #expect(await owner.openDeckPage(deckID: 1).value)
        #expect(!owner.homeDeckEmpty(1), "Unread: finished, never guessed empty")
        let request = try #require(owner.deckPageRefreshRequest)
        owner.loadFixtureDeckEmptiness = { _ in true }
        await owner.refreshDeckPage(request)
        #expect(owner.homeDeckEmpty(1))
        #expect(!owner.homeDeckEmpty(2))
        owner.loadFixtureDeckEmptiness = { _ in throw BridgeError.internal("Synthetic detail failure") }
        await owner.refreshDeckPage(request)
        #expect(owner.homeDeckEmpty(1), "A failed read keeps the last one")
        owner.loadFixtureDeckEmptiness = { _ in false }
        await owner.refreshDeckPage(request)
        #expect(!owner.homeDeckEmpty(1))
    }

    @Test(arguments: [false, true])
    func abandonedSelectionFailureDoesNotReachHomeOrAnotherSection(showsAnotherSection: Bool) async {
        let (owner, store) = await makeOwner()
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        store.selectionFailures = [2]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        if showsAnotherSection {
            owner.navigation.show(.stats)
        } else {
            owner.navigation.select(.section(.home))
        }
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.section == (showsAnotherSection ? .stats : .home))
        #expect(owner.navigation.homeDeckPage == nil)
        #expect(owner.error == nil)
        #expect(store.selectionAttempts == [2])
        #expect(store.selected == 1)
        owner.navigation.select(.section(.home))
        #expect(owner.error == nil)
    }

    @Test(arguments: [false, true])
    func regularStackPopOrPushSupersedesPendingSelection(pushes: Bool) async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        if !pushes { owner.navigation.push(.noteTypes) }
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        if pushes {
            owner.navigation.push(.noteTypes)
        } else {
            // The regular NavigationStack binding writes this on native Back.
            owner.navigation.detailPath = []
        }
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.section == .home)
        #expect(owner.navigation.homeDeckPage == DeckID(4))
        #expect(owner.navigation.detailPath == (pushes ? [.noteTypes] : []))
        #expect(store.selected == 2)
        #expect(owner.error == nil)
    }

    @Test func unchangedRegularPathDoesNotSupersedeSameDeckSelection() async {
        let (owner, store) = await makeOwner()
        #expect(await owner.openDeckPage(deckID: 2).value)
        owner.navigation.push(.noteTypes)
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.navigation.detailPath = [.noteTypes]
        gate.release()
        #expect(await opening.value)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        #expect(owner.navigation.detailPath == [.noteTypes])
        #expect(store.selectionAttempts == [2, 2])
        #expect(store.selectionWrites == 1)
    }

    @Test func compactBackSupersedesPendingSelection() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.navigation.setCompactPath([], for: .decks)
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.homeDeckPage == nil)
        #expect(owner.navigation.compactPath(.decks).isEmpty)
        #expect(store.selected == 2)
        #expect(owner.error == nil)
    }

    @Test func generationChangeRejectsInFlightAndQueuedSelectionAndStaleSummary() async {
        let (owner, store) = await makeOwner()
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let old = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        let queued = owner.openDeckPage(deckID: 3)
        store.generation = 2
        owner.receiveLifecycle(.ready(generation: 2))
        #expect(owner.navigation.pendingDeckPage == nil)
        gate.release()
        #expect(!(await old.value))
        #expect(!(await queued.value))
        #expect(store.selectionAttempts == [2])
        #expect(owner.navigation.homeDeckPage == nil)
        #expect(owner.error == nil)
        await owner.loadCounts()
        #expect(await owner.openDeckPage(deckID: 4).value)

        let (other, _) = await makeOwner()
        other.saveSelectedDeck = { _ in 99 }
        #expect(!(await other.openDeckPage(deckID: 2).value))
        #expect(other.navigation.homeDeckPage == nil)
    }

    @Test func nestedDisclosureUsesSavedStudyFlagsAndPreservesChildrenBrowserFlagsAndSelection() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(3))
        #expect(!owner.studyExpanded(deckID: 1))
        #expect(owner.studyExpanded(deckID: 2))
        #expect(!owner.studyExpanded(deckID: 3))
        let before = owner.decks
        await owner.setStudyExpanded(deckID: 1, expanded: true).value
        await owner.loadCounts()
        #expect(owner.studyExpanded(deckID: 1))
        #expect(owner.studyExpanded(deckID: 2))
        #expect(!owner.studyExpanded(deckID: 3))
        #expect(store.collapseCalls.count == 1)
        #expect(store.collapseCalls.first?.ids == [1])
        #expect(owner.decks.map(\.id) == before.map(\.id))
        #expect(owner.deck(1)?.children.map(\.id) == [2])
        #expect(owner.deck(2)?.children.map(\.id) == [3])
        #expect(owner.deck(1)?.browserCollapsed == true)
        #expect(owner.deck(2)?.browserCollapsed == false)
        #expect(owner.deck(3)?.browserCollapsed == true)
        #expect(owner.navigation.homeDeckPage == DeckID(3))
        #expect(owner.deck(1)?.counts == before.first?.counts)

        await owner.setStudyExpanded(deckID: 1, expanded: true, includingDescendants: true).value
        await owner.loadCounts()
        #expect(store.collapseCalls.last?.ids == [1, 2, 3])
        #expect([Int64(1), 2, 3].allSatisfy { owner.studyExpanded(deckID: $0) })
        #expect(!owner.studyExpanded(deckID: 4))
        // Collapsing the parent alone retains descendant expansion for reopening it.
        await owner.setStudyExpanded(deckID: 1, expanded: false).value
        await owner.setStudyExpanded(deckID: 1, expanded: true).value
        await owner.loadCounts()
        #expect(owner.studyExpanded(deckID: 2) && owner.studyExpanded(deckID: 3))
    }

    @Test func rapidDisclosureKeepsLatestIntentWhileEarlierWriteCompletes() async {
        let (owner, store) = await makeOwner()
        let first = DeckStateGate(), last = DeckStateGate()
        store.collapseGates = [first, last]
        let expanding = owner.setStudyExpanded(deckID: 1, expanded: true, includingDescendants: true)
        await first.waitUntilEntered()
        let collapsingChild = owner.setStudyExpanded(deckID: 2, expanded: false)
        #expect(owner.studyExpanded(deckID: 1))
        #expect(!owner.studyExpanded(deckID: 2))
        #expect(owner.studyExpanded(deckID: 3))
        first.release()
        await last.waitUntilEntered()
        #expect(!owner.studyExpanded(deckID: 2))
        last.release()
        await expanding.value; await collapsingChild.value
        await owner.loadCounts()
        #expect(owner.studyExpanded(deckID: 1))
        #expect(!owner.studyExpanded(deckID: 2))
        #expect(owner.studyExpanded(deckID: 3))
        #expect(store.collapseCalls.map(\.ids) == [[1, 2, 3], [2]])

        let one = owner.setStudyExpanded(deckID: 1, expanded: false)
        let two = owner.setStudyExpanded(deckID: 1, expanded: true)
        await one.value; await two.value
        await owner.loadCounts()
        #expect(owner.studyExpanded(deckID: 1))
        #expect(store.collapseCalls.count == 3)
    }

    @Test func failedDisclosureRestoresSavedFlagsAndStaleGenerationCannotRestoreOldIntent() async {
        let (owner, store) = await makeOwner()
        store.failCollapse = true
        let gate = DeckStateGate()
        store.collapseGates = [gate]
        let action = owner.setStudyExpanded(deckID: 1, expanded: true)
        await gate.waitUntilEntered()
        #expect(owner.studyExpanded(deckID: 1))
        gate.release()
        await action.value
        await owner.loadCounts()
        #expect(!owner.studyExpanded(deckID: 1))
        #expect(owner.error == "Couldn’t save deck expansion. Try again.")
        #expect(owner.deck(1)?.browserCollapsed == true)

        let (fresh, nextStore) = await makeOwner()
        let staleGate = DeckStateGate()
        nextStore.collapseGates = [staleGate]
        let stale = fresh.setStudyExpanded(deckID: 1, expanded: true)
        await staleGate.waitUntilEntered()
        let queued = fresh.setStudyExpanded(deckID: 2, expanded: false)
        nextStore.generation = 2
        fresh.receiveLifecycle(.ready(generation: 2))
        nextStore.failCollapse = true
        await fresh.loadCounts()
        #expect(!fresh.studyExpanded(deckID: 1))
        #expect(fresh.studyExpanded(deckID: 2))
        staleGate.release()
        await stale.value; await queued.value
        #expect(fresh.error == nil)
        #expect(nextStore.collapseCalls.count == 1)
        #expect(!fresh.studyExpanded(deckID: 1))
        #expect(fresh.studyExpanded(deckID: 2))
    }

    @Test func countQueryStartedBeforeDisclosureSaveCannotReplaceSavedTree() async {
        let (owner, store) = await makeOwner()
        let gate = DeckStateGate()
        store.queryGates = [gate]
        let stale = Task { await owner.loadCounts() }
        await gate.waitUntilEntered()
        await owner.setStudyExpanded(deckID: 1, expanded: true).value
        await owner.loadCounts()
        #expect(owner.studyExpanded(deckID: 1))
        gate.release()
        await stale.value
        #expect(owner.studyExpanded(deckID: 1))
    }

    @Test func sidebarSelectionRequestsSavingEvenForAlreadySelectedRow() {
        let navigation = AppNavigation()
        navigation.openDeckPage(DeckID(2))
        var requests: [DeckID] = []
        navigation.deckSelectionAction = { requests.append($0) }
        navigation.select(.deck(DeckID(3)))
        navigation.select(.deck(DeckID(2)))
        #expect(requests == [DeckID(3), DeckID(2)])
        #expect(navigation.homeDeckPage == DeckID(2))
    }

    @Test func pendingSplitDetailBindingPreservesCommittedStackAndRejectsEchoes() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        owner.navigation.push(.noteTypes)
        let saved = owner.navigation.uiState
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        #expect(owner.navigation.displayedHomeDeckPage == DeckID(2))
        #expect(owner.navigation.detailPath == [.noteTypes])
        #expect(owner.navigation.pendingDetailPath == [.deckPage(DeckID(2))])
        // Both split layouts use this binding; native echoes cannot finish the save.
        owner.navigation.pendingDetailPath = owner.navigation.pendingDetailPath
        owner.navigation.detailPath = [.noteTypes]
        owner.navigation.setCompactPath([.deckPage(DeckID(4)), .noteTypes], for: .decks)
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        #expect(owner.navigation.uiState == saved)
        await gate.waitUntilEntered()
        // The transient stack's own native Back reveals exactly the prior stack.
        owner.navigation.pendingDetailPath = []
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.navigation.detailPath == [.noteTypes])
        #expect(owner.navigation.displayedHomeDeckPage == DeckID(4))
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.uiState == saved)
    }

    @Test func compactReselectionWhilePendingFromBrowseReturnsToDecksRoot() async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        owner.navigation.show(.browse)
        owner.navigation.push(.noteTypes)
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.navigation.selectTab(.decks)
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.navigation.compactTab == .decks)
        #expect(owner.navigation.compactPath(.decks).isEmpty)
        #expect(owner.navigation.path(.browse) == [.noteTypes])
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.section == .home)
    }

    @Test(arguments: [CollectionLifecycle.failed(.corrupt(detail: "synthetic failure")),
        .exclusiveBusy(op: .checkDatabase, phase: .working, cancellable: false),
        .opening, .noCollection, .ready(generation: 2)])
    func invalidationRetiresHeldSaveFeedbackSynchronously(lifecycle: CollectionLifecycle) async {
        let (owner, store) = await makeOwner()
        owner.navigation.openDeckPage(DeckID(4))
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.receiveLifecycle(lifecycle)
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.navigation.homeDeckPage == DeckID(4))
        if case .failed = lifecycle { #expect(!owner.homePhase.unknown) }
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.error == nil)
    }

    @Test func invalidationCannotRetireANewerNavigationIntent() async {
        let (owner, store) = await makeOwner()
        let gate = DeckStateGate()
        store.selectionGates = [gate]
        let opening = owner.openDeckPage(deckID: 2)
        await gate.waitUntilEntered()
        owner.navigation.show(.stats)
        let newer = owner.navigation.beginDeckSelection(deck: DeckID(3))
        owner.receiveLifecycle(.noCollection)
        #expect(owner.navigation.intentRevision == newer)
        #expect(owner.navigation.pendingDeckPage == DeckID(3))
        gate.release()
        #expect(!(await opening.value))
        #expect(owner.navigation.pendingDeckPage == DeckID(3))
    }

    @Test func firstReadPreselectionSurvivesOpeningButFailureRetiresItImmediately() async {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        let opening = owner.openDeckPage(deckID: 2)
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        owner.receiveLifecycle(.opening)
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        owner.receiveLifecycle(.ready(generation: 1))
        #expect(owner.navigation.pendingDeckPage == DeckID(2))
        owner.receiveLifecycle(.failed(.corrupt(detail: "synthetic failure")))
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(!(await opening.value))
        #expect(owner.navigation.homeDeckPage == nil)
    }

    @Test func firstReadFailureRetiresPreselectionWithoutShowingLoadedContent() async {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        owner.loadDeckTree = { throw BridgeError.internal("synthetic failure") }
        let opening = owner.openDeckPage(deckID: 2)
        owner.receiveLifecycle(.ready(generation: 1))
        await owner.loadCounts()
        #expect(owner.navigation.pendingDeckPage == nil)
        #expect(owner.homePhase == .readFailed)
        #expect(!(await opening.value))
        #expect(owner.navigation.homeDeckPage == nil)
    }

    @Test func unchangedTreeVersionStillTracksGenerationAndReadFailure() async {
        let (owner, store) = await makeOwner()
        let nodes = owner.decks, version = owner.deckTreeVersion
        await owner.loadCounts()
        #expect(owner.deckTreeVersion == version)
        store.generation = 2
        owner.receiveLifecycle(.ready(generation: 2))
        owner.loadDeckTree = { throw BridgeError.internal("synthetic failure") }
        await owner.loadCounts()
        #expect(owner.deckTreeVersion == version)
        #expect(owner.deckTreeGeneration == 1)
        #expect(owner.homePhase == .readFailed)
        owner.loadDeckTree = { try await store.load() }
        await owner.loadCounts()
        #expect(owner.decks == nodes)
        #expect(owner.deckTreeVersion == version)
        #expect(owner.deckTreeGeneration == 2)
        #expect(owner.homePhase == .decks)
        #expect(owner.deck(3)?.name == "C")
    }

    private func makeOwner() async -> (ReviewCollection, SavedDeckStore) {
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let store = SavedDeckStore()
        owner.loadDeckTree = { try await store.load() }
        owner.saveSelectedDeck = { try await store.select($0) }
        owner.saveStudyCollapse = { try await store.collapse($0, collapsed: $1) }
        owner.receiveLifecycle(.ready(generation: store.generation))
        await owner.loadCounts()
        return (owner, store)
    }
}

@MainActor
private final class DeckStateGate {
    private var entered = false
    private var enteredWaiter: CheckedContinuation<Void, Never>?
    private var releaseWaiter: CheckedContinuation<Void, Never>?
    func waitUntilEntered() async {
        if entered { return }
        await withCheckedContinuation { enteredWaiter = $0 }
    }
    func hold() async {
        await withCheckedContinuation { continuation in
            releaseWaiter = continuation
            entered = true
            enteredWaiter?.resume(); enteredWaiter = nil
        }
    }
    func release() { releaseWaiter?.resume(); releaseWaiter = nil }
}

@MainActor
private final class SavedDeckStore {
    var generation: UInt64 = 1
    var selected: Int64 = 1
    var selectionAttempts: [Int64] = []
    var selectionWrites = 0
    var selectionFailures: Set<Int64> = []
    var selectionGates: [DeckStateGate] = []
    var collapseGates: [DeckStateGate] = []
    var queryGates: [DeckStateGate] = []
    var failCollapse = false
    var collapsed: Set<Int64> = [1, 3, 4]
    var collapseCalls: [(ids: [Int64], collapsed: Bool)] = []

    func select(_ id: Int64) async throws -> UInt64 {
        let expected = generation
        selectionAttempts.append(id)
        if !selectionGates.isEmpty { await selectionGates.removeFirst().hold() }
        if selectionFailures.contains(id) { throw BridgeError.internal("private diagnostic") }
        if selected != id { selectionWrites += 1; selected = id }
        return expected
    }
    func collapse(_ ids: [Int64], collapsed value: Bool) async throws -> UInt64 {
        let expected = generation
        collapseCalls.append((ids, value))
        if !collapseGates.isEmpty { await collapseGates.removeFirst().hold() }
        if failCollapse { throw BridgeError.internal("private diagnostic") }
        for id in ids {
            if value { collapsed.insert(id) } else { collapsed.remove(id) }
        }
        return expected
    }
    func load() async throws -> CountedDeckTree {
        let counts = ReviewCounts(new: 2, learning: 1, review: 3)
        let c = CountedDeckNode(id: 3, name: "C", filtered: false,
            studyCollapsed: collapsed.contains(3), browserCollapsed: true, counts: counts, children: [])
        let b = CountedDeckNode(id: 2, name: "B", filtered: false,
            studyCollapsed: collapsed.contains(2), browserCollapsed: false, counts: counts, children: [c])
        let a = CountedDeckNode(id: 1, name: "A", filtered: false,
            studyCollapsed: collapsed.contains(1), browserCollapsed: true, counts: counts, children: [b])
        let d = CountedDeckNode(id: 4, name: "D", filtered: false,
            studyCollapsed: collapsed.contains(4), browserCollapsed: false, counts: counts, children: [])
        let tree = CountedDeckTree(generation: generation, nextChangeAt: Int64.max,
            nodes: [a, d])
        if !queryGates.isEmpty { await queryGates.removeFirst().hold() }
        return tree
    }
}
