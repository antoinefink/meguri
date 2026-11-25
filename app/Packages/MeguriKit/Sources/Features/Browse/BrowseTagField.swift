import SwiftUI
#if os(macOS)
import AppKit

/// V12's native token editor and macOS suggestions presentation.
struct BrowseTagField: NSViewRepresentable {
    let tags: [String]
    let names: [String]
    let commit: ([String]) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTokenField {
        let field = NSTokenField()
        field.delegate = context.coordinator
        field.suggestionsDelegate = context.coordinator
        field.tokenStyle = .rounded
        field.tokenizingCharacterSet = CharacterSet.whitespacesAndNewlines
        field.isBordered = true
        field.isBezeled = true
        field.bezelStyle = .squareBezel
        field.lineBreakMode = .byWordWrapping
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.objectValue = tags
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateNSView(_ field: NSTokenField, context: Context) {
        let coordinator = context.coordinator
        coordinator.parent = self
        // SwiftUI can refresh while the field editor owns uncommitted text.
        if field.currentEditor() == nil, (field.objectValue as? [String]) != tags { field.objectValue = tags }
    }

    static func dismantleNSView(_ field: NSTokenField, coordinator: Coordinator) {
        // Finish native editing while its original note's commit is still bound.
        if field.currentEditor() != nil { field.window?.endEditing(for: field) }
        field.suggestionsDelegate = nil
        field.delegate = nil
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView field: NSTokenField, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        let height = field.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width, height: .greatestFiniteMagnitude)).height
        return CGSize(width: width, height: max(height ?? 0, field.intrinsicContentSize.height))
    }

    final class Coordinator: NSObject, NSTokenFieldDelegate, NSTextSuggestionsDelegate {
        typealias SuggestionItemType = String

        var parent: BrowseTagField
        private var editingCommit: (([String]) -> Void)?

        init(_ parent: BrowseTagField) { self.parent = parent }

        func textField(_ textField: NSTextField, provideUpdatedSuggestions responseHandler: @escaping (ItemResponse) -> Void) {
            guard let editor = textField.currentEditor() as? NSTextView,
                  !editor.hasMarkedText(), let query = Query(text: editor.string, selection: editor.selectedRange()) else {
                responseHandler(NSSuggestionItemResponse(items: []))
                return
            }
            let held = textField.objectValue as? [String] ?? []
            let items = query.suggestions(in: parent.names, excluding: held).map {
                NSSuggestionItem(representedValue: $0, title: $0)
            }
            responseHandler(NSSuggestionItemResponse(items: items))
        }

        func textField(_ textField: NSTextField, textCompletionFor item: Item) -> String? {
            // Highlighting must not replace the field's other token attachments.
            // Only didSelect inserts text, using the active token's UTF-16 range.
            nil
        }

        func textField(_ textField: NSTextField, didSelect item: Item) {
            guard let editor = textField.currentEditor() as? NSTextView,
                  !editor.hasMarkedText(), let query = Query(text: editor.string, selection: editor.selectedRange()),
                  query.isEligible(item.representedValue, in: parent.names,
                                   excluding: textField.objectValue as? [String] ?? []) else { return }
            editor.insertText(item.representedValue, replacementRange: query.range)
            editor.doCommand(by: #selector(NSResponder.insertNewline(_:)))
            // Tokenizing with Return can end editing. Keep accepting successive
            // tags from the keyboard, without disturbing an editor still active.
            if textField.currentEditor() == nil, let window = textField.window,
               window.makeFirstResponder(textField), let resumed = textField.currentEditor() as? NSTextView {
                resumed.setSelectedRange(NSRange(location: resumed.string.utf16.count, length: 0))
            }
        }

        func controlTextDidBeginEditing(_ notification: Notification) {
            editingCommit = parent.commit
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField else { return }
            let commit = editingCommit ?? parent.commit
            let tags = (field.objectValue as? [String] ?? []).flatMap { $0.split(whereSeparator: \.isWhitespace).map(String.init) }
            var seen = Set<String>(), unique: [String] = []
            for tag in tags where seen.insert(tag.lowercased()).inserted { unique.append(tag) }
            editingCommit = nil
            commit(unique)
        }
    }

    /// Native token attachments delimit editable tag text. NSTextInputClient
    /// uses UTF-16 ranges, including for non-ASCII tags.
    struct Query {
        let range: NSRange
        let text: String

        init?(text: String, selection: NSRange) {
            let string = text as NSString
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
            self.text = string.substring(with: range)
        }

        func suggestions(in names: [String], excluding held: [String]) -> [String] {
            let held = Set(held.map { $0.lowercased() })
            let typed = text.lowercased()
            var matches: [String] = []
            for name in names where isEligible(name, typed: typed, excluding: held) {
                matches.append(name)
                if matches.count == 50 { break }
            }
            return matches
        }

        func isEligible(_ name: String, in names: [String], excluding held: [String]) -> Bool {
            names.contains(name) && isEligible(name, typed: text.lowercased(), excluding: Set(held.map { $0.lowercased() }))
        }

        private func isEligible(_ name: String, typed: String, excluding held: Set<String>) -> Bool {
            let lower = name.lowercased()
            guard !held.contains(lower) else { return false }
            var start = lower.startIndex
            while true {
                if lower[start...].hasPrefix(typed) { return true }
                guard let separator = lower.range(of: "::", range: start..<lower.endIndex) else { return false }
                start = separator.upperBound
            }
        }
    }
}
#endif
