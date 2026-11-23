import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Decks section's box (plan U301; `stats/statistics-page` S7.15, S15, S16): By Deck, the
/// lead. The model writes the rows (`StatsByDeckTable`, H10's partition by home deck); this view
/// sorts them, and a row's click filters the page to its deck through the model's
/// `selectDeck`, as the deck pop-up does (S3.1).
enum StatsDecksSection {
    @MainActor @ViewBuilder static func box(_ slot: StatsBoxSlot) -> some View {
        StatsByDeckBox(slot: slot)
    }
}

/// By Deck's columns (S7.15), in table order.
enum StatsByDeckColumn: String, CaseIterable, Hashable, Sendable {
    case deck, cards, reviews, retention, time

    /// Whether a first click sorts it ascending: the deck order reads top to bottom, a number
    /// the largest first (as a Mac table sorts sizes).
    var startsAscending: Bool { self == .deck }
}

/// By Deck's order: a column and a direction. The deck column keeps the deck tree's order (the
/// model's, `sched/counts` C1: the deck's own cards first, then its subdecks), reversed when
/// descending; a number column orders by its value, a row with no retention last either way,
/// ties in the deck order.
struct StatsByDeckSort: Hashable, Sendable {
    var column: StatsByDeckColumn = .deck
    var ascending = true

    /// A click on a column's header: the same column turns the direction, another starts it in
    /// its first direction.
    func clicked(_ column: StatsByDeckColumn) -> StatsByDeckSort {
        column == self.column ? StatsByDeckSort(column: column, ascending: !ascending)
            : StatsByDeckSort(column: column, ascending: column.startsAscending)
    }

    func sorted(_ rows: [StatsByDeckTable.Row]) -> [StatsByDeckTable.Row] {
        let indexed = Array(rows.enumerated())
        func key(_ row: StatsByDeckTable.Row) -> Double? {
            switch column {
            case .deck: nil
            case .cards: Double(row.cards)
            case .reviews: Double(row.reviews)
            case .retention: row.retention
            case .time: row.timeShare
            }
        }
        guard column != .deck else { return ascending ? rows : rows.reversed() }
        return indexed.sorted { lhs, rhs in
            switch (key(lhs.element), key(rhs.element)) {
            case let (l?, r?) where l != r: return ascending ? l < r : l > r
            case (nil, _?): return false
            case (_?, nil): return true
            default: return lhs.offset < rhs.offset
            }
        }.map(\.element)
    }
}

/// S7.15: the subdecks of the scope (the top-level decks for All Decks) with their cards,
/// reviews in the period window, retention and share of study time as a number and a small
/// indigo bar, sortable by any column with its header (Mac, iPad); at compact width the name,
/// retention and the bar. A row's click (a tap) shows the page for that deck, the deck pop-up
/// changing with it; the row of the deck's own cards does not, since it is the deck shown. A
/// long name truncates in the middle, its full name the help. With no subdecks the box says so.
struct StatsByDeckBox: View {
    let slot: StatsBoxSlot
    @State private var sort = StatsByDeckSort()
    @State private var hovered: String?
    @Environment(\.locale) private var locale
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(\.statsScrolling) private var scrolling
    @ScaledMetric(relativeTo: .subheadline) private var numberWidth: CGFloat = 76
    @ScaledMetric(relativeTo: .subheadline) private var timeWidth: CGFloat = 116
    @ScaledMetric(relativeTo: .subheadline) private var shareBarWidth: CGFloat = 44

    private var compact: Bool { horizontalSizeClass == .compact }
    /// The order shown: a column the compact table leaves out sorts as the deck order.
    private var shownSort: StatsByDeckSort { columns.contains(sort.column) ? sort : StatsByDeckSort() }
    private var columns: [StatsByDeckColumn] {
        compact ? [.deck, .retention, .time] : StatsByDeckColumn.allCases
    }

