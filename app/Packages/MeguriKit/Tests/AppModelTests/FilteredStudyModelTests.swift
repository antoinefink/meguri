import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor @Suite struct FilteredStudyModelTests {
    @Test func creatingAndCancellingIsInertAndScopesLiteralDeckNames() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.create(deck: 1)
        #expect(model.definition.terms == [.init(search: "deck:\"Source\\*\\_\\\"::OR\" is:due", limit: 100, order: 1)])
        #expect(model.secondTerm == .init(search: "deck:\"Source\\*\\_\\\"::OR\" is:new", limit: 20, order: 6))
        #expect(model.definition.reschedule && !model.usesSecondTerm)
        #expect(model.definition.againSeconds == 60 && model.definition.hardSeconds == 600 && model.definition.goodSeconds == 0)
        model.definition.name = "Draft"
        owner.navigation.adoptLaunchLayout(.compact)
        #expect(owner.filteredStudy === model && model.definition.name == "Draft")
        #expect(owner.navigation.sheet == .filteredStudy)
        model.cancel()
        #expect(owner.navigation.sheet == nil && service.writes == 0)
        model.create(search: "(tag:one OR tag:two) -is:suspended")
        #expect(model.definition.terms[0].search == "(tag:one OR tag:two) -is:suspended")
        model.cancel()
        #expect(service.writes == 0)
    }

    @Test func incompleteAndOverflowingNumbersRemainVisibleAndCannotSubmit() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.create(); model.definition.name = "Valid name"
        model.firstLimitText = "-"; #expect(!model.canSubmit && model.firstLimitText == "-")
        model.firstLimitText = "4294967296"; #expect(!model.canSubmit)
        model.firstLimitText = "0"; #expect(!model.canSubmit)
        model.firstLimitText = "2"; #expect(model.canSubmit)
        model.definition.reschedule = false
        model.againText = "1.5"; #expect(!model.canSubmit && model.againText == "1.5")
        model.againText = "0"; #expect(model.canSubmit && model.definition.againSeconds == 0)
        model.cancel(); model.custom(1); await model.waitForOperation()
        model.amountText = "-"; #expect(!model.canSubmit)
        model.amountText = "-5"; #expect(model.canSubmit && model.customRequest.amount == -5)
        #expect(FilteredStudyModel.deckSearch("current") == "deck:\"Current\"")
        #expect(FilteredStudyModel.deckSearch("filtered") == "deck:\"Filtered\"")
    }

    @Test func editorKeepsBothTermsAllOrderCodesAndDelaysOnFailure() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        for order in UInt32(0)...10 {
            service.value.terms[0].order = order
            model.edit(3); await model.waitForOperation()
            #expect(model.loaded && model.usesSecondTerm)
            #expect(model.definition.terms.count == 1 && model.definition.terms[0].order == order)
            #expect(model.secondTerm == service.value.terms[1])
            model.definition.name = "Kept name"
            model.definition.reschedule = false
            model.definition.againSeconds = 17; model.definition.hardSeconds = 83; model.definition.goodSeconds = 0
            model.submit(); model.submit(); await model.waitForOperation()
            #expect(service.saved.count == Int(order) + 1)
            #expect(service.saved.last?.terms.count == 2)
            #expect(service.saved.last?.hardSeconds == 83)
            #expect(model.failure == .write && model.definition.name == "Kept name")
            #expect(model.canSubmit && owner.navigation.sheet == .filteredStudy)
            model.cancel()
        }
    }

    @Test func customReadsRealDefaultsAndMapsEveryPresetAndStateWithoutParsingTags() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.custom(1); await model.waitForOperation()
        #expect(model.defaults?.availableNew == 731 && model.defaults?.availableReview == 82)
        #expect(model.amount == 14 && model.includeTags == "language::a*b")
        let modes: [CustomStudyMode] = [.increaseNew, .increaseReview, .forgotten, .ahead, .previewNew, .byState]
        for (index, mode) in modes.enumerated() {
            model.modeIndex = index
            #expect(model.customRequest.mode == mode)
            if index == 1 { #expect(model.amount == 27) }
            if index > 1 && index < 5 { #expect(model.amount == 1) }
        }
        #expect(model.amount == 100)
        let states: [CustomStudyState] = [.new, .due, .review, .all]
        for (index, state) in states.enumerated() {
            model.stateIndex = index
            #expect(model.customRequest.state == state)
        }
        model.includeTags = "tag with spaces\nword\" OR *::child"
        model.excludeTags = "-is:new\nother\\tag"
        #expect(model.customRequest.includeTags == ["tag with spaces", "word\" OR *::child"])
        #expect(model.customRequest.excludeTags == ["-is:new", "other\\tag"])
        model.submit(); await model.waitForOperation()
        #expect(model.failure == .write && model.includeTags == "tag with spaces\nword\" OR *::child")
        model.modeIndex = 2; model.amount = 31; #expect(!model.canSubmit)
        model.amount = 0; #expect(!model.canSubmit)
        model.amount = 30; #expect(model.canSubmit)
        model.modeIndex = 0; model.amount = -4; #expect(model.canSubmit && model.customRequest.amount == -4)
        model.cancel()
        #expect(service.defaultsReads == 1 && service.customRequests.count == 1)
    }

    @Test func duplicateWriteIsReservedAndAChangedCollectionRejectsTheLateResult() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        let gate = FilteredStudyGate(); service.saveGate = gate
        model.create(search: "is:due"); model.definition.name = "One operation"
        model.submit(); model.submit()
        #expect(model.busy && !model.canSubmit && !owner.canStudy)
        await gate.entered()
        #expect(service.writes == 1)
        owner.receiveLifecycle(.ready(generation: 12))
        gate.release(); await model.waitForOperation()
        #expect(owner.navigation.sheet == nil && model.failure == nil && !model.operationFailed)
        #expect(owner.navigation.selectedDeck == nil)
    }

    @Test func laterNavigationWinsOverACommittedSaveAndLateRead() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        let gate = FilteredStudyGate(); service.saveGate = gate
        model.create(search: "is:due"); model.definition.name = "Saved elsewhere"
        model.submit(); await gate.entered()
        owner.navigation.show(.browse)
        gate.release(); await model.waitForOperation()
        #expect(owner.navigation.section == .browse && owner.navigation.selectedDeck == nil)
        #expect(model.failure == nil && !model.operationFailed)
        model.cancel()
        let readGate = FilteredStudyGate(); service.readGate = readGate
        model.edit(3); await readGate.entered()
        owner.navigation.collectionReplaced()
        readGate.release(); await model.waitForOperation()
        #expect(!model.loaded && owner.navigation.sheet == nil)
    }

    @Test func savedResultRetriesReadsWithoutRepeatingTheWriteAndSelectsTheDeck() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        service.refuse = false
        model.create(search: "is:due"); model.definition.name = "Created"
        owner.loadDeckTree = { throw BridgeError.internal("read failed") }
        model.submit(); await model.waitForOperation()
        #expect(model.failure == .refresh && model.needsRefresh && !model.canSubmit && service.writes == 1)
        owner.loadDeckTree = { Self.tree(generation: 11) }
        model.refresh(); await model.waitForOperation()
        #expect(service.writes == 1 && owner.navigation.sheet == nil)
        #expect(owner.navigation.homeDeckPage == DeckID(3))
    }

    @Test func filteredDeckStudyAndSettingsUseTheNormalNavigationRoutes() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service)
        #expect(owner.offersStudy(deckID: 3))
        owner.openDeckOptions(deckID: 3)
        await owner.filteredStudy.waitForOperation()
        #expect(owner.navigation.sheet == .filteredStudy && owner.filteredStudy.editingDeck == 3)
        #expect(!owner.navigation.showsDeckOptions)
        owner.filteredStudy.cancel()
        owner.filteredStudy.empty(3); await owner.filteredStudy.waitForOperation()
        owner.filteredStudy.rebuild(3); await owner.filteredStudy.waitForOperation()
        #expect(service.emptied == [3] && service.rebuilt == [3])
        #expect(!owner.filteredStudy.canCustom(3))
        #expect(!owner.deckActions.canPresent(.subdeck(3)))
    }

    private func makeOwner(_ service: FilteredStudySpy) async -> ReviewCollection {
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), filteredStudyService: service)
        owner.loadDeckTree = { Self.tree(generation: 11) }
        owner.saveSelectedDeck = { _ in 11 }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
    private static func tree(generation: UInt64) -> CountedDeckTree {
        let counts = ReviewCounts(new: 0, learning: 0, review: 4)
        return .init(generation: generation, nextChangeAt: Int64.max, nodes: [
            .init(id: 1, name: "Source*_\"::OR", filtered: false, studyCollapsed: false, browserCollapsed: false, counts: counts, children: []),
            .init(id: 2, name: "Second", filtered: false, studyCollapsed: false, browserCollapsed: false, counts: counts, children: []),
            .init(id: 3, name: "Filtered", filtered: true, studyCollapsed: false, browserCollapsed: false, counts: counts, children: [])
        ])
    }
}

