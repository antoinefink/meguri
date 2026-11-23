import AppModel
import DesignSystem
import HeatmapKit
import SwiftUI

/// Activity (`stats/statistics-page` S7.5, contract N25.8, the owner's Option E): one grid for
/// all of history (HeatmapKit's `ActivityContinuousGrid`: N24.6's fixed squares, the system
/// green and AH7's levels), a column per week, wrapped onto lines like text, oldest at the top
/// left and today at the bottom right; never scrolled sideways. Under it, in a row that wraps:
/// Current Streak, Longest Streak, Days Studied This Year, All Reviews, then each year's total.
/// The period never changes it. Hover (Mac) or a tap (touch) shows a day's date and count; a
/// click pins it; Esc clears it; Left and Right move a week, Up and Down a day (S8.3, S14).
/// The model writes every value (`StatsBoxDetail.activity`); this view only lays them out.
struct StatsActivityBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for a day with reviews (S8.4: "Cards Reviewed on <date>", H13's
    /// `answered`), given the day (H1); U303 passes its `StatsBrowseLink` here. nil offers none.
    let showInBrowse: ((Int) -> Void)?

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.layoutDirection) private var layoutDirection
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
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
        let span: StatsActivitySpan? = if let activity { activity.span }
            else if case let .activitySpan(value)? = content.detail { value } else { nil }
        StatsBoxChrome(box: slot.id.rawValue, title: slot.title(locale: locale), open: slot.open) {
            // The figures under the grid, so the lines keep the box's whole width (N25.8).
            VStack(alignment: .leading, spacing: Spacing.medium) {
                grid(activity, span: span, phase: content.phase)
                figures(content, activity: activity, span: span)
            }
        }
    }

    // MARK: The grid

    @ViewBuilder
    private func grid(_ activity: StatsActivity?, span: StatsActivitySpan?, phase: StatsBoxPhase) -> some View {
        if let activity {
            StatsActivityGrid(activity: activity, square: Self.square, selection: slot.selection, showInBrowse: showInBrowse,
                              pinned: { focused = true })
            #if os(macOS)
            // As the charts' plots: Tab reaches it with keyboard navigation on and a click focuses
            // it from its tap handler; click-to-focus would take the click that pins (S8.3).
            .focusable(interactions: .activate)
            #else
            .focusable()
            #endif
            .focused($focused)
            .onKeyPress(.leftArrow) { move(.left, activity: activity) }
            .onKeyPress(.rightArrow) { move(.right, activity: activity) }
            .onKeyPress(.upArrow) { move(.up, activity: activity) }
            .onKeyPress(.downArrow) { move(.down, activity: activity) }
            .onKeyPress(.escape) { clear() }
            .onKeyPress(.return) { browseSelected(activity) }
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
            // The grid's final size for the width while its part is not back or has failed
            // (S10.3, S8.5); one line when no review is known.
            StatsActivityPlaceholder(columns: span?.grid().columns ?? 1, square: Self.square, phase: phase)
        }
    }

    /// The grid's value: the streaks, all reviews and the year totals, as the figures under it say.
    private func summary(_ activity: StatsActivity) -> String {
        let format = StatsChartFormat(locale: locale, calendar: calendar, now: activity.date(ofDay: 0))
        let figures = [
            "\(text.figureCurrentStreak), \(text.days(activity.currentStreak, format.count(Double(activity.currentStreak))))",
            "\(text.figureLongestStreak), \(text.days(activity.longestStreak, format.count(Double(activity.longestStreak))))",
            "\(text.figureDaysStudiedThisYear), \(format.count(Double(activity.daysStudiedThisYear)))",
            "\(text.figureAllReviews), \(format.count(Double(activity.total)))",
        ]
        return (figures + activity.years.reversed().map(\.totalText)).joined(separator: "; ")
    }

    // MARK: The figures

    /// The small figures (labels in every phase, values once ready), then each year's total,
    /// oldest first as the grid reads; each a row that wraps.
    private func figures(_ content: StatsBoxContent, activity: StatsActivity?, span: StatsActivitySpan?) -> some View {
        let ready = content.phase == .ready
        // While loading, each year the header knows with a dash, as wide as a six-figure total, so
        // the row keeps its lines when the counts arrive (S10.3).
        let reserve = 888_888
        let reserveText = reserve.formatted(.number.locale(locale))
        let years: [(year: Int, text: String, reserve: String?)] = activity.map { $0.years.map { ($0.year, $0.totalText, nil) } }
            ?? (span?.years ?? []).map { ($0, text.yearTotal(String($0), 0, text.noValue), text.yearTotal(String($0), reserve, reserveText)) }
        // At the accessibility sizes, one under another (S15), as the other boxes' figures.
        let stacked = dynamicTypeSize.isAccessibilitySize
        let row = stacked ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
            : AnyLayout(StatsWrappingRow(spacing: Spacing.large, lineSpacing: Spacing.small))
        return VStack(alignment: .leading, spacing: Spacing.small) {
            row {
                ForEach(content.figures) { item in
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        Text(verbatim: item.label)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(verbatim: ready ? item.value ?? text.noValue : text.noValue)
                            .font(.subheadline.weight(.medium))
                            .monospacedDigit()
                            .lineLimit(1)
                            .foregroundStyle(ready && item.value != nil ? .primary : .secondary)
                    }
                    .accessibilityElement(children: .combine)
                }
            }
            if !years.isEmpty {
                StatsWrappingRow(spacing: Spacing.medium, lineSpacing: Spacing.xxSmall) {
                    ForEach(years.reversed(), id: \.year) { year in
                        ZStack(alignment: .leading) {
                            if let reserve = year.reserve { Text(verbatim: reserve).hidden().accessibilityHidden(true) }
                            Text(verbatim: year.text)
                        }
                        .font(.subheadline)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("stats.activity.yearTotal.\(year.year)")
                    }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.activity.figures")
    }

    // MARK: Keys (S14)

    /// A key's move, as the grid is drawn: Left and Right go to the square beside (a week),
    /// Up and Down to the square above or below (a day); a line's end continues on the next
    /// line, as the weeks do.
    enum Key { case left, right, up, down }

    private func move(_ key: Key, activity: StatsActivity) -> KeyPress.Result {
        let selection = slot.selection
        let next = Self.moved(selection.wrappedValue.bin.map(activity.day(ofAbsoluteDay:)), key: key, first: activity.span.firstDay,
                              last: 0, rightToLeft: layoutDirection == .rightToLeft)
        selection.wrappedValue = StatsChartSelection(bin: activity.absoluteDay(ofDay: next), pinned: true)
        return .handled
    }

    /// The day a key moves the selection to, kept between the first day and today; with none
    /// selected, today. Under right-to-left layout the weeks run from the right, so Left and
    /// Right swap.
    static func moved(_ day: Int?, key: Key, first: Int, last: Int, rightToLeft: Bool = false) -> Int {
        guard let day else { return last }
        let step = switch key {
        case .left: rightToLeft ? 7 : -7
        case .right: rightToLeft ? -7 : 7
        case .up: -1
        case .down: 1
        }
        return min(max(day + step, first), last)
    }

    /// Where the callout goes, never over the day (N25.4): beside it, on the side with room
    /// (its right in the grid's first half, its left in the second, when both have room), level
    /// with it; when neither side has room, under the day, or over it at the grid's bottom edge. Always
    /// inside the grid, so it never runs onto the figures.
    nonisolated static func calloutOrigin(day: CGRect, callout width: CGFloat, _ height: CGFloat, in grid: CGSize) -> CGPoint {
        let gap = Spacing.xSmall
        let left = day.minX - gap - width, right = day.maxX + gap
        let fitsLeft = left >= 0, fitsRight = right + width <= grid.width
        let preferLeft = day.midX > grid.width / 2
        func clampedY(_ y: CGFloat) -> CGFloat { min(max(0, y), max(0, grid.height - height)) }
        if fitsLeft && (preferLeft || !fitsRight) { return CGPoint(x: left, y: clampedY(day.minY - gap)) }
        if fitsRight { return CGPoint(x: right, y: clampedY(day.minY - gap)) }
        let x = min(max(0, day.midX - width / 2), max(0, grid.width - width))
        let below = day.maxY + gap
        let y = below + height <= grid.height ? below : max(0, day.minY - gap - height)
        return CGPoint(x: x, y: y)
    }

    private func clear() -> KeyPress.Result {
        let selection = slot.selection
        guard selection.wrappedValue != .none else { return .ignored }
        selection.wrappedValue = .none
        return .handled
    }

    private func browseSelected(_ activity: StatsActivity) -> KeyPress.Result {
        guard let day = slot.selection.wrappedValue.bin.map(activity.day(ofAbsoluteDay:)), let showInBrowse,
              (activity.counts[day] ?? 0) > 0 else { return .ignored }
        showInBrowse(day)
        return .handled
    }
}

// MARK: - Layout

/// The space between a line's labels and its squares, and between one line's squares and the
/// next line's labels: a little more than between two rows, so lines read as lines.
private nonisolated enum StatsActivityMetrics {
    static let labelGap = Spacing.xxSmall
    static let lineGap = Spacing.xSmall

    static func geometry(size: CGSize, columns: Int, square: ActivityGrid.Square) -> ActivityContinuousGrid.Geometry {
        ActivityContinuousGrid.Geometry(size: size, columns: columns, square: square, labelGap: labelGap, lineGap: lineGap)
    }
}

/// Sizes the grid from the width it is offered: as many week columns as fit on a line, as many
/// lines as the weeks need, each with a caption line for its labels. The first subview is a
/// hidden caption line, measured for the band; the others get the grid's size. A width change
/// re-runs only this, in the same layout pass, so the line breaks follow the window with no
/// frame drawn at a stale height.
private struct StatsActivityLayout: Layout {
    let columns: Int
    let square: ActivityGrid.Square

    private func geometry(width: CGFloat, subviews: Subviews) -> ActivityContinuousGrid.Geometry {
        let band = subviews.first?.sizeThatFits(.unspecified).height ?? 0
        return ActivityContinuousGrid.Geometry(width: width, columns: columns, square: square, band: band,
                                               labelGap: StatsActivityMetrics.labelGap, lineGap: StatsActivityMetrics.lineGap)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        // With no width offered, a year's weeks (Today's 53).
        let pitch = square.side + square.gap
        let ideal = CGFloat(min(columns, ActivityGrid.maximumWeeks)) * pitch - square.gap
        // No width, an infinite one or a zero probe: a year's weeks, never a column a line.
        let width = proposal.width.flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? ideal
        return CGSize(width: width, height: geometry(width: width, subviews: subviews).height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let height = geometry(width: bounds.width, subviews: subviews).height
        for (index, subview) in subviews.enumerated() {
            subview.place(at: bounds.origin, anchor: .topLeading,
                          proposal: index == 0 ? .unspecified : ProposedViewSize(width: bounds.width, height: height))
        }
    }
}

/// A caption line, never drawn: the label band's height at the current text size, measured
/// with the locale's own month names and a year, so a script with taller glyphs gets its room.
private struct StatsActivityBandProbe: View {
    @Environment(\.locale) private var locale

    var body: some View {
        var calendar = Calendar(identifier: .gregorian)
        calendar.locale = locale
        return Text(verbatim: (calendar.shortStandaloneMonthSymbols + ["2026"]).joined(separator: " "))
            .font(.caption.weight(.semibold))
            .lineLimit(1)
            .hidden()
            .accessibilityHidden(true)
    }
}

// MARK: - The grid

/// The grid with the selection's outline and callout over it, and the pointer's and the
/// finger's handling. Drawn left to right: under right-to-left layout every position is
/// mirrored by hand, as the canvas is.
private struct StatsActivityGrid: View {
    let activity: StatsActivity
    let square: ActivityGrid.Square
    /// The pinned or hovered day as an absolute day (`StatsActivity.absoluteDay`), so a
    /// rollover never moves it to another date.
    @Binding var selection: StatsChartSelection
    let showInBrowse: ((Int) -> Void)?
    /// A click or tap pinned a day: the box takes the focus, so the keys move it.
    let pinned: () -> Void

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.layoutDirection) private var layoutDirection
    /// The drawn size, for hit-testing and the overlays; the layout never reads it.
    @State private var size: CGSize = .zero

    init(activity: StatsActivity, square: ActivityGrid.Square, selection: Binding<StatsChartSelection>,
         showInBrowse: ((Int) -> Void)?, pinned: @escaping () -> Void) {
        self.activity = activity
        self.square = square
        _selection = selection
        self.showInBrowse = showInBrowse
        self.pinned = pinned
    }

    private var grid: ActivityContinuousGrid { activity.grid }
    private var rightToLeft: Bool { layoutDirection == .rightToLeft }
    private var geometry: ActivityContinuousGrid.Geometry {
        StatsActivityMetrics.geometry(size: size, columns: grid.columns, square: square)
    }

    var body: some View {
        let rightToLeft = rightToLeft
        StatsActivityLayout(columns: grid.columns, square: square) {
            StatsActivityBandProbe()
            StatsActivityCanvas(grid: grid, square: square, rightToLeft: rightToLeft, size: size,
                                labels: StatsActivityDayLabels(days: activity.days, locale: locale, calendar: calendar))
                .equatable()
                .onGeometryChange(for: CGSize.self) { $0.size } action: { size = $0 }
                .overlay(alignment: .topLeading) { interaction }
                .overlay(alignment: .topLeading) {
                    if let day = selectedDay, day >= activity.span.firstDay, day <= 0, size.width > 0 { selectionMarks(day) }
                }
        }
        .environment(\.layoutDirection, .leftToRight)
    }

    /// The selected day (H1), from the stored absolute day.
    private var selectedDay: Int? { selection.bin.map(activity.day(ofAbsoluteDay:)) }

    /// Selects a day (H1), or none.
    private func select(_ day: Int?, pinned: Bool) {
        selection = day.map { StatsChartSelection(bin: activity.absoluteDay(ofDay: $0), pinned: pinned) } ?? .none
    }

    /// The day (H1) under a point of the grid.
    private func day(at point: CGPoint) -> Int? {
        grid.daysAgo(at: point, geometry: geometry, rightToLeft: rightToLeft).map { -$0 }
    }

    /// The pointer and the finger (S8.3): hover shows a day, a click or tap pins it, a click on
    /// no day clears; on the Mac a double-click runs Show in Browse (S8.4). Hit-testing is by
    /// the grid's geometry, with no view per day.
    private var interaction: some View {
        Rectangle()
            .fill(.clear)
            .contentShape(Rectangle())
            #if os(macOS)
            .onContinuousHover(coordinateSpace: .local) { phase in
                guard !selection.pinned else { return }
                switch phase {
                case let .active(location):
                    let hovered = day(at: location)
                    if hovered != selectedDay { select(hovered, pinned: false) }
                case .ended:
                    if selection.bin != nil { selection = .none }
                }
            }
            .gesture(SpatialTapGesture(count: 2, coordinateSpace: .local).onEnded { value in
                guard let found = day(at: value.location) else { return }
                select(found, pinned: true)
                pinned()
                if (activity.counts[found] ?? 0) > 0 { showInBrowse?(found) }
            })
            .simultaneousGesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                let found = day(at: value.location)
                select(found, pinned: true)
                if found != nil { pinned() }
            })
            #else
            .onTapGesture(coordinateSpace: .local) { location in
                let found = day(at: location)
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

    /// The selected day's outline and its callout beside it, never over it (N25.4), kept
    /// inside the grid's height.
    private func selectionMarks(_ day: Int) -> some View {
        let geometry = geometry
        let rect = grid.rect(daysAgo: -day, geometry: geometry, rightToLeft: rightToLeft)
        let count = activity.counts[day] ?? 0
        let format = StatsChartFormat(locale: locale, calendar: calendar, now: activity.date(ofDay: 0))
        let text = StatsText(locale: locale)
        let height = size.height
        let width = size.width
        return ZStack(alignment: .topLeading) {
            RoundedRectangle(cornerRadius: square.cornerRadius + 1)
                .strokeBorder(.primary, lineWidth: 2)
                .frame(width: square.side + 4, height: square.side + 4)
                .offset(x: rect.minX - 2, y: rect.minY - 2)
                .allowsHitTesting(false)
            StatsChartCallout(
                box: "activity",
                title: format.name(.day(activity.date(ofDay: day))),
                rows: [(name: text.reviewsSeries, value: format.count(Double(count)), color: nil)],
                showInBrowse: browseAction(count: count, day: day)
            )
            // Its own text in the app's direction; the grid around it is laid out left to right.
            .environment(\.layoutDirection, layoutDirection)
            .fixedSize()
            .alignmentGuide(.leading) { dimensions in
                -StatsActivityBox.calloutOrigin(day: rect, callout: dimensions.width, dimensions.height, in: CGSize(width: width, height: height)).x
            }
            .alignmentGuide(.top) { dimensions in
                -StatsActivityBox.calloutOrigin(day: rect, callout: dimensions.width, dimensions.height, in: CGSize(width: width, height: height)).y
            }
            .allowsHitTesting(selection.pinned)
        }
        .accessibilityHidden(true)
    }
}

/// The squares, today's outline, the month and year labels and the year rules, in one canvas
/// for all of history; one accessibility element per day inside a container per month (S15,
/// `stats/heatmap-presentation` M15.4). Equatable, so a hover, a pin or the page's scrolling
/// never draws it again: only new counts, a new size, the direction or the appearance do.
private struct StatsActivityCanvas: View, Equatable {
    let grid: ActivityContinuousGrid
    let square: ActivityGrid.Square
    /// The app's layout direction: the canvas is laid out left to right and mirrors by hand.
    let rightToLeft: Bool
    /// The drawn size, for the days' elements (the canvas reads its own).
    let size: CGSize
    let labels: StatsActivityDayLabels

    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.locale) private var locale

    /// The canvas draws itself again at a new size on its own; the body (the days' elements)
    /// is built again only when their places change: a line break, the height, or under
    /// right-to-left layout the width. A live resize between two line breaks rebuilds nothing.
    nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.grid == rhs.grid && lhs.square == rhs.square && lhs.rightToLeft == rhs.rightToLeft && lhs.labels == rhs.labels
            && lhs.placesKey == rhs.placesKey
    }

    private nonisolated var placesKey: [CGFloat] {
        [CGFloat(ActivityContinuousGrid.Geometry.perLine(width: size.width, square: square)), size.height,
         rightToLeft ? size.width : 0]
    }

    var body: some View {
        let increased = contrast == .increased
        let dark = colorScheme == .dark
        Canvas { context, size in
            let geometry = StatsActivityMetrics.geometry(size: size, columns: grid.columns, square: square)
            drawSquares(&context, geometry: geometry, increased: increased, dark: dark)
            drawLabels(&context, geometry: geometry)
        }
        .accessibilityElement(children: .contain)
        .accessibilityChildren { dayElements }
    }

    /// The squares, one path per level, then today's 1 pt outline in the label colour, as on Today.
    private func drawSquares(_ context: inout GraphicsContext, geometry: ActivityContinuousGrid.Geometry, increased: Bool,
                             dark: Bool) {
        var paths = [Path](repeating: Path(), count: 5)
        let corner = CGSize(width: square.cornerRadius, height: square.cornerRadius)
        for daysAgo in 0..<grid.days {
            let rect = grid.rect(daysAgo: daysAgo, geometry: geometry, rightToLeft: rightToLeft)
            paths[min(4, max(0, grid.level(daysAgo: daysAgo)))].addRoundedRect(in: rect, cornerSize: corner)
        }
        context.fill(paths[0], with: .style(.fill.secondary))
        for level in 1...4 {
            guard let opacity = ActivityGrid.opacity(level: level, increasedContrast: increased, dark: dark) else { continue }
            // N24.6: with Increase Contrast the steps lie over the empty day's own fill.
            if increased { context.fill(paths[level], with: .style(.fill.secondary)) }
            context.fill(paths[level], with: .color(.green.opacity(opacity)))
        }
        let today = grid.rect(daysAgo: 0, geometry: geometry, rightToLeft: rightToLeft)
        let outline = Path(roundedRect: today.insetBy(dx: 0.5, dy: 0.5), cornerRadius: max(0, square.cornerRadius - 0.5))
        context.stroke(outline, with: .style(.primary), lineWidth: 1)
    }

    /// AH9's month labels in caption secondary text, each year in caption semibold at its
    /// 1 January after a thin rule, over each line (`ActivityContinuousGrid.labels`).
    private func drawLabels(_ context: inout GraphicsContext, geometry: ActivityContinuousGrid.Geometry) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.locale = locale
        let names = calendar.shortStandaloneMonthSymbols
        guard names.count == 12 else { return }
        #if os(macOS)
        let secondary = AnyShapeStyle(.secondary)
        #else
        // As Today's: the system grey nearly misses 4.5:1 on the grouped background.
        let secondary = AnyShapeStyle(.token(.textSecondary))
        #endif
        let months = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(secondary)) }
        var years: [Int: GraphicsContext.ResolvedText] = [:]
        for month in grid.months where years[month.year] == nil {
            years[month.year] = context.resolve(Text(verbatim: String(month.year)).font(.caption.weight(.semibold))
                .foregroundStyle(.primary))
        }
        let unbounded = CGSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
        let labels = grid.labels(geometry, rightToLeft: rightToLeft,
                                 monthWidth: { months[$0 - 1].measure(in: unbounded).width },
                                 yearWidth: { years[$0]?.measure(in: unbounded).width ?? 0 })
        for label in labels.items {
            let top = geometry.lineTop(label.line)
            switch label.kind {
            case let .month(month): context.draw(months[month - 1], at: CGPoint(x: label.x, y: top), anchor: .topLeading)
            case let .year(year):
                if let resolved = years[year] { context.draw(resolved, at: CGPoint(x: label.x, y: top), anchor: .topLeading) }
            }
            if let rule = label.rule {
                // From the band's top down to the squares, at the column holding 1 January.
                context.fill(Path(CGRect(x: rule, y: top, width: 1, height: geometry.band + geometry.labelGap)),
                             with: .style(secondary))
            }
        }
    }

    /// One element per drawn day, at its square, inside a container per month. Built with the
    /// canvas, so only when the counts or the size change; each date is formatted when read.
    private var dayElements: some View {
        let geometry = StatsActivityMetrics.geometry(size: size, columns: grid.columns, square: square)
        return ZStack(alignment: .topLeading) {
            ForEach(grid.months) { month in
                ZStack(alignment: .topLeading) {
                    ForEach(month.daysAgo.reversed(), id: \.self) { daysAgo in
                        let rect = grid.rect(daysAgo: daysAgo, geometry: geometry, rightToLeft: rightToLeft)
                        Color.clear
                            .frame(width: square.side, height: square.side)
                            .offset(x: rect.minX, y: rect.minY)
                            .accessibilityElement()
                            .accessibilityLabel(labels.date(-daysAgo))
                            .accessibilityValue(labels.value(Int(grid.count(daysAgo: daysAgo)), today: daysAgo == 0))
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityLabel(labels.month(-(month.daysAgo.upperBound - 1)))
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
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

    /// A Gregorian month, as the grid groups the days, whatever the display calendar.
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

/// The grid's place while its part is not ready or has failed (S8.5, S10.3): the grid's own
/// size for the width, with the small progress indicator or the line that says why in its middle.
private struct StatsActivityPlaceholder: View {
    let columns: Int
    let square: ActivityGrid.Square
    let phase: StatsBoxPhase

    var body: some View {
        StatsActivityLayout(columns: columns, square: square) {
            StatsActivityBandProbe()
            Color.clear
        }
        .overlay { message }
        .accessibilityElement(children: .contain)
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

/// Items side by side at their own widths, leading-aligned, wrapping onto a new line when the
/// next does not fit, like words (the figures under the grid).
struct StatsWrappingRow: Layout {
    let spacing: CGFloat
    let lineSpacing: CGFloat

    /// Each item's frame for a width, and the height used.
    private func frames(width: CGFloat, subviews: Subviews) -> (frames: [CGRect], size: CGSize) {
        var frames: [CGRect] = []
        var x: CGFloat = 0, y: CGFloat = 0, lineHeight: CGFloat = 0, used: CGFloat = 0
        for subview in subviews {
            var size = subview.sizeThatFits(.unspecified)
            if size.width > width { size = subview.sizeThatFits(ProposedViewSize(width: width, height: nil)) }
            if x > 0, x + size.width > width {
                y += lineHeight + lineSpacing
                x = 0
                lineHeight = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            used = max(used, x + size.width)
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
        return (frames, CGSize(width: used, height: frames.isEmpty ? 0 : y + lineHeight))
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? .greatestFiniteMagnitude
        let size = frames(width: width, subviews: subviews).size
        return CGSize(width: proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? size.width, height: size.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let placed = frames(width: bounds.width, subviews: subviews).frames
        for (subview, frame) in zip(subviews, placed) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          proposal: ProposedViewSize(frame.size))
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
