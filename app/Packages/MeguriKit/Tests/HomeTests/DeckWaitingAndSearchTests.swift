import AppModel
import Foundation
import Testing
@testable import CoreBridge
@testable import Home

@Suite struct DeckWaitingAndSearchTests {
    private func node(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32) = (0, 0, 0),
                      filtered: Bool = false, _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
    }
    private var tree: [CountedDeckNode] {
        [node(1, "Languages", (20, 3, 72), [
            node(2, "Languages::Español", (10, 1, 18)),
            node(3, "Languages::日本語", (15, 2, 48), [node(4, "Languages::日本語::文法", (5, 0, 12))]),
        ]), node(5, "Reading"), node(6, "Science", (4, 0, 3), [node(7, "Science::Chemistry", (4, 0, 3))])]
    }

    // MARK: Deck page summary and sidebar tone (N14.3, N14.6)

    @Test func theSummaryCountsNewLearningAndReviewTogether() {
        #expect(DeckWaiting(counts: ReviewCounts(new: 15, learning: 2, review: 48), filtered: false).total == 65)
        #expect(DeckWaiting(counts: ReviewCounts(new: 1, learning: 0, review: 0), filtered: false).total == 1)
        // The three counts are summed without wrapping.
        #expect(DeckWaiting(counts: ReviewCounts(new: .max, learning: .max, review: 1), filtered: false).total
            == 2 * Int(UInt32.max) + 1)
    }

    @Test func onlyARegularDeckWithNothingWaitingOffersAddNote() {
        let none = ReviewCounts(new: 0, learning: 0, review: 0)
        #expect(DeckWaiting(counts: none, filtered: false).offersAddNote)
        // A filtered deck cannot take notes: it keeps its counts and Study.
        #expect(!DeckWaiting(counts: none, filtered: true).offersAddNote)
        for counts in [ReviewCounts(new: 1, learning: 0, review: 0), ReviewCounts(new: 0, learning: 1, review: 0),
                       ReviewCounts(new: 0, learning: 0, review: 1)] {
            #expect(!DeckWaiting(counts: counts, filtered: false).offersAddNote)
        }
    }

    @Test func aSidebarRowIsQuietOnlyWithNothingWaitingAndUnselected() {
        let none = DeckWaiting(counts: ReviewCounts(new: 0, learning: 0, review: 0), filtered: false)
        let some = DeckWaiting(counts: ReviewCounts(new: 0, learning: 0, review: 4), filtered: false)
        #expect(none.quietInSidebar(selected: false))
        #expect(!none.quietInSidebar(selected: true))
        #expect(!some.quietInSidebar(selected: false))
        #expect(!some.quietInSidebar(selected: true))
        // New or learning cards alone also count as waiting.
        #expect(!DeckWaiting(counts: ReviewCounts(new: 2, learning: 0, review: 0), filtered: false).quietInSidebar(selected: false))
    }

    // MARK: Page headings (N14.4)

    @MainActor @Test func headingsFollowWhatThePageShows() {
        let locale = Locale(identifier: "en_US")
        #expect(HomeHeading.rootTitle(showsToday: true) == "Today")
        #expect(HomeHeading.rootTitle(showsToday: false) == "Decks")
        #expect(HomeHeading.deckTitle("Languages::日本語::文法") == "文法")
        #expect(HomeHeading.deckTitle("Reading") == "Reading")
        // Counts keep their zeros; a regular deck with nothing waiting says so.
        #expect(HomeHeading.deckSubtitle(DeckWaiting(counts: ReviewCounts(new: 15, learning: 0, review: 1480), filtered: false), locale: locale)
            == "15 new · 0 learning · 1,480 to review")
        #expect(HomeHeading.deckSubtitle(DeckWaiting(counts: ReviewCounts(new: 0, learning: 0, review: 0), filtered: false), locale: locale)
            == "Nothing to study right now")
        // A filtered deck keeps its counts.
        #expect(HomeHeading.deckSubtitle(DeckWaiting(counts: ReviewCounts(new: 0, learning: 0, review: 0), filtered: true), locale: locale)
            == "0 new · 0 learning · 0 to review")
        // Today has no subtitle at all while nothing waits.
        #expect(HomeHeading.todaySubtitle(ReviewCounts(new: 0, learning: 0, review: 0), locale: locale) == "")
        #expect(HomeHeading.todaySubtitle(ReviewCounts(new: 24, learning: 3, review: 75), locale: locale) == "24 new · 3 learning · 75 to review")
    }

    @MainActor @Test func theSearchFieldKeepsItsQueryOutsideTheView() {
        let search = DeckListSearch()
        #expect(search.query == nil)
        search.text = "  chem \n"
        #expect(search.query == "chem")
        search.text = "   "
        #expect(search.query == nil)
        // AppNavigation hands every layout the same instance.
        let navigation = AppNavigation()
        let first = navigation.featureState(DeckListSearch.self) { DeckListSearch() }
        first.text = "Korean"; first.presented = true
        let second = navigation.featureState(DeckListSearch.self) { DeckListSearch() }
        #expect(second === first)
        #expect(second.query == "Korean" && second.presented)
    }

    @Test func theCatalogHasTheSingularWaitingForm() throws {
        // SwiftPM builds do not compile string catalogs: read the source the app ships.
        let catalog = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "Sources/Features/Home/Resources/Localizable.xcstrings")
        let root = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: catalog)) as? [String: Any])
        let strings = try #require(root["strings"] as? [String: Any])
        let entry = try #require(strings["%lld cards waiting"] as? [String: Any])
        let english = try #require((entry["localizations"] as? [String: Any])?["en"] as? [String: Any])
        let plural = try #require((english["variations"] as? [String: Any])?["plural"] as? [String: Any])
        func value(_ form: String) -> String? {
            ((plural[form] as? [String: Any])?["stringUnit"] as? [String: Any])?["value"] as? String
        }
        #expect(value("one") == "%lld card waiting")
        #expect(value("other") == "%lld cards waiting")
        #expect(strings["Nothing to study right now"] != nil)
    }

    // MARK: Deck list search (N14.2)

    @Test func searchMatchesComponentsAndNamesParents() {
        let search = DeckSearch(nodes: tree, query: "chem")
        #expect(search.matches.map(\.id) == [7])
        #expect(search.matches.first?.name == "Chemistry")
        #expect(search.matches.first?.ancestors == ["Science"])
        #expect(search.matches.first?.fullName == "Science::Chemistry")
        // Every term must match some component, in any order and case.
        #expect(DeckSearch(nodes: tree, query: "文法 LANG").matches.map(\.id) == [4])
        #expect(DeckSearch(nodes: tree, query: "文法 science").matches.isEmpty)
        // A parent's match lists the parent and, through its path, its subdecks.
        #expect(DeckSearch(nodes: tree, query: "日本語").matches.map(\.id) == [3, 4])
    }

    @Test func createIsOfferedOnlyForANameNoDeckHas() {
        #expect(DeckSearch(nodes: tree, query: "Korean").offersCreate)
        #expect(DeckSearch(nodes: tree, query: "Korean").matches.isEmpty)
        // A partial match still offers the typed name as a new deck.
        #expect(DeckSearch(nodes: tree, query: "Chem").offersCreate)
        // An existing full name, in any case and with outer spaces, is not offered again.
        #expect(!DeckSearch(nodes: tree, query: "science::chemistry").offersCreate)
        #expect(!DeckSearch(nodes: tree, query: "  Reading ").offersCreate)
        // A last component alone is not a full name: "Chemistry" would be a new top-level deck.
        #expect(DeckSearch(nodes: tree, query: "Chemistry").offersCreate)
    }

    @Test func aBlankQueryShowsNoResultsAndNoOffer() {
        for query in ["", "   ", "\n"] {
            let search = DeckSearch(nodes: tree, query: query)
            #expect(search.matches.isEmpty)
            #expect(!search.offersCreate)
        }
    }
}
