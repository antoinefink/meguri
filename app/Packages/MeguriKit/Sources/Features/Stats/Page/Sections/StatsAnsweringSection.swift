import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Answering section's boxes (plan U300; `stats/statistics-page` S7.10, S7.11, S15, S16):
/// Answers and Time of Day, side by side (no lead). The model writes every value; these views
/// draw Answers' three bars and table and Time of Day's two aligned panels. Neither offers Show
/// in Browse (S8.4).
enum StatsAnsweringSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .timeOfDay: StatsTimeOfDayBox(slot: slot)
        default: StatsAnswersBox(slot: slot)
        }
    }
}

/// S7.10: presses in the period window (S4.3) as one full-width bar for each of Learning, Young
/// and Mature, divided Again, Hard, Good, Easy in that order, the segments meeting as one bar
/// (N25.4; S7.10's gap between segments gave way to the owner's flat bars of 2026-11-23, and
/// only Differentiate Without Colour parts them); under it the table repeats the four counts as
/// columns per group, its header the legend. The figure, "<p>% Good" and each group's Again
/// share. A segment's callout gives its count and its share of the group.
struct StatsAnswersBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    /// A button's column, wide enough for its legend entry and "12,345".
    @ScaledMetric(relativeTo: .subheadline) private var countWidth: CGFloat = 64

    var body: some View {
        let content = slot.content
        let ready = content.phase == .ready
        let title = slot.title(locale: locale)
        let table: StatsAnswersTable? = if ready, case let .answers(value)? = content.detail { value } else { nil }
        let bars = table.map { Self.bars($0, data: content.data) } ?? []
        StatsBoxContainer {
            VStack(alignment: .leading, spacing: Spacing.small) {
                StatsBoxHeader(box: slot.id.rawValue, title: title, open: slot.open) { EmptyView() }
                StatsBoxFigureView(figure: content.figure, ready: ready)
                Group {
                    if !bars.isEmpty {
                        StatsSegmentedBars(box: slot.id.rawValue, rows: bars, showsNames: true, selection: slot.selection,
                                           summary: StatsSectionText.summary(title, content.figure) ?? title,
                                           audioGraph: content.data)
                    } else {
                        StatsBoxPhaseView(phase: content.phase)
                            .frame(maxWidth: .infinity, minHeight: Self.barsHeight)
                    }
                }
                .frame(minHeight: Self.barsHeight, alignment: .top)
                // The callout floats over the table under the bars.
                .zIndex(1)
                .contextMenu {
                    if ready {
                        StatsExportMenu(values: StatsChartFormat(locale: locale, calendar: calendar, now: clock.now()).tabSeparatedValues(content.data)) {
                            StatsSegmentedBars(box: slot.id.rawValue, rows: bars, showsNames: true, selection: .constant(.none),
                                               summary: title, audioGraph: content.data)
                        }
                    }
                }
                tableView(table, data: content.data)
                StatsBoxSmallFigures(figures: content.figures, ready: ready)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(slot.id.rawValue)")
    }

    /// Three bars and the space between them: the plot's place in every phase.
    static var barsHeight: CGFloat { 3 * StatsSegmentedBars.barHeight + 2 * Spacing.xSmall }

    /// A bar per group, its segments the four buttons with the series' names and colours.
    static func bars(_ table: StatsAnswersTable, data: StatsChartData) -> [StatsBarRow] {
        table.rows.map { row in
            StatsBarRow(id: row.name, name: row.name, segments: data.series.indices.map { button in
                let series = data.series[button]
                let count = row.counts.indices.contains(button) ? row.counts[button] : 0
                let countText = row.countTexts.indices.contains(button) ? row.countTexts[button] : ""
                let share = row.shareTexts.indices.contains(button) ? row.shareTexts[button] : ""
                return .init(id: series.id, name: series.name, color: series.color, value: Double(count),
                             text: "\(countText) (\(share))")
            })
        }
    }

    /// The table: a column per button (its header the legend, in stack order), a row per group.
    private func tableView(_ table: StatsAnswersTable?, data: StatsChartData) -> some View {
        let text = StatsText(locale: locale)
        let buttons: [(String, StatsSeriesColor)] = data.series.isEmpty
            ? [(text.again, .again), (text.hard, .hard), (text.good, .good), (text.easy, .easy)]
            : data.series.map { ($0.name, $0.color) }
        let groups = table?.rows.map { ($0.name, $0.countTexts) }
            ?? [text.learning, text.young, text.mature].map { ($0, [String]()) }
        return VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            HStack(spacing: Spacing.small) {
                Spacer(minLength: 0)
                ForEach(buttons, id: \.0) { name, color in
                    StatsLegendEntry(name: name, color: color)
                        .minimumScaleFactor(0.8)
                        .frame(width: countWidth, alignment: .trailing)
                }
            }
            .accessibilityHidden(true)
            ForEach(Array(groups.enumerated()), id: \.offset) { group, row in
                let (name, counts) = row
                HStack(spacing: Spacing.small) {
                    Text(verbatim: name)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    ForEach(buttons.indices, id: \.self) { index in
                        Text(verbatim: counts.indices.contains(index) ? counts[index] : "—")
                            .monospacedDigit()
                            .foregroundStyle(counts.isEmpty ? .secondary : .primary)
                            .lineLimit(1)
                            .frame(width: countWidth, alignment: .trailing)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(verbatim: Self.spoken(name, buttons: buttons.map(\.0), counts: counts, text: text)))
                .accessibilityIdentifier("stats.box.answers.row.\(["learning", "young", "mature"][min(group, 2)])")
            }
        }
        .font(.subheadline)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.answers.table")
    }

    /// A group's row for VoiceOver: "Young, Again 12, Hard 3, Good 200, Easy 50".
    static func spoken(_ group: String, buttons: [String], counts: [String], text: StatsText) -> String {
        ([group] + buttons.indices.map { "\(buttons[$0]) \(counts.indices.contains($0) ? counts[$0] : text.noValue)" })
            .joined(separator: ", ")
    }
}

/// S7.11: two aligned panels on one clock, sharing the x axis and never a y axis: above, the
/// share remembered per hour (green points and line, only hours with at least 20 counted
/// reviews), below, reviews per hour (indigo bars). One selection draws its rule through both
/// panels, and the lower panel's callout names the hour, the reviews and the share remembered.
/// The clock follows the system's 12- or 24-hour setting; on compact widths only even hours are
/// labelled. The figure is the busiest hour; Best Hour for Remembering and Reviews Before Noon.
struct StatsTimeOfDayBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    /// The y axis labels' width in both panels ("100%" and "1.2K"), so their plots line up.
    @ScaledMetric(relativeTo: .caption2) private var axisLabelWidth: CGFloat = 30

    var body: some View {
        let content = slot.content
        let ready = content.phase == .ready
        let title = slot.title(locale: locale)
        let text = StatsText(locale: locale)
        let upper = Self.upper(content, text: text)
        let lower = Self.lower(content, upper: upper, text: text)
        let summary = StatsSectionText.summary(title, content.figure) ?? title
        StatsBoxContainer {
            VStack(alignment: .leading, spacing: Spacing.small) {
                StatsBoxHeader(box: slot.id.rawValue, title: title, open: slot.open) { EmptyView() }
                StatsBoxFigureView(figure: content.figure, ready: ready)
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    StatsChartPlot(
                        box: "\(slot.id.rawValue).remembered", data: ready ? upper : upper.skeleton(), phase: .ready,
                        size: .panel, summary: summary, selection: slot.selection, showInBrowse: nil, separatesSeries: nil,
                        options: StatsPlotOptions(legend: false, xAxis: false, callout: false, focusable: false,
                                                  accessible: false, yLabelWidth: axisLabelWidth)
                    )
                    StatsChartPlot(
                        box: slot.id.rawValue, data: ready ? lower : lower.skeleton(), phase: plotPhase(content.phase),
                        size: .panel, summary: summary, selection: slot.selection, showInBrowse: nil, separatesSeries: nil,
                        options: StatsPlotOptions(legend: false, yLabelWidth: axisLabelWidth)
                    )
                    HStack(spacing: Spacing.medium) {
                        ForEach(upper.series + lower.series) { series in
                            StatsLegendEntry(name: series.name, color: series.color)
                        }
                    }
                    .accessibilityHidden(true)
                }
                .contextMenu {
                    if ready {
                        StatsExportMenu(values: StatsChartFormat(locale: locale, calendar: calendar, now: clock.now())
                            .tabSeparatedValues(lower)) {
                            VStack(spacing: Spacing.xxSmall) {
                                StatsChartPlot(box: slot.id.rawValue, data: upper, phase: .ready, size: .panel, summary: title,
                                               selection: .constant(.none), showInBrowse: nil, separatesSeries: nil,
                                               options: StatsPlotOptions(legend: false, xAxis: false, yLabelWidth: axisLabelWidth))
                                StatsChartPlot(box: slot.id.rawValue, data: lower, phase: .ready, size: .panel, summary: title,
                                               selection: .constant(.none), showInBrowse: nil, separatesSeries: nil,
                                               options: StatsPlotOptions(yLabelWidth: axisLabelWidth))
                            }
                        }
                    }
                }
                StatsBoxSmallFigures(figures: content.figures, ready: ready)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(slot.id.rawValue)")
    }

    private func plotPhase(_ phase: StatsBoxPhase) -> StatsPlotPhase {
        switch phase {
        case .ready: .ready
        case let .loading(fraction): .loading(fraction)
        case let .empty(message, detail): .message(message, detail)
        case .failed: .message(String(localized: "stats.chart.failed", bundle: .module, locale: locale), nil)
        }
    }

    /// The upper panel: the share remembered per hour, from the model's detail.
    static func upper(_ content: StatsBoxContent, text: StatsText) -> StatsChartData {
        if case let .timeOfDay(retention)? = content.detail { return retention }
        return StatsChartData(bins: [], series: [StatsSeries(id: "remembered", name: text.remembered, color: .remembered,
                                                             mark: .line(points: true), values: [])],
                              unit: .fraction, yScale: .fraction(lower: 0.7))
    }

    /// The lower panel: reviews per hour, its callout also naming the hour's share remembered,
    /// so one callout reads both panels (and each hour's element, S15).
    static func lower(_ content: StatsBoxContent, upper: StatsChartData, text: StatsText) -> StatsChartData {
        var data = content.data
        if let remembered = upper.series.first, remembered.values.count == data.bins.count, !data.bins.isEmpty {
            data.calloutLines.append(StatsCalloutLine(id: "remembered", name: remembered.name, values: remembered.values,
                                                      unit: .fraction))
        }
        return data
    }
}
