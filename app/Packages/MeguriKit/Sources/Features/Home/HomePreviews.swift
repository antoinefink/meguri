import AppModel
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

/// N24.2's Today over sample values, for the test VM's captures (`home/today/*` routes):
/// the live list's tree (`TodaySummaryContent`) with the decks' counts, T12's numbers, a
/// previous session and a heatmap with one catch-up day. It proves appearance only.
public enum HomeTodayPreview {
    public enum State: String, CaseIterable, Sendable {
        /// Four decks waiting, with a filtered deck that the walk leaves out.
        case summary
        /// Nothing waiting, with the next learning card's time.
        case nothingWaiting
        /// After a walk in this launch: its decks name the previous session.
        case afterWalk
    }

    public static func preview(_ state: State) -> some View { TodayPreviewHost(state: state) }

    static func nodes(_ state: State) -> [CountedDeckNode] {
        func deck(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32), filtered: Bool = false,
                  children: [CountedDeckNode] = []) -> CountedDeckNode {
            CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
                counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
        }
        switch state {
        case .summary:
            let vocabulary = deck(11, "Languages::Español::Vocabulary", (12, 3, 80))
            return [deck(1, "Languages", (20, 3, 172), children: [deck(10, "Languages::Español", (12, 3, 80), children: [vocabulary])]),
                    deck(2, "Science", (10, 2, 85)), deck(3, "Culture", (0, 0, 40)),
                    deck(4, "Quick Review", (0, 0, 30), filtered: true), deck(5, "Music Theory", (5, 0, 15)),
                    deck(6, "Reading", (0, 0, 0))]
        case .nothingWaiting:
            return [deck(1, "Languages", (0, 0, 0)), deck(2, "Science", (0, 0, 0)), deck(6, "Reading", (0, 0, 0))]
        case .afterWalk:
            return [deck(1, "Languages", (0, 0, 0)), deck(2, "Science", (0, 0, 0)), deck(3, "Culture", (0, 0, 0)),
                    deck(5, "Music Theory", (0, 2, 0))]
        }
    }

    static func summary(_ state: State, now: Date) -> TodaySummary {
        let studied = state == .afterWalk
            ? StudiedToday(answers: 402, cards: 344, timeMilliseconds: 2_460_000, recentAnswers: 900, recentTimeMilliseconds: 5_400_000)
            : StudiedToday(answers: 140, cards: 128, timeMilliseconds: 1_140_000, recentAnswers: 900, recentTimeMilliseconds: 5_400_000)
        let previous = state == .afterWalk
            ? TodaySummary.PreviousSession(deckNames: ["Languages", "Science", "Culture"], walk: true, cards: 216,
                                           endedAt: now.addingTimeInterval(-5 * 60))
            : TodaySummary.PreviousSession(deckNames: ["Languages::Español::Vocabulary"], walk: false, cards: 46,
                                           endedAt: now.addingTimeInterval(-50 * 60))
        return TodaySummary(nodes: nodes(state), studied: studied, previous: previous)
    }
}

private struct TodayPreviewHost: View {
    let state: HomeTodayPreview.State
    @State private var navigation = AppNavigation()
    @State private var activity: ActivityHeatmapStore?
    @Environment(\.appClock) private var clock
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale

    var body: some View {
        let now = clock.now()
        let totals = TodayDecks(nodes: HomeTodayPreview.nodes(state)).totals
        Group {
            if let activity {
                TodaySummaryContent(summary: HomeTodayPreview.summary(state, now: now),
                    nextLearningReadyAt: state == .nothingWaiting ? now.addingTimeInterval(2 * 3_600) : nil,
                    totals: totals, error: nil, canStudy: true, study: {},
                    activity: activity, widths: navigation.activityRowWidths)
                    .activityLoading(activity, scope: .collection, active: true,
                                     widths: navigation.activityRowWidths, page: .today)
            }
        }
        .onAppear {
            guard activity == nil else { return }
            activity = ActivityHeatmapStore(service: PreviewActivity(today: Self.day(now, calendar: calendar)))
        }
        .navigationTitle(Text("Today", bundle: .module))
        #if os(macOS)
        .navigationSubtitle(Text(verbatim: HomeHeading.todaySubtitle(totals, locale: locale)))
        #endif
        .accessibilityIdentifier("home.collection.screen")
    }

    private static func day(_ date: Date, calendar: Calendar) -> ActivityDays.Day {
        let parts = calendar.dateComponents([.year, .month, .day, .weekday], from: date)
        return ActivityDays.Day(year: parts.year ?? 2026, month: parts.month ?? 1, day: parts.day ?? 1, weekday: parts.weekday ?? 1)
    }
}

/// A year of ordinary days, some empty, and one catch-up day of 1,400 reviews (AH7's case).
private struct PreviewActivity: ActivityService {
    let today: ActivityDays.Day
    func heatmap(deckID: Int64?) async throws(BridgeError) -> ActivityDays {
        var counts = [UInt32](repeating: 0, count: 371)
        for day in counts.indices where day % 9 != 4 {
            let spread: Int = (day * 37) % 140
            counts[day] = UInt32(40 + spread)
        }
        counts[12] = 1_400
        return ActivityDays(generation: 0, schedulerDay: 1, nextDayAt: 0, today: today, counts: counts)
    }
}
