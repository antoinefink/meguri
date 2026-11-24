// CardInfo: the shared Card Info view for Browse, Review, the editor and the summary.
import AppModel
import Charts
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Card Info's strings (`stats/card-info` C12), from its String Catalog.
public enum CardInfoStrings {
    static func text(_ key: String.LocalizationValue, locale: Locale = .current) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }

    /// C4.1's labels, by row key; a key the catalog does not know shows as it is.
    public static func label(_ key: String, locale: Locale = .current) -> String {
        switch key {
        case "added": text("cardInfo.row.added", locale: locale)
        case "first_review": text("cardInfo.row.first_review", locale: locale)
        case "latest_review": text("cardInfo.row.latest_review", locale: locale)
        case "due": text("cardInfo.row.due", locale: locale)
        case "position": text("cardInfo.row.position", locale: locale)
        case "interval": text("cardInfo.row.interval", locale: locale)
        case "stability": text("cardInfo.row.stability", locale: locale)
        case "difficulty": text("cardInfo.row.difficulty", locale: locale)
        case "retrievability": text("cardInfo.row.retrievability", locale: locale)
        case "reviews": text("cardInfo.row.reviews", locale: locale)
        case "lapses": text("cardInfo.row.lapses", locale: locale)
        case "average_time": text("cardInfo.row.average_time", locale: locale)
        case "total_time": text("cardInfo.row.total_time", locale: locale)
        case "card_type": text("cardInfo.row.card_type", locale: locale)
        case "note_type": text("cardInfo.row.note_type", locale: locale)
        case "deck": text("cardInfo.row.deck", locale: locale)
        case "preset": text("cardInfo.row.preset", locale: locale)
        case "card_id": text("cardInfo.row.card_id", locale: locale)
        case "note_id": text("cardInfo.row.note_id", locale: locale)
        case "custom_data": text("cardInfo.row.custom_data", locale: locale)
        default: key
        }
    }

    /// C12's unavailable view: its title and text for a C11 kind (nil: a
    /// read that failed for another reason).
    public static func unavailable(_ kind: CardInfoUnavailable?, locale: Locale = .current) -> (title: String, text: String?) {
        let repair = text("cardInfo.unavailable.repair", locale: locale)
        let reason: String? = switch kind {
        case .notFound: nil
        case .missingNote: text("cardInfo.unavailable.missingNote", locale: locale) + " " + repair
        case .missingNotetype: text("cardInfo.unavailable.missingNotetype", locale: locale) + " " + repair
        case .missingTemplate: text("cardInfo.unavailable.missingTemplate", locale: locale) + " " + repair
        case .missingDeck: text("cardInfo.unavailable.missingDeck", locale: locale) + " " + repair
        case .missingHomeDeck: text("cardInfo.unavailable.missingHomeDeck", locale: locale) + " " + repair
        case .filteredHomeDeck: text("cardInfo.unavailable.filteredHomeDeck", locale: locale) + " " + repair
        case .missingPreset: text("cardInfo.unavailable.missingPreset", locale: locale) + " " + repair
        case .invalidCutoff: text("cardInfo.unavailable.invalidCutoff", locale: locale)
        case .fsrsFailed: text("cardInfo.unavailable.fsrsFailed", locale: locale)
        case .unreadableRecord, nil: text("cardInfo.unavailable.unreadableRecord", locale: locale)
        }
        if kind == .notFound { return (text("cardInfo.noCard", locale: locale), nil) }
        return (text("cardInfo.unavailable.title", locale: locale), reason)
    }

    /// C12's range names.
    public static func range(_ kind: CardInfoCurveRangeKind, locale: Locale = .current) -> String {
        switch kind {
        case .firstWeek: text("cardInfo.curve.firstWeek", locale: locale)
        case .firstMonth: text("cardInfo.curve.firstMonth", locale: locale)
        case .firstYear: text("cardInfo.curve.firstYear", locale: locale)
        case .allTime: text("cardInfo.curve.allTime", locale: locale)
        }
    }

    /// One history entry as VoiceOver reads it: each column's label, then its value.
    public static func historyLabel(_ entry: CardInfoReview, locale: Locale = .current) -> String {
        [(text("cardInfo.history.date", locale: locale), entry.date), (text("cardInfo.history.type", locale: locale), entry.kindText),
         (text("cardInfo.history.rating", locale: locale), entry.ratingText),
         (text("cardInfo.history.interval", locale: locale), entry.interval), (text("cardInfo.history.time", locale: locale), entry.time)]
            .filter { !$0.1.isEmpty }
            .map { "\($0.0), \($0.1)" }
            .joined(separator: "; ")
    }
}

