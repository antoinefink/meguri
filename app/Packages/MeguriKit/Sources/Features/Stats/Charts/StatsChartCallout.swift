import DesignSystem
import SwiftUI

/// The selection's callout (S8.3, N25.4): the period, every series with its
/// swatch, name and value, and Show in Browse when the bin can be listed. A
/// small material panel beside the rule, never over it.
struct StatsChartCallout: View {
    let title: String
    let rows: [(name: String, value: String, color: StatsSeriesColor?)]
    let showInBrowse: (() -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: title)
                .font(.caption.weight(.semibold))
            Grid(alignment: .leading, horizontalSpacing: Spacing.xSmall, verticalSpacing: Spacing.xxSmall) {
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        Group {
                            if let color = row.color {
                                StatsLegendSwatch(color: color)
                            } else {
                                Color.clear.frame(width: 1, height: 1)
                            }
                        }
                        Text(verbatim: row.name)
                            .foregroundStyle(.secondary)
                        Text(verbatim: row.value)
                            .monospacedDigit()
                            .gridColumnAlignment(.trailing)
                    }
                }
            }
            .font(.caption)
            if let showInBrowse {
                Button(action: showInBrowse) {
                    Text("stats.chart.showInBrowse", bundle: .module)
                }
                .font(.caption)
                .controlSize(.small)
                .accessibilityIdentifier("stats.chart.callout.showInBrowse")
            }
        }
        .padding(Spacing.xSmall)
        .materialPanel(in: Radius.rounded(Radius.small))
    }
}

/// A legend swatch: the series' colour, as the system legend draws it.
struct StatsLegendSwatch: View {
    let color: StatsSeriesColor

    var body: some View {
        Circle()
            .fill(color.color)
            .frame(width: 8, height: 8)
            .accessibilityHidden(true)
    }
}
