import CoreBridge
import Foundation
import SwiftUI
import Testing
@testable import Home

/// The heatmap's row widths outlive its views (N14.11 rule 1): a page drawn again
/// knows its width at once, and a group that is off screen measures again after the
/// list's width changed.
@MainActor
@Suite struct ActivityRowWidthsTests {
    @Test func aRowKeepsItsWidthAcrossViewsAndForgetsItOnlyWhenAwayAndResized() {
        let widths = ActivityRowWidths()
        #expect(widths.row(.deck) == 0)
        widths.measuredList(.deck, 402)
        widths.rowAppeared(.deck)
        widths.measuredRow(.deck, 322)
        // A new view instance (a pushed page, a recreated row) reads it back.
        #expect(widths.row(.deck) == 322)
        // On screen, a list resize leaves the row to measure itself.
        widths.measuredList(.deck, 874)
        #expect(widths.row(.deck) == 322)
        // Hidden or left: a resize forgets the width, so the group measures again.
        widths.rowLeft(.deck)
        widths.measuredList(.deck, 874)
        #expect(widths.row(.deck) == 322, "the same width changes nothing")
        widths.measuredList(.deck, 402)
        #expect(widths.row(.deck) == 0)
        #expect(widths.row(.today) == 0, "each list keeps its own")
    }

    @Test func anOverlappingReplacementKeepsTheWidthWhateverTheCallbackOrder() {
        let widths = ActivityRowWidths()
        widths.measuredList(.deckList, 402)
        widths.rowAppeared(.deckList)
        widths.measuredRow(.deckList, 362)
        // The new row appears and measures before the old one's disappear arrives,
        // then the list reports its new width.
        widths.rowAppeared(.deckList)
        widths.measuredRow(.deckList, 700)
        widths.rowLeft(.deckList)
        widths.measuredList(.deckList, 740)
        #expect(widths.row(.deckList) == 700)
    }

    /// N24.6 (changed 2026-11-27): on every platform the grid and its month labels start at
    /// the row's leading edge, under the header and the summary line; whatever width whole
    /// weeks leave over, always less than one pitch, is at the trailing side.
    @Test func theGridStartsAtTheLeadingEdgeAndLeavesUnderOnePitchTrailing() {
        #expect(ActivityHeatmapSection.gridAlignment == Alignment.leading)
        let history = ActivityDays(generation: 1, schedulerDay: 10, nextDayAt: 0,
                                   today: .init(year: 2026, month: 11, day: 24, weekday: 3),
                                   counts: [], history: [0: 3])
        let pitch = ActivityHeatmapSection.square.side + ActivityHeatmapSection.square.gap
        var weeks: [Int] = []
        // Every point from one week less than a year to well past it, Mac and touch alike.
        for width in stride(from: CGFloat(600), through: 700, by: 1) {
            let geometry = ActivityHeatmapSection.Presentation(days: history, width: width, firstWeekday: 1).geometry
            let unused = width - geometry.width
            #expect(unused >= 0 && unused < pitch, "\(width) pt leaves \(unused) pt")
            weeks.append(geometry.weeks)
        }
        // A wider row only adds whole weeks, one per pitch.
        #expect(zip(weeks, weeks.dropFirst()).allSatisfy { $1 == $0 || $1 == $0 + 1 })
    }

    #if os(macOS)
    private func days(_ history: [Int: UInt32]?) -> ActivityDays {
        ActivityDays(generation: 1, schedulerDay: 10, nextDayAt: 0,
                     today: .init(year: 2026, month: 11, day: 24, weekday: 3),
                     counts: [UInt32](repeating: 0, count: 371), history: history)
    }

    @Test func oldOnlyHistoryCanHideRemeasureRevealAndHideAgain() {
        let widths = ActivityRowWidths()
        let old = days([371: 4, 500: 7])
        for page in [ActivityRowWidths.Page.today, .deck] {
            widths.measuredList(page, 700)
            widths.measuredRow(page, 634)
            let hidden = ActivityHeatmapSection.Presentation(days: old, width: widths.row(page), firstWeekday: 1)
            #expect(!hidden.visible && hidden.grid?.total == 0)
            // A hidden row has no measurement view. The outer list invalidates its
            // saved width so it can appear briefly to measure its wider space.
            widths.measuredList(page, 1_100)
            #expect(widths.row(page) == 0)
            let measuring = ActivityHeatmapSection.Presentation(days: old, width: widths.row(page), firstWeekday: 1)
            #expect(measuring.visible && !measuring.shown && measuring.geometry.height == 82)
            widths.measuredRow(page, 1_000)
            let wide = ActivityHeatmapSection.Presentation(days: old, width: widths.row(page), firstWeekday: 1)
            #expect(wide.visible && wide.shown && wide.grid?.total == 11 && wide.geometry.weeks == 83)
            widths.measuredRow(page, 634)
            #expect(!ActivityHeatmapSection.Presentation(days: old, width: widths.row(page), firstWeekday: 1).visible)
        }
    }

    @Test func emptyStartupAndBoundedLegacyResultsKeepTheirCoverage() {
        let startup = ActivityHeatmapSection.Presentation(days: nil, width: 0, firstWeekday: 1)
        #expect(startup.visible && !startup.shown && startup.geometry.height == 82)
        for width in [CGFloat(0), 634, 1_000] {
            #expect(!ActivityHeatmapSection.Presentation(days: days([:]), width: width, firstWeekday: 1).visible)
        }
        let bounded = ActivityHeatmapSection.Presentation(days: days(nil), width: 1_000, firstWeekday: 1)
        #expect(bounded.geometry.weeks == 53 && bounded.grid?.weeks == 53)
        #expect(bounded.geometry.height == 82 && !bounded.visible)
    }
    #endif
}
