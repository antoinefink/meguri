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

/// The Today page (N14.1, N24.2): Study for every deck with cards waiting, Studied Today,
/// the previous session and the collection's activity.
/// The sidebar layouts show it with no deck selected; an unfolded iPhone held wide shows it beside the deck list.
public struct TodayView: View {
    public let collection: ReviewCollection
    private let besideDeckList: Bool
    @Environment(\.locale) private var locale

    /// `besideDeckList` on an unfolded iPhone held wide: no Home screen around it names
    /// the page, and the deck list beside it already shows the collection's error line.
    public init(collection: ReviewCollection, besideDeckList: Bool = false) {
        self.collection = collection; self.besideDeckList = besideDeckList
    }

    public var body: some View {
        let totals = TodayDecks(nodes: collection.decks).totals
        TodaySummaryContent(
            summary: TodaySummary(nodes: collection.decks, studied: collection.studiedToday, previous: previous),
            nextLearningReadyAt: collection.nextLearningReadyAt, totals: totals,
            error: besideDeckList ? nil : collection.error,
            canStudy: collection.canStudy && collection.navigation.sheet == nil && !collection.addNotePresentation.opening,
            study: { Task { await collection.startStudyAll() } },
            activity: collection.activity, widths: collection.navigation.activityRowWidths)
            .activityLoading(collection.activity, scope: .collection, active: collection.heatmapRefreshes,
                             widths: collection.navigation.activityRowWidths, page: .today)
            .modifier(TodayTitle(enabled: besideDeckList, subtitle: HomeHeading.todaySubtitle(totals, locale: locale)))
    }

    /// The walk's in this launch, else the last deck's session the store keeps (N24.2).
    private var previous: TodaySummary.PreviousSession? {
        if let walk = collection.previousWalk {
            return .init(deckNames: walk.decks.compactMap { collection.fullDeckName($0) }, walk: true,
                         cards: walk.cards, endedAt: walk.endedAt)
        }
        guard let record = collection.previousRecord, let last = record.answers.last else { return nil }
        return .init(deckNames: collection.fullDeckName(record.deckID).map { [$0] } ?? [], walk: false,
                     cards: Set(record.answers.map(\.cardID)).count, endedAt: record.endedAt ?? last.answeredAt)
    }
}

/// N24.2's list, from values: the live page and its DTO preview draw the same tree.
struct TodaySummaryContent: View {
    let summary: TodaySummary
    let nextLearningReadyAt: Date?
    let totals: ReviewCounts
    let error: String?
    let canStudy: Bool
    let study: () -> Void
    let activity: ActivityHeatmapStore
    let widths: ActivityRowWidths
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone
    @Environment(\.appClock) private var clock
    #if !os(macOS)
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    #endif

    var body: some View {
        List {
            #if !os(macOS)
            // The Mac shows the totals as the window's subtitle; both hide while nothing waits (N14.4).
            if totals.waiting > 0 {
                Section {
                    Text(verbatim: totals.summary(locale: locale))
                        .foregroundStyle(.token(.textSecondary))
                        .listRowBackground(Color.clear)
                        .accessibilityIdentifier("home.today.totals")
                }
            }
            #endif
            Section {
                if summary.nothingWaiting { nothingWaiting } else { studyGroup }
            }
            Section { studiedRow }
            // Only after a session with at least one answer.
            if let value = summary.previousValue(now: clock.now(), calendar: zoned, locale: locale) {
                Section { previousRow(decks: summary.previousDecks(locale: locale), value: value) }
            }
            // N14.11, N24.6: the collection's activity, with its header.
            ActivityHeatmapSection(store: activity, scope: .collection, widths: widths, page: .today)
            if let error {
                Section {
                    Text(verbatim: error)
                        .foregroundStyle(.token(.statusError))
                        .accessibilityIdentifier("home.collection.error")
                }
            }
        }
        #if !os(macOS)
        // N24.5: the deck page's inset grouped list (N14.3).
        .listStyle(.insetGrouped)
        #endif
    }

