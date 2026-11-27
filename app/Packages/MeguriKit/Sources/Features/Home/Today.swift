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
        totals = Self.totals(nodes: nodes)
    }

    /// Totals need only the roots: their counts already include descendants and
    /// daily limits. Do not build waiting-deck entries for a heading or badge.
    public static func totals(nodes: [CountedDeckNode]) -> ReviewCounts {
        nodes.reduce(ReviewCounts(new: 0, learning: 0, review: 0)) {
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
        waiting.regularNothingWaiting ? String(localized: "Nothing to study right now", bundle: .module)
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
        let phase = collection.homePhase
        if phase == .pending || collection.homeCountsHeld || !phase.showsDecks {
            // Not known yet, or kept from an earlier study day: nothing here would be true
            // yet, so the page waits for the read in its own background (N27.3).
            HomePendingPage(grouped: true, label: Text("Loading Today", bundle: .module))
                .modifier(TodayTitle(enabled: besideDeckList))
        } else {
            let decks = collection.homeDecks
            let totals = TodayDecks.totals(nodes: decks)
            TodaySummaryContent(
                summary: TodaySummary(nodes: decks, studied: collection.homeStudiedToday, previous: previous),
                nextLearningReadyAt: collection.homeNextLearningReadyAt, totals: totals,
                error: besideDeckList ? nil : collection.error,
                // While Study starts, the button keeps its look until the card shows (N16.8);
                // on kept content it looks as it will, and its press waits for the read (N27.4).
                canStudy: collection.studyAllAvailable || collection.studyStarting || collection.studyAllKept,
                study: { Task { await collection.startStudyAll() } },
                activity: collection.activity, widths: collection.navigation.activityRowWidths)
                .activityLoading(collection.activity, scope: .collection, active: collection.heatmapRefreshes,
                                 widths: collection.navigation.activityRowWidths, page: .today)
                .modifier(TodayTitle(enabled: besideDeckList))
                // Study starts the walk's first deck: its first card waits beneath (N16.8).
                .modifier(StudyActionTarget(collection: collection, deckID: collection.studyAllDecks.first?.id))
        }
    }

    /// The walk's in this launch, else the last deck's session the store keeps (N24.2).
    private var previous: TodaySummary.PreviousSession? {
        collection.homePreviousSession.map {
            .init(deckNames: $0.deckNames, walk: $0.walk, cards: $0.cards, endedAt: $0.endedAt)
        }
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
        // N26.2: the totals are the large title's subtitle, as the Mac's toolbar shows them
        // (N14.4, set by the page there); no subtitle at all while nothing waits.
        .modifier(TodaySubtitle(text: HomeHeading.todaySubtitle(totals, locale: locale)))
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
        let value = summary.studiedValue(locale: locale)
        return Group {
            if stacks {
                rowLayout {
                    Text("Studied Today", bundle: .module)
                        .fixedSize(horizontal: false, vertical: true)
                    studiedBar
                    Text(verbatim: value).quietText()
                        .fixedSize(horizontal: false, vertical: true)
                }
            } else {
                StudiedTodayLine(value: value, fraction: summary.studiedFraction)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Studied Today", bundle: .module))
        .accessibilityValue(Text(verbatim: summary.studiedAccessibilityValue(locale: locale)))
        .accessibilityIdentifier("home.today.studied")
        .listRowSeparator(.hidden)
    }

    private var studiedBar: some View {
        ProgressView(value: summary.studiedFraction).progressViewStyle(.linear)
    }

    private func previousRow(decks: String?, value: String) -> some View {
        PreviousSessionLine(decks: decks, value: value, stacks: stacks)
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

#if !os(macOS)
/// N26.2: the totals under the large title; none while nothing waits, so no empty line shows.
private struct TodaySubtitle: ViewModifier {
    let text: String
    func body(content: Content) -> some View {
        if text.isEmpty { content } else { content.navigationSubtitle(Text(verbatim: text)) }
    }
}
#endif

private struct TodayTitle: ViewModifier {
    let enabled: Bool
    func body(content: Content) -> some View {
        if enabled {
            content
                .navigationTitle(Text("Today", bundle: .module))
        } else {
            content
        }
    }
}

/// Studied Today's row where it does not stack (N24.2): one line while the label, a usable
/// bar and the value fit; otherwise the bar goes on its own line under them; at the
/// narrowest the label and the value wrap, never running past the row.
struct StudiedTodayLine: View {
    let value: String
    let fraction: Double

    /// The narrowest bar still worth drawing beside the label and value.
    private static let usableBar = Spacing.xxLarge * 2

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .center, spacing: Spacing.medium) {
                label.lineLimit(1).fixedSize()
                bar.frame(minWidth: Self.usableBar, idealWidth: Self.usableBar, maxWidth: .infinity)
                valueText.lineLimit(1).fixedSize()
            }
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                    label.lineLimit(1).fixedSize()
                    Spacer(minLength: Spacing.small)
                    valueText.lineLimit(1).fixedSize()
                }
                bar
            }
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                    label
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    valueText
                        .multilineTextAlignment(.trailing)
                        .fixedSize(horizontal: false, vertical: true)
                }
                bar
            }
        }
    }

    private var label: Text { Text("Studied Today", bundle: .module) }
    private var valueText: some View { Text(verbatim: value).quietText() }
    private var bar: some View { ProgressView(value: fraction).progressViewStyle(.linear) }
}

/// Previous Session's row (N24.2): the label, with the decks under it, and the value beside
/// them while both fit on one line; otherwise, and always at the accessibility text sizes,
/// the value goes under them. Every text wraps, never running past the row.
struct PreviousSessionLine: View {
    let decks: String?
    let value: String
    let stacks: Bool

    var body: some View {
        if stacks {
            stacked
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .center, spacing: Spacing.medium) {
                    label
                    Spacer(minLength: Spacing.small)
                    Text(verbatim: value).quietText().lineLimit(1).fixedSize()
                }
                stacked
            }
        }
    }

    private var stacked: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            label
            Text(verbatim: value).quietText()
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var label: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text("Previous Session", bundle: .module)
            if let decks {
                Text(verbatim: decks).quietText()
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}
