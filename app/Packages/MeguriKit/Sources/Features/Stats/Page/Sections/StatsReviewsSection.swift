import DesignSystem
import SwiftUI

/// The Reviews section's boxes: Reviews Over Time and Activity (`stats/statistics-page` S7.4,
/// S7.5; contract N25.4, N25.8).
///
/// Show in Browse (S8.4) is a seam here: until U303 lands no callout offers it (S20's last line),
/// so both boxes get nil. U303 passes its `StatsBrowseLink` for a day bar of Reviews Over Time
/// (1M) and a day of Activity, both "Cards Reviewed on <date>" (`stats/browse-queries` Q9).
enum StatsReviewsSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        switch slot.id {
        case .activity: StatsActivityBox(slot: slot, showInBrowse: nil)
        default: StatsReviewsOverTimeBox(slot: slot, showInBrowse: nil)
        }
    }
}

/// Reviews Over Time (S7.4), the section's lead: stacked bars By Type (the state family:
/// Mature, Young, Relearning, Learning, then Filtered only when the period has filtered rows)
/// or By Answer (the answer family: Again, Hard, Good, Easy), in Count or Time; the figure and
/// its sentence, Days Studied, Average a Day, Average on Days Studied and, with Time, Time per
/// Card; the callout's every segment and the total. The model writes every value.
struct StatsReviewsOverTimeBox: View {
    let slot: StatsBoxSlot
    /// Show in Browse for a day bar (1M), given the bin; nil offers none.
    let showInBrowse: ((Int) -> Void)?
    @Environment(\.locale) private var locale

    var body: some View {
        let content = slot.content
        StatsChartBox(
            box: slot.id.rawValue, title: slot.title(locale: locale), figure: content.figure, data: content.data,
            phase: content.phase, size: slot.size, figures: content.figures, footnote: content.footnote,
            footnoteLines: slot.id.footnoteLines, selection: slot.selection, open: slot.open, showInBrowse: showInBrowse
        ) {
            StatsBoxAccessory(id: slot.id, model: slot.model)
        }
    }
}

/// A box's frame for a box that is not a chart (Activity): the header button with its chevron
/// (N25.4 part 1) over the content, in the system group box on the Mac and the iPad at regular
/// width, and a section-like group on the grouped background at compact width (N25.6), as
/// `StatsChartBox` draws its own.
struct StatsBoxChrome<Content: View>: View {
    let box: String
    let title: String
    let open: () -> Void
    @ViewBuilder let content: () -> Content
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    var body: some View {
        container {
            VStack(alignment: .leading, spacing: Spacing.small) {
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
                content()
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(box)")
    }

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
