import Foundation
import SwiftUI
import Testing

import AppModel

/// `meguri://` deep links (R319–R321): parsing, and what opening one does to
/// the navigation. Opening a link never writes and never starts a review.
@MainActor
@Suite struct DeepLinkRouterTests {
    private let existing = DeckID(1_700_000_000_001)

    /// The deck lookup a link may ask: it records each id asked about and
    /// knows one deck. It is the router's only way to the collection, and it
    /// can only read.
    private final class Lookup {
        var asked: [DeckID] = []
        let decks: Set<DeckID>

        init(_ decks: Set<DeckID>) {
            self.decks = decks
        }

        func exists(_ deck: DeckID) -> Bool {
            asked.append(deck)
            return decks.contains(deck)
        }
    }

    private final class Session {}

    private func url(_ string: String) throws -> URL {
        try #require(URL(string: string))
    }

    // MARK: Parsing

    @Test(arguments: [
        ("meguri://home", DeepLink.home),
        ("meguri://home/", DeepLink.home),
        ("MEGURI://Home", DeepLink.home),
        ("meguri://deck?id=1700000000001", DeepLink.deck(DeckID(1_700_000_000_001))),
        ("meguri://deck?id=1", DeepLink.deck(DeckID(1))),
        ("meguri://deck?id=-5&extra=ignored", DeepLink.deck(DeckID(-5))),
        ("meguri://browse?search=deck:Japanese", DeepLink.browse(search: "deck:Japanese")),
        ("meguri://browse?search=deck%3A%22Japanese%20Core%22%20is%3Adue", DeepLink.browse(search: #"deck:"Japanese Core" is:due"#)),
        ("meguri://browse?search=", DeepLink.browse(search: "")),
        ("meguri://browse", DeepLink.browse(search: "")),
    ])
    func aMeguriLinkParses(link: String, expected: DeepLink) throws {
        #expect(DeepLinkRouter.parse(try url(link)) == expected)
    }

    @Test(arguments: [
        "anki://deck?id=1",
        "https://meguri.app/deck?id=1",
        "meguri://deck",
        "meguri://deck?id=",
        "meguri://deck?id=abc",
        "meguri://deck?id=1.5",
        "meguri://deck?id=99999999999999999999",
        "meguri://review?deck=1",
        "meguri://home/deck",
        "meguri:home",
        "meguri://",
    ])
    func anythingElseIsNoMeguriLink(link: String) throws {
        #expect(DeepLinkRouter.parse(try url(link)) == nil)
    }

    // MARK: Opening

    @Test func aLinkToAnExistingDeckOpensItsPage() async throws {
        let navigation = AppNavigation()
        navigation.select(.section(.stats))
        let lookup = Lookup([existing])
        let outcome = await DeepLinkRouter.open(try url("meguri://deck?id=1700000000001"), in: navigation, deckExists: lookup.exists)
        #expect(outcome == .opened(.deck(existing)))
        #expect(navigation.section == .home)
        #expect(navigation.selectedDeck == existing)
        #expect(navigation.sidebarSelection == .deck(existing))
        #expect(navigation.compactPath(.decks) == [.deckPage(existing)])
        #expect(navigation.review == nil, "a link never starts a review (R320)")
        #expect(lookup.asked == [existing])
    }

    @Test func aLinkToAMissingDeckLeavesTheNavigationAlone() async throws {
        let navigation = AppNavigation()
        navigation.select(.section(.browse))
        navigation.push(.noteTypes)
        let lookup = Lookup([existing])
        let outcome = await DeepLinkRouter.open(try url("meguri://deck?id=999"), in: navigation, deckExists: lookup.exists)
        #expect(outcome == .refused(.missingDeck(DeckID(999))))
        #expect(navigation.section == .browse)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.path(.browse) == [.noteTypes])
        #expect(lookup.asked == [DeckID(999)])
    }

    /// R320: where Browse is not shown yet (the iPad's and iPhone's layouts,
    /// U278, U279), a Browse link is refused and changes nothing, rather
    /// than leaving the person on Today with the search kept for later.
    @Test func aBrowseLinkWhereBrowseIsNotShownIsRefusedAndChangesNothing() async throws {
        let navigation = AppNavigation()
        navigation.select(.section(.stats))
        let outcome = await DeepLinkRouter.open(try url("meguri://browse?search=deck:Japanese"), in: navigation,
                                                browseAvailable: false, deckExists: Lookup([existing]).exists)
        #expect(outcome == .refused(.browseUnavailable))
        #expect(navigation.section == .stats && navigation.browseSearch.isEmpty)
    }

    @Test func aLinkToAMissingDeckLeavesAReviewRunning() async throws {
        let navigation = AppNavigation()
        let session = Session()
        navigation.startReview(ReviewPresentation(deck: existing, session: session))
        let outcome = await DeepLinkRouter.open(try url("meguri://deck?id=999"), in: navigation, deckExists: Lookup([existing]).exists)
        #expect(outcome == .refused(.missingDeck(DeckID(999))))
        #expect(navigation.review?.session === session)
    }

    @Test func aBrowseLinkOpensBrowseWithItsSearch() async throws {
        let navigation = AppNavigation()
        navigation.select(.section(.browse))
        navigation.push(.noteTypes)
        navigation.setScrollPosition(ScrollPosition(id: "row-20"), for: .section(.browse))
        navigation.select(.section(.home))
        let lookup = Lookup([existing])
        let outcome = await DeepLinkRouter.open(try url("meguri://browse?search=deck:Japanese"), in: navigation, deckExists: lookup.exists)
        #expect(outcome == .opened(.browse(search: "deck:Japanese")))
        #expect(navigation.section == .browse)
        #expect(navigation.browseSearch == "deck:Japanese")
        #expect(navigation.path(.browse).isEmpty, "the search shows in Browse itself, not under a pushed screen")
        #expect(navigation.scrollPosition(for: .section(.browse)).edge == .top, "a new search shows its results from the top")
        #expect(lookup.asked.isEmpty)
    }

    @Test func aHomeLinkOpensHome() async throws {
        let navigation = AppNavigation()
        navigation.openDeckPage(existing)
        navigation.push(.noteTypes)
        let outcome = await DeepLinkRouter.open(try url("meguri://home"), in: navigation, deckExists: Lookup([]).exists)
        #expect(outcome == .opened(.home))
        #expect(navigation.section == .home)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.compactPath(.decks).isEmpty)
    }

    @Test func anOpenedLinkEndsAReviewButNeverStartsOne() async throws {
        let navigation = AppNavigation()
        navigation.startReview(ReviewPresentation(deck: existing, session: Session()))
        let outcome = await DeepLinkRouter.open(try url("meguri://home"), in: navigation, deckExists: Lookup([]).exists)
        #expect(outcome == .opened(.home))
        #expect(navigation.review == nil)
    }

    @Test func anUnknownLinkIsRefusedAndChangesNothing() async throws {
        let navigation = AppNavigation()
        navigation.select(.section(.stats))
        let lookup = Lookup([existing])
        for link in ["meguri://review?deck=1", "anki://deck?id=1700000000001"] {
            let outcome = await DeepLinkRouter.open(try url(link), in: navigation, deckExists: lookup.exists)
            #expect(outcome == .refused(.unknownLink))
        }
        #expect(navigation.section == .stats)
        #expect(lookup.asked.isEmpty)
    }
}
