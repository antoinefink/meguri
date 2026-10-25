import CoreBridge
import DesignSystem
import SwiftUI

public enum HomeDeckListPreview: PreviewStates {
    public static let previewID = "home.deckList"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in PreviewFixture(state) {
            Group {
                #if os(macOS)
                GeometryReader { geometry in
                    scrollContent(state: state, availableHeight: geometry.size.height)
                }
                #else
                scrollContent(state: state)
                #endif
            }
            .background(.token(.backgroundPrimary))
            .navigationTitle(Text("Decks", bundle: .module))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.collection.screen")
        } }
    }
    private static func scrollContent(state: PreviewState, availableHeight: CGFloat = 0) -> some View {
        #if os(macOS)
        let centersContent = state == .empty
        #else
        let centersContent = false
        #endif
        return ScrollView {
            VStack(alignment: .leading, spacing: Spacing.large) {
                switch state {
                case .empty: HomeCollectionEmptyContent(importAction: {})
                case .loading: LoadingStateView(Text(verbatim: "Opening collection…"))
                case .error: ErrorStateView(Text(verbatim: "Couldn’t Open Collection"), message: Text(verbatim: "The collection is open in another program."), retryIdentifier: "home.collection.retry", retry: {})
                case .typical, .overflow: HomeDeckTree(nodes: nodes(overflow: state == .overflow), select: { _ in })
                }
            }
            .padding(Spacing.medium)
            .frame(maxWidth: ContentWidth.readable, alignment: .leading)
            .frame(maxWidth: .infinity)
            .frame(minHeight: centersContent ? availableHeight : 0, alignment: centersContent ? .center : .top)
        }
    }
    private static func nodes(overflow: Bool) -> [CountedDeckNode] {
        let child = CountedDeckNode(id: 2, name: overflow ? "Japanese::Vocabulary with a longer deck title that wraps onto another line" : "Japanese::Vocabulary",
            filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: overflow ? 12_345 : 5, learning: 2, review: overflow ? 99_999 : 24), children: [])
        return [CountedDeckNode(id: 1, name: "Japanese", filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: child.counts, children: [child])]
    }
}
