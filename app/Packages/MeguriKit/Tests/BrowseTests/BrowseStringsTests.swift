import AppModel
import CoreBridge
import Foundation
import Testing
@testable import Browse

/// Browse's String Catalog (`browser/workspace` V16, `search/filter-tokens` T15).
@MainActor @Suite struct BrowseStringsTests {
    private let english = Locale(identifier: "en_US")

    @Test func theSidebarsSectionsAndValuesReadAsTheSpecSays() {
        #expect(BrowserSections.allTitles(english) == ["Saved Searches", "Card State", "Flags", "Decks", "Note Types", "Tags"])
        #expect(BrowseModel.states.map { BrowseStrings.state($0, locale: english) } == ["New", "Learning", "Review", "Suspended", "Buried"])
        #expect(FilterBuiltIn.allCases.map { BrowseStrings.builtIn($0, locale: english) } == ["Due Today", "Added in Last 7 Days", "Leeches"])
        #expect(BrowseStrings.text("browse.flag.none", locale: english) == "No Flag")
        #expect(BrowseStrings.text("browse.title", locale: english) == "Browse")
        #expect(BrowseStrings.text("browse.sidebar.filter", locale: english) == "Filter")
        #expect(BrowseStrings.showAll(46, locale: english) == "Show All 46…")
        #expect(BrowseStrings.showAll(1_204, locale: english) == "Show All 1,204…")
    }

    @Test func countsReadAloudWithPlurals() {
        #expect(BrowseStrings.count(2_203, mode: .cards, locale: english) == "2,203 cards")
        #expect(BrowseStrings.count(1, mode: .cards, locale: english) == "1 card")
        #expect(BrowseStrings.count(1, mode: .notes, locale: english) == "1 note")
        #expect(BrowseStrings.count(0, mode: .notes, locale: english) == "0 notes")
    }
}

private enum BrowserSections {
    static func allTitles(_ locale: Locale) -> [String] {
        BrowseSidebarSection.allCases.map { BrowseStrings.section($0, locale: locale) }
    }
}
