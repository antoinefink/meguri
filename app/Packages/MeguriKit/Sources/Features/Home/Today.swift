import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// What Today lists (N14.1): the deepest decks that have cards waiting, and a parent that has cards of its own.
public nonisolated struct TodayDecks: Sendable {
    public struct Entry: Hashable, Sendable, Identifiable {
        public let id: Int64
        public let name: String
        /// The ancestors' names, outermost first; empty for a top-level deck.
        public let ancestors: [String]
        public let counts: ReviewCounts
        public var path: String { (ancestors + [name]).joined(separator: "::") }
    }

    public let entries: [Entry]
    /// The collection's totals: the top-level decks' counts, which already hold their subdecks'.
    public let totals: ReviewCounts

    public init(nodes: [CountedDeckNode]) {
        var found: [Entry] = []
        func visit(_ node: CountedDeckNode, ancestors: [String]) {
            let name = node.name.components(separatedBy: "::").last ?? node.name
            guard node.counts.waiting > 0 else { return }
            let waitingChildren = node.children.filter { $0.counts.waiting > 0 }
            // A parent's counts hold its subdecks'. It is listed itself when it has no waiting
            // subdeck, or when it has cards of its own beyond theirs: Study on it reaches those.
            let own = node.counts.new > waitingChildren.reduce(0) { $0 + $1.counts.new }
                || node.counts.learning > waitingChildren.reduce(0) { $0 + $1.counts.learning }
                || node.counts.review > waitingChildren.reduce(0) { $0 + $1.counts.review }
            if waitingChildren.isEmpty || own {
                found.append(Entry(id: node.id, name: name, ancestors: ancestors, counts: node.counts))
            }
            for child in waitingChildren { visit(child, ancestors: ancestors + [name]) }
        }
        for node in nodes { visit(node, ancestors: []) }
        entries = found
        totals = nodes.reduce(ReviewCounts(new: 0, learning: 0, review: 0)) {
            ReviewCounts(new: $0.new + $1.counts.new, learning: $0.learning + $1.counts.learning,
                         review: $0.review + $1.counts.review)
        }
    }
}

/// The page headings that follow what the body shows (N14.4): the Mac's toolbar
/// title and subtitle, and the same words in touch pages' content.
enum HomeHeading {
    /// The sidebar layouts' root is Today in every collection state; the tab layout's is the deck list.
    static func rootTitle(showsToday: Bool) -> String {
        showsToday ? String(localized: "Today", bundle: .module) : String(localized: "Decks", bundle: .module)
    }
    /// A deck's last name component.
    static func deckTitle(_ fullName: String) -> String { fullName.components(separatedBy: "::").last ?? fullName }
    /// The deck's counts, zeros included, or the nothing-waiting line.
    static func deckSubtitle(_ waiting: DeckWaiting, locale: Locale) -> String {
        waiting.offersAddNote ? String(localized: "Nothing to study right now", bundle: .module)
            : waiting.counts.summary(locale: locale)
    }
    /// Today's totals; nothing at all while nothing waits.
    static func todaySubtitle(_ totals: ReviewCounts, locale: Locale) -> String {
        totals.waiting == 0 ? "" : totals.summary(locale: locale)
    }
}

extension View {
    /// Secondary text on a content background: the token that keeps 4.5:1 on touch, the system's on the Mac.
    func quietText() -> some View {
        #if os(macOS)
        foregroundStyle(.secondary)
        #else
        foregroundStyle(.token(.textSecondary))
        #endif
    }
}

extension ReviewCounts {
    /// "20 new · 3 learning · 72 to review", zeros included (N12.1).
    func summary(locale: Locale) -> String {
        [String(localized: "\(new.formatted(.number.locale(locale))) new", bundle: .module),
         String(localized: "\(learning.formatted(.number.locale(locale))) learning", bundle: .module),
         String(localized: "\(review.formatted(.number.locale(locale))) to review", bundle: .module)].joined(separator: " · ")
    }
}

/// The Today page (N14.1).
/// The sidebar layouts show it with no deck selected; an unfolded iPhone shows it beside the deck list.
public struct TodayView: View {
    public let collection: ReviewCollection
    private let besideDeckList: Bool
    @Environment(\.locale) private var locale
    #if !os(macOS)
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    #endif

    /// `besideDeckList` on an unfolded iPhone held wide: no Home screen around it names
    /// the page, and the deck list beside it already shows the collection's error line.
    public init(collection: ReviewCollection, besideDeckList: Bool = false) {
        self.collection = collection; self.besideDeckList = besideDeckList
    }

