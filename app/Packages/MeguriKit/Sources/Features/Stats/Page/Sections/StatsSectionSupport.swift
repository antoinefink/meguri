import DesignSystem
import SwiftUI

/// The texts the section views of U299 to U301 add to the model's (`StatsText`): the tables'
/// column titles and their spoken forms, from the String Catalog (S16).
struct StatsSectionText {
    let locale: Locale

    private func text(_ value: String.LocalizationValue) -> String {
        String(localized: value, bundle: .module, locale: locale)
    }

    /// The chart element's label (S15): what the chart is, then its figure and sentence, then any
    /// extra the plot shows ("Desired retention 90.0%"), so VoiceOver names the chart first.
    static func summary(_ subject: String, _ figure: StatsBoxFigure?, _ extras: [String] = []) -> String? {
        guard let figure else { return nil }
        return ([subject, figure.value, figure.sentence] + extras).filter { !$0.isEmpty }.joined(separator: ", ")
    }

    /// "<name> <value>", a value read with its name ("Retention 91.2%").
    func labelled(_ name: String, _ value: String) -> String { text("stats.value.labelled \(name) \(value)") }

    // By Deck (S7.15)
    func column(_ column: StatsByDeckColumn) -> String {
        switch column {
        case .deck: text("stats.byDeck.column.deck")
        case .cards: text("stats.byDeck.column.cards")
        case .reviews: text("stats.byDeck.column.reviews")
        case .retention: text("stats.byDeck.column.retention")
        case .time: text("stats.byDeck.column.time")
        }
    }
    var ascending: String { text("stats.byDeck.ascending") }
    var descending: String { text("stats.byDeck.descending") }
    var rowHint: String { text("stats.byDeck.rowHint") }
}

/// Copy Image and Copy Values (S9) for a box that draws its own plot (Card Counts, Answers,
/// Time of Day, By Deck), as the chart box's own menu does.
struct StatsExportMenu<Picture: View>: View {
    let values: String
    @ViewBuilder let image: () -> Picture
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.displayScale) private var displayScale
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar

    var body: some View {
        Button {
            StatsChartExport.copyImage(of: image().environment(\.locale, locale).environment(\.calendar, calendar),
                                       width: 640, colorScheme: colorScheme, scale: displayScale)
        } label: {
            Label { Text("stats.chart.copyImage", bundle: .module) } icon: { Image(systemName: "photo") }
        }
        Button {
            StatsChartExport.copyValues(values)
        } label: {
            Label { Text("stats.chart.copyValues", bundle: .module) } icon: { Image(systemName: "tablecells") }
        }
    }
}

/// One legend entry drawn by hand, as the system chart legend draws its own (a swatch and the
/// series' name in the caption style), for a box whose plots draw none (Time of Day's two
/// panels share one legend; Answers' table header is its legend).
struct StatsLegendEntry: View {
    let name: String
    let color: StatsSeriesColor

    var body: some View {
        HStack(spacing: Spacing.xxSmall) {
            StatsLegendSwatch(color: color)
            Text(verbatim: name)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

/// A number in a table column, right-aligned in tabular digits and as wide as the column's
/// widest value, so the column lines up on the right in any locale ("52,6 %", "1 234") and a
/// value never truncates or pushes the row past its box.
struct StatsColumnText: View {
    let text: String
    /// Every value of the column.
    let widest: [String]

    var body: some View {
        ZStack(alignment: .trailing) {
            ForEach(Array(Set(widest)).sorted(), id: \.self) { value in
                Text(verbatim: value).hidden()
            }
            Text(verbatim: text)
        }
        .monospacedDigit()
        .lineLimit(1)
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: text))
    }
}
