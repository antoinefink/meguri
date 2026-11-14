import AppModel
import SwiftUI

/// Only the system TimelineView schedules work. Every iterator ends at the cap;
/// the owning view removes the timeline entirely for hidden or static samples.
nonisolated struct ReviewTimerSchedule: TimelineSchedule, Hashable {
    let clockIdentity: ReviewTimerPresentation.ClockIdentity?
    let anchor: Date
    let firstUpdate: Date
    let updateCount: UInt64

    init(presentation: ReviewTimerPresentation) {
        // TimelineView requests entries in the system date domain, even when
        // appClock is fixed. This date only schedules repainting; displayed
        // elapsed time still comes from the retained monotonic answer clock.
        self.init(presentation: presentation, at: AppClock.system.now())
    }

    init(presentation: ReviewTimerPresentation, at anchor: Date) {
        clockIdentity = presentation.clockIdentity
        self.anchor = anchor
        firstUpdate = anchor.addingTimeInterval(Double(1_000 - presentation.milliseconds % 1_000) / 1_000)
        updateCount = presentation.isCounting ? UInt64(presentation.capSeconds) - presentation.seconds : 0
    }

    func entries(from startDate: Date, mode: TimelineScheduleMode) -> Entries {
        let skipped = max(0, ceil(startDate.timeIntervalSince(firstUpdate)))
        return Entries(initial: updateCount > 0 && startDate <= anchor ? anchor : nil,
            firstUpdate: firstUpdate, index: min(updateCount, UInt64(skipped)), count: updateCount)
    }

    nonisolated struct Entries: Sequence, IteratorProtocol {
        var initial: Date?
        let firstUpdate: Date
        var index: UInt64
        let count: UInt64

        mutating func next() -> Date? {
            if let initial { self.initial = nil; return initial }
            guard index < count else { return nil }
            defer { index += 1 }
            return firstUpdate.addingTimeInterval(Double(index))
        }
    }
}

struct OnScreenTimerView: View {
    let read: () -> ReviewTimerPresentation

    var body: some View {
        let presentation = read()
        if presentation.isVisible {
            if presentation.isCounting {
                let schedule = ReviewTimerSchedule(presentation: presentation)
                TimelineView(schedule) { _ in
                    ReviewTimerLabel(presentation: read())
                }
                // A changed answer clock/preset or resume gets a fresh finite
                // iterator even if the preceding timeline had exhausted its entries.
                .id(schedule)
            } else {
                ReviewTimerLabel(presentation: presentation)
            }
        }
    }
}

struct ReviewTimerLabel: View {
    let presentation: ReviewTimerPresentation
    @Environment(\.locale) private var locale

    static func accessibilityValue(_ presentation: ReviewTimerPresentation, locale: Locale) -> String {
        guard presentation.capped else { return presentation.text }
        let format = String(localized: "review.timer.capped", bundle: .module, locale: locale)
        return String(format: format, locale: locale, presentation.text)
    }

    var body: some View {
        HStack {
            Text(verbatim: presentation.text).monospacedDigit()
            if presentation.capped {
                // Minimal non-colour cap cue (T2). Native design acceptance is
                // pending: standard hourglass, no custom drawing or sizing.
                Image(systemName: "hourglass").accessibilityHidden(true)
            }
        }
        .font(.caption)
        .accessibilityElement(children: .ignore)
        // Keep the exact S17 value without also supplying the same elapsed
        // time as a label. The cap symbol stays outside the narration.
        .accessibilityLabel(Text(verbatim: ""))
        .accessibilityValue(Text(verbatim: Self.accessibilityValue(presentation, locale: locale)))
        .accessibilityIdentifier("review.session.timer")
    }
}
