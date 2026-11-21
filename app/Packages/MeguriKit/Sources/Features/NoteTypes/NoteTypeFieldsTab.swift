import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// One row of the Fields table as it is drawn.
nonisolated struct FieldLine: Identifiable, Hashable, Sendable {
    let id: Int
    /// The drag order (the field order the save keeps).
    let position: Int
    let name: String
    let deleted: Bool
    let isSort: Bool
    /// The card type names, nil until the references are read.
    let usedOn: [String]?
    let notes: UInt64?
    let ofNotes: UInt64?
    let deleteBlock: NoteTypeFieldDeleteBlock?

    var usedOnSortKey: String { usedOn?.joined(separator: ", ") ?? "" }
    var notesSortKey: UInt64 { notes ?? 0 }
}

/// The Fields tab (`notetypes/workspace` W3, W4; N22.2): a native table of
/// the draft's fields (Name, Sort, Used On, Notes) with drag reordering, +
/// and − under it, and the selected field's options in the inspector.
/// Nothing here writes before Save.
struct NoteTypeFieldsTab: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    @State private var sortOrder = [KeyPathComparator(\FieldLine.position)]
    @State private var showsInspector = true
    @Environment(\.locale) private var locale

    private var draft: NoteTypeDraft? { model.draft(id) }

    private var lines: [FieldLine] {
        guard let draft else { return [] }
        let usedOn = model.usedOn(id)
        let templates = draft.templates.map(\.name)
        return draft.rows.enumerated().map { position, row in
            let counts = model.notesWithContent(id, row: row)
            return FieldLine(
                id: row.id, position: position, name: row.name, deleted: row.deleted, isSort: row.id == draft.sortRow,
                usedOn: usedOn.map { ($0[row.id] ?? []).sorted().compactMap { templates[safe: $0] } },
                notes: counts?.count, ofNotes: counts?.of, deleteBlock: draft.deleteBlock(row.id))
        }
    }

    /// The table's order: the field order unless a header click sorted the view.
    private var sortedLines: [FieldLine] { lines.sorted(using: sortOrder) }
    private var inFieldOrder: Bool { sortOrder == [KeyPathComparator(\FieldLine.position)] }

    private var selection: Binding<Int?> {
        Binding(get: { model.selectedField[id] }, set: { model.selectedField[id] = $0 })
    }
    private var editable: Bool { model.editable(id) }

    var body: some View {
        VStack(spacing: .zero) {
            table
            Divider()
            bottomBar
        }
        .inspector(isPresented: $showsInspector) {
            NoteTypeFieldInspector(model: model, id: id)
                .inspectorColumnWidth(min: Self.inspectorMinimum, ideal: Self.inspectorIdeal, max: Self.inspectorMaximum)
        }
    }

    private static let inspectorMinimum: CGFloat = 240
    private static let inspectorIdeal: CGFloat = 280
    private static let inspectorMaximum: CGFloat = 360

    private var table: some View {
        Table(of: FieldLine.self, selection: selection, sortOrder: $sortOrder) {
            TableColumn(Text("noteTypes.fields.name", bundle: .module), value: \.position) { line in
                FieldNameCell(model: model, id: id, line: line)
            }
            TableColumn(Text("noteTypes.fields.sort", bundle: .module)) { line in
                sortRadio(line)
            }
            .width(min: ControlMetrics.minimumTarget, ideal: ControlMetrics.minimumTarget * 1.5)
            TableColumn(Text("noteTypes.fields.usedOn", bundle: .module), value: \.usedOnSortKey) { line in
                usedOn(line)
            }
            TableColumn(Text("noteTypes.fields.notes", bundle: .module), value: \.notesSortKey) { line in
                notes(line)
            }
        } rows: {
            ForEach(sortedLines) { line in
                TableRow(line)
                    .draggable(String(line.id))
            }
            .dropDestination(for: String.self) { index, payload in
                move(payload, to: index)
            }
        }
        .contextMenu(forSelectionType: Int.self) { rows in
            if let row = rows.first, let line = lines.first(where: { $0.id == row }) {
                if line.deleted {
                    Button { model.restoreField(id, row: row) } label: { Text("noteTypes.fields.restore", bundle: .module) }
                } else {
                    Button { startRename(row) } label: { Text("noteTypes.fields.rename", bundle: .module) }
                    Button(role: .destructive) { model.deleteField(id, row: row) } label: { Text("noteTypes.fields.delete", bundle: .module) }
                        .disabled(line.deleteBlock != nil)
                }
            }
        } primaryAction: { rows in
            // Double-click or Return renames in place (W3).
            if let row = rows.first { startRename(row) }
        }
        #if os(macOS)
        .onDeleteCommand { deleteSelected() }
        #endif
        .disabled(!editable)
        .accessibilityIdentifier("noteTypes.fields.table")
    }

    private func sortRadio(_ line: FieldLine) -> some View {
        Button {
            model.setSortField(id, row: line.id)
        } label: {
            Image(systemName: line.isSort ? "largecircle.fill.circle" : "circle")
                .foregroundStyle(line.isSort ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
        }
        .buttonStyle(.borderless)
        .disabled(line.deleted)
        .accessibilityLabel(Text(NoteTypesStrings.sortBy(line.name, locale: locale)))
        .accessibilityAddTraits(line.isSort ? .isSelected : [])
        .accessibilityIdentifier("noteTypes.fields.sort.\(line.id)")
    }

    @ViewBuilder private func usedOn(_ line: FieldLine) -> some View {
        if let names = line.usedOn {
            if names.isEmpty {
                Text("noteTypes.fields.notUsed", bundle: .module).foregroundStyle(.secondary)
            } else {
                Text(verbatim: names.joined(separator: ", "))
                    .foregroundStyle(line.deleted ? .secondary : .primary)
                    .strikethrough(line.deleted)
                    .lineLimit(1)
            }
        }
    }

    @ViewBuilder private func notes(_ line: FieldLine) -> some View {
        // W3: blank until the core's count is known.
        if let notes = line.notes, let total = line.ofNotes {
            HStack(spacing: Spacing.xSmall) {
                ProgressView(value: Double(notes), total: Double(max(total, 1)))
                    .progressViewStyle(.linear)
                    .controlSize(.small)
                    .frame(maxWidth: ContentWidth.answerButton * 0.6)
                    .accessibilityHidden(true)
                Text(notes, format: .number)
                    .monospacedDigit()
                    .foregroundStyle(line.deleted ? .secondary : .primary)
                    .strikethrough(line.deleted)
            }
        }
    }

    private var bottomBar: some View {
        HStack(spacing: Spacing.xxSmall) {
            Button {
                model.addField(id)
            } label: {
                Label { Text("noteTypes.fields.add", bundle: .module) } icon: { Image(systemName: "plus") }
                    .labelStyle(.iconOnly)
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
            }
            .accessibilityIdentifier("noteTypes.fields.add")
            Button {
                deleteSelected()
            } label: {
                Label { Text("noteTypes.fields.deleteSelected", bundle: .module) } icon: { Image(systemName: "minus") }
                    .labelStyle(.iconOnly)
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
            }
            .disabled(selectedDeleteBlocked)
            .help(selectedDeleteHelp)
            .accessibilityIdentifier("noteTypes.fields.delete")
            Spacer()
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, Spacing.xSmall)
        .disabled(!editable)
    }

    private var selectedLine: FieldLine? {
        model.selectedField[id].flatMap { row in lines.first { $0.id == row } }
    }
    private var selectedDeleteBlocked: Bool {
        guard let line = selectedLine else { return true }
        return line.deleted || line.deleteBlock != nil
    }
    private var selectedDeleteHelp: String {
        guard let block = selectedLine?.deleteBlock else { return "" }
        return NoteTypesStrings.deleteBlock(block, locale: locale)
    }

    private func deleteSelected() {
        guard let line = selectedLine, !line.deleted, line.deleteBlock == nil else { return }
        model.deleteField(id, row: line.id)
    }

    private func startRename(_ row: Int) {
        guard editable, let line = lines.first(where: { $0.id == row }), !line.deleted else { return }
        model.selectedField[id] = row
        model.cancelRename()
        model.renaming = row
    }

    /// A drag dropped at `index` of the shown order. Only the field order
    /// can be dragged: a view sorted by a header keeps the order as it is.
    private func move(_ payload: [String], to index: Int) {
        guard inFieldOrder else { return }
        let ids = Set(payload.compactMap(Int.init))
        let offsets = IndexSet(lines.filter { ids.contains($0.id) }.map(\.position))
        guard !offsets.isEmpty else { return }
        model.moveFields(id, fromOffsets: offsets, toOffset: index)
    }
}

