import Accessibility
import SwiftUI

/// One accessibility element per bin over the plot, in runs of ten (S15, P18.2):
/// each run a container named "<first> to <last>", each bin reading the
/// callout's text, with Show in Browse as a named action where the bin offers it.
/// Swift Charts' own mark elements are hidden in its place.
///
/// Equatable on what its texts are made of, so a hover, which redraws the plot's marks, never
/// formats every bin's text again (R2078).
struct StatsAccessibilityBins: View, Equatable {
    let data: StatsChartData
    let format: StatsChartFormat
    let browse: ((Int) -> Void)?

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.data == rhs.data && lhs.format.locale == rhs.format.locale && lhs.format.calendar == rhs.format.calendar
            && lhs.format.calendar.isDate(lhs.format.now, inSameDayAs: rhs.format.now) && (lhs.browse == nil) == (rhs.browse == nil)
    }

    var body: some View {
        // Categories are equal bands, so equal widths sit over the bars.
        GeometryReader { geometry in
            let width = geometry.size.width / CGFloat(max(1, data.bins.count))
            HStack(spacing: 0) {
                ForEach(format.runs(data), id: \.bins) { run in
                    HStack(spacing: 0) {
                        ForEach(Array(run.bins), id: \.self) { bin in
                            element(bin)
                                .frame(width: width)
                        }
                    }
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(run.label)
                }
            }
            .frame(maxHeight: .infinity)
        }
    }

    @ViewBuilder private func element(_ bin: Int) -> some View {
        let base = Color.clear
            .allowsHitTesting(false)
            .accessibilityElement()
            .accessibilityLabel(format.calloutText(data, bin: bin))
        if data.browsable.contains(bin), let browse {
            base.accessibilityAction(named: Text("stats.chart.showInBrowse", bundle: .module)) { browse(bin) }
        } else {
            base
        }
    }
}

/// The chart's Audio Graph (S15, P18.3): one category per bin, named by its
/// period; a numeric axis in the chart's unit; one series per stacked kind or line.
struct StatsAudioGraph: AXChartDescriptorRepresentable {
    let data: StatsChartData
    let format: StatsChartFormat
    let summary: String

    func makeChartDescriptor() -> AXChartDescriptor {
        let names = data.bins.map(format.name)
        let xAxis = AXCategoricalDataAxisDescriptor(
            title: String(localized: "stats.chart.period", bundle: .module),
            categoryOrder: names
        )
        let domain = data.bins.isEmpty ? 0...1 : data.yDomain
        let unit = data.unit
        let format = format
        let yAxis = AXNumericDataAxisDescriptor(
            title: data.unitName ?? Self.unitName(unit),
            range: domain,
            gridlinePositions: []
        ) { value in format.value(value, unit: unit) }
        let series = data.series.map { series in
            AXDataSeriesDescriptor(
                name: series.name,
                isContinuous: series.isLine,
                dataPoints: zip(names, series.values).compactMap { name, value in
                    value.map { AXDataPoint(x: name, y: $0) }
                }
            )
        }
        return AXChartDescriptor(title: summary, summary: nil, xAxis: xAxis, yAxis: yAxis, additionalAxes: [], series: series)
    }

    static func unitName(_ unit: StatsValueUnit) -> String {
        switch unit {
        case .count: String(localized: "stats.chart.unit.count", bundle: .module)
        case .duration: String(localized: "stats.chart.unit.time", bundle: .module)
        case .fraction: String(localized: "stats.chart.unit.percent", bundle: .module)
        }
    }
}
