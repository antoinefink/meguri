import Testing

import Home

/// `decks/deck-list` H6.2: the deck search's filter over full human names.
@Suite struct GoToDeckFilterTests {
    @Test(arguments: [
        ("Japanese::Lesson1", "ja 1", true),
        ("Japanese::Lesson1", "JA les", true),
        ("Japanese::Lesson1", "ja 2", false),
        ("Japanese::Lesson1", "se::le", true),
        ("Japanese::Lesson1", "ja  1", true),
        ("Straße", "strasse", false),
        ("Café", "cafe", false),
        ("日本語::N5", "n5 日本", true),
    ])
    func theSpecTable(name: String, query: String, listed: Bool) {
        #expect(GoToDeckFilter(query: query).matches(fullName: name) == listed)
    }

    @Test func termsSplitOnlyAtSpaces() {
        #expect(GoToDeckFilter(query: "  ja   1  ").matches(fullName: "Japanese::Lesson1"))
        // Any other whitespace is part of its term.
        #expect(!GoToDeckFilter(query: "ja\tles").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "ja\u{3000}1").matches(fullName: "Japanese::Lesson1"))
    }

    @Test(arguments: ["", " ", "   "])
    func anEmptyQueryListsEveryEntry(query: String) {
        let filter = GoToDeckFilter(query: query)
        #expect(filter.matches(fullName: "Japanese::Lesson1"))
        #expect(filter.matches(fullName: ""))
    }

    @Test func aTermMaySpanTheSeparator() {
        #expect(GoToDeckFilter(query: "ese::Les").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "::").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "eseLes").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "::").matches(fullName: "Japanese"))
    }

    @Test func lowercasingIsTheFullMappingNotFolding() {
        #expect(GoToDeckFilter(query: "ÛMÉ").matches(fullName: "Žûmér::日本語"))
        #expect(!GoToDeckFilter(query: "ss").matches(fullName: "Straße"))
        #expect(GoToDeckFilter(query: "ß").matches(fullName: "STRAßE"))
        // U+0130 lowers to i U+0307.
        #expect(GoToDeckFilter(query: "i\u{0307}stanbul").matches(fullName: "\u{0130}stanbul"))
        // Final_Sigma: a capital sigma at the end of a word lowers to ς.
        #expect(GoToDeckFilter(query: "οδος").matches(fullName: "ΟΔΟΣ"))
    }

    @Test func theQueryIsNFC() {
        #expect(GoToDeckFilter(query: "Cafe\u{0301}").matches(fullName: "Caf\u{00E9}"))
    }

    @Test func punctuationIsLiteral() {
        #expect(GoToDeckFilter(query: "a.b").matches(fullName: "Study::a.b"))
        #expect(!GoToDeckFilter(query: "a.b").matches(fullName: "Study::axb"))
    }
}
