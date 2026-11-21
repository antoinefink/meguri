import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Note Types section (`native-interface-contract` N22.1): the list of
/// note types beside the selected one's detail, with Fields and Cards in the
/// toolbar, then Revert and Save at its trailing edge (`notetypes/workspace`
/// W2, `notetypes/draft-save` D1, D3, D5). The shell shows it as the section's
/// root in the sidebar layout (Mac, iPad); the model outlives it.
public struct NoteTypesView: View {
    @Bindable private var model: NoteTypesModel
    /// The Cards tab, until U282 builds it.
    private let cards: (Int64) -> AnyView
    @Environment(\.locale) private var locale

    public init(model: NoteTypesModel, cards: @escaping (Int64) -> AnyView = { _ in AnyView(CardsPlaceholder()) }) {
        self.model = model
        self.cards = cards
    }

    public var body: some View {
        split
            .navigationTitle(title)
            .navigationSubtitle(subtitle)
            .toolbar { toolbar }
            .confirmationDialog(Text("noteTypes.discard.title", bundle: .module), isPresented: $model.confirmingRevert,
                                titleVisibility: .visible) {
                Button(role: .destructive) {
                    if let id = model.selection { model.revert(id) }
                } label: { Text("noteTypes.discard.discard", bundle: .module) }
                    .accessibilityIdentifier("noteTypes.list.discard.discard")
                Button(role: .cancel) { model.confirmingRevert = false } label: { Text("noteTypes.discard.keepEditing", bundle: .module) }
            }
            .alert(Text("noteTypes.failed.title", bundle: .module), isPresented: Binding(
                get: { model.failure != nil }, set: { if !$0 { model.failure = nil } })) {
                Button(role: .cancel) { model.failure = nil } label: { Text("noteTypes.failed.ok", bundle: .module) }
            } message: {
                Text("noteTypes.failed.message", bundle: .module)
            }
            .onAppear { model.appear() }
    }

    @ViewBuilder private var split: some View {
        #if os(macOS)
        HSplitView {
            NoteTypeListColumn(model: model)
                .frame(minWidth: Self.listMinimum, idealWidth: Self.listIdeal, maxWidth: Self.listMaximum)
            detail
                .frame(minWidth: ContentWidth.primaryAction, maxWidth: .infinity, maxHeight: .infinity)
        }
        #else
        HStack(spacing: .zero) {
            NoteTypeListColumn(model: model)
                .frame(width: ContentWidth.primaryAction)
            Divider()
            detail
        }
        #endif
    }

    /// The list column fits the longest stock name with its count (N22.1).
    private static let listMinimum: CGFloat = 200
    private static let listIdeal: CGFloat = 260
    private static let listMaximum: CGFloat = 320

