import AppModel
import DesignSystem
import SwiftUI

/// The chart system's gallery (plan U295): every mark kind and every box phase
/// on SAMPLE data, reached only through the `stats/charts/…` test-hook routes
/// until the page exists (U296). Not a screen of the app.
public struct StatsChartGallery: View {
    public enum Page: String, CaseIterable, Sendable {
        /// Bars: stacked by state with working switches, single measures, bars and a line.
        case bars
        /// Lines: Knowledge, Retention with open points and its rule, Added and Started.
        case lines
        /// A stacked area and Reviews Due with Overdue and Later.
        case areas
        /// Loading (determinate and not), empty, and series drawn apart.
        case states
        /// A failed part and a sparse chart.
        case edges
        /// Pinned selections with their callouts.
        case selection
    }

    private let page: Page
    private let period: StatsSamplePeriod
    @Environment(\.calendar) private var calendar
    @Environment(\.appClock) private var clock
    @State private var width: CGFloat = 0
    @Environment(\.horizontalSizeClass) private var sizeClass

    public init(page: Page, period: StatsSamplePeriod = .all) {
        self.page = page
        self.period = period
    }

    private var samples: StatsChartSamples { StatsChartSamples(period: period, calendar: calendar, now: clock.now()) }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.medium) {
                Text(verbatim: "Sample data · \(page.rawValue) · \(period.rawValue.uppercased())")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if page == .selection {
                    StatsGallerySelection(samples: samples)
                } else {
                    StatsGalleryGrid(width: width, items: items)
                }
            }
            .padding(Spacing.medium)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        }
        .accessibilityIdentifier("stats.charts.gallery")
        #if os(iOS)
        // Compact: grouped cells on the grouped background (N25.6); regular:
        // group boxes on the system background (N25.3).
        .background(sizeClass == .regular ? Color(uiColor: .systemBackground) : Color(uiColor: .systemGroupedBackground))
        #endif
        .navigationTitle(Text(verbatim: "Chart System"))
    }

    private var items: [StatsGalleryItem] {
        switch page {
        case .bars:
            [
                .lead { StatsGalleryReviewsBox(samples: samples) },
                .regular { forgotten() },
                .regular { stability() },
                .regular { addedAndStarted(sparse: false) },
            ]
        case .lines:
            [
                .lead { knowledge() },
                .regular { retention() },
                .regular { addedAndStarted(sparse: false) },
            ]
        case .areas:
            [
                .lead { cardsOverTime() },
                .lead { reviewsDue() },
            ]
        case .states:
            [
                .lead {
                    StatsChartBox(
                        box: "reviewsOverTime", title: "Reviews Over Time", figure: nil,
                        data: samples.reviewsByType(browsable: false), phase: .loading(fraction: 0.4), size: .lead,
                        figures: reviewFigures(nil), open: {}
                    ) { StatsGallerySwitch(id: "loadingKind", options: ["By Type", "By Answer"]) }
                },
                .regular {
                    StatsChartBox(
                        box: "retention", title: "Retention", figure: nil, data: samples.retention, phase: .loading(fraction: nil),
                        figures: retentionFigures(nil), open: {}
                    ) { StatsGallerySwitch(id: "loadingRetention", options: ["All", "Young", "Mature"]) }
                },
                .regular {
                    StatsChartBox(
                        box: "forgotten", title: "Forgotten", figure: nil, data: samples.forgotten,
                        phase: .empty(StatsEmptyReason.noReviews(in: period.span ?? .lastMonth, deck: nil).text),
                        figures: [.init(label: "Young", value: nil), .init(label: "Mature", value: nil)], open: {}
                    )
                },
                .regular {
                    StatsChartBox(
                        box: "reviewsByAnswer", title: "Reviews Over Time",
                        figure: StatsBoxFigure(value: "41,233 reviews", sentence: "Differentiate Without Colour: segments drawn apart"),
                        data: samples.reviewsByAnswer, figures: Array(reviewFigures("").prefix(2)),
                        separatesSeries: true, open: {}
                    )
                },
            ]
        case .edges:
            [
                .regular {
                    StatsChartBox(
                        box: "stability", title: "Stability", figure: nil, data: StatsChartSamples.stability, phase: .failed,
                        figures: [.init(label: "Median Interval", value: nil), .init(label: "Over 1 Year", value: nil)],
                        footnoteLines: 1, open: {}
                    ) { StatsBoxMarker.now }
                },
                .regular { addedAndStarted(sparse: true) },
                .regular {
                    StatsChartBox(
                        box: "knowledgeOverTime", title: "Knowledge Over Time", figure: nil, data: samples.knowledge,
                        phase: .empty(StatsEmptyReason.noReviewsYet.text, detail: "This fills in as you study."),
                        figures: [.init(label: "Studied", value: nil), .init(label: "Likely Forgotten", value: nil)],
                        footnoteLines: 2, open: {}
                    ) { StatsBoxMarker.estimated }
                },
                .regular {
                    StatsChartBox(
                        box: "reviewsDue", title: "Reviews Due", figure: nil, data: samples.reviewsDue,
                        phase: .empty(StatsEmptyReason.noCardsDue.text),
                        figures: [.init(label: "Overdue", value: nil), .init(label: "Due Tomorrow", value: nil)], open: {}
                    )
                },
            ]
        case .selection:
            []
        }
    }

    private func reviewFigures(_ value: String?) -> [StatsSmallFigure] {
        [
            .init(label: "Days Studied", value: value.map { $0.isEmpty ? "318 of 330" : $0 }),
            .init(label: "Average a Day", value: value.map { $0.isEmpty ? "125" : $0 }),
            .init(label: "Average on Days Studied", value: value.map { $0.isEmpty ? "130" : $0 }),
        ]
    }

    private func retentionFigures(_ ready: Bool?) -> [StatsSmallFigure] {
        [
            .init(label: "Remembered", value: ready == true ? "37,277" : nil),
            .init(label: "Forgotten", value: ready == true ? "3,956" : nil),
            .init(label: "Young", value: ready == true ? "87.1%" : nil),
            .init(label: "Mature", value: ready == true ? "92.3%" : nil),
        ]
    }

    private func retention(selection: Binding<StatsChartSelection>? = nil) -> some View {
        StatsChartBox(
            box: "retention", title: "Retention",
            figure: StatsBoxFigure(value: "90.4%", sentence: "Desired retention 90%"),
            data: samples.retention, figures: retentionFigures(true), selection: selection, open: {}
        ) { StatsGallerySwitch(id: "retention", options: ["All", "Young", "Mature"]) }
    }

    private func forgotten(selection: Binding<StatsChartSelection>? = nil) -> some View {
        StatsChartBox(
            box: "forgotten", title: "Forgotten",
            figure: StatsBoxFigure(value: "3,956", sentence: "11 a day on average"),
            data: samples.forgotten,
            figures: [.init(label: "Young", value: "2,418"), .init(label: "Mature", value: "1,538")],
            selection: selection, open: {}, showInBrowse: { _ in }
        )
    }

    private func knowledge(selection: Binding<StatsChartSelection>? = nil) -> some View {
        StatsChartBox(
            box: "knowledgeOverTime", title: "Knowledge Over Time",
            figure: StatsBoxFigure(value: "3,512 cards", sentence: "88.9% of the 3,950 cards with an estimate, up 612 since 1 Jan"),
            data: samples.knowledge, size: .lead,
            figures: [.init(label: "Studied", value: "3,950"), .init(label: "Likely Forgotten", value: "438 (11.1%)"), .init(label: "Change in Period", value: "+612")],
            footnote: "12 suspended cards are not included.", selection: selection, open: {}
        ) { StatsBoxMarker.estimated }
    }

    private func cardsOverTime() -> some View {
        StatsChartBox(
            box: "cardsOverTime", title: "Cards Over Time",
            figure: StatsBoxFigure(value: "4,236 cards", sentence: "2,612 mature, up 1,204 in the period"),
            data: samples.cardsOverTime, size: .lead,
            figures: [.init(label: "New", value: "521 (−379)"), .init(label: "Learning", value: "180 (+50)"), .init(label: "Young", value: "903 (+203)"), .init(label: "Mature", value: "2,612 (+1,204)")],
            footnote: "12 suspended cards are not shown.", open: {}
        )
    }

    private func addedAndStarted(sparse: Bool) -> some View {
        StatsChartBox(
            box: "addedAndStarted", title: "Added and Started",
            figure: StatsBoxFigure(value: sparse ? "200" : "2,410", sentence: sparse ? "77 started" : "1,960 started"),
            data: samples.addedAndStarted(sparse: sparse),
            figures: [.init(label: "Added a Day", value: sparse ? "6.7" : "8.6"), .init(label: "Started a Day", value: sparse ? "2.6" : "7.0")],
            open: {}, showInBrowse: { _ in }
        )
    }

    private func stability() -> some View {
        StatsChartBox(
            box: "stability", title: "Stability",
            figure: StatsBoxFigure(value: "2.3 months", sentence: "Median stability"),
            data: StatsChartSamples.stability,
            figures: [.init(label: "Median Interval", value: "38 days"), .init(label: "Over 1 Year", value: "350")],
            footnote: "41 cards have no memory state.", open: {}, showInBrowse: { _ in }
        ) { StatsBoxMarker.now }
    }

    private func reviewsDue(selection: Binding<StatsChartSelection>? = nil) -> some View {
        StatsChartBox(
            box: "reviewsDue", title: "Reviews Due",
            figure: StatsBoxFigure(value: "2,904", sentence: "128 remaining today · 96 tomorrow"),
            data: samples.reviewsDue, size: .lead,
            figures: [.init(label: "Overdue", value: "42"), .init(label: "Due Tomorrow", value: "96"), .init(label: "Next 30 Days", value: "2,904"), .init(label: "Daily Load", value: "97")],
            selection: selection, open: {}, showInBrowse: { _ in }
        )
    }
}

