import SwiftUI
#if os(macOS)
import AppKit

/// V12's system token editor. Only its completion presentation is replaced:
/// a field-anchored native popover leaves the tokens and insertion point visible.
struct BrowseTagField: NSViewRepresentable {
    let tags: [String]
    let names: [String]
    let commit: ([String]) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTokenField {
        let field = NSTokenField()
        field.delegate = context.coordinator
        field.tokenStyle = .rounded
        field.tokenizingCharacterSet = CharacterSet.whitespacesAndNewlines
        field.completionDelay = 0
        field.isBordered = true
        field.isBezeled = true
        field.bezelStyle = .squareBezel
        field.lineBreakMode = .byWordWrapping
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.objectValue = tags
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        context.coordinator.field = field
        return field
    }

    func updateNSView(_ field: NSTokenField, context: Context) {
        let coordinator = context.coordinator
        let namesChanged = coordinator.parent.names != names
        coordinator.parent = self
        // SwiftUI can refresh while the field editor owns uncommitted text.
        if field.currentEditor() == nil, (field.objectValue as? [String]) != tags { field.objectValue = tags }
        if namesChanged { coordinator.scheduleRefresh() }
    }

    static func dismantleNSView(_ field: NSTokenField, coordinator: Coordinator) {
        // Finish native editing while its original note's commit is still bound.
        if field.currentEditor() != nil { field.window?.endEditing(for: field) }
        coordinator.stopEditing()
        field.delegate = nil
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView field: NSTokenField, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        let height = field.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width, height: .greatestFiniteMagnitude)).height
        return CGSize(width: width, height: max(height ?? 0, field.intrinsicContentSize.height))
    }

    final class Coordinator: NSObject, NSTokenFieldDelegate, NSPopoverDelegate {
        var parent: BrowseTagField
        weak var field: NSTokenField?
        private weak var editor: NSTextView?
        private var selectionObserver: NSObjectProtocol?
        private var generation = 0
        private var editingCommit: (([String]) -> Void)?
        private var accepting = false
        private var dismissed: Query?
        private var offered: Query?
        private let suggestions = BrowseTagSuggestions()
        private let popover = NSPopover()

        init(_ parent: BrowseTagField) {
            self.parent = parent
            super.init()
            popover.behavior = .semitransient
            popover.animates = false
            popover.delegate = self
            popover.contentViewController = suggestions
            suggestions.accept = { [weak self] row in self?.accept(row) }
        }

        // Never let NSTokenField create the insertion-point completion window.
        func tokenField(_ tokenField: NSTokenField, completionsForSubstring substring: String, indexOfToken tokenIndex: Int,
                        indexOfSelectedItem selectedIndex: UnsafeMutablePointer<Int>?) -> [Any]? {
            selectedIndex?.pointee = -1
            return []
        }

        func controlTextDidBeginEditing(_ notification: Notification) {
            stopEditing()
            guard let field, let editor = field.currentEditor() as? NSTextView else { return }
            self.editor = editor
            editingCommit = parent.commit
            selectionObserver = NotificationCenter.default.addObserver(
                forName: NSTextView.didChangeSelectionNotification, object: editor, queue: .main
            ) { [weak self] _ in
                // AppKit posts selection changes during its text edits too.
                // Re-read only after it has completed the current operation.
                MainActor.assumeIsolated { self?.scheduleRefresh() }
            }
            scheduleRefresh()
        }

        func controlTextDidChange(_ notification: Notification) {
            dismissed = nil
            scheduleRefresh()
        }

        func scheduleRefresh() {
            generation += 1
            let ticket = generation
            DispatchQueue.main.async { [weak self] in
                guard let self, self.generation == ticket else { return }
                self.refresh()
            }
        }

        private func refresh() {
            guard !accepting, let field, let editor, field.currentEditor() === editor,
                  field.window?.firstResponder === editor, let query = Query(editor), query != dismissed else {
                closeSuggestions()
                return
            }
            let held = Set((field.objectValue as? [String] ?? []).map { $0.lowercased() })
            let typed = query.text.lowercased()
            let matches = Array(parent.names.filter { name in
                let lower = name.lowercased()
                return !held.contains(lower) && (lower.hasPrefix(typed)
                    || lower.components(separatedBy: "::").contains { $0.hasPrefix(typed) })
            }.prefix(50))
            guard !matches.isEmpty else { closeSuggestions(); return }
            let changed = offered != query || suggestions.names != matches
            offered = query
            if changed { suggestions.show(matches, fieldWidth: field.bounds.width) }
            popover.contentSize = suggestions.preferredContentSize
            if !popover.isShown {
                // The anchor is the whole field, never the current glyph. AppKit
                // chooses the available edge and keeps the popover on screen.
                popover.show(relativeTo: field.bounds, of: field, preferredEdge: .minY)
                field.window?.makeFirstResponder(editor)
            }
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            // Composition owns all its keys, including arrows, Return and Escape.
            guard !accepting, !textView.hasMarkedText() else { return false }
            if selector == #selector(NSTextView.complete(_:)) {
                dismissed = nil
                refresh()
                return true
            }
            guard popover.isShown else { return false }
            switch selector {
            case #selector(NSResponder.moveDown(_:)):
                suggestions.move(1)
            case #selector(NSResponder.moveUp(_:)):
                suggestions.move(-1)
            case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertTab(_:)):
                guard suggestions.table.selectedRow >= 0 else { closeSuggestions(); return false }
                accept(suggestions.table.selectedRow)
            case #selector(NSResponder.cancelOperation(_:)):
                dismissed = Query(textView)
                closeSuggestions()
            default: return false
            }
            return true
        }

        private func accept(_ row: Int) {
            guard let field, let editor, field.currentEditor() === editor, !editor.hasMarkedText(),
                  let offered, Query(editor) == offered, suggestions.names.indices.contains(row) else { return }
            let name = suggestions.names[row]
            accepting = true
            closeSuggestions()
            // Use the native field editor's text-input path, then its ordinary
            // Return command to tokenize. Never construct token attachments or
            // replace objectValue while editing: other tokens and undo survive.
            editor.insertText(name, replacementRange: offered.range)
            editor.doCommand(by: #selector(NSResponder.insertNewline(_:)))
            accepting = false
            scheduleRefresh()
        }

        private func closeSuggestions() {
            offered = nil
            popover.close()
        }

        func popoverWillClose(_ notification: Notification) {
            // Outside dismissal must survive queued SwiftUI/name refreshes.
            if let editor { dismissed = Query(editor) }
        }

        func stopEditing() {
            generation += 1
            if let selectionObserver { NotificationCenter.default.removeObserver(selectionObserver) }
            selectionObserver = nil
            editor = nil
            editingCommit = nil
            dismissed = nil
            closeSuggestions()
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField else { return }
            let commit = editingCommit ?? parent.commit
            let tags = (field.objectValue as? [String] ?? []).flatMap { $0.split(whereSeparator: \.isWhitespace).map(String.init) }
            var seen = Set<String>(), unique: [String] = []
            for tag in tags where seen.insert(tag.lowercased()).inserted { unique.append(tag) }
            stopEditing()
            commit(unique)
        }

        /// Native token attachments delimit editable tag text. UTF-16 ranges
        /// are required by NSTextInputClient, including for non-ASCII tags.
        private struct Query: Equatable {
            let range: NSRange
            let text: String
            let selection: NSRange

            init?(_ editor: NSTextView) {
                guard !editor.hasMarkedText() else { return nil }
                let string = editor.string as NSString
                let selection = editor.selectedRange()
                guard selection.length == 0, selection.location != NSNotFound, selection.location <= string.length else { return nil }
                let separators = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "\u{FFFC}"))
                let before = string.rangeOfCharacter(from: separators, options: .backwards,
                                                   range: NSRange(location: 0, length: selection.location))
                let start = before.location == NSNotFound ? 0 : NSMaxRange(before)
                let after = string.rangeOfCharacter(from: separators, options: [],
                                                  range: NSRange(location: selection.location, length: string.length - selection.location))
                let end = after.location == NSNotFound ? string.length : after.location
                guard end > start else { return nil }
                range = NSRange(location: start, length: end - start)
                text = string.substring(with: range)
                self.selection = selection
            }
        }
    }
}

