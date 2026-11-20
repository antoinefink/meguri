import CoreBridge
import Foundation
import Testing
@testable import AppModel
@testable import Home

@MainActor
@Suite struct GoToDeckModelTests {
    @Test func canonicalReadAloneControlsCreateAndBlankNeverLooksUp() async {
        let harness = DeckPickerHarness()
        var queries: [String] = []
        harness.lookupName = { query in
            queries.append(query)
            return .init(generation: 11, deck: query == "new" ? nil : harness.snapshot.decks.first)
        }
        let picker = DeckPickerHarness.makeModel(harness)
        await picker.load(presentation: 1, generation: 11)
        picker.query = " \n\t "
        await picker.waitForLookup()
        #expect(queries.isEmpty && !picker.canCreate)
        // Display filtering is not authority for canonical absence or Default's identity.
        picker.query = " DEFAULT: "
        await picker.waitForLookup()
        #expect(picker.matches.isEmpty && !picker.canCreate)
        picker.query = "new"
        #expect(!picker.canCreate && !picker.canSubmit)
        await picker.waitForLookup()
        #expect(picker.canCreate && picker.selection == .create && picker.canSubmit)
        #expect(queries == [" DEFAULT: ", "new"])
    }

    @Test func newerDraftDiscardsEarlierNameDecision() async {
        let harness = DeckPickerHarness(), barrier = PickerGate()
        let (finishedRead, finishRead) = AsyncStream<Void>.makeStream()
        harness.lookupName = { text in
            switch text {
            case "Fresh Entry": return .init(generation: 11, deck: nil)
            case "Pending Entry":
                await barrier.hold(); finishRead.finish()
                return .init(generation: 11, deck: nil)
            default: return .init(generation: 11, deck: harness.snapshot.decks.first)
            }
        }
        let model = DeckPickerHarness.makeModel(harness)
        await model.load(presentation: 1, generation: 11)
        model.query = "Fresh Entry"
        await model.waitForLookup()
        #expect(model.canCreate && model.selection == .create)
        model.query = "Pending Entry"
        #expect(!model.canCreate && model.selection != .create)
        await barrier.entered()
        model.query = "Current Entry"
        #expect(!model.canCreate && model.selection != .create)
        await model.waitForLookup()
        barrier.release()
        for await _ in finishedRead {}
        #expect(!model.canCreate && !model.canSubmit)
        model.submit(layout: .sidebar)
        #expect(harness.submittedNames.isEmpty)
    }

    @Test(arguments: ["readFailure", "collectionChange", "sheetChange", "replyChange"])
    func nameDecisionRequiresMatchingOrigin(reason: String) async {
        let harness = DeckPickerHarness(), barrier = PickerGate()
        harness.lookupName = { _ in
            await barrier.hold()
            if reason == "readFailure" { throw BridgeError.internal("synthetic read detail") }
            return .init(generation: reason == "replyChange" ? 12 : 11, deck: nil)
        }
        let model = DeckPickerHarness.makeModel(harness)
        await model.load(presentation: 1, generation: 11)
        model.query = "Candidate Deck"
        await barrier.entered()
        if reason == "collectionChange" { harness.generation = 12 }
        if reason == "sheetChange" { harness.active = false }
        barrier.release(); await model.waitForLookup()
        #expect(!model.canCreate && !model.canSubmit)
        model.submit(layout: .compact)
        #expect(harness.submittedNames.isEmpty)
        let expected: GoToDeckModel.Message? = reason == "readFailure" ? .lookupFailed :
            reason == "replyChange" ? .unavailable : nil
        #expect(model.message == expected)
    }

    @Test func savedContinuationKeepsSubmittedTextAndReadsCurrentLayout() async {
        let harness = DeckPickerHarness(), barrier = PickerGate()
        harness.lookupName = { _ in .init(generation: 11, deck: nil) }
        harness.outcome = .saved; harness.operationGate = barrier
        let model = DeckPickerHarness.makeModel(harness)
        await model.load(presentation: 1, generation: 11)
        let original = "  QZ74 :: QZ75:  "
        model.query = original
        await model.waitForLookup()
        model.submit(layout: .sidebar); model.submit(layout: .compact)
        #expect(model.busy && harness.submittedNames == [original])
        await barrier.entered()
        model.liveLayout = .compact
        barrier.release(); await model.waitForActivation()
        #expect(model.hasSavedResult && model.message == .saved && model.canSubmit)
        await model.load(presentation: 1, generation: 11)
        #expect(model.hasSavedResult && model.message == .saved)
        #expect(harness.observedLayouts == [.compact])
        model.query = "Another Branch"
        #expect(model.query == original)
        harness.operationGate = nil; harness.outcome = .opened
        model.submit(layout: .compact); await model.waitForActivation()
        #expect(harness.submittedNames == [original, original])
        // The owner separately proves that continuation requests share one write.
        #expect(harness.actions.isEmpty)
    }

