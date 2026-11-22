import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// One row of the table: its index in the shown list. Its cells read the
/// row from `BrowseResults` when it has been read (`browser/result-sets`
/// H7); until then they draw nothing.
nonisolated struct BrowseLine: Identifiable, Hashable {
    let id: Int
}

/// The list's indexes as rows, without a value per row in memory.
nonisolated struct BrowseLines: RandomAccessCollection {
    let count: Int
    var startIndex: Int { 0 }
    var endIndex: Int { count }
    subscript(position: Int) -> BrowseLine { BrowseLine(id: position) }
}

/// A header's sort: the table never sorts; a click asks the core (V4.1).
nonisolated struct BrowseSortComparator: SortComparator, Hashable {
    let key: String
    var order: SortOrder
    func compare(_ lhs: BrowseLine, _ rhs: BrowseLine) -> ComparisonResult { .orderedSame }
}

/// A column of the list (V2.3).
nonisolated struct BrowseColumn: Identifiable, Hashable {
    let key: String
    let index: Int
    let mode: BrowseMode
    var id: String { key }
}

/// The results table (N21.3, `browser/workspace` V2.5, V5): a native
/// `Table` with the state column first, then the set's columns, system
/// selection and headers whose sort indicator is the order shown.
struct BrowseTable: View {
    let results: BrowseResults

    var body: some View {
        let list = results.list
        let columns = (list?.columns ?? []).enumerated().map { BrowseColumn(key: $1, index: $0, mode: list?.mode ?? .cards) }
        ScrollViewReader { proxy in
            Table(of: BrowseLine.self, selection: selection, sortOrder: sortOrder) {
                TableColumn(Text(verbatim: "")) { line in
                    BrowseStateCell(results: results, index: line.id)
                }
                .width(BrowseMetrics.stateColumn)
                .disabledCustomizationBehavior(.all)
                TableColumnForEach(columns) { column in
                    dataColumn(column)
                }
            } rows: {
                ForEach(BrowseLines(count: list?.count ?? 0)) { line in
                    TableRow(line)
                }
            }
            // Another handle shown is another table: no diff of its rows with the last list's.
            .id(list?.token ?? 0)
            .accessibilityIdentifier("browse.results.table")
            .onChange(of: results.scrollTarget, initial: true) {
                guard let target = results.scrollTarget, target.token == list?.token else { return }
                proxy.scrollTo(target.index, anchor: .center)
            }
        }
    }

    /// A column of the set: sortable ones sort through the core (V4.1);
    /// the others show no sort affordance (`browser/sorting` S1).
    @TableColumnBuilder<BrowseLine, BrowseSortComparator>
    private func dataColumn(_ column: BrowseColumn) -> some TableColumnContent<BrowseLine, BrowseSortComparator> {
        let title = Text(verbatim: BrowseStrings.column(column.key, mode: column.mode))
        let alignment: TableColumnAlignment = BrowseMetrics.trailing(column.key, mode: column.mode) ? .trailing : .leading
        // Two ifs, not if and else: the builder joins a sortable column and
        // an unsortable one only as a block.
        let first = BrowseColumns.sortable(column.key, mode: column.mode) ? BrowseColumns.firstDirection(column.key) : nil
        if let first {
            TableColumn(title, sortUsing: BrowseSortComparator(key: column.key, order: first == .ascending ? .forward : .reverse)) { line in
                BrowseCellView(results: results, index: line.id, column: column)
            }
            .width(min: BrowseMetrics.minimum(column.key), ideal: BrowseMetrics.ideal(column.key))
            .alignment(alignment)
        }
        if first == nil {
            TableColumn(title) { line in
                BrowseCellView(results: results, index: line.id, column: column)
            }
            .width(min: BrowseMetrics.minimum(column.key), ideal: BrowseMetrics.ideal(column.key))
            .alignment(alignment)
        }
    }

    private var selection: Binding<Set<Int>> {
        Binding(get: { results.selection }, set: { results.select($0) })
    }

    /// The order shown; a click asks for a sort, which the header shows once
    /// its rows do.
    private var sortOrder: Binding<[BrowseSortComparator]> {
        Binding(get: {
            guard let sort = results.list?.sort else { return [] }
            return [BrowseSortComparator(key: sort.key, order: sort.direction == .ascending ? .forward : .reverse)]
        }, set: { comparators in
            guard let first = comparators.first, let sort = results.list?.sort else { return }
            let shown = BrowseSortComparator(key: sort.key, order: sort.direction == .ascending ? .forward : .reverse)
            guard first != shown else { return }
            results.sort(by: first.key)
        })
    }
}

/// Column widths and alignment (N21.3: text leading, numbers and dates
/// trailing). Widths are the system table's own until U272 keeps them.
enum BrowseMetrics {
    static let stateColumn: CGFloat = 22