    @ViewBuilder private var detail: some View {
        if let id = model.selection {
            if model.draft(id) != nil {
                VStack(spacing: .zero) {
                    if model.changedElsewhere.contains(id) {
                        ChangedElsewhereRow(model: model, id: id)
                        Divider()
                    }
                    switch model.tab {
                    case .fields: NoteTypeFieldsTab(model: model, id: id)
                    case .cards: cards(id)
                    }
                }
            } else if model.recordFailed.contains(id) {
                ContentUnavailableView {
                    Label { Text("noteTypes.detail.failed.title", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
                } actions: {
                    Button { model.reload(id) } label: { Text("noteTypes.tryAgain", bundle: .module) }
                }
            } else {
                DelayedProgress()
            }
        } else if model.phase == .ready {
            ContentUnavailableView {
                Label { Text("noteTypes.detail.none", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
            }
        } else {
            Color.clear
        }
    }

    private var title: Text {
        guard let id = model.selection, let name = model.draft(id)?.name ?? model.item(id)?.name else {
            return Text("noteTypes.title", bundle: .module)
        }
        return Text(verbatim: name)
    }

    private var subtitle: String {
        guard let id = model.selection, let counts = model.typeCounts[id] else { return "" }
        return NoteTypesStrings.subtitle(notes: counts.notes, cards: counts.cards, locale: locale)
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            Picker(selection: $model.tab) {
                Text("noteTypes.tab.fields", bundle: .module).tag(NoteTypesModel.Tab.fields)
                    .accessibilityIdentifier("noteTypes.list.tabFields")
                Text("noteTypes.tab.cards", bundle: .module).tag(NoteTypesModel.Tab.cards)
                    .accessibilityIdentifier("noteTypes.list.tabCards")
            } label: {
                Text("noteTypes.tabs", bundle: .module)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .disabled(model.selection == nil)
            .accessibilityIdentifier("noteTypes.list.tabs")
        }
        ToolbarItemGroup(placement: .primaryAction) {
            if let id = model.selection, let progress = model.progress, progress.notetypeId == id {
                SaveProgressView(progress: progress)
            }
            Button {
                model.confirmingRevert = true
            } label: {
                Text("noteTypes.revert", bundle: .module)
            }
            .disabled(model.selection.map { !(model.isEdited($0) && model.editable($0)) } ?? true)
            .accessibilityIdentifier("noteTypes.list.revert")
            Button {
                model.save()
            } label: {
                if let id = model.selection, model.saveAsks(id) {
                    Text("noteTypes.saveAsks", bundle: .module)
                } else {
                    Text("noteTypes.save", bundle: .module)
                }
            }
            // ⌘S is File › Save's (N4).
            .buttonStyle(.borderedProminent)
            .disabled(model.selection.map { !model.canSave($0) } ?? true)
            .accessibilityIdentifier("noteTypes.list.save")
        }
    }
}

/// The Cards tab until U282 builds it.
public struct CardsPlaceholder: View {
    public init() {}
    public var body: some View {
        ContentUnavailableView {
            Label { Text("noteTypes.cards.title", bundle: .module) } icon: { Image(systemName: "rectangle.on.rectangle") }
        } description: {
            Text("noteTypes.cards.message", bundle: .module)
        }
        .accessibilityIdentifier("noteTypes.templates.placeholder")
    }
}

/// D5 step 5: "Updating 212 of 412 notes", shown once a save has run for a moment.
private struct SaveProgressView: View {
    let progress: NoteTypesModel.Progress
    @Environment(\.locale) private var locale
    @State private var visible = false

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            ProgressView().controlSize(.small)
            if let total = progress.total {
                Text(NoteTypesStrings.progress(completed: progress.completed, total: total, locale: locale))
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .opacity(visible ? 1 : 0)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("noteTypes.saving", bundle: .module))
        .accessibilityIdentifier("noteTypes.list.progress")
        .task {
            try? await Task.sleep(for: .milliseconds(500))
            visible = true
        }
    }
}

/// A progress indicator that shows only once a read has taken about half a second.
struct DelayedProgress: View {
    @State private var visible = false
    var body: some View {
        ProgressView()
            .opacity(visible ? 1 : 0)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .task {
                try? await Task.sleep(for: .milliseconds(500))
                visible = true
            }
    }
}

/// D1: the note type changed elsewhere while its draft is open.
private struct ChangedElsewhereRow: View {
    let model: NoteTypesModel
    let id: Int64

    var body: some View {
        HStack(spacing: Spacing.small) {
            Label {
                Text("noteTypes.changed.message", bundle: .module)
            } icon: {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
            }
            Spacer(minLength: Spacing.small)
            Button { model.keepEditing(id) } label: { Text("noteTypes.changed.keepEditing", bundle: .module) }
                .accessibilityIdentifier("noteTypes.list.changed.keepEditing")
            Button { model.reload(id) } label: { Text("noteTypes.changed.reload", bundle: .module) }
                .accessibilityIdentifier("noteTypes.list.changed.reload")
        }
        .padding(.horizontal, Spacing.medium)
        .padding(.vertical, Spacing.xSmall)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.list.changed")
    }
}

// MARK: The list (W2)

private struct NoteTypeListColumn: View {
    @Bindable var model: NoteTypesModel

    var body: some View {
        VStack(spacing: .zero) {
            FilterField(text: $model.filter, prompt: NoteTypesStrings.text("noteTypes.list.filter"))
                .padding(.horizontal, Spacing.xSmall)
                .padding(.vertical, Spacing.xSmall)
                .accessibilityIdentifier("noteTypes.list.filter")
            list
        }
    }

    private var list: some View {
        List(selection: $model.selection) {
            ForEach(model.filteredItems) { item in
                NoteTypeListRow(item: item, edited: model.isEdited(item.id))
                    .tag(item.id)
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #else
        .listStyle(.plain)
        #endif
        .accessibilityIdentifier("noteTypes.list.table")
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
    }
}

private struct NoteTypeListRow: View {
    let item: NoteTypesModel.Item
    let edited: Bool

    var body: some View {
        HStack(spacing: Spacing.xSmall) {
            Text(verbatim: item.name)
                .lineLimit(1)
            if edited {
                Text("noteTypes.list.edited", bundle: .module)
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .accessibilityIdentifier("noteTypes.list.edited.\(item.id)")
            }
            Spacer(minLength: Spacing.xSmall)
            if let notes = item.notes {
                Text(notes, format: .number)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    // The count is never cut; the name truncates first.
                    .fixedSize()
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("noteTypes.list.row.\(item.id)")
    }
}
