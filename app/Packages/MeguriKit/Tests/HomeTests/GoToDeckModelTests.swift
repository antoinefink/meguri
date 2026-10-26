import CoreBridge
import Foundation
import Testing
@testable import AppModel
@testable import Home

@MainActor
@Suite struct GoToDeckModelTests {
    @Test func snapshotOrderLiteralNamesAndActualIDSelection() async {
        let fixture = PickerFixture()
        let picker = PickerFixture.makeModel(fixture)
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.matches == fixture.snapshot.decks)
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
        #expect(fixture.actions.isEmpty)
        picker.query = "Default"
        picker.selectedID = 42 // A stale native selection cannot submit an invisible row.
        #expect(!picker.canSubmit)
        picker.submit(layout: .sidebar)
        #expect(fixture.actions.isEmpty)
    }

    @Test func failureRetainsDraftSelectionAndRetryUsesSameIDWithLiveLayout() async {
        let fixture = PickerFixture()
        fixture.outcome = .failed
        let picker = PickerFixture.makeModel(fixture)
        await picker.load(presentation: 1, generation: 11)
        picker.query = "ja 1"; picker.selectedID = 43
        picker.submit(layout: .sidebar)
        await picker.waitForActivation()
        #expect(picker.message == .activationFailed)
        #expect(picker.query == "ja 1" && picker.selectedID == 43)
        fixture.outcome = .opened
        picker.submit(layout: .compact)
        await picker.waitForActivation()
        #expect(fixture.actions.map { $0.0.id } == [43, 43])
        #expect(fixture.actions.map { $0.1 } == [.sidebar, .compact])
        #expect(fixture.actions.last?.0.fullName == "Japanese::Lesson11")
    }

    @Test func loadingRetryKeepsTypedQueryAndNeverPublishesRawDiagnostics() async {
        let fixture = PickerFixture(), gate = PickerGate()
        fixture.read = { await gate.hold(); throw BridgeError.internal("private diagnostic") }
        let picker = PickerFixture.makeModel(fixture)
        let first = Task { await picker.load(presentation: 1, generation: 11) }
        await gate.entered()
        picker.query = "猫"
        #expect(picker.loading && !picker.canSubmit)
        gate.release(); await first.value
        #expect(picker.message == .loadFailed && picker.snapshot == nil)
        fixture.read = { fixture.snapshot }
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.query == "猫" && picker.selectedID == 91)
        #expect(picker.message == nil)
    }

    @Test func repeatedSubmitIsReservedBeforeWaitingAndNewQuerySurvivesLateFailure() async {
        let fixture = PickerFixture(), gate = PickerGate()
        fixture.actionGate = gate; fixture.outcome = .failed
        let picker = PickerFixture.makeModel(fixture)
        await picker.load(presentation: 1, generation: 11)
        picker.query = "ja 1"; picker.selectedID = 43
        picker.submit(layout: .sidebar); picker.submit(layout: .sidebar)
        #expect(picker.busy && fixture.actions.count == 1)
        await gate.entered()
        picker.query = "猫"
        picker.submit(layout: .compact)
        gate.release(); await picker.waitForActivation()
        #expect(fixture.actions.count == 1)
        #expect(picker.query == "猫" && picker.selectedID == 91 && picker.message == nil)
    }

    @Test(arguments: [false, true])
    func lateReadCannotReplaceNewerSnapshotOrPublishAfterDismissal(reload: Bool) async {
        let fixture = PickerFixture(), gate = PickerGate()
        let old = fixture.snapshot
        fixture.read = { await gate.hold(); return old }
        let picker = PickerFixture.makeModel(fixture)
        let first = Task { await picker.load(presentation: 1, generation: 11) }
        await gate.entered()
        picker.query = "猫"
        if reload {
            fixture.snapshot = DeckNameSnapshot(generation: 11, decks: [.init(id: 90, fullName: "猫::新", filtered: false)])
            fixture.read = { fixture.snapshot }
            await picker.load(presentation: 1, generation: 11)
        } else {
            picker.cancel()
            await picker.waitForActivation()
        }
        gate.release(); await first.value
        #expect(picker.query == "猫")
        #expect(picker.snapshot == (reload ? fixture.snapshot : nil))
        #expect(picker.selectedID == (reload ? 90 : nil))
        #expect(picker.message == nil)
    }

    @Test func ownerLifecycleInvalidatesHeldRead() async throws {
        let owner = await makeOwner(), fixture = PickerFixture(), gate = PickerGate()
        owner.presentGoToDeck()
        let token = try #require(owner.goToDeckPresentation)
        let picker = GoToDeckModel(read: { await gate.hold(); return fixture.snapshot },
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
        let fixture = PickerFixture()
        fixture.snapshot = .init(generation: 12, decks: fixture.snapshot.decks)
        let picker = PickerFixture.makeModel(fixture)
        await picker.load(presentation: 1, generation: 11)
        #expect(picker.message == .unavailable && !picker.canSubmit)
        picker.submit(layout: .compact)
        #expect(fixture.actions.isEmpty)
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
        #expect(await owner.activateGoToDeck(entry, generation: 11, presentation: token, layout: .compact).value == .failed)
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

@MainActor private final class PickerFixture {
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
    var actionGate: PickerGate?
    var read: (@MainActor () async throws -> DeckNameSnapshot)?

    static func makeModel(_ fixture: PickerFixture) -> GoToDeckModel {
        GoToDeckModel(read: { try await fixture.read?() ?? fixture.snapshot },
            isCurrent: { token, generation in fixture.active && token == 1 && generation == fixture.generation },
            activate: { entry, _, _, layout in
                fixture.actions.append((entry, layout))
                return Task { await fixture.actionGate?.hold(); return fixture.outcome }
            }, dismiss: { _ in fixture.active = false; return Task { true } })
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
