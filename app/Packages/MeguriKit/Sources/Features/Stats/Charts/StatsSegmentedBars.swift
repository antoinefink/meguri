import AppModel
import DesignSystem
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// One full-width horizontal bar divided into flat segments (Card Counts' states, S7.7; an
/// Answers group's buttons, S7.10).
struct StatsBarRow: Identifiable, Hashable {
    let id: String
    let name: String
    let segments: [Segment]

    struct Segment: Identifiable, Hashable {
        let id: String
        let name: String
        let color: StatsSeriesColor
        let value: Double
        /// The callout's value: the count and its share of the bar ("12 (5.0%)").
        let text: String
    }

    var total: Double { segments.reduce(0) { $0 + max(0, $1.value) } }

    /// Each segment's leading edge and width as fractions of the bar, from the leading edge.
    var spans: [(start: Double, width: Double)] {
        let total = total
        var start = 0.0
        return segments.map { segment in
            let width = total > 0 ? max(0, segment.value) / total : 0
            defer { start += width }
            return (start, width)
        }
    }

    /// The segment under a point a fraction of the bar's width from its leading edge, or nil
    /// past the bar or over an empty one.
    func segment(at fraction: Double) -> Int? {
        guard total > 0, fraction >= 0, fraction <= 1 else { return nil }
        let spans = spans
        return spans.indices.last { spans[$0].width > 0 && spans[$0].start <= fraction } ?? spans.indices.first { spans[$0].width > 0 }
    }

    /// The bar's reading for VoiceOver: its name, then each segment's name and value.
    var spoken: String {
        ([name] + segments.map { "\($0.name) \($0.text)" }).filter { !$0.isEmpty }.joined(separator: ", ")
    }
}

/// The selection of a segment among bars: the next one with a value in reading order, for the
/// plot keys (S14), wrapping neither way.
enum StatsBarSelection {
    static func moved(_ selection: StatsChartSelection, by step: Int, in rows: [StatsBarRow]) -> StatsChartSelection {
        let order = rows.indices.flatMap { row in rows[row].segments.indices.filter { rows[row].segments[$0].value > 0 }.map { (row, $0) } }
        guard !order.isEmpty else { return selection }
        let current = order.firstIndex { $0.0 == selection.bin && $0.1 == selection.segment }
        let next = current.map { min(max($0 + step, 0), order.count - 1) } ?? (step > 0 ? 0 : order.count - 1)
        return StatsChartSelection(bin: order[next].0, pinned: true, segment: order[next].1)
    }
}

/// Horizontal bars of flat segments that touch (N25.4: one ordinary bar; a separator only with
/// Differentiate Without Colour), each optionally named on its leading side. The pointer over a
/// segment shows its callout and a click pins it (a tap on touch); the other segments fade, as
/// a chart's other bars do; Left and Right move a pinned segment, Esc clears it (S8.3, S14). The
/// bars are one chart element with an Audio Graph, each bar one element inside it (S15).
struct StatsSegmentedBars: View {
    let box: String
    let rows: [StatsBarRow]
    let showsNames: Bool
    @Binding var selection: StatsChartSelection
    /// The chart element's label (S15).
    let summary: String
    /// The Audio Graph's data: a category per bar or per segment, as the box reads it.
    let audioGraph: StatsChartData

    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiateWithoutColor
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    @Environment(\.statsScrolling) private var scrolling
    /// A bar's height; the plot keeps it at every text size (S15).
    static let barHeight: CGFloat = 18
    private var barHeight: CGFloat { Self.barHeight }
    @FocusState private var focused: Bool

