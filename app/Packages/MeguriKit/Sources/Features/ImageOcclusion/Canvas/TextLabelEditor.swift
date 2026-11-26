import SwiftUI

/// Native text entry owns its keyboard and text undo while the sheet is open.
/// Ending the edit (including swipe-down and Escape) is one document change.
struct TextLabelEditor: View {
    @State private var text: String
    @State private var selection: TextSelection?
    @State private var bracesError = false
    @FocusState private var focused: Bool
    @Environment(\.dismiss) private var dismiss
    let reject: () -> Void
    let completed: (String) -> Void

    init(initial: String, reject: @escaping () -> Void, completed: @escaping (String) -> Void) {
        _text = State(initialValue: initial); self.reject = reject; self.completed = completed
    }
    var body: some View {
        NavigationStack {
            VStack {
                if bracesError {
                    Label(ioString("io.label.braces"), systemImage: "exclamationmark.triangle")
                }
                TextEditor(text: Binding(get: { text }, set: { value in
                    guard !value.contains("}}") else { bracesError = true; reject(); return }
                    bracesError = false; text = value
                }), selection: $selection)
                .focused($focused)
                .accessibilityLabel(ioString("io.tool.text"))
                .accessibilityIdentifier("io.label.text")
            }
            .padding()
            .navigationTitle(ioString("io.tool.text"))
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(ioString("io.label.done")) { dismiss() }
                }
            }
            .onAppear { focused = true; selection = TextSelection(range: text.startIndex..<text.endIndex) }
            .onKeyPress(.escape) { dismiss(); return .handled }
            .onDisappear { completed(text) }
        }
    }
}
