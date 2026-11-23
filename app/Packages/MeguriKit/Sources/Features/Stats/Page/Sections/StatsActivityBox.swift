import AppModel
import DesignSystem
import HeatmapKit
import SwiftUI

/// Activity (`stats/statistics-page` S7.5, contract N25.8): Today's heatmap extended to every
/// year (HeatmapKit's `ActivityYearGrid`: N24.6's fixed squares, the system green and AH7's
/// levels per year block), one block per year under its year label, the current year first;
/// beside it (under it when narrow) Current Streak, Longest Streak, Days Studied This Year and
/// each year's total. The period never changes it. Hover (Mac) or a tap (touch) shows a day's
/// date and count; a click pins it, Esc clears it, Left and Right move a day (S8.3, S14).
/// The model writes every value (`StatsBoxDetail.activity`); this view only lays them out.
struct StatsActivityBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for a day with reviews (S8.4: "Cards Reviewed on <date>", H13's
    /// `answered`), given the day (H1); U303 passes its `StatsBrowseLink` here. nil offers none.
    let showInBrowse: ((Int) -> Void)?

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.layoutDirection) private var layoutDirection
    @FocusState private var focused: Bool

    /// N24.6's fixed square, as on Today: it never grows with the width or the text size.
    #if os(macOS)
    static let square = ActivityGrid.Square.mac
    #else
    static let square = ActivityGrid.Square.touch
    #endif

    private var text: StatsText { StatsText(locale: locale) }

    var body: some View {
        let content = slot.content
        let activity: StatsActivity? = if content.phase == .ready, case let .activity(value)? = content.detail { value } else { nil }
        let blocks = activity.map { Self.blocks($0, firstWeekday: calendar.firstWeekday) } ?? []
        StatsBoxChrome(box: slot.id.rawValue, title: slot.title(locale: locale), open: slot.open) {
            ViewThatFits(in: .horizontal) {
                // Wide: the figures trailing the grid (N25.8).
                HStack(alignment: .top, spacing: Spacing.large) {
                    grid(activity, blocks: blocks, phase: content.phase, scrolls: false)
                    figures(content, activity: activity, column: true)
                        .frame(minWidth: 180, alignment: .leading)
                }
                // Narrow: the figures under it, and the years scroll sideways to today.
                VStack(alignment: .leading, spacing: Spacing.medium) {
                    grid(activity, blocks: blocks, phase: content.phase, scrolls: true)
                    figures(content, activity: activity, column: dynamicTypeSize.isAccessibilitySize)
                }
            }
        }
    }

    /// One block per year, the current year first (S7.5).
    struct Block: Identifiable {
        var id: Int { year.year }
        let year: StatsActivity.Year
        let grid: ActivityYearGrid
    }

    static func blocks(_ activity: StatsActivity, firstWeekday: Int) -> [Block] {
        activity.years.map { year in
            let length = max(0, year.lastDay - year.firstDay + 1)
            var counts = [UInt32](repeating: 0, count: length)
            for (day, count) in year.counts where day >= year.firstDay && day <= year.lastDay {
                counts[day - year.firstDay] = UInt32(clamping: count)
            }
            // The current year's block ends today (day 0); an earlier year's at 31 December.
            let today = year.lastDay == 0 ? year.lastDay - year.firstDay : nil
            return Block(year: year, grid: ActivityYearGrid(year: year.year, firstWeekday: firstWeekday, today: today, counts: counts))
        }
    }

    // MARK: The grid

    @ViewBuilder
    private func grid(_ activity: StatsActivity?, blocks: [Block], phase: StatsBoxPhase, scrolls: Bool) -> some View {
        if let activity, !blocks.isEmpty {
            // A width narrower than a year (a narrow window, the iPad's compact width) scrolls each
            // block sideways under its year label, starting at its newest drawn week.
            StatsActivityYears(blocks: blocks, activity: activity, square: Self.square, scrolls: scrolls,
                               selection: slot.selection, showInBrowse: showInBrowse)
            .focusable()
            .focused($focused)
            .onKeyPress(.leftArrow) { move(layoutDirection == .rightToLeft ? 1 : -1, blocks: blocks) }
            .onKeyPress(.rightArrow) { move(layoutDirection == .rightToLeft ? -1 : 1, blocks: blocks) }
            .onKeyPress(.escape) { clear() }
            .onKeyPress(.return) { browseSelected(blocks) }
            #if os(macOS)
            .onChange(of: focused) {
                // A click elsewhere that takes the focus unpins (S8.3).
                let selection = slot.selection
                if StatsChartSelection.unpinsOnFocusLoss(focused: focused, pinned: selection.wrappedValue.pinned, ready: true) {
                    selection.wrappedValue = .none
                }
            }
            #endif
            .accessibilityElement(children: .contain)
            .accessibilityLabel(slot.title(locale: locale))
            .accessibilityValue(summary(activity))
            .accessibilityIdentifier("stats.box.activity.plot")
        } else {
            StatsActivityPlaceholder(square: Self.square, phase: phase, fitsWidth: scrolls)
        }
    }

    /// The grid's value: the streaks and the year totals, as the figures beside it say.
    private func summary(_ activity: StatsActivity) -> String {
        let format = StatsChartFormat(locale: locale, calendar: calendar, now: activity.date(ofDay: 0))
        let streaks = [
            "\(text.figureCurrentStreak), \(text.days(activity.currentStreak, format.count(Double(activity.currentStreak))))",
            "\(text.figureLongestStreak), \(text.days(activity.longestStreak, format.count(Double(activity.longestStreak))))",
        ]
        return (streaks + activity.years.map(\.totalText)).joined(separator: "; ")
    }

    // MARK: The figures

    /// The small figures (labels in every phase, values once ready), then one line per year.
    @ViewBuilder
    private func figures(_ content: StatsBoxContent, activity: StatsActivity?, column: Bool) -> some View {
        let ready = content.phase == .ready
        let layout = column
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.small))
            : AnyLayout(HStackLayout(alignment: .top, spacing: Spacing.medium))
        VStack(alignment: .leading, spacing: Spacing.medium) {
            layout {
                ForEach(content.figures) { item in
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        Text(verbatim: item.label)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(verbatim: ready ? item.value ?? "—" : "—")
                            .font(.subheadline.weight(.medium))
                            .monospacedDigit()
                            .lineLimit(1)
                            .foregroundStyle(ready && item.value != nil ? .primary : .secondary)
                    }
                    .frame(maxWidth: column ? nil : .infinity, alignment: .leading)
                    .accessibilityElement(children: .combine)
                }
            }
            if let activity {
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    ForEach(activity.years) { year in
                        Text(verbatim: year.totalText)
                            .font(.subheadline)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("stats.activity.yearTotal.\(year.year)")
                    }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.activity.figures")
    }

    // MARK: Keys (S14)

    private func move(_ step: Int, blocks: [Block]) -> KeyPress.Result {
        guard let first = blocks.last?.year.firstDay, let last = blocks.first?.year.lastDay else { return .ignored }
        let selection = slot.selection
        let next = selection.wrappedValue.bin.map { $0 + step } ?? last
        selection.wrappedValue = StatsChartSelection(bin: min(max(next, first), last), pinned: true)
        return .handled
    }

    private func clear() -> KeyPress.Result {
        let selection = slot.selection
        guard selection.wrappedValue != .none else { return .ignored }
        selection.wrappedValue = .none
        return .handled
    }

    private func browseSelected(_ blocks: [Block]) -> KeyPress.Result {
        guard let day = slot.selection.wrappedValue.bin, let showInBrowse,
              blocks.contains(where: { ($0.year.counts[day] ?? 0) > 0 }) else { return .ignored }
        showInBrowse(day)
        return .handled
    }
}

