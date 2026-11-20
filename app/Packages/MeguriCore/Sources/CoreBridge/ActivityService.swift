internal import MeguriCoreBindings
import Instrumentation

/// One scope's review counts for the activity heatmap (`stats/activity-heatmap` AH6).
public struct ActivityDays: Sendable, Hashable {
    /// A calendar date (proleptic Gregorian) and its weekday, 1 Sunday to 7 Saturday as `Calendar` numbers them.
    public struct Day: Sendable, Hashable {
        public let year: Int
        public let month: Int
        public let day: Int
        public let weekday: Int
        public init(year: Int, month: Int, day: Int, weekday: Int) {
            self.year = year; self.month = month; self.day = day; self.weekday = weekday
        }
    }

    public let generation: UInt64
    /// The scheduler day number the counts were taken for.
    public let schedulerDay: UInt32
    /// The instant the next scheduler day starts, epoch seconds.
    public let nextDayAt: Int64
    /// The date of day 0.
    public let today: Day
    /// Reviews per day; index = days ago, 0 (today) to 370.
    public let counts: [UInt32]

    public init(generation: UInt64, schedulerDay: UInt32, nextDayAt: Int64, today: Day, counts: [UInt32]) {
        self.generation = generation; self.schedulerDay = schedulerDay; self.nextDayAt = nextDayAt
        self.today = today; self.counts = counts
    }

    init(_ raw: MeguriCoreBindings.ActivityCounts) {
        self.init(generation: raw.generation, schedulerDay: raw.schedulerDay, nextDayAt: raw.boundarySecs,
                  today: Day(year: Int(raw.dayZeroYear), month: Int(raw.dayZeroMonth), day: Int(raw.dayZeroDay),
                             weekday: Int(raw.dayZeroWeekday) + 1),
                  counts: raw.perDay)
    }
}

public protocol ActivityService: Sendable {
    /// The whole collection (`deckID` nil) or a deck and its subdecks.
    func heatmap(deckID: Int64?) async throws(BridgeError) -> ActivityDays
}

/// Reads on the query lane and the core's query reader, never the interactive lane or the
/// writer's ops (only the day clock's own rollover check, when the day has changed, may
/// wait for the writer, as the deck counts do). A request cancelled while it waits for
/// the lane never reaches the core.
public struct CoreActivityService: ActivityService {
    public init() {}
    public func heatmap(deckID: Int64?) async throws(BridgeError) -> ActivityDays {
        if Task.isCancelled { throw .cancelled }
        return try await QueryLane.shared.call(.activityHeatmap) {
            if Task.isCancelled { throw BridgeError.cancelled }
            return ActivityDays(try activityHeatmap(deckId: deckID))
        }
    }
}
