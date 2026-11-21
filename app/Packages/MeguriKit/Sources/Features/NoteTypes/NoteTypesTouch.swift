#if os(iOS)
import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Note Types on iPad (`native-interface-contract` N22.1, N22.7): the list of
/// note types is the section's root, with the system search field as its
/// filter and a large title; a note type is a navigation step whose page
/// holds Fields and Cards, Revert and Save. Field rows open their options in
/// a popover, and Edit gives drag handles and delete buttons.
struct NoteTypesTouchRoot: View {
    @Bindable var model: NoteTypesModel
    let cards: (Int64) -> AnyView

    var body: some View {
        List {
            ForEach(model.filteredItems) { item in
                Button {
                    model.opened = item.id
                } label: {
                    HStack(spacing: Spacing.small) {
                        NoteTypeListRow(item: item, edited: model.isEdited(item.id))
                        // The row opens a page, as a navigation row shows.
                        Image(systemName: "chevron.forward")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(.tertiary)
                            .accessibilityHidden(true)
                    }
                    .contentShape(Rectangle())
                }
                .foregroundStyle(.primary)
            }
        }
        .overlay {
            switch model.phase {
            case .loading:
                DelayedProgress()
            case .failed:
                ContentUnavailableView {
                    Label { Text("noteTypes.list.failed.title", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                } description: {
                    Text("noteTypes.list.failed.message", bundle: .module)
                } actions: {
                    Button { model.reloadList() } label: { Text("noteTypes.tryAgain", bundle: .module) }
                }
            case .ready where model.filteredItems.isEmpty && !model.items.isEmpty:
                ContentUnavailableView.search(text: model.filter)
            default:
                EmptyView()
            }
        }
        .accessibilityIdentifier("noteTypes.list.table")
        .navigationTitle(Text("noteTypes.title", bundle: .module))
        .searchable(text: $model.filter, prompt: Text("noteTypes.list.filter", bundle: .module))
        .navigationDestination(item: $model.opened) { id in
            NoteTypeTouchPage(model: model, id: id, cards: cards)
        }
        .onAppear { model.appear() }
    }
}

/// One note type's page: its name with "412 notes · 786 cards" under it,
/// Fields | Cards, then Revert and Save.
private struct NoteTypeTouchPage: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    let cards: (Int64) -> AnyView
    @Environment(\.locale) private var locale

    var body: some View {
        NoteTypeDetailContent(model: model, id: id, cards: cards)
            .navigationTitle(Text(verbatim: model.draft(id)?.name ?? model.item(id)?.name ?? ""))
            .navigationSubtitle(subtitle)
            .navigationBarTitleDisplayMode(.inline)
            // A changed draft is never left by the back gesture; Revert or Save first (D1).
            .navigationBarBackButtonHidden(model.isEdited(id))
            .toolbar {
                if model.isEdited(id) {
                    ToolbarItem(placement: .topBarLeading) {
                        Button { model.confirmingRevert = true } label: { Text("noteTypes.revert", bundle: .module) }
                            .disabled(!model.editable(id))
                            .accessibilityIdentifier("noteTypes.list.revert")
                    }
                }
                // The centre keeps the note type's name and counts; Fields | Cards sit with Save.
                ToolbarItem(placement: .topBarTrailing) {
                    Picker(selection: $model.tab) {
                        Text("noteTypes.tab.fields", bundle: .module).tag(NoteTypesModel.Tab.fields)
                        Text("noteTypes.tab.cards", bundle: .module).tag(NoteTypesModel.Tab.cards)
                    } label: {
                        Text("noteTypes.tabs", bundle: .module)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                    .accessibilityIdentifier("noteTypes.list.tabs")
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    if let progress = model.progress, progress.notetypeId == id {
                        SaveProgressView(progress: progress)
                    }
                    Button { model.save() } label: {
                        model.saveAsks(id) ? Text("noteTypes.saveAsks", bundle: .module) : Text("noteTypes.save", bundle: .module)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!model.canSave(id))
                    .accessibilityIdentifier("noteTypes.list.save")
                }
            }
            .confirmationDialog(Text("noteTypes.discard.title", bundle: .module), isPresented: $model.confirmingRevert,
                                titleVisibility: .visible) {
                Button(role: .destructive) { model.revert(id) } label: { Text("noteTypes.discard.discard", bundle: .module) }
                Button(role: .cancel) { model.confirmingRevert = false } label: { Text("noteTypes.discard.keepEditing", bundle: .module) }
            }
            .alert(Text("noteTypes.failed.title", bundle: .module), isPresented: Binding(
                get: { model.failure != nil }, set: { if !$0 { model.failure = nil } })) {
                Button(role: .cancel) { model.failure = nil } label: { Text("noteTypes.failed.ok", bundle: .module) }
            } message: {
                Text("noteTypes.failed.message", bundle: .module)
            }
    }

    private var subtitle: String {
        guard let counts = model.typeCounts[id] else { return "" }
        return NoteTypesStrings.subtitle(notes: counts.notes, cards: counts.cards, locale: locale)
    }
}

/// The Fields tab on iPad (W3, N22.7): a list with the Mac table's columns
/// (Name, Sort, Used On, Notes), the name taking the width it needs. A tap
/// opens the field's options in a popover; Edit gives drag handles and
/// delete buttons; a swipe deletes or restores. Nothing writes before Save.
struct NoteTypeFieldsTouchList: View {
    @Bindable var model: NoteTypesModel
    let id: Int64
    let lines: [FieldLine]
    @Environment(\.editMode) private var editMode
    @Environment(\.locale) private var locale

    private var editable: Bool { model.editable(id) }

    var body: some View {
        List {
            Section {
                ForEach(lines) { line in
                    row(line)
                        .deleteDisabled(line.deleted || line.deleteBlock != nil)
                        .swipeActions(edge: .trailing) {
                            if line.deleted {
                                Button { model.restoreField(id, row: line.id) } label: { Text("noteTypes.fields.restore", bundle: .module) }
                                    .tint(.accentColor)
                            } else if line.deleteBlock == nil {
                                Button(role: .destructive) { model.deleteField(id, row: line.id) } label: {
                                    Text("noteTypes.fields.delete", bundle: .module)
                                }
                            }
                        }
                }
                .onMove { offsets, destination in model.moveFields(id, fromOffsets: offsets, toOffset: destination) }
                .onDelete { offsets in
                    for offset in offsets { model.deleteField(id, row: lines[offset].id) }
                }
            } header: {
                header
            }
        }
        .listStyle(.plain)
        .disabled(!editable)
        .toolbar {
            ToolbarItemGroup(placement: .bottomBar) {
                Button { model.addField(id) } label: {
                    Label { Text("noteTypes.fields.add", bundle: .module) } icon: { Image(systemName: "plus") }
                }
                .disabled(!editable)
                .accessibilityIdentifier("noteTypes.fields.add")
                Spacer()
                EditButton()
                    .disabled(!editable)
                    .accessibilityIdentifier("noteTypes.fields.edit")
            }
        }
        // + adds a field and opens its options with the name to type (W3).
        .onChange(of: model.renaming) { _, row in
            guard let row else { return }
            model.cancelRename()
            model.optionsField = row
        }
        .accessibilityIdentifier("noteTypes.fields.table")
    }

    private var header: some View {
        HStack(spacing: Spacing.small) {
            Text("noteTypes.fields.name", bundle: .module)
            Spacer(minLength: Spacing.small)
            Text("noteTypes.fields.sort", bundle: .module)
                .frame(minWidth: ControlMetrics.minimumTarget)
            Text("noteTypes.fields.usedOn", bundle: .module)
                .frame(width: Self.usedOnWidth, alignment: .leading)
            Text("noteTypes.fields.notes", bundle: .module)
                .frame(width: Self.notesWidth, alignment: .leading)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.fields.header")
    }

    /// The Mac table's Used On and Notes column widths; Name takes the rest.
    private static let usedOnWidth: CGFloat = 200
    private static let notesWidth: CGFloat = 120

    private func row(_ line: FieldLine) -> some View {
        HStack(spacing: Spacing.small) {
            Button {
                model.selectedField[id] = line.id
                model.optionsField = line.id
            } label: {
                Text(verbatim: line.name)
                    .strikethrough(line.deleted)
                    .foregroundStyle(line.deleted ? .secondary : .primary)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(line.deleted || editMode?.wrappedValue.isEditing == true)
            .popover(isPresented: Binding(get: { model.optionsField == line.id }, set: { if !$0 { model.optionsField = nil } })) {
                NoteTypeFieldInspector(model: model, id: id)
                    .frame(minWidth: ContentWidth.primaryAction, minHeight: ContentWidth.primaryAction)
                    .presentationCompactAdaptation(.popover)
            }
            .accessibilityValue(line.deleted ? Text("noteTypes.fields.deleted", bundle: .module) : Text(verbatim: ""))
            .accessibilityIdentifier("noteTypes.fields.row.\(line.id)")
            Button {
                model.setSortField(id, row: line.id)
            } label: {
                Image(systemName: line.isSort ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(line.isSort ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(line.deleted)
            .accessibilityLabel(Text(NoteTypesStrings.sortBy(line.name, locale: locale)))
            .accessibilityAddTraits(line.isSort ? .isSelected : [])
            .accessibilityIdentifier("noteTypes.fields.sort.\(line.id)")
            Group {
                if let names = line.usedOn {
                    if names.isEmpty {
                        Text("noteTypes.fields.notUsed", bundle: .module).foregroundStyle(.secondary)
                    } else {
                        Text(verbatim: names.joined(separator: ", ")).foregroundStyle(line.deleted ? .secondary : .primary)
                    }
                } else {
                    Text(verbatim: "")
                }
            }
            .lineLimit(1)
            .frame(width: Self.usedOnWidth, alignment: .leading)
            .accessibilityIdentifier("noteTypes.fields.usedOn.\(line.id)")
            HStack(spacing: Spacing.xSmall) {
                if let notes = line.notes, let total = line.ofNotes {
                    ProgressView(value: Double(notes), total: Double(max(total, 1)))
                        .progressViewStyle(.linear)
                        .accessibilityHidden(true)
                    Text(notes, format: .number)
                        .monospacedDigit()
                        .foregroundStyle(line.deleted ? .secondary : .primary)
                        .strikethrough(line.deleted)
                }
            }
            .frame(width: Self.notesWidth, alignment: .leading)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("noteTypes.fields.notes.\(line.id)")
        }
        .listRowSeparator(.visible)
    }
}
#endif
