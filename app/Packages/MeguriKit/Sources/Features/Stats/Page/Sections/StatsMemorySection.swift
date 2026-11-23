import AppModel
import SwiftUI

/// The Memory section's boxes (plan U301; `stats/statistics-page` S7.12 to S7.14, S15, S16):
/// Retrievability, Stability and Difficulty, the charts of the present ("Now"; the period never
/// changes them), side by side. The model writes every value; these views add Retrievability's
/// desired-retention marker from the model's `StatsBoxDetail.retrievability`.
///
/// Show in Browse (S8.4) is a seam: every bar opens its cards (Q9) once U303 passes its
/// `StatsBrowseLink`; until then no callout offers it (S20's last line).
enum StatsMemorySection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .retrievability: StatsRetrievabilityBox(slot: slot, showInBrowse: nil)
        default: StatsMemoryBox(slot: slot, showInBrowse: nil)
        }
    }
}

/// S7.12: cards by retrievability in bars of 5% (green), with a dashed vertical rule at the
/// desired retention in the label colour, named in the legend "Desired retention" and in the
/// callout; the figure is the average retrievability, the sentence the estimated total
/// knowledge; Below Desired Retention and Cards with a Memory State; the left-out footnote.
/// The rule stands between bars, where the desired retention falls on the 0 to 100% axis: 90%
/// at the leading edge of the 90%–95% bar. While the desired retention is not known yet (its
/// part comes with the review log, S10.1), the legend keeps its entry and no rule is drawn.
struct StatsRetrievabilityBox: View {
    let slot: StatsBoxSlot
    let showInBrowse: ((Int) -> Void)?
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        let format = StatsChartFormat(locale: locale, calendar: calendar, now: clock.now())
        let data = Self.marked(content, name: StatsText(locale: locale).desiredRetention, format: format)
        let extras = data.markers.compactMap { marker in
            marker.valueText.map { StatsSectionText(locale: locale).labelled(marker.name, $0) }
        }
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open, showInBrowse: showInBrowse,
            summary: StatsSectionText.summary(title, content.figure, extras)
        ) {
            StatsBoxMarker.now
        }
    }

    /// The box's chart with the desired-retention marker: at its place on the bins' 0 to 100%
    /// scale when the model gives the value, else a legend entry alone.
    static func marked(_ content: StatsBoxContent, name: String, format: StatsChartFormat) -> StatsChartData {
        var data = content.data
        let desired: Double? = if case let .retrievability(value)? = content.detail { value } else { nil }
        let position = desired.flatMap { StatsMarker.position(of: $0, lower: 0, upper: 1, bins: data.bins.count) }
        data.markers = [StatsMarker(id: "desiredRetention", name: name, position: position,
                                    valueText: position == nil ? nil : desired.map(format.percent))]
        return data
    }
}

/// S7.13 and S7.14: Stability's doubling bins (under 1 day to over 2 years) with the median
/// stability, Median Interval and Over 1 Year; Difficulty's ten bins of 10% from easy to hard
/// with the median difficulty and Top Tenth; both indigo, "Now", with the left-out footnote.
struct StatsMemoryBox: View {
    let slot: StatsBoxSlot
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
        ) {
            StatsBoxMarker.now
        }
    }
}