/// A gallery cell: a lead box spans its row (N25.3), the others share one.
struct StatsGalleryItem {
    let lead: Bool
    let view: AnyView

    static func lead(@ViewBuilder _ view: () -> some View) -> StatsGalleryItem { StatsGalleryItem(lead: true, view: AnyView(view())) }
    static func regular(@ViewBuilder _ view: () -> some View) -> StatsGalleryItem { StatsGalleryItem(lead: false, view: AnyView(view())) }
}

/// N25.3's grid for the gallery: one column under about 560 points, two up to
/// about 1,250, three beyond; a lead box spans its row.
struct StatsGalleryGrid: View {
    let width: CGFloat
    let items: [StatsGalleryItem]

    private var columns: Int { width < 560 ? 1 : width < 1_250 ? 2 : 3 }

    /// Rows of item indices: a lead box alone, the others side by side.
    private var rows: [[Int]] {
        var rows: [[Int]] = []
        var row: [Int] = []
        for (index, item) in items.enumerated() {
            if item.lead || row.count == columns {
                if !row.isEmpty { rows.append(row) }
                row = []
            }
            if item.lead {
                rows.append([index])
            } else {
                row.append(index)
            }
        }
        if !row.isEmpty { rows.append(row) }
        return rows
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            ForEach(rows, id: \.self) { row in
                HStack(alignment: .top, spacing: Spacing.medium) {
                    ForEach(row, id: \.self) { index in
                        items[index].view
                            .frame(maxWidth: .infinity, alignment: .topLeading)
                    }
                    // Keep a short last row's boxes at one column's width.
                    if !items[row[0]].lead, row.count < columns {
                        ForEach(row.count..<columns, id: \.self) { _ in
                            Color.clear.frame(maxWidth: .infinity, maxHeight: 0)
                        }
                    }
                }
            }
        }
    }
}

