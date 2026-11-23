import AppModel
import Charts
import DesignSystem
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// A chart's selection (S8.3): the bin under the pointer or the finger, and
/// whether a click, a touch or the keyboard pinned it. A caller that shares one
/// selection between plots (Time of Day's two panels) or clears it (a switch, the
/// period, a click outside) holds it; otherwise the plot keeps its own.
public struct StatsChartSelection: Hashable, Sendable {
    public var bin: Int?
    public var pinned: Bool
    /// The segment of the bin's stacked bar, where a box selects one (Answers' buttons, S7.10);
    /// nil selects the whole bin.
    public var segment: Int?

    public init(bin: Int? = nil, pinned: Bool = false, segment: Int? = nil) {
        self.bin = bin
        self.pinned = pinned
        self.segment = segment
    }

    public static let none = StatsChartSelection()

    /// Whether losing the plot's focus unpins: only a ready plot that lost it
    /// to a click elsewhere (S8.3), never one a loading phase made unfocusable.
    public static func unpinsOnFocusLoss(focused: Bool, pinned: Bool, ready: Bool) -> Bool {
        !focused && pinned && ready
    }

    /// The selection carried from the last ready bins to new ones (S10.2):
    /// kept while the selected period is still there (at its new index),
    /// cleared when it is gone; never out of range. Empty new bins (a loading
    /// phase) change nothing.
    public func carried(from old: [StatsPeriod], to new: [StatsPeriod]) -> StatsChartSelection {
        guard !new.isEmpty, let bin else { return self }
        if old.isEmpty || old == new {
            return new.indices.contains(bin) ? self : .none
        }
        guard old.indices.contains(bin), let moved = new.firstIndex(of: old[bin]) else { return .none }
        var carried = self
        carried.bin = moved
        return carried
    }
}

/// Whether the page is scrolling (R2078): a plain reference, not observed, so setting it redraws
/// nothing. While the page moves under a resting pointer, a hover would select a new bar every few
/// frames and redraw that chart each time; hover waits until the page stops instead.
public nonisolated final class StatsScrolling: @unchecked Sendable {
    /// Written and read on the main actor only (the page's scroll phase, the plots' hover).
    public nonisolated(unsafe) var active = false
    public init() {}

    /// A plot outside the page never scrolls.
    static let never = StatsScrolling()
}

extension EnvironmentValues {
    /// The page's scrolling state.
    @Entry var statsScrolling = StatsScrolling.never
}

/// How tall a box's plot is (N25.4: a fixed height per box size). The plot
/// keeps it at every text size (S15).
public enum StatsPlotSize: Hashable, Sendable {
    /// A section's lead box, which spans its row.
    case lead
    /// The other boxes.
    case regular
    /// A chart's own page (N25.5's taller plot).
    case page
    /// One of two aligned panels in a regular box (Time of Day, S7.11).
    case panel

    var height: CGFloat {
        switch self {
        case .lead: 200
        case .regular: 150
        case .page: 320
        case .panel: 84
        }
    }

    /// About how many axis labels fit under it.
    var axisLabels: Int {
        switch self {
        case .lead, .page: 7
        case .regular, .panel: 4
        }
    }
}

/// What a plot draws beyond its marks, for a box that puts two plots together (Time of Day's
/// panels, S7.11): the upper panel has no legend, x axis labels, callout or focus of its own, and
/// the lower one speaks for both.
struct StatsPlotOptions: Hashable {
    /// The system legend under the plot.
    var legend = true
    /// The x axis labels.
    var xAxis = true
    /// The selection's callout beside its rule.
    var callout = true
    /// A stop for Tab and the plot keys (S14).
    var focusable = true
    /// The chart element, its Audio Graph and its per-bar elements (S15).
    var accessible = true
    /// A width every y axis label keeps, so two panels' plots share their x positions.
    var yLabelWidth: CGFloat?

    static let standard = StatsPlotOptions()
}

