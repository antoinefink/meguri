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
                        .onSubmit { commitName(row) }
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
                    TextField(text: $placeholder) {
                        Text("noteTypes.inspector.placeholder", bundle: .module)
                        Text("noteTypes.inspector.placeholderHelp", bundle: .module)
                    }
                    .focused($focus, equals: .placeholder)
                    .onSubmit { commitPlaceholder(row) }
                    .accessibilityIdentifier("noteTypes.fields.inspector.placeholder")
                } header: {
                    Text("noteTypes.inspector.title", bundle: .module)
                }
                .disabled(row.deleted || !model.editable(id))
            }
        }
        .formStyle(.grouped)
        .task(id: row) { load() }
        .onChange(of: focus) { previous, _ in
            guard let row else { return }
            if previous == .name { commitName(row) }
            if previous == .placeholder { commitPlaceholder(row) }
        }
        .accessibilityIdentifier("noteTypes.fields.inspector")
    }

    private func load() {
        guard let row else { return }
        if focus != .name { name = row.name }
        if focus != .placeholder { placeholder = row.field.description }
    }

    private func option(_ row: NoteTypeFieldRow, _ key: WritableKeyPath<StructureDraftField, Bool>) -> Binding<Bool> {
        Binding(get: { row.field[keyPath: key] }, set: { value in model.setOption(id, row: row.id) { $0[keyPath: key] = value } })
    }

    private func commitName(_ row: NoteTypeFieldRow) {
        let typed = name
        guard typed != row.name else { return }
        Task {
            await model.commitRename(id, row: row.id, typed: typed)
            if model.renameRefusal == nil { name = model.draft(id)?.row(row.id)?.name ?? typed }
        }
    }

    private func commitPlaceholder(_ row: NoteTypeFieldRow) {
        let typed = placeholder
        model.setOption(id, row: row.id) { $0.description = typed }
    }
}