    static func trailing(_ key: String, mode: BrowseMode) -> Bool {
        // Notes mode's Cards column is a count (C8).
        if key == "template" { return mode == .notes }
        switch key {
        case "cardDue", "cardEase", "cardIvl", "cardReps", "cardLapses", "noteCrt", "cardMod", "noteMod",
             "originalPosition", "stability", "difficulty", "retrievability":
            true
        default: false
        }
    }

    static func minimum(_ key: String) -> CGFloat {
        switch key {
        case "noteFld", "question", "answer": 120
        default: 48
        }
    }

    static func ideal(_ key: String) -> CGFloat {
        switch key {
        case "noteFld", "question", "answer": 260
        case "deck": 170
        case "template", "note", "noteTags": 110
        default: 80
        }
    }
}

/// V2.5: suspended, buried, else the flag, else nothing; the row's
/// identifier (`browse.table.row.<id>`) and its paging trigger.
struct BrowseStateCell: View {
    let results: BrowseResults
    let index: Int

    var body: some View {
        let row = results.row(at: index)
        let symbol = Self.symbol(row)
        ZStack {
            if let symbol {
                Image(systemName: symbol.name)
                    .foregroundStyle(symbol.color)
                    .imageScale(.small)
            }
        }
        .frame(maxWidth: .infinity, alignment: .center)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(symbol.map { Text(verbatim: $0.label) } ?? Text(verbatim: ""))
        .accessibilityIdentifier(row?.id.map { "browse.table.row.\($0)" } ?? "browse.table.placeholder.\(index)")
        .onAppear { results.rowAppeared(index) }
    }

    struct Symbol {
        let name: String
        let color: Color
        let label: String
    }

    static func symbol(_ row: BrowseShownRow?) -> Symbol? {
        guard let row else { return nil }
        if row.status != .row { return Symbol(name: "exclamationmark.triangle", color: .secondary, label: "") }
        guard let state = row.state else { return nil }
        if state.suspended {
            return Symbol(name: "pause.circle", color: .secondary, label: BrowseStrings.state(.suspended))
        }
        if state.buried { return Symbol(name: "moon", color: .secondary, label: BrowseStrings.state(.buried)) }
        if (1...7).contains(state.flag) {
            return Symbol(name: "flag.fill", color: BrowseStrings.flagColor(state.flag), label: BrowseStrings.flagLabel(state.flag))
        }
        return nil
    }
}

/// One cell (V3, V5): text leading, values trailing in tabular figures, a
/// deck as its name and its parent path, secondary text for a suspended or
/// buried row.
struct BrowseCellView: View {
    let results: BrowseResults
    let index: Int
    let column: BrowseColumn

    var body: some View {
        if let row = results.row(at: index), row.cells.indices.contains(column.index) {
            cell(row.cells[column.index], row: row)
                .foregroundStyle(row.isDimmed || results.isDimmed ? .secondary : .primary)
                .lineLimit(1)
        }
    }

    @ViewBuilder private func cell(_ cell: BrowseShownCell, row: BrowseShownRow) -> some View {
        switch cell {
        case .text(let text):
            Text(verbatim: text)
                .truncationMode(.tail)
                .font(font(row))
                .environment(\.layoutDirection, column.key == "noteFld" && row.sortFieldRightToLeft ? .rightToLeft : .leftToRight)
                .help(Text(verbatim: text))
        case .value(let text):
            Text(verbatim: text).monospacedDigit()
        case .deck(let name, let parent, let full):
            // V5.1: the parent path is cut first, from its end; then the name.
            HStack(spacing: Spacing.xxSmall) {
                Text(verbatim: name).layoutPriority(1)
                if let parent { Text(verbatim: parent).foregroundStyle(.secondary) }
            }
            .help(Text(verbatim: full))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(verbatim: full))
        case .noField:
            Text(verbatim: "–").foregroundStyle(.secondary)
        case .media(let sound, let image):
            HStack(spacing: Spacing.xxSmall) {
                if image { Image(systemName: "photo").accessibilityLabel(Text("browse.cell.image", bundle: .module)) }
                if sound { Image(systemName: "speaker.wave.2").accessibilityLabel(Text("browse.cell.sound", bundle: .module)) }
            }
        case .pending, .empty:
            EmptyView()
        }
    }

    /// C6: the template's browser font, for the cell-font columns (C2).
    private func font(_ row: BrowseShownRow) -> Font? {
        guard ["noteFld", "question", "answer"].contains(column.key), let font = row.font, font.name != nil || font.size != nil
        else { return nil }
        let size = font.size.map { CGFloat($0) }
        // A size alone keeps the system family (C6: each member applies on its own).
        return .custom(font.name ?? NSFontSize.systemName, size: size ?? NSFontSize.body)
    }
}

/// The system body size, for a browser font that names only its family.
enum NSFontSize {
    #if os(macOS)
    static var body: CGFloat { NSFont.systemFontSize }
    static var systemName: String { NSFont.preferredFont(forTextStyle: .body).fontName }
    #else
    static var body: CGFloat { UIFont.preferredFont(forTextStyle: .body).pointSize }
    static var systemName: String { UIFont.preferredFont(forTextStyle: .body).fontName }
    #endif
}
