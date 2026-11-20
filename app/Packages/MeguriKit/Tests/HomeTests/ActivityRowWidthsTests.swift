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
}