    var body: some View {
        let content = slot.content
        let ready = content.phase == .ready
        let title = slot.title(locale: locale)
        let table: StatsByDeckTable? = if ready, case let .byDeck(value)? = content.detail { value } else { nil }
        StatsBoxContainer {
            VStack(alignment: .leading, spacing: Spacing.small) {
                StatsBoxHeader(box: slot.id.rawValue, title: title, open: slot.open) { EmptyView() }
                StatsBoxFigureView(figure: content.figure, ready: ready)
                VStack(alignment: .leading, spacing: 0) {
                    header
                    Divider()
                    if let table, !table.rows.isEmpty {
                        ForEach(shownSort.sorted(table.rows)) { row in
                            rowView(row)
                        }
                    } else {
                        StatsBoxPhaseView(phase: content.phase)
                            .frame(maxWidth: .infinity, minHeight: StatsPlotSize.regular.height)
                    }
                }
                .font(.subheadline)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(Text(verbatim: StatsSectionText.summary(title, content.figure) ?? title))
                .accessibilityIdentifier("stats.box.byDeck.table")
                .contextMenu { if let table { exportMenu(table, title: title) } }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.box.\(slot.id.rawValue)")
    }

    // MARK: Header

    private var header: some View {
        let text = StatsSectionText(locale: locale)
        return HStack(spacing: Spacing.small) {
            ForEach(columns, id: \.self) { column in
                let title = text.column(column)
                let sorted = shownSort.column == column
                Button {
                    sort = shownSort.clicked(column)
                } label: {
                    HStack(spacing: Spacing.xxSmall) {
                        if column != .deck { Spacer(minLength: 0) }
                        Text(verbatim: title)
                            .lineLimit(1)
                        Image(systemName: shownSort.ascending ? "chevron.up" : "chevron.down")
                            .font(.caption2.weight(.semibold))
                            .opacity(sorted ? 1 : 0)
                            .accessibilityHidden(true)
                        if column == .deck { Spacer(minLength: 0) }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .font(.caption.weight(sorted ? .semibold : .regular))
                .foregroundStyle(sorted ? .primary : .secondary)
                .frame(width: width(column), alignment: column == .deck ? .leading : .trailing)
                .frame(maxWidth: column == .deck ? .infinity : nil, alignment: .leading)
                .accessibilityLabel(Text(verbatim: title))
                .accessibilityValue(Text(verbatim: sorted ? (shownSort.ascending ? text.ascending : text.descending) : ""))
                .accessibilityAddTraits(sorted ? [.isHeader, .isSelected] : .isHeader)
                .accessibilityIdentifier("stats.box.byDeck.sort.\(column.rawValue)")
            }
        }
        .padding(.vertical, Spacing.xxSmall)
    }

    private func width(_ column: StatsByDeckColumn) -> CGFloat? {
        switch column {
        case .deck: nil
        case .time: timeWidth
        default: numberWidth
        }
    }

    // MARK: Rows

    @ViewBuilder private func rowView(_ row: StatsByDeckTable.Row) -> some View {
        let cells = HStack(spacing: Spacing.small) {
            ForEach(columns, id: \.self) { column in
                cell(column, row)
                    .frame(width: width(column), alignment: column == .deck ? .leading : .trailing)
                    .frame(maxWidth: column == .deck ? .infinity : nil, alignment: .leading)
            }
        }
        .padding(.vertical, Spacing.xxSmall)
        .padding(.horizontal, Spacing.xxSmall)
        .background(hovered == row.id && !row.ownCards ? AnyShapeStyle(HierarchicalShapeStyle.quaternary) : AnyShapeStyle(Color.clear),
                    in: Radius.rounded(Radius.small))
        .padding(.horizontal, -Spacing.xxSmall)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: spoken(row)))
        .accessibilityIdentifier("stats.box.byDeck.row.\(row.id)")
        if row.ownCards {
            // The deck shown: its own cards do not filter (S7.15).
            cells
        } else {
            Button { slot.model.selectDeck(row.deckID) } label: { cells.contentShape(Rectangle()) }
                .buttonStyle(.plain)
                #if os(macOS)
                .onHover { inside in
                    // Hover waits while the page scrolls (N25.14): rows passing under a resting
                    // pointer redraw nothing.
                    if inside, !scrolling.active { hovered = row.id } else if hovered == row.id { hovered = nil }
                }
                #endif
                .accessibilityHint(Text(verbatim: StatsSectionText(locale: locale).rowHint))
        }
    }

    @ViewBuilder private func cell(_ column: StatsByDeckColumn, _ row: StatsByDeckTable.Row) -> some View {
        switch column {
        case .deck:
            Text(verbatim: row.name)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(Text(verbatim: row.name))
        case .cards:
            number(row.cardsText)
        case .reviews:
            number(row.reviewsText)
        case .retention:
            number(row.retentionText, secondary: row.retention == nil)
        case .time:
            HStack(spacing: Spacing.xSmall) {
                number(row.timeShareText)
                StatsShareBar(share: row.timeShare)
                    .frame(width: shareBarWidth, height: StatsLegendSwatch.side)
            }
        }
    }

    private func number(_ text: String, secondary: Bool = false) -> some View {
        Text(verbatim: text)
            .monospacedDigit()
            .lineLimit(1)
            .foregroundStyle(secondary ? .secondary : .primary)
    }

    /// A row for VoiceOver: its name, then each column's title with its value.
    private func spoken(_ row: StatsByDeckTable.Row) -> String {
        let text = StatsSectionText(locale: locale)
        let values: [(StatsByDeckColumn, String)] = [
            (.cards, row.cardsText), (.reviews, row.reviewsText), (.retention, row.retentionText), (.time, row.timeShareText),
        ]
        return ([row.name] + values.filter { columns.contains($0.0) }.map { text.labelled(text.column($0.0), $0.1) })
            .joined(separator: ", ")
    }

    @ViewBuilder private func exportMenu(_ table: StatsByDeckTable, title: String) -> some View {
        let text = StatsSectionText(locale: locale)
        let header = StatsByDeckColumn.allCases.map(text.column).joined(separator: "\t")
        let lines = sort.sorted(table.rows).map {
            [$0.name, $0.cardsText, $0.reviewsText, $0.retentionText, $0.timeShareText].joined(separator: "\t")
        }
        StatsExportMenu(values: ([header] + lines).joined(separator: "\n")) {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                ForEach(sort.sorted(table.rows)) { row in
                    HStack {
                        Text(verbatim: row.name)
                        Spacer()
                        Text(verbatim: row.timeShareText).monospacedDigit()
                        StatsShareBar(share: row.timeShare)
                            .frame(width: shareBarWidth, height: StatsLegendSwatch.side)
                    }
                }
            }
            .font(.subheadline)
        }
    }
}

/// A share as a small flat bar on a quiet track (By Deck's time, S7.15), indigo as a single
/// measure is (S8.1).
struct StatsShareBar: View {
    let share: Double

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Rectangle().fill(.quaternary)
                Rectangle()
                    .fill(StatsSeriesColor.measure.color)
                    .frame(width: geometry.size.width * min(max(share, 0), 1))
            }
        }
        .accessibilityHidden(true)
    }
}
