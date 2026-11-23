#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import Stats
import SwiftUI

extension TestHooks.StatsRoutes {
    /// The states of the Cards, Coming Up, Answering, Memory and Decks sections (plan U299 to
    /// U301), on the sample collection with the page scrolled to the section, All Decks and All
    /// unless named. For each section's first box, `stats/chart/<box>/<state>`:
    /// - `1m`, `3m`, `6m`, `1y`: that period (Activity too: it follows the period); `deck`:
    ///   Languages; `contrast`: Increase Contrast.
    /// - `loading`: the present tier back, the review log still reading (Card Counts, Reviews
    ///   Due and the Memory boxes filled, the others with their progress); `cold`: nothing back.
    /// - `empty`: no review yet (S12.3); `failed`: the section's parts fail alone (S12.8).
    /// - `pinned`: a pinned selection in every box of the section, with its callout.
    /// By Deck's sorting and a row's click are real clicks (`stats.box.byDeck.sort.<column>`,
    /// `stats.box.byDeck.row.<id>`), not routes.
    static func registerSections(in registry: inout TestHooks.RouteRegistry) {
        registry.registerShell("stats/chart/cards-over-time/1m") { _ in section(.cardsOverTime, period: .oneMonth) }
        registry.registerShell("stats/chart/cards-over-time/3m") { _ in section(.cardsOverTime, period: .threeMonths) }
        registry.registerShell("stats/chart/cards-over-time/6m") { _ in section(.cardsOverTime, period: .sixMonths) }
        registry.registerShell("stats/chart/cards-over-time/1y") { _ in section(.cardsOverTime, period: .oneYear) }
        registry.registerShell("stats/chart/cards-over-time/deck") { _ in section(.cardsOverTime, deck: StatsSampleService.decks[0].id) }
        registry.registerShell("stats/chart/cards-over-time/contrast") { _ in section(.cardsOverTime, option: .contrast) }
        registry.registerShell("stats/chart/cards-over-time/loading") { _ in section(.cardsOverTime, .ending(.stall(.reviewLog))) }
        registry.registerShell("stats/chart/cards-over-time/cold") { _ in section(.cardsOverTime, .ending(.stall(.present))) }
        registry.registerShell("stats/chart/cards-over-time/empty") { _ in section(.cardsOverTime, .shape(.noReviews)) }
        registry.registerShell("stats/chart/cards-over-time/failed") { _ in section(.cardsOverTime, .ending(.partFails(.cardStatesOverTime))) }
        registry.registerShell("stats/chart/card-counts/failed") { _ in section(.cardsOverTime, .ending(.partFails(.cardCounts))) }
        registry.registerShell("stats/chart/cards-over-time/pinned") { _ in
            section(.cardsOverTime, period: .oneMonth, pins: [
                (.cardsOverTime, { StatsChartSelection(bin: max(0, $0.data.bins.count - 6), pinned: true) }),
                (.cardCounts, { _ in StatsChartSelection(bin: 1, pinned: true) }),
                (.addedAndStarted, { StatsChartSelection(bin: max(0, $0.data.bins.count - 3), pinned: true) }),
            ])
        }
        registry.registerShell("stats/chart/activity/1m") { _ in section(.activity, period: .oneMonth) }
        registry.registerShell("stats/chart/activity/3m") { _ in section(.activity, period: .threeMonths) }
        registry.registerShell("stats/chart/activity/6m") { _ in section(.activity, period: .sixMonths) }
        registry.registerShell("stats/chart/activity/1y") { _ in section(.activity, period: .oneYear) }
        registry.registerShell("stats/chart/reviews-due/1m") { _ in section(.reviewsDue, period: .oneMonth) }
        registry.registerShell("stats/chart/reviews-due/3m") { _ in section(.reviewsDue, period: .threeMonths) }
        registry.registerShell("stats/chart/reviews-due/6m") { _ in section(.reviewsDue, period: .sixMonths) }
        registry.registerShell("stats/chart/reviews-due/1y") { _ in section(.reviewsDue, period: .oneYear) }
        registry.registerShell("stats/chart/reviews-due/deck") { _ in section(.reviewsDue, deck: StatsSampleService.decks[0].id) }
        registry.registerShell("stats/chart/reviews-due/contrast") { _ in section(.reviewsDue, option: .contrast) }
        registry.registerShell("stats/chart/reviews-due/loading") { _ in section(.reviewsDue, .ending(.stall(.present))) }
        registry.registerShell("stats/chart/reviews-due/empty") { _ in section(.reviewsDue, .shape(.noReviews)) }
        registry.registerShell("stats/chart/reviews-due/failed") { _ in section(.reviewsDue, .ending(.partFails(.futureDue))) }
        registry.registerShell("stats/chart/reviews-due/pinned") { _ in
            section(.reviewsDue, pins: [(.reviewsDue, { StatsChartSelection(bin: min(4, max(0, $0.data.bins.count - 1)), pinned: true) })])
        }
        registry.registerShell("stats/chart/answers/1m") { _ in section(.answers, period: .oneMonth) }
        registry.registerShell("stats/chart/answers/3m") { _ in section(.answers, period: .threeMonths) }
        registry.registerShell("stats/chart/answers/6m") { _ in section(.answers, period: .sixMonths) }
        registry.registerShell("stats/chart/answers/1y") { _ in section(.answers, period: .oneYear) }
        registry.registerShell("stats/chart/answers/deck") { _ in section(.answers, deck: StatsSampleService.decks[0].id) }
        registry.registerShell("stats/chart/answers/contrast") { _ in section(.answers, option: .contrast) }
        registry.registerShell("stats/chart/answers/loading") { _ in section(.answers, .ending(.stall(.reviewLog))) }
        registry.registerShell("stats/chart/answers/empty") { _ in section(.answers, .shape(.noReviews)) }
        registry.registerShell("stats/chart/answers/failed") { _ in section(.answers, .ending(.partFails(.buttons))) }
        registry.registerShell("stats/chart/time-of-day/failed") { _ in section(.answers, .ending(.partFails(.hours))) }
        registry.registerShell("stats/chart/answers/pinned") { _ in
            section(.answers, pins: [
                (.answers, { _ in StatsChartSelection(bin: 2, pinned: true) }),
                (.timeOfDay, { _ in StatsChartSelection(bin: 9, pinned: true) }),
            ])
        }
        registry.registerShell("stats/chart/retrievability/deck") { _ in section(.retrievability, deck: StatsSampleService.decks[0].id) }
        registry.registerShell("stats/chart/retrievability/contrast") { _ in section(.retrievability, option: .contrast) }
        registry.registerShell("stats/chart/retrievability/loading") { _ in section(.retrievability, .ending(.stall(.present))) }
        registry.registerShell("stats/chart/retrievability/empty") { _ in section(.retrievability, .shape(.noReviews)) }
        registry.registerShell("stats/chart/retrievability/failed") { _ in section(.retrievability, .ending(.partFails(.retrievability))) }
        registry.registerShell("stats/chart/stability/failed") { _ in section(.retrievability, .ending(.partFails(.stability))) }
        registry.registerShell("stats/chart/retrievability/pinned") { _ in
            section(.retrievability, pins: [
                (.retrievability, { StatsChartSelection(bin: max(0, $0.data.bins.count - 3), pinned: true) }),
                (.stability, { _ in StatsChartSelection(bin: 5, pinned: true) }),
                (.difficulty, { _ in StatsChartSelection(bin: 4, pinned: true) }),
            ])
        }
        registry.registerShell("stats/chart/by-deck/1m") { _ in section(.byDeck, period: .oneMonth) }
        registry.registerShell("stats/chart/by-deck/1y") { _ in section(.byDeck, period: .oneYear) }
        registry.registerShell("stats/chart/by-deck/deck") { _ in section(.byDeck, deck: StatsSampleService.decks[0].id) }
        registry.registerShell("stats/chart/by-deck/leaf") { _ in section(.byDeck, deck: StatsSampleService.decks[0].children[0].id) }
        registry.registerShell("stats/chart/by-deck/contrast") { _ in section(.byDeck, option: .contrast) }
        registry.registerShell("stats/chart/by-deck/loading") { _ in section(.byDeck, .ending(.stall(.reviewLog))) }
        registry.registerShell("stats/chart/by-deck/empty") { _ in section(.byDeck, .shape(.noReviews)) }
        registry.registerShell("stats/chart/by-deck/failed") { _ in section(.byDeck, .ending(.partFails(.byDeck))) }
    }

    /// The page scrolled to `box`'s section; `pins` pin a selection in boxes of it once they are
    /// ready, after the section scroll so the callouts are on screen.
    @MainActor private static func section(
        _ box: StatsBoxID, _ sample: Sample = .typical, period: StatsPagePeriod = .all, deck: Int64? = nil,
        option: Option = .none, pins: [(box: StatsBoxID, selection: (StatsBoxContent) -> StatsChartSelection)] = []
    ) -> TestHooks.ShellLaunch {
        var launch = page(sample, period: period, deck: deck, box: box, option: option)
        guard !pins.isEmpty, let model = TestHooks.StatsHooks.shared.page?.model else { return launch }
        let prepare = launch.prepare
        launch.prepare = { navigation in
            prepare(navigation)
            Task { @MainActor in
                for _ in 0..<100 where pins.contains(where: { model.box($0.box).phase != .ready }) {
                    try? await Task.sleep(for: .milliseconds(100))
                }
                try? await Task.sleep(for: .milliseconds(800))
                for pin in pins {
                    model.select(pin.selection(model.box(pin.box)), in: pin.box)
                }
            }
        }
        return launch
    }
}
#endif
