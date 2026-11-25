import AppModel
import Foundation
import Testing
@testable import DeckOptions

/// `decks/deck-options-strings` K6 and K10 (U232's String Catalog test for the page's keys).
@Suite struct DeckOptionsStringsTests {
    private let english = Locale(identifier: "en_US")

    @Test func presetMenuItemsAndFooterPluralizeOnTheCountAndGroupIt() {
        #expect(DeckOptionsStrings.usedBy(name: "Default", count: 1, locale: english) == "Default (used by 1 deck)")
        #expect(DeckOptionsStrings.usedBy(name: "Test", count: 0, locale: english) == "Test (used by 0 decks)")
        #expect(DeckOptionsStrings.usedBy(name: "Languages", count: 1234, locale: english) == "Languages (used by 1,234 decks)")
        #expect(DeckOptionsStrings.presetFooter(count: 1, locale: english) == "Used by this deck only.")
        #expect(DeckOptionsStrings.presetFooter(count: 3, locale: english) == "Used by 3 decks. Changes apply to all of them.")
        #expect(DeckOptionsStrings.presetFooter(count: 1234, locale: english)
            == "Used by 1,234 decks. Changes apply to all of them.")
    }

    @Test func pageTextsAreK10s() {
        let bundle = DeckOptionsStrings.bundle
        let expected = [
            "deckOptions.v1.title": "Deck Options", "deckOptions.v1.preset": "Preset",
            "deckOptions.v1.section.dailyLimits": "Daily Limits", "deckOptions.v1.newPerDay": "New Cards per Day",
            "deckOptions.v1.reviewsPerDay": "Maximum Reviews per Day", "deckOptions.v1.section.memory": "Memory",
            "deckOptions.v1.desiredRetention": "Desired Retention",
            "deckOptions.v1.desiredRetentionFooter": "How likely you are to remember a card when it comes back.",
            "deckOptions.v1.ownLimits": "This deck has its own limits, which stay in force.",
            "deckOptions.v1.ownRetention": "This deck has its own desired retention, which stays in force.",
            "deckOptions.v1.save": "Save", "deckOptions.discard.title": "Discard changes?",
            "deckOptions.discard.discard": "Discard", "deckOptions.discard.keepEditing": "Keep Editing",
        ]
        for (key, value) in expected {
            #expect(bundle.localizedString(forKey: key, value: "missing", table: nil) == value, "\(key)")
        }
        #expect(DeckOptionsStrings.memoryFooter(ownRetention: true, locale: english)
            == "How likely you are to remember a card when it comes back. This deck has its own desired retention, which stays in force.")
        #expect(DeckOptionsStrings.path("Languages::Japanese::Kanji") == "Languages › Japanese › Kanji")
    }

    @Test func displayOrderLabelsAndChoiceMappingMatchK3AndK5() {
        let lists: [(DeckOptionsDisplayOrder, [Int32], [String])] = [
            (.newGatherPriority, [0, 5, 1, 2, 3, 4],
             ["Deck", "Deck, then random notes", "Ascending position", "Descending position", "Random notes", "Random cards"]),
            (.newSortOrder, [0, 1, 2, 3, 4],
             ["Card type, then order gathered", "Order gathered", "Card type, then random", "Random note, then card type", "Random"]),
            (.newMix, [0, 1, 2], ["Mix with reviews", "Show after reviews", "Show before reviews"]),
            (.interdayLearningMix, [0, 1, 2], ["Mix with reviews", "Show after reviews", "Show before reviews"]),
            (.reviewOrder, [0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10],
             ["Due date, then random", "Due date, then deck", "Deck, then due date", "Ascending intervals", "Descending intervals",
              "Easy cards first", "Difficult cards first", "Ascending retrievability", "Descending retrievability",
              "Relative overdueness", "Random", "Order added", "Latest added first"]),
        ]
        #expect(lists.map { $0.0 } == DeckOptionsDisplayOrder.allCases)
        for (row, values, expected) in lists {
            let labels = values.map { value in
                DeckOptionsStrings.bundle.localizedString(forKey: DeckOptionsStrings.choiceCatalogKey(row, value: value),
                    value: "missing", table: nil)
            }
            #expect(labels == expected)
        }
        let expected = [
            "deckOptions.section.displayOrder": "Display Order", "deckOptions.disclosure.advanced": "Advanced",
            "deckOptions.option.newGatherPriority": "New card gather order", "deckOptions.option.newSortOrder": "New card sort order",
            "deckOptions.option.newMix": "New/review order", "deckOptions.option.interdayLearningMix": "Interday learning/review order",
            "deckOptions.option.reviewOrder": "Review sort order", "deckOptions.preset.new": "New Preset…",
            "deckOptions.preset.newTitle": "New Preset", "deckOptions.preset.name": "Name", "deckOptions.preset.create": "Create",
            "deckOptions.displayOrder.footer": "Gathering chooses which new cards enter the daily limit. Sorting sets the order in which they appear.",
        ]
        for (key, text) in expected {
            #expect(DeckOptionsStrings.bundle.localizedString(forKey: key, value: "missing", table: nil) == text)
        }
    }

    @Test func issueTextsNameTheRangeInTheRowsUnit() {
        let limit = DeckOptionsPageIssue(row: .newPerDay, preset: 1, code: .outOfRange, severity: .error)
        #expect(DeckOptionsStrings.issue(limit, range: 0...9999, locale: english) == "Enter a number from 0 to 9,999.")
        let retention = DeckOptionsPageIssue(row: .desiredRetention, preset: 1, code: .outOfRange, severity: .error)
        #expect(DeckOptionsStrings.issue(retention, range: 0.7...0.99, locale: english) == "Enter a percentage from 70% to 99%.")
        let reviews = DeckOptionsPageIssue(row: .reviewsPerDay, preset: 1, code: .reviewsBelowTenTimesNew,
            severity: .warning, params: ["expected": "500"])
        #expect(DeckOptionsStrings.issue(reviews, range: nil, locale: english)
            == "Use 500 or more reviews per day to keep up with your new cards.")
    }
}
