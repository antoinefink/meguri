#if MEGURI_TEST_HOOKS
import DesignSystem
import SwiftUI

extension TestHooks {
    /// A sample screen for the verification tooling (U10): one deck's
    /// title, counts and study button, in all five states. It goes through
    /// the same pipeline as product screens (state fixtures, matrix
    /// snapshots, accessibility audit, critic, gallery) so the tooling can be
    /// shown working before any product screen exists. It is not a product
    /// screen: it is not in `docs/design/screen-inventory.yaml`, and its id
    /// is `sample.deckOverview`.
    struct SampleDeckOverview: View {
        enum Content {
            case deck(name: String, new: Int, learn: Int, due: Int)
            case empty(name: String)
            case error
            case loading
        }

        let content: Content

        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.medium) {
                    switch content {
                    case let .deck(name, new, learn, due):
                        title(name)
                        VStack(alignment: .trailing, spacing: Spacing.xxSmall) {
                            CountColumns.Header()
                            CountColumns(new: new, learn: learn, due: due)
                        }
                        .frame(maxWidth: .infinity, alignment: .trailing)
                        Button {} label: { Text(verbatim: "Study Now").frame(maxWidth: .infinity) }
                            .buttonStyle(.bordered)
                            .tint(ColorToken.textPrimary.color)
                            .controlSize(.large)
                            .accessibilityIdentifier("sample.deckOverview.study")
                    case let .empty(name):
                        title(name)
                        EmptyStateView(
                            Text(verbatim: "No Cards"), systemImage: "rectangle.stack",
                            description: Text(verbatim: "Add notes to this deck to start studying.")
                        )
                    case .error:
                        ErrorStateView(
                            Text(verbatim: "Couldn’t Load the Deck"),
                            message: Text(verbatim: "The collection is busy. Try again in a moment."),
                            retry: {}
                        )
                    case .loading:
                        LoadingStateView(Text(verbatim: "Loading deck…"))
                    }
                }
                .padding(Spacing.medium)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(.token(.backgroundPrimary))
            .accessibilityIdentifier("sample.deckOverview.screen")
        }

        /// The deck name wraps at every text size; a cut-off name is the
        /// truncation defect the critic must flag (AE90).
        private func title(_ name: String) -> some View {
            Text(verbatim: name)
                .font(.largeTitle.bold())
                .foregroundStyle(.token(.textPrimary))
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("sample.deckOverview.title")
        }
    }

    /// Two buttons for the touch-target check (R1980): a 20 pt one, below
    /// the iPhone and iPad minimum, and a 44×44 pt one.
    struct SampleTouchTargets: View {
        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.large) {
                Text(verbatim: "Touch targets")
                    .font(.headline)
                    .accessibilityIdentifier("sample.touchTargets.title")
                // The symbol inside a 20 pt frame: the hierarchy reports the
                // glyph's frame (19×18 pt), and the system audit reports a
                // hitRegion issue for it.
                Button {} label: { Image(systemName: "star").frame(width: 20, height: 20) }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Text(verbatim: "Small"))
                    .accessibilityIdentifier("sample.touchTargets.small")
                Button {} label: { Image(systemName: "star.fill").resizable().frame(width: 44, height: 44) }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Text(verbatim: "Large"))
                    .accessibilityIdentifier("sample.touchTargets.large")
            }
            .padding(Spacing.medium)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(.token(.backgroundPrimary))
        }
    }
}

extension TestHooks.SampleDeckOverview: PreviewStates {
    static let previewID = "sample.deckOverview"

    static var previewFixtures: [PreviewFixture] {
        [
            PreviewFixture(.empty) { Self(content: .empty(name: "Spanish")) },
            PreviewFixture(.typical) { Self(content: .deck(name: "Spanish", new: 20, learn: 3, due: 41)) },
            PreviewFixture(.overflow) {
                Self(content: .deck(
                    name: "Japanese::Vocabulary::Core 6000::Part 4 — kanji compounds with long readings",
                    new: 12_345, learn: 678, due: 99_999
                ))
            },
            PreviewFixture(.error) { Self(content: .error) },
            PreviewFixture(.loading) { Self(content: .loading) },
        ]
    }
}

#Preview("Typical") { TestHooks.SampleDeckOverview.preview(.typical) }
#Preview("Overflow") { TestHooks.SampleDeckOverview.preview(.overflow) }
#endif