/// Card Info for one card (U274; `browser/workspace` V13, `docs/design/
/// screen-inventory.yaml` `cardInfo.card`): C4's rows as a plain list, a
/// label column trailing-aligned beside the core's values (the Note tab's
/// Deck and Type rows' layout); then Review History, newest first, one row
/// per entry in a grid; then the Forgetting Curve, when the card has one;
/// then Copy Debug Info. One scroll view; nothing in it scrolls by itself.
public struct CardInfoView: View {
    let model: CardInfoModel
    @Environment(\.locale) private var locale

    public init(model: CardInfoModel) {
        self.model = model
    }

    public var body: some View {
        switch model.content {
        case .none, .loading:
            Color.clear.accessibilityIdentifier("cardInfo.loading")
        case .snapshot(let snapshot):
            CardInfoSnapshotView(model: model, snapshot: snapshot)
        case .unavailable(let kind):
            unavailable(kind)
        case .failed:
            unavailable(nil)
        }
    }

    private func unavailable(_ kind: CardInfoUnavailable?) -> some View {
        let shown = CardInfoStrings.unavailable(kind, locale: locale)
        return ContentUnavailableView {
            Text(verbatim: shown.title)
        } description: {
            if let text = shown.text { Text(verbatim: text) }
        }
        .accessibilityIdentifier("cardInfo.unavailable")
    }
}

private struct CardInfoSnapshotView: View {
    let model: CardInfoModel
    let snapshot: CardInfoSnapshot
    @Environment(\.locale) private var locale
    @State private var copied = false
    /// The history's column widths: each its widest cell.
    @State private var widths: [Int: CGFloat] = [:]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                rows
                if !snapshot.history.isEmpty {
                    Divider().padding(.horizontal)
                    history
                }
                if snapshot.curve != nil, let range = model.shownRange {
                    Divider().padding(.horizontal)
                    curve(range)
                }
                Divider().padding(.horizontal)
                copyDebugInfo
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("cardInfo.card")
    }

    // MARK: Stats rows (C4)

    /// The label leading in the secondary style, the core's value trailing
    /// (`cardInfo.card`); a long value wraps under its own edge.
    private var rows: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            ForEach(snapshot.rows, id: \.key) { row in
                let label = CardInfoStrings.label(row.key, locale: locale)
                HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                    Text(verbatim: label)
                        .foregroundStyle(.secondary)
                        .fixedSize()
                    Spacer(minLength: Spacing.small)
                    Text(verbatim: row.value)
                        .multilineTextAlignment(.trailing)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // One element: "Due, 2026-11-02".
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(verbatim: label))
                .accessibilityValue(Text(verbatim: row.value))
                .accessibilityIdentifier("cardInfo.row.\(row.key)")
            }
        }
        .padding(.horizontal)
        .padding(.vertical, Spacing.small)
    }

    // MARK: Review History (C6)

    private var history: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            Text("cardInfo.history", bundle: .module)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            // Each entry is one accessibility element; a cell takes its column's widest width.
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                    cell(0, Text("cardInfo.history.date", bundle: .module))
                    cell(1, Text("cardInfo.history.type", bundle: .module))
                    cell(2, Text("cardInfo.history.rating", bundle: .module), trailing: true)
                    cell(3, Text("cardInfo.history.interval", bundle: .module), trailing: true)
                    cell(4, Text("cardInfo.history.time", bundle: .module), trailing: true)
                }
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
                ForEach(snapshot.history) { entry in
                    HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
                        cell(0, Text(verbatim: entry.date))
                        cell(1, Text(verbatim: entry.kindText).foregroundStyle(Self.kindColor(entry.kind)))
                        cell(2, Text(verbatim: entry.ratingText)
                            .foregroundStyle(entry.rating == 1 ? Color.token(.countLearn) : Color.primary), trailing: true)
                        cell(3, Text(verbatim: entry.interval), trailing: true)
                        cell(4, Text(verbatim: entry.time), trailing: true)
                    }
                    .monospacedDigit()
                    .help(Text(verbatim: entry.dateTime))
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(Text(verbatim: CardInfoStrings.historyLabel(entry, locale: locale)))
                    .accessibilityIdentifier("cardInfo.history.\(entry.id)")
                }
            }
            .font(.callout)
            .lineLimit(1)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("cardInfo.history")
        }
        .padding(.horizontal)
        .padding(.vertical, Spacing.small)
    }

    /// A history cell as wide as its column's widest cell.
    private func cell(_ column: Int, _ text: some View, trailing: Bool = false) -> some View {
        text
            .fixedSize()
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width in
                if width > widths[column, default: 0] { widths[column] = width }
            }
            .frame(minWidth: widths[column], alignment: trailing ? .trailing : .leading)
    }

    /// C6.2's colours: Learn the New state's, Review the Review state's,
    /// Relearn the Learn state's; the others none.
    private static func kindColor(_ kind: CardInfoReviewKind) -> Color {
        switch kind {
        case .learn: .token(.countNew)
        case .review: .token(.countDue)
        case .relearn: .token(.countLearn)
        case .filtered, .manual, .rescheduled: .primary
        }
    }

    // MARK: Forgetting Curve (C7)

    private func curve(_ range: CardInfoCurveRange) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            Text("cardInfo.curve", bundle: .module)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            if let curve = snapshot.curve, curve.ranges.count > 1 {
                Picker(selection: Binding(get: { range.kind }, set: { model.range = $0 })) {
                    ForEach(curve.ranges, id: \.kind) { range in
                        Text(verbatim: CardInfoStrings.range(range.kind, locale: locale)).tag(range.kind)
                    }
                } label: { Text("cardInfo.curve.range", bundle: .module) }
                .pickerStyle(.segmented)
                .labelsHidden()
                .accessibilityIdentifier("cardInfo.curve.range")
            }
            CardInfoCurveChart(range: range, retention: snapshot.curve?.desiredRetention ?? 0)
                .frame(height: CardInfoMetrics.chartHeight)
                .accessibilityIdentifier("cardInfo.curve")
        }
        .padding(.horizontal)
        .padding(.vertical, Spacing.small)
    }

    // MARK: Copy Debug Info (C8)

    private var copyDebugInfo: some View {
        Button {
            Task {
                guard let json = await model.debugJSON(snapshot.cardID) else { return }
                #if os(macOS)
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(json, forType: .string)
                #else
                UIPasteboard.general.string = json
                #endif
                copied = true
            }
        } label: {
            Text(copied ? "cardInfo.copied" : "cardInfo.copyDebug", bundle: .module)
        }
        .buttonStyle(.bordered)
        .padding()
        .onChange(of: snapshot.cardID) { copied = false }
        .accessibilityIdentifier("cardInfo.copyDebug")
    }
}

