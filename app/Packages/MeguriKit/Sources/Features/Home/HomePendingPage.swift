import DesignSystem
import SwiftUI

/// Home while nothing is known about the decks and nothing is kept (N27.3): the page's own
/// background, as the list that replaces it draws it, with no state view. A requested deck
/// can already be known by name, so touch keeps its ordinary content heading. Only
/// when the read takes longer than half a second does the system's small progress indicator
/// show, centred; it goes with the page the moment content arrives.
struct HomePendingPage: View {
    /// The grouped pages' background (Today on touch, a deck page); else the deck list's.
    let grouped: Bool
    let label: Text
    let deckName: String?
    /// How long the page stays empty before the indicator shows.
    var indicatorDelay: Duration = .milliseconds(500)
    @State private var showsIndicator: Bool

    /// `indicatorAtOnce`: as it is after the delay (the DTO preview's capture).
    init(grouped: Bool, indicatorAtOnce: Bool = false, label: Text = Text("Loading Decks", bundle: .module),
         deckName: String? = nil) {
        self.grouped = grouped
        self.label = label
        self.deckName = deckName
        _showsIndicator = State(initialValue: indicatorAtOnce)
    }

    var body: some View {
        list
            .overlay {
                if showsIndicator {
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityLabel(label)
                        .accessibilityIdentifier("home.collection.pending.progress")
                }
            }
            .task {
                try? await Task.sleep(for: indicatorDelay)
                if !Task.isCancelled { showsIndicator = true }
            }
            .accessibilityIdentifier("home.collection.pending")
    }

    @ViewBuilder private var list: some View {
        #if os(macOS)
        List {}
        #else
        if grouped {
            List {
                if let deckName {
                    Section {} header: { HomeDeckHeading(name: deckName) }
                }
            }.listStyle(.insetGrouped)
        } else { List {} }
        #endif
    }
}

#if !os(macOS)
/// The same native section heading before and after a deck's content arrives.
struct HomeDeckHeading: View {
    let name: String
    var body: some View {
        Text(verbatim: HomeHeading.deckTitle(name))
            .font(.largeTitle.bold())
            .foregroundStyle(.token(.textPrimary))
            .textCase(nil)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityAddTraits(.isHeader)
            .accessibilityIdentifier("home.deck.title")
    }
}
#endif
