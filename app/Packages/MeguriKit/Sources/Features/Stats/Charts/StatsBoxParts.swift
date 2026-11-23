import DesignSystem
import SwiftUI

// The parts of a box (contract N25.4) that every box draws the same way, whether its plot is a
// chart, a donut with its table (Card Counts) or a table (By Deck): the header,
// the figure and its sentence, the small figures and the footnote. Each keeps its size in every
// phase (S8.5), so the boxes of a row line up and nothing moves when the numbers arrive.

/// The header (N25.4 part 1): the title, then the box's switches or marker trailing on the same
/// line; stacked at the accessibility text sizes. Every header is as tall as one with a small
/// switch. The title is a plain heading while the box has no page to open (Antoine, 2026-11-23:
/// the carets served no purpose); once a chart's own page exists (S9, U302) its `open` makes the
/// title a button with a chevron again.
struct StatsBoxHeader<Accessory: View>: View {
    let box: String
    let title: String
    let open: (() -> Void)?
    @ViewBuilder let accessory: () -> Accessory
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        let stacks = dynamicTypeSize.isAccessibilitySize
        let layout = stacks
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: Spacing.xSmall))
        layout {
            StatsBoxTitle(box: box, title: title, open: open)
            if !stacks {
                Spacer(minLength: Spacing.xSmall)
                // Every header as tall as one with a small switch, aligned on its baseline, so
                // the figures, plots and axes of the boxes in a row line up (Retention's switch
                // beside Forgotten's bare title).
                Picker(selection: .constant(0)) { Text(verbatim: " ").tag(0) } label: { EmptyView() }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .controlSize(.small)
                    .fixedSize()
                    .frame(width: 0)
                    .hidden()
                    .accessibilityHidden(true)
            }
            accessory()
                .controlSize(.small)
        }
    }
}

/// A box's title (N25.4 part 1), `stats.box.<box>.header`: a plain heading, or with `open` a
/// button with a trailing chevron that opens the chart's page (S9).
struct StatsBoxTitle: View {
    let box: String
    let title: String
    let open: (() -> Void)?

    var body: some View {
        if let open {
            Button(action: open) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.xxSmall) {
                    text
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
        } else {
            text
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("stats.box.\(box).header")
        }
    }

    private var text: some View {
        Text(verbatim: title)
            .font(.headline)
            .foregroundStyle(.primary)
            .multilineTextAlignment(.leading)
    }
}

/// The large figure and its sentence (N25.4 part 2): an em dash until the box is ready, and two
/// lines kept for the sentence in every phase (three at the accessibility sizes), so the plot
/// never moves down when the numbers arrive. The chart element reads them (S15).
struct StatsBoxFigureView: View {
    let figure: StatsBoxFigure?
    let ready: Bool
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: ready ? figure?.value ?? "—" : "—")
                .font(.title2.weight(.semibold))
                .monospacedDigit()
                .foregroundStyle(ready && figure != nil ? .primary : .secondary)
                .lineLimit(1)
            Text(verbatim: ready ? figure?.sentence ?? " " : " ")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(dynamicTypeSize.isAccessibilitySize ? 3 : 2, reservesSpace: true)
        }
        .accessibilityHidden(true)
    }
}

/// The small figures (N25.4 part 5) in one row; two to a row at compact width when there are
/// four (decided by width class and count, never by the values, so the box keeps its size when
/// they arrive); one column at the accessibility sizes. Labels show in every phase, values once
/// the box is ready.
struct StatsBoxSmallFigures: View {
    let figures: [StatsSmallFigure]
    let ready: Bool
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    var body: some View {
        if dynamicTypeSize.isAccessibilitySize {
            VStack(alignment: .leading, spacing: Spacing.xSmall) { cells(stacked: true) }
        } else if horizontalSizeClass == .compact, figures.count > 3 {
            Grid(alignment: .topLeading, horizontalSpacing: Spacing.medium, verticalSpacing: Spacing.xSmall) {
                ForEach(Array(stride(from: 0, to: figures.count, by: 2)), id: \.self) { start in
                    GridRow {
                        ForEach(figures[start..<min(start + 2, figures.count)]) { cell($0, stacked: false) }
                    }
                }
            }
        } else {
            HStack(alignment: .top, spacing: Spacing.medium) { cells(stacked: false) }
        }
    }

    private func cells(stacked: Bool) -> some View {
        ForEach(figures) { cell($0, stacked: stacked) }
    }

    private func cell(_ item: StatsSmallFigure, stacked: Bool) -> some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: item.label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(verbatim: ready ? item.value ?? "—" : "—")
                .font(.subheadline.weight(.medium))
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .foregroundStyle(ready && item.value != nil ? .primary : .secondary)
        }
        .frame(maxWidth: stacked ? nil : .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

/// The footnote (N25.4 part 6), keeping its lines in every phase.
struct StatsBoxFootnote: View {
    let footnote: String?
    let lines: Int
    let ready: Bool
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        Text(verbatim: ready ? footnote ?? " " : " ")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .lineLimit(dynamicTypeSize.isAccessibilitySize ? lines * 3 : lines, reservesSpace: true)
    }
}

/// What a box that is not a Swift Charts plot shows in its plot area while it is not ready
/// (S8.5, S10.3, S12.8): the system's small progress indicator, determinate with the core's
/// fraction, or the line that says why, centred in the secondary colour.
struct StatsBoxPhaseView: View {
    let phase: StatsBoxPhase

    var body: some View {
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
        case let .empty(text, detail):
            VStack(spacing: Spacing.xxSmall) {
                Text(verbatim: text)
                    .font(.subheadline)
                if let detail {
                    Text(verbatim: detail)
                        .font(.footnote)
                }
            }
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .padding(.horizontal, Spacing.medium)
        case .failed:
            Text("stats.chart.failed", bundle: .module)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, Spacing.medium)
        case .ready:
            EmptyView()
        }
    }
}
