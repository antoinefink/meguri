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
    /// No data: one line in the plot area that says why (S8.5).
    case empty(String)
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
    private let reservesFootnote: Bool
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
        figures: [StatsSmallFigure] = [], footnote: String? = nil, reservesFootnote: Bool = false,
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
        self.reservesFootnote = reservesFootnote || footnote != nil
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
        case let .empty(message): .message(message)
        case .failed: .message(String(localized: "stats.chart.failed", bundle: .module))
        }
    }

    public var body: some View {
        container {
            VStack(alignment: .leading, spacing: Spacing.small) {
                header
                figureView
                plot
                    .contextMenu { if isReady { exportMenu } }
                if !figures.isEmpty { figuresView }
                if reservesFootnote { footnoteView }
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
                .lineLimit(stacks ? 4 : 2, reservesSpace: true)
        }
        .accessibilityElement(children: .combine)
    }

    private var plot: some View {
        StatsChartPlot(
            data: isReady ? data : data.skeleton(),
            phase: plotPhase,
            size: size,
            summary: isReady ? [figure?.value, figure?.sentence].compactMap(\.self).joined(separator: ", ") : title,
            selection: selection,
            showInBrowse: showInBrowse,
            separatesSeries: separatesSeries
        )
    }

    private var figuresView: some View {
        let layout = stacks
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
            : AnyLayout(HStackLayout(alignment: .top, spacing: Spacing.medium))
        return layout {
            ForEach(figures) { item in
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    Text(verbatim: item.label)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(verbatim: isReady ? item.value ?? "—" : "—")
                        .font(.subheadline.weight(.medium))
                        .monospacedDigit()
                        .foregroundStyle(isReady && item.value != nil ? .primary : .secondary)
                }
                .frame(maxWidth: stacks ? nil : .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
            }
        }
    }

    private var footnoteView: some View {
        Text(verbatim: isReady ? footnote ?? " " : " ")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .lineLimit(stacks ? 3 : 1, reservesSpace: true)
    }

    @ViewBuilder private var exportMenu: some View {
        Button {
            StatsChartExport.copyImage(
                of: StatsChartPlot(
                    data: data, phase: .ready, size: size, summary: title,
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

    /// N25.3: the system group box on the Mac. On iPhone and iPad the box is a
    /// grouped cell on the grouped background, as inset grouped lists draw
    /// their sections (N25.6).
    @ViewBuilder private func container(@ViewBuilder _ content: () -> some View) -> some View {
        #if os(macOS)
        GroupBox {
            content()
                .padding(Spacing.xxSmall)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        #else
        content()
            .padding(Spacing.medium)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(uiColor: .secondarySystemGroupedBackground), in: Radius.rounded(Radius.large))
        #endif
    }
}

extension StatsChartBox where Accessory == EmptyView {
    /// A box with nothing trailing its title.
    public init(
        box: String, title: String, figure: StatsBoxFigure?, data: StatsChartData,
        phase: StatsBoxPhase = .ready, size: StatsPlotSize = .regular,
        figures: [StatsSmallFigure] = [], footnote: String? = nil, reservesFootnote: Bool = false,
        separatesSeries: Bool? = nil, selection: Binding<StatsChartSelection>? = nil,
        open: @escaping () -> Void, showInBrowse: ((Int) -> Void)? = nil
    ) {
        self.init(
            box: box, title: title, figure: figure, data: data, phase: phase, size: size,
            figures: figures, footnote: footnote, reservesFootnote: reservesFootnote,
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