/// The year blocks, one under another, with the selection's outline and callout over the block
/// that holds it.
private struct StatsActivityYears: View {
    let blocks: [StatsActivityBox.Block]
    let activity: StatsActivity
    let square: ActivityGrid.Square
    /// Each block scrolls sideways (narrower than a year).
    let scrolls: Bool
    @Binding var selection: StatsChartSelection
    let showInBrowse: ((Int) -> Void)?

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.layoutDirection) private var layoutDirection

    init(blocks: [StatsActivityBox.Block], activity: StatsActivity, square: ActivityGrid.Square, scrolls: Bool,
         selection: Binding<StatsChartSelection>, showInBrowse: ((Int) -> Void)?) {
        self.blocks = blocks
        self.activity = activity
        self.square = square
        self.scrolls = scrolls
        _selection = selection
        self.showInBrowse = showInBrowse
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            ForEach(blocks) { block in
                let selected = selection.bin.flatMap { day in
                    (block.year.firstDay...max(block.year.firstDay, block.year.lastDay)).contains(day) ? day - block.year.firstDay : nil
                }
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    Text(verbatim: String(block.year.year))
                        .font(.subheadline.weight(.semibold))
                        .accessibilityHidden(true)
                    if scrolls {
                        ScrollView(.horizontal) { grid(block, selected: selected) }
                            .defaultScrollAnchor(UnitPoint(x: newestFraction(block.grid), y: 0))
                    } else {
                        grid(block, selected: selected)
                    }
                }
                // The block holding the callout is drawn over the blocks after it.
                .zIndex(selected == nil ? 0 : 1)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(block.year.totalText)
                .accessibilityIdentifier("stats.activity.block.\(block.year.year)")
            }
        }
        .fixedSize(horizontal: !scrolls, vertical: true)
    }

    /// The month labels and the squares of a block, with the selection over them.
    private func grid(_ block: StatsActivityBox.Block, selected: Int?) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            StatsActivityMonthLabels(grid: block.grid, square: square)
            StatsActivitySquares(grid: block.grid, firstDay: block.year.firstDay, square: square,
                                 labels: StatsActivityDayLabels(days: activity.days, locale: locale, calendar: calendar))
                .equatable()
                .overlay(alignment: .topLeading) {
                    interaction(block)
                }
                .overlay(alignment: .topLeading) {
                    if let selected { selectionMarks(block, day: selected) }
                }
        }
    }

    /// Where a block's newest drawn week sits across it (today's in the current year), so a
    /// scrolled block starts there.
    private func newestFraction(_ grid: ActivityYearGrid) -> CGFloat {
        guard grid.weeks > 1 else { return 1 }
        let column = CGFloat(grid.cell(ofDay: grid.drawnDays - 1).column) / CGFloat(grid.weeks - 1)
        return rightToLeft ? 1 - column : column
    }

    private var rightToLeft: Bool { layoutDirection == .rightToLeft }

    /// The pointer and the finger over a block (S8.3): hover shows a day, a click or tap pins
    /// it, a click on no day clears; on the Mac a double-click runs Show in Browse (S8.4).
    private func interaction(_ block: StatsActivityBox.Block) -> some View {
        let grid = block.grid
        func day(_ point: CGPoint) -> Int? {
            grid.day(at: point, square: square, rightToLeft: rightToLeft).map { block.year.firstDay + $0 }
        }
        return Rectangle()
            .fill(.clear)
            .contentShape(Rectangle())
            .frame(width: grid.width(square), height: ActivityYearGrid.height(square))
            #if os(macOS)
            .onContinuousHover(coordinateSpace: .local) { phase in
                guard !selection.pinned else { return }
                switch phase {
                case let .active(location):
                    let hovered = day(location)
                    if hovered != selection.bin { selection.bin = hovered }
                case .ended:
                    if selection.bin != nil { selection.bin = nil }
                }
            }
            .gesture(SpatialTapGesture(count: 2, coordinateSpace: .local).onEnded { value in
                guard let found = day(value.location) else { return }
                selection = StatsChartSelection(bin: found, pinned: true)
                if (block.year.counts[found] ?? 0) > 0 { showInBrowse?(found) }
            })
            .simultaneousGesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                selection = day(value.location).map { StatsChartSelection(bin: $0, pinned: true) } ?? .none
            })
            #else
            .onTapGesture(coordinateSpace: .local) { location in
                let found = day(location)
                selection = found == nil || (found == selection.bin && selection.pinned) ? .none : StatsChartSelection(bin: found, pinned: true)
            }
            #endif
            .accessibilityHidden(true)
    }

    /// The callout's Show in Browse: a pinned day with reviews, when the caller offers it (S8.4).
    private func browseAction(count: Int, day: Int) -> (() -> Void)? {
        guard count > 0, selection.pinned, let showInBrowse else { return nil }
        return { showInBrowse(day) }
    }

    /// The selected day's outline and its callout beside it, never over it (N25.4).
    @ViewBuilder
    private func selectionMarks(_ block: StatsActivityBox.Block, day: Int) -> some View {
        let grid = block.grid
        if grid.day(column: grid.cell(ofDay: day).column, row: grid.cell(ofDay: day).row) != nil {
            let cell = grid.cell(ofDay: day)
            let pitch = square.side + square.gap
            let width = grid.width(square)
            let x = rightToLeft ? width - CGFloat(cell.column) * pitch - square.side : CGFloat(cell.column) * pitch
            let y = CGFloat(cell.row) * pitch
            let count = block.year.counts[block.year.firstDay + day] ?? 0
            let dayIndex = block.year.firstDay + day
            let date = activity.date(ofDay: dayIndex)
            let format = StatsChartFormat(locale: locale, calendar: calendar, now: activity.date(ofDay: 0))
            let text = StatsText(locale: locale)
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: square.cornerRadius + 1)
                    .strokeBorder(.primary, lineWidth: 2)
                    .frame(width: square.side + 4, height: square.side + 4)
                    .offset(x: x - 2, y: y - 2)
                    .allowsHitTesting(false)
                StatsChartCallout(
                    box: "activity",
                    title: format.name(.day(date)),
                    rows: [(name: text.reviewsSeries, value: format.count(Double(count)), color: nil)],
                    showInBrowse: browseAction(count: count, day: dayIndex)
                )
                .fixedSize()
                .alignmentGuide(.leading) { dimensions in
                    // Beside the day, to its right in the block's first half and to its left in the second.
                    let leading = x > width / 2
                    let origin = leading ? x - Spacing.xSmall - dimensions.width : x + square.side + Spacing.xSmall
                    return -origin
                }
                .alignmentGuide(.top) { _ in -max(0, y - Spacing.xSmall) }
                .allowsHitTesting(selection.pinned)
            }
            .accessibilityHidden(true)
        }
    }
}

