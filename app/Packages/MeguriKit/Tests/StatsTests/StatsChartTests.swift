import Foundation
import SwiftUI
import Testing

@testable import Stats

/// The chart system (plan U295): stacking, the y domain, colour families, the
/// sparse rule, bin names and axis labels (S8.7), runs of ten (P18.2), Copy
/// Values, and a box that keeps its size in every phase (S8.5).
@Suite struct StatsChartTests {
    static let locale = Locale(identifier: "en_GB")
    static var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        calendar.firstWeekday = 2
        return calendar
    }

    static func date(_ y: Int, _ m: Int, _ d: Int) -> Date {
        calendar.date(from: DateComponents(year: y, month: m, day: d))!
    }

    static let now = date(2026, 11, 22)
    static var format: StatsChartFormat { StatsChartFormat(locale: locale, calendar: calendar, now: now) }

    static func stack(_ values: [[Double?]], bins: Int = 3) -> StatsChartData {
        StatsChartData(
            bins: (0..<bins).map { StatsPeriod.bucket("b\($0)") },
            series: values.enumerated().map { index, values in
                StatsSeries(id: "s\(index)", name: "S\(index)", color: [.mature, .young, .learning][index % 3], mark: .stackedBar, values: values)
            }
        )
    }

    // MARK: Stacking and domain

    @Test func stacksFromTheBaselineInSeriesOrderWithNilAsZero() {
        let data = Self.stack([[3, 1, nil], [2, nil, 4]])
        let segments = data.stackedSegments()
        #expect(segments.map { [$0.series, $0.bin] } == [[0, 0], [1, 0], [0, 1], [1, 1], [0, 2], [1, 2]])
        #expect(segments.map(\.lower) == [0, 3, 0, 1, 0, 0])
        #expect(segments.map(\.upper) == [3, 5, 1, 1, 0, 4])
        #expect(data.stackTotal(at: 0) == 5)
    }

    @Test func aZeroBasedAxisEndsOnANiceValueAboveTheTallestStack() {
        #expect(Self.stack([[30, 10, 0], [17, 0, 0]]).yDomain == 0...50)
        #expect(Self.stack([[0, 0, 0]]).yDomain == 0...1)
        #expect(StatsChartData.niceCeiling(101) == 200)
        #expect(StatsChartData.niceCeiling(2_400) == 2_500)
        #expect(StatsChartData.niceCeiling(0.83) == 1)
    }

    @Test func theRetentionAxisRunsFrom70PercentAndExtendsDown() {
        func data(_ values: [Double?]) -> StatsChartData {
            StatsChartData(
                bins: values.indices.map { StatsPeriod.bucket("\($0)") },
                series: [StatsSeries(id: "r", name: "R", color: .remembered, mark: .line(points: true), values: values)],
                rules: [StatsRule(id: "d", name: "Desired", value: 0.9)],
                unit: .fraction, yScale: .fraction(lower: 0.7)
            )
        }
        #expect(data([0.88, 0.93, nil]).yDomain == 0.7...1)
        #expect(data([0.62, 0.93]).yDomain == 0.6...1)
    }

    @Test func aFittedAxisStartsNearTheLowestValue() {
        let data = StatsChartData(
            bins: [.bucket("a"), .bucket("b")],
            series: [StatsSeries(id: "k", name: "K", color: .remembered, mark: .line(), values: [3_000, 3_500])],
            yScale: .fitted
        )
        let domain = data.yDomain
        #expect(domain.lowerBound > 2_500 && domain.lowerBound < 3_000)
        #expect(domain.upperBound >= 3_500)
    }

    // MARK: Lines, sparse, families

    @Test func aMissingValueBreaksALineAndAnOpenPointIsLeftOut() {
        let series = StatsSeries(id: "r", name: "R", color: .remembered, mark: .line(), values: [1, 2, nil, 4, 5, 6], openPoints: [4])
        #expect(StatsChartPlot.linePieces(series).map(\.bins) == [[0, 1], [3, 5]])
    }

    @Test func fewerThanThreeBinsWithDataIsSparse() {
        #expect(Self.stack([[0, 4, 2]]).isSparse)
        #expect(!Self.stack([[1, 4, 2]]).isSparse)
        #expect(Self.stack([[nil, nil, nil]]).isSparse)
    }

    @Test func aChartKeepsToOneColourFamily() {
        #expect(StatsSeriesColor.keepsOneFamily([.mature, .young, .relearning, .learning, .filtered]))
        #expect(StatsSeriesColor.keepsOneFamily([.again, .hard, .good, .easy]))
        #expect(StatsSeriesColor.keepsOneFamily([.remembered, .reference, .rule]))
        #expect(!StatsSeriesColor.keepsOneFamily([.mature, .good]))
        let samples = StatsChartSamples(period: .year, calendar: Self.calendar, now: Self.now)
        for data in [samples.knowledge, samples.reviewsByType(browsable: true), samples.reviewsByAnswer, samples.retention,
                     samples.forgotten, samples.cardsOverTime, samples.addedAndStarted(), samples.reviewsDue, StatsChartSamples.stability] {
            #expect(data.keepsOneFamily)
        }
    }

    @Test func theFamiliesUseTheContractsSystemColours() {
        #expect(StatsSeriesColor.new.color == .blue)
        #expect(StatsSeriesColor.learning.color == .orange)
        #expect(StatsSeriesColor.relearning.color == .red)
        #expect(StatsSeriesColor.young.color == .mint)
        #expect(StatsSeriesColor.mature.color == .green)
        #expect(StatsSeriesColor.suspended.color == .yellow)
        #expect(StatsSeriesColor.buried.color == .gray)
        #expect(StatsSeriesColor.again.color == .red)
        #expect(StatsSeriesColor.hard.color == .orange)
        #expect(StatsSeriesColor.good.color == .green)
        #expect(StatsSeriesColor.easy.color == .blue)
        #expect(StatsSeriesColor.measure.color == .indigo)
        #expect(StatsSeriesColor.forgotten.color == .orange)
    }

    // MARK: Names and labels (S8.7)

    @Test func periodsAreNamedWithTheSystemsShortStyles() {
        let format = Self.format
        #expect(format.name(.day(Self.date(2026, 11, 17))) == "17 Nov")
        #expect(format.name(.day(Self.date(2025, 11, 17))) == "17 Nov 2025")
        #expect(format.name(.week(start: Self.date(2026, 11, 16))) == "Week of 16 Nov")
        #expect(format.name(.month(Self.date(2026, 3, 1))) == "March 2026")
        #expect(format.name(.overdue) == "Overdue")
        #expect(format.name(.later) == "Later")
        #expect(format.name(.bucket("1–3 d")) == "1–3 d")
    }

    @Test func numbersUseTheSystemsFormatters() {
        let format = Self.format
        #expect(format.count(12_345) == "12,345")
        #expect(format.percent(0.9137) == "91.4%")
        #expect(format.duration(seconds: 19 * 60) == "19 min")
        #expect(format.duration(seconds: 72 * 60).contains("1"))
        #expect(format.value(0.5, unit: .fraction) == "50.0%")
    }

    @Test func dayAxesLabelEveryWeekCountedBackFromToday() {
        let bins = (0..<30).map { StatsPeriod.day(Self.calendar.date(byAdding: .day, value: $0 - 29, to: Self.now)!) }
        let labels = Self.format.labelledBins(bins, maximum: 5)
        #expect(labels.keys.contains(29))
        #expect(labels.count <= 5)
        #expect(labels[29] == "22 Nov")
    }

    @Test func longRunsOfMonthsLabelTheirJanuariesWithTheYear() {
        let first = Self.date(2023, 1, 1)
        let bins = (0..<47).map { StatsPeriod.month(Self.calendar.date(byAdding: .month, value: $0, to: first)!) }
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(Set(labels.values) == ["2023", "2024", "2025", "2026"])
    }

    @Test func overdueAndLaterAreAlwaysLabelled() {
        let bins: [StatsPeriod] = [.overdue] + (1...24).map { _ in .bucket("x") } + [.later]
        let labels = Self.format.labelledBins(bins, maximum: 4)
        #expect(labels[0] == "Overdue")
        #expect(labels[25] == "Later")
    }

    @Test func compactHourAxesKeepEvenHours() {
        let bins = (0..<24).map(StatsPeriod.hour)
        let labels = Self.format.labelledBins(bins, maximum: 12)
        #expect(labels.keys.allSatisfy { $0.isMultiple(of: 2) })
        #expect(labels.count == 12)
    }

    // MARK: Accessibility and Copy Values

    @Test func binsAreGroupedInRunsOfTen() {
        let bins = (0..<25).map { StatsPeriod.day(Self.calendar.date(byAdding: .day, value: $0 - 24, to: Self.now)!) }
        let data = StatsChartData(bins: bins, series: [])
        let runs = Self.format.runs(data)
        #expect(runs.map(\.bins) == [0..<10, 10..<20, 20..<25])
        #expect(runs[0].label == "29 Oct to 7 Nov")
        let single = Self.format.runs(StatsChartData(bins: [.overdue], series: []))
        #expect(single.map(\.label) == ["Overdue"])
    }

    @Test func theCalloutTextNamesThePeriodAndEverySeries() {
        let data = StatsChartData(
            bins: [.day(Self.date(2026, 11, 17))],
            series: [
                StatsSeries(id: "m", name: "Mature", color: .mature, mark: .stackedBar, values: [70]),
                StatsSeries(id: "y", name: "Young", color: .young, mark: .stackedBar, values: [nil]),
            ],
            calloutLines: [StatsCalloutLine(id: "t", name: "Total by then", values: [1_204])]
        )
        #expect(Self.format.calloutText(data, bin: 0) == "17 Nov, Mature 70, Total by then 1,204")
    }

    @Test func copyValuesIsATabSeparatedRowPerBin() {
        let data = StatsChartData(
            bins: [.bucket("a"), .bucket("b")],
            series: [StatsSeries(id: "m", name: "Mature", color: .mature, mark: .stackedBar, values: [1_000, nil])]
        )
        #expect(Self.format.tabSeparatedValues(data) == "Period\tMature\na\t1,000\nb\t")
    }

    @Test func aSkeletonKeepsTheLegendAndDropsTheValues() {
        let samples = StatsChartSamples(period: .month, calendar: Self.calendar, now: Self.now)
        let skeleton = samples.retention.skeleton()
        #expect(skeleton.bins.isEmpty)
        #expect(skeleton.legendNames == ["Retention", "Desired retention"])
        #expect(skeleton.series.allSatisfy { $0.values.isEmpty })
    }

    @Test func samplePeriodsHaveTheBinCountsOfS41() {
        func count(_ period: StatsSamplePeriod) -> Int { StatsChartSamples(period: period, calendar: Self.calendar, now: Self.now).bins.count }
        #expect(count(.month) == 30)
        #expect(count(.threeMonths) == 13)
        #expect(count(.sixMonths) == 26)
        #expect(count(.year) == 52)
        #expect(count(.all) == 23)
    }

    // MARK: The box keeps its size (S8.5, S10.3)

    #if os(macOS)
    @MainActor
    @Test(arguments: [StatsPlotSize.lead, .regular])
    func aBoxKeepsItsSizeInEveryPhase(size: StatsPlotSize) {
        let samples = StatsChartSamples(period: .year, calendar: Self.calendar, now: Self.now)
        func height(_ phase: StatsBoxPhase) -> CGFloat {
            let box = StatsChartBox(
                box: "sample", title: "Reviews Over Time",
                figure: StatsBoxFigure(value: "41,233 reviews", sentence: "23 days in a row"),
                data: samples.reviewsByType(browsable: false), phase: phase, size: size,
                figures: [.init(label: "Days Studied", value: "318 of 330"), .init(label: "Average a Day", value: "125")],
                reservesFootnote: true, open: {}
            )
            .frame(width: 520)
            .environment(\.locale, Self.locale)
            let host = NSHostingView(rootView: box)
            return host.fittingSize.height
        }
        let ready = height(.ready)
        #expect(ready > 0)
        for phase in [StatsBoxPhase.loading(fraction: nil), .loading(fraction: 0.4), .empty("No reviews in the last month"), .failed] {
            #expect(height(phase) == ready, "\(phase) against ready \(ready)")
        }
    }
    #endif
}
