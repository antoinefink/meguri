import AppModel
import Charts
import CoreBridge
import DesignSystem
import SwiftUI

/// The Cards section's boxes (plan U299; `stats/statistics-page` S7.6 to S7.8, S15, S16): Cards
/// Over Time, the lead, then Card Counts and Added and Started. The model writes every value
/// (`StatsPresenter`); these views lay them out and choose each chart element's label.
///
/// Show in Browse (S8.4) is a seam: until U303 lands no callout or row offers it (S20's last
/// line), so every box here gets nil. U303 passes its `StatsBrowseLink` for a Card Counts row
/// (Q4.1's text for the state) and an Added bar (the cards added in its days, Q9).
enum StatsCardsSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .cardCounts: StatsCardCountsBox(slot: slot, showInBrowse: nil)
        case .addedAndStarted: StatsAddedAndStartedBox(slot: slot, showInBrowse: nil)
        default: StatsCardsOverTimeBox(slot: slot)
        }
    }
}

/// S7.6: the cards in each state at each bar's end as stacked areas from zero (Mature, Young,
/// Relearning, Learning, New from the baseline), bands meeting with no line between them
/// unless Differentiate Without Colour parts them (N25.4); the figure, the mature count's
/// change, the four state figures with their changes and the suspended footnote. Its callout
/// names the date, the five states and the total. No Show in Browse: a past state cannot be
/// listed, today's states open from Card Counts.
struct StatsCardsOverTimeBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open,
            summary: StatsSectionText.summary(title, content.figure)
        )
    }
}

/// S7.8: cards added per bar (blue, New's colour) and cards first studied (an orange line,
/// Learning's colour) on one count axis; the figure with its started sentence, Added a Day and
/// Started a Day. A bar of Added offers Show in Browse once U303 passes its link.
struct StatsAddedAndStartedBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for an Added bar, given the bin; nil offers none.
    let showInBrowse: ((Int) -> Void)?
    @Environment(\.locale) private var locale
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        #if os(macOS)
        if slot.size == .regular {
            if dynamicTypeSize.isAccessibilitySize {
                overview(size: .regular)
            } else {
                ViewThatFits(in: .horizontal) {
                    overview(size: .overview)
                        .frame(minWidth: StatsNarrowLayout.figureMinimum * 4 + Spacing.medium * 5)
                    overview(size: .regular)
                }
            }
        } else {
            standard
        }
        #else
        standard
        #endif
    }

    private var standard: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        return StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open, showInBrowse: showInBrowse,
            summary: StatsSectionText.summary(title, content.figure)
        )
    }

    private func overview(size: StatsPlotSize) -> some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        return StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: size,
            summaryFigures: Self.summaryFigures(content, text: StatsText(locale: locale)),
            selection: slot.selection, open: slot.open, showInBrowse: showInBrowse,
            summary: StatsSectionText.summary(title, content.figure)
        )
    }

    /// Totals are formatted by the presenter, never recovered from the localized sentence.
    static func summaryFigures(_ content: StatsBoxContent, text: StatsText) -> [StatsSmallFigure] {
        let totals: (String?, String?)
        if case let .addedAndStarted(added, started)? = content.detail {
            totals = (added, started)
        } else {
            totals = (nil, nil)
        }
        return [StatsSmallFigure(label: text.added, value: totals.0),
                StatsSmallFigure(label: text.started, value: totals.1),
                StatsSmallFigure(label: text.figureAddedADay, value: content.figures.first?.value),
                StatsSmallFigure(label: text.figureStartedADay, value: content.figures.dropFirst().first?.value)]
    }
}

