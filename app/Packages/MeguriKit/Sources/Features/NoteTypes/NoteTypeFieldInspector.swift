import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The selected field's options (`notetypes/workspace` W4, N22.2): Name,
/// Collapsed in Editor, Right-to-Left and Placeholder, each with its purpose
/// where W4 gives one. The other stored options are kept as they are.
struct NoteTypeFieldInspector: View {
    let model: NoteTypesModel
    let id: Int64
    @State private var name = ""
    @State private var placeholder = ""
    @FocusState private var focus: Field?
    @Environment(\.locale) private var locale

    private enum Field: Hashable { case name, placeholder }

    private var row: NoteTypeFieldRow? {
        model.selectedField[id].flatMap { model.draft(id)?.row($0) }
    }

    var body: some View {
        Form {
            if let row {
                Section {
                    TextField(text: $name) { Text("noteTypes.inspector.name", bundle: .module) }
                        .focused($focus, equals: .name)
                        .autocorrectionDisabled()
                        .onSubmit { Task { _ = await commitName(name, row: row.id) } }
                        .accessibilityIdentifier("noteTypes.fields.inspector.name")
                    if let refusal = model.renameRefusal, refusal.row == row.id, model.renaming != row.id {
                        Text(NoteTypesStrings.refusal(refusal.refusal, locale: locale))
                            .font(.caption)
                            .foregroundStyle(.red)
                            .accessibilityIdentifier("noteTypes.fields.inspector.nameRefusal")
                    }
                    Toggle(isOn: option(row, \.collapsed)) {
                        Text("noteTypes.inspector.collapsed", bundle: .module)
                        Text("noteTypes.inspector.collapsedHelp", bundle: .module)
                    }
                    .accessibilityIdentifier("noteTypes.fields.inspector.collapsed")
                    Toggle(isOn: option(row, \.rtl)) {
                        Text("noteTypes.inspector.rtl", bundle: .module)
                    }
                    .accessibilityIdentifier("noteTypes.fields.inspector.rtl")
                    TextField(text: $placeholder, prompt: Text("noteTypes.inspector.placeholderPrompt", bundle: .module)) {
                        Text("noteTypes.inspector.placeholder", bundle: .module)
                        Text("noteTypes.inspector.placeholderHelp", bundle: .module)
                    }
                    .focused($focus, equals: .placeholder)
                    .onSubmit { commitPlaceholder(placeholder, row: row.id) }
                    .accessibilityIdentifier("noteTypes.fields.inspector.placeholder")
                } header: {
                    Text("noteTypes.inspector.title", bundle: .module)
                }
                .disabled(row.deleted || !model.editable(id))
            }
        }
        .formStyle(.grouped)
        .task(id: row) { load() }
        .onChange(of: focus) { previous, now in
            // Typed text belongs to the row it was typed for, even when the
            // selection moves in the same update.
            if let previous, let editing = editingRow { commit(previous, row: editing) }
            editingRow = now == nil ? nil : row?.id
            // On focus loss the entry stays until the commit above ends, so Save waits for it.
            if now != nil { refreshPending() }
        }
        .onChange(of: name) { _, _ in refreshPending() }
        .onChange(of: placeholder) { _, _ in refreshPending() }
        .onDisappear {
            if let focus, let editing = editingRow { commit(focus, row: editing) }
        }
        .accessibilityIdentifier("noteTypes.fields.inspector")
    }

    private static let pendingKey = "inspector"
    /// The row the focused field was typed for.
    @State private var editingRow: Int?

    private func load() {
        guard let row else { return }
        if focus != .name || editingRow != row.id { name = row.name }
        if focus != .placeholder || editingRow != row.id { placeholder = row.field.description }
    }

    private func refreshPending() {
        guard focus != nil, let editing = editingRow, let current = model.draft(id)?.row(editing) else { return }
        let typedName = name, typedPlaceholder = placeholder, which = focus
        // Only text that differs from the field enables Save.
        guard which == .name ? typedName != current.name : typedPlaceholder != current.field.description else {
            model.pendingEdits[Self.pendingKey] = nil
            return
        }
        model.pendingEdits[Self.pendingKey] = {
            if which == .name { return await commitName(typedName, row: editing) }
            commitPlaceholder(typedPlaceholder, row: editing)
            return true
        }
    }

    private func commit(_ field: Field, row: Int) {
        let typedName = name, typedPlaceholder = placeholder, model = model
        switch field {
        case .name:
            Task {
                _ = await commitName(typedName, row: row)
                if focus == nil { model.pendingEdits[Self.pendingKey] = nil }
            }
        case .placeholder:
            commitPlaceholder(typedPlaceholder, row: row)
            model.pendingEdits[Self.pendingKey] = nil
        }
    }

    private func option(_ row: NoteTypeFieldRow, _ key: WritableKeyPath<StructureDraftField, Bool>) -> Binding<Bool> {
        Binding(get: { row.field[keyPath: key] }, set: { value in model.setOption(id, row: row.id) { $0[keyPath: key] = value } })
    }

    private func commitName(_ typed: String, row: Int) async -> Bool {
        guard let current = model.draft(id)?.row(row), typed != current.name else { return true }
        return await model.commitRename(id, row: row, typed: typed)
    }

    private func commitPlaceholder(_ typed: String, row: Int) {
        model.setOption(id, row: row) { $0.description = typed }
    }
}
