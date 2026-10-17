import Foundation

/// A `meguri://` link (R319).
nonisolated public enum DeepLink: Equatable, Sendable {
    /// `meguri://home`
    case home
    /// `meguri://deck?id=<deck id>`: the deck's page.
    case deck(DeckID)
    /// `meguri://browse?search=<search>`: Browse with that search.
    case browse(search: String)
}

/// What opening a link did.
nonisolated public enum DeepLinkOutcome: Equatable, Sendable {
    case opened(DeepLink)
    /// Ignored, with the navigation left as it was; the shell tells the user
    /// why (R320).
    case refused(DeepLinkRefusal)
}

nonisolated public enum DeepLinkRefusal: Equatable, Sendable {
    /// Not a `meguri://` link Meguri knows.
    case unknownLink
    /// No deck has this id.
    case missingDeck(DeckID)
}

/// Opens `meguri://` links (R319–R321). A link only navigates: it never
/// writes to the collection and never starts a review (R320). Meguri does
/// not register `anki://` (R321): AnkiMobile keeps its links.
public enum DeepLinkRouter {
    public nonisolated static let scheme = "meguri"

    /// The link `url` names, or nil when it is none of Meguri's.
    public nonisolated static func parse(_ url: URL) -> DeepLink? {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              components.scheme?.lowercased() == scheme,
              let host = components.host?.lowercased(), !host.isEmpty,
              components.path.isEmpty || components.path == "/"
        else { return nil }
        let query = components.queryItems ?? []
        func value(_ name: String) -> String? {
            query.first { $0.name == name }?.value
        }
        switch host {
        case "home":
            return .home
        case "deck":
            guard let id = value("id").flatMap({ Int64($0) }) else { return nil }
            return .deck(DeckID(id))
        case "browse":
            return .browse(search: value("search") ?? "")
        default:
            return nil
        }
    }

    /// Opens `url` in `navigation`. A deck link asks `deckExists` first and
    /// changes nothing when the deck is missing. Opening a link ends an open
    /// review, as any navigation away from it does (R22); a refused link
    /// leaves the review running.
    public static func open(
        _ url: URL, in navigation: AppNavigation, deckExists: (DeckID) async -> Bool
    ) async -> DeepLinkOutcome {
        guard let link = parse(url) else { return .refused(.unknownLink) }
        switch link {
        case .home:
            navigation.openHome()
        case let .deck(deck):
            guard await deckExists(deck) else { return .refused(.missingDeck(deck)) }
            navigation.openDeckPage(deck)
        case let .browse(search):
            navigation.endReview()
            navigation.browseSearch = search
            // A new search: its results from the top, at Browse's root.
            navigation.setPath([], for: .browse)
            navigation.scrollToTop(.section(.browse))
            navigation.show(.browse)
        }
        return .opened(link)
    }
}
