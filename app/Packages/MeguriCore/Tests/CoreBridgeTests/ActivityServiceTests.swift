@testable import CoreBridge
import MeguriCoreBindings
import Testing

@Suite struct ActivityServiceTests {
    @Test func sparseHistoryCrossesTheBridgeWithoutAncientDatePadding() {
        let window = MeguriCoreBindings.ActivityCounts(
            perDay: [2] + [UInt32](repeating: 0, count: 370),
            dayZeroYear: 2026, dayZeroMonth: 11, dayZeroDay: 24, dayZeroWeekday: 2,
            schedulerDay: 10, boundarySecs: 1_795_579_200, generation: 9)
        let raw = MeguriCoreBindings.ActivityHistoryCounts(window: window,
            history: [.init(daysAgo: 0, count: 2), .init(daysAgo: 500, count: 7),
                      .init(daysAgo: 100_000_000_000, count: 1)])
        let days = ActivityDays(raw)
        #expect(days.counts.count == 371 && days.counts[0] == 2)
        #expect(days.history == [0: 2, 500: 7, 100_000_000_000: 1])
        #expect(days.today == .init(year: 2026, month: 11, day: 24, weekday: 3))
        #expect(days.schedulerDay == 10 && days.nextDayAt == window.boundarySecs && days.generation == 9)
        #expect(ActivityDays(window).history == nil)
        let bounded = ActivityDays(generation: 9, schedulerDay: 10, nextDayAt: days.nextDayAt,
                                   today: days.today, counts: days.counts)
        #expect(bounded.history == nil, "existing DTO callers do not claim unqueried older data")
    }
}
