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
    /// Counts the keyboard's moves, so a narrow grid scrolls the newly selected day into view.
    @State private var revealRequest = 0

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
            // Narrow: the figures under the grid, and each year scrolls sideways to its newest
            // week; always at the accessibility text sizes, where figures stack under the plot (S15).
            let narrow = VStack(alignment: .leading, spacing: Spacing.medium) {
                grid(activity, blocks: blocks, phase: content.phase, scrolls: true)
                figures(content, activity: activity, column: dynamicTypeSize.isAccessibilitySize)
            }
            if dynamicTypeSize.isAccessibilitySize {
                narrow
            } else {
                ViewThatFits(in: .horizontal) {
                    // Wide: the figures trailing the grid (N25.8).
                    HStack(alignment: .top, spacing: Spacing.large) {
                        grid(activity, blocks: blocks, phase: content.phase, scrolls: false)
                        figures(content, activity: activity, column: true)
                            .frame(minWidth: 180, alignment: .leading)
                    }
                    narrow
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
                               revealRequest: revealRequest, selection: slot.selection, showInBrowse: showInBrowse)
            .focusable()
            .focused($focused)
            .onKeyPress(.leftArrow) { move(layoutDirection == .rightToLeft ? 1 : -1, blocks: blocks, activity: activity) }
            .onKeyPress(.rightArrow) { move(layoutDirection == .rightToLeft ? -1 : 1, blocks: blocks, activity: activity) }
            .onKeyPress(.escape) { clear() }
            .onKeyPress(.return) { browseSelected(blocks, activity: activity) }
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
            StatsActivityPlaceholder(square: Self.square, phase: phase, fitsWidth: scrolls, years: placeholderYears)
        }
    }

    /// The year blocks a box not ready yet keeps room for: the header's count when the model
    /// knows it (S10.3), else one.
    private var placeholderYears: Int {
        if case let .activityYears(years)? = slot.content.detail { return years }
        return 1
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

    private func move(_ step: Int, blocks: [Block], activity: StatsActivity) -> KeyPress.Result {
        guard let first = blocks.last?.year.firstDay, let last = blocks.first?.year.lastDay else { return .ignored }
        let selection = slot.selection
        let next = Self.moved(selection.wrappedValue.bin.map(activity.day(ofAbsoluteDay:)), by: step, first: first, last: last)
        selection.wrappedValue = StatsChartSelection(bin: activity.absoluteDay(ofDay: next), pinned: true)
        revealRequest += 1
        return .handled
    }

    /// The scroll offset that centres a column in a viewport, kept inside the block.
    static func revealOffset(column: Int, grid: ActivityYearGrid, square: ActivityGrid.Square, viewport: CGFloat,
                             rightToLeft: Bool) -> CGFloat {
        let pitch = square.side + square.gap
        let width = grid.width(square)
        let x = rightToLeft ? width - CGFloat(column) * pitch - square.side : CGFloat(column) * pitch
        return min(max(0, x + square.side / 2 - viewport / 2), max(0, width - viewport))
    }

    /// The day a key moves the selection to: one day on, kept between the first block's
    /// 1 January and today; with none selected, today.
    static func moved(_ day: Int?, by step: Int, first: Int, last: Int) -> Int {
        min(max(day.map { $0 + step } ?? last, first), last)
    }

    private func clear() -> KeyPress.Result {
        let selection = slot.selection
        guard selection.wrappedValue != .none else { return .ignored }
        selection.wrappedValue = .none
        return .handled
    }

    private func browseSelected(_ blocks: [Block], activity: StatsActivity) -> KeyPress.Result {
        guard let day = slot.selection.wrappedValue.bin.map(activity.day(ofAbsoluteDay:)), let showInBrowse,
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
    /// Changes when the keyboard moves the selection: a scrolled block shows the day.
    let revealRequest: Int
    /// The pinned or hovered day as an absolute day (`StatsActivity.absoluteDay`), so a
    /// rollover never moves it to another date.
    @Binding var selection: StatsChartSelection
    let showInBrowse: ((Int) -> Void)?

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.layoutDirection) private var layoutDirection
    @State private var positions: [Int: ScrollPosition] = [:]
    @State private var viewport: CGFloat = 0

    init(blocks: [StatsActivityBox.Block], activity: StatsActivity, square: ActivityGrid.Square, scrolls: Bool,
         revealRequest: Int, selection: Binding<StatsChartSelection>, showInBrowse: ((Int) -> Void)?) {
        self.blocks = blocks
        self.activity = activity
        self.square = square
        self.scrolls = scrolls
        self.revealRequest = revealRequest
        _selection = selection
        self.showInBrowse = showInBrowse
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            ForEach(blocks) { block in
                let selected = selectedDay.flatMap { day in
                    (block.year.firstDay...max(block.year.firstDay, block.year.lastDay)).contains(day) ? day - block.year.firstDay : nil
                }
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    Text(verbatim: String(block.year.year))
                        .font(.subheadline.weight(.semibold))
                        .accessibilityHidden(true)
                    // The squares are drawn mirrored by hand under right-to-left layout (a canvas is
                    // not mirrored), so the grid, its overlays and its scroll view lay out left to
                    // right and every position is mirrored the same way.
                    Group {
                        if scrolls {
                            ScrollView(.horizontal) { grid(block, selected: selected) }
                                .scrollPosition(position(block.year.year))
                                .defaultScrollAnchor(UnitPoint(x: newestFraction(block.grid), y: 0))
                                .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { viewport = $0 }
                        } else {
                            grid(block, selected: selected)
                        }
                    }
                    .environment(\.layoutDirection, .leftToRight)
                }
                // The block holding the callout is drawn over the blocks after it.
                .zIndex(selected == nil ? 0 : 1)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(block.year.totalText)
                .accessibilityIdentifier("stats.activity.block.\(block.year.year)")
            }
        }
        .fixedSize(horizontal: !scrolls, vertical: true)
        .onChange(of: revealRequest) { reveal() }
    }

    /// The selected day (H1), from the stored absolute day.
    private var selectedDay: Int? { selection.bin.map(activity.day(ofAbsoluteDay:)) }

    /// Selects a day (H1), or none.
    private func select(_ day: Int?, pinned: Bool) {
        selection = day.map { StatsChartSelection(bin: activity.absoluteDay(ofDay: $0), pinned: pinned) } ?? .none
    }

    private func position(_ year: Int) -> Binding<ScrollPosition> {
        Binding(get: { positions[year] ?? ScrollPosition() }, set: { positions[year] = $0 })
    }

    /// Scrolls the block holding the selected day so the day is in view (S14's keys in a narrow grid).
    private func reveal() {
        guard scrolls, viewport > 0, let day = selectedDay,
              let block = blocks.first(where: { ($0.year.firstDay...max($0.year.firstDay, $0.year.lastDay)).contains(day) })
        else { return }
        var moved = ScrollPosition()
        moved.scrollTo(x: StatsActivityBox.revealOffset(column: block.grid.cell(ofDay: day - block.year.firstDay).column, grid: block.grid,
                                            square: square, viewport: viewport, rightToLeft: rightToLeft))
        positions[block.year.year] = moved
    }


    /// The month labels and the squares of a block, with the selection over them.
    private func grid(_ block: StatsActivityBox.Block, selected: Int?) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            StatsActivityMonthLabels(grid: block.grid, square: square, rightToLeft: rightToLeft)
            StatsActivitySquares(grid: block.grid, firstDay: block.year.firstDay, square: square, rightToLeft: rightToLeft,
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
                    if hovered != selectedDay { select(hovered, pinned: false) }
                case .ended:
                    if selection.bin != nil { selection = .none }
                }
            }
            .gesture(SpatialTapGesture(count: 2, coordinateSpace: .local).onEnded { value in
                guard let found = day(value.location) else { return }
                select(found, pinned: true)
                if (block.year.counts[found] ?? 0) > 0 { showInBrowse?(found) }
            })
            .simultaneousGesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                select(day(value.location), pinned: true)
            })
            #else
            .onTapGesture(coordinateSpace: .local) { location in
                let found = day(location)
                select(found == selectedDay && selection.pinned ? nil : found, pinned: true)
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
                .alignmentGuide(.top) { dimensions in
                    // Level with the day, kept inside the block's height, so a scrolled block
                    // never clips it and it never runs onto the next box.
                    -min(max(0, y - Spacing.xSmall), max(0, ActivityYearGrid.height(square) - dimensions.height))
                }
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
    /// The app's layout direction: the canvas is laid out left to right and mirrors by hand.
    let rightToLeft: Bool
    let labels: StatsActivityDayLabels

    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.colorScheme) private var colorScheme

    nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.grid == rhs.grid && lhs.firstDay == rhs.firstDay && lhs.square == rhs.square
            && lhs.rightToLeft == rhs.rightToLeft && lhs.labels == rhs.labels
    }

    var body: some View {
        let increased = contrast == .increased
        let dark = colorScheme == .dark
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

    /// The day's long date, formatted only when VoiceOver reads it.
    func date(_ day: Int) -> Text {
        Text(days.date(ofDay: day), format: Date.FormatStyle(date: .complete, time: .omitted, locale: locale, calendar: calendar,
                                                            timeZone: calendar.timeZone))
    }

    /// A Gregorian month, as the blocks group the days, whatever the display calendar.
    func month(_ day: Int) -> String {
        var gregorian = Calendar(identifier: .gregorian)
        gregorian.locale = locale
        gregorian.timeZone = calendar.timeZone
        return days.date(ofDay: day).formatted(Date.FormatStyle(locale: locale, calendar: gregorian, timeZone: calendar.timeZone)
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
    let rightToLeft: Bool
    @Environment(\.locale) private var locale

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
                    #if os(macOS)
                    let resolved = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(.secondary)) }
                    #else
                    // As Today's: the system grey nearly misses 4.5:1 on the grouped background.
                    let resolved = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(.token(.textSecondary))) }
                    #endif
                    let labels = grid.monthLabels(pitch: square.side + square.gap, width: size.width,
                                                  rightToLeft: rightToLeft) {
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
    /// Year blocks to keep room for (S10.3: the box at its final height).
    let years: Int

    var body: some View {
        // One block's room per year, as the blocks stack (StatsActivityYears).
        VStack(alignment: .leading, spacing: Spacing.medium) {
            ForEach(0..<max(1, years), id: \.self) { _ in
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    Text(verbatim: "2026").font(.subheadline.weight(.semibold)).hidden()
                    Text(verbatim: "Nov").font(.caption).hidden()
                    Color.clear.frame(height: ActivityYearGrid.height(square))
                }
            }
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

extension StatsActivity {
    /// A day (H1) as a number of days since 1970-01-01 by its calendar date: a selection kept
    /// this way names the same date after a rollover, when every H1 day moves by one.
    public func absoluteDay(ofDay day: Int) -> Int { day + dayZeroAbsolute }

    /// The day (H1) of an absolute day.
    public func day(ofAbsoluteDay day: Int) -> Int { day - dayZeroAbsolute }

    private var dayZeroAbsolute: Int { Int((days.dayZero.timeIntervalSince1970 / 86_400).rounded(.down)) }
}
