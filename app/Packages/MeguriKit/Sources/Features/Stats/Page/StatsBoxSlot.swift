import CoreBridge
import DesignSystem
import SwiftUI

/// What the page gives a box's view (plan U296's extension point; the section units U297 to
/// U301 fill it): the box, the model's content for it, its plot size, its selection and what its
/// header opens. A section's view reads the model; it adds no model logic (plan §2).
@MainActor
public struct StatsBoxSlot {
    public let id: StatsBoxID
    public let model: StatsPageModel
    /// `.lead` for a section's lead box, else `.regular` (N25.3).
    public let size: StatsPlotSize
    /// The header's button: the chart's own page (S9, U302; nothing until it lands).
    public let open: () -> Void

    /// The box's content from the model; every box always has one (S8.5).
    public var content: StatsBoxContent { model.box(id) }

    /// The box's title (S16).
    public func title(locale: Locale) -> String { StatsPageText.title(id, locale: locale) }

    /// The model's selection for the box, cleared by a switch or the period (S8.3) and kept across
    /// a refresh: a loading phase never unpins it.
    public var selection: Binding<StatsChartSelection> {
        let model = model, id = id
        return Binding(get: { model.selection(for: id) }, set: { model.select($0, in: id) })
    }
}

/// A box drawn from its `StatsBoxContent` alone (the chart, figure, small figures and footnote)
/// with its header's switches or marker. Every box shows this until its section unit gives it its
/// own view; tables, the Activity grid and Time of Day's second panel are theirs.
public struct StatsGenericBox: View {
    private let slot: StatsBoxSlot
    @Environment(\.locale) private var locale

    public init(slot: StatsBoxSlot) {
        self.slot = slot
    }

    public var body: some View {
        let content = slot.content
        StatsChartBox(
            box: slot.id.rawValue, title: slot.title(locale: locale), figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open
        ) {
            StatsBoxAccessory(id: slot.id, model: slot.model)
        }
    }
}

/// The header's trailing part (N25.4 part 1): the box's switches as small segmented controls, or
/// "Now" for the charts of the present and "Estimated" for Knowledge Over Time.
public struct StatsBoxAccessory: View {
    let id: StatsBoxID
    let model: StatsPageModel
    @Environment(\.locale) private var locale

    public init(id: StatsBoxID, model: StatsPageModel) {
        self.id = id
        self.model = model
    }

    private func text(_ key: String.LocalizationValue) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }

    public var body: some View {
        switch id {
        case .reviewsOverTime:
            HStack(spacing: Spacing.xSmall) {
                StatsBoxSwitch(id: "stats.boxSwitch.reviewsMode", label: text("stats.switch.mode"),
                               selection: Binding(get: { model.selection.reviewsMode }, set: { model.setReviewsMode($0) }),
                               options: [(.byType, text("stats.switch.byType"), "byType"),
                                         (.byAnswer, text("stats.switch.byAnswer"), "byAnswer")])
                StatsBoxSwitch(id: "stats.boxSwitch.reviewsMeasure", label: text("stats.switch.measure"),
                               selection: Binding(get: { model.selection.reviewsMeasure }, set: { model.setReviewsMeasure($0) }),
                               options: [(.count, text("stats.switch.count"), "count"), (.time, text("stats.switch.time"), "time")])
            }
        case .retention:
            StatsBoxSwitch(id: "stats.boxSwitch.retentionKind", label: text("stats.switch.kind"),
                           selection: Binding(get: { model.selection.retentionKind }, set: { model.setRetentionKind($0) }),
                           options: [(.all, text("stats.switch.all"), "all"), (.young, text("stats.switch.young"), "young"),
                                     (.mature, text("stats.switch.mature"), "mature")])
        case .knowledgeOverTime:
            StatsBoxMarker.estimated
        default:
            if id.isPresent { StatsBoxMarker.now }
        }
    }
}

/// One of a box's switches: a small segmented control (N25.4), its options identified
/// `<id>.<option>`.
struct StatsBoxSwitch<Value: Hashable>: View {
    let id: String
    let label: String
    @Binding var selection: Value
    /// The value, its title and its identifier part.
    let options: [(Value, String, String)]

    var body: some View {
        Picker(selection: $selection) {
            ForEach(options, id: \.2) { option in
                Text(verbatim: option.1).tag(option.0)
                    .accessibilityIdentifier("\(id).\(option.2)")
            }
        } label: {
            Text(verbatim: label)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .accessibilityIdentifier(id)
    }
}