/// C7.5's points in Swift Charts' own style: the past solid, the future
/// dashed, and the desired retention as a dashed rule when it is drawn.
private struct CardInfoCurveChart: View {
    let range: CardInfoCurveRange
    let retention: Double

    private struct Segment: Identifiable {
        let id: Int
        let future: Bool
        let points: [CardInfoCurvePoint]
    }

    /// Runs of points with one flag: the solid past and the dashed future meet at `now`.
    private var segments: [Segment] {
        var segments: [Segment] = []
        for point in range.points {
            if let last = segments.last, last.future == point.future {
                segments[segments.count - 1] = Segment(id: last.id, future: last.future, points: last.points + [point])
            } else {
                segments.append(Segment(id: segments.count, future: point.future, points: [point]))
            }
        }
        return segments
    }

    var body: some View {
        Chart {
            ForEach(segments) { segment in
                ForEach(Array(segment.points.enumerated()), id: \.offset) { _, point in
                    LineMark(x: .value("Date", Date(timeIntervalSince1970: point.instant)),
                             y: .value("Retrievability", point.retrievability), series: .value("Segment", segment.id))
                        .lineStyle(segment.future ? CardInfoMetrics.futureLine : CardInfoMetrics.pastLine)
                }
            }
            if range.showsRetention {
                RuleMark(y: .value("Desired Retention", retention * 100))
                    .lineStyle(CardInfoMetrics.retentionLine)
                    .foregroundStyle(.secondary)
            }
        }
        .chartYScale(domain: range.yMin...100)
        .chartYAxis {
            AxisMarks { value in
                AxisGridLine()
                AxisValueLabel { if let percent = value.as(Double.self) { Text(verbatim: "\(Int(percent))%") } }
            }
        }
        .chartLegend(.hidden)
        .accessibilityElement(children: .contain)
    }
}

enum CardInfoMetrics {
    static let chartHeight: CGFloat = 160
    static let pastLine = StrokeStyle(lineWidth: 2)
    static let futureLine = StrokeStyle(lineWidth: 2, dash: [4, 3])
    static let retentionLine = StrokeStyle(lineWidth: 1, dash: [3, 3])
}
