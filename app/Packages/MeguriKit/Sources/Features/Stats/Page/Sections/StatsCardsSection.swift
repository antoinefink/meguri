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

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open, showInBrowse: showInBrowse,
            summary: StatsSectionText.summary(title, content.figure)
        )
    }
}

/// S7.7: today's cards by state, suspended and buried always their own rows (C4.1's
/// `excluding_inactive`): one horizontal bar divided by state (no donut), then the table, a row
/// per state with its swatch, name, count and share (a row with none stays, in grey), then
/// Total and Notes. The header says "Now"; the period never changes it. The Mature figure
/// shows only at compact width, where the strip has no Mature tile (S6). The table is always
/// drawn at its final size: while loading its names show with dashes and the bar's place holds
/// the progress indicator (S10.3).
struct StatsCardCountsBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for a state's row, given its label and Q4.1's search text; nil offers none.
    let showInBrowse: ((StatsCountLabel, String) -> Void)?
    @Environment(\.locale) private var locale
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    /// The count and share columns, wide enough for "150,000" and "100.0%" at the text size.
    @ScaledMetric(relativeTo: .subheadline) private var countWidth: CGFloat = 64
    @ScaledMetric(relativeTo: .subheadline) private var shareWidth: CGFloat = 52

    var body: some View {
        let content = slot.content
        let ready = content.phase == .ready
        let title = slot.title(locale: locale)
        let table: StatsCardCountsTable? = if ready, case let .cardCounts(value)? = content.detail { value } else { nil }
        StatsBoxContainer {
            VStack(alignment: .leading, spacing: Spacing.small) {
                StatsBoxHeader(box: slot.id.rawValue, title: title, open: slot.open) { StatsBoxMarker.now }
                StatsBoxFigureView(figure: content.figure, ready: ready)
                Group {
                    if let table {
                        StatsSegmentedBars(box: slot.id.rawValue, rows: [Self.bar(table, title: title)], showsNames: false,
                                           selection: slot.selection,
                                           summary: StatsSectionText.summary(title, content.figure) ?? title,
                                           audioGraph: Self.values(table, series: StatsText(locale: locale).cardsSeries))
                    } else {
                        StatsBoxPhaseView(phase: content.phase)
                            .frame(maxWidth: .infinity, minHeight: StatsSegmentedBars.barHeight)
                    }
                }
                // The callout floats over the table under the bar.
                .zIndex(1)
                .contextMenu { if let table { exportMenu(table, title: title) } }
                rows(table)
                if horizontalSizeClass == .compact {
                    StatsBoxSmallFigures(figures: content.figures, ready: ready)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(slot.id.rawValue)")
    }

    /// The one bar: a segment per state, in the table's order.
    static func bar(_ table: StatsCardCountsTable, title: String) -> StatsBarRow {
        StatsBarRow(id: "states", name: title, segments: table.rows.map { row in
            .init(id: "\(row.label)", name: row.name, color: row.color, value: Double(row.count),
                  text: "\(row.countText) (\(row.shareText))")
        })
    }

    /// The table as a chart's data, a category per state: the Audio Graph's.
    static func values(_ table: StatsCardCountsTable, series: String) -> StatsChartData {
        StatsChartData(bins: table.rows.map { .bucket($0.name) },
                       series: [StatsSeries(id: "cards", name: series, color: .measure, mark: .bar,
                                            values: table.rows.map { Double($0.count) })])
    }

    @ViewBuilder private func exportMenu(_ table: StatsCardCountsTable, title: String) -> some View {
        let text = StatsText(locale: locale)
        let lines = table.rows.map { "\($0.name)\t\($0.countText)\t\($0.shareText)" }
            + ["\(text.figureTotal)\t\(table.total)", "\(text.figureNotes)\t\(table.notes)"]
        StatsExportMenu(values: lines.joined(separator: "\n")) {
            StatsSegmentedBars(box: slot.id.rawValue, rows: [Self.bar(table, title: title)], showsNames: false,
                               selection: .constant(.none), summary: title, audioGraph: Self.values(table, series: StatsText(locale: locale).cardsSeries))
        }
    }

    /// The state rows, then Total and Notes: names in every phase, values once ready.
    private func rows(_ table: StatsCardCountsTable?) -> some View {
        let text = StatsText(locale: locale)
        let names: [(StatsCountLabel, String, StatsSeriesColor)] = [
            (.mature, text.mature, .mature), (.young, text.young, .young), (.relearning, text.relearning, .relearning),
            (.learning, text.learning, .learning), (.new, text.new, .new), (.suspended, text.suspended, .suspended),
            (.buried, text.buried, .buried),
        ]
        return VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            ForEach(names, id: \.0) { label, name, color in
                let row = table?.rows.first { $0.label == label }
                stateRow(label: label, name: row?.name ?? name, color: row?.color ?? color, row: row)
            }
            Divider()
                .padding(.vertical, Spacing.xxSmall)
            totalRow(text.figureTotal, table?.total, id: "total")
            totalRow(text.figureNotes, table?.notes, id: "notes")
        }
        .font(.subheadline)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.cardCounts.table")
    }

    @ViewBuilder
    private func stateRow(label: StatsCountLabel, name: String, color: StatsSeriesColor, row: StatsCardCountsTable.Row?) -> some View {
        // A state with no card stays, in grey (S7.7).
        let grey = row == nil || row?.count == 0
        let cells = HStack(spacing: Spacing.small) {
            StatsLegendSwatch(color: row?.count == 0 ? .buried : color)
            Text(verbatim: name)
                .foregroundStyle(grey ? .secondary : .primary)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(verbatim: row?.countText ?? "—")
                .monospacedDigit()
                .foregroundStyle(grey ? .secondary : .primary)
                .lineLimit(1)
                .frame(minWidth: countWidth, alignment: .trailing)
            Text(verbatim: row?.shareText ?? "—")
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .frame(minWidth: shareWidth, alignment: .trailing)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("stats.box.cardCounts.row.\(label)")
        if let row, let item = row.itemText, let showInBrowse {
            Button { showInBrowse(label, item) } label: { cells.contentShape(Rectangle()) }
                .buttonStyle(.plain)
                .accessibilityAction(named: Text("stats.chart.showInBrowse", bundle: .module)) { showInBrowse(label, item) }
        } else {
            cells
        }
    }

    private func totalRow(_ name: String, _ value: String?, id: String) -> some View {
        HStack(spacing: Spacing.small) {
            // The swatch's place, so Total lines up with the state names.
            Color.clear.frame(width: StatsLegendSwatch.side, height: 1)
            Text(verbatim: name)
                .fontWeight(.medium)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(verbatim: value ?? "—")
                .monospacedDigit()
                .fontWeight(.medium)
                .foregroundStyle(value == nil ? .secondary : .primary)
                .lineLimit(1)
                .frame(minWidth: countWidth, alignment: .trailing)
            Color.clear.frame(width: shareWidth, height: 1)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("stats.box.cardCounts.row.\(id)")
    }
}
