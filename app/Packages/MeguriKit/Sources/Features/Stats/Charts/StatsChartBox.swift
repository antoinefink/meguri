import AppModel
import DesignSystem
import SwiftUI

/// What a box's data is doing (S8.5, S10.3, S12.8). The box draws every phase
/// at the same size: the title, the legend and the figure labels stay; only the
/// plot area and the values change.
public enum StatsBoxPhase: Hashable, Sendable {
    case ready
    /// Not read yet: the system's small progress indicator in the plot area,
    /// determinate when the core reports a fraction (S10.3).
    case loading(fraction: Double?)
    /// No data: one line in the plot area that says why (S8.5), and an
    /// optional second line ("This fills in as you study.", S7.1).
    case empty(String, detail: String? = nil)
    /// The box's part failed alone (S12.8): "Couldn't load this chart."
    case failed
}

/// The large figure and its sentence (N25.4 part 2).
public struct StatsBoxFigure: Hashable, Sendable {
    public var value: String
    public var sentence: String

    public init(value: String, sentence: String) {
        self.value = value
        self.sentence = sentence
    }
}

/// One of the two to four small figures (N25.4 part 5). Its label shows in every
/// phase; its value only when the box is ready.
public struct StatsSmallFigure: Identifiable, Hashable, Sendable {
    public var id: String { label }
    public var label: String
    public var value: String?

    public init(label: String, value: String?) {
        self.label = label
        self.value = value
    }
}

/// A box (contract N25.4): the header button with its chevron and trailing
/// switches or marker, the figure and its sentence, the plot with the system
/// legend, the small figures and the footnote. It keeps its size in every
/// phase (S8.5): the sentence keeps two lines, a box that can have a footnote
/// keeps its line, and the plot keeps its height at every text size (S15).
public struct StatsChartBox<Accessory: View>: View {
    private let box: String
    private let title: String
    private let accessory: Accessory
    private let figure: StatsBoxFigure?
    private let data: StatsChartData
    private let phase: StatsBoxPhase
    private let size: StatsPlotSize
    private let figures: [StatsSmallFigure]
    private let footnote: String?
    private let footnoteLines: Int
    private let separatesSeries: Bool?
    private let externalSelection: Binding<StatsChartSelection>?
    private let open: () -> Void
    private let showInBrowse: ((Int) -> Void)?

