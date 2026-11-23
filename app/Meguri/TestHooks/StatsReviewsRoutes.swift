#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import Stats
import SwiftUI

extension TestHooks.StatsRoutes {
    /// The Reviews section's states (plan U298), on the sample collection with the page scrolled
    /// to the section, All Decks and All unless named:
    /// - `stats/chart/reviews-over-time/<state>`: `by-answer`, `time` (By Type in Time),
    ///   `1m`, `3m`, `6m`, `1y`, `deck` (Languages), `loading` (the review log still reading),
    ///   `empty` (no review yet, S12.3), `failed` (its part fails alone, S12.8), `pinned` (1M with
    ///   a day pinned), `contrast` (Increase Contrast).
    /// - `stats/chart/activity/<state>`: `pinned` (a day pinned), `failed` (its part fails alone),
    ///   `recent` (reviews only this year: one partial year).
    static func registerReviews(in registry: inout RouteRegistry) {
        registry.registerShell("stats/chart/reviews-over-time/by-answer") { _ in reviews(.typical) { $0.setReviewsMode(.byAnswer) } }
        registry.registerShell("stats/chart/reviews-over-time/time") { _ in reviews(.typical) { $0.setReviewsMeasure(.time) } }
        registry.registerShell("stats/chart/reviews-over-time/1m") { _ in reviews(.typical, period: .oneMonth) }
        registry.registerShell("stats/chart/reviews-over-time/3m") { _ in reviews(.typical, period: .threeMonths) }
        registry.registerShell("stats/chart/reviews-over-time/6m") { _ in reviews(.typical, period: .sixMonths) }
        registry.registerShell("stats/chart/reviews-over-time/1y") { _ in reviews(.typical, period: .oneYear) }
        registry.registerShell("stats/chart/reviews-over-time/deck") { _ in
            reviews(.typical, deck: StatsSampleService.decks[0].id)
        }
        registry.registerShell("stats/chart/reviews-over-time/loading") { _ in reviews(.ending(.stall(.reviewLog))) }
        registry.registerShell("stats/chart/reviews-over-time/empty") { _ in reviews(.shape(.noReviews)) }
        registry.registerShell("stats/chart/reviews-over-time/failed") { _ in reviews(.ending(.partFails(.reviews))) }
        registry.registerShell("stats/chart/reviews-over-time/pinned") { _ in
            reviews(.typical, period: .oneMonth, pin: (.reviewsOverTime, { $0.data.bins.count - 3 }))
        }
        registry.registerShell("stats/chart/reviews-over-time/contrast") { _ in reviews(.typical, option: .contrast) }
        registry.registerShell("stats/chart/activity/pinned") { _ in reviews(.typical, pin: (.activity, { _ in -2 })) }
        registry.registerShell("stats/chart/activity/failed") { _ in reviews(.ending(.partFails(.activityDays))) }
        registry.registerShell("stats/chart/activity/recent") { _ in reviews(.shape(.recent)) }
    }

    /// The page scrolled to the Reviews section; `configure` sets the boxes' switches before the
    /// first refresh, and `pin` pins a bin of a box once it is ready (Activity's bins are days).
    @MainActor private static func reviews(
        _ sample: Sample, period: StatsPagePeriod = .all, deck: Int64? = nil, option: Option = .none,
        pin: (box: StatsBoxID, bin: (StatsBoxContent) -> Int)? = nil,
        configure: (StatsPageModel) -> Void = { _ in }
    ) -> ShellLaunch {
        var launch = page(sample, period: period, deck: deck, box: .reviewsOverTime, option: option)
        guard let model = StatsHooks.shared.page?.model else { return launch }
        configure(model)
        if let pin {
            let prepare = launch.prepare
            launch.prepare = { navigation in
                prepare(navigation)
                Task { @MainActor in
                    for _ in 0..<100 where model.box(pin.box).phase != .ready { try? await Task.sleep(for: .milliseconds(100)) }
                    // After the section scroll, so the pinned callout is on screen.
                    try? await Task.sleep(for: .milliseconds(800))
                    model.select(StatsChartSelection(bin: pin.bin(model.box(pin.box)), pinned: true), in: pin.box)
                }
            }
        }
        return launch
    }
}
#endif