/// The Name cell: the name, crossed out when deleted, or the rename field
/// with V5's refusal under it (W3).
private struct FieldNameCell: View {
    let model: NoteTypesModel
    let id: Int64
    let line: FieldLine
    @State private var text = ""
    @FocusState private var focused: Bool
    @Environment(\.locale) private var locale

    var body: some View {
        if model.renaming == line.id {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                TextField(text: $text) { Text("noteTypes.fields.name", bundle: .module) }
                    .labelsHidden()
                    .focused($focused)
                    .autocorrectionDisabled()
                    .onSubmit { commit() }
                    #if os(macOS)
                    .onExitCommand { model.cancelRename() }
                    #endif
                    .onChange(of: focused) { _, now in if !now, model.renaming == line.id { commit() } }
                    .accessibilityIdentifier("noteTypes.fields.renameField")
                if let refusal = model.renameRefusal, refusal.row == line.id {
                    Text(NoteTypesStrings.refusal(refusal.refusal, locale: locale))
                        .font(.caption)
                        .foregroundStyle(.red)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("noteTypes.fields.renameRefusal")
                }
            }
            .onAppear {
                text = line.name
                focused = true
            }
        } else {
            Text(verbatim: line.name)
                .strikethrough(line.deleted)
                .foregroundStyle(line.deleted ? .secondary : .primary)
                .lineLimit(1)
                .accessibilityValue(line.deleted ? Text("noteTypes.fields.deleted", bundle: .module) : Text(verbatim: ""))
                .accessibilityIdentifier("noteTypes.fields.row.\(line.id)")
        }
    }

    private func commit() {
        let typed = text
        Task { await model.commitRename(id, row: line.id, typed: typed) }
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
