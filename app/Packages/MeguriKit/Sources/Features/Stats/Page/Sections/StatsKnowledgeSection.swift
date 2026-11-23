import SwiftUI

/// The Knowledge section's boxes (plan U297; `stats/statistics-page` S7.1 to S7.3, S15, S16):
/// Knowledge Over Time, the lead, then Retention and Forgotten. The model writes every value
/// (`StatsPresenter`); these views choose the header's accessory, the chart element's label and
/// what a bar offers.
enum StatsKnowledgeSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .knowledgeOverTime: StatsKnowledgeOverTimeBox(slot: slot)
        case .retention: StatsRetentionBox(slot: slot)
        case .forgotten: StatsForgottenBox(slot: slot)
        default: StatsGenericBox(slot: slot)
        }
    }

    /// The chart element's label (S15): what the chart is, then its figure and sentence, so a
    /// VoiceOver user hears "Retention, 93.1%, Desired retention 90.0%" rather than a bare number.
    static func summary(_ subject: String, _ figure: StatsBoxFigure?) -> String? {
        guard let figure else { return nil }
        return [subject, figure.value, figure.sentence].filter { !$0.isEmpty }.joined(separator: ", ")
    }

    static func text(_ key: String.LocalizationValue, locale: Locale) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }
}

/// S7.1: Known (green) and Studied (grey, dashed) on one fitted count axis, the figure "<K>
/// cards" with its share and change, the three small figures and the footnote of left-out and
/// unestimated cards. The header says "Estimated", and its help says how: every sample is
/// replayed with today's parameters, so the chart is honest about being a reconstruction. No
/// Show in Browse: a past state cannot be listed (S8.4).
struct StatsKnowledgeOverTimeBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        let help = StatsKnowledgeSection.text("stats.knowledge.estimatedHelp", locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: slot.title(locale: locale), figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open,
            summary: StatsKnowledgeSection.summary(StatsKnowledgeSection.text("stats.knowledge.spokenTitle", locale: locale),
                                                   content.figure)
        ) {
            StatsBoxMarker.estimated
                .help(Text(verbatim: help))
                .accessibilityHint(Text(verbatim: help))
                .accessibilityIdentifier("stats.box.knowledgeOverTime.estimated")
        }
    }
}

/// S7.2: the period's retention as a green line with points, the desired retention as a dashed
/// rule in the label colour, a bar with fewer than 20 counted reviews an open point left out of
/// the line (its callout and element say so), a bar with none a break. The All, Young and Mature
/// switch trails the title and clears the selection. By Period lives on the chart's own page
/// (U302), which the header opens. No Show in Browse (S8.4).
struct StatsRetentionBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open,
            summary: StatsKnowledgeSection.summary(title, content.figure)
        ) {
            StatsBoxAccessory(id: slot.id, model: slot.model)
        }
    }
}

/// S7.3: forgotten reviews per bar, orange from zero (decided 2026-11-22), the figure with its
/// daily average and the Young and Mature figures. A 1M day bar offers Show in Browse ("Cards
/// Forgotten on <date>", Q9) once U303 gives the page its link: until then no callout offers
/// it (S20's last line), though the model already marks those bars.
struct StatsForgottenBox: View {
    let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        let title = slot.title(locale: locale)
        StatsChartBox(
            box: slot.id.rawValue, title: title, figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open, showInBrowse: nil,
            summary: StatsKnowledgeSection.summary(title, content.figure)
        )
    }
}
