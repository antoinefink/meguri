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
        #expect(widths.row(.collection) == 0, "pages are kept apart")
        widths.measuredRow(.deck, 322)
        widths.measuredList(.deck, 874)
        #expect(widths.row(.deck) == 322)
    }
}
