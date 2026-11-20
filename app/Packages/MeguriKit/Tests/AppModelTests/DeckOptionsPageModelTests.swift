@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// The first version's Deck Options page (`sched/deck-options` O17,
/// `native-interface-contract` N20.9) against a scripted core draft.
@MainActor @Suite struct DeckOptionsPageModelTests {
    @Test func openShowsTheSnapshotAndTheFootersFacts() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        #expect(model.canOpen(deckID: 1) && !model.canOpen(deckID: 3))
        model.open(deckID: 1)
        #expect(model.phase == .loading && model.deckName == "Parent")
        await model.waitForCompletion()
        #expect(model.phase == .editing && model.selectedPreset == 1 && !model.hasChanges)
        #expect(model.fieldText == [.newPerDay: "20", .reviewsPerDay: "200", .desiredRetention: "90%"])
        #expect(model.ownLimits && model.ownRetention)
        // O6.1: Default is stored by 6 decks, this one included; Solo by one other deck.
        let presets = model.presets
        #expect(presets.map(model.useCount) == [6, 1])
        #expect(model.selectedUseCount == 6)
        model.selectPreset(2)
        #expect(presets.map(model.useCount) == [5, 2] && model.selectedUseCount == 2)
        #expect(model.fieldText[.newPerDay] == "10" && model.hasChanges)
        await model.waitForCompletion()
        #expect(core.session?.calls == ["select 2"])
        // A deck alone in its preset: the footer's N is 1.
        let alone = await makeOwner(stored: [1, 0], ownValues: false)
        alone.0.deckOptions.open(deckID: 1); await alone.0.deckOptions.waitForCompletion()
        #expect(alone.0.deckOptions.selectedUseCount == 1)
        #expect(!alone.0.deckOptions.ownLimits && !alone.0.deckOptions.ownRetention)
    }

    @Test func outOfRangeValuesStayInTheDraftAndDisableSave() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.newPerDay] = "10000"; model.commit(.newPerDay)
        await model.waitForCompletion()
        #expect(model.value(.newPerDay) == 10000 && model.fieldText[.newPerDay] == "10,000")
        #expect(model.issues(for: .newPerDay).map(\.code) == [.outOfRange] && !model.canSave)
        model.fieldText[.newPerDay] = "2.5"; model.commit(.newPerDay)
        await model.waitForCompletion()
        #expect(model.issues(for: .newPerDay).map(\.code) == [.notWhole] && !model.canSave)
        model.fieldText[.desiredRetention] = "65%"; model.commit(.desiredRetention)
        await model.waitForCompletion()
        #expect(model.value(.desiredRetention) == 0.65)
        #expect(model.issues(for: .desiredRetention).map(\.code) == [.outOfRange, .retentionLow])
        // Emptied or non-numeric text changes nothing and shows the value again.
        for text in ["", "  ", "abc"] {
            model.fieldText[.reviewsPerDay] = text; model.commit(.reviewsPerDay)
            #expect(model.value(.reviewsPerDay) == 200 && model.fieldText[.reviewsPerDay] == "200")
        }
        model.fieldText[.newPerDay] = "15"; model.commit(.newPerDay)
        model.fieldText[.desiredRetention] = "85"; model.commit(.desiredRetention)
        await model.waitForCompletion()
        #expect(model.value(.desiredRetention) == 0.85 && model.fieldText[.desiredRetention] == "85%")
        #expect(model.issues.isEmpty && model.canSave)
    }

    @Test func warningsShowUnderTheirRowAndKeepSaveEnabled() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 50)
        model.setValue(.desiredRetention, 0.75)
        await model.waitForCompletion()
        #expect(model.issues(for: .reviewsPerDay).map(\.code) == [.reviewsBelowTenTimesNew])
        #expect(model.issues(for: .reviewsPerDay).first?.params["expected"] == "500")
        #expect(model.issues(for: .desiredRetention).map(\.severity) == [.warning])
        #expect(model.canSave)
    }

    @Test func saveSendsOnlyTheShownValuesAndThePresetSwitch() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 30)
        model.selectPreset(2)
        // O15.3: a typed value that is not committed yet is saved.
        model.fieldText[.reviewsPerDay] = "300"
        model.save()
        #expect(model.phase == .saving)
        await model.waitForCompletion()
        #expect(core.session?.calls == ["set newPerDay 1 30.0", "select 2", "set reviewsPerDay 2 300.0", "save"])
        #expect(model.outcome == .saved && model.phase == .closed)
        model.acknowledgeOutcome()
        #expect(model.outcome == nil)
    }

    @Test func aValueEqualToTheStoredF32IsNoChange() async throws {
        let (owner, _) = await makeOwner(retention: Double(Float(0.9)))
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(model.fieldText[.desiredRetention] == "90%")
        model.fieldText[.desiredRetention] = "90 %"; model.commit(.desiredRetention)
        #expect(!model.hasChanges)
        model.stepRetention(by: 1)
        #expect(model.value(.desiredRetention) == 0.91 && model.hasChanges)
        model.setValue(.desiredRetention, 0.99); model.stepRetention(by: 1)
        #expect(model.value(.desiredRetention) == 0.99)
        model.stepRetention(by: -1)
        #expect(model.value(.desiredRetention) == 0.98)
    }

    @Test func leavingAsksOnlyWhenTheDraftDiffers() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(model.requestLeave())
        #expect(model.outcome == .left && model.phase == .closed && core.session?.calls == [])
        model.acknowledgeOutcome()
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.newPerDay] = "21"
        #expect(!model.requestLeave())
        #expect(model.confirmingDiscard && model.value(.newPerDay) == 21)
        model.keepEditing()
        #expect(!model.confirmingDiscard && model.phase == .editing && model.outcome == nil)
        #expect(!model.requestLeave())
        model.discardChanges()
        #expect(model.outcome == .left && model.phase == .closed)
        await model.waitForCompletion()
        #expect(core.session?.calls.contains("save") == false)
    }

    @Test func changesElsewhereReloadACleanDraftAndKeepAChangedOne() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(core.opens == 1)
        model.changesCommitted(Self.changes(decks: [1]), generation: 11)
        await model.waitForCompletion()
        #expect(core.opens == 2 && model.phase == .editing && !model.changedElsewhere)
        // Unrelated rows (cards answered) leave the draft alone.
        model.changesCommitted(Self.changes(decks: []), generation: 11)
        #expect(core.opens == 2)
        model.setValue(.newPerDay, 25)
        model.changesCommitted(Self.changes(decks: [1]), generation: 11)
        #expect(model.changedElsewhere && model.value(.newPerDay) == 25 && core.opens == 2)
        // N20.9 has no reload row: Save is refused, says nothing was changed and reads anew.
        model.save(); await model.waitForCompletion()
        #expect(core.opens == 3 && model.saveFailure == .failed && !model.changedElsewhere && !model.hasChanges)
        // A save the core refuses as stale does the same.
        model.setValue(.newPerDay, 26)
        core.saveResult = .failure(.stale(table: .deckConfig, key: "1"))
        model.save(); await model.waitForCompletion()
        #expect(core.opens == 4 && model.saveFailure == .failed && model.outcome == nil && !model.hasChanges)
    }

    @Test func thePageEndsWhenItLeavesItsStackAndAReopenKeepsAnOpenDraft() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        let original = try #require(model.value(.newPerDay))
        model.setValue(.newPerDay, 25)
        // The same deck again, or another deck, never replaces a changed draft.
        #expect(!model.canOpen(deckID: 1) && !model.canOpen(deckID: 2))
        // Text typed but not committed counts as a change (O15.3).
        model.setValue(.newPerDay, original)
        #expect(!model.hasChanges)
        model.fieldText[.newPerDay] = "40"
        #expect(model.hasChanges)
        // Back (the page leaves its stack) ends the draft in any phase.
        owner.navigation.setPath([], for: .home)
        #expect(model.phase == .closed && model.outcome == .left && core.opens == 1)
    }

    @Test func aDraftThatEndsWhileAnotherSectionShowsLeavesNoPageBehind() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        owner.navigation.show(.settings)
        model.discardChanges()
        #expect(model.phase == .closed && owner.navigation.path(.home).isEmpty)
    }

    @Test func refusedSavesKeepThePageWithItsIssues() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 40)
        let issue = OptionIssue(field: .newPerDay, preset: 1, code: .outOfRange, severity: .error, params: [:])
        core.saveResult = .failure(.deckOptionsInvalid([issue]))
        model.save(); await model.waitForCompletion()
        #expect(model.phase == .editing && model.issues(for: .newPerDay).map(\.code) == [.outOfRange] && !model.canSave)
        model.setValue(.newPerDay, 41); await model.waitForCompletion()
        core.saveResult = .failure(.busy(runningOperation: "sync"))
        model.save(); await model.waitForCompletion()
        #expect(model.phase == .editing && model.saveFailure == .busy && model.value(.newPerDay) == 41)
    }

    @Test func aRunningSaveReservesTheCollectionAndAGenerationChangeEndsThePage() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 30)
        let gate = PageGate()
        core.saveGate = gate
        model.save()
        await gate.entered()
        #expect(model.phase == .saving && !owner.canStudy && !owner.deckActions.canPresent(.create))
        #expect(!model.requestLeave() && !model.canOpen(deckID: 2))
        gate.release(); await model.waitForCompletion()
        #expect(model.outcome == .saved)
        model.acknowledgeOutcome()
        model.open(deckID: 1); await model.waitForCompletion()
        owner.receiveLifecycle(.ready(generation: 12))
        #expect(model.phase == .closed && model.outcome == .ended)
    }

    @Test func readFailuresShowTheUnavailableState() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        core.openFailure = .notFound(table: .decks, key: "1")
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(model.phase == .unavailable(.notFound))
        core.openFailure = .internal("storage")
        model.reload(); await model.waitForCompletion()
        #expect(model.phase == .unavailable(.failed))
        core.openFailure = nil
        model.reload(); await model.waitForCompletion()
        #expect(model.phase == .editing)
    }

    // MARK: Fixtures

    private static func changes(decks: [Int64]) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [],
            decks: decks.map { .init(id: $0, change: .updated) }, deckConfigs: [], configKeys: [], tags: [],
            revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [],
            skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)
    }

    private func makeOwner(stored: [UInt64] = [6, 1], ownValues: Bool = true,
                           retention: Double = 0.9) async -> (ReviewCollection, FakePageCore) {
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let counts = ReviewCounts(new: 0, learning: 0, review: 0)
        let parent = CountedDeckNode(id: 1, name: "Parent", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        let other = CountedDeckNode(id: 2, name: "Other", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        let filtered = CountedDeckNode(id: 3, name: "Filtered", filtered: true, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: [parent, other, filtered]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        let snapshot = DeckOptionsPageSnapshot(deckID: 1, deckName: "Parent", assignedPreset: 1, openingPreset: 1,
            presets: [
                .init(id: 1, name: "Default", storedUseCount: stored[0], newPerDay: 20, reviewsPerDay: 200, desiredRetention: retention),
                .init(id: 2, name: "Solo", storedUseCount: stored[1], newPerDay: 10, reviewsPerDay: 100, desiredRetention: 0.85),
            ],
            ownLimits: ownValues, ownRetention: ownValues,
            ranges: [.newPerDay: 0...9999, .reviewsPerDay: 0...9999, .desiredRetention: 0.7...0.99])
        let core = FakePageCore(snapshot: snapshot)
        owner.deckOptions.locale = Locale(identifier: "en_US")
        owner.deckOptions.openSession = { [core] _ in try core.open() }
        return (owner, core)
    }
}

@MainActor
final class FakePageCore {
    let snapshot: DeckOptionsPageSnapshot
    var opens = 0
    var openFailure: BridgeError?
    var saveResult: Result<UInt64, BridgeError> = .success(11)
    var saveGate: PageGate?
    private(set) var session: FakePageSession?
    init(snapshot: DeckOptionsPageSnapshot) { self.snapshot = snapshot }
    func open() throws -> FakePageSession {
        opens += 1
        if let openFailure { throw openFailure }
        let value = FakePageSession(core: self)
        session = value
        return value
    }
}

/// A scripted core draft: it records every edit and validates as O4 does for the three rows.
@MainActor
final class FakePageSession: DeckOptionsPageSession {
    let snapshot: DeckOptionsPageSnapshot
    private unowned let core: FakePageCore
    private(set) var calls: [String] = []
    private var values: [Int64: DeckOptionsPagePreset]
    private var selected: Int64
    init(core: FakePageCore) {
        self.core = core; snapshot = core.snapshot
        values = Dictionary(uniqueKeysWithValues: core.snapshot.presets.map { ($0.id, $0) })
        selected = core.snapshot.openingPreset
    }
    func select(_ preset: Int64) async throws { calls.append("select \(preset)"); selected = preset }
    func set(_ row: DeckOptionsPageRow, preset: Int64, value: Double) async throws {
        calls.append("set \(row.rawValue) \(preset) \(value)"); values[preset]?[row] = value
    }
    func issues() async throws -> [DeckOptionsPageIssue] {
        var issues: [DeckOptionsPageIssue] = []
        for preset in values.values.sorted(by: { $0.id < $1.id }) {
            for row in DeckOptionsPageRow.allCases {
                let value = preset[row], range = snapshot.ranges[row]!
                if !range.contains(value) {
                    issues.append(.init(row: row, preset: preset.id, code: .outOfRange, severity: .error))
                } else if row != .desiredRetention, value.rounded() != value {
                    issues.append(.init(row: row, preset: preset.id, code: .notWhole, severity: .error))
                }
            }
        }
        if let preset = values[selected] {
            let expected = min(9999, 10 * preset.newPerDay)
            if preset.reviewsPerDay < expected {
                issues.append(.init(row: .reviewsPerDay, preset: selected, code: .reviewsBelowTenTimesNew,
                    severity: .warning, params: ["expected": String(Int(expected))]))
            }
            if preset.desiredRetention < 0.8 {
                issues.append(.init(row: .desiredRetention, preset: selected, code: .retentionLow,
                    severity: preset.desiredRetention < 0.7 ? .strongWarning : .warning))
            }
        }
        return issues
    }
    func save() async throws -> UInt64 {
        calls.append("save")
        if let gate = core.saveGate { await gate.hold() }
        return try core.saveResult.get()
    }
    func cancel() async {}
}

@MainActor
final class PageGate {
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