@MainActor private final class FilteredStudySpy: FilteredStudyService {
    var refuse = true
    var saveGate: FilteredStudyGate?
    var readGate: FilteredStudyGate?
    var saved: [FilteredStudyDefinition] = []
    var customRequests: [CustomStudyRequest] = []
    var rebuilt: [Int64] = []
    var emptied: [Int64] = []
    var defaultsReads = 0
    var writes: Int { saved.count + customRequests.count }
    var value = FilteredStudyDefinition(name: "Imported", terms: [
        .init(search: "is:due", limit: 100, order: 1), .init(search: "is:new", limit: 20, order: 6)
    ], reschedule: false, againSeconds: 13, hardSeconds: 97, goodSeconds: 7)
    func definition(deck: Int64) async throws(BridgeError) -> FilteredStudyDefinition {
        await readGate?.hold(); return value
    }
    func save(deck: Int64?, definition: FilteredStudyDefinition) async throws(BridgeError) -> Int64 {
        saved.append(definition)
        if let saveGate { await saveGate.hold(); return 3 }
        if refuse { throw .invalidArgument("zero matches or invalid definition") }
        return 3
    }
    func rebuild(deck: Int64) async throws(BridgeError) { rebuilt.append(deck) }
    func empty(deck: Int64) async throws(BridgeError) { emptied.append(deck) }
    func custom(deck: Int64, request: CustomStudyRequest) async throws(BridgeError) -> Int64 {
        customRequests.append(request)
        if refuse { throw .invalidArgument("zero matches") }
        return request.mode == .increaseNew || request.mode == .increaseReview ? deck : 3
    }
    func customDefaults(deck: Int64) async throws(BridgeError) -> CustomStudyDefaults {
        defaultsReads += 1
        return .init(availableNew: 731, availableReview: 82, extendNew: 14, extendReview: 27,
                     includeTags: ["language::a*b"], excludeTags: ["draft"])
    }
}

@MainActor private final class FilteredStudyGate {
    private var started = false
    private var entry: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    func entered() async {
        if started { return }
        await withCheckedContinuation { entry = $0 }
    }
    func hold() async {
        started = true; entry?.resume(); entry = nil
        await withCheckedContinuation { continuation = $0 }
    }
    func release() { continuation?.resume(); continuation = nil }
}
