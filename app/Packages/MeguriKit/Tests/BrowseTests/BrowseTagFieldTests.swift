#if os(macOS)
import AppKit
@testable import Browse
import Testing

@MainActor
struct BrowseTagFieldTests {
    @Test
    func componentPrefixesExcludeHeldTagsWithoutMatchingInsideComponents() throws {
        let names = ["Spanish::Grammar::Nouns", "Spanish::Grammar::Verbs", "Spanish::Vocabulary", "German::Adverbs"]
        func matches(_ text: String, held: [String] = []) throws -> [String] {
            let query = try #require(BrowseTagField.Query(text: text, selection: NSRange(location: text.utf16.count, length: 0)))
            return query.suggestions(in: names, excluding: held)
        }
        #expect(try matches("SPANISH::GRAMMAR::", held: [names[0].lowercased()]) == [names[1]])
        #expect(try matches("gRaMmAr::v") == [names[1]])
        #expect(try matches("VER") == [names[1]])
        #expect(try matches("erb").isEmpty)
    }

    @Test
    func selectionMustStillMatchCurrentQueryNamesAndHeldTokens() throws {
        let name = "Spanish::Grammar::Verbs"
        let query = try #require(BrowseTagField.Query(text: "VER", selection: NSRange(location: 3, length: 0)))
        #expect(query.isEligible(name, in: [name], excluding: []))
        #expect(!query.isEligible(name, in: [], excluding: []))
        #expect(!query.isEligible(name, in: [name], excluding: [name.lowercased()]))
        let changed = try #require(BrowseTagField.Query(text: "Nouns", selection: NSRange(location: 5, length: 0)))
        #expect(!changed.isEligible(name, in: [name], excluding: []))
        #expect(changed.suggestions(in: [name], excluding: []).isEmpty)
    }

    @Test
    func suggestionsStopAtFiftyEligibleNamesInInputOrder() throws {
        let names = (0..<80).map { "Spanish::Grammar::Verbs\($0)" }
        let held = Array(names.prefix(5))
        let query = try #require(BrowseTagField.Query(text: "grammar::v", selection: NSRange(location: 10, length: 0)))
        #expect(query.suggestions(in: names, excluding: held) == Array(names[5..<55]))
        #expect(query.suggestions(in: ["Unrelated"] + names, excluding: held) == Array(names[5..<55]))
    }

    @Test
    func replacementRangePreservesBothAdjacentTokensAndUnicode() throws {
        let text = "\u{FFFC} 語::🦉partial \u{FFFC}"
        let caret = ("\u{FFFC} 語::🦉" as NSString).length
        let query = try #require(BrowseTagField.Query(text: text, selection: NSRange(location: caret, length: 0)))
        #expect(query.text == "語::🦉partial")
        #expect((text as NSString).replacingCharacters(in: query.range, with: "語::完成") == "\u{FFFC} 語::完成 \u{FFFC}")
        #expect(BrowseTagField.Query(text: "\u{FFFC}", selection: NSRange(location: 1, length: 0)) == nil)
        #expect(BrowseTagField.Query(text: "word", selection: NSRange(location: 0, length: 4)) == nil)
    }

    @Test
    func highlightingDoesNotCompleteTheEntireFieldAndEditingEndKeepsOriginalNote() {
        var committed: [[String]] = []
        let coordinator = BrowseTagField.Coordinator(BrowseTagField(tags: [], names: []) { committed.append($0) })
        let field = NSTokenField()
        coordinator.controlTextDidBeginEditing(Notification(name: NSControl.textDidBeginEditingNotification, object: field))
        // A SwiftUI refresh must not redirect this edit to a different note.
        coordinator.parent = BrowseTagField(tags: [], names: []) { _ in Issue.record("wrong note") }
        let item = NSSuggestionItem(representedValue: "Spanish::Grammar::Verbs", title: "Spanish::Grammar::Verbs")
        #expect(coordinator.textField(field, textCompletionFor: item) == nil)
        field.objectValue = ["Kept::Token", "Spanish::Grammar::Verbs", "kept::token", "New Tag"]
        coordinator.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification, object: field))
        #expect(committed == [["Kept::Token", "Spanish::Grammar::Verbs", "New", "Tag"]])
    }
}
#endif
