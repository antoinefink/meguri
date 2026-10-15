import SwiftUI

/// One state of one component in the gallery (R1999).
public struct GalleryEntry: Identifiable {
    /// lowerCamel component name, e.g. `countColumns`.
    public let component: String
    /// lowerCamel state name, e.g. `typical`, `empty`, `overflow`.
    public let state: String
    private let make: () -> AnyView

    public init(_ component: String, _ state: String, @ViewBuilder _ content: @escaping () -> some View) {
        self.component = component
        self.state = state
        make = { AnyView(content()) }
    }

    /// The accessibility identifier of the entry in the gallery.
    public var identifier: String { "designsystem.gallery.\(component).\(state)" }
    public var id: String { identifier }

    public func makeContent() -> AnyView { make() }
}

/// The component gallery (R1999): every shared component in every state.
/// Agents build screens from these components, and a new reusable component
/// is registered here before any screen uses it. The gallery is reachable only
/// through the `designsystem/gallery` test-hook route (debug and test builds);
/// U10 snapshots each entry. Each entry's container carries
/// `designsystem.gallery.<component>.<state>`, and the summary line lists them
/// all in its accessibility value.
public struct ComponentGallery: View {
    public static let summaryIdentifier = "designsystem.gallery.summary"

    private let entries: [GalleryEntry]

    public init(entries: [GalleryEntry] = ComponentGallery.entries) {
        self.entries = entries
    }

    public var body: some View {
        ScrollView {
            // Not lazy: every entry exists in the accessibility hierarchy, so
            // UI tests find entries that are off screen.
            VStack(alignment: .leading, spacing: Spacing.large) {
                Text("\(entries.count) states of \(Set(entries.map(\.component)).count) components")
                    .font(Typography.caption)
                    .foregroundStyle(.token(.textSecondary))
                    .accessibilityIdentifier(Self.summaryIdentifier)
                    .accessibilityValue(entries.map(\.identifier).joined(separator: " "))
                ForEach(entries) { entry in
                    VStack(alignment: .leading, spacing: Spacing.xSmall) {
                        Text("\(entry.component) · \(entry.state)")
                            .font(Typography.caption)
                            .foregroundStyle(.token(.textSecondary))
                        entry.makeContent()
                    }
                    .padding(Spacing.medium)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.token(.surface), in: Radius.rounded(Radius.medium))
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier(entry.identifier)
                }
            }
            .padding(Spacing.medium)
        }
        .background(.token(.backgroundSecondary))
        .navigationTitle(Text(verbatim: "Component Gallery"))
    }
}

extension ComponentGallery {
    /// Every registered component state, in gallery order.
    public static var entries: [GalleryEntry] {
        [
            GalleryEntry("countColumns", "typical") {
                VStack(alignment: .trailing, spacing: Spacing.xxSmall) {
                    CountColumns.Header()
                    CountColumns(new: 12, learn: 3, due: 40)
                }
            },
            GalleryEntry("countColumns", "empty") { CountColumns(new: 0, learn: 0, due: 0) },
            GalleryEntry("countColumns", "overflow") { CountColumns(new: 12_345, learn: 678, due: 99_999) },

            GalleryEntry("sectionHeader", "typical") {
                SectionHeader(Text(verbatim: "Today"), subtitle: Text(verbatim: "Studied 42 cards in 12 minutes")) {
                    Button { } label: { Image(systemName: "ellipsis.circle") }
                        .accessibilityLabel(Text(verbatim: "More"))
                }
            },
            GalleryEntry("sectionHeader", "overflow") {
                SectionHeader(Text(verbatim: "Japanese::Vocabulary::Core 6000::Part 4 — kanji compounds with long readings"))
            },

            GalleryEntry("emptyState", "typical") {
                EmptyStateView(Text(verbatim: "No Decks"), systemImage: "rectangle.stack", description: Text(verbatim: "Create a deck or import a package to start studying.")) {
                    Button { } label: { Text(verbatim: "Create Deck") }
                        .accessibilityIdentifier("designsystem.gallery.emptyState.typical.create")
                }
            },
            GalleryEntry("emptyState", "overflow") {
                EmptyStateView(
                    Text(verbatim: "No Cards Match This Search in Any of the Selected Decks"),
                    systemImage: "magnifyingglass",
                    description: Text(verbatim: "Try fewer search terms, check the spelling of tags and deck names, or search the whole collection instead of the current deck.")
                )
            },

            GalleryEntry("errorState", "typical") {
                ErrorStateView(
                    Text(verbatim: "Couldn’t Load Decks"), message: Text(verbatim: "The collection is busy. Try again in a moment."),
                    retryIdentifier: "designsystem.gallery.errorState.typical.retry", retry: {}
                )
            },
            GalleryEntry("errorState", "overflow") {
                ErrorStateView(
                    Text(verbatim: "Couldn’t Open the Collection File Because Another Device Is Replacing It"),
                    message: Text(verbatim: "A full sync from another device is replacing this collection. Meguri will reopen it when the sync finishes; your reviews from this device are kept in the backup made before the sync.")
                )
            },

            GalleryEntry("loadingState", "indeterminate") { LoadingStateView(Text(verbatim: "Opening collection…")) },
            GalleryEntry("loadingState", "determinate") { LoadingStateView(Text(verbatim: "Importing 1,204 notes…"), fractionDone: 0.4) },

            GalleryEntry("toast", "typical") { ToastView(Text(verbatim: "Card buried"), systemImage: "eye.slash") },
            GalleryEntry("toast", "error") { ToastView(Text(verbatim: "Couldn’t save the note"), systemImage: "exclamationmark.triangle", style: .error) },
            GalleryEntry("toast", "overflow") {
                ToastView(Text(verbatim: "3 cards were moved to “Japanese::Vocabulary::Core 6000” and their siblings were buried until tomorrow"), systemImage: "checkmark")
            },

            GalleryEntry("progressBar", "empty") { ProgressBar(value: 0, label: Text(verbatim: "Session progress")) },
            GalleryEntry("progressBar", "typical") { ProgressBar(value: 0.4, label: Text(verbatim: "Session progress")) },
            GalleryEntry("progressBar", "complete") { ProgressBar(value: 1, label: Text(verbatim: "Session progress")) },

            GalleryEntry("destructiveConfirmation", "typical") { DestructiveConfirmationSample() },

            GalleryEntry("controlBar", "typical") {
                ControlBar {
                    Button { } label: { Text(verbatim: "Again").foregroundStyle(.token(.ratingAgain)) }
                    Button { } label: { Text(verbatim: "Hard") }
                    Button { } label: { Text(verbatim: "Good").foregroundStyle(.token(.ratingGood)) }
                    Button { } label: { Text(verbatim: "Easy") }
                }
            },
            GalleryEntry("floatingControl", "typical") {
                Button { } label: { Image(systemName: "arrow.uturn.backward") }
                    .chromeButtonStyle()
                    .accessibilityLabel(Text(verbatim: "Undo"))
            },
        ]
    }
}

/// The destructive confirmation entry: a button that asks before acting.
private struct DestructiveConfirmationSample: View {
    @State private var isPresented = false

    var body: some View {
        Button(role: .destructive) { isPresented = true } label: { Text(verbatim: "Delete Deck…") }
            .destructiveConfirmation(
                Text(verbatim: "Delete “Spanish”?"),
                isPresented: $isPresented,
                actionTitle: Text(verbatim: "Delete Deck"),
                message: Text(verbatim: "Its 1,204 cards are deleted too. You can undo this.")
            ) {}
    }
}