    /// The device's calendar in the device's zone (N24.2's "Yesterday").
    private var zoned: Calendar {
        var calendar = calendar
        calendar.timeZone = timeZone
        return calendar
    }

    private var stacks: Bool {
        #if os(macOS)
        false
        #else
        dynamicTypeSize.isAccessibilitySize
        #endif
    }

    // MARK: Study

    /// The deck page's Study button, with the walk's lines beside it on the Mac and under it on touch.
    @ViewBuilder private var studyGroup: some View {
        let first = summary.studyLine(locale: locale)
        #if os(macOS)
        HStack(alignment: .center, spacing: Spacing.medium) {
            studyButton(value: first)
            studyLines(first: first)
        }
        .listRowSeparator(.hidden)
        #else
        VStack(alignment: .leading, spacing: Spacing.small) {
            studyButton(value: first)
            studyLines(first: first)
        }
        #endif
    }

    private func studyButton(value: String) -> some View {
        Button(action: study) {
            #if os(macOS)
            Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
            #else
            Text("Study", bundle: .module).font(.headline)
            #endif
        }
        .buttonStyle(.borderedProminent)
        // The deck page's Study target (N14.3).
        .frame(minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        .disabled(!canStudy)
        .accessibilityLabel(Text("Study All Decks", bundle: .module))
        .accessibilityInputLabels([Text("Study", bundle: .module), Text("Study All Decks", bundle: .module)])
        .accessibilityValue(Text(verbatim: value))
        .accessibilityIdentifier("home.today.studyAll")
    }

    private func studyLines(first: String) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: first)
                .accessibilityIdentifier("home.today.studyAll.summary")
            if let order = summary.orderLine(locale: locale) {
                Text(verbatim: order)
                    .accessibilityIdentifier("home.today.studyAll.order")
            }
        }
        .quietText()
        // Both lines wrap and never truncate.
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var nothingWaiting: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text("Nothing to study right now", bundle: .module)
                .accessibilityIdentifier("home.today.empty")
            if let next = nextLearningReadyAt, next > clock.now() {
                let time = next.formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale,
                                                           calendar: zoned, timeZone: timeZone))
                Text("Next learning card at \(time)", bundle: .module)
                    .quietText()
                    .accessibilityIdentifier("home.today.nextLearning")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .listRowSeparator(.hidden)
    }

    // MARK: Rows

    /// Studied Today: the label, the system's linear bar in the accent colour, and the value.
    private var studiedRow: some View {
        rowLayout {
            Text("Studied Today", bundle: .module)
                .fixedSize(horizontal: false, vertical: true)
            ProgressView(value: summary.studiedFraction)
                .progressViewStyle(.linear)
            Text(verbatim: summary.studiedValue(locale: locale))
                .quietText()
                .fixedSize(horizontal: !stacks, vertical: true)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Studied Today", bundle: .module))
        .accessibilityValue(Text(verbatim: summary.studiedAccessibilityValue(locale: locale)))
        .accessibilityIdentifier("home.today.studied")
        .listRowSeparator(.hidden)
    }

    private func previousRow(decks: String?, value: String) -> some View {
        rowLayout {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text("Previous Session", bundle: .module)
                if let decks {
                    Text(verbatim: decks).quietText()
                }
            }
            .fixedSize(horizontal: false, vertical: true)
            if !stacks { Spacer(minLength: Spacing.small) }
            Text(verbatim: value)
                .quietText()
                .fixedSize(horizontal: !stacks, vertical: true)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("home.today.previous")
        .listRowSeparator(.hidden)
    }

    /// A row's label and value side by side; at the accessibility text sizes the value goes under it.
    private func rowLayout<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        (stacks ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
                : AnyLayout(HStackLayout(alignment: .center, spacing: Spacing.medium))) { content() }
    }
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
