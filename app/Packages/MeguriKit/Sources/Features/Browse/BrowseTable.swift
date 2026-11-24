import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

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
    let model: BrowseModel
    @FocusState private var focused: Bool
    @Environment(\.locale) private var locale

    private var results: BrowseResults { model.results }

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
            // N21.5: the commands on the rows it was opened on (the selection,
            // or a row outside it, which a secondary click outlines).
            .contextMenu(forSelectionType: Int.self) { rows in
                BrowseRowMenu(model: model, rows: rows)
            } primaryAction: { _ in
                // N21.3: Return or a double click on a row shows the inspector.
                model.inspector.show()
            }
            .focused($focused)
            #if os(macOS)
            // N21.3: Return shows the inspector too (the primary action answers a double click).
            .onKeyPress(.return) { model.inspector.show(); return .handled }
            .background { BrowseScrollReporter(results: results, token: list?.token) }
            // U272: the header's column chooser, order and widths.
            .background {
                BrowseHeaderBridge(model: model, columns: list?.columns ?? [], token: list?.token, rows: list?.count ?? 0,
                                   locale: locale)
            }
            #endif
            // Another handle shown is another table: no diff of its rows with the last list's.
            // Widths reset (Reset to Standard Columns) lay the columns out again.
            .id(BrowseTableIdentity(token: list?.token ?? 0, widths: results.widthsRevision))
            .accessibilityIdentifier("browse.results.table")
            .onChange(of: results.scrollTarget) {
                guard let target = results.scrollTarget, target.token == list?.token else { return }
                // Only as far as brings the row into view: a row already shown
                // stays put, and the first rows stay below the column header.
                proxy.scrollTo(target.index)
            }
            .onChange(of: focused, initial: true) { _, focused in results.tableFocused = focused }
            .onAppear {
                // V1: back at the row it showed when Browse was left; else at the
                // row a new list brought into view.
                let kept = results.tableAppeared()
                let target = results.scrollTarget.flatMap { $0.token == list?.token ? $0.index : nil }
                // Kept at its top (row 0): no scroll, and no older target either.
                guard kept.map({ $0 > 0 }) ?? (target != nil) else { return results.tableSettled() }
                let request = results.scrollTarget
                Task { @MainActor in
                    if let kept { proxy.scrollTo(kept, anchor: .center) } else if let target { proxy.scrollTo(target) }
                    // A table shown again may not have laid its rows out yet: the
                    // scroll is repeated once they are (VM, 2026-11-23: it came back
                    // at its top).
                    await Task.yield()
                    try? await Task.sleep(for: .milliseconds(60))
                    // Unless a newer scroll request came meanwhile.
                    if let kept, results.scrollTarget == request { proxy.scrollTo(kept, anchor: .center) }
                    results.tableSettled()
                }
            }
            .onDisappear {
                results.tableDisappeared()
                results.tableFocused = false
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
            .width(min: BrowseMetrics.minimum(column.key), ideal: ideal(column.key))
            .alignment(alignment)
        }
        if first == nil {
            TableColumn(title) { line in
                BrowseCellView(results: results, index: line.id, column: column)
            }
            .width(min: BrowseMetrics.minimum(column.key), ideal: ideal(column.key))
            .alignment(alignment)
        }
    }

    /// V2.3: the width the person gave the column on this device, else its own.
    private func ideal(_ key: String) -> CGFloat {
        results.width(of: key).map { CGFloat($0) } ?? BrowseMetrics.ideal(key)
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
/// The table's identity: its list, and the widths' layout.
nonisolated struct BrowseTableIdentity: Hashable {
    let token: UInt64
    let widths: UInt64
}

enum BrowseMetrics {
    static let stateColumn: CGFloat = 22
    /// Size Columns to Fit: room beside a title for the sort indicator,
    /// beside a cell for its insets, and the widest a column is fitted to.
    static let headerSlack: CGFloat = 24
    static let cellSlack: CGFloat = 12
    static let fittedMaximum: CGFloat = 360
    /// How near a header's edge a double click fits that column.
    static let edgeSlop: CGFloat = 4
    /// A media cell's symbols.
    static let mediaCell: CGFloat = 40

    static func trailing(_ key: String, mode: BrowseMode) -> Bool {
        // Notes mode's Cards column is a count (C8).
        if key == "template" { return mode == .notes }
        return switch key {
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
        .onDisappear { results.rowDisappeared(index) }
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

#if os(macOS)
/// V1 on the Mac: the rows the results table's scroll view shows, read from
/// the table itself whenever its clip view moves. SwiftUI's appear and
/// disappear events of the table's cells did not follow a scroll in the VM
/// (2026-11-23), so the kept row came back at the top.
private struct BrowseScrollReporter: NSViewRepresentable {
    let results: BrowseResults
    /// The list this table shows.
    let token: UInt64?

    func makeNSView(context: Context) -> Reporter { Reporter(results: results) }
    func updateNSView(_ view: Reporter, context: Context) {
        view.results = results
        if view.token != token {
            view.token = token
            view.reattach()
        }
    }

    final class Reporter: NSView {
        var results: BrowseResults
        var token: UInt64?
        private weak var table: NSTableView?
        private var observer: NSObjectProtocol?
        private var liveObserver: NSObjectProtocol?

        init(results: BrowseResults) {
            self.results = results
            super.init(frame: .zero)
        }

        required init?(coder: NSCoder) { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            reattach()
        }

        /// The list changed or the view moved: the table to watch is looked for again.
        func reattach() {
            if let observer { NotificationCenter.default.removeObserver(observer) }
            if let liveObserver { NotificationCenter.default.removeObserver(liveObserver) }
            observer = nil
            liveObserver = nil
            table = nil
            guard window != nil else { return }
            attach(tries: 100)
        }

        /// The table view whose row count is the list's: SwiftUI swaps the
        /// table when a list arrives, and the old one never scrolls (VM trace,
        /// 2026-11-23). Retried until SwiftUI has made it.
        private func attach(tries: Int) {
            guard let window, table == nil, token != nil else { return }
            let rows = results.list?.count ?? 0
            table = window.contentView.flatMap { BrowseResponder.resultsTables(in: $0) }?
                .first { $0.window != nil && rows > 0 && $0.numberOfRows == rows }
            guard let clip = table?.enclosingScrollView?.contentView else {
                if tries == 0 { results.trace("no table found") }
                if tries > 0 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in self?.attach(tries: tries - 1) }
                }
                return
            }
            clip.postsBoundsChangedNotifications = true
            // Delivered as the clip view moves (no queue), while the event that moved
            // it is still the application's current event: on the main queue it came
            // later, and no scroll was ever kept (VM trace, 2026-11-23).
            observer = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clip,
                                                              queue: nil) { [weak self] _ in
                MainActor.assumeIsolated {
                    // Only the user's scrolling moves the kept row: the clip view also
                    // moves while SwiftUI takes the table down for another section
                    // (a click in the sidebar), and that must not keep its top.
                    let event = NSApp.currentEvent
                    switch event?.type {
                    case .scrollWheel, .keyDown, .leftMouseDragged, .swipe, .periodic: self?.report()
                    // A click on the scroller's track pages the table: also the user's scroll.
                    case .leftMouseDown where self?.onScroller(event) == true, .leftMouseUp where self?.onScroller(event) == true:
                        self?.report()
                    default: self?.skipped(event?.type)
                    }
                }
            }
            // A gesture or a scroller drag ends with its own notice: that position is kept too.
            liveObserver = NotificationCenter.default.addObserver(forName: NSScrollView.didEndLiveScrollNotification,
                                                                  object: table?.enclosingScrollView, queue: nil) { [weak self] _ in
                MainActor.assumeIsolated { self?.report() }
            }
            results.trace("attached \(table.map { "\($0.numberOfRows) rows" } ?? "")")
            restore(tries: 20)
        }

        /// V1: the kept row back at the table's middle, through the table itself
        /// (the scroll view reader's scroll did not move a SwiftUI table in the
        /// VM). Repeated until the table holds that row.
        private func restore(tries: Int) {
            guard let table, let scrollView = table.enclosingScrollView else { return }
            guard let row = results.pendingRestore, row < table.numberOfRows else {
                if tries == 0, let row = results.pendingRestore { results.trace("restore \(row) gave up: \(table.numberOfRows) rows") }
                if tries > 0 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in self?.restore(tries: tries - 1) }
                }
                return
            }
            let clip = scrollView.contentView
            let target = table.convert(table.rect(ofRow: row), to: clip.documentView ?? table)
            let insets = clip.contentInsets
            let visibleHeight = clip.bounds.height - insets.top - insets.bottom
            let top = target.midY - visibleHeight / 2 - insets.top
            let documentHeight = clip.documentView?.frame.height ?? table.frame.height
            let y = min(max(top, -insets.top), max(documentHeight - clip.bounds.height + insets.bottom, -insets.top))
            clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: y))
            scrollView.reflectScrolledClipView(clip)
            results.trace("restored \(row) of \(table.numberOfRows) to y \(Int(y)), now \(Int(clip.bounds.origin.y))")
            results.restored()
        }

        private var lastSkipped: NSEvent.EventType?? = .none
        /// Debug trace: the first bounds change of a run of non-scroll events.
        private func skipped(_ type: NSEvent.EventType?) {
            if case .some(let last) = lastSkipped, last == type { return }
            lastSkipped = .some(type)
            results.trace("moved on \(type.map { "\($0.rawValue)" } ?? "no event")")
        }

        /// The event lands on the table's vertical scroller.
        private func onScroller(_ event: NSEvent?) -> Bool {
            guard let event, let scroller = table?.enclosingScrollView?.verticalScroller, scroller.window === event.window else {
                return false
            }
            return scroller.convert(scroller.bounds, to: nil).contains(event.locationInWindow)
        }

        private func report() {
            guard let table = table ?? window?.contentView.flatMap(BrowseResponder.resultsTable(in:)),
                  let clip = table.enclosingScrollView?.contentView else { return }
            // The rows under the clip view's insets (the toolbar, the header) are not shown.
            var shown = clip.documentVisibleRect
            shown.origin.y += clip.contentInsets.top
            shown.size.height -= clip.contentInsets.top + clip.contentInsets.bottom
            let rows = table.rows(in: table.convert(shown, from: clip.documentView ?? table))
            guard rows.length > 0 else { return }
            guard let token else { return }
            results.tableScrolled(first: rows.location, last: rows.location + rows.length - 1, token: token)
        }
    }
}
#endif
