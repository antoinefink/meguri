import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor @Suite struct FilteredStudyModelTests {
    @Test func creatingAndCancellingIsInertAndScopesLiteralDeckNames() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.create(deck: 1); await model.waitForOperation()
        #expect(model.definition.terms == [.init(search: service.scope + " is:due", limit: 100, order: 1)])
        #expect(model.secondTerm == .init(search: service.scope + " is:new", limit: 20, order: 6))
        #expect(service.generations == [11])
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
            model.againText = "17"; model.hardText = "83"; model.goodText = "0"
            model.submit(); model.submit(); await model.waitForOperation()
            #expect(service.saved.count == Int(order) + 1)
            #expect(service.saved.last?.terms.count == 2)
            #expect(service.saved.last?.againSeconds == 17)
            #expect(service.saved.last?.hardSeconds == 83)
            #expect(service.saved.last?.goodSeconds == 0)
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

    @Test(arguments: [0, 1], [-5, 5])
    func committedDailyAdjustmentRefreshesWithoutRepeatingSignedWrite(mode: Int, amount: Int) async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.custom(1); await model.waitForOperation()
        model.modeIndex = mode; model.amountText = String(amount)
        service.customError = .commitCompleted(sequence: 42, message: "Saved result unavailable")
        var reads = 0, selections = 0
        owner.loadDeckTree = { reads += 1; throw BridgeError.internal("Read unavailable") }
        owner.saveSelectedDeck = { _ in selections += 1; return 11 }
        model.submit(); model.submit(); await model.waitForOperation()
        #expect(service.customRequests.count == 1 && service.customRequests[0].amount == Int32(amount))
        #expect(reads == 1 && model.failure == .refresh && model.needsRefresh && !model.canSubmit)
        model.submit(); model.confirmEmpty(); model.load(); await model.waitForOperation()
        #expect(service.writes == 1 && service.defaultsReads == 1 && reads == 1)
        owner.loadDeckTree = { reads += 1; return Self.tree(generation: 11, review: 19) }
        model.refresh(); await model.waitForOperation()
        #expect(reads >= 2 && owner.deck(1)?.counts.review == 19)
        #expect(model.failure == .saved && !model.needsRefresh && !model.canSubmit)
        #expect(owner.navigation.sheet == .customStudy(1) && selections == 0)
        model.submit(); model.confirmEmpty(); model.refresh(); await model.waitForOperation()
        #expect(service.writes == 1 && selections == 0)
        model.cancel()
        #expect(owner.navigation.sheet == nil)
    }

    @Test(arguments: [0, 1], [-5, 5])
    func precommitDailyAdjustmentFailurePreservesEditableDraftAndAllowsRetry(mode: Int, amount: Int) async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.custom(1); await model.waitForOperation()
        model.modeIndex = mode; model.amountText = String(amount)
        model.includeTags = "keep this tag"
        service.customError = .invalidArgument("Rejected before commit")
        var reads = 0
        owner.loadDeckTree = { reads += 1; return Self.tree(generation: 11) }
        model.submit(); await model.waitForOperation()
        #expect(model.failure == .write && model.canSubmit && !model.needsRefresh && reads == 0)
        #expect(model.amountText == String(amount) && model.includeTags == "keep this tag")
        model.amountText = String(amount + 1)
        model.submit(); await model.waitForOperation()
        #expect(service.customRequests.map(\.amount) == [Int32(amount), Int32(amount + 1)])
        #expect(model.failure == .write && model.canSubmit && reads == 0)
    }

    @Test(arguments: [false, true])
    func committedDefinitionRefreshesWithoutGuessingADeckIdentity(editing: Bool) async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        if editing { model.edit(3); await model.waitForOperation() }
        else { model.create(search: "is:due") }
        model.definition.name = "Saved definition"
        service.saveError = .commitCompleted(sequence: 43, message: "Saved result unavailable")
        var selections = 0
        owner.saveSelectedDeck = { _ in selections += 1; return 11 }
        owner.loadDeckTree = { Self.tree(generation: 11, review: 23) }
        model.submit(); await model.waitForOperation()
        #expect(service.writes == 1 && owner.deck(3)?.counts.review == 23)
        #expect(model.failure == .saved && !model.canSubmit && !model.canSaveEmpty && !model.needsRefresh)
        #expect(model.definition.name == "Saved definition" && owner.navigation.sheet == .filteredStudy)
        #expect(selections == 0 && owner.navigation.homeDeckPage == nil)
        model.submit(); model.confirmEmpty(); model.load(); await model.waitForOperation()
        #expect(service.writes == 1 && selections == 0)
    }

    @Test(arguments: [false, true])
    func committedEmptyAndRebuildRefreshTheirRealDeckState(empty: Bool) async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        service.mutationError = .commitCompleted(sequence: 44, message: "Saved result unavailable")
        owner.loadDeckTree = { Self.tree(generation: 11, review: 0) }
        if empty { model.empty(3) } else { model.rebuild(3) }
        await model.waitForOperation()
        #expect(owner.deck(3)?.counts.review == 0 && !model.operationFailed)
        #expect(service.emptied.count + service.rebuilt.count == 1)
    }

    @Test func dailyAdjustmentBoundsIncludeZeroAndRefuseAdjacentValuesInBothForms() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.custom(1); await model.waitForOperation()
        for mode in 0...1 {
            model.modeIndex = mode
            for amount in [-99_999, 0, 99_999] {
                model.amountText = String(amount)
                #expect(model.canSubmit && model.customRequest.amount == Int32(amount))
            }
            for amount in [-100_000, 100_000, Int32.min, Int32.max] {
                model.amountText = String(amount)
                #expect(!model.canSubmit)
                let writes = service.writes
                model.submit(); await model.waitForOperation()
                #expect(service.writes == writes && model.amountText == String(amount))
            }
        }
    }

    @Test func emptyConsentRequiresNoMatchesForTheExactDraftAndIsNeverImplicit() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        service.refuse = false
        model.edit(3); await model.waitForOperation()
        model.definition.name = "Empty candidate"
        model.confirmEmpty(); await model.waitForOperation()
        #expect(service.writes == 0 && !model.canSaveEmpty)
        service.saveError = .invalidArgument("invalid search")
        model.submit(); await model.waitForOperation()
        #expect(model.failure == .write && !model.canSaveEmpty)
        service.saveError = .deckRefused(.noMatches)
        model.submit(); await model.waitForOperation()
        #expect(model.failure == .noMatches && model.canSaveEmpty)
        #expect(model.definition.name == "Empty candidate" && model.usesSecondTerm)
        #expect(owner.navigation.sheet == .filteredStudy && service.emptyConsents == [false, false])
        model.secondTerm.search = "changed"
        #expect(!model.canSaveEmpty)
        model.confirmEmpty(); await model.waitForOperation()
        #expect(service.writes == 2)
        model.submit(); await model.waitForOperation()
        #expect(model.canSaveEmpty)
        model.confirmEmpty(); model.confirmEmpty(); await model.waitForOperation()
        #expect(service.emptyConsents == [false, false, false, true])
        #expect(service.saved.last?.terms.last?.search == "changed")
    }

    @Test func replacementRenewsTheModelAndRoutesNewOperationsToItsGeneration() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), first = owner.filteredStudy
        first.edit(3); await first.waitForOperation()
        owner.receiveLifecycle(.ready(generation: 12))
        owner.loadDeckTree = { Self.tree(generation: 12) }
        await owner.loadCounts()
        let next = owner.filteredStudy
        #expect(next !== first && !first.loaded)
        let previousCalls = service.generations.count
        next.edit(3); await next.waitForOperation(); next.cancel()
        next.custom(1); await next.waitForOperation(); next.cancel()
        next.empty(3); await next.waitForOperation()
        next.rebuild(3); await next.waitForOperation()
        #expect(service.generations.dropFirst(previousCalls) == [12, 12, 12, 12])
    }

    @Test func browseRequiresAnAvailableDestinationAndUsesTheCoreScope() async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        model.browseCards(1); await model.waitForOperation()
        #expect(service.generations.isEmpty && owner.navigation.section != .browse)
        owner.deckImportBrowseAvailable = true
        model.browseCards(1); await model.waitForOperation()
        #expect(owner.navigation.section == .browse && owner.navigation.browseSearch == service.scope)
    }

    @Test(arguments: ["current", "navigation", "sheet", "notReady", "replacement", "invalidated", "unavailable"], [false, true])
    func browseCompletionRequiresItsCurrentReadyRoute(change: String, failing: Bool) async {
        let service = FilteredStudySpy(), owner = await makeOwner(service), model = owner.filteredStudy
        let gate = FilteredStudyGate(); service.scopeGate = gate
        service.scopeError = failing ? .internal("Scope read failed") : nil
        owner.deckImportBrowseAvailable = true
        model.browseCards(1); await gate.entered()
        #expect(model.busy && service.generations == [11])
        switch change {
        case "navigation": owner.navigation.show(.settings)
        case "sheet": owner.navigation.present(.collectionExport)
        case "notReady": owner.receiveLifecycle(.opening)
        case "replacement": owner.receiveLifecycle(.ready(generation: 12))
        case "invalidated": model.invalidate()
        case "unavailable": owner.deckImportBrowseAvailable = false
        default: break
        }
        gate.release(); await model.waitForOperation()
        #expect(!model.busy)
        #expect(model.operationFailed == (change == "current" && failing))
        #expect((owner.navigation.section == .browse) == (change == "current" && !failing))
        if change != "current" || failing { #expect(owner.navigation.browseSearch.isEmpty) }
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
    private static func tree(generation: UInt64, review: UInt32 = 4) -> CountedDeckTree {
        let counts = ReviewCounts(new: 0, learning: 0, review: review)
        return .init(generation: generation, nextChangeAt: Int64.max, nodes: [
            .init(id: 1, name: "Source*_\"::OR", filtered: false, studyCollapsed: false, browserCollapsed: false, counts: counts, children: []),
            .init(id: 2, name: "Second", filtered: false, studyCollapsed: false, browserCollapsed: false, counts: counts, children: []),
            .init(id: 3, name: "Filtered", filtered: true, studyCollapsed: false, browserCollapsed: false, counts: counts, children: [])
        ])
    }
}

@MainActor private final class FilteredStudySpy: FilteredStudyService {
    var scope = "did:1,4"
    var generations: [UInt64] = []
    var emptyConsents: [Bool] = []
    var saveError: BridgeError?
    var customError: BridgeError?
    var mutationError: BridgeError?
    var scopeGate: FilteredStudyGate?
    var scopeError: BridgeError?
    func deckScope(deck: Int64, generation: UInt64) async throws(BridgeError) -> String {
        generations.append(generation)
        await scopeGate?.hold()
        if let scopeError { throw scopeError }
        return scope
    }
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
    func definition(deck: Int64, generation: UInt64) async throws(BridgeError) -> FilteredStudyDefinition {
        generations.append(generation)
        await readGate?.hold(); return value
    }
    func save(deck: Int64?, definition: FilteredStudyDefinition, allowEmpty: Bool, generation: UInt64) async throws(BridgeError) -> Int64 {
        generations.append(generation); emptyConsents.append(allowEmpty)
        saved.append(definition)
        if let saveError, !allowEmpty { throw saveError }
        if let saveGate { await saveGate.hold(); return 3 }
        if refuse { throw .invalidArgument("zero matches or invalid definition") }
        return 3
    }
    func rebuild(deck: Int64, generation: UInt64) async throws(BridgeError) {
        generations.append(generation); rebuilt.append(deck)
        if let mutationError { throw mutationError }
    }
    func empty(deck: Int64, generation: UInt64) async throws(BridgeError) {
        generations.append(generation); emptied.append(deck)
        if let mutationError { throw mutationError }
    }
    func custom(deck: Int64, request: CustomStudyRequest, generation: UInt64) async throws(BridgeError) -> Int64 {
        generations.append(generation); customRequests.append(request)
        if let customError { throw customError }
        if refuse { throw .invalidArgument("zero matches") }
        return request.mode == .increaseNew || request.mode == .increaseReview ? deck : 3
    }
    func customDefaults(deck: Int64, generation: UInt64) async throws(BridgeError) -> CustomStudyDefaults {
        generations.append(generation); defaultsReads += 1
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

// Lifecycle work uses the serialized real-service owner, in a disposable synthetic
// collection, so a background transition never touches another suite’s SDK.
extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func browseFailureWhileHiddenOrResumingNeverPublishesAnAlert(resume: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let service = FilteredStudySpy(), gate = FilteredStudyGate()
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            providerDirectory: root.appending(path: "synthetic-installation"), filteredStudyService: service)
        do {
            await owner.start()
            owner.entry.inspect(try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg")))
            await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion(); await owner.studyPreparation.waitForJob()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            owner.navigation.dismissSheet()
            owner.deckImportBrowseAvailable = true
            service.scopeGate = gate; service.scopeError = .internal("Scope read failed")
            let model = owner.filteredStudy
            model.browseCards(1); await gate.entered()
            owner.setForeground(false)
            if resume { owner.setForeground(true) }
            #expect(resume ? owner.resuming : !owner.foreground)
            gate.release(); await model.waitForOperation()
            #expect(!model.operationFailed && owner.navigation.section != .browse)
            await owner.waitForLifecycle()
            #expect(!model.operationFailed, "The suppressed failure cannot appear after lifecycle completion")
            #expect(await owner.close())
        } catch {
            gate.release(); await owner.filteredStudy.waitForOperation()
            _ = await owner.close()
            throw error
        }
    }
}
