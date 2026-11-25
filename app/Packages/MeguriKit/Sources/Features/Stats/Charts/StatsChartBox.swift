import AppModel
import DesignSystem
import SwiftUI

/// What a box's data is doing (S8.5, S10.3, S12.8). The box draws every phase
/// at the same size: the title, the legend and the figure labels stay; only the
/// plot area and the values change.
nonisolated public enum StatsBoxPhase: Hashable, Sendable {
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
nonisolated public struct StatsBoxFigure: Hashable, Sendable {
    public var value: String
    public var sentence: String

    public init(value: String, sentence: String) {
        self.value = value
        self.sentence = sentence
    }
}

/// One of the two to four small figures (N25.4 part 5). Its label shows in every
/// phase; its value only when the box is ready.
nonisolated public struct StatsSmallFigure: Identifiable, Hashable, Sendable {
    public var id: String { label }
    public var label: String
    public var value: String?

    public init(label: String, value: String?) {
        self.label = label
        self.value = value
    }
}

/// A box (contract N25.4): the header (a plain title until the chart's page
/// exists) with its trailing switches or marker, the figure and its sentence,
/// the plot with the system legend, the small figures and the footnote. It
/// keeps its size in every phase (S8.5): the sentence keeps two lines and the
/// plot keeps its height at every text size (S15). A footnote's line is kept
/// only while the box has one (Antoine, 2026-11-23: no empty space at the
/// bottom); a footnote that comes with the data adds its line then, the one
/// change of height accepted for it.
public struct StatsChartBox<Accessory: View>: View {
    private let box: String
    private let title: String
    private let accessory: Accessory
    private let figure: StatsBoxFigure?
    private let data: StatsChartData
    private let phase: StatsBoxPhase
    private let size: StatsPlotSize
    private let figures: [StatsSmallFigure]
    private let summaryFigures: [StatsSmallFigure]?
    private let footnote: String?
    private let footnoteLines: Int
    private let separatesSeries: Bool?
    private let externalSelection: Binding<StatsChartSelection>?
    private let open: (() -> Void)?
    private let showInBrowse: ((Int) -> Void)?
    private let spokenSummary: String?

    @State private var ownSelection = StatsChartSelection.none
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.displayScale) private var displayScale
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock

    /// - Parameters:
    ///   - box: the box's id in `stats.box.<box>` (S15), e.g. `reviewsOverTime`.
    ///   - accessory: the header's trailing switches (small segmented controls)
    ///     or marker ("Now", "Estimated").
    ///   - selection: a selection the caller holds and clears (a switch, the
    ///     period, Time of Day's two panels); nil keeps the box's own.
    ///   - showInBrowse: runs Show in Browse for a bin `data.browsable` lists
    ///     (S8.4); nil offers none.
    ///   - summary: the chart element's label and Audio Graph title when ready
    ///     (S15); nil reads the figure and its sentence.
    ///   - summaryFigures: replaces the large figure and bottom figures with adaptive
    ///     summaries above the plot (macOS overview, S7.8); nil keeps the standard layout.
    public init(
        box: String, title: String, figure: StatsBoxFigure?, data: StatsChartData,
        phase: StatsBoxPhase = .ready, size: StatsPlotSize = .regular,
        figures: [StatsSmallFigure] = [], footnote: String? = nil, footnoteLines: Int = 0,
        summaryFigures: [StatsSmallFigure]? = nil,
        separatesSeries: Bool? = nil, selection: Binding<StatsChartSelection>? = nil,
        open: (() -> Void)? = nil, showInBrowse: ((Int) -> Void)? = nil, summary: String? = nil,
        @ViewBuilder accessory: () -> Accessory
    ) {
        spokenSummary = summary
        self.box = box
        self.title = title
        self.accessory = accessory()
        self.figure = figure
        self.data = data
        self.phase = phase
        self.size = size
        self.figures = figures
        self.summaryFigures = summaryFigures
        self.footnote = footnote
        self.footnoteLines = max(footnoteLines, footnote == nil ? 0 : 1)
        self.separatesSeries = separatesSeries
        self.externalSelection = selection
        self.open = open
        self.showInBrowse = showInBrowse
    }

    private var isReady: Bool { phase == .ready }
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
                headline
                plot
                    .contextMenu { if isReady { exportMenu } }
                #else
                // On touch a hold on the plot selects (S8.3), so Copy Image
                // and Copy Values are on a hold of the figure instead.
                headline
                    .contextMenu { if isReady { exportMenu } }
                plot
                #endif
                if summaryFigures == nil, !figures.isEmpty { figuresView }
                if footnoteLines > 0 { footnoteView }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(box)")
    }

    // MARK: Parts

    private var header: some View {
        StatsBoxHeader(box: box, title: title, open: open) { accessory }
    }

    private var figureView: some View {
        StatsBoxFigureView(figure: figure, ready: isReady)
    }

    @ViewBuilder private var headline: some View {
        if let summaryFigures {
            StatsBoxSmallFigures(figures: summaryFigures, ready: isReady)
                .environment(\.statsNarrow, true)
        } else {
            figureView
        }
    }

    private var plot: some View {
        StatsChartPlot(
            box: box,
            data: isReady ? data : data.skeleton(),
            phase: plotPhase,
            size: size,
            summary: isReady ? spokenSummary ?? [figure?.value, figure?.sentence].compactMap(\.self).joined(separator: ", ") : title,
            selection: selection,
            showInBrowse: showInBrowse,
            separatesSeries: separatesSeries
        )
    }

    private var figuresView: some View {
        StatsBoxSmallFigures(figures: figures, ready: isReady)
    }

    private var footnoteView: some View {
        StatsBoxFootnote(footnote: footnote, lines: footnoteLines, ready: isReady)
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

    @ViewBuilder private func container(@ViewBuilder _ content: () -> some View) -> some View {
        StatsBoxContainer(content: content)
    }
}

