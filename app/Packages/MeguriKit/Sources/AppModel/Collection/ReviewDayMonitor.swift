import CoreBridge
import Foundation

/// One absolute wall-clock timer per collection. The version protects both
/// check and resume results from events received while those calls awaited.
public final class ReviewDayMonitor {
    public private(set) var deadline: Date?
    /// The day boundary the last check or event gave (`next_day_at`, Unix seconds): when the
    /// study day of the counts read now ends. Kept while paused, as the day it names is.
    public private(set) var nextDayAt: Int64?
    private var version: UInt64 = 0
    private var open = true
    private let clock: AppClock
    private let zone: () -> TimeZone
    private let check: () async throws -> DayClockReport
    private let didCheck: () async -> Void
    private let schedule: (Date, @escaping @MainActor () -> Void) -> (() -> Void)
    private let usesInjectedSchedule: Bool
    private var cancel: (() -> Void)?
    public init(clock: AppClock, zone: @escaping () -> TimeZone,
                check: @escaping () async throws -> DayClockReport = { try await Core.checkRollover() },
                didCheck: @escaping () async -> Void = {},
                schedule: ((Date, @escaping @MainActor () -> Void) -> (() -> Void))? = nil) {
        usesInjectedSchedule = schedule != nil
        self.clock = clock; self.zone = zone; self.check = check; self.didCheck = didCheck
        self.schedule = schedule ?? { date, action in
            let timer = Timer(fire: date, interval: 0, repeats: false) { _ in
                Task { @MainActor in action() }
            }
            RunLoop.main.add(timer, forMode: .common)
            return { timer.invalidate() }
        }
    }
    public static func deadline(nextDayAt: Int64, now: Date, zone: TimeZone) -> Date {
        let boundary = Date(timeIntervalSince1970: Double(nextDayAt))
        let offsetChange = zone.nextDaylightSavingTimeTransition(after: now) ?? .distantFuture
        let nextSecond = Date(timeIntervalSince1970: floor(now.timeIntervalSince1970) + 1)
        return max(nextSecond, min(boundary, offsetChange))
    }
    public func beginResult() -> UInt64 { version }
    public func result(nextDayAt: Int64, beganAt: UInt64) {
        guard open, beganAt == version else { return }
        arm(nextDayAt)
    }
    public func event(nextDayAt: Int64) { version &+= 1; if open { arm(nextDayAt) } }
    public func checkNow() async {
        guard open else { return }
        let began = beginResult()
        do {
            let report = try await check()
            result(nextDayAt: report.nextDayAt, beganAt: began)
            await didCheck()
        } catch {
            if began == version { cancel?(); cancel = nil; deadline = nil }
            // Ready/CollectionReplaced retries. No timer after an unavailable call.
        }
    }
    public func pause() { cancel?(); cancel = nil; deadline = nil; version &+= 1 }
    public func close() { open = false; pause() }
    private func arm(_ nextDayAt: Int64) {
        self.nextDayAt = nextDayAt
        cancel?()
        let date = Self.deadline(nextDayAt: nextDayAt, now: clock.now(), zone: zone())
        deadline = date
        if clock.isFixed && !usesInjectedSchedule { return }
        cancel = schedule(date) { [weak self] in
            guard let self else { return }
            self.cancel = nil; self.deadline = nil
            Task { await self.checkNow() }
        }
    }
}