    @Test func savedReplyRestoresItsOwnDraftAfterAnEditDuringTheWrite() async {
        let harness = DeckPickerHarness(), barrier = PickerGate()
        harness.lookupName = { _ in .init(generation: 11, deck: nil) }
        harness.operationGate = barrier; harness.outcome = .saved
        let model = DeckPickerHarness.makeModel(harness)
        await model.load(presentation: 1, generation: 11)
        model.query = "Original Path"
        await model.waitForLookup()
        model.submit(layout: .sidebar)
        await barrier.entered()
        model.query = "Later Draft"
        await model.waitForLookup()
        barrier.release(); await model.waitForActivation()
        #expect(model.hasSavedResult && model.message == .saved && model.query == "Original Path")
        model.query = "Third Draft"
        #expect(model.query == "Original Path")
        harness.operationGate = nil
        model.submit(layout: .compact); await model.waitForActivation()
        #expect(harness.submittedNames == ["Original Path", "Original Path"])
    }

    @Test func ownerIntentChangeKeepsSavedModelBoundToSubmittedQuery() async throws {
        let owner = await makeOwner(), barrier = PickerGate()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        var requests: [String] = []
        let model = GoToDeckModel(read: { .init(generation: 11, decks: []) },
            isCurrent: { owner.goToDeckIsCurrent($0, generation: $1) },
            activate: { owner.activateGoToDeck($0, generation: $1, presentation: $2, layout: $3) },
            dismiss: { owner.cancelGoToDeck($0) },
            matchName: { _ in .init(generation: 11, deck: nil) },
            create: { text, _, _, _, _ in
                requests.append(text)
                // Real-writer owner tests establish this committed-result contract.
                let first = requests.count == 1
                return Task { if first { await barrier.hold() }; return .saved }
            })
        await model.load(presentation: token, generation: 11)
        model.query = "Submitted Origin"; await model.waitForLookup()
        model.submit(layout: .sidebar)
        await barrier.entered()
        let intent = owner.navigation.intentRevision
        owner.navigation.show(.settings)
        #expect(owner.navigation.intentRevision != intent)
        barrier.release(); await model.waitForActivation()
        #expect(model.hasSavedResult && model.message == .saved && model.canSubmit)
        #expect(owner.goToDeckPresentation == token && owner.navigation.sheet == .goToDeck)
        #expect(owner.navigation.section == .settings && owner.navigation.homeDeckPage == nil && owner.review == nil)
        model.query = "Different Request"
        #expect(model.query == "Submitted Origin")
        model.submit(layout: .compact)
        await model.waitForActivation()
        #expect(requests == ["Submitted Origin", "Submitted Origin"])
    }

    @Test(arguments: ["parent", "depth", "internal", "argument"])
    func ownerCreateRefusalLeavesRawDraftEditable(kind: String) async throws {
        let owner = await makeOwner()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        var writes: [String] = [], selections: [Int64] = []
        owner.deckActions.createDeck = { text in
            writes.append(text)
            switch kind {
            case "parent": throw BridgeError.deckRefused(.filteredParent)
            case "depth": throw BridgeError.deckRefused(.tooDeep)
            case "internal": throw BridgeError.internal("synthetic operation detail")
            default: throw BridgeError.invalidArgument("synthetic operation detail")
            }
        }
        owner.saveSelectedDeck = { id in selections.append(id); return 11 }
        let model = GoToDeckModel(read: { .init(generation: 11, decks: []) },
            isCurrent: { owner.goToDeckIsCurrent($0, generation: $1) },
            activate: { owner.activateGoToDeck($0, generation: $1, presentation: $2, layout: $3) },
            dismiss: { owner.cancelGoToDeck($0) },
            matchName: { _ in .init(generation: 11, deck: nil) },
            create: { owner.createGoToDeck(name: $0, match: $1, presentation: $2, knownDecks: $3, layout: $4) })
        await model.load(presentation: token, generation: 11)
        let raw = "  QZ76 :: QZ77:  "
        model.query = raw; await model.waitForLookup()
        model.submit(layout: .compact); await model.waitForActivation()
        let expected: GoToDeckModel.Message = kind == "parent" ? .filteredParent :
            kind == "depth" ? .tooDeep : .activationFailed
        #expect(model.message == expected && !model.hasSavedResult && model.query == raw)
        #expect(model.canCreate && model.canSubmit && writes == [raw] && selections.isEmpty)
        #expect(owner.review == nil && owner.navigation.review == nil && owner.navigation.homeDeckPage == nil)
        #expect(owner.goToDeckPresentation == token && owner.navigation.sheet == .goToDeck)
        model.query = "Shorter Path"; await model.waitForLookup()
        #expect(model.query == "Shorter Path" && model.message == nil && model.canSubmit)
        model.submit(layout: .sidebar); await model.waitForActivation()
        #expect(writes == [raw, "Shorter Path"] && selections.isEmpty && !model.hasSavedResult)
    }