/// What a plot shows besides its marks.
enum StatsPlotPhase: Hashable {
    case ready
    /// S10.3: the system's small progress indicator, determinate with a fraction.
    case loading(Double?)
    /// S8.5: one line that says why, and an optional second one.
    case message(String, String?)
}

/// The plot of a box (N25.4 part 3): Swift Charts, the y axis on the trailing
/// side, one axis only (S8.2), the selection and its callout (S8.3), the plot
/// keys (S14), the accessibility elements (S15) and the system legend under it.
struct StatsChartPlot: View {
    /// The box's id, for the callout button's identifier.
    let box: String
    let data: StatsChartData
    let phase: StatsPlotPhase
    let size: StatsPlotSize
    /// The box's sentence: the chart element's label (S15).
    let summary: String
    @Binding var selection: StatsChartSelection
    /// Runs Show in Browse for a bin; nil when the caller offers none.
    let showInBrowse: ((Int) -> Void)?
    /// Draws a hairline between stacked series; nil follows Differentiate Without Colour,
    /// the only time a stack is drawn apart (a stack otherwise reads as one flat bar or band).
    let separatesSeries: Bool?
    let options: StatsPlotOptions
    /// Worked out once per data, not on every pointer move.
    private let isReady: Bool
    private let domain: ClosedRange<Double>
    private let segments: [StatsSegment]
    /// Per bin, the last stacked series with a value: the bar's top segment.
    private let topBarSeries: [Int: Int]
    /// The last stacked series: the top band of an area.
    private let topSeries: Int?

    init(
        box: String, data: StatsChartData, phase: StatsPlotPhase, size: StatsPlotSize, summary: String,
        selection: Binding<StatsChartSelection>, showInBrowse: ((Int) -> Void)?, separatesSeries: Bool?,
        options: StatsPlotOptions = .standard
    ) {
        self.options = options
        self.box = box
        self.data = data
        self.phase = phase
        self.size = size
        self.summary = summary
        _selection = selection
        self.showInBrowse = showInBrowse
        self.separatesSeries = separatesSeries
        let ready = phase == .ready && !data.bins.isEmpty
        isReady = ready
        domain = ready ? data.yDomain : 0...1
        let segments = ready ? data.stackedSegments() : []
        self.segments = segments
        topBarSeries = Dictionary(grouping: segments.filter { $0.upper > $0.lower }, by: \.bin).mapValues { $0.map(\.series).max() ?? 0 }
        topSeries = data.series.indices.last { data.series[$0].isStacked }
    }

    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiateWithoutColor
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    @Environment(\.statsScrolling) private var scrolling
    @FocusState private var focused: Bool
    /// The bins of the last ready data, kept across a loading phase.
    @State private var lastReadyBins: [StatsPeriod] = []

    private var format: StatsChartFormat { StatsChartFormat(locale: locale, calendar: calendar, now: clock.now()) }
    private var separated: Bool { separatesSeries ?? differentiateWithoutColor }

    /// The separator between two stacked segments with Differentiate Without Colour, in data
    /// units: about 1.5 points of the plot's fixed height (N25.12's separating lines).
    private var segmentGap: Double { (domain.upperBound - domain.lowerBound) * 1.5 / size.height }

