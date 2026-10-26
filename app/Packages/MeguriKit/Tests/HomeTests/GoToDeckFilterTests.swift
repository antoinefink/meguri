import Testing

import Home

/// Product Contract R26/R27; predicate use preserves the snapshot's values and order.
@Suite struct GoToDeckFilterTests {
    @Test func everyTermMayMatchADifferentComponent() {
        #expect(GoToDeckFilter(query: "ja 1").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "JA les").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "1 ja").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "pan ese").matches(fullName: "Japanese::Lesson1"))
    }

    @Test func anyUnmatchedTermRejectsTheDeck() {
        #expect(!GoToDeckFilter(query: "ja 2").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "missing").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "ja").matches(fullName: ""))
    }

    @Test func termsSplitOnWhitespace() {
        #expect(GoToDeckFilter(query: "  JA\t\nles\r\n 1  ").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "ja　1").matches(fullName: "Japanese::Lesson1"))
    }

    @Test(arguments: ["", "   ", "\t\n\r", "　"])
    func emptyOrWhitespaceQueryMatchesAll(query: String) {
        let filter = GoToDeckFilter(query: query)
        #expect(filter.matches(fullName: "Japanese::Lesson1"))
        #expect(filter.matches(fullName: "日本語::初級"))
        #expect(filter.matches(fullName: ""))
    }

    @Test func unicodeUsesCaseInsensitiveSubstringComparison() {
        #expect(GoToDeckFilter(query: "ÛMÉ").matches(fullName: "Žûmér Žûmér::日本語"))
        #expect(GoToDeckFilter(query: "初 猫").matches(fullName: "日本語::初級::猫"))
        #expect(!GoToDeckFilter(query: "犬").matches(fullName: "日本語::初級::猫"))
        #expect(!GoToDeckFilter(query: "cafe").matches(fullName: "Café"))
    }

    @Test func aTermCannotCrossAComponentBoundary() {
        #expect(!GoToDeckFilter(query: "eseLes").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "ese::Les").matches(fullName: "Japanese::Lesson1"))
        #expect(!GoToDeckFilter(query: "::").matches(fullName: "Japanese::Lesson1"))
        #expect(GoToDeckFilter(query: "ese Les").matches(fullName: "Japanese::Lesson1"))
    }

    @Test func punctuationIsLiteral() {
        #expect(GoToDeckFilter(query: "a.b").matches(fullName: "Study::a.b"))
        #expect(!GoToDeckFilter(query: "a.b").matches(fullName: "Study::axb"))
    }

    @Test func filteringASnapshotPreservesNamesDuplicatesAndOrder() {
        let names = ["  日本語::Été  ", "Other", "日本語::été", "  日本語::Été  "]
        let query = "語 ÉTÉ"
        let filter = GoToDeckFilter(query: query)
        let matches = names.filter { filter.matches(fullName: $0) }

        #expect(matches == ["  日本語::Été  ", "日本語::été", "  日本語::Été  "])
        #expect(names == ["  日本語::Été  ", "Other", "日本語::été", "  日本語::Été  "])
        #expect(query == "語 ÉTÉ")
    }
}
