@_spi(MeguriTestHooks) @testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// U277's sheets (`browser/workspace` V9, V10; `browser/find-replace` F2,
/// F10, F11) against a scripted session: what each sheet offers, the exact
/// requests it sends on the rows it opened on, when its default button is
/// enabled, the refusals that write nothing (a list that moved on, a count
/// larger than the preview's), Cancel, the undo title, VoiceOver's result,
/// what the device remembers, and Find Duplicates' token and previous query.
@MainActor @Suite struct BrowseFindSheetsTests {
    /// Cards 1 to 4 (notes 10 to 40), the model's note type names read.
    private func browse(types: [Int64: Int64] = [:]) async throws -> (BrowseModel, FakeSearchSession, FakeSheetReads) {
        let backend = FakeBrowseBackend(), session = FakeSearchSession(), reads = FakeSheetReads()
        session.results[""] = [1, 2, 3, 4]
        session.notetypesOfNote = types
        backend.searchSession = session
        let model = BrowseModel(backend: backend, sheetReads: reads)
        model.results.progressDelay = .seconds(60)
        await model.prepareForTestHooks(query: FilterQuery())
        await waitUntil { model.results.list?.count == 4 && model.results.list?.isComplete == true && model.results.row(at: 3) != nil }
        await waitUntil { !model.notetypes.isEmpty }
        return (model, session, reads)
    }

    private func select(_ model: BrowseModel, _ rows: Set<Int>) async {
        model.results.select(rows)
        await waitUntil { model.commands.facts?.rows == rows.count && model.commands.facts?.notetypes != nil }
    }

    private func openFindReplace(_ model: BrowseModel) async throws -> BrowseFindReplaceSheet {
        model.commands.perform(.findReplace)
        let sheet = try #require(model.commands.findReplaceSheet)
        await waitUntil { sheet.allFound != nil && sheet.foundNotes != nil }
        return sheet
    }

    /// Every target note changes when the find text is not empty.
    private nonisolated static func changingAll(_ request: FindReplaceRequest, _ notes: [Int64]) throws -> FindReplacePreview {
        guard !request.find.isEmpty else { return FindReplacePreview(changed: 0, considered: UInt64(notes.count), entries: []) }
        return FindReplacePreview(changed: UInt64(notes.count), considered: UInt64(notes.count), entries: notes.map {
            FindReplacePreviewEntry(noteID: $0, sortField: "note \($0)", field: "Front",
                                    spans: [.init(edit: .removed, text: request.find), .init(edit: .inserted, text: request.replace)])
        })
    }

    // MARK: Find and Replace

