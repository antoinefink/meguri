import SwiftUI

/// The list column's filter (`notetypes/workspace` W2, N22.1): the system's
/// own search field, `NSSearchField` on the Mac and `UISearchTextField` on
/// iPad, with its magnifying glass and clear button. SwiftUI's `searchable`
/// places its field in the toolbar or the window's sidebar, not at the top of
/// a list inside the detail column, so the control is hosted directly.
struct FilterField: View {
    @Binding var text: String
    let prompt: String

    var body: some View {
        FilterFieldRepresentable(text: $text, prompt: prompt)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityLabel(Text(prompt))
    }
}

#if os(macOS)
import AppKit

private struct FilterFieldRepresentable: NSViewRepresentable {
    @Binding var text: String
    let prompt: String

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = prompt
        field.sendsSearchStringImmediately = true
        field.delegate = context.coordinator
        // The clear button sends the action, not a text change.
        field.target = context.coordinator
        field.action = #selector(Coordinator.searched(_:))
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        // The field's own magnifier and clear buttons, named for the hierarchy checks.
        (field.cell as? NSSearchFieldCell)?.searchButtonCell?.setAccessibilityIdentifier("noteTypes.list.filterSearch")
        (field.cell as? NSSearchFieldCell)?.cancelButtonCell?.setAccessibilityIdentifier("noteTypes.list.filterClear")
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.text = $text
        if field.stringValue != text { field.stringValue = text }
        field.placeholderString = prompt
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var text: Binding<String>
        init(text: Binding<String>) { self.text = text }
        @objc func searched(_ field: NSSearchField) { text.wrappedValue = field.stringValue }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSSearchField else { return }
            text.wrappedValue = field.stringValue
        }
    }
}
#else
import UIKit

private struct FilterFieldRepresentable: UIViewRepresentable {
    @Binding var text: String
    let prompt: String

    func makeUIView(context: Context) -> UISearchTextField {
        let field = UISearchTextField()
        field.placeholder = prompt
        field.returnKeyType = .done
        field.autocorrectionType = .no
        field.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        field.addTarget(context.coordinator, action: #selector(Coordinator.done(_:)), for: .editingDidEndOnExit)
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateUIView(_ field: UISearchTextField, context: Context) {
        context.coordinator.text = $text
        if field.text != text { field.text = text }
        field.placeholder = prompt
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    final class Coordinator: NSObject {
        var text: Binding<String>
        init(text: Binding<String>) { self.text = text }
        @objc func changed(_ field: UISearchTextField) { text.wrappedValue = field.text ?? "" }
        @objc func done(_ field: UISearchTextField) { field.resignFirstResponder() }
    }
}
#endif