/// Reviews Over Time with its two working switches: By Type and By Answer swap
/// the colour family; Count and Time the unit. A switch clears the selection (S8.3).
private struct StatsGalleryReviewsBox: View {
    let samples: StatsChartSamples
    @State private var byAnswer = false
    @State private var time = false
    @State private var selection = StatsChartSelection.none
    @Environment(\.horizontalSizeClass) private var sizeClass

    private var data: StatsChartData {
        var data = byAnswer ? samples.reviewsByAnswer : samples.reviewsByType(browsable: true)
        if time {
            data.unit = .duration
            data.series = data.series.map { var s = $0; s.values = s.values.map { $0.map { $0 * 9 } }; return s }
            data.calloutLines = data.calloutLines.map { var l = $0; l.values = l.values.map { $0.map { $0 * 9 } }; return l }
        }
        return data
    }

    var body: some View {
        StatsChartBox(
            box: "reviewsOverTime", title: "Reviews Over Time",
            figure: StatsBoxFigure(value: time ? "103 hr, 4 min" : "41,233 reviews", sentence: "23 days in a row"),
            data: data, size: .lead,
            figures: [.init(label: "Days Studied", value: "318 of 330"), .init(label: "Average a Day", value: "125"), .init(label: "Average on Days Studied", value: "130")],
            selection: $selection, open: {}, showInBrowse: { _ in }
        ) {
            HStack(spacing: Spacing.xSmall) {
                Picker(selection: $byAnswer) {
                    Text(verbatim: "By Type").tag(false)
                        .accessibilityIdentifier("stats.charts.gallery.kind.byType")
                    Text(verbatim: "By Answer").tag(true)
                        .accessibilityIdentifier("stats.charts.gallery.kind.byAnswer")
                } label: { Text(verbatim: "Kind") }
                .accessibilityIdentifier("stats.charts.gallery.kind")
                #if os(macOS)
                Picker(selection: $time) {
                    Text(verbatim: "Count").tag(false)
                        .accessibilityIdentifier("stats.charts.gallery.measure.count")
                    Text(verbatim: "Time").tag(true)
                        .accessibilityIdentifier("stats.charts.gallery.measure.time")
                } label: { Text(verbatim: "Measure") }
                .accessibilityIdentifier("stats.charts.gallery.measure")
                #endif
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
        }
        .onChange(of: byAnswer) { selection = .none }
        .onChange(of: time) { selection = .none }
    }
}

/// A small segmented switch for the gallery's headers.
private struct StatsGallerySwitch: View {
    let id: String
    let options: [String]
    @State var selected = 0