    @Test func lookupFailureRetryCompletesNameDecisionWithoutWriting() async {
        let harness = DeckPickerHarness()
        var reads = 0
        harness.lookupName = { _ in
            reads += 1
            if reads == 1 { throw BridgeError.internal("synthetic lookup detail") }
            return .init(generation: 11, deck: nil)
        }
        let model = DeckPickerHarness.makeModel(harness)
        await model.load(presentation: 1, generation: 11)
        model.query = "Retry Candidate"; await model.waitForLookup()
        #expect(model.message == .lookupFailed && !model.canCreate && !model.canSubmit)
        model.retryLookup(); await model.waitForLookup()
        #expect(model.message == nil && model.canCreate && model.canSubmit && reads == 2)
        #expect(harness.submittedNames.isEmpty && harness.actions.isEmpty)
    }

    @Test func snapshotOrderLiteralNamesAndActualIDSelection() async {
        let harness = DeckPickerHarness()
        let picker = DeckPickerHarness.makeModel(harness)
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.matches == harness.snapshot.decks)
        #expect(picker.selectedID == 1) // Empty Default is included by D17, independently of any display tree.
        picker.query = "ja 1"
        #expect(picker.matches.map(\.id) == [42, 43])
        #expect(picker.selectedID == 42)
        picker.selectedID = 43
        picker.query = "JA lesson"
        #expect(picker.selectedID == 43)
        picker.query = "猫"
        #expect(picker.matches.map(\.fullName) == ["日本語::初級::猫"])
        #expect(picker.selectedID == 91)
        picker.query = "missing"
        #expect(picker.selectedID == nil && !picker.canSubmit)
        picker.submit(layout: .sidebar)
        #expect(harness.actions.isEmpty)
        picker.query = "Default"
        picker.selectedID = 42 // A stale native selection cannot submit an invisible row.
        #expect(!picker.canSubmit)
        picker.submit(layout: .sidebar)
        #expect(harness.actions.isEmpty)
    }

    @Test func failureRetainsDraftSelectionAndRetryUsesSameIDWithLiveLayout() async {
        let harness = DeckPickerHarness()
        harness.outcome = .failed
        let picker = DeckPickerHarness.makeModel(harness)
        await picker.load(presentation: 1, generation: 11)
        picker.query = "ja 1"; picker.selectedID = 43
        picker.submit(layout: .sidebar)
        await picker.waitForActivation()
        #expect(picker.message == .activationFailed)
        #expect(picker.query == "ja 1" && picker.selectedID == 43)
        harness.outcome = .opened
        picker.submit(layout: .compact)
        await picker.waitForActivation()
        #expect(harness.actions.map { $0.0.id } == [43, 43])
        #expect(harness.actions.map { $0.1 } == [.sidebar, .compact])
        #expect(harness.actions.last?.0.fullName == "Japanese::Lesson11")
    }

    @Test func loadingRetryKeepsTypedQueryAndNeverPublishesRawDiagnostics() async {
        let harness = DeckPickerHarness(), gate = PickerGate()
        harness.read = { await gate.hold(); throw BridgeError.internal("private diagnostic") }
        let picker = DeckPickerHarness.makeModel(harness)
        let first = Task { await picker.load(presentation: 1, generation: 11) }
        await gate.entered()
        picker.query = "猫"
        #expect(picker.loading && !picker.canSubmit)
        gate.release(); await first.value
        #expect(picker.message == .loadFailed && picker.snapshot == nil)
        harness.read = { harness.snapshot }
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.query == "猫" && picker.selectedID == 91)
        #expect(picker.message == nil)
    }

    @Test func repeatedSubmitIsReservedBeforeWaitingAndNewQuerySurvivesLateFailure() async {
        let harness = DeckPickerHarness(), gate = PickerGate()
        harness.operationGate = gate; harness.outcome = .failed
        let picker = DeckPickerHarness.makeModel(harness)
        await picker.load(presentation: 1, generation: 11)
        picker.query = "ja 1"; picker.selectedID = 43
        picker.submit(layout: .sidebar); picker.submit(layout: .sidebar)
        #expect(picker.busy && harness.actions.count == 1)
        await gate.entered()
        picker.query = "猫"
        picker.submit(layout: .compact)
        gate.release(); await picker.waitForActivation()
        #expect(harness.actions.count == 1)
        #expect(picker.query == "猫" && picker.selectedID == 91 && picker.message == nil)
    }

    @Test(arguments: [false, true])
    func lateReadCannotReplaceNewerSnapshotOrPublishAfterDismissal(reload: Bool) async {
        let harness = DeckPickerHarness(), gate = PickerGate()
        let old = harness.snapshot
        harness.read = { await gate.hold(); return old }
        let picker = DeckPickerHarness.makeModel(harness)
        let first = Task { await picker.load(presentation: 1, generation: 11) }
        await gate.entered()
        picker.query = "猫"
        if reload {
            harness.snapshot = DeckNameSnapshot(generation: 11, decks: [.init(id: 90, fullName: "猫::新", filtered: false)])
            harness.read = { harness.snapshot }
            await picker.load(presentation: 1, generation: 11)
        } else {
            picker.cancel()
            await picker.waitForActivation()
        }
        gate.release(); await first.value
        #expect(picker.query == "猫")
        #expect(picker.snapshot == (reload ? harness.snapshot : nil))
        #expect(picker.selectedID == (reload ? 90 : nil))
        #expect(picker.message == nil)
    }

    @Test func ownerLifecycleInvalidatesHeldRead() async throws {
        let owner = await makeOwner(), harness = DeckPickerHarness(), gate = PickerGate()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        let picker = GoToDeckModel(read: { await gate.hold(); return harness.snapshot },
            isCurrent: { owner.goToDeckIsCurrent($0, generation: $1) },
            activate: { owner.activateGoToDeck($0, generation: $1, presentation: $2, layout: $3) },
            dismiss: { owner.cancelGoToDeck($0) })
        let read = Task { await picker.load(presentation: token, generation: 11) }
        await gate.entered()
        picker.query = "kept"
        owner.receiveLifecycle(.noCollection)
        gate.release(); await read.value
        #expect(picker.snapshot == nil && picker.message == nil && !picker.canSubmit)
        #expect(picker.query == "kept" && owner.navigation.sheet == nil)
    }

    @Test func snapshotGenerationMismatchCannotActivate() async {
        let harness = DeckPickerHarness()
        harness.snapshot = .init(generation: 12, decks: harness.snapshot.decks)
        let picker = DeckPickerHarness.makeModel(harness)
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.message == .unavailable && !picker.canSubmit)
        picker.submit(layout: .compact)
        #expect(harness.actions.isEmpty)
    }

    @Test func ownerAdmissionFilteredRefusalAndSidebarRetryUseExistingSelection() async throws {
        let owner = await makeOwner()
        #expect(!owner.canPresentGoToDeck(host: .settings))
        owner.entry.pickerPresented = true
        #expect(!owner.canPresentGoToDeck())
        owner.entry.pickerPresented = false
        owner.navigation.present(.collectionExport)
        #expect(!owner.canPresentGoToDeck())
        owner.navigation.dismissSheet()
        owner.navigation.startReview(ReviewPresentation(deck: DeckID(42), session: NSObject()))
        #expect(!owner.canPresentGoToDeck())
        owner.navigation.endReview()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        var saved: [Int64] = []
        owner.saveSelectedDeck = { saved.append($0); throw BridgeError.internal("private diagnostic") }
        let filtered = DeckNameEntry(id: 9, fullName: "Filtered", filtered: true)
        #expect(await owner.activateGoToDeck(filtered, generation: 11, presentation: token, layout: .compact).value == .filtered)
        #expect(saved.isEmpty && owner.navigation.review == nil)
        let entry = DeckNameEntry(id: 42, fullName: "Japanese::Lesson1", filtered: false)
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .sidebar).value == .failed)
        #expect(owner.navigation.sheet == .goToDeck && owner.navigation.selectedDeck == nil)
        owner.saveSelectedDeck = { saved.append($0); return 11 }
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .sidebar).value == .opened)
        #expect(saved == [42, 42])
        #expect(owner.navigation.homeDeckPage == DeckID(42) && owner.navigation.sheet == nil)
        #expect(owner.decks.first?.studyCollapsed == true) // No expansion to expose the child.
    }

    @Test func compactActivationKeepsPolicyAdmissionAndDoesNotDismissOnRefusal() async throws {
        let owner = await makeOwner()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        var calls = 0
        owner.saveSelectedDeck = { _ in calls += 1; return 11 }
        let entry = DeckNameEntry(id: 42, fullName: "Japanese::Lesson1", filtered: false)
        // A collection that cannot be prepared refuses study before any write (F5.3).
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .compact).value == .studyRefused)
        #expect(calls == 0 && owner.navigation.review == nil && owner.navigation.sheet == .goToDeck)
    }

    @Test(arguments: ["route", "generation", "intent"])
    func ownerHeldSelectionCannotPublishIntoNewContext(change: String) async throws {
        let owner = await makeOwner(), gate = PickerGate()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        var calls = 0
        owner.saveSelectedDeck = { _ in calls += 1; await gate.hold(); return 11 }
        let entry = DeckNameEntry(id: 42, fullName: "Japanese::Lesson1", filtered: false)
        let activation = owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .sidebar)
        #expect(owner.goToDeckBusy && !owner.canStudy && !owner.manualAllowed && !owner.exportAllowed)
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .sidebar).value == .unavailable)
        await gate.entered()
        switch change {
        case "route": owner.navigation.present(.collectionExport)
        case "generation": owner.receiveLifecycle(.ready(generation: 12))
        default: owner.navigation.show(.settings)
        }
        gate.release()
        #expect(await activation.value != .opened)
        #expect(calls == 1 && owner.navigation.selectedDeck == nil)
        if change == "route" { #expect(owner.navigation.sheet == .collectionExport) }
    }

    @Test func ownerRejectsWrongHostAndOldPresentationBeforeDispatch() async throws {
        let owner = await makeOwner()
        owner.presentGoToDeck()
        let old = try #require(owner.goToDeckPresentation)
        owner.navigation.sheetHost = .settings
        let entry = DeckNameEntry(id: 42, fullName: "Japanese::Lesson1", filtered: false)
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: old, layout: .sidebar).value == .unavailable)
        owner.navigation.sheetHost = .main
        #expect(await owner.cancelGoToDeck(old).value)
        owner.presentGoToDeck()
        #expect(owner.goToDeckPresentation != old)
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: old, layout: .sidebar).value == .unavailable)
        #expect(owner.navigation.sheet == .goToDeck)
    }

    private func makeOwner() async -> ReviewCollection {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        let counts = ReviewCounts(new: 2, learning: 3, review: 4)
        let child = CountedDeckNode(id: 42, name: "Lesson1", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        let parent = CountedDeckNode(id: 10, name: "Japanese", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [child])
        owner.loadDeckTree = { .init(generation: 11, nextChangeAt: Int64.max, nodes: [parent]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}

@MainActor private final class DeckPickerHarness {
    var snapshot = DeckNameSnapshot(generation: 11, decks: [
        .init(id: 1, fullName: "Default", filtered: false),
        .init(id: 9, fullName: "Filtered", filtered: true),
        .init(id: 42, fullName: "Japanese::Lesson1", filtered: false),
        .init(id: 43, fullName: "Japanese::Lesson11", filtered: false),
        .init(id: 91, fullName: "日本語::初級::猫", filtered: false),
    ])
    var generation: UInt64 = 11
    var active = true
    var outcome: ReviewCollection.GoToDeckResult = .opened
    var actions: [(DeckNameEntry, ShellLayout)] = []
    var operationGate: PickerGate?
    var read: (@MainActor () async throws -> DeckNameSnapshot)?
    var lookupName: (@MainActor (String) async throws -> DeckNameMatch)?
    var submittedNames: [String] = []
    var observedLayouts: [ShellLayout] = []

    static func makeModel(_ harness: DeckPickerHarness) -> GoToDeckModel {
        GoToDeckModel(read: { try await harness.read?() ?? harness.snapshot },
            isCurrent: { token, epoch in harness.active && token == 1 && epoch == harness.generation },
            activate: { row, _, _, arrangement in
                harness.actions.append((row, arrangement))
                return Task {
                    await harness.operationGate?.hold()
                    return harness.outcome
                }
            }, dismiss: { _ in
                harness.active = false
                return Task { true }
            }, matchName: harness.lookupName,
            create: { text, _, _, _, arrangement in
                harness.submittedNames.append(text)
                return Task {
                    await harness.operationGate?.hold()
                    harness.observedLayouts.append(arrangement())
                    return harness.outcome
                }
            })
    }
}

@MainActor private final class PickerGate {
    private var held = false
    private var entry: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    func entered() async {
        if held { return }
        await withCheckedContinuation { entry = $0 }
    }
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; held = true; entry?.resume(); entry = nil
        }
    }
    func release() { continuation?.resume(); continuation = nil }
}