    var body: some View {
        chart
            .chartPlotStyle { $0.frame(height: size.height) }
            .chartYScale(domain: domain)
            .chartXScale(domain: binIDs)
            .chartForegroundStyleScale(domain: data.legendNames, range: legendColors)
            .chartLegend(position: .bottom, alignment: .leading)
            .chartLegend(options.legend ? .automatic : .hidden)
            .chartXAxis { xAxis }
            .chartXAxis(options.xAxis ? .automatic : .hidden)
            .chartYAxis { yAxis }
            .chartOverlay { proxy in overlay(proxy) }
            #if os(macOS)
            // Not `.focusable()`'s click-to-focus (edit) interaction: on the Mac it takes the
            // mouse-down, so the plot's own click never pinned a bar (seen in the VM: a click
            // focused the plot and cleared the hover, and no tap gesture fired). The plot is
            // reached by Tab with keyboard navigation on, as buttons are, and a click focuses
            // it from its tap handler.
            .focusable(isReady && options.focusable, interactions: .activate)
            #else
            .focusable(isReady && options.focusable)
            #endif
            .focused($focused)
            .onKeyPress(.leftArrow) { move(-1) }
            .onKeyPress(.rightArrow) { move(1) }
            .onKeyPress(.escape) { clear() }
            .onKeyPress(.return) { browseSelected() }
            .onChange(of: data.bins) { _, new in
                // Compared with the last ready bins, not a loading phase's
                // empty ones: the selected period is kept where it still
                // exists and cleared where it does not (S10.2).
                guard !new.isEmpty else { return }
                let carried = selection.carried(from: lastReadyBins, to: new)
                if carried != selection { selection = carried }
                lastReadyBins = new
            }
            .onAppear { if !data.bins.isEmpty { lastReadyBins = data.bins } }
            #if os(macOS)
            .onChange(of: focused) {
                // A click elsewhere that takes the focus unpins (S8.3); a
                // loading phase, which makes the plot unfocusable, does not.
                if StatsChartSelection.unpinsOnFocusLoss(focused: focused, pinned: selection.pinned, ready: isReady) {
                    selection = .none
                }
            }
            #endif
            .accessibilityElement(children: .contain)
            .accessibilityLabel(summary)
            // The pinned period (S8.3), as the callout names it.
            .accessibilityValue(Text(verbatim: pinnedName))
            .accessibilityIdentifier("stats.box.\(box).plot")
            .accessibilityChartDescriptor(StatsAudioGraph(data: isReady ? data : data.skeleton(), format: format, summary: summary))
            .accessibilityHidden(!options.accessible)
    }

    /// The pinned bin's name, empty with none (or while the box is not ready).
    private var pinnedName: String {
        guard isReady, selection.pinned, let bin = selection.bin, data.bins.indices.contains(bin) else { return "" }
        return format.name(data.bins[bin])
    }

    // MARK: Marks

    /// Bin ids: categories, so bars sit in bands and lines join their centres.
    private var binIDs: [String] {
        isReady ? data.bins.indices.map(Self.id) : [Self.placeholderID]
    }

    private static let placeholderID = " "
    static func id(_ bin: Int) -> String { "b\(bin)" }
    static func bin(_ id: String) -> Int? { id.hasPrefix("b") ? Int(id.dropFirst()) : nil }

    private var legendColors: [Color] {
        data.series.map(\.color.color) + data.rules.map(\.color.color) + data.markers.map(\.color.color)
    }

    private var fadedOpacity: Double { contrast == .increased ? 0.5 : 0.3 }

    private func opacity(at bin: Int) -> Double {
        guard let selected = selection.bin else { return 1 }
        return selected == bin ? 1 : fadedOpacity
    }

    private var chart: some View {
        Chart {
            if isReady {
                stacked
                singles
                rules
                selectionRule
            } else {
                // No values yet: one invisible mark per legend entry keeps the
                // system legend and the axes in place (S10.3).
                ForEach(data.legendNames, id: \.self) { name in
                    PointMark(x: .value("Period", Self.placeholderID), y: .value("Value", 0))
                        .foregroundStyle(by: .value("Series", name))
                        .opacity(0)
                        .accessibilityHidden(true)
                }
            }
        }
    }