/// A box's frame (N25.3, N25.6), shared by the chart boxes and the boxes that are not charts
/// (Activity, the tables): the system group box on the Mac and the iPad at regular width, which
/// takes the height its row gives it, so the boxes of a row share it; on the narrow page (iPhone,
/// the folded Duo, the Duo held tall, a narrow iPad) a section-like group on the grouped
/// background, as inset grouped lists draw their sections.
///
/// Every box keeps one even inset of about 16 points on all four sides, as the touch group box
/// and the grouped sections draw theirs (Antoine, 2026-11-23: "not enough internal margin around
/// this grey section"). The Mac's group box insets its content by only about 4 points, so the
/// content adds `Spacing.small` inside it there; the touch group box already insets by
/// `Spacing.medium`.
struct StatsBoxContainer<Content: View>: View {
    private let content: Content
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(\.statsNarrow) private var statsNarrow

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        #if os(macOS)
        GroupBox {
            content
                .padding(Spacing.small)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        // The narrow page (N25.6) is the shell's tab layout, the Duo held tall included, whose
        // size class can be regular; outside the page the size class decides.
        if !(statsNarrow ?? (horizontalSizeClass == .compact)) {
            GroupBox {
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        } else {
            content
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
        summaryFigures: [StatsSmallFigure]? = nil,
        separatesSeries: Bool? = nil, selection: Binding<StatsChartSelection>? = nil,
        open: (() -> Void)? = nil, showInBrowse: ((Int) -> Void)? = nil, summary: String? = nil
    ) {
        self.init(
            box: box, title: title, figure: figure, data: data, phase: phase, size: size,
            figures: figures, footnote: footnote, footnoteLines: footnoteLines,
            summaryFigures: summaryFigures,
            separatesSeries: separatesSeries, selection: selection, open: open, showInBrowse: showInBrowse,
            summary: summary
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