/// A native popover, scroll view and table supply material, selection, scrollers
/// and accessible rows. It has no key view: typing stays with the token editor.
private final class BrowseTagSuggestions: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    let table = BrowseTagSuggestionTable()
    var names: [String] = []
    var accept: ((Int) -> Void)?

    override func loadView() {
        let scroll = BrowseTagSuggestionScrollView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        table.headerView = nil
        table.backgroundColor = .clear
        table.rowSizeStyle = .medium
        // A useful pointer/AX target, with at most six visible choices.
        table.rowHeight = max(28, table.rowHeight)
        table.allowsEmptySelection = true
        table.allowsMultipleSelection = false
        table.setAccessibilityIdentifier("browse.inspector.tagSuggestions")
        table.setAccessibilityLabel(BrowseStrings.text("browse.inspector.tags"))
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("tag"))
        table.addTableColumn(column)
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.autoresizingMask = [.width]
        table.dataSource = self
        table.delegate = self
        table.accept = { [weak self] row in self?.accept?(row) }
        scroll.documentView = table
        view = scroll
    }

    func show(_ names: [String], fieldWidth: CGFloat) {
        _ = view // Load the native scroll view before configuring its table.
        self.names = names
        let font = NSFont.preferredFont(forTextStyle: .body)
        let longest = names.map { ($0 as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
        // Width grows for paths, but cannot turn into a screen-wide completion
        // panel. Unusually long paths retain their full AX label and tooltip.
        let width = max(fieldWidth, min(420, longest + 32))
        let height = CGFloat(min(names.count, 6)) * (table.rowHeight + table.intercellSpacing.height)
        preferredContentSize = NSSize(width: width, height: height)
        view.frame.size = preferredContentSize
        let contentWidth = (view as? NSScrollView)?.contentSize.width ?? width
        table.frame = NSRect(x: 0, y: 0, width: contentWidth,
                             height: CGFloat(names.count) * (table.rowHeight + table.intercellSpacing.height))
        table.sizeLastColumnToFit()
        table.reloadData()
        table.deselectAll(nil)
        table.scrollRowToVisible(0)
    }

    func move(_ step: Int) {
        let current = table.selectedRow
        let next = current < 0 ? (step > 0 ? 0 : names.count - 1) : current + step
        if names.indices.contains(next) {
            table.selectRowIndexes(IndexSet(integer: next), byExtendingSelection: false)
            table.scrollRowToVisible(next)
        } else { table.deselectAll(nil) }
    }

    func numberOfRows(in tableView: NSTableView) -> Int { names.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = BrowseTagSuggestionCell()
        let label = NSTextField(labelWithString: names[row])
        label.font = .preferredFont(forTextStyle: .body)
        label.lineBreakMode = .byTruncatingMiddle
        label.translatesAutoresizingMaskIntoConstraints = false
        label.setAccessibilityElement(false)
        cell.addSubview(label)
        cell.textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: tableView.intercellSpacing.width),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -tableView.intercellSpacing.width),
            label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        cell.toolTip = names[row]
        cell.setAccessibilityElement(true)
        cell.setAccessibilityRole(.button)
        cell.setAccessibilityLabel(names[row])
        cell.setAccessibilityIdentifier("browse.inspector.tagSuggestion.\(row)")
        cell.press = { [weak self] in self?.accept?(row) }
        return cell
    }
}

private final class BrowseTagSuggestionScrollView: NSScrollView {
    override var acceptsFirstResponder: Bool { false }
}

private final class BrowseTagSuggestionTable: NSTableView {
    var accept: ((Int) -> Void)?
    override var acceptsFirstResponder: Bool { false }

    override func mouseDown(with event: NSEvent) {
        let row = row(at: convert(event.locationInWindow, from: nil))
        if row >= 0 { accept?(row) }
    }
}

private final class BrowseTagSuggestionCell: NSTableCellView {
    var press: (() -> Void)?
    override func accessibilityPerformPress() -> Bool {
        press?()
        return true
    }
}
#endif