/// A block's squares, drawn in one canvas (hundreds of days per year), with one accessibility
/// element per day inside a container per month (S15, `stats/heatmap-presentation` M15.4).
/// Equatable, so a hover over another block never draws this one again.
private struct StatsActivitySquares: View, Equatable {
    let grid: ActivityYearGrid
    /// The block's first day (H1), for the days' dates.
    let firstDay: Int
    let square: ActivityGrid.Square
    let labels: StatsActivityDayLabels

    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.layoutDirection) private var layoutDirection

    nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.grid == rhs.grid && lhs.firstDay == rhs.firstDay && lhs.square == rhs.square && lhs.labels == rhs.labels
    }

    var body: some View {
        let increased = contrast == .increased
        let dark = colorScheme == .dark
        let rightToLeft = layoutDirection == .rightToLeft
        let width = grid.width(square)
        Canvas { context, _ in
            let pitch = square.side + square.gap
            for day in 0..<grid.drawnDays {
                let cell = grid.cell(ofDay: day)
                let x = rightToLeft ? width - CGFloat(cell.column) * pitch - square.side : CGFloat(cell.column) * pitch
                let rect = CGRect(x: x, y: CGFloat(cell.row) * pitch, width: square.side, height: square.side)
                let path = Path(roundedRect: rect, cornerRadius: square.cornerRadius)
                let level = grid.level(day: day)
                if let opacity = ActivityGrid.opacity(level: level, increasedContrast: increased, dark: dark) {
                    // N24.6: with Increase Contrast the steps lie over the empty day's own fill.
                    if increased { context.fill(path, with: .style(.fill.secondary)) }
                    context.fill(path, with: .color(.green.opacity(opacity)))
                } else {
                    context.fill(path, with: .style(.fill.secondary))
                }
                if day == grid.today {
                    // Today keeps its 1 pt outline in the primary label colour, as on Today.
                    let outline = Path(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), cornerRadius: max(0, square.cornerRadius - 0.5))
                    context.stroke(outline, with: .style(.primary), lineWidth: 1)
                }
            }
        }
        .frame(width: width, height: ActivityYearGrid.height(square))
        .accessibilityElement(children: .contain)
        .accessibilityChildren { dayElements(width: width, rightToLeft: rightToLeft) }
    }

    /// One element per drawn day, at its square, grouped by month.
    private func dayElements(width: CGFloat, rightToLeft: Bool) -> some View {
        let pitch = square.side + square.gap
        return ZStack(alignment: .topLeading) {
            ForEach(grid.months) { month in
                ZStack(alignment: .topLeading) {
                    ForEach(month.days, id: \.self) { day in
                        let cell = grid.cell(ofDay: day)
                        let x = rightToLeft ? width - CGFloat(cell.column) * pitch - square.side : CGFloat(cell.column) * pitch
                        Color.clear
                            .frame(width: square.side, height: square.side)
                            .offset(x: x, y: CGFloat(cell.row) * pitch)
                            .accessibilityElement()
                            .accessibilityLabel(labels.date(firstDay + day))
                            .accessibilityValue(labels.value(Int(grid.count(day: day)), today: day == grid.today))
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityLabel(labels.month(firstDay + month.days.lowerBound))
            }
        }
        .frame(width: width, height: ActivityYearGrid.height(square), alignment: .topLeading)
    }
}

/// The texts of a day's accessibility element: its long date, and its count ("Today" first on
/// today), and a month container's name (S15).
private nonisolated struct StatsActivityDayLabels: Equatable {
    let days: StatsDays
    let locale: Locale
    let calendar: Calendar

    func date(_ day: Int) -> String {
        days.date(ofDay: day).formatted(Date.FormatStyle(date: .complete, time: .omitted, locale: locale, calendar: calendar,
                                                             timeZone: calendar.timeZone))
    }

    func month(_ day: Int) -> String {
        days.date(ofDay: day).formatted(Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
            .month(.wide).year())
    }

    func value(_ count: Int, today: Bool) -> String {
        let reviews = String(localized: "stats.value.reviews \(count) \(count.formatted(.number.locale(locale)))",
                             bundle: .module, locale: locale)
        return today ? String(localized: "stats.activity.today \(reviews)", bundle: .module, locale: locale) : reviews
    }
}

/// AH9's month labels over a block, in caption secondary text; drawn, not laid out, as on Today.
/// The row keeps a caption line's height whatever is drawn.
private struct StatsActivityMonthLabels: View {
    let grid: ActivityYearGrid
    let square: ActivityGrid.Square
    @Environment(\.locale) private var locale
    @Environment(\.layoutDirection) private var layoutDirection

    var body: some View {
        Text(verbatim: "Nov")
            .font(.caption)
            .hidden()
            .frame(width: grid.width(square), alignment: .leading)
            .overlay(alignment: .leading) {
                Canvas { context, size in
                    var calendar = Calendar(identifier: .gregorian)
                    calendar.locale = locale
                    let names = calendar.shortStandaloneMonthSymbols
                    guard names.count == 12 else { return }
                    let resolved = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(.secondary)) }
                    let labels = grid.monthLabels(pitch: square.side + square.gap, width: size.width,
                                                  rightToLeft: layoutDirection == .rightToLeft) {
                        resolved[$0 - 1].measure(in: CGSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude)).width
                    }
                    for label in labels {
                        context.draw(resolved[label.month - 1], at: CGPoint(x: label.x, y: 0), anchor: .topLeading)
                    }
                }
            }
            .accessibilityHidden(true)
    }
}