    init(id: String, options: [String], selected: Int = 0) {
        self.id = id
        self.options = options
        _selected = State(initialValue: selected)
    }

    /// "By Answer" → "byAnswer": identifier parts are lowerCamel words.
    static func identifierPart(_ text: String) -> String {
        let words = text.split(separator: " ").map(String.init)
        guard let first = words.first else { return "option" }
        return first.lowercased() + words.dropFirst().map { $0.prefix(1).uppercased() + $0.dropFirst().lowercased() }.joined()
    }

    var body: some View {
        Picker(selection: $selected) {
            ForEach(Array(options.enumerated()), id: \.offset) { index, option in
                Text(verbatim: option).tag(index)
                    .accessibilityIdentifier("stats.charts.gallery.\(id).\(Self.identifierPart(option))")
            }
        } label: { Text(verbatim: "Show") }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityIdentifier("stats.charts.gallery.\(id)")
    }
}

/// Selections pinned as a click, a touch or the keyboard leaves them: a
/// stacked bar with its total (and Show in Browse on 1M's days), Reviews Due's
/// Total by then, a line with its rule, and a single measure.
private struct StatsGallerySelection: View {
    let samples: StatsChartSamples
    @State private var reviews: StatsChartSelection
    @State private var due = StatsChartSelection(bin: 3, pinned: true)
    @State private var retention: StatsChartSelection
    @State private var knowledge: StatsChartSelection
    @State private var width: CGFloat = 0

    init(samples: StatsChartSamples) {
        self.samples = samples
        let n = samples.bins.count
        _reviews = State(initialValue: StatsChartSelection(bin: max(0, n - 4), pinned: true))
        _retention = State(initialValue: StatsChartSelection(bin: max(0, n / 2), pinned: true))
        _knowledge = State(initialValue: StatsChartSelection(bin: max(0, n / 4), pinned: false))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            StatsGalleryGrid(width: width, items: [
                .lead {
                    StatsChartBox(
                        box: "reviewsOverTime", title: "Reviews Over Time",
                        figure: StatsBoxFigure(value: "41,233 reviews", sentence: "23 days in a row"),
                        data: samples.reviewsByType(browsable: true), size: .lead,
                        figures: [.init(label: "Days Studied", value: "318 of 330"), .init(label: "Average a Day", value: "125")],
                        selection: $reviews, open: {}, showInBrowse: { _ in }
                    )
                },
                .regular {
                    StatsChartBox(
                        box: "retention", title: "Retention",
                        figure: StatsBoxFigure(value: "90.4%", sentence: "Desired retention 90%"),
                        data: samples.retention,
                        figures: [.init(label: "Remembered", value: "37,277"), .init(label: "Forgotten", value: "3,956")],
                        selection: $retention, open: {}
                    )
                },
                .regular {
                    StatsChartBox(
                        box: "reviewsDue", title: "Reviews Due",
                        figure: StatsBoxFigure(value: "2,904", sentence: "128 remaining today · 96 tomorrow"),
                        data: samples.reviewsDue,
                        figures: [.init(label: "Overdue", value: "42"), .init(label: "Due Tomorrow", value: "96")],
                        selection: $due, open: {}, showInBrowse: { _ in }
                    )
                },
                .regular {
                    StatsChartBox(
                        box: "knowledgeOverTime", title: "Knowledge Over Time",
                        figure: StatsBoxFigure(value: "3,512 cards", sentence: "Hover, not pinned: no Show in Browse"),
                        data: samples.knowledge,
                        figures: [.init(label: "Studied", value: "3,950"), .init(label: "Likely Forgotten", value: "438")],
                        selection: $knowledge, open: {}
                    ) { StatsBoxMarker.estimated }
                },
            ])
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }
}
