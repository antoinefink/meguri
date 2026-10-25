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
            case error(name: String)
            case loading
        }

        let content: Content

        #if os(macOS)
        private let regularWidth = true
        #else
        @Environment(\.horizontalSizeClass) private var sizeClass
        private var regularWidth: Bool { sizeClass == .regular }
        #endif

        var body: some View {
            scrollingContent
                // Read before the bar: the bar's content comes first in the
                // accessibility children, and the Mac reads in that order
                // (iOS VoiceOver reads by position).
                .accessibilitySortPriority(1)
                // The primary action stays in reach whatever the title's
                // length, in a bar the content scrolls under: the system's
                // scroll edge effect shows that the content goes on (a hard
                // cut above a pinned button read as cut-off text). It fills
                // an iPhone's width and keeps a button's width in an iPad or
                // Mac window, on the content column's leading edge.
                .safeAreaBar(edge: .bottom) {
                    if case .deck = content {
                        Button {} label: { Text(verbatim: "Study Now").frame(maxWidth: .infinity) }
                            .buttonStyle(.bordered)
                            .tint(ColorToken.textPrimary.color)
                            .controlSize(.large)
                            .frame(maxWidth: regularWidth ? ContentWidth.primaryAction : .infinity)
                            .padding(Spacing.medium)
                            .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                            .frame(maxWidth: .infinity)
                            .accessibilityIdentifier("sample.deckOverview.study")
                    }
                }
                .background(.token(.backgroundPrimary))
                .accessibilityElement(children: .contain)
        }

        private var scrollingContent: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.medium) {
                    switch content {
                    case let .deck(name, new, learn, due):
                        title(name)
                        // A line of counts, not CountColumns: its fixed column
                        // widths overflow narrow screens at accessibility
                        // sizes, a defect of the component that its own
                        // gallery snapshots show; this sample is about the
                        // deck name.
                        counts(new: new, learn: learn, due: due)
                    case let .empty(name):
                        title(name)
                        EmptyStateView(
                            Text(verbatim: "No Cards"), systemImage: "rectangle.stack",
                            description: Text(verbatim: "Add notes to this deck to start studying.")
                        ) {
                            Button {} label: { Text(verbatim: "Add Notes") }
                                .accessibilityIdentifier("sample.deckOverview.addNotes")
                                #if os(macOS)
                                .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                                .contentShape(Rectangle())
                                #endif
                        }
                    case let .error(name):
                        title(name)
                        ErrorStateView(
                            Text(verbatim: "Couldn’t Load the Deck"),
                            message: Text(verbatim: "The collection is busy. Try again in a moment."),
                            retryIdentifier: "sample.deckOverview.retry",
                            retry: {}
                        )
                    case .loading:
                        LoadingStateView(Text(verbatim: "Loading deck…"))
                    }
                }
                .padding(Spacing.medium)
                // A readable column, centred in a wide window: never a
                // phone layout stretched across an iPad or a Mac.
                .frame(maxWidth: ContentWidth.readable, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .accessibilityIdentifier("sample.deckOverview.screen")
        }

        /// New, Learn and Due counts in their colours, each with its name
        /// (colour is never the only cue).
        private func counts(new: Int, learn: Int, due: Int) -> some View {
            Text(
                "\(Text(new, format: .number).foregroundStyle(.token(.countNew))) new · \(Text(learn, format: .number).foregroundStyle(.token(.countLearn))) learning · \(Text(due, format: .number).foregroundStyle(.token(.countDue))) due"
            )
            .font(.headline)
            .foregroundStyle(.token(.textSecondary))
            .accessibilityIdentifier("sample.deckOverview.counts")
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
            PreviewFixture(.error) { Self(content: .error(name: "Spanish")) },
            PreviewFixture(.loading) { Self(content: .loading) },
        ]
    }
}

#Preview("Typical") { TestHooks.SampleDeckOverview.preview(.typical) }
#Preview("Overflow") { TestHooks.SampleDeckOverview.preview(.overflow) }
#endif