/// The grid's place while its part is not ready (S8.5, S10.3): one block's size, with the
/// small progress indicator or the line that says why in its middle.
private struct StatsActivityPlaceholder: View {
    let square: ActivityGrid.Square
    let phase: StatsBoxPhase
    /// Narrower than a year: the box's width; else a year's, so the figures beside it keep
    /// their place when the grid arrives.
    let fitsWidth: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: "2026").font(.subheadline.weight(.semibold)).hidden()
            Text(verbatim: "Nov").font(.caption).hidden()
            Color.clear.frame(height: ActivityYearGrid.height(square))
        }
        .frame(width: fitsWidth ? nil : ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: []).width(square),
               alignment: .leading)
        .frame(maxWidth: fitsWidth ? .infinity : nil, alignment: .leading)
        .overlay { message }
        .accessibilityIdentifier("stats.box.activity.plot")
    }

    @ViewBuilder private var message: some View {
        switch phase {
        case let .loading(fraction):
            Group {
                if let fraction {
                    ProgressView(value: min(max(fraction, 0), 1)).frame(maxWidth: 160)
                } else {
                    ProgressView()
                }
            }
            .controlSize(.small)
        case let .empty(line, detail):
            VStack(spacing: Spacing.xxSmall) {
                Text(verbatim: line).font(.subheadline)
                if let detail { Text(verbatim: detail).font(.footnote) }
            }
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .accessibilityElement(children: .combine)
        case .failed:
            Text("stats.chart.failed", bundle: .module)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        case .ready:
            EmptyView()
        }
    }
}