    public var body: some View {
        let today = TodayDecks(nodes: collection.decks)
        List {
            if today.entries.isEmpty {
                // No totals and no header over the one line (N14.1).
                Text("Nothing to study right now", bundle: .module)
                    .quietText()
                    .listRowSeparator(.hidden)
                    .accessibilityIdentifier("home.today.empty")
            } else {
                #if !os(macOS)
                // The Mac shows the totals as the window's subtitle.
                Text(verbatim: today.totals.summary(locale: locale))
                    .foregroundStyle(.token(.textSecondary))
                    .listRowSeparator(.hidden)
                    .accessibilityIdentifier("home.today.totals")
                #endif
                // No header: the title and totals already say what the rows are (N14.1).
                ForEach(today.entries) { row($0) }
            }
            if let error = collection.error, !besideDeckList {
                Section {
                    Text(verbatim: error)
                        .foregroundStyle(.token(.statusError))
                        .accessibilityIdentifier("home.collection.error")
                }
            }
            if let record = collection.previousRecord, !record.answers.isEmpty {
                Section {
                    Text("\(record.answers.count) answers recorded", bundle: .module)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("home.session.recorded")
                } header: {
                    Text("Previous Session", bundle: .module)
                        .accessibilityIdentifier("home.collection.previousSessionHeader")
                }
            }
        }
        #if !os(macOS)
        .listStyle(.plain)
        #endif
        .modifier(TodayTitle(enabled: besideDeckList, subtitle: HomeHeading.todaySubtitle(today.totals, locale: locale)))
    }

    private func row(_ entry: TodayDecks.Entry) -> some View {
        // At the accessibility sizes the count and Study go under the name, as other Home rows stack.
        (stacks ? AnyLayout(VStackLayout(alignment: .leading)) : AnyLayout(HStackLayout())) {
            VStack(alignment: .leading) {
                Text(verbatim: entry.name)
                if !entry.ancestors.isEmpty {
                    // A flat list: the parents as text, not as indentation (N14.1).
                    Text(verbatim: entry.ancestors.joined(separator: " › "))
                        .font(.caption)
                        .quietText()
                }
            }
            if !stacks { Spacer() }
            HStack {
                #if os(macOS)
                Text(verbatim: nonZeroCounts(entry.counts))
                    .foregroundStyle(.secondary)
                #else
                Text(entry.counts.waiting, format: .number)
                    .quietText()
                    .monospacedDigit()
                    .lineLimit(1)
                    .fixedSize()
                    // A bare number says nothing to VoiceOver.
                    .accessibilityLabel(Text("\(entry.counts.waiting) cards waiting", bundle: .module))
                #endif
                Button { Task { await collection.startStudy(deckID: entry.id) } } label: {
                    Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
                }
                .buttonStyle(.bordered)
                // The deck page's Study target.
                .frame(minHeight: ControlMetrics.minimumTarget)
                .contentShape(Rectangle())
                .disabled(!collection.canStudy || collection.navigation.sheet != nil || collection.addNotePresentation.opening)
                .accessibilityLabel(Text("Study \(entry.path)", bundle: .module))
                .accessibilityIdentifier("home.today.study.\(entry.id)")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.today.deck.\(entry.id)")
    }

    private var stacks: Bool {
        #if os(macOS)
        false
        #else
        dynamicTypeSize.isAccessibilitySize
        #endif
    }

    #if os(macOS)
    /// As the Decks rows had them (N11.5): only the counts that are not zero.
    private func nonZeroCounts(_ counts: ReviewCounts) -> String {
        [counts.new == 0 ? nil : String(localized: "\(counts.new.formatted(.number.locale(locale))) new", bundle: .module),
         counts.learning == 0 ? nil : String(localized: "\(counts.learning.formatted(.number.locale(locale))) learning", bundle: .module),
         counts.review == 0 ? nil : String(localized: "\(counts.review.formatted(.number.locale(locale))) to review", bundle: .module)]
            .compactMap { $0 }.joined(separator: " · ")
    }
    #endif
}

private struct TodayTitle: ViewModifier {
    let enabled: Bool
    let subtitle: String
    func body(content: Content) -> some View {
        if enabled {
            content
                .navigationTitle(Text("Today", bundle: .module))
                #if os(macOS)
                .navigationSubtitle(Text(verbatim: subtitle))
                #endif
        } else {
            content
        }
    }
}
