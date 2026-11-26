import AppModel
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Tags are native host input too: a held WebKit flush must first finish a
/// token being typed. Each callback retains its original draft's host.
struct OcclusionTagsField: View {
    let host: FieldEditorHost
    @State private var text = ""
    @FocusState private var focused: Bool
    var body: some View {
        HStack {
            Text("Tags", bundle: .module)
            #if os(macOS)
            NativeOcclusionTags(host: host)
            #else
            TextField(text: Binding(get: { text }, set: {
                guard host.acceptsHostInput else { return }
                text = $0; host.setNativeTags(Self.tags($0))
            })) { Text("Tags", bundle: .module) }
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused($focused)
                .accessibilityIdentifier("io.editor.tags.input")
                .onChange(of: host.maskEditorVisible) { _, shown in if shown { focused = false } }
                .onChange(of: host.draft.identity, initial: true) { text = host.draft.tags.joined(separator: " ") }
                .onChange(of: host.draft.tags) { _, tags in
                    if Self.tags(text) != tags { text = tags.joined(separator: " ") }
                }
            #endif
        }
        .padding()
        .disabled(!host.acceptsHostInput)
        .accessibilityIdentifier("io.editor.tags")
    }
    static func tags(_ text: String) -> [String] { text.split(whereSeparator: \.isWhitespace).map(String.init) }
}

#if os(macOS)
struct NativeOcclusionTags: NSViewRepresentable {
    let host: FieldEditorHost
    func makeCoordinator() -> Coordinator { Coordinator(host: host) }
    func makeNSView(context: Context) -> NSTokenField {
        let field = NSTokenField()
        field.delegate = context.coordinator
        field.tokenizingCharacterSet = .whitespacesAndNewlines
        field.objectValue = host.draft.tags
        field.setAccessibilityLabel(String(localized: "Tags", bundle: .module))
        field.setAccessibilityIdentifier("io.editor.tags.input")
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        if host.usesOcclusionForm {
            host.nativeTagsControl = field
            host.focusLeave = { [weak host, weak field] previous in
                guard let host, host.acceptsHostInput, let field else { return }
                if previous { host.webView.window?.selectPreviousKeyView(nil) }
                else { field.window?.makeFirstResponder(field) }
            }
        }
        host.flushNativeTags = { [weak field] in
            guard let field, field.currentEditor() != nil else { return }
            field.window?.endEditing(for: field)
        }
        return field
    }
    func updateNSView(_ field: NSTokenField, context: Context) {
        // A new draft gets a new native control, so an unfinished token can
        // never be committed with a replacement draft's tags or note id.
        field.isEnabled = host.acceptsHostInput && (host.usesOcclusionForm || !host.maskEditorVisible)
        if field.currentEditor() == nil, (field.objectValue as? [String]) != host.draft.tags { field.objectValue = host.draft.tags }
    }
    static func dismantleNSView(_ field: NSTokenField, coordinator: Coordinator) {
        if field.currentEditor() != nil { field.window?.endEditing(for: field) }
        field.delegate = nil
    }
    final class Coordinator: NSObject, NSTokenFieldDelegate {
        let host: FieldEditorHost
        init(host: FieldEditorHost) { self.host = host }
        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField else { return }
            let values = (field.objectValue as? [String] ?? []).flatMap(OcclusionTagsField.tags)
            host.setNativeTags(values)
        }
    }
}
#endif
