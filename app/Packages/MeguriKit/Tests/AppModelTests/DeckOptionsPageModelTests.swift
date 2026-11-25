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
        await model.waitForCompletion()
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

    @Test func untouchedTextKeepsAFinerStoredValueAndTheStepStartsFromTypedText() async throws {
        let (owner, _) = await makeOwner(retention: 0.875123)
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(model.fieldText[.desiredRetention] == "87.51%")
        // Back commits every row: the field's unchanged text is not parsed back.
        #expect(model.requestLeave() && model.phase == .closed)
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.desiredRetention] = "80%"
        model.stepRetention(by: 1)
        #expect(model.value(.desiredRetention) == 0.81)
    }

    @Test func saveCommitsATypedCorrectionOfAnError() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.desiredRetention] = "65%"; model.commit(.desiredRetention)
        await model.waitForCompletion()
        #expect(!model.canSave)
        // The correction is still in the field: Save commits and validates it first.
        model.fieldText[.desiredRetention] = "85%"
        #expect(model.canSave)
        // The page's ⌘S, ⌘↩ and Save may run after the field committed itself
        // (focus left it): the old error still shows until validation, and
        // Save checks only after it.
        model.commit(.desiredRetention)
        #expect(model.issues(for: .desiredRetention).contains { $0.severity == .error })
        model.save(); await model.waitForCompletion()
        #expect(model.outcome == .saved)
        // A correction that still fails keeps the page, with its error.
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.desiredRetention] = "65%"; model.commit(.desiredRetention)
        await model.waitForCompletion()
        model.fieldText[.desiredRetention] = "60%"
        model.save(); await model.waitForCompletion()
        #expect(model.phase == .editing && !model.canSave && model.value(.desiredRetention) == 0.6)
    }

    @Test func aSaveWhosePageLeftEndsTheDraftWhenItFailsAndABackgroundCancelBeforeItStartsHoldsIt() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        model.setValue(.newPerDay, 30)
        let gate = PageGate()
        core.saveGate = gate
        core.saveResult = .failure(.busy(runningOperation: "sync"))
        model.save()
        try #require(model.phase == .saving)
        await gate.entered()
        owner.navigation.setPath([], for: .home)
        #expect(model.phase == .saving)
        gate.release(); await model.waitForCompletion()
        #expect(model.phase == .closed && model.outcome == .left)
        // Background before the save starts: nothing is saved, the draft stays.
        core.saveResult = .success(11); core.saveGate = nil
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31)
        model.save()
        await model.cancelAndDrain()
        #expect(model.phase == .editing && model.outcome == nil && model.hasChanges)
    }

    @Test func aChangeWhileTheSnapshotLoadsReadsItAgain() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1)
        model.changesCommitted(Self.changes(decks: [1]), generation: 11)
        await model.waitForCompletion(); await model.waitForCompletion()
        #expect(core.opens == 2 && model.phase == .editing)
    }

    @Test func textTypedWhileASwitchWaitsCancelsIt() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.newPerDay] = "21"
        model.selectPreset(2)
        model.fieldText[.newPerDay] = "22"
        await model.waitForCompletion(); await Task.yield(); await model.waitForCompletion()
        #expect(model.selectedPreset == 1 && model.fieldText[.newPerDay] == "22")
    }

    @Test func aTypedErrorIsCheckedBeforeAnotherPresetIsChosen() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.fieldText[.newPerDay] = "10000"
        model.selectPreset(2)
        model.selectPreset(2)
        #expect(model.selectedPreset == 1 && model.selectingPreset && !model.canSwitchPreset)
        model.beginNewPreset()
        #expect(!model.namingPreset)
        await model.waitForCompletion(); await Task.yield(); await model.waitForCompletion()
        #expect(model.selectedPreset == 1 && model.issues(for: .newPerDay).map(\.code) == [.outOfRange])
        #expect(model.fieldText[.newPerDay] == "10,000" && !model.selectingPreset)
        #expect(core.session?.calls == ["set newPerDay 1 10000.0"])
    }

    @Test func aFocusLossCommitIsValidatedBeforeSwitchingPresets() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        let gate = PageGate(); core.issuesGate = gate
        model.fieldText[.newPerDay] = "10000"
        model.commit(.newPerDay)
        await gate.entered()
        #expect(model.issues.isEmpty && !model.canSwitchPreset)
        model.beginNewPreset()
        #expect(!model.namingPreset)
        model.selectPreset(2)
        #expect(model.selectedPreset == 1 && model.selectingPreset)
        gate.release(); await model.waitForCompletion()
        #expect(model.selectedPreset == 1 && !model.selectingPreset && !model.canSwitchPreset)
        #expect(model.fieldText[.newPerDay] == "10,000")
        #expect(model.issues(for: .newPerDay).map(\.code) == [.outOfRange])
        #expect(core.session?.calls == ["set newPerDay 1 10000.0"])

        // Once a correction is validated, an ordinary switch is immediate.
        model.fieldText[.newPerDay] = "21"; model.commit(.newPerDay)
        await model.waitForCompletion()
        #expect(model.canSwitchPreset)
        model.selectPreset(2)
        #expect(model.selectedPreset == 2 && !model.selectingPreset)
        await model.waitForCompletion()
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
        // A generation that ends while Settings shows ends the draft the same way.
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        owner.receiveLifecycle(.ready(generation: 12))
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
        // The save was admitted, and a save that ends before the gate fails the test.
        try #require(model.phase == .saving)
        Task { await model.waitForCompletion(); gate.abandon() }
        await gate.entered()
        try #require(!gate.abandoned, "the save reached the core")
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

    @Test func displayOrderEditsStayWithEachPresetAndSaveInSequence() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(!model.advancedExpanded)
        #expect(model.selected?.displayOrder[.reviewOrder] == 11)
        #expect(model.choices(.newGatherPriority) == [0, 5, 1, 2, 3, 4])
        model.advancedExpanded = true
        model.setDisplayOrder(.reviewOrder, 12)
        model.setDisplayOrder(.newGatherPriority, 5)
        model.selectPreset(2)
        model.setDisplayOrder(.newSortOrder, 4)
        model.selectPreset(1)
        #expect(model.selected?.displayOrder[.reviewOrder] == 12)
        model.setDisplayOrder(.reviewOrder, 999) // Only the core's choices are offered.
        model.save(); model.save()
        model.setDisplayOrder(.newMix, 2); model.selectPreset(2)
        await model.waitForCompletion()
        #expect(core.session?.calls == ["order reviewOrder 1 12", "order newGatherPriority 1 5",
            "select 2", "order newSortOrder 2 4", "select 1", "save"])
        #expect(model.outcome == .saved)
        model.open(deckID: 1); await model.waitForCompletion()
        #expect(!model.advancedExpanded)
    }

    @Test(arguments: [false, true])
    func newPresetOwnsMainWindowAdmissionUntilCancelled(interactively: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions, history = owner.collectionUndo
        let undo = PresetAdmissionUndo()
        history.service = undo
        await history.waitForRefresh(); await history.refreshNow()
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        // A previous Settings presentation must not host this main-window form.
        owner.navigation.sheetHost = .settings
        #expect(history.canUndo && history.canRedo && owner.deckOptionsAllowed)
        model.beginNewPreset(); model.presetName = "Temporary"
        #expect(owner.navigation.sheet == .newDeckOptionsPreset && owner.navigation.sheetHost == .main)
        #expect(model.namingPreset && model.canCreatePreset)
        #expect(!history.canUndo && !history.canRedo && !owner.deckOptionsAllowed)
        #expect(!owner.deckSearchAvailable(layout: .sidebar) && !owner.deckActions.canPresent(.create))
        history.undo(); history.redo()
        await history.waitForCompletion()
        #expect(await undo.writes == 0)
        #expect(!model.requestLeave() && !model.canOpen(deckID: 2))
        if interactively { owner.navigation.dismissSheet() } else { model.cancelNewPreset() }
        #expect(owner.navigation.sheet == nil && !model.namingPreset && !model.canCreatePreset)
        #expect(history.canUndo && history.canRedo && owner.deckOptionsAllowed)
        #expect(owner.deckSearchAvailable(layout: .sidebar) && owner.deckActions.canPresent(.create))
        #expect(!model.hasChanges && core.session?.calls.isEmpty == true)
        model.beginNewPreset()
        #expect(model.namingPreset && model.presetName.isEmpty && !model.canCreatePreset)
        model.cancelNewPreset()
        // An unchanged page may open another deck after either dismissal path.
        model.open(deckID: 2); await model.waitForCompletion()
        #expect(model.deckID == 2 && model.phase == .editing && owner.navigation.sheet == nil)
    }

    @Test(arguments: [false, true])
    func newPresetCannotReplaceAnotherHostsSheet(settings: Bool) async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        let host: AppNavigation.SheetHost = settings ? .settings : .main
        model.open(deckID: 1); await model.waitForCompletion()
        for route in [SheetRoute.collectionExport, .noteTypeSave, .add(deck: nil), .deckName(.create)] {
            owner.navigation.present(route); owner.navigation.sheetHost = host
            model.beginNewPreset()
            #expect(owner.navigation.sheet == route && owner.navigation.sheetHost == host)
            #expect(!model.namingPreset && !model.canCreatePreset)
            model.cancelNewPreset()
            #expect(owner.navigation.sheet == route)
            owner.navigation.dismissSheet()
        }
    }

    @Test func endingTheDraftReleasesOnlyItsOwnSheet() async throws {
        let (owner, _) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        owner.navigation.setPath([.deckOptions(DeckID(1))], for: .home)
        model.beginNewPreset()
        owner.navigation.collectionReplaced()
        #expect(model.phase == .closed && !model.namingPreset && owner.navigation.sheet == nil)
        model.open(deckID: 1); await model.waitForCompletion()
        model.beginNewPreset()
        owner.navigation.present(.collectionExport)
        model.invalidate()
        #expect(model.phase == .closed && owner.navigation.sheet == .collectionExport)
    }

    @Test func newPresetValidatesNameSelectsCanonicalDraftAndKeepsPriorEdits() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31)
        model.setDisplayOrder(.reviewOrder, 12)
        await model.waitForCompletion()
        model.beginNewPreset()
        model.presetName = " \n "
        #expect(!model.canCreatePreset && !model.canSave)
        model.createPreset()
        model.presetName = "  Default  "
        model.createPreset(); model.createPreset()
        // Nothing can switch or save while the canonical Add is in flight.
        model.selectPreset(2); model.save(); model.setValue(.newPerDay, 55)
        await model.waitForCompletion()
        #expect(owner.navigation.sheet == nil && !model.namingPreset && owner.deckOptionsAllowed)
        #expect(model.selectedPreset == -1 && model.selected?.name == "Default1894701600")
        #expect(model.selectedUseCount == 0 && model.value(.newPerDay) == 20)
        #expect(model.selected?.displayOrder[.reviewOrder] == 0)
        model.setDisplayOrder(.reviewOrder, 6)
        model.selectPreset(1)
        #expect(model.value(.newPerDay) == 31 && model.selected?.displayOrder[.reviewOrder] == 12)
        #expect(model.selectedUseCount == 6)
        model.selectPreset(-1)
        #expect(model.selectedUseCount == 0 && model.selected?.displayOrder[.reviewOrder] == 6)
        model.save(); await model.waitForCompletion()
        #expect(core.session?.calls == ["set newPerDay 1 31.0", "order reviewOrder 1 12", "add Default",
            "order reviewOrder -1 6", "select 1", "select -1", "save"])
        #expect(model.outcome == .saved)
    }

    @Test func cancellingNameEntryAndDiscardingAStagedPresetNeverSaves() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.beginNewPreset(); model.presetName = "Abandoned"; model.cancelNewPreset()
        #expect(!model.hasChanges && !model.namingPreset && core.session?.calls.isEmpty == true)
        model.beginNewPreset(); model.presetName = "Staged"; model.createPreset()
        await model.waitForCompletion()
        #expect(model.hasChanges && !model.requestLeave())
        model.keepEditing()
        #expect(model.selectedPreset < 0)
        model.requestLeave(); model.discardChanges()
        await model.waitForCompletion()
        #expect(core.session?.calls == ["add Staged"] && model.outcome == .left)
    }

    @Test(arguments: [false, true])
    func rejectedAddKeepsEarlierEditsForCancelOrRetry(retry: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31); model.setDisplayOrder(.reviewOrder, 12)
        await model.waitForCompletion()
        let draft = model.presets
        model.beginNewPreset(); model.presetName = "Temporary"
        core.addFailure = .busy(runningOperation: "sync")
        model.createPreset(); await model.waitForCompletion()
        #expect(model.presetCreationFailed && model.canCreatePreset && model.namingPreset)
        #expect(!model.changedElsewhere && model.presets == draft && model.selectedPreset == 1)
        #expect(core.session?.snapshot == core.snapshot)
        if retry {
            model.presetName = "Retry"
            model.createPreset(); model.createPreset()
            await model.waitForCompletion()
            #expect(!model.namingPreset && !model.presetCreationFailed && model.selectedPreset == -1)
        } else {
            model.cancelNewPreset()
            #expect(model.selectedPreset == 1 && model.value(.newPerDay) == 31)
        }
        #expect(model.canSave)
        model.save(); await model.waitForCompletion()
        let saved = try #require(core.session?.savedPresets)
        #expect(saved.first { $0.id == 1 }?.newPerDay == 31)
        #expect(saved.first { $0.id == 1 }?.displayOrder[.reviewOrder] == 12)
        #expect(saved.filter { $0.id < 0 }.count == (retry ? 1 : 0))
        #expect(core.session?.savedSelection == (retry ? -1 : 1))
        #expect(core.opens == 1 && model.outcome == .saved)
        #expect(core.session?.calls.filter { $0.hasPrefix("add ") }.count == (retry ? 2 : 1))
    }

    @Test(arguments: [false, true])
    func addedPresetSurvivesValidationReadFailureAndSaveRetriesWithoutDuplicate(numeric: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31); model.setDisplayOrder(.reviewOrder, 12)
        await model.waitForCompletion()
        model.beginNewPreset(); model.presetName = "Default"
        let gate = PageGate(); core.issuesGate = gate
        core.issuesFailure = .cancelled
        model.createPreset()
        await gate.entered()
        #expect(model.selectedPreset == -1 && model.selected?.name == "Default1894701600")
        #expect(model.presets == core.session?.snapshot.presets && model.addingPreset)
        model.createPreset()
        gate.release(); await model.waitForCompletion()
        #expect(model.presetValidationFailed && !model.changedElsewhere && !model.namingPreset)
        #expect(model.canSave && !model.canSwitchPreset && model.selectedUseCount == 0)
        model.createPreset(); model.beginNewPreset(); model.selectPreset(1)
        #expect(model.selectedPreset == -1 && !model.namingPreset)
        core.issuesFailure = .cancelled
        if numeric { model.setValue(.newPerDay, 42) }
        else { model.setDisplayOrder(.reviewOrder, 6) }
        await model.waitForCompletion()
        #expect(model.presetValidationFailed && !model.changedElsewhere && model.canSave && !model.canSwitchPreset)
        let staged = try #require(model.selected)
        #expect(staged.id == -1 && staged.name == "Default1894701600")
        #expect(staged.newPerDay == (numeric ? 42 : 20) && staged.displayOrder[.reviewOrder] == (numeric ? 0 : 6))
        core.saveResult = .failure(.sessionActive)
        model.save(); await model.waitForCompletion()
        #expect(model.phase == .editing && model.saveFailure == .busy && model.canSave)
        #expect(core.session?.savedPresets == nil && model.presets.count == 3)
        core.saveResult = .success(11)
        model.save(); await model.waitForCompletion()
        let saved = try #require(core.session?.savedPresets)
        #expect(saved.filter { $0.id < 0 }.count == 1 && core.session?.savedSelection == -1)
        #expect(saved.first { $0.id == -1 } == staged)
        #expect(saved.first { $0.id == 1 }?.newPerDay == 31)
        #expect(saved.first { $0.id == 1 }?.displayOrder[.reviewOrder] == 12)
        #expect(core.session?.calls == ["set newPerDay 1 31.0", "order reviewOrder 1 12", "add Default",
            numeric ? "set newPerDay -1 42.0" : "order reviewOrder -1 6", "save", "save"])
        #expect(core.opens == 1 && model.outcome == .saved)
    }

    @Test(arguments: [false, true])
    func validationErrorsAfterAddStillBlockWritesAndKeepTheStagedPreset(duringSave: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31); await model.waitForCompletion()
        model.beginNewPreset(); model.presetName = "Staged"
        let invalid = BridgeError.deckOptionsInvalid([
            .init(field: .newPerDay, preset: -1, code: .outOfRange, severity: .error, params: [:]),
        ])
        core.issuesFailure = duringSave ? .cancelled : invalid
        model.createPreset(); await model.waitForCompletion()
        if duringSave {
            #expect(model.presetValidationFailed && model.canSave)
            core.saveResult = .failure(invalid)
            model.save(); await model.waitForCompletion()
        }
        #expect(!model.presetValidationFailed && !model.canSave && model.hasErrors)
        #expect(model.selectedPreset == -1 && model.presets.first { $0.id == 1 }?.newPerDay == 31)
        model.save(); await model.waitForCompletion()
        #expect(core.session?.calls.filter { $0 == "save" }.count == (duringSave ? 1 : 0))
        #expect(core.session?.savedPresets == nil && !model.changedElsewhere)
        model.setValue(.newPerDay, 21); await model.waitForCompletion()
        core.saveResult = .success(11)
        model.save(); await model.waitForCompletion()
        #expect(model.outcome == .saved && core.opens == 1)
        #expect(core.session?.savedPresets?.filter { $0.id < 0 }.count == 1)
        #expect(core.session?.calls.filter { $0 == "add Staged" }.count == 1)
    }

    @Test(arguments: [false, true])
    func presetFailuresKeepStaleDraftProtection(afterAdd: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.setValue(.newPerDay, 31); await model.waitForCompletion()
        model.beginNewPreset(); model.presetName = "Stale"
        let failure = BridgeError.stale(table: .deckConfig, key: "1")
        if afterAdd { core.issuesFailure = failure } else { core.addFailure = failure }
        model.createPreset(); await model.waitForCompletion()
        #expect(model.changedElsewhere && !model.canCreatePreset)
        let session = try #require(core.session)
        model.cancelNewPreset()
        model.save(); await model.waitForCompletion()
        #expect(core.opens == 2 && model.saveFailure == .failed && !model.hasChanges)
        #expect(session.savedPresets == nil && !session.calls.contains("save"))
    }

    @Test(arguments: [false, true])
    func presetFailuresEndAStaleGeneration(afterAdd: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        model.beginNewPreset(); model.presetName = "Stale"
        let failure = BridgeError.staleGeneration(planned: 11, current: 12)
        if afterAdd { core.issuesFailure = failure } else { core.addFailure = failure }
        model.createPreset(); await model.waitForCompletion()
        #expect(model.outcome == .ended && model.phase == .closed && model.presets.isEmpty)
        #expect(!model.namingPreset && !model.addingPreset && !model.presetValidationFailed)
    }

    @Test(arguments: ["31", "10000"])
    func newPresetWaitsForTypedNumericValidationOnTheFirstClick(text: String) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        let gate = PageGate(); core.issuesGate = gate
        model.fieldText[.newPerDay] = text
        model.beginNewPreset()
        await gate.entered()
        #expect(model.selectingPreset && !model.namingPreset && !model.canSwitchPreset)
        model.createPreset()
        gate.release(); await model.waitForCompletion()
        #expect(!model.selectingPreset && model.namingPreset == (text == "31"))
        #expect(model.value(.newPerDay) == Double(text) && model.selectedPreset == 1)
        #expect(core.session?.calls == ["set newPerDay 1 \(Double(text)!)"])
        model.cancelNewPreset()
    }

    @Test(arguments: [false, true])
    func pendingNewPresetDoesNotOpenAfterNewerInputOrGeneration(generationEnds: Bool) async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        let gate = PageGate(); core.issuesGate = gate
        model.fieldText[.newPerDay] = "31"; model.beginNewPreset()
        await gate.entered()
        if generationEnds { owner.receiveLifecycle(.ready(generation: 12)) }
        else { model.fieldText[.newPerDay] = "32" }
        gate.release(); await model.waitForCompletion()
        #expect(!model.namingPreset && !model.selectingPreset)
        if generationEnds { #expect(model.phase == .closed && model.outcome == .ended) }
        else { #expect(model.fieldText[.newPerDay] == "32" && model.phase == .editing) }
    }

    @Test func anAddInFlightCannotReplaceANewerGeneration() async throws {
        let (owner, core) = await makeOwner()
        let model = owner.deckOptions
        model.open(deckID: 1); await model.waitForCompletion()
        let gate = PageGate(); core.addGate = gate
        model.beginNewPreset(); model.presetName = "Held"; model.createPreset()
        await gate.entered()
        #expect(model.addingPreset && !model.canSave && !model.canSwitchPreset)
        #expect(owner.navigation.sheet == .newDeckOptionsPreset && !owner.deckOptionsAllowed)
        model.cancelNewPreset(); model.reload(); model.save()
        #expect(model.namingPreset && core.opens == 1)
        owner.receiveLifecycle(.ready(generation: 12))
        #expect(owner.navigation.sheet == nil && !model.namingPreset)
        gate.release(); await model.waitForCompletion()
        #expect(model.phase == .closed && model.presets.isEmpty && model.outcome == .ended)
        #expect(core.session?.calls == ["add Held"])
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
                .init(id: 1, name: "Default", storedUseCount: stored[0], newPerDay: 20, reviewsPerDay: 200, desiredRetention: retention,
                    displayOrder: [.newGatherPriority: 3, .newSortOrder: 2, .newMix: 1, .interdayLearningMix: 2, .reviewOrder: 11]),
                .init(id: 2, name: "Solo", storedUseCount: stored[1], newPerDay: 10, reviewsPerDay: 100, desiredRetention: 0.85,
                    displayOrder: [.newGatherPriority: 4, .newSortOrder: 1, .newMix: 2, .interdayLearningMix: 1, .reviewOrder: 6]),
            ],
            ownLimits: ownValues, ownRetention: ownValues,
            ranges: [.newPerDay: 0...9999, .reviewsPerDay: 0...9999, .desiredRetention: 0.7...0.99],
            displayOrderChoices: pageOrderChoices)
        let core = FakePageCore(snapshot: snapshot)
        owner.deckOptions.locale = Locale(identifier: "en_US")
        owner.deckOptions.openSession = { [core] _ in try core.open() }
        return (owner, core)
    }
}