    @Test func findAndReplaceOpensOnTheSelectionPreviewsAndRunsOneStep() async throws {
        let (model, session, _) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        session.previewing = Self.changingAll
        await select(model, [1, 2])
        let handle = try #require(model.results.list?.handle)
        #expect(model.commands.state(.findReplace).enabled)
        let sheet = try await openFindReplace(model)
        // Selected first and the default with a selection; k and m counted in the core.
        #expect(sheet.notes == .selected && sheet.canChooseSelected)
        #expect(sheet.selected == BrowseTarget(handle: handle, revision: 0, ranges: [1..<3], mode: .cards))
        #expect(sheet.allFound == BrowseTarget(handle: handle, revision: 0, ranges: [0..<4], mode: .cards))
        await waitUntil { sheet.selectedNotes == 2 && sheet.foundNotes == 4 }
        // In: All Fields, the note type's fields, then Tags (one note type: no groups).
        await waitUntil { sheet.entries.count == 4 }
        #expect(sheet.entries.map(\.kind) == [.allFields, .field("Front"), .field("Back"), .tags])
        #expect(sheet.entries.allSatisfy { $0.group == nil } && sheet.inEntry == 0)
        // Other commands wait while the sheet is open.
        #expect(!model.commands.state(.suspend).enabled && !model.commands.state(.findDuplicates).enabled)
        #expect(model.commands.unavailable == .busy)
        // An empty find text changes nothing here: No Matches, Replace disabled.
        await waitUntil { if case .ready = sheet.preview { true } else { false } }
        #expect(sheet.confirmCount == 0 && !sheet.canConfirm)
        // Typing previews in the core on the selected notes; Match Case off ignores case.
        sheet.find = "zorvel"
        sheet.replace = "plinth"
        await waitUntil { sheet.confirmCount == 2 }
        // Previews run concurrently in the core; the one shown is the current inputs'.
        #expect(session.previewCalls.contains(.init(request: FindReplaceRequest(find: "zorvel", replace: "plinth", scope: .allFields),
                                                     notes: [20, 30], confirmed: nil)))
        #expect(sheet.canConfirm && sheet.shownPreview?.entries.count == 2)
        sheet.confirm()
        await waitUntil { model.commands.findReplaceSheet == nil }
        // One run, confirming the preview's count; one step titled after the command.
        #expect(session.replaceCalls == [.init(request: FindReplaceRequest(find: "zorvel", replace: "plinth", scope: .allFields),
                                               notes: [20, 30], confirmed: 2)])
        #expect(steps == ["Find and Replace"])
        #expect(model.commands.announcement?.kind == .notesChanged && model.commands.announcement?.count == 2)
        // F2: the texts entered the histories and the options were kept.
        #expect(model.device.findReplace?.finds == ["zorvel"] && model.device.findReplace?.replaces == ["plinth"])
        #expect(model.device.findReplace?.inEntry == "allFields" && model.device.findReplace?.inPosition == 0)
    }

    @Test func theOptionsAndInReachTheCoresRequest() async throws {
        let (model, session, _) = try await browse()
        await select(model, [0])
        let sheet = try await openFindReplace(model)
        await waitUntil { sheet.entries.count == 4 }
        sheet.find = "a"
        sheet.matchCase = true
        sheet.wholeWords = true
        sheet.regex = true
        sheet.inEntry = 3
        let tags = FindReplaceRequest(find: "a", replace: "", regex: true, ignoreCase: false, wholeWords: true, scope: .tags)
        #expect(sheet.request == tags)
        await waitUntil { session.previewCalls.contains { $0.request == tags } && sheet.confirmCount != nil }
        sheet.inEntry = 1
        await waitUntil { session.previewCalls.contains { $0.request.scope == .field("Front") } && sheet.confirmCount != nil }
        #expect(sheet.chosenEntry?.kind == .field("Front"))
    }

    @Test func anInvalidPatternShowsTheCoresReasonAndDisablesReplace() async throws {
        let (model, session, _) = try await browse()
        session.previewing = { request, _ in
            guard request.find == "(" else { return FindReplacePreview(changed: 1, considered: 1, entries: []) }
            throw BridgeError.findReplaceRefused(.invalidRegex(message: "regex parse error:\n    (\n    ^\nerror: unclosed group"))
        }
        await select(model, [0])
        let sheet = try await openFindReplace(model)
        sheet.regex = true
        sheet.find = "("
        await waitUntil { if case .invalid = sheet.preview { true } else { false } }
        #expect(sheet.preview == .invalid(.pattern(reason: "unclosed group")) && !sheet.canConfirm)
        // W2.1: a message without an `error: ` line is the size limit; a Tags space its own text.
        #expect(BrowseFindReplaceInvalid.of(message: "Compiled regex exceeds size limit") == .tooLarge)
        session.previewing = { _, _ in throw BridgeError.findReplaceRefused(.separatorInReplacement) }
        sheet.find = "x"
        await waitUntil { sheet.preview == .invalid(.tagSeparator) }
        #expect(!sheet.canConfirm)
    }

    @Test func withNoSelectionItOpensOnAllFoundWhichNeverLeavesTheResults() async throws {
        let (model, session, _) = try await browse()
        session.previewing = Self.changingAll
        model.results.select([])
        let sheet = try await openFindReplace(model)
        #expect(sheet.selected == nil && !sheet.canChooseSelected && sheet.notes == .allFound)
        sheet.find = "x"
        await waitUntil { sheet.confirmCount == 4 }
        #expect(session.previewCalls.last?.notes == [10, 20, 30, 40])
        #expect(sheet.allFound?.ranges == [0..<4])
    }

    @Test func aListThatMovedOnIsRefusedAndALargerCountIsShownAgain() async throws {
        let (model, session, _) = try await browse()
        session.previewing = Self.changingAll
        await select(model, [0, 1])
        let handle = try #require(model.results.list?.handle)
        let sheet = try await openFindReplace(model)
        sheet.find = "x"
        await waitUntil { sheet.confirmCount == 2 }
        // F11: the run counts more notes than the preview showed: refused, nothing written, shown again.
        session.replaceError = .findReplaceRefused(.countChanged(notes: 3))
        // The core now counts three notes: the preview read again shows them.
        session.previewing = { _, notes in FindReplacePreview(changed: 3, considered: UInt64(notes.count), entries: []) }
        sheet.confirm()
        await waitUntil { sheet.failure != nil }
        #expect(sheet.failure == .countChanged(3) && sheet.phase == .editing && model.commands.findReplaceSheet === sheet)
        #expect(session.replaceCalls.last?.confirmed == 2, "the run confirmed the count the preview showed")
        await waitUntil { sheet.confirmCount == 3 }
        session.replaceError = nil
        session.previewing = Self.changingAll
        // The core's list moved on: the preview says so and Replace is refused before the core.
        session.revisions[handle.rawValue] = 1
        sheet.find = "xy"
        await waitUntil { sheet.preview == .rowsChanged }
        #expect(!sheet.canConfirm)
        #expect(session.replaceCalls.count == 1, "only the refused run reached the core")
        sheet.cancel()
        #expect(model.commands.findReplaceSheet == nil)
    }

    @Test func cancelDuringTheRunStopsItWithNothingWritten() async throws {
        let (model, session, _) = try await browse()
        var steps: [String] = []
        model.commands.stepRecorded = { steps.append($1) }
        session.previewing = Self.changingAll
        session.holdsReplace = true
        await select(model, [0])
        let sheet = try await openFindReplace(model)
        sheet.find = "x"
        await waitUntil { sheet.canConfirm }
        sheet.confirm()
        await waitUntil { if case .running(let progress) = sheet.phase { progress != nil } else { false } }
        #expect(sheet.canCancel && !sheet.canConfirm)
        sheet.cancel()
        await waitUntil { model.commands.findReplaceSheet == nil }
        #expect(session.heldAction.cancelled && session.replaceCalls.isEmpty && steps.isEmpty)
    }

    @Test func inGroupsFieldsByNoteTypeAndKeepsTheChosenNameAcrossNotes() async throws {
        // Note 10 is Basic (8); notes 20 to 40 are Vocabulary (7).
        let (model, session, _) = try await browse(types: [10: 8, 20: 7, 30: 7, 40: 7])
        await select(model, [1])
        let sheet = try await openFindReplace(model)
        // Selected: Vocabulary only, no groups.
        await waitUntil { sheet.entries.count == 5 }
        #expect(sheet.entries.map(\.kind) == [.allFields, .field("Word"), .field("Meaning"), .field("Front"), .tags])
        sheet.inEntry = 3
        // All Found: both note types, grouped by name in A2's order (Basic, then Vocabulary).
        sheet.notes = .allFound
        await waitUntil { sheet.entries.count == 7 }
        #expect(sheet.entries.map(\.kind) == [.allFields, .field("Front"), .field("Back"), .field("Word"), .field("Meaning"),
                                              .field("Front"), .tags])
        #expect(sheet.entries.map(\.group) == [nil, "Basic", "Basic", "Vocabulary", "Vocabulary", "Vocabulary", nil])
        // The name chosen stays chosen; it replaces in every note type that has it (F6).
        #expect(sheet.chosenEntry?.kind == .field("Front"))
        await waitUntil { session.previewCalls.contains { $0.request.scope == .field("Front") && $0.notes.count == 4 } }
    }

    @Test func theDeviceRemembersTheOptionsTheInChoiceAndTheLastTexts() async throws {
        let (model, session, _) = try await browse()
        session.previewing = Self.changingAll
        await select(model, [0])
        let first = try await openFindReplace(model)
        await waitUntil { first.entries.count == 4 }
        first.find = "brask"
        first.replace = "vell"
        first.matchCase = true
        first.inEntry = 1
        await waitUntil { first.canConfirm }
        first.confirm()
        await waitUntil { model.commands.findReplaceSheet == nil }
        await select(model, [0])
        let second = try await openFindReplace(model)
        await waitUntil { second.entries.count == 4 }
        // Within the launch the last texts are prefilled; options and In are restored (F2).
        #expect(second.find == "brask" && second.replace == "vell" && second.matchCase && !second.regex)
        await waitUntil { second.inEntry == 1 }
        #expect(second.findHistory == ["brask"] && second.replaceHistory == ["vell"])
        // F2's histories: newest first, no repeat, no empty text, 30 at most.
        var history = BrowseFindReplaceDevice.entering("b", into: ["a", "b", "c"])
        #expect(history == ["b", "a", "c"])
        history = BrowseFindReplaceDevice.entering("", into: history)
        #expect(history == ["b", "a", "c"])
        #expect(BrowseFindReplaceDevice.entering("new", into: (0..<30).map(String.init)).count == 30)
        // "e" + U+0301 and "é" differ code point by code point: both stay.
        #expect(BrowseFindReplaceDevice.entering("e\u{301}", into: ["\u{e9}"]).count == 2)
        second.cancel()
    }

    // MARK: Find Duplicates

    @Test func findDuplicatesStartsOnTheSortFieldAndItsGroupsBecomeAToken() async throws {
        let (model, session, _) = try await browse(types: [10: 8, 20: 7, 30: 7, 40: 7])
        let previous = FilterQuery(text: "")
        await select(model, [1])
        #expect(model.commands.state(.findDuplicates).enabled)
        model.commands.perform(.findDuplicates)
        let sheet = try #require(model.commands.findDuplicatesSheet)
        // Every note type's fields, grouped; it starts on the current row's note type's Sort Field.
        await waitUntil { sheet.field != nil }
        #expect(sheet.entries.map(\.kind) == [.field("Front"), .field("Back"), .field("Word"), .field("Meaning"), .field("Front")])
        #expect(sheet.field == "Meaning" && sheet.notes == .allNotes)
        await waitUntil { sheet.foundNotes == 4 }
        session.duplicates = FindDuplicatesResult(groups: [[20, 30]], noteIds: [20, 30], considered: 3)
        sheet.confirm()
        await waitUntil { model.commands.findDuplicatesSheet == nil }
        #expect(session.duplicatesCalls == [.init(field: "Meaning", notes: nil)])
        // The query is one Duplicates token, in Notes mode, sorted by the field with its column first.
        #expect(model.query == FilterQuery(tokens: [.value([.duplicates(noteIDs: [20, 30])], negated: false)]))
        #expect(model.mode == .notes && model.duplicates?.field == "Meaning" && model.duplicates?.groups == 1)
        await waitUntil { session.starts.last?.mode == .notes && session.starts.last?.sort.key == "fld:Meaning" }
        #expect(model.results.displayField == "Meaning")
        await waitUntil { model.results.list?.columns.first == "fld:Meaning" }
        // A header click meanwhile sorts the rows shown and writes nothing to the column sets (V10).
        let backend = try #require(model.backend as? FakeBrowseBackend)
        let written = backend.columnChanges.count
        model.results.sort(by: "noteCrt")
        await waitUntil { model.results.list?.sort.key == "noteCrt" }
        #expect(backend.columnChanges.count == written)
        // Removing the token restores the query before Find.
        model.removeToken(0)
        #expect(model.query == previous && model.duplicates == nil && model.results.displayField == nil)
    }

    @Test func anotherChangeOfTheQueryDropsThePreviousQuery() async throws {
        let (model, session, _) = try await browse()
        session.results["run:0:brask"] = [1, 2]
        model.setQuery(FilterQuery(text: "brask"))
        await waitUntil { model.results.list?.count == 2 }
        model.commands.perform(.findDuplicates)
        let sheet = try #require(model.commands.findDuplicatesSheet)
        await waitUntil { sheet.field != nil }
        #expect(sheet.field == "Front", "the first result's note type's Sort Field with no current row")
        session.duplicates = FindDuplicatesResult(groups: [[10, 20]], noteIds: [10, 20], considered: 2)
        sheet.notes = .allFound
        await waitUntil { sheet.canFind }
        sheet.confirm()
        await waitUntil { model.duplicates != nil }
        #expect(session.duplicatesCalls == [.init(field: "Front", notes: [10, 20])])
        // Text typed beside the token: the previous query is dropped; removing the token then keeps the text.
        model.editText("vell")
        await waitUntil { model.query.text == "vell" }
        model.removeToken(0)
        #expect(model.query == FilterQuery(text: "vell"))
    }

    @Test func noGroupKeepsTheSheetAndNoResultsLeaveNoField() async throws {
        let (model, session, _) = try await browse()
        model.commands.perform(.findDuplicates)
        let sheet = try #require(model.commands.findDuplicatesSheet)
        await waitUntil { sheet.field != nil }
        sheet.confirm()
        await waitUntil { sheet.noneFound }
        #expect(model.commands.findDuplicatesSheet === sheet && sheet.phase == .editing)
        #expect(model.duplicates == nil && model.query == FilterQuery())
        sheet.cancel()
        #expect(model.commands.findDuplicatesSheet == nil)
        // No results: no field to start on, Find disabled until one is chosen.
        session.results["run:0:nothing"] = []
        model.setQuery(FilterQuery(text: "nothing"))
        await waitUntil { model.results.list?.count == 0 && model.results.list?.isComplete == true }
        model.commands.perform(.findDuplicates)
        let empty = try #require(model.commands.findDuplicatesSheet)
        await waitUntil { !empty.entries.isEmpty }
        #expect(empty.field == nil && !empty.canFind)
        empty.fieldEntry = empty.entries.first?.id
        #expect(empty.canFind && empty.allFound == nil)
        empty.notes = .allFound
        #expect(!empty.canFind, "All Found with no results finds nothing")
        empty.cancel()
    }

    /// Waits, suspended, until `condition` holds (at most five seconds), and
    /// reports the waiting line when it does not.
    private func waitUntil(_ condition: () -> Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
        for _ in 0..<500 where !condition() {
            try? await Task.sleep(for: .milliseconds(10))
        }
        #expect(condition(), sourceLocation: sourceLocation)
    }
}