/// Today's seven card states as a full pie and an accessible table (S7.7). Zero states
/// keep their rows and take no sector. Pie and row hover/selection highlight each other;
/// the period does not affect these present counts. Loading keeps the names with dashes
/// and a grey pie with progress. The narrow page adds Mature, absent from its strip.
/// The macOS overview uses a width-based, capped pie and compact rows (2026-11-25).
/// Detail and touch retain the earlier layout, which can grow beside a taller neighbor.
struct StatsCardCountsBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for a state's row, given its label and Q4.1's search text; nil offers none.
    let showInBrowse: ((StatsCountLabel, String) -> Void)?
    @Environment(\.locale) private var locale
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(\.statsNarrow) private var statsNarrow
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var hoveredRow: Int?
    /// Whether the pie is larger than its own side, so the rows are spread beside it.
    @State private var grown = false

    /// The pie's side: the regular plot's height, at every text size (S15); a smaller one where
    /// that lets it stay beside the table; and the most it grows to beside a taller box.
    static let side = StatsPlotSize.regular.height
    nonisolated static let smallSide = StatsPlotSize.panel.height + Spacing.large
    static let largeSide = StatsPlotSize.page.height

    private var compactOverview: Bool {
        #if os(macOS)
        slot.size == .regular
        #else
        false
        #endif
    }

    var body: some View {
        let content = slot.content
        let ready = content.phase == .ready
        let title = slot.title(locale: locale)
        let table: StatsCardCountsTable? = if ready, case let .cardCounts(value)? = content.detail { value } else { nil }
        let selection = slot.selection
        let highlighted = selection.wrappedValue.bin ?? hoveredRow
        StatsBoxContainer {
            VStack(alignment: .leading, spacing: Spacing.small) {
                StatsBoxHeader(box: slot.id.rawValue, title: title, open: slot.open) { StatsBoxMarker.now }
                // No figure line (Antoine, 2026-11-23: "Remove the xxx cards in xxx notes"). The
                // pie beside the table where both fit the box's width, a smaller pie where that
                // fits, else the pie centred above the table: never past the box's inset.
                if compactOverview {
                    overview(table, phase: content.phase, highlighted: highlighted, title: title)
                        .contextMenu { if let table { exportMenu(table) } }
                } else {
                    legacyLayout(table, phase: content.phase, highlighted: highlighted, title: title)
                }
                // The narrow page (N25.6), the Duo held tall included, whatever its size class.
                if statsNarrow ?? (horizontalSizeClass == .compact) {
                    StatsBoxSmallFigures(figures: content.figures, ready: ready)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(slot.id.rawValue)")
    }

    private func overview(_ table: StatsCardCountsTable?, phase: StatsBoxPhase, highlighted: Int?, title: String) -> some View {
        ViewThatFits(in: .horizontal) {
            if !dynamicTypeSize.isAccessibilitySize {
                StatsOverviewPieBesideTable {
                    overviewPie(table, phase: phase, highlighted: highlighted, title: title)
                    rows(table, highlighted: highlighted)
                }
            }
            VStack(alignment: .leading, spacing: Spacing.medium) {
                overviewPie(table, phase: phase, highlighted: highlighted, title: title)
                    .frame(width: StatsOverviewPieBesideTable.limit, height: StatsOverviewPieBesideTable.limit)
                    .frame(maxWidth: .infinity)
                rows(table, highlighted: highlighted)
            }
        }
    }

    private func overviewPie(_ table: StatsCardCountsTable?, phase: StatsBoxPhase, highlighted: Int?, title: String) -> some View {
        StatsCardCountsPie(
            table: table, phase: phase, highlighted: highlighted,
            summary: StatsSectionText.summary(title, slot.content.figure) ?? title,
            selection: slot.selection, hovered: $hoveredRow
        )
    }

    private func legacyLayout(_ table: StatsCardCountsTable?, phase: StatsBoxPhase, highlighted: Int?, title: String) -> some View {
        ViewThatFits(in: .horizontal) {
            StatsPieBesideTable(side: Self.side, limit: Self.largeSide, spacing: Spacing.large) {
                pie(table, phase: phase, highlighted: highlighted, summary: title, side: Self.side, grows: true)
                rows(table, highlighted: highlighted)
            }
            StatsPieBesideTable(side: Self.smallSide, limit: Self.largeSide, spacing: Spacing.medium) {
                pie(table, phase: phase, highlighted: highlighted, summary: title, side: Self.smallSide, grows: true)
                rows(table, highlighted: highlighted)
            }
            VStack(alignment: .leading, spacing: Spacing.medium) {
                pie(table, phase: phase, highlighted: highlighted, summary: title, side: Self.side, grows: false)
                    .frame(maxWidth: .infinity)
                rows(table, highlighted: highlighted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        // Beside a taller box the row's height is more than the pie and table need: the
        // pair takes it (`StatsPieBesideTable`), and past the pie's limit sits in the
        // middle of what is left under the header. Alone in its row the box keeps its
        // own height.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .contextMenu { if let table { exportMenu(table) } }
    }

    /// The pie at its side, or with `grows` at the square its layout gives it, never under its
    /// side. It says whether it grew, which spreads the rows.
    private func pie(_ table: StatsCardCountsTable?, phase: StatsBoxPhase, highlighted: Int?, summary: String,
                     side: CGFloat, grows: Bool) -> some View {
        StatsCardCountsPie(
            table: table, phase: phase, highlighted: highlighted,
            summary: StatsSectionText.summary(summary, slot.content.figure) ?? summary,
            selection: slot.selection, hovered: $hoveredRow
        )
        .aspectRatio(1, contentMode: .fit)
        .frame(minWidth: side, idealWidth: side, maxWidth: grows ? Self.largeSide : side,
               minHeight: side, idealHeight: side, maxHeight: grows ? Self.largeSide : side)
        .onGeometryChange(for: Bool.self) { $0.size.height > side } action: { grown = $0 }
    }

    static func total(_ table: StatsCardCountsTable) -> Int { table.rows.reduce(0) { $0 + $1.count } }

    @ViewBuilder private func exportMenu(_ table: StatsCardCountsTable) -> some View {
        let lines = table.rows.map { "\($0.name)\t\($0.countText)\t\($0.shareText)" }
        StatsExportMenu(values: lines.joined(separator: "\n")) {
            StatsCardCountsPie(table: table, phase: .ready, highlighted: nil, summary: "", selection: .constant(.none),
                               hovered: .constant(nil))
                .frame(width: Self.side, height: Self.side)
        }
    }

    /// The columns' widths while nothing is shown: a five-digit count and 100%, in the locale, so
    /// the layout chosen while loading is the one the values usually keep.
    private var loadingCounts: [String] { [StatsChartFormat(locale: locale, now: .distantPast).count(88_888)] }
    private var loadingShares: [String] { [StatsChartFormat(locale: locale, now: .distantPast).percent(1)] }

    /// The state rows (Antoine, 2026-11-23: "Remove the total and notes counters"): names in
    /// every phase, values once ready. The count and share columns are each as wide as their
    /// widest value, so the numbers line up on the right in any locale ("52,6 %").
    private func rows(_ table: StatsCardCountsTable?, highlighted: Int?) -> some View {
        let text = StatsText(locale: locale)
        let names: [(StatsCountLabel, String, StatsSeriesColor)] = [
            (.mature, text.mature, .mature), (.young, text.young, .young), (.relearning, text.relearning, .relearning),
            (.learning, text.learning, .learning), (.new, text.new, .new), (.suspended, text.suspended, .suspended),
            (.buried, text.buried, .buried),
        ]
        return VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(names.enumerated()), id: \.offset) { index, item in
                let (label, name, color) = item
                let row = table?.rows.firstIndex { $0.label == label }.map { (index: $0, row: table!.rows[$0]) }
                stateRow(label: label, name: row?.row.name ?? name, color: row?.row.color ?? color, row: row?.row,
                         index: row?.index ?? index, first: index == 0,
                         highlighted: highlighted == (row?.index ?? index) && table != nil,
                         counts: table?.rows.map(\.countText) ?? loadingCounts, shares: table?.rows.map(\.shareText) ?? loadingShares)
            }
        }
        .font(.subheadline)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.cardCounts.table")
    }

    @ViewBuilder
    private func stateRow(label: StatsCountLabel, name: String, color: StatsSeriesColor, row: StatsCardCountsTable.Row?,
                          index: Int, first: Bool, highlighted: Bool, counts: [String], shares: [String]) -> some View {
        // A state with no card stays, in grey (S7.7).
        let grey = row == nil || row?.count == 0
        let cells = HStack(spacing: Spacing.small) {
            StatsLegendSwatch(color: row?.count == 0 ? .buried : color)
            Text(verbatim: name)
                .foregroundStyle(grey ? .secondary : .primary)
                .lineLimit(compactOverview ? nil : 1)
                .frame(maxWidth: .infinity, alignment: .leading)
            StatsColumnText(text: row?.countText ?? "—", widest: counts)
                .foregroundStyle(grey ? .secondary : .primary)
            StatsColumnText(text: row?.shareText ?? "—", widest: shares)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, Spacing.xxSmall / 2)
        // Given more height than the rows need (beside a grown pie), they share it out.
        .frame(maxHeight: compactOverview ? nil : .infinity)
        // The highlight reaches a little past the row without widening it: a wider row would
        // push the box past its inset.
        .background {
            Radius.rounded(Radius.small)
                .fill(highlighted ? AnyShapeStyle(HierarchicalShapeStyle.quaternary) : AnyShapeStyle(Color.clear))
                .padding(.horizontal, -Spacing.xxSmall)
        }
        // A hairline between rows that share a grown pie's height, none between rows at their own.
        .overlay(alignment: .top) {
            if (compactOverview || grown), !first {
                Divider().accessibilityHidden(true)
            }
        }
        .contentShape(Rectangle())
        #if os(macOS)
        .onHover { inside in
            guard row != nil, (row?.count ?? 0) > 0 else { return }
            if inside { hoveredRow = index } else if hoveredRow == index { hoveredRow = nil }
        }
        #endif
        .onTapGesture {
            guard let row, row.count > 0 else { return }
            let selection = slot.selection
            let pinned = StatsChartSelection(bin: index, pinned: true)
            selection.wrappedValue = selection.wrappedValue == pinned ? .none : pinned
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("stats.cardCounts.row.\(label)")
        if let row, let item = row.itemText, let showInBrowse {
            cells
                .accessibilityAddTraits(.isButton)
                .accessibilityAction(named: Text("stats.chart.showInBrowse", bundle: .module)) { showInBrowse(label, item) }
        } else {
            cells
        }
    }
}

/// macOS overview only: about a third of the inner width, capped at 160 pt. The pair
/// measures at its natural height; an equal-height grid row never enlarges either child.
struct StatsOverviewPieBesideTable: Layout {
    static let limit: CGFloat = 160
    static var minimum: CGFloat { StatsCardCountsBox.smallSide }

    static func pieSide(width: CGFloat, tableWidth: CGFloat) -> CGFloat {
        min(limit, max(minimum, width / 3), max(0, width - Spacing.medium - tableWidth))
    }

    private func sizes(width: CGFloat?, subviews: Subviews) -> (width: CGFloat, pie: CGFloat, table: CGSize) {
        let ideal = subviews[1].sizeThatFits(.unspecified)
        let minimumWidth = Self.minimum + Spacing.medium + ideal.width
        let width = max(width ?? minimumWidth, minimumWidth)
        let pie = Self.pieSide(width: width, tableWidth: ideal.width)
        let table = subviews[1].sizeThatFits(ProposedViewSize(width: width - pie - Spacing.medium, height: nil))
        return (width, pie, table)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard subviews.count == 2 else { return .zero }
        let sizes = sizes(width: proposal.width, subviews: subviews)
        return CGSize(width: sizes.width, height: max(sizes.pie, sizes.table.height))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let sizes = sizes(width: bounds.width, subviews: subviews)
        let height = max(sizes.pie, sizes.table.height)
        subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.minY + height / 2), anchor: .leading,
                          proposal: ProposedViewSize(width: sizes.pie, height: sizes.pie))
        subviews[1].place(at: CGPoint(x: bounds.minX + sizes.pie + Spacing.medium, y: bounds.minY), anchor: .topLeading,
                          proposal: ProposedViewSize(width: sizes.width - sizes.pie - Spacing.medium, height: sizes.table.height))
    }
}

/// Card Counts' pie beside its table (S7.7), the pie first. Asked for its own size it is the
/// pie at its side and the table at its own size, as tall as the taller. Given more height
/// (the box beside a taller one, whose height the row shares) the pie grows into it, up to
/// `limit` and never into width the table needs, and the table takes the pie's height.
struct StatsPieBesideTable: Layout {
    let side: CGFloat
    let limit: CGFloat
    let spacing: CGFloat

    /// The pie's side in a given height, with `own` the height the pair asks for itself and
    /// `room` the width the table leaves the pie. Only height beyond the pair's own grows it:
    /// a table taller than the pie (large text) leaves the pie at its side.
    static func pieSide(side: CGFloat, limit: CGFloat, own: CGFloat, height: CGFloat, room: CGFloat) -> CGFloat {
        height > own ? max(side, min(limit, height, room)) : side
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard subviews.count == 2 else { return .zero }
        let table = subviews[1].sizeThatFits(.unspecified)
        let own = max(side, table.height)
        return CGSize(width: proposal.width ?? side + spacing + table.width,
                      height: proposal.height.map { min(max($0, own), max(limit, own)) } ?? own)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let table = subviews[1].sizeThatFits(.unspecified)
        // The height offered, not only the bounds: a table taller than the limit keeps the bounds
        // at its own height, and the pie still grows to the limit beside it.
        let pie = Self.pieSide(side: side, limit: limit, own: max(side, table.height),
                               height: max(bounds.height, proposal.height ?? 0),
                               room: bounds.width - spacing - table.width)
        subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading,
                          proposal: ProposedViewSize(width: pie, height: pie))
        subviews[1].place(at: CGPoint(x: bounds.minX + pie + spacing, y: bounds.midY), anchor: .leading,
                          proposal: ProposedViewSize(width: max(0, bounds.width - pie - spacing),
                                                     height: pie > side ? max(pie, table.height) : table.height))
    }
}

/// Card Counts' pie (S7.7; Antoine, 2026-11-23: "Fill in the center of the donut"): a sector per
/// state with cards, in the table's order from 12 o'clock clockwise with their true angles,
/// flat and meeting (a thin gap only with Differentiate Without Colour), no empty centre and nothing
/// written on it. A highlighted state keeps its colour
/// and the others fade, as a chart's other bars do. The pointer over a sector highlights it, a
/// click (a tap) pins it and a second one clears it; Left and Right move a pinned state, Esc
/// clears (S14). One element with the box's sentence: the table beside it is the chart's
/// accessible form, so it has no Audio Graph of its own (S15).
struct StatsCardCountsPie: View {
    let table: StatsCardCountsTable?
    let phase: StatsBoxPhase
    let highlighted: Int?
    let summary: String
    @Binding var selection: StatsChartSelection
    @Binding var hovered: Int?

    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiateWithoutColor
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.statsScrolling) private var scrolling
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            if let table, StatsCardCountsBox.total(table) > 0 {
                chart(table)
            } else {
                // While loading: a grey circle with the progress indicator, at the pie's size.
                Circle()
                    .fill(.quaternary)
                    .accessibilityHidden(true)
                StatsBoxPhaseView(phase: phase == .ready ? .empty("", detail: nil) : phase)
            }
        }
        #if os(macOS)
        .focusable(table != nil, interactions: .activate)
        #else
        .focusable(table != nil)
        #endif
        .focused($focused)
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .onKeyPress(.escape) { clear() }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: summary))
        .accessibilityIdentifier("stats.box.cardCounts.plot")
    }

    private var fadedOpacity: Double { contrast == .increased ? 0.5 : 0.3 }

    private func chart(_ table: StatsCardCountsTable) -> some View {
        let rows = Array(table.rows.enumerated()).filter { $0.element.count > 0 }
        return Chart(rows, id: \.offset) { index, row in
            SectorMark(angle: .value("Cards", row.count), angularInset: differentiateWithoutColor ? 1 : 0)
                .foregroundStyle(row.color.color)
                .opacity(highlighted == nil || highlighted == index ? 1 : fadedOpacity)
                .accessibilityHidden(true)
        }
        .chartLegend(.hidden)
        .overlay {
            GeometryReader { geometry in
                let counts = table.rows.map(\.count)
                Color.clear
                    .contentShape(Rectangle())
                    #if os(macOS)
                    .onContinuousHover(coordinateSpace: .local) { phase in
                        guard !scrolling.active else { return }
                        switch phase {
                        case let .active(location):
                            let next = Self.state(at: location, in: geometry.size, counts: counts)
                            if next != hovered { hovered = next }
                        case .ended:
                            if hovered != nil { hovered = nil }
                        }
                    }
                    #endif
                    .gesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                        let next = Self.state(at: value.location, in: geometry.size, counts: counts)
                            .map { StatsChartSelection(bin: $0, pinned: true) } ?? .none
                        selection = next == selection ? .none : next
                        focused = next != .none
                    })
            }
        }
    }

    /// The state under a point: its angle from 12 o'clock clockwise among the cumulative counts,
    /// inside the pie, or nil.
    static func state(at point: CGPoint, in size: CGSize, counts: [Int]) -> Int? {
        let radius = min(size.width, size.height) / 2
        let dx = point.x - size.width / 2, dy = point.y - size.height / 2
        let distance = (dx * dx + dy * dy).squareRoot()
        guard radius > 0, distance <= radius else { return nil }
        var angle = atan2(dx, -dy)
        if angle < 0 { angle += 2 * .pi }
        return state(atFraction: angle / (2 * .pi), counts: counts)
    }

    /// The state whose sector holds a fraction of the turn from 12 o'clock clockwise.
    static func state(atFraction fraction: Double, counts: [Int]) -> Int? {
        let total = counts.reduce(0) { $0 + max(0, $1) }
        guard total > 0, fraction >= 0, fraction <= 1 else { return nil }
        var start = 0.0
        for (index, count) in counts.enumerated() where count > 0 {
            let end = start + Double(count) / Double(total)
            if fraction < end || index == counts.lastIndex(where: { $0 > 0 }) { return index }
            start = end
        }
        return nil
    }

    private func move(_ step: Int) -> KeyPress.Result {
        guard let table else { return .ignored }
        let states = table.rows.indices.filter { table.rows[$0].count > 0 }
        guard !states.isEmpty else { return .ignored }
        let current = selection.bin.flatMap { states.firstIndex(of: $0) }
        let next = current.map { min(max($0 + step, 0), states.count - 1) } ?? (step > 0 ? 0 : states.count - 1)
        selection = StatsChartSelection(bin: states[next], pinned: true)
        return .handled
    }

    private func clear() -> KeyPress.Result {
        guard selection != .none else { return .ignored }
        selection = .none
        return .handled
    }
}
