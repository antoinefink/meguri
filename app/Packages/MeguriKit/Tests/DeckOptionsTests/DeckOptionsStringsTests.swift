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
            "deckOptions.section.displayOrder": "Display Order",
            "deckOptions.option.newGatherPriority": "New card gather order", "deckOptions.option.newSortOrder": "New card sort order",
            "deckOptions.option.newMix": "New/review order", "deckOptions.option.interdayLearningMix": "Interday learning/review order",
            "deckOptions.option.reviewOrder": "Review sort order", "deckOptions.preset.new": "New Preset…",
            "deckOptions.preset.newTitle": "New Preset", "deckOptions.preset.name": "Name", "deckOptions.preset.create": "Create",
            "deckOptions.preset.createFailed": "The preset could not be prepared. Cancel to return to your options.",
            "deckOptions.preset.validationFailed": "The preset was prepared, but its options could not be checked. Save to try again.",
        ]
        for (key, text) in expected {
            #expect(DeckOptionsStrings.bundle.localizedString(forKey: key, value: "missing", table: nil) == text)
        }
        // N20.9 (changed 2026-11-27): the Advanced wrapper and the gathering footer are gone.
        for key in ["deckOptions.disclosure.advanced", "deckOptions.displayOrder.footer"] {
            #expect(DeckOptionsStrings.bundle.localizedString(forKey: key, value: "missing", table: nil) == "missing", "\(key)")
        }
    }

    /// N20.9 (changed 2026-11-27): each row's help names the row, says what it
    /// does, and explains every choice of K5's list, in the menu's order.
    @Test func displayOrderHelpCoversEveryChoiceOfEveryRow() {
        let bundle = DeckOptionsStrings.bundle
        func text(_ key: String) -> String { bundle.localizedString(forKey: key, value: "missing", table: nil) }
        let intros: [DeckOptionsDisplayOrder: String] = [
            .newGatherPriority: "Sets which new cards go into the daily limit. Cards added earlier usually have a lower position.",
            .newSortOrder: "Sets the order in which the gathered new cards appear.",
            .newMix: "Sets where new cards appear among the reviews. Interday learning cards count as reviews here.",
            .interdayLearningMix: "An interday learning card is a learning or relearning card whose current step ends on a later day. "
                + "This option sets where these cards appear among the reviews.",
            .reviewOrder: "Sets the order of the reviews. Interday learning cards use the same order.",
        ]
        let lines: [(DeckOptionsDisplayOrder, [Int32: String])] = [
            (.newGatherPriority, [
                0: "Takes one deck at a time, in deck list order. In each deck, the lowest position comes first.",
                5: "Takes one deck at a time, in deck list order. In each deck, the notes are in random order.",
                1: "Takes the lowest position first, from the deck and all its subdecks together.",
                2: "Takes the highest position first, from the deck and all its subdecks together.",
                3: "Takes notes in random order from the deck and all its subdecks. The cards of one note stay together.",
                4: "Takes cards in random order from the deck and all its subdecks."]),
            (.newSortOrder, [
                0: "Shows the cards of the first card type, then of the second, and so on. Cards of one type keep the gather order.",
                1: "Shows the cards in the gather order.",
                2: "Shows the cards of the first card type, then of the second, and so on. Cards of one type are in random order.",
                3: "Shows the notes in random order. The cards of one note stay together, in card type order.",
                4: "Shows the gathered cards in random order."]),
            (.newMix, [
                0: "Spreads the new cards evenly between the reviews.",
                1: "Shows all reviews first, then the new cards.",
                2: "Shows all new cards first, then the reviews."]),
            (.interdayLearningMix, [
                0: "Spreads these cards evenly between the reviews.",
                1: "Shows all reviews first, then these cards.",
                2: "Shows these cards first, then the reviews."]),
            (.reviewOrder, [
                0: "Shows the earliest due date first. Cards due on the same day are in random order.",
                1: "Shows the earliest due date first. Cards due on the same day are in deck list order.",
                2: "Shows one deck at a time, in deck list order. In each deck, the earliest due date comes first.",
                3: "Shows the cards with the shortest interval first.",
                4: "Shows the cards with the longest interval first.",
                6: "Shows the cards with the lowest difficulty first.",
                5: "Shows the cards with the highest difficulty first.",
                7: "Shows first the cards that you are least likely to remember now.",
                11: "Shows first the cards that you are most likely to remember now.",
                12: "Shows first the cards that are most overdue relative to their interval.",
                8: "Shows the cards in random order.",
                9: "Shows the cards of the oldest notes first.",
                10: "Shows the cards of the newest notes first."]),
        ]
        #expect(lines.map { $0.0 } == DeckOptionsDisplayOrder.allCases)
        for (row, explanations) in lines {
            #expect(text(DeckOptionsStrings.helpIntroKey(row)) == intros[row], "\(row)")
            for (value, explanation) in explanations {
                let key = DeckOptionsStrings.helpChoiceKey(row, value: value)
                #expect(key.map(text) == explanation, "\(row) \(value)")
            }
        }
        // A value the core might add without a label has no help line.
        #expect(DeckOptionsStrings.helpChoiceKey(.reviewOrder, value: 99) == nil)
        #expect(text("deckOptions.help.choice %@ %@") == "%1$@: %2$@")
        #expect(DeckOptionsStrings.helpLabel(.newGatherPriority, locale: english) == "Help: New card gather order")
        #expect(DeckOptionsStrings.helpLabel(.interdayLearningMix, locale: english) == "Help: Interday learning/review order")
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
