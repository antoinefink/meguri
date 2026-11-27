@testable import AppModel
import CoreBridge
import Testing

/// `notetypes/draft-save` D3's table, D4's rows and D8's comparison.
@Suite struct NoteTypeSaveDecisionTests {
    nonisolated private static let none = NotetypeDraftSummary(reasons: [], changes: [.fieldRenamed(from: "A", to: "B"), .textOrStylingChanged], losesContent: false)
    nonisolated private static let structural = NotetypeDraftSummary(
        reasons: [.fieldsChanged, .sortFieldChanged],
        changes: [.fieldAdded(name: "New"), .fieldRemoved(ord: 2, name: "Empty", notesWithContent: 0), .fieldsReordered, .sortFieldChanged(name: "New")],
        losesContent: false)
    nonisolated private static let losing = NotetypeDraftSummary(
        reasons: [.fieldsChanged],
        changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 12)], losesContent: true)

    /// D3: the draft's three kinds against the two service columns, every cell.
    @Test(arguments: [
        // (summary, service, asks, syncParagraph, confirm, destructive, route)
        (none, NoteTypeSaveService.meguriSync, false, false, NoteTypeSaveDecision.Confirm.save, false, NoteTypeSaveDecision.Route.local),
        (none, .other, false, false, .save, false, .local),
        (structural, .meguriSync, true, true, .saveAndUpload, false, .meguriSync),
        (structural, .other, false, false, .save, false, .local),
        (losing, .meguriSync, true, true, .saveAndUpload, true, .meguriSync),
        (losing, .other, true, false, .save, true, .local),
    ])
    func d3sTable(summary: NotetypeDraftSummary, service: NoteTypeSaveService, asks: Bool, syncParagraph: Bool,
                  confirm: NoteTypeSaveDecision.Confirm, destructive: Bool, route: NoteTypeSaveDecision.Route) {
        let decision = NoteTypeSaveDecision.decide(summary, service: service)
        #expect(decision.asks == asks)
        #expect(decision.syncParagraph == syncParagraph)
        #expect(decision.confirm == confirm)
        #expect(decision.destructive == destructive)
        #expect(decision.route == route)
    }

    @Test func aLossWithoutReasonsStillAsks() {
        let odd = NotetypeDraftSummary(reasons: [], changes: [.fieldRemoved(ord: 0, name: "A", notesWithContent: 3)], losesContent: true)
        #expect(NoteTypeSaveDecision.decide(odd, service: .other).asks)
        #expect(NoteTypeSaveDecision.decide(odd, service: .other).destructive)
    }

    /// D8: per category, by count; a category absent from the confirmed losses counts as 0.
    @Test func lossesAreComparedPerCategoryByCount() {
        let confirmed = NotetypeConfirmedLosses(fields: [1: 12], templates: [0: 5])
        func summary(_ changes: [NotetypeDraftChange]) -> NotetypeDraftSummary {
            NotetypeDraftSummary(reasons: [.fieldsChanged], changes: changes, losesContent: true)
        }
        #expect(!NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 1, name: "B", notesWithContent: 12)]), confirmed))
        #expect(!NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 1, name: "B", notesWithContent: 3)]), confirmed), "shrinking never asks")
        #expect(NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 1, name: "B", notesWithContent: 13)]), confirmed))
        #expect(NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 4, name: "E", notesWithContent: 1)]), confirmed), "a new loss")
        #expect(!NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 4, name: "E", notesWithContent: 0)]), confirmed))
        #expect(NoteTypeSaveDecision.exceeds(summary([.templateRemoved(ord: 0, name: "Card 1", cardsRemoved: 6, notesWithoutCards: 0)]), confirmed))
        // notes_without_cards is shown, never compared.
        #expect(!NoteTypeSaveDecision.exceeds(summary([.templateRemoved(ord: 0, name: "Card 1", cardsRemoved: 5, notesWithoutCards: 99)]), confirmed))
        // The counts are a total per category, never summed across categories.
        #expect(!NoteTypeSaveDecision.exceeds(summary([.fieldRemoved(ord: 1, name: "B", notesWithContent: 12),
                                                       .templateRemoved(ord: 0, name: "Card 1", cardsRemoved: 5, notesWithoutCards: 0)]), confirmed))
    }

    /// D5 step 4: the read right before submitting shows the sheet again only when D3 still asks and something grew or moved.
    @Test func theSheetShowsAgainWhenLossesGrowOrTheServiceChanges() {
        let shown = NoteTypeSaveDecision.decide(Self.losing, service: .other)
        let confirmed = NotetypeConfirmedLosses(confirming: Self.losing)
        #expect(confirmed.fields == [1: 12])
        #expect(!NoteTypeSaveDecision.asksAgain(confirmed: confirmed, shown: shown, latest: Self.losing, service: .other))
        let grown = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 13)], losesContent: true)
        #expect(NoteTypeSaveDecision.asksAgain(confirmed: confirmed, shown: shown, latest: grown, service: .other))
        // Meguri Sync became ready since the sheet showed: the sheet gains its paragraph.
        #expect(NoteTypeSaveDecision.asksAgain(confirmed: confirmed, shown: shown, latest: Self.losing, service: .meguriSync))
        // The losses went away and the service asks nothing: no sheet, the save goes on.
        let gone = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 0)], losesContent: false)
        #expect(!NoteTypeSaveDecision.asksAgain(confirmed: confirmed, shown: shown, latest: gone, service: .other))
        // A Meguri Sync sheet with no loss: the binding went away, and Off asks nothing.
        let meguri = NoteTypeSaveDecision.decide(Self.structural, service: .meguriSync)
        #expect(!NoteTypeSaveDecision.asksAgain(confirmed: .init(), shown: meguri, latest: Self.structural, service: .other))
    }

    /// D4: one row per listed change, losses first; renames, text and styling are not listed.
    @Test func d4sRowsPutTheLossesFirst() {
        let summary = NotetypeDraftSummary(
            reasons: [.fieldsChanged, .templatesChanged],
            changes: [
                .fieldAdded(name: "Origin"), .fieldRemoved(ord: 3, name: "Tavrin Notes", notesWithContent: 12),
                .fieldRemoved(ord: 9, name: "Source", notesWithContent: 0), .fieldRenamed(from: "A", to: "B"), .fieldsReordered,
                .sortFieldChanged(name: "Word"), .templateAdded(name: "Card 3", cardsCreated: 4),
                .templateRemoved(ord: 1, name: "Production", cardsRemoved: 374, notesWithoutCards: 2),
                .templateRemoved(ord: 2, name: "Spare", cardsRemoved: 0, notesWithoutCards: 0),
                .templateRenamed(from: "C", to: "D"), .templatesReordered, .notetypeRenamed(from: "X", to: "Y"), .textOrStylingChanged,
            ], losesContent: true)
        #expect(NoteTypeSaveRow.rows(summary).map(\.kind) == [
            .deleteField(name: "Tavrin Notes", notes: 12),
            .deleteCardType(name: "Production", cards: 374, notesWithoutCards: 2),
            .addField(name: "Origin"), .deleteField(name: "Source", notes: 0), .reorderFields, .sortBy(name: "Word"),
            .addCardType(name: "Card 3", cards: 4), .deleteCardType(name: "Spare", cards: 0, notesWithoutCards: 0), .reorderCardTypes,
        ])
        #expect(NoteTypeSaveRow.rows(summary).map(\.loses) == [true, true, false, false, false, false, false, false, false])
    }
}