    private var format: StatsChartFormat { StatsChartFormat(locale: locale, calendar: calendar, now: clock.now()) }

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: Spacing.small, verticalSpacing: Spacing.xSmall) {
            ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                GridRow {
                    if showsNames {
                        Text(verbatim: row.name)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .gridColumnAlignment(.leading)
                    }
                    bar(row, index: index)
                        .frame(maxWidth: .infinity)
                        .zIndex(selection.bin == index ? 1 : 0)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(verbatim: row.spoken))
                .accessibilityIdentifier("stats.box.\(box).bar.\(row.id)")
            }
        }
        #if os(macOS)
        .focusable(true, interactions: .activate)
        #else
        .focusable(true)
        #endif
        .focused($focused)
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .onKeyPress(.escape) { clear() }
        #if os(macOS)
        .onChange(of: focused) {
            if StatsChartSelection.unpinsOnFocusLoss(focused: focused, pinned: selection.pinned, ready: true) {
                selection = .none
            }
        }
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(verbatim: summary))
        .accessibilityIdentifier("stats.box.\(box).plot")
        .accessibilityChartDescriptor(StatsAudioGraph(data: audioGraph, format: format, summary: summary))
    }

    private var fadedOpacity: Double { contrast == .increased ? 0.5 : 0.3 }

    private func opacity(row: Int, segment: Int) -> Double {
        guard let bin = selection.bin, let selected = selection.segment else { return 1 }
        return bin == row && selected == segment ? 1 : fadedOpacity
    }

    private func bar(_ row: StatsBarRow, index: Int) -> some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let spans = row.spans
            let last = spans.indices.last { spans[$0].width > 0 }
            ZStack(alignment: .topLeading) {
                if row.total <= 0 {
                    Rectangle().fill(.quaternary)
                } else {
                    ForEach(Array(row.segments.enumerated()), id: \.element.id) { segment, item in
                        let span = spans[segment]
                        if span.width > 0 {
                            // About 1.5 points between segments with Differentiate Without Colour only.
                            let gap: CGFloat = differentiateWithoutColor && segment != last ? min(1.5, span.width * width / 2) : 0
                            Rectangle()
                                .fill(item.color.color)
                                .opacity(opacity(row: index, segment: segment))
                                .frame(width: max(0, span.width * width - gap))
                                .offset(x: span.start * width)
                        }
                    }
                }
                if selection.bin == index, let selected = selection.segment, row.segments.indices.contains(selected) {
                    callout(row, segment: selected, span: spans[selected], width: width)
                }
            }
            .frame(width: width, height: barHeight, alignment: .topLeading)
            .contentShape(Rectangle())
            #if os(macOS)
            .onContinuousHover(coordinateSpace: .local) { phase in
                guard !selection.pinned, !scrolling.active else { return }
                switch phase {
                case let .active(location):
                    let segment = row.segment(at: location.x / max(width, 1))
                    let next = segment.map { StatsChartSelection(bin: index, pinned: false, segment: $0) } ?? .none
                    if next != selection { selection = next }
                case .ended:
                    if selection.bin == index { selection = .none }
                }
            }
            .gesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                guard let segment = row.segment(at: value.location.x / max(width, 1)) else { return }
                selection = StatsChartSelection(bin: index, pinned: true, segment: segment)
                focused = true
            })
            #else
            .gesture(SpatialTapGesture(coordinateSpace: .local).onEnded { value in
                let segment = row.segment(at: value.location.x / max(width, 1))
                let next = segment.map { StatsChartSelection(bin: index, pinned: true, segment: $0) } ?? .none
                selection = next == selection ? .none : next
            })
            #endif
        }
        .frame(height: barHeight)
    }

    /// The callout under the bar, centred on its segment and kept inside the bar's width; it
    /// passes the pointer through, so it never takes the hover from the bar.
    private func callout(_ row: StatsBarRow, segment: Int, span: (start: Double, width: Double), width: CGFloat) -> some View {
        let item = row.segments[segment]
        let mid = (span.start + span.width / 2) * width
        let below = barHeight + Spacing.xxSmall
        return StatsChartCallout(box: box, title: row.name.isEmpty ? item.name : row.name,
                                 rows: [(name: item.name, value: item.text, color: Optional(item.color))], showInBrowse: nil)
            .fixedSize()
            .alignmentGuide(.leading) { dimensions in
                -min(max(mid - dimensions.width / 2, 0), max(0, width - dimensions.width))
            }
            .alignmentGuide(.top) { _ in -below }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }

    private func move(_ step: Int) -> KeyPress.Result {
        selection = StatsBarSelection.moved(selection, by: step, in: rows)
        return .handled
    }

    private func clear() -> KeyPress.Result {
        guard selection != .none else { return .ignored }
        selection = .none
        return .handled
    }
}
