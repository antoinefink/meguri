import AppModel
import CoreBridge
import Foundation
import Testing
@testable import NoteTypes

/// `notetypes/draft-save` D4 and D9, `notetypes/workspace` W15: the section's String Catalog.
@Suite struct NoteTypesStringsTests {
    private let english = Locale(identifier: "en_US")

    @Test func d4sRowsAndDetailLinesReadAsTheSpecSays() {
        func line(_ kind: NoteTypeSaveRow.Kind) -> (String, [String]) {
            let line = NoteTypesStrings.line(NoteTypeSaveRow(kind: kind), locale: english)
            return (line.title, line.details)
        }
        #expect(line(.deleteCardType(name: "Production", cards: 374, notesWithoutCards: 2))
            == ("Delete Card Type “Production”", ["374 cards and their progress are deleted.", "2 notes will have no cards."]))
        #expect(line(.deleteCardType(name: "Production", cards: 1, notesWithoutCards: 0))
            == ("Delete Card Type “Production”", ["1 card and its progress are deleted."]))
        #expect(line(.deleteCardType(name: "Spare", cards: 0, notesWithoutCards: 0)) == ("Delete Card Type “Spare”", ["No cards."]))
        #expect(line(.deleteField(name: "Tavrin Notes", notes: 1234)) == ("Delete Field “Tavrin Notes”", ["Its content in 1,234 notes is deleted."]))
        #expect(line(.deleteField(name: "Source", notes: 0)) == ("Delete Field “Source”", ["Empty in every note."]))
        #expect(line(.addCardType(name: "Card 3", cards: 4)) == ("Add Card Type “Card 3”", ["4 cards will be created."]))
        #expect(line(.addCardType(name: "Card 3", cards: 0)) == ("Add Card Type “Card 3”", []))
        #expect(line(.addField(name: "Origin")) == ("Add Field “Origin”", []))
        #expect(line(.reorderFields) == ("Change the Order of Fields", []))
        #expect(line(.reorderCardTypes) == ("Change the Order of Card Types", []))
        #expect(line(.sortBy(name: "Word")) == ("Sort by “Word”", []))
    }

    @Test func theSheetsTitleParagraphsAndButtons() {
        #expect(NoteTypesStrings.saveTitle("Vocabulary", locale: english) == "Save Changes to “Vocabulary”?")
        #expect(NoteTypesStrings.syncBody(device: "Mac", locale: english)
            == "These changes alter the structure of your collection, so they cannot be merged. Meguri syncs this Mac first, saves, then uploads this collection in place of the one on Meguri Sync.")
        #expect(NoteTypesStrings.backup(device: "iPad", locale: english) == "A backup of the collection is kept on this iPad.")
        let bundle = NoteTypesStrings.bundle
        let expected = [
            "noteTypes.save.sync.title": "Meguri Sync will be replaced.",
            "noteTypes.save.sync.others": "Sync your other devices before you continue. Changes on them that have not synced will be lost.",
            "noteTypes.save.cancel": "Cancel", "noteTypes.save.confirm.save": "Save",
            "noteTypes.save.confirm.saveAndUpload": "Save and Upload", "noteTypes.saveAsks": "Save…", "noteTypes.save": "Save",
            "noteTypes.revert": "Revert", "noteTypes.discard.title": "Discard changes?", "noteTypes.discard.discard": "Discard",
            "noteTypes.discard.keepEditing": "Keep Editing", "noteTypes.changed.message": "This note type changed on another device.",
            "noteTypes.changed.reload": "Reload", "noteTypes.changed.keepEditing": "Keep Editing",
            "noteTypes.title": "Note Types", "noteTypes.tab.fields": "Fields", "noteTypes.tab.cards": "Cards",
            "noteTypes.list.filter": "Filter", "noteTypes.list.edited": "Edited",
            "noteTypes.fields.name": "Name", "noteTypes.fields.sort": "Sort", "noteTypes.fields.usedOn": "Used On",
            "noteTypes.fields.notes": "Notes", "noteTypes.fields.notUsed": "Not Used", "noteTypes.fields.restore": "Restore",
            "noteTypes.inspector.collapsed": "Collapsed in Editor", "noteTypes.inspector.collapsedHelp": "Closed in Add Note until you open it",
            "noteTypes.inspector.rtl": "Right-to-Left", "noteTypes.inspector.placeholder": "Placeholder",
            "noteTypes.inspector.placeholderHelp": "Grey text shown while the field is empty",
            "noteTypes.save.meguriUnavailable": "Saving structure changes with Meguri Sync isn’t available yet.",
            "noteTypes.fields.fieldOrder": "Show in Field Order",
        ]
        for (key, value) in expected {
            #expect(bundle.localizedString(forKey: key, value: "missing", table: nil) == value, "\(key)")
        }
    }

    @Test func subtitleAndProgressCountWithPlurals() {
        #expect(NoteTypesStrings.subtitle(notes: 412, cards: 786, locale: english) == "412 notes · 786 cards")
        #expect(NoteTypesStrings.subtitle(notes: 1, cards: 1, locale: english) == "1 note · 1 card")
        #expect(NoteTypesStrings.subtitle(notes: 0, cards: 0, locale: english) == "0 notes · 0 cards")
        #expect(NoteTypesStrings.progress(completed: 212, total: 412, locale: english) == "Updating 212 of 412 notes")
    }
}
