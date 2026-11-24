#if os(macOS)
import AppKit
import AppModel
import CoreBridge
import SwiftUI

/// The results table's header on the Mac (U272; `browser/workspace` V2.3):
/// its context menu is the column chooser (the Mac's own place for it: a
/// secondary click on the header), dragging a header reorders the set's
/// list, dragging an edge sets a width kept on this device, and a double
/// click on an edge fits that column to its rows on screen. The Sort Field
/// column takes the remaining width.
///
/// Exception to the native contract, for that functional need: SwiftUI's
/// `Table` offers its own header menu only with `TableColumnCustomization`,
/// which keeps the order and widths in an opaque value of its own and has
/// no Fields submenu; V2.4 stores the order in the collection. So the
/// header's `NSMenu`, the table view's column notifications and a local
/// monitor of double clicks on the header (never consumed elsewhere) are
/// AppKit's, on the table SwiftUI draws.
struct BrowseHeaderBridge: NSViewRepresentable {
    let model: BrowseModel
    /// The columns the table draws, in order (the state column aside).
    let columns: [String]
    /// The list the table shows: another list is another table view.
    let token: UInt64?
    let rows: Int
    let locale: Locale

    func makeNSView(context: Context) -> Bridge { Bridge(model: model) }

    func updateNSView(_ view: Bridge, context: Context) {
        view.model = model
        view.locale = locale
        view.columns = columns
        view.rows = rows
        if view.token != token {
            view.token = token
            view.reattach()
        } else {
            view.sync()
        }
    }

    static func dismantleNSView(_ view: Bridge, coordinator: ()) { view.detach() }

    final class Bridge: NSView, NSMenuDelegate {
        var model: BrowseModel
        var locale = Locale.current
        var columns: [String] = []
        var rows = 0
        var token: UInt64?
        private weak var table: NSTableView?
        private var observers: [NSObjectProtocol] = []
        private var monitor: Any?
        private let chooser = NSMenu()
        /// Each table column's key, as the table drew them when it last held the model's order.
        private var keys: [ObjectIdentifier: String] = [:]
        /// A header is being dragged: the order is written once the mouse is up.
        private var dragging = false
        private var widthsDue: [String: Double] = [:]
        private var widthsTimer: Timer?

        init(model: BrowseModel) {
            self.model = model
            super.init(frame: .zero)
            chooser.delegate = self
            chooser.identifier = NSUserInterfaceItemIdentifier("browse.columns.menu")
        }

        required init?(coder: NSCoder) { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            reattach()
        }

        func detach() {
            observers.forEach(NotificationCenter.default.removeObserver)
            observers = []
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
            if table?.headerView?.menu === chooser { table?.headerView?.menu = nil }
            table = nil
            keys = [:]
        }

        func reattach() {
            detach()
            guard window != nil else { return }
            attach(tries: 100)
        }

