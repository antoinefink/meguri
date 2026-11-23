import SwiftUI

/// The Coming Up section's box (plan U300; `stats/statistics-page` S7.9, S15, S16): Reviews Due,
/// the lead. The model writes every value; the chart system labels its axis so that no date
/// touches the Overdue and Later bars (`StatsChartFormat.labelledBins`).
///
/// Show in Browse (S8.4) is a seam: any bar offers it once U303 passes its `StatsBrowseLink`
/// (Q9's due days, weeks, months and Overdue); until then no callout does (S20's last line).
enum StatsComingUpSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsReviewsDueBox(slot: slot, showInBrowse: nil)
    }
}

/// S7.9: cards due per bar ahead, stacked from the baseline Mature (green) and Young (mint), the
/// Overdue bar first in grey with its count on the plot, Later last past the 24th month (All);
/// the figure is what falls due in the shown span with today's remaining and tomorrow's count,
/// then Overdue, Due Tomorrow, Next 30 Days and Daily Load. The callout names the period, Young,
/// Mature and the running "Total by then" (no second axis, S8.2).
struct StatsReviewsDueBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for any bar, given the bin; nil offers none.
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
