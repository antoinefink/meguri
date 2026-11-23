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
        #expect(BrowseStrings.showAll(46, locale: english) == "Show All 46…")
        #expect(BrowseStrings.showAll(1_204, locale: english) == "Show All 1,204…")
    }

    /// V8.1's titles; with nothing suspended or buried, the menu bar's
    /// disabled items read plain "Unsuspend" and "Unbury" (N21.8).
    @Test func commandTitlesCountTheirTargets() {
        func title(_ command: BrowseCommand, _ count: UInt64?, asks: Bool = false) -> String {
            BrowseCommandText.title(command, BrowseCommandState(shown: true, enabled: true, count: count, asks: asks), locale: english)
        }
        #expect(title(.suspend, 1) == "Suspend Card" && title(.suspend, 3) == "Suspend Cards")
        #expect(title(.unsuspend, 1) == "Unsuspend Card" && title(.unsuspend, 2) == "Unsuspend 2 Cards")
        #expect(title(.unsuspend, 0) == "Unsuspend" && title(.unbury, 0) == "Unbury")
        #expect(title(.unsuspend, nil) == "Unsuspend Cards")
        #expect(title(.delete, 1) == "Delete Note" && title(.delete, 3, asks: true) == "Delete 3 Notes…")
        #expect(title(.delete, 1, asks: true) == "Delete Note…")
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