        /// The table view whose row count is the list's (SwiftUI swaps the
        /// table when a list arrives); retried until SwiftUI has made it.
        private func attach(tries: Int) {
            guard let window, table == nil, token != nil else { return }
            table = window.contentView.flatMap { BrowseResponder.resultsTables(in: $0) }?
                .first { $0.window != nil && rows > 0 && $0.numberOfRows == rows }
            guard let table else {
                if tries > 0 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in self?.attach(tries: tries - 1) }
                }
                return
            }
            let center = NotificationCenter.default
            observers.append(center.addObserver(forName: NSTableView.columnDidMoveNotification, object: table, queue: nil) { [weak self] note in
                let old = note.userInfo?["NSOldColumn"] as? Int, new = note.userInfo?["NSNewColumn"] as? Int
                MainActor.assumeIsolated { self?.moved(from: old, to: new) }
            })
            observers.append(center.addObserver(forName: NSTableView.columnDidResizeNotification, object: table, queue: nil) { [weak self] note in
                let column = note.userInfo?["NSTableColumn"] as? NSTableColumn
                MainActor.assumeIsolated { self?.resized(column) }
            })
            monitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { [weak self] event in
                MainActor.assumeIsolated { self?.edgeDoubleClicked(event) == true } ? nil : event
            }
            sync()
        }

        /// After each SwiftUI update: the menu is the header's, the Sort Field
        /// column takes the remaining width, and the columns are known by key.
        func sync() {
            guard let table else { return }
            if table.headerView?.menu !== chooser { table.headerView?.menu = chooser }
            table.allowsColumnReordering = true
            table.allowsColumnResizing = true
            table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
            let drawn = table.tableColumns
            guard !dragging, drawn.count == columns.count + 1 else { return trace() }
            keys = [:]
            for (index, column) in drawn.enumerated() {
                guard index > 0 else {
                    column.resizingMask = []
                    continue
                }
                let key = columns[index - 1]
                keys[ObjectIdentifier(column)] = key
                column.resizingMask = key == "noteFld" ? [.autoresizingMask, .userResizingMask] : .userResizingMask
                if let title = column.headerCell.stringValue.nilIfEmpty, title != BrowseStrings.column(key, mode: model.results.columnsMode,
                                                                                                         locale: locale) {
                    report("mismatch at \(index): \(title) for \(key)")
                }
            }
            trace()
        }

        private func trace() {
            guard let table else { return }
            let titles = table.tableColumns.dropFirst().map { $0.headerCell.stringValue.nilIfEmpty ?? keys[ObjectIdentifier($0)] ?? "?" }
            let sort = model.results.list.map { " sort \($0.sort.key) \($0.sort.direction)" } ?? ""
            report("model \(columns.joined(separator: ","))\(sort) | table \(titles.joined(separator: ","))")
        }

        /// Out of the view update that called it.
        private func report(_ step: String) {
            DispatchQueue.main.async { [weak self] in self?.model.results.traceColumns(step) }
        }

        // MARK: Order (R1135)

        private func moved(from old: Int?, to new: Int?) {
            guard let table, let old, let new, old != new else { return }
            // The state column stays first (V2.5).
            if old == 0 || new == 0 {
                DispatchQueue.main.async { [weak table] in table?.moveColumn(new, toColumn: old) }
                return
            }
            // The table moves a dragged header past each column it crosses: the order
            // is written once, when the mouse is up.
            guard !dragging else { return }
            dragging = true
            waitForMouseUp()
            _ = table
        }

        private func waitForMouseUp() {
            guard NSEvent.pressedMouseButtons & 1 == 0 else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in self?.waitForMouseUp() }
                return
            }
            dragging = false
            guard let table else { return }
            let order = table.tableColumns.dropFirst().compactMap { keys[ObjectIdentifier($0)] }
            if order.count == columns.count, order != columns {
                model.results.moveColumns(order)
            } else {
                sync()
            }
        }

        // MARK: Widths (V2.3, K11)

        private func resized(_ column: NSTableColumn?) {
            // Only the person's drags: the window's own resizing moves the Sort Field column.
            guard let column, let header = table?.headerView, header.resizedColumn >= 0,
                  let key = keys[ObjectIdentifier(column)] else { return }
            widthsDue[key] = Double(column.width)
            scheduleWidths()
        }

        private func scheduleWidths() {
            widthsTimer?.invalidate()
            widthsTimer = Timer.scheduledTimer(withTimeInterval: 0.3, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, !self.widthsDue.isEmpty else { return }
                    self.model.results.setWidths(self.widthsDue)
                    self.widthsDue = [:]
                }
            }
        }

        /// A double click on a header's trailing edge fits that column (V2.3).
        private func edgeDoubleClicked(_ event: NSEvent) -> Bool {
            guard event.clickCount == 2, let table, let header = table.headerView, event.window === header.window else { return false }
            let point = header.convert(event.locationInWindow, from: nil)
            guard header.bounds.contains(point) else { return false }
            for (index, column) in table.tableColumns.enumerated() where index > 0 {
                let edge = header.headerRect(ofColumn: index).maxX
                guard abs(point.x - edge) <= 4, let key = keys[ObjectIdentifier(column)] else { continue }
                if key != "noteFld" { fit([column]) }
                return true
            }
            return false
        }

        /// Size Columns to Fit: each column but the Sort Field (which takes the
        /// rest) as wide as its title and the rows on screen need.
        func sizeToFit() {
            guard let table else { return }
            fit(table.tableColumns.dropFirst().filter { keys[ObjectIdentifier($0)] != "noteFld" })
        }

        private func fit(_ fitted: [NSTableColumn]) {
            guard let table else { return }
            let visible = table.rows(in: table.visibleRect)
            // The cells' text style, and the header's smaller one (measured, not drawn).
            let font = NSFont.preferredFont(forTextStyle: .body)
            let digits = font
            let headerFont = NSFont.preferredFont(forTextStyle: .subheadline)
            var widths: [String: Double] = [:]
            for column in fitted {
                guard let key = keys[ObjectIdentifier(column)], let index = columns.firstIndex(of: key) else { continue }
                let title = BrowseStrings.column(key, mode: model.results.columnsMode, locale: locale)
                // The title, with room for the sort indicator.
                var width = (title as NSString).size(withAttributes: [.font: headerFont]).width + BrowseMetrics.headerSlack
                for row in visible.location..<(visible.location + visible.length) {
                    guard let shown = model.results.row(at: row), shown.cells.indices.contains(index) else { continue }
                    width = max(width, Self.width(of: shown.cells[index], font: font, digits: digits))
                }
                let fittedWidth = min(max(width + BrowseMetrics.cellSlack, column.minWidth), BrowseMetrics.fittedMaximum)
                column.width = fittedWidth
                widths[key] = Double(fittedWidth)
            }
            model.results.setWidths(widths)
        }

        private static func width(of cell: BrowseShownCell, font: NSFont, digits: NSFont) -> CGFloat {
            let measure = { (text: String, font: NSFont) in (text as NSString).size(withAttributes: [.font: font]).width }
            return switch cell {
            case .text(let text): measure(text, font)
            case .value(let text): measure(text, digits)
            case .deck(let name, let parent, _): measure(name + (parent.map { " " + $0 } ?? ""), font)
            case .noField: measure("–", font)
            case .media: BrowseMetrics.mediaCell
            case .pending, .empty: 0
            }
        }

        // MARK: The menu (V2.3)

        func menuNeedsUpdate(_ menu: NSMenu) {
            menu.removeAllItems()
            for item in BrowseColumnMenu.of(model, canSizeToFit: true, locale: locale).items { add(item, to: menu) }
        }

        private func add(_ item: BrowseColumnMenu.Item, to menu: NSMenu) {
            switch item {
            case .column(let key, let title, let checked, let enabled):
                let entry = NSMenuItem(title: title, action: enabled ? #selector(toggle(_:)) : nil, keyEquivalent: "")
                entry.target = self
                entry.representedObject = key
                entry.state = checked ? .on : .off
                entry.isEnabled = enabled
                entry.identifier = NSUserInterfaceItemIdentifier("browse.columns.column.\(key)")
                menu.addItem(entry)
            case .submenu(let title, let identifier, let items):
                let entry = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                entry.identifier = NSUserInterfaceItemIdentifier("browse.columns.\(identifier)")
                let submenu = NSMenu(title: title)
                for item in items { add(item, to: submenu) }
                entry.submenu = submenu
                menu.addItem(entry)
            case .section(let title, let items):
                menu.addItem(NSMenuItem.sectionHeader(title: title))
                for item in items { add(item, to: menu) }
            case .separator:
                if let last = menu.items.last, !last.isSeparatorItem { menu.addItem(.separator()) }
            case .sizeToFit:
                let entry = NSMenuItem(title: BrowseStrings.text("browse.columns.sizeToFit", locale: locale),
                                       action: #selector(sizeColumnsToFit), keyEquivalent: "")
                entry.target = self
                entry.identifier = NSUserInterfaceItemIdentifier("browse.columns.sizeToFit")
                menu.addItem(entry)
            case .reset(let own, let enabled):
                let entry = NSMenuItem(title: BrowseColumnText.reset(own: own, locale: locale),
                                       action: enabled ? #selector(reset) : nil, keyEquivalent: "")
                entry.target = self
                entry.isEnabled = enabled
                entry.identifier = NSUserInterfaceItemIdentifier("browse.columns.reset")
                menu.addItem(entry)
            }
        }

        @objc private func toggle(_ sender: NSMenuItem) {
            guard let key = sender.representedObject as? String else { return }
            model.results.toggleColumn(key)
        }

        @objc private func sizeColumnsToFit() { sizeToFit() }

        @objc private func reset() { model.results.resetColumns() }
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
#endif