/// Available history in both directions, recording any write admitted by the modal guard.
private actor PresetAdmissionUndo: UndoService {
    private(set) var writes = 0
    func peekUndo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus {
        .available(step: ReviewStepInfo(id: 1, name: "Deck Options", kind: .other, session: nil, longWrite: nil))
    }
    func peekRedo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { try peekUndo(scope: scope) }
    func startUndo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        writes += 1
        throw .cancelled
    }
    func startRedo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        writes += 1
        throw .cancelled
    }
}

@MainActor
final class FakePageCore {
    let snapshot: DeckOptionsPageSnapshot
    var opens = 0
    var openFailure: BridgeError?
    var saveResult: Result<UInt64, BridgeError> = .success(11)
    var saveGate: PageGate?
    var addGate: PageGate?
    var issuesGate: PageGate?
    var addFailure: BridgeError?
    var issuesFailure: BridgeError?
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
    private(set) var snapshot: DeckOptionsPageSnapshot
    private unowned let core: FakePageCore
    private(set) var calls: [String] = []
    private(set) var savedPresets: [DeckOptionsPagePreset]?
    private(set) var savedSelection: Int64?
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
    func setDisplayOrder(_ row: DeckOptionsDisplayOrder, preset: Int64, value: Int32) async throws {
        calls.append("order \(row.rawValue) \(preset) \(value)")
        values[preset]?.displayOrder[row] = value
    }
    func addPreset(name: String) async throws -> DeckOptionsPageSnapshot {
        calls.append("add \(name)")
        if let gate = core.addGate { await gate.hold() }
        if let failure = core.addFailure { core.addFailure = nil; throw failure }
        let id = min(values.keys.min() ?? 0, 0) - 1
        let canonical = values.values.contains { $0.name == name } ? name + "1894701600" : name
        values[id] = DeckOptionsPagePreset(id: id, name: canonical, storedUseCount: 0,
            newPerDay: 20, reviewsPerDay: 200, desiredRetention: 0.9,
            displayOrder: Dictionary(uniqueKeysWithValues: DeckOptionsDisplayOrder.allCases.map { ($0, Int32(0)) }))
        selected = id
        snapshot = DeckOptionsPageSnapshot(deckID: snapshot.deckID, deckName: snapshot.deckName,
            assignedPreset: snapshot.assignedPreset, openingPreset: id,
            presets: values.values.sorted { $0.name < $1.name }, ownLimits: snapshot.ownLimits,
            ownRetention: snapshot.ownRetention, ranges: snapshot.ranges, displayOrderChoices: snapshot.displayOrderChoices)
        return snapshot
    }
    func issues() async throws -> [DeckOptionsPageIssue] {
        if let gate = core.issuesGate { core.issuesGate = nil; await gate.hold() }
        if let failure = core.issuesFailure { core.issuesFailure = nil; throw failure }
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
        let generation = try core.saveResult.get()
        savedPresets = values.values.sorted { $0.id < $1.id }; savedSelection = selected
        return generation
    }
    func cancel() async {}
}

@MainActor
final class PageGate {
    private var didEnter = false
    private var entry: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    private(set) var abandoned = false
    func entered() async {
        if didEnter || abandoned { return }
        await withCheckedContinuation { entry = $0 }
    }
    /// The save ended without reaching the gate: the wait ends, so the test fails instead of hanging.
    func abandon() { if !didEnter { abandoned = true; entry?.resume(); entry = nil } }
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; didEnter = true; entry?.resume(); entry = nil
        }
    }
    func release() { continuation?.resume(); continuation = nil }
}

private let pageOrderChoices: [DeckOptionsDisplayOrder: [Int32]] = [
    .newGatherPriority: [0, 5, 1, 2, 3, 4], .newSortOrder: [0, 1, 2, 3, 4],
    .newMix: [0, 1, 2], .interdayLearningMix: [0, 1, 2],
    .reviewOrder: [0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10],
]
