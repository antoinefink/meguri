import CoreBridge
import DesignSystem
import SwiftUI

public enum HomeDeckListPreview: PreviewStates {
    public static let previewID = "home.deckList"
    public static var previewFixtures: [PreviewFixture] {
        PreviewState.allCases.map { state in PreviewFixture(state) {
            Group {
                if state == .typical || state == .overflow {
                    HomeDeckTree(nodes: nodes(overflow: state == .overflow), select: { _ in })
                } else {
                    if state == .empty {
                        // The live screen's three rows, enabled (N18.3).
                        HomeWelcome(sync: .init(), importFile: .init(), startEmpty: .init())
                    } else {
                        GeometryReader { geometry in
                            scrollContent(state: state, availableHeight: geometry.size.height)
                        }
                    }
                }
            }
            .background(.token(.backgroundPrimary))
            .navigationTitle(Text("Decks", bundle: .module))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.collection.screen")
        } }
    }
    /// The typical tree as it shows beside the detail (N14.10): Today first, and the
    /// selected row marked. `deck` nil selects Today.
    public static func besideDetail(deck: Int64?) -> some View {
        HomeDeckTree(nodes: nodes(overflow: false),
            today: HomeTodayRow(waiting: 31, selection: DeckListSelection(deck: deck)) {}, select: { _ in })
            .navigationTitle(Text("Decks", bundle: .module))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.collection.screen")
    }
    /// Every state view is centred, as in `HomeView` (N2).
    private static func scrollContent(state: PreviewState, availableHeight: CGFloat) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.large) {
                switch state {
                case .loading: LoadingStateView(Text(verbatim: "Opening collection…"))
                case .error: ErrorStateView(Text(verbatim: "Couldn’t Open Collection"), message: Text(verbatim: "The collection is open in another program."), retryIdentifier: "home.collection.retry", retry: {})
                case .empty, .typical, .overflow: EmptyView() // Rendered by their own lists above.
                }
            }
            .padding(Spacing.medium)
            .frame(maxWidth: ContentWidth.readable, alignment: .leading)
            .frame(maxWidth: .infinity)
            .frame(minHeight: availableHeight, alignment: .center)
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

#Preview("Native Deck Tree") { HomeDeckListPreview.preview(.typical) }
#Preview("Native Deck Tree Overflow") { HomeDeckListPreview.preview(.overflow) }
