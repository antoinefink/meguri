import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Answering section's boxes (plan U300; `stats/statistics-page` S7.10, S7.11, S15, S16):
/// Answers and Time of Day, side by side (no lead). The model writes every value; these views
/// draw Answers' shares and Time of Day's two aligned panels. Neither offers Show in Browse
/// (S8.4).
enum StatsAnsweringSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .timeOfDay: StatsTimeOfDayBox(slot: slot)
        default: StatsAnswersBox(slot: slot)
        }
    }
}

/// S7.10, as Antoine changed it on 2026-11-23 ("percentages instead of counts"): one vertical
/// bar per card kind (Learning, Young, Mature), stacked to 100% by answer from the baseline
/// Again, Hard, Good, Easy on a 0 to 100% axis, so a kind with few answers reads as well as
/// one with many. The callout names the kind, each answer's share with its count, and the
/// kind's answers ("15,880 answers"); the figure stays the presses with "<p>% Good", and the
/// three Again shares stay. The segments meet, as every stacked bar on the page does.
struct StatsAnswersBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure,
            data: Self.shares(content.data, text: StatsText(locale: locale), format: StatsChartFormat(locale: locale, now: .distantPast)),
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open,
            summary: StatsSectionText.summary(title, content.figure)
        )
    }

    /// The model's presses per kind and answer as shares of each kind's presses, each written
    /// with its count, and each kind's answers as a callout line. With no data yet the four
    /// answers' series stay, so the legend is in place while the box loads (S10.3).
    static func shares(_ data: StatsChartData, text: StatsText, format: StatsChartFormat) -> StatsChartData {
        let answers: [(String, String, StatsSeriesColor)] = [
            ("again", text.again, .again), ("hard", text.hard, .hard), ("good", text.good, .good), ("easy", text.easy, .easy),
        ]
        guard !data.series.isEmpty else {
            return StatsChartData(bins: [], series: answers.map { StatsSeries(id: $0.0, name: $0.1, color: $0.2, mark: .stackedBar, values: []) },
                                  unit: .fraction, yScale: .fraction(lower: 0))
        }
        let totals = data.bins.indices.map { bin in data.series.reduce(0) { $0 + max(0, data.value($1, at: bin) ?? 0) } }
        let series = data.series.map { series in
            var share = series
            share.values = data.bins.indices.map { bin in
                totals[bin] > 0 ? (data.value(series, at: bin) ?? 0) / totals[bin] : nil
            }
            share.valueTexts = data.bins.indices.map { bin in
                guard totals[bin] > 0, let count = data.value(series, at: bin) else { return nil }
                return text.withShare(format.percent(count / totals[bin]), format.count(count))
            }
            return share
        }
        let answered = totals.map { total -> String? in text.answers(Int(total), format.count(total)) }
        // A fraction axis from 0 to 100%, whatever a sum of shares rounds to.
        return StatsChartData(bins: data.bins, series: series, unit: .fraction, yScale: .fraction(lower: 0),
                              calloutLines: [StatsCalloutLine(id: "answers", name: "", values: totals.map { $0 }, texts: answered)],
                              unitName: data.unitName)
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
                                                  accessible: false, interactive: false, yLabelWidth: axisLabelWidth)
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
