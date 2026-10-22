import CoreBridge
import DesignSystem
import SwiftUI

public enum HomeDeckListPreview: PreviewStates {
    public static let previewID = "home.deckList"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in PreviewFixture(state) {
            ScrollView {
                Group {
                    switch state {
                    case .empty: EmptyStateView(Text(verbatim: "No Collection"), systemImage: "rectangle.stack", description: Text(verbatim: "There is no collection in Meguri yet. Local collection import is coming next."))
                    case .loading: LoadingStateView(Text(verbatim: "Opening collection…"))
                    case .error: ErrorStateView(Text(verbatim: "Couldn’t Open Collection"), message: Text(verbatim: "The collection is open in another program."), retryIdentifier: "home.collection.retry", retry: {})
                    case .typical, .overflow: HomeDeckTree(nodes: nodes(overflow: state == .overflow), select: { _ in })
                    }
                }
                .padding(Spacing.medium)
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(.token(.backgroundPrimary))
        } }
    }
    private static func nodes(overflow: Bool) -> [CountedDeckNode] {
        let child = CountedDeckNode(id: 2, name: overflow ? "Japanese::Vocabulary with a longer deck title that wraps onto another line" : "Japanese::Vocabulary",
            filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: overflow ? 12_345 : 5, learning: 2, review: overflow ? 99_999 : 24), children: [])
        return [CountedDeckNode(id: 1, name: "Japanese", filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: child.counts, children: [child])]
    }
}
