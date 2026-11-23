import CoreGraphics
import Foundation
import HeatmapKit
import Testing

@testable import Stats

/// The narrow page's layout rules (plan U304; `stats/statistics-page` S6, S7.4; contract N25.6):
/// the strip's four tiles and how they wrap, the small figures two to a row, where the switches
/// become menus, and the column with the small boxes two abreast on the Duo held tall. Every rule
/// is decided by the width, the count and the text size, never by the values.
@MainActor
@Suite struct StatsNarrowLayoutTests {
    /// A phone's column: 16 points of inset on each side of 375 or 393 points; a small one; the
    /// box's content inside its 16-point inset.
    private nonisolated static let phoneColumn: CGFloat = 343
    private nonisolated static let smallColumn: CGFloat = 288
    private nonisolated static let boxContent: CGFloat = 311

    @Test func theNarrowStripHasFourTilesWithoutMature() {
        #expect(StatsStripID.narrow == [.studiedToday, .retentionToday, .remainingToday, .streak])
        // Mature is a figure of Card Counts there (S6, S7.7), and its tile scrolls to Cards on the Mac.
        #expect(!StatsStripID.narrow.contains(.mature))
    }

    /// The tile's minimum at the default text size and as `@ScaledMetric` scales it for larger
    /// sizes (about 1.35 at xxxLarge, 2.4 at the largest accessibility size).
    @Test(arguments: [
        (phoneColumn, 1.0, 4),
        (smallColumn, 1.0, 2),
        (phoneColumn, 1.35, 2),
        (phoneColumn, 2.4, 1),
        (637.0, 1.35, 4),
    ])
    func theTilesSitFourInARowThenTwoByTwoThenOneByOne(width: CGFloat, scale: CGFloat, columns: Int) {
        #expect(StatsNarrowLayout.stripColumns(width: width, tileMinimum: StatsNarrowLayout.tileMinimum * scale) == columns)
    }

    @Test(arguments: [
        (4, boxContent, 2),
        (3, boxContent, 3),
        (2, boxContent, 2),
        (1, boxContent, 1),
        (3, 260.0, 2),
        (4, 400.0, 4),
    ])
    func theFiguresSitInOneRowWhereTheyFitElseTwoToARow(count: Int, width: CGFloat, columns: Int) {
        #expect(StatsNarrowLayout.figureColumns(count: count, width: width, figureMinimum: StatsNarrowLayout.figureMinimum) == columns)
    }

    @Test func aRowIsWholeOrSplitInPairs() {
        // Four that do not fit are never three and one.
        for width in stride(from: CGFloat(150), through: 450, by: 5) {
            let columns = StatsNarrowLayout.columns(count: 4, width: width, minimum: 88, spacing: 16)
            #expect([1, 2, 4].contains(columns), "\(width) gave \(columns)")
        }
        #expect(StatsNarrowLayout.columns(count: 0, width: 300, minimum: 88, spacing: 16) == 1)
    }

    @Test func theSwitchesAreSegmentedWhileTheyFitThenMenus() {
        #expect(StatsNarrowLayout.switchStyles(accessibilitySize: false) == [.segmented, .menu])
        // Stacked under the title at the accessibility sizes, a segmented control of large text
        // would still run past the box: only the menu.
        #expect(StatsNarrowLayout.switchStyles(accessibilitySize: true) == [.menu])
    }

    @Test func countAndTimeMoveIntoTheBoxsMenuOnlyOnTheNarrowPage() {
        #expect(StatsNarrowLayout.measureInMenu(narrow: true))
        #expect(!StatsNarrowLayout.measureInMenu(narrow: false))
    }

    // MARK: Activity's recent lines (the coordinator's decision of 2026-11-23)

    @Test func activityShowsItsRecentLinesOnlyOnTheNarrowPageUntilShowAllYears() {
        #expect(StatsNarrowLayout.activityRecentWeeks(narrow: true, showsAllYears: false) == 53)
        #expect(StatsNarrowLayout.activityRecentWeeks(narrow: true, showsAllYears: true) == nil)
        #expect(StatsNarrowLayout.activityRecentWeeks(narrow: false, showsAllYears: false) == nil, "The Mac and the iPad show all")
    }

    @Test func theKeysStopAtTheOldestDayShown() {
        // Six years to Monday 2026-11-23 at 16 columns a line: the lines shown start 399 days ago.
        let grid = ActivityContinuousGrid(year: 2026, month: 11, day: 23, days: 2_192, firstWeekday: 2, counts: [])
        #expect(StatsActivityBox.oldestShownDay(grid, perLine: 16) == -399)
        #expect(StatsActivityBox.moved(-399, key: .left, first: max(-2_191, -399), last: 0) == -399, "Left stops at the first line")
        #expect(StatsActivityBox.moved(-395, key: .up, first: -399, last: 0) == -396)
        // A year's history shows every day.
        let year = ActivityContinuousGrid(year: 2026, month: 11, day: 23, days: 365, firstWeekday: 2, counts: [])
        #expect(StatsActivityBox.oldestShownDay(year, perLine: 16) == -364)
    }

    @Test func theDeviceKeepsShowAllYears() throws {
        let suite = "StatsNarrowLayoutTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = StatsSelectionStore(defaults: defaults)
        #expect(!store.loadActivityAllYears(), "The first time: the recent lines")
        store.saveActivityAllYears(true)
        #expect(StatsSelectionStore(defaults: defaults).loadActivityAllYears())
        #expect(!StatsSelectionStore(defaults: nil).loadActivityAllYears(), "A test-hook launch keeps nothing")
    }

    @Test func theDuoHeldTallPutsTheSmallBoxesTwoAbreast() {
        // The Duo held tall: 669 points less the column's insets. A phone or the folded Duo: one.
        #expect(StatsBoxGrid.columns(for: 669 - 32) == 2)
        #expect(StatsBoxGrid.columns(for: Self.phoneColumn) == 1)
        #expect(StatsBoxGrid.columns(for: 430 - 32) == 1)
    }
}
