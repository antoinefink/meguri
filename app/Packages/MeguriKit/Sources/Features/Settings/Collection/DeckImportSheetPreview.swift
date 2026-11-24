import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The deck package import sheet over fixed values (`settings.importExport`),
/// for the snapshot matrix and the capture routes: no core, no file.
public enum DeckImportSheetPreview: PreviewStates {
    public static let previewID = "settings.importExport.deck"

    /// Every named fixture the capture routes draw.
    public static let fixtures = [
        "options-small", "options-large", "options-changed", "importing", "confirm-losses", "confirm-sync",
        "confirm-sync-losses", "confirm-unreachable", "result", "result-log", "result-undone", "refused-too-new",
        "refused-space", "refused-kind-conflict", "refused-rows", "refused-sync", "imported-without-result",
    ]

    public static var previewFixtures: [PreviewFixture] {
        [PreviewFixture(.typical) { fixture("options-small") },
         PreviewFixture(.overflow) { fixture("options-large") },
         PreviewFixture(.loading) { fixture("importing") },
         PreviewFixture(.error) { fixture("refused-too-new") },
         PreviewFixture(.empty) { fixture("result") }]
    }

    @MainActor public static func fixture(_ name: String) -> some View {
        Fixture(name: name)
    }

    private struct Fixture: View {
        let name: String
        @State private var options = DeckImportOptions(withScheduling: true)
        var body: some View {
            DeckImportContent(presentation: DeckImportSheetPreview.presentation(name), options: $options, actions: .init(),
                              initiallyExpanded: name == "result-log" ? [DeckImportText.duplicate(2)] : [])
                .onAppear {
                    if name == "options-changed" {
                        options = DeckImportOptions(withScheduling: false, withDeckConfigs: true, mergeNotetypes: true,
                                                    updateNotes: .always, updateNotetypes: .never)
                    }
                    if name == "refused-kind-conflict" { options.mergeNotetypes = true }
                }
        }
    }

    static let small = DeckImportFacts(notes: 42, cards: 84, decks: ["Spanish Verbs"], notetypes: ["Basic (and reversed card)"])
    static let large = DeckImportFacts(
        format: .legacy2, notes: 12_480, cards: 31_204,
        decks: ["Japanese::Core 2k", "Japanese::Core 2k::Listening", "Japanese::Kana", "Japanese::Grammar", "Japanese::Kanji"],
        notetypes: ["Japanese (recognition)", "Japanese (production)", "Cloze"],
        hasScheduling: true, hasPresets: true, mediaFiles: 2_310, mediaBytes: 412_000_000,
        repaired: .init(noteIds: 3, fieldCounts: 1))

    static func presentation(_ name: String) -> DeckImportPresentation {
        var value = DeckImportPresentation()
        value.fileName = name == "options-large" ? "Japanese Core 2k.apkg" : "Spanish Verbs.apkg"
        value.facts = name == "options-large" ? large : small
        value.step = .options
        switch name {
        case "options-large":
            value.otherFilesGiven = true
        case "importing":
            value.step = .importing; value.progressVisible = true
            value.progressStage = .media; value.progressCompleted = 40; value.progressTotal = 100
        case "confirm-losses":
            value.step = .confirming; value.confirmation = losses
        case "confirm-sync":
            value.step = .confirming; value.confirmation = DeckImportPreview(); value.confirmationReasons = true
            value.syncParagraph = .replaces
        case "confirm-sync-losses":
            value.step = .confirming; value.confirmation = losses; value.confirmationReasons = true; value.syncParagraph = .replaces
        case "confirm-unreachable":
            value.step = .confirming; value.confirmation = DeckImportPreview(); value.confirmationReasons = true
            value.syncParagraph = .unreachable
        case "result", "result-log", "result-undone":
            value.step = .result; value.result = result; value.canUndo = name != "result-undone"
            value.undone = name == "result-undone"; value.canShowInBrowse = true
            value.log = [.duplicate: [.init(noteId: 11, fields: ["zorvel", "a made-up word"]), .init(noteId: 12, fields: [])],
                         .new: [.init(noteId: 13, fields: ["quandle", "another made-up word"])]]
        case "refused-too-new":
            value.step = .refused; value.facts = nil; value.refusal = .engine(.tooNew); value.refusalSource = .inspect
        case "refused-space":
            value.step = .refused
            value.refusal = .engine(.insufficientSpace(needed: 2_400_000_000, available: 900_000_000)); value.refusalSource = .apply
        case "refused-kind-conflict":
            value.step = .refused; value.refusal = .engine(.notetypeKindConflict(incoming: 1, target: 2)); value.refusalSource = .apply
        case "refused-rows":
            value.step = .refused; value.refusal = .engine(.tooLarge(what: .rows, found: 9, limit: 1)); value.refusalSource = .apply
        case "refused-sync":
            value.step = .refused; value.refusal = .sync(.fullSyncRequired); value.refusalSource = .apply
        case "imported-without-result":
            value.step = .importedWithoutResult
        default: break
        }
        return value
    }

    static let losses = DeckImportPreview(
        emptiedFields: [.init(notetypeId: 5, notetypeName: "Basic (and reversed card)", position: 2, fieldName: "Example", notes: 18)],
        removedNotetypes: [.init(notetypeId: 6, name: "Basic (and reversed card)-a1b2c", movedTo: 5,
                                 movedToName: "Basic (and reversed card)", notes: 7)],
        droppedValues: [.init(notetypeId: 5, notetypeName: "Basic (and reversed card)", notes: 3)])

    static var result: DeckImportResult {
        var counts = DeckImportCounts()
        counts.notesAdded = 38; counts.cardsAdded = 76; counts.decksAdded = 1; counts.notetypesAdded = 1
        counts.mediaWritten = 12; counts.mediaSkipped = 2
        return DeckImportResult(sequence: 9, generation: 1, undoStep: "Import", counts: counts,
            log: .init(foundNotes: 42, entries: [.new: 38, .duplicate: 2, .updated: 1, .conflicting: 1]))
    }
}
