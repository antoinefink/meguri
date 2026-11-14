import AppModel
import Foundation
import SwiftUI
import Testing
@testable import Review

@MainActor
@Suite struct OnScreenTimerTests {
    private let anchor = Date(timeIntervalSince1970: 1_800_000_000)

    @Test(arguments: [Date(timeIntervalSince1970: 0), Date(timeIntervalSince1970: 32_503_680_000)])
    func productionAnchorUsesSystemDatesDespiteFixedAppDates(appDate: Date) {
        let appClock = AppClock.fixed(appDate)
        let sample = ReviewTimerPresentation(milliseconds: 58_250, capSeconds: 60, isVisible: true, isCounting: true)
        let before = AppClock.system.now()
        // This is the initializer OnScreenTimerView uses to choose its anchor,
        // without supplying an already-correct date from the test.
        let schedule = ReviewTimerSchedule(presentation: sample)
        let after = AppClock.system.now()
        #expect(schedule.anchor >= before && schedule.anchor <= after)
        #expect(abs(schedule.anchor.timeIntervalSince(appClock.now())) > 86_400)
        let updates = Array(schedule.entries(from: schedule.anchor, mode: .normal))
        #expect(updates == [schedule.anchor, schedule.anchor.addingTimeInterval(0.75),
            schedule.anchor.addingTimeInterval(1.75)])
        // A normal system request just after creation still gets the next
        // display second and the final cap update, then exhausts the iterator.
        #expect(Array(schedule.entries(from: schedule.anchor.addingTimeInterval(0.1), mode: .normal)) == Array(updates.dropFirst()))
        #expect(Array(schedule.entries(from: schedule.anchor.addingTimeInterval(2), mode: .normal)).isEmpty)
        let appAnchored = ReviewTimerSchedule(presentation: sample, at: appClock.now())
        let misplaced = Array(appAnchored.entries(from: schedule.anchor, mode: .normal))
        #expect(misplaced.isEmpty || misplaced[0] > schedule.anchor.addingTimeInterval(86_400))
    }

    @Test func lazyIteratorIncludesTheLastCapUpdateAndThenEnds() {
        let sample = ReviewTimerPresentation(milliseconds: 58_250, capSeconds: 60, isVisible: true, isCounting: true)
        let schedule = ReviewTimerSchedule(presentation: sample, at: anchor)
        var entries = schedule.entries(from: anchor, mode: .normal).makeIterator()
        #expect(entries.next() == anchor)
        #expect(entries.next() == anchor.addingTimeInterval(0.75))
        #expect(entries.next() == anchor.addingTimeInterval(1.75))
        #expect(entries.next() == nil)
        #expect(entries.next() == nil)
        // SwiftUI may request a new iterator after suspension, including past the cap.
        #expect(Array(schedule.entries(from: anchor.addingTimeInterval(1), mode: .lowFrequency)) == [anchor.addingTimeInterval(1.75)])
        #expect(Array(schedule.entries(from: anchor.addingTimeInterval(2), mode: .normal)).isEmpty)
    }

    @Test func staticStatesHaveNoEntriesEvenWhenSwiftUIRequestsAnotherIterator() {
        let samples = [
            ReviewTimerPresentation(milliseconds: 5_000, capSeconds: 60, isVisible: false, isCounting: true),
            ReviewTimerPresentation(milliseconds: 5_000, capSeconds: 60, isVisible: true, isCounting: false),
            ReviewTimerPresentation(milliseconds: 60_000, capSeconds: 60, isVisible: true, isCounting: true),
        ]
        for sample in samples {
            let schedule = ReviewTimerSchedule(presentation: sample, at: anchor)
            #expect(Array(schedule.entries(from: anchor, mode: .normal)).isEmpty)
            #expect(Array(schedule.entries(from: anchor.addingTimeInterval(600), mode: .normal)).isEmpty)
        }
    }

    @Test func exhaustedScheduleDoesNotPreventANewQuestionOrResume() {
        let capped = ReviewTimerSchedule(presentation: .init(milliseconds: 60_000, capSeconds: 60,
            isVisible: true, isCounting: true), at: anchor)
        #expect(Array(capped.entries(from: anchor, mode: .normal)).isEmpty)
        let later = anchor.addingTimeInterval(600)
        let newQuestion = ReviewTimerSchedule(presentation: .init(milliseconds: 0, capSeconds: 2,
            isVisible: true, isCounting: true), at: later)
        #expect(Array(newQuestion.entries(from: later, mode: .normal)) == [later, later.addingTimeInterval(1), later.addingTimeInterval(2)])
        let resumed = ReviewTimerSchedule(presentation: .init(milliseconds: 1_250, capSeconds: 2,
            isVisible: true, isCounting: true), at: later)
        #expect(Array(resumed.entries(from: later, mode: .normal)) == [later, later.addingTimeInterval(0.75)])
    }

    @Test func maximumCapIsLazyAndFinite() {
        let schedule = ReviewTimerSchedule(presentation: .init(milliseconds: 0, capSeconds: 7_200,
            isVisible: true, isCounting: true), at: anchor)
        var entries = schedule.entries(from: anchor.addingTimeInterval(7_199), mode: .normal).makeIterator()
        #expect(entries.next() == anchor.addingTimeInterval(7_199))
        #expect(entries.next() == anchor.addingTimeInterval(7_200))
        #expect(entries.next() == nil)
    }

    @Test func newClockRestartsEvenWithTheSameWallAnchorAndDisplaySecond() {
        var clock = ReviewActiveTime()
        clock.start(at: 0)
        let first = ReviewTimerSchedule(presentation: .init(milliseconds: 0, capSeconds: 2,
            isVisible: true, isCounting: true, clockIdentity: clock.displayClockIdentity), at: anchor)
        #expect(Array(first.entries(from: anchor, mode: .normal)).count == 3)
        clock.start(at: 100)
        let next = ReviewTimerSchedule(presentation: .init(milliseconds: 0, capSeconds: 2,
            isVisible: true, isCounting: true, clockIdentity: clock.displayClockIdentity), at: anchor)
        // This is the identity consumed by TimelineView.id, not merely new data
        // for an already exhausted iterator.
        #expect(first != next)
        #expect(Array(next.entries(from: anchor, mode: .normal)).count == 3)
    }

    @Test func accessibilityUsesTheReviewCatalogAndPlainValue() {
        for (seconds, text) in [(0, "00:00"), (5, "00:05"), (60, "01:00"), (754, "12:34"), (7_200, "120:00")] {
            let sample = ReviewTimerPresentation(milliseconds: UInt64(seconds) * 1_000, capSeconds: 7_200,
                isVisible: true, isCounting: true)
            #expect(ReviewTimerLabel.accessibilityValue(sample, locale: Locale(identifier: "en")) ==
                (seconds == 7_200 ? "120:00, limit reached" : text))
        }
        let cap = ReviewTimerPresentation(milliseconds: 75_000, capSeconds: 60, isVisible: true, isCounting: false)
        #expect(ReviewTimerLabel.accessibilityValue(cap, locale: Locale(identifier: "en")) == "01:00, limit reached")
    }
}