    @ChartContentBuilder private var stacked: some ChartContent {
        ForEach(segments, id: \.self) { segment in
            let series = data.series[segment.series]
            switch series.mark {
            case .stackedArea:
                // Every bin, zeros included, so a band never joins across a
                // zero. The bands touch; with Differentiate Without Colour each
                // band but the top keeps a separator above it.
                let gap = separated && segment.series != topSeries ? min(segmentGap, (segment.upper - segment.lower) / 2) : 0
                AreaMark(
                    x: .value("Period", Self.id(segment.bin)),
                    yStart: .value("From", segment.lower),
                    yEnd: .value("To", max(segment.lower, segment.upper - gap)),
                    series: .value("Series", series.name)
                )
                .foregroundStyle(by: .value("Series", series.name))
                .accessibilityHidden(true)
            default:
                if segment.upper > segment.lower {
                    // The segments of a stack touch with square corners, so the
                    // stack reads as one bar; only Differentiate Without Colour
                    // separates them (never more than half a thin segment, so it
                    // never vanishes).
                    let gap = separated && topBarSeries[segment.bin] != segment.series ? min(segmentGap, (segment.upper - segment.lower) / 2) : 0
                    BarMark(
                        x: .value("Period", Self.id(segment.bin)),
                        yStart: .value("From", segment.lower),
                        yEnd: .value("To", max(segment.lower, segment.upper - gap)),
                        width: barWidth
                    )
                    .cornerRadius(0)
                    .foregroundStyle(by: .value("Series", series.name))
                    .opacity(opacity(at: segment.bin))
                    .accessibilityHidden(true)
                }
            }
        }
        // Reviews Due's Overdue bar is labelled on the plot with its count (S7.9).
        ForEach(data.bins.indices.filter { data.bins[$0] == .overdue && data.stackTotal(at: $0) > 0 }, id: \.self) { bin in
            RuleMark(x: .value("Period", Self.id(bin)), yStart: .value("From", 0), yEnd: .value("To", data.stackTotal(at: bin)))
                // Clear, not transparent: an invisible mark would hide its annotation too.
                .foregroundStyle(Color.clear)
                .annotation(position: .top, spacing: Spacing.xxSmall) {
                    Text(verbatim: format.count(data.stackTotal(at: bin)))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .accessibilityHidden(true)
        }
    }

    @ChartContentBuilder private var singles: some ChartContent {
        ForEach(data.series.filter { $0.mark == .bar }) { series in
            ForEach(Array(series.values.enumerated()).filter { $0.element != nil }, id: \.offset) { bin, value in
                // Flat, as the stacked bars are (one bar style on the page).
                BarMark(x: .value("Period", Self.id(bin)), y: .value("Value", value ?? 0), width: barWidth)
                    .cornerRadius(0)
                    .foregroundStyle(by: .value("Series", series.name))
                    .opacity(opacity(at: bin))
                    .accessibilityHidden(true)
            }
        }
        ForEach(data.series.filter(\.isLine)) { series in
            if case let .line(dashed, points) = series.mark {
                // A line of fewer than three values shows its points (S8.5).
                lineContent(series, dashed: dashed, points: points || series.values.count(where: { $0 != nil }) < 3)
            }
        }
    }

    /// A line: a nil value breaks it; an open point is drawn as a ring and left out (S7.2).
    @ChartContentBuilder private func lineContent(_ series: StatsSeries, dashed: Bool, points: Bool) -> some ChartContent {
        let pieces = Self.linePieces(series)
        ForEach(pieces, id: \.self) { piece in
            ForEach(piece.bins, id: \.self) { bin in
                LineMark(
                    x: .value("Period", Self.id(bin)),
                    y: .value("Value", series.values[bin] ?? 0),
                    series: .value("Piece", "\(series.id)#\(piece.index)")
                )
                .foregroundStyle(by: .value("Series", series.name))
                .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round, dash: dashed ? [4, 3] : []))
                .accessibilityHidden(true)
                if points || piece.bins.count == 1 {
                    PointMark(x: .value("Period", Self.id(bin)), y: .value("Value", series.values[bin] ?? 0))
                        .foregroundStyle(by: .value("Series", series.name))
                        .symbolSize(30)
                        .accessibilityHidden(true)
                }
            }
        }
        ForEach(Array(series.openPoints).filter { series.values.indices.contains($0) && series.values[$0] != nil }, id: \.self) { bin in
            PointMark(x: .value("Period", Self.id(bin)), y: .value("Value", series.values[bin] ?? 0))
                .symbol { StatsOpenPoint(color: series.color.color) }
                .accessibilityHidden(true)
        }
    }

    struct LinePiece: Hashable {
        var index: Int
        var bins: [Int]
    }

    /// The runs of a line between the bins with no value; open points are left
    /// out without breaking the line.
    static func linePieces(_ series: StatsSeries) -> [LinePiece] {
        var pieces: [LinePiece] = []
        var current: [Int] = []
        for (bin, value) in series.values.enumerated() {
            if value == nil {
                if !current.isEmpty { pieces.append(LinePiece(index: pieces.count, bins: current)) }
                current = []
            } else if !series.openPoints.contains(bin) {
                current.append(bin)
            }
        }
        if !current.isEmpty { pieces.append(LinePiece(index: pieces.count, bins: current)) }
        return pieces
    }

    @ChartContentBuilder private var rules: some ChartContent {
        ForEach(data.rules) { rule in
            RuleMark(y: .value("Value", rule.value))
                .foregroundStyle(by: .value("Series", rule.name))
                .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                .accessibilityHidden(true)
        }
    }

    @ChartContentBuilder private var selectionRule: some ChartContent {
        if let bin = selection.bin, data.bins.indices.contains(bin) {
            RuleMark(x: .value("Period", Self.id(bin)))
                .foregroundStyle(Color.secondary)
                .lineStyle(StrokeStyle(lineWidth: 1))
                .accessibilityHidden(true)
        }
    }

    /// Bars fill their band; with fewer than three bins holding data they keep a
    /// fixed width instead (S8.5).
    private var barWidth: MarkDimension {
        data.isSparse ? .fixed(18) : .ratio(0.8)
    }

    // MARK: Axes

    @AxisContentBuilder private var xAxis: some AxisContent {
        if isReady {
            // Fewer labels at the accessibility sizes, so they never collide.
            let labels = format.labelledBins(data.bins, maximum: dynamicTypeSize.isAccessibilitySize ? max(2, size.axisLabels / 2 - 1) : size.axisLabels)
            AxisMarks(values: labels.keys.sorted().map(Self.id)) { value in
                AxisValueLabel {
                    if let id = value.as(String.self), let bin = Self.bin(id), let label = labels[bin] {
                        Text(verbatim: label)
                    }
                }
            }
        } else {
            // A blank label keeps the axis row's height (S8.5: the box keeps its size).
            AxisMarks(values: [Self.placeholderID]) { _ in
                AxisValueLabel { Text(verbatim: " ") }
            }
        }
    }

    @AxisContentBuilder private var yAxis: some AxisContent {
        if isReady {
            AxisMarks(position: .trailing, values: yAxisValues) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let number = value.as(Double.self) {
                        Text(verbatim: format.axisValue(number, unit: data.unit))
                            .frame(minWidth: options.yLabelWidth, alignment: .leading)
                    }
                }
            }
        } else {
            AxisMarks(position: .trailing, values: [0.0]) { _ in
                AxisValueLabel { Text(verbatim: " ").frame(minWidth: options.yLabelWidth, alignment: .leading) }
            }
        }
    }

    /// About four values (three in a small box); whole numbers on a small
    /// count axis, never "0.5 reviews".
    private var yAxisValues: AxisMarkValues {
        if data.unit == .count, domain.upperBound <= 10 {
            let step = domain.upperBound <= 5 ? 1.0 : 2.0
            return .stride(by: step)
        }
        return .automatic(desiredCount: size == .regular ? 3 : 4)
    }

    // MARK: Selection and the callout

    @ViewBuilder private func overlay(_ proxy: ChartProxy) -> some View {
        GeometryReader { geometry in
            if let anchor = proxy.plotFrame {
                let plot = geometry[anchor]
                ZStack(alignment: .topLeading) {
                    if isReady {
                        markers(proxy, plot: plot)
                        interactionLayer(proxy, plot: plot)
                        if options.accessible {
                            StatsAccessibilityBins(data: data, format: format, browse: showInBrowse)
                                .equatable()
                                .frame(width: plot.width, height: plot.height)
                                .offset(x: plot.minX, y: plot.minY)
                        }
                        if options.callout {
                            callout(proxy, plot: plot, in: geometry.size)
                        }
                    } else {
                        phaseView
                            .frame(width: plot.width, height: plot.height)
                            .offset(x: plot.minX, y: plot.minY)
                    }
                }
            }
        }
    }

    /// The markers (Retrievability's desired retention): a dashed vertical rule in the label
    /// colour across the plot, between bins where the value falls inside one.
    @ViewBuilder private func markers(_ proxy: ChartProxy, plot: CGRect) -> some View {
        if !data.markers.isEmpty, let first = proxy.position(forX: Self.id(0)) {
            let step = data.bins.count > 1 ? (proxy.position(forX: Self.id(1)) ?? first) - first : plot.width
            ForEach(data.markers) { marker in
                if let position = marker.position {
                    let x = plot.minX + CGFloat(StatsMarker.x(position, firstCenter: Double(first), step: Double(step)))
                    Path { path in
                        path.move(to: CGPoint(x: x, y: plot.minY))
                        path.addLine(to: CGPoint(x: x, y: plot.maxY))
                    }
                    .stroke(marker.color.color, style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
            }
        }
    }

    @ViewBuilder private var phaseView: some View {
        switch phase {
        case let .loading(fraction):
            Group {
                if let fraction {
                    ProgressView(value: min(max(fraction, 0), 1))
                        .frame(maxWidth: 160)
                } else {
                    ProgressView()
                }
            }
            .controlSize(.small)
        case let .message(text, detail):
            VStack(spacing: Spacing.xxSmall) {
                Text(text)
                    .font(.subheadline)
                if let detail {
                    Text(detail)
                        .font(.footnote)
                }
            }
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .padding(.horizontal, Spacing.medium)
        case .ready:
            // Ready with no bins: nothing to draw yet.
            EmptyView()
        }
    }

    private func bin(at x: CGFloat, proxy: ChartProxy, plot: CGRect) -> Int? {
        guard let id: String = proxy.value(atX: min(max(x, plot.minX), plot.maxX - 1) - plot.minX, as: String.self) else { return nil }
        return Self.bin(id)
    }

    private func interactionLayer(_ proxy: ChartProxy, plot: CGRect) -> some View {
        Rectangle()
            .fill(.clear)
            .contentShape(Rectangle())
            .frame(width: plot.width, height: plot.height)
            .offset(x: plot.minX, y: plot.minY)
            #if os(macOS)
            // Mac: the pointer shows the callout; a click pins it; a double-click
            // runs Show in Browse (S8.3, S8.4).
            .onContinuousHover(coordinateSpace: .local) { phase in
                guard !selection.pinned, !scrolling.active else { return }
                switch phase {
                case let .active(location):
                    let hovered = bin(at: location.x + plot.minX, proxy: proxy, plot: plot)
                    if hovered != selection.bin { selection.bin = hovered }
                case .ended:
                    if selection.bin != nil { selection.bin = nil }
                }
            }
            .gesture(SpatialTapGesture(count: 2, coordinateSpace: .local).onEnded { value in
                guard let bin = bin(at: value.location.x + plot.minX, proxy: proxy, plot: plot) else { return }
                selection = StatsChartSelection(bin: bin, pinned: true)
                browse(bin)
            })
            .simultaneousGesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                selection = StatsChartSelection(bin: bin(at: value.location.x + plot.minX, proxy: proxy, plot: plot), pinned: true)
                focused = true
            })
            #else
            // Touch: hold, then drag along the plot. A UIKit long press, so the
            // scroll view's pan wins a swipe that moves first and the press
            // wins a finger held still (S8.3: a plain swipe still scrolls).
            // The selection stays after the finger lifts, so the callout's
            // button can be tapped; a short tap clears it.
            .gesture(StatsHoldGesture { location in
                selection = StatsChartSelection(bin: bin(at: location.x + plot.minX, proxy: proxy, plot: plot), pinned: true)
            })
            .simultaneousGesture(TapGesture().onEnded { selection = .none })
            #endif
            .accessibilityHidden(true)
    }

    @ViewBuilder private func callout(_ proxy: ChartProxy, plot: CGRect, in size: CGSize) -> some View {
        if let bin = selection.bin, data.bins.indices.contains(bin), let x = proxy.position(forX: Self.id(bin)) {
            let ruleX = plot.minX + x
            let leading = ruleX > plot.midX
            StatsChartCallout(
                box: box,
                title: format.name(data.bins[bin]),
                rows: format.calloutRows(data, bin: bin),
                showInBrowse: data.browsable.contains(bin) && showInBrowse != nil && selection.pinned ? { browse(bin) } : nil
            )
            .fixedSize()
            .alignmentGuide(.leading) { dimensions in
                // Beside the rule, never over it: to its right in the left half,
                // to its left in the right half, kept inside the chart.
                let width = dimensions.width
                let gap = Spacing.xSmall
                let origin = leading ? ruleX - gap - width : ruleX + gap
                return -min(max(origin, 0), max(0, size.width - width))
            }
            .alignmentGuide(.top) { _ in -plot.minY }
            // A hovering pointer passes through, so the callout never takes the
            // hover from the plot under it; a pinned one takes its button's click.
            .allowsHitTesting(selection.pinned)
            .accessibilityHidden(true)
        }
    }

    // MARK: Keys

    private func move(_ step: Int) -> KeyPress.Result {
        guard isReady else { return .ignored }
        let last = data.bins.count - 1
        let next = selection.bin.map { $0 + step } ?? (step > 0 ? 0 : last)
        selection = StatsChartSelection(bin: min(max(next, 0), last), pinned: true)
        return .handled
    }

    private func clear() -> KeyPress.Result {
        guard selection != .none else { return .ignored }
        selection = .none
        return .handled
    }

    private func browseSelected() -> KeyPress.Result {
        guard let bin = selection.bin, data.browsable.contains(bin), showInBrowse != nil else { return .ignored }
        browse(bin)
        return .handled
    }

    private func browse(_ bin: Int) {
        guard data.browsable.contains(bin) else { return }
        showInBrowse?(bin)
    }
}

#if os(iOS)
/// A hold of 0.25 s, then every move until the finger lifts, in the plot's
/// coordinates: UIKit's long press, which the scroll view's pan fails against
/// a still finger and beats on a swipe.
struct StatsHoldGesture: UIGestureRecognizerRepresentable {
    let changed: (CGPoint) -> Void

    func makeUIGestureRecognizer(context: Context) -> UILongPressGestureRecognizer {
        let recognizer = UILongPressGestureRecognizer()
        recognizer.minimumPressDuration = 0.25
        return recognizer
    }

    func handleUIGestureRecognizerAction(_ recognizer: UILongPressGestureRecognizer, context: Context) {
        switch recognizer.state {
        case .began, .changed: changed(context.converter.localLocation)
        default: break
        }
    }
}
#endif

/// An open point (S7.2): a ring in the series' colour on the box's background.
struct StatsOpenPoint: View {
    let color: Color

    var body: some View {
        Circle()
            .strokeBorder(color, lineWidth: 1.5)
            .frame(width: 7, height: 7)
    }
}