    @State private var ownSelection = StatsChartSelection.none
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.displayScale) private var displayScale
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    /// - Parameters:
    ///   - box: the box's id in `stats.box.<box>` (S15), e.g. `reviewsOverTime`.
    ///   - accessory: the header's trailing switches (small segmented controls)
    ///     or marker ("Now", "Estimated").
    ///   - selection: a selection the caller holds and clears (a switch, the
    ///     period, Time of Day's two panels); nil keeps the box's own.
    ///   - showInBrowse: runs Show in Browse for a bin `data.browsable` lists
    ///     (S8.4); nil offers none.
    public init(
        box: String, title: String, figure: StatsBoxFigure?, data: StatsChartData,
        phase: StatsBoxPhase = .ready, size: StatsPlotSize = .regular,
        figures: [StatsSmallFigure] = [], footnote: String? = nil, footnoteLines: Int = 0,
        separatesSeries: Bool? = nil, selection: Binding<StatsChartSelection>? = nil,
        open: @escaping () -> Void, showInBrowse: ((Int) -> Void)? = nil,
        @ViewBuilder accessory: () -> Accessory
    ) {
        self.box = box
        self.title = title
        self.accessory = accessory()
        self.figure = figure
        self.data = data
        self.phase = phase
        self.size = size
        self.figures = figures
        self.footnote = footnote
        self.footnoteLines = max(footnoteLines, footnote == nil ? 0 : 1)
        self.separatesSeries = separatesSeries
        self.externalSelection = selection
        self.open = open
        self.showInBrowse = showInBrowse
    }

    private var isReady: Bool { phase == .ready }
    private var stacks: Bool { dynamicTypeSize.isAccessibilitySize }
    private var selection: Binding<StatsChartSelection> { externalSelection ?? $ownSelection }

    private var plotPhase: StatsPlotPhase {
        switch phase {
        case .ready: .ready
        case let .loading(fraction): .loading(fraction)
        case let .empty(message, detail): .message(message, detail)
        case .failed: .message(String(localized: "stats.chart.failed", bundle: .module), nil)
        }
    }

    public var body: some View {
        container {
            VStack(alignment: .leading, spacing: Spacing.small) {
                header
                #if os(macOS)
                figureView
                plot
                    .contextMenu { if isReady { exportMenu } }
                #else
                // On touch a hold on the plot selects (S8.3), so Copy Image
                // and Copy Values are on a hold of the figure instead.
                figureView
                    .contextMenu { if isReady { exportMenu } }
                plot
                #endif
                if !figures.isEmpty { figuresView }
                if footnoteLines > 0 { footnoteView }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(box)")
    }

    // MARK: Parts

    private var header: some View {
        let layout = stacks
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: Spacing.xSmall))
        return layout {
            Button(action: open) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.xxSmall) {
                    Text(verbatim: title)
                        .font(.headline)
                        .foregroundStyle(.primary)
                        .multilineTextAlignment(.leading)
                    Image(systemName: "chevron.right")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(title)
            .accessibilityHint(Text("stats.chart.openHint", bundle: .module))
            .accessibilityIdentifier("stats.box.\(box).header")
            if !stacks { Spacer(minLength: Spacing.xSmall) }
            accessory
                .controlSize(.small)
        }
    }

    private var figureView: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: isReady ? figure?.value ?? "—" : "—")
                .font(.title2.weight(.semibold))
                .monospacedDigit()
                .foregroundStyle(isReady && figure != nil ? .primary : .secondary)
                .lineLimit(1)
            // Two lines kept in every phase, so the plot never moves down when
            // the numbers arrive (S8.5: the box keeps its size).
            Text(verbatim: isReady ? figure?.sentence ?? " " : " ")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(stacks ? 3 : 2, reservesSpace: true)
        }
        // The chart element reads the figure and its sentence (S15).
        .accessibilityHidden(true)
    }

    private var plot: some View {
        StatsChartPlot(
            box: box,
            data: isReady ? data : data.skeleton(),
            phase: plotPhase,
            size: size,
            summary: isReady ? [figure?.value, figure?.sentence].compactMap(\.self).joined(separator: ", ") : title,
            selection: selection,
            showInBrowse: showInBrowse,
            separatesSeries: separatesSeries
        )
    }

    /// The small figures in one row; two to a row at compact width when there
    /// are four (decided by width class and count, never by the values, so the
    /// box keeps its size when they arrive); one column at the accessibility sizes.
    @ViewBuilder private var figuresView: some View {
        if stacks {
            VStack(alignment: .leading, spacing: Spacing.xSmall) { figureCells }
        } else if horizontalSizeClass == .compact, figures.count > 3 {
            Grid(alignment: .topLeading, horizontalSpacing: Spacing.medium, verticalSpacing: Spacing.xSmall) {
                ForEach(Array(stride(from: 0, to: figures.count, by: 2)), id: \.self) { start in
                    GridRow {
                        ForEach(figures[start..<min(start + 2, figures.count)]) { figureCell($0) }
                    }
                }
            }
        } else {
            HStack(alignment: .top, spacing: Spacing.medium) { figureCells }
        }
    }

    private var figureCells: some View {
        ForEach(figures) { figureCell($0) }
    }

    private func figureCell(_ item: StatsSmallFigure) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: item.label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(verbatim: isReady ? item.value ?? "—" : "—")
                .font(.subheadline.weight(.medium))
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .foregroundStyle(isReady && item.value != nil ? .primary : .secondary)
        }
        .frame(maxWidth: stacks ? nil : .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private var footnoteView: some View {
        Text(verbatim: isReady ? footnote ?? " " : " ")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .lineLimit(stacks ? footnoteLines * 3 : footnoteLines, reservesSpace: true)
    }

    @ViewBuilder private var exportMenu: some View {
        Button {
            StatsChartExport.copyImage(
                of: StatsChartPlot(
                    box: box, data: data, phase: .ready, size: size, summary: title,
                    selection: .constant(.none), showInBrowse: nil, separatesSeries: separatesSeries
                )
                .environment(\.locale, locale)
                .environment(\.calendar, calendar),
                width: 640, colorScheme: colorScheme, scale: displayScale
            )
        } label: {
            Label { Text("stats.chart.copyImage", bundle: .module) } icon: { Image(systemName: "photo") }
        }
        Button {
            StatsChartExport.copyValues(StatsChartFormat(locale: locale, calendar: calendar, now: clock.now()).tabSeparatedValues(data))
        } label: {
            Label { Text("stats.chart.copyValues", bundle: .module) } icon: { Image(systemName: "tablecells") }
        }
    }

    // MARK: Container

    /// N25.3: the system group box on the Mac and the iPad at regular width; it
    /// takes the height its row gives it, so the boxes of a row share it. At
    /// compact width (iPhone, the folded Duo, a narrow iPad) the box is a
    /// section-like group on the grouped background, as inset grouped lists
    /// draw their sections (N25.6).
    @ViewBuilder private func container(@ViewBuilder _ content: () -> some View) -> some View {
        #if os(macOS)
        GroupBox {
            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        if horizontalSizeClass == .regular {
            GroupBox {
                content()
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        } else {
            content()
                .padding(Spacing.medium)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .background(Color(uiColor: .secondarySystemGroupedBackground), in: Radius.rounded(Radius.large))
        }
        #endif
    }
}

extension StatsChartBox where Accessory == EmptyView {
    /// A box with nothing trailing its title.
    public init(
        box: String, title: String, figure: StatsBoxFigure?, data: StatsChartData,
        phase: StatsBoxPhase = .ready, size: StatsPlotSize = .regular,
        figures: [StatsSmallFigure] = [], footnote: String? = nil, footnoteLines: Int = 0,
        separatesSeries: Bool? = nil, selection: Binding<StatsChartSelection>? = nil,
        open: @escaping () -> Void, showInBrowse: ((Int) -> Void)? = nil
    ) {
        self.init(
            box: box, title: title, figure: figure, data: data, phase: phase, size: size,
            figures: figures, footnote: footnote, footnoteLines: footnoteLines,
            separatesSeries: separatesSeries, selection: selection, open: open, showInBrowse: showInBrowse
        ) { EmptyView() }
    }
}

/// The header's marker for the charts of the present ("Now") and Knowledge Over
/// Time ("Estimated"), in secondary text (N25.4 part 1).
public struct StatsBoxMarker: View {
    private let text: String

    public init(_ text: String) { self.text = text }

    public static var now: StatsBoxMarker { StatsBoxMarker(String(localized: "stats.chart.now", bundle: .module)) }
    public static var estimated: StatsBoxMarker { StatsBoxMarker(String(localized: "stats.chart.estimated", bundle: .module)) }

    public var body: some View {
        Text(verbatim: text)
            .font(.subheadline)
            .foregroundStyle(.secondary)
    }
}
