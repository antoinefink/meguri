import CoreGraphics
import Testing

import DesignSystem

/// Nothing interactive sits on a foldable's fold (R9, N13).
@Suite struct FoldAwareLayoutTests {
    @Test func aFoldRunningTopToBottomSplitsTheWidth() {
        let fold = FoldGeometry(frames: [CGRect(x: 455, y: 0, width: 40, height: 669)])
        #expect(fold.acrossWidth == 455...495)
    }

    @Test func aFoldRunningSideToSideLeavesARowWhole() {
        let fold = FoldGeometry(frames: [CGRect(x: 0, y: 455, width: 669, height: 40)])
        #expect(fold.acrossWidth == nil)
    }

    @Test func noRegionsMeansNoFold() {
        #expect(FoldGeometry(frames: []).acrossWidth == nil)
    }

    @Test func columnsStopAtTheFold() {
        let columns = FoldAvoidingRow.columns(width: 951, fold: 455...495, spacing: 8)
        #expect(columns.leading == 0...455)
        #expect(columns.trailing == 495...951)
    }

    @Test func withoutAFoldTheColumnsAreEqual() {
        let columns = FoldAvoidingRow.columns(width: 408, fold: nil, spacing: 8)
        #expect(columns.leading == 0...200)
        #expect(columns.trailing == 208...408)
    }

    @Test func aFoldTooCloseToAnEdgeIsNotARowSplit() {
        // A window that only just reaches the fold keeps two usable columns.
        let columns = FoldAvoidingRow.columns(width: 500, fold: 455...495, spacing: 8)
        #expect(columns.leading == 0...246)
        #expect(columns.trailing == 254...500)
    }

    @Test func showAnswerIsCentredAcrossTheFoldAtItsHalfRowSize() {
        // The iPhone Duo held wide, inside the row's padding: 919 points, the fold at 439...479.
        let width: CGFloat = 919, fold: ClosedRange<CGFloat> = 439...479
        let span = FoldCentredRow.span(width: width, fold: fold, spacing: 8)
        let trailing = FoldAvoidingRow.columns(width: width, fold: fold, spacing: 8).trailing
        #expect(span.upperBound - span.lowerBound == trailing.upperBound - trailing.lowerBound, "Same width as before")
        #expect((span.lowerBound + span.upperBound) / 2 == width / 2, "Centred on the row")
        #expect(span.lowerBound < fold.lowerBound && span.upperBound > fold.upperBound, "It spans the fold")
    }

    @Test func withoutAFoldItIsHalfTheRowCentred() {
        let span = FoldCentredRow.span(width: 408, fold: nil, spacing: 8)
        #expect(span == 104...304)
    }
}
