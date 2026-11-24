import CoreGraphics
import Testing
@testable import HeatmapKit

/// Mac Home's width-filling geometry (N14.11, N24.6); other callers keep fixed cells.
@Suite struct ActivityHomeGeometryTests {
    @Test(arguments: [CGFloat(22), 23.5, 321.25, 633, 634, 635, 1_000, 2_000, 10_000])
    func fillsTheRowWithSquareCellsAndFixedGaps(width: CGFloat) {
        let baseline = ActivityGrid.Geometry(width: width, square: .mac)
        let geometry = ActivityGrid.Geometry(width: width, square: .mac, fillingWidth: true)
        #expect(geometry.weeks == baseline.weeks)
        #expect((1...53).contains(geometry.weeks))
        #expect(abs(geometry.width - width) < 1e-9)
        #expect(geometry.gap == 2)
        #expect(geometry.pitch == geometry.side + 2)
        #expect(geometry.height == 7 * geometry.side + 12)
        #expect(geometry.square.cornerRadius == geometry.side / 5)
        // The last square/placeholder ends at the row's edge; rows use the same side.
        #expect(abs(CGFloat(geometry.weeks - 1) * geometry.pitch + geometry.side - width) < 1e-9)
    }

    @Test func resizingUsesBaselineWeekBoundariesAndKeepsFillingAfterTheCap() {
        let widths: [CGFloat] = [633, 634, 635, 2_000, 2_000.5, 635, 633]
        let geometries = widths.map { ActivityGrid.Geometry(width: $0, square: .mac, fillingWidth: true) }
        #expect(geometries.map(\.weeks) == [52, 53, 53, 53, 53, 53, 52])
        for (width, geometry) in zip(widths, geometries) {
            #expect(abs(geometry.width - width) < 1e-9)
        }
        #expect(geometries[4].side > geometries[3].side)
        #expect(geometries[4].height > geometries[3].height)
        #expect(geometries.first == geometries.last)
    }

    @Test func unmeasuredWidthsReserveBaselineHeightAndTinyWidthsKeepTheCurrentWeek() {
        for width in [CGFloat(0), -1, .infinity, -.infinity, .nan] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, fillingWidth: true)
            #expect(geometry.weeks == 0 && geometry.width == 0)
            #expect(geometry.square == .mac && geometry.height == 82)
        }
        for width in [CGFloat(0.25), 1, 5, 10] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, fillingWidth: true)
            #expect(geometry.weeks == 1 && geometry.side == width && geometry.width == width)
            #expect(geometry.height.isFinite && geometry.height > 0)
        }
    }

    @Test func labelsUseTheResizedPitchAndStayInsideTheRowInBothDirections() {
        let grid = ActivityGrid(counts: [], weeks: 53, todayWeekday: 3, firstWeekday: 1)
        for width in [CGFloat(634), 635.5, 2_000] {
            let geometry = ActivityGrid.Geometry(width: width, square: .mac, fillingWidth: true)
            let labels = grid.monthLabels(year: 2026, month: 11, day: 24,
                                          pitch: geometry.pitch, width: geometry.width) { _ in 18 }
            let mirrored = grid.monthLabels(year: 2026, month: 11, day: 24,
                                            pitch: geometry.pitch, width: geometry.width,
                                            rightToLeft: true) { _ in 18 }
            #expect(labels.count == 12)
            for (label, mirror) in zip(labels, mirrored) {
                #expect(abs(label.x - CGFloat(label.column) * geometry.pitch) < 1e-9)
                #expect(label.x >= 0 && label.x + 18 <= geometry.width)
                #expect(abs(mirror.x - (geometry.width - label.x - 18)) < 1e-9)
            }
            for (left, right) in zip(labels, labels.dropFirst()) {
                #expect(left.x + 18 <= right.x)
            }
        }
    }

    @Test func fillingIsOptInForExistingTouchAndSharedCallers() {
        for width in [CGFloat(322), 634, 2_000] {
            for square in [ActivityGrid.Square.mac, .touch] {
                let geometry = ActivityGrid.Geometry(width: width, square: square)
                #expect(geometry.square == square)
                #expect(geometry.height == 7 * square.side + 6 * square.gap)
            }
        }
    }
}
