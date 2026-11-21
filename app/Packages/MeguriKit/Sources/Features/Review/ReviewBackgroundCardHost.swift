import AppModel
import CardRendering
import SwiftUI

/// The next session's card pages, in the window beneath its content and
/// never seen (R1813). WebKit runs a page's animation frames, which a card's
/// settling needs, only in a window it reports visible; a page occluded by
/// other views runs them as an uncovered one does
/// (docs/design/decisions/s1-occluded-page-swap.md). So Study's first card
/// can be prepared before its review shows, and the review takes the same
/// pages when it appears.
public struct ReviewBackgroundCardHost: View {
    public let collection: ReviewCollection
    @Environment(\.colorScheme) private var colorScheme
    public init(collection: ReviewCollection) { self.collection = collection }

    public var body: some View {
        if let pair = collection.backgroundCardPair {
            CardWebView(pair: pair)
                .opacity(0)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
                .task(id: Appearance(pair: ObjectIdentifier(pair), dark: colorScheme == .dark)) {
                    try? await pair.setAppearance(colorScheme == .dark ? .dark : .light)
                }
        }
    }

    private struct Appearance: Equatable {
        let pair: ObjectIdentifier
        let dark: Bool
    }
}
