import AppModel
import Foundation
import Testing
@testable import Home

/// N20.7's exact English text.
@Suite struct DeckDeletionTextTests {
    private let english = Locale(identifier: "en_US")

    @Test(arguments: [
        (UInt64(0), UInt64(1), "Its 1 card will also be deleted."),
        (0, 412, "Its 412 cards will also be deleted."),
        (1, 1, "Its subdeck and 1 card will also be deleted."),
        (1, 2, "Its subdeck and 2 cards will also be deleted."),
        (2, 1, "Its 2 subdecks and 1 card will also be deleted."),
        (3, 1_204, "Its 3 subdecks and 1,204 cards will also be deleted."),
        (1_500, 2, "Its 1,500 subdecks and 2 cards will also be deleted."),
    ])
    func messageVariesBySubdecksAndCards(subdecks: UInt64, cards: UInt64, expected: String) {
        #expect(DeckDeletionText.message(subdecks: subdecks, cards: cards, locale: english) == expected)
    }

    @Test func titlesAndFailureText() {
        #expect(DeckDeletionText.title(name: "Kanji", locale: english) == "Delete “Kanji”?")
        #expect(DeckDeletionText.failureTitle(name: "Kanji", locale: english) == "Couldn’t Delete “Kanji”")
        #expect(DeckDeletionText.failureMessage(.backupFailed, locale: english)
            == "Meguri couldn’t make a backup first, so nothing was deleted.")
        #expect(DeckDeletionText.failureMessage(.other, locale: english) == "Nothing was deleted.")
        #expect([DeckDeletionText.deleteDeck, DeckDeletionText.delete, DeckDeletionText.cancel, DeckDeletionText.ok]
            == ["Delete Deck", "Delete", "Cancel", "OK"])
    }
}
