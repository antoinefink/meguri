import CoreGraphics
import Testing
@testable import HeatmapKit

/// Mac Home always keeps 10 pt cells and reveals older complete weeks (N14.11, N24.6).
@Suite struct ActivityHomeGeometryTests {
    @Test(arguments: [CGFloat(22), 23.5, 321.25, 633, 634, 635, 645.99, 646, 1_000, 2_000, 10_000])
    func fixedSquaresLeaveLessThanOnePitch(width: CGFloat) {
        let geometry = ActivityGrid.Geometry(width: width, square: .mac, maximumWeeks: nil)
        #expect(geometry.square == .mac && geometry.height == 82)
        #expect(geometry.pitch == 12 && geometry.square.cornerRadius == 2)
        #expect(geometry.width <= width)
        #expect(width - geometry.width < geometry.pitch)
        #expect(CGFloat(geometry.weeks - 1) * 12 + 10 == geometry.width)
    }

    @Test func resizeRevealsOlderWeeksBeyondFiftyThreeAndBack() {
        let widths: [CGFloat] = [633, 634, 635, 645.99, 646, 1_000, 2_000, 646, 634, 633]
        let geometries = widths.map { ActivityGrid.Geometry(width: $0, square: .mac, maximumWeeks: nil) }
        #expect(geometries.map(\.weeks) == [52, 53, 53, 53, 54, 83, 166, 54, 53, 52])
        #expect(geometries.first == geometries.last)
        let history: [Int: UInt32] = [0: 2, 371: 3, 500: 5, 900: 7, Int.max / 100: 100]
        let grids = geometries.map { ActivityGrid(history: history, weeks: $0.weeks, todayWeekday: 7, firstWeekday: 1) }
        #expect(grids.map(\.total) == [2, 2, 2, 2, 5, 10, 17, 5, 2, 2])
        for grid in grids {
            #expect(grid.daysAgo(column: grid.weeks - 1, row: 6) == 0)
            #expect(grid.count(daysAgo: 372) == 0, "missing days are implicit zero")
        }
        #expect(grids[4].maximum == 3 && grids[6].maximum == 7)
    }

    @Test func unknownAndTinyWidthsKeepFixedHeightAndCells() {
        for width in [CGFloat(0), -1, .infinity, -.infinity, .nan, .greatestFiniteMagnitude] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, maximumWeeks: nil)
            #expect(geometry.weeks == 0 && geometry.width == 0)
            #expect(geometry.square == .mac && geometry.height == 82)
        }
        for width in [CGFloat(0.25), 1, 5, 10] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, maximumWeeks: nil)
            #expect(geometry.weeks == 1 && geometry.side == 10 && geometry.width == 10)
            #expect(geometry.height == 82)
        }
    }

    @Test func labelsUseFixedPitchAcrossMoreThanAYearInBothDirections() {
        for width in [CGFloat(634), 646, 1_000, 2_000] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, maximumWeeks: nil)
            let grid = ActivityGrid(history: [:], weeks: geometry.weeks, todayWeekday: 3, firstWeekday: 1)
            let labels = grid.monthLabels(year: 2026, month: 11, day: 24,
                                          pitch: geometry.pitch, width: geometry.width) { _ in 18 }
            let mirrored = grid.monthLabels(year: 2026, month: 11, day: 24,
                                            pitch: geometry.pitch, width: geometry.width,
                                            rightToLeft: true) { _ in 18 }
            #expect(labels.count == mirrored.count)
            if width >= 1_000 { #expect(labels.count > 12) }
            for (label, mirror) in zip(labels, mirrored) {
                #expect(label.column == mirror.column && label.month == mirror.month)
                #expect(label.x >= 0 && label.x + 18 <= geometry.width)
                #expect(abs(mirror.x - (geometry.width - label.x - 18)) < 1e-9)
            }
            for (left, right) in zip(labels, labels.dropFirst()) { #expect(left.x + 18 <= right.x) }
        }
    }

    @Test func existingCallersKeepTheBoundedWindow() {
        for square in [ActivityGrid.Square.mac, .touch] {
            let geometry = ActivityGrid.Geometry(width: 2_000, square: square)
            #expect(geometry.weeks == 53 && geometry.square == square)
        }
        #expect(ActivityGrid(counts: [1], weeks: 99, todayWeekday: 3, firstWeekday: 1).weeks == 53)
    }
}
