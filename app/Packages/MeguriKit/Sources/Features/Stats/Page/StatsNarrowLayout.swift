import DesignSystem
import SwiftUI

/// N25.6's narrow page: iPhone, the folded Duo, the Duo held tall and a narrow iPad window (the
/// shell's tab layout). The same sections in one column on the grouped background, each box a
/// section-like group; the strip as four tiles; figures two to a row where a row of them does
/// not fit; the boxes' switches as menus where a segmented control does not fit.
///
/// Every rule here is decided from the width, the count and the text size, never from the values,
/// so a box keeps its size when its numbers arrive (S8.5, S10.2).
nonisolated enum StatsNarrowLayout {
    /// How many cells of at least `minimum` points fit in a row of `width` with `spacing` between
    /// them: all of them, else two to a row, else one. Never three of four: a row of figures or
    /// tiles is whole or split in pairs, so no row ends with a lone cell beside an empty one when
    /// the count is even.
    static func columns(count: Int, width: CGFloat, minimum: CGFloat, spacing: CGFloat) -> Int {
        guard count > 1 else { return max(count, 1) }
        func fits(_ columns: Int) -> Bool {
            CGFloat(columns) * minimum + CGFloat(columns - 1) * spacing <= width
        }
        if fits(count) { return count }
        if count > 2, fits(2) { return 2 }
        return 1
    }

    /// The strip's tiles per row (S6: four tiles, Mature moves to Card Counts): four in a row on
    /// a phone, two by two where four would be narrower than a tile's minimum (a small width, a
    /// large text size), one per row at the accessibility sizes where two would be.
    static func stripColumns(width: CGFloat, tileMinimum: CGFloat) -> Int {
        columns(count: StatsStripID.narrow.count, width: width, minimum: tileMinimum, spacing: Spacing.xSmall)
    }

    /// A box's small figures per row (N25.4 part 5 at N25.6's width): two to four in a row where
    /// they fit at their minimum width, else two to a row. At the accessibility sizes they stack
    /// (S15), which the box decides before asking.
    static func figureColumns(count: Int, width: CGFloat, figureMinimum: CGFloat) -> Int {
        columns(count: count, width: width, minimum: figureMinimum, spacing: Spacing.medium)
    }

    /// How a box's switches are drawn in its header, in the order the header tries them: a small
    /// segmented control while it fits beside the title, else a menu showing the chosen option
    /// (N25.6). At the accessibility sizes the header stacks the title over its switches and a
    /// segmented control of large text would still run past the box, so only the menu is tried.
    static func switchStyles(accessibilitySize: Bool) -> [StatsSwitchStyle] {
        accessibilitySize ? [.menu] : [.segmented, .menu]
    }

    /// Where Reviews Over Time's two switches go (N25.6): By Type | By Answer stays in the header,
    /// Count | Time moves into the box's menu.
    static func measureInMenu(narrow: Bool) -> Bool { narrow }

    /// The base minimum widths, before Dynamic Type scales them (`@ScaledMetric`).
    static let tileMinimum: CGFloat = 72
    static let figureMinimum: CGFloat = 88
}

/// How a box's switch is drawn (N25.4, N25.6).
nonisolated enum StatsSwitchStyle: Hashable, Sendable {
    case segmented
    case menu
}

extension StatsStripID {
    /// The narrow page's strip (S6): Mature is a figure of Card Counts there.
    nonisolated static let narrow: [StatsStripID] = allCases.filter { $0 != .mature }
}

extension EnvironmentValues {
    /// The narrow page (N25.6), set by `StatisticsView` from the shell's layout; nil (outside the
    /// page, the chart gallery) follows the horizontal size class.
    @Entry var statsNarrow: Bool? = nil
    /// The style a box's switches take in its header (`StatsNarrowLayout.switchStyles`).
    @Entry var statsSwitchStyle: StatsSwitchStyle = .segmented
    /// Runs a change that grows or shrinks a box above the place the reader is looking at (Show
    /// All Years, Show Less), then scrolls the page so the view with the given id, the control
    /// that made it, is back at the height it was on the screen (the tap's place in the scroll
    /// view, read when the scroll happens): what is under the change, the line with today and
    /// the control, stays where it was. Outside the page it just runs the change.
    @Entry var statsKeepInPlace: StatsKeepInPlace = StatsKeepInPlace { _, _, change in change() }
}

/// See `EnvironmentValues.statsKeepInPlace`.
struct StatsKeepInPlace: Sendable {
    typealias Run = @MainActor @Sendable (_ id: String, _ onScreenY: @escaping @MainActor () -> CGFloat?,
                                         _ change: @escaping @MainActor () -> Void) -> Void
    let run: Run

    init(_ run: @escaping Run) {
        self.run = run
    }

    @MainActor func callAsFunction(id: String, onScreenY: @escaping @MainActor () -> CGFloat?,
                                   _ change: @escaping @MainActor () -> Void) {
        run(id, onScreenY, change)
    }
}

/// The page's visible height and the last tap's height in it: plain references, not observed, so
/// setting them redraws nothing (a resize, a tap).
final class StatsViewport {
    var height: CGFloat = 0
}

final class StatsTapPlace {
    var y: CGFloat?
}

nonisolated extension StatsNarrowLayout {
    /// The narrow page's Activity grid shows the lines holding the last 53 weeks (a year's
    /// weeks, as Today's grid) until Show All Years (the coordinator's decision of 2026-11-23,
    /// pending the owner's look: six years at phone width are about 17 lines).
    static let activityRecentWeeks = 53

    /// The weeks the Activity grid shows, nil for all: on the narrow page its recent lines unless
    /// the device keeps Show All Years; elsewhere (the Mac, the iPad at regular width) all.
    static func activityRecentWeeks(narrow: Bool, showsAllYears: Bool) -> Int? {
        narrow && !showsAllYears ? activityRecentWeeks : nil
    }
}

/// Cells in equal columns, `StatsNarrowLayout.columns` of them for the proposed width, each row as
/// tall as its tallest cell and every cell of a row given that height (the strip's tiles line
/// up). The column count comes from the width alone, in the same layout pass: no state, no
/// second pass, nothing that changes while the page scrolls.
struct StatsColumnsLayout: Layout {
    let minimum: CGFloat
    let spacing: CGFloat
    let rowSpacing: CGFloat

    private func arrange(width: CGFloat, subviews: Subviews) -> (frames: [CGRect], height: CGFloat) {
        let columns = StatsNarrowLayout.columns(count: subviews.count, width: width, minimum: minimum, spacing: spacing)
        let cell = max(0, (width - spacing * CGFloat(columns - 1)) / CGFloat(columns))
        var frames: [CGRect] = []
        var y: CGFloat = 0
        for start in stride(from: 0, to: subviews.count, by: columns) {
            if start > 0 { y += rowSpacing }
            let row = start..<min(start + columns, subviews.count)
            let height = row.map { subviews[$0].sizeThatFits(ProposedViewSize(width: cell, height: nil)).height }.max() ?? 0
            for (position, _) in row.enumerated() {
                frames.append(CGRect(x: CGFloat(position) * (cell + spacing), y: y, width: cell, height: height))
            }
            y += height
        }
        return (frames, y)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? 360
        return CGSize(width: width, height: arrange(width: width, subviews: subviews).height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (subview, frame) in zip(subviews, arrange(width: bounds.width, subviews: subviews).frames) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), anchor: .topLeading,
                          proposal: ProposedViewSize(width: frame.width, height: frame.height))
        }
    }
}
