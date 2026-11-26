import AppModel
import DesignSystem
import SwiftUI

/// W12, D4: a native window/form sheet. No destructive keyboard default.
struct NoteTypeDeletionSheet: View {
    let model: NoteTypesModel
    let question: NoteTypesModel.DeletionQuestion
    @Environment(\.locale) private var locale

    var body: some View {
        #if os(macOS)
        VStack(alignment: .leading, spacing: Spacing.medium) {
            details
            HStack {
                Spacer()
                cancel
                confirm
            }
        }
        .padding(Spacing.large)
        .frame(idealWidth: ContentWidth.readable * 0.7)
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.delete.sheet")
        #else
        NavigationStack {
            Form { details }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { cancel }
                    ToolbarItem(placement: .destructiveAction) { confirm }
                }
        }
        .presentationDetents([.large])
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.delete.sheet")
        #endif
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            Text(verbatim: NoteTypesStrings.deleteTitle(question.name, locale: locale))
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("noteTypes.delete.title")
            Text(verbatim: NoteTypesStrings.deleteLosses(question.counts, locale: locale))
                .accessibilityIdentifier("noteTypes.delete.counts")
            if question.service == .meguriSync, model.meguriSyncState == .ready {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    if model.meguriUnreachable {
                        Text("\(Text("noteTypes.save.unreachable.title", bundle: .module).bold()) \(NoteTypesStrings.unreachableBody(device: device, locale: locale))")
                    } else {
                        Text("\(Text("noteTypes.save.sync.title", bundle: .module).bold()) \(NoteTypesStrings.syncBody(device: device, locale: locale))")
                    }
                    Text("noteTypes.save.sync.others", bundle: .module)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("noteTypes.delete.sync")
            }
            Text(verbatim: NoteTypesStrings.backup(device: device, locale: locale))
                .foregroundStyle(.secondary)
            if let unavailable {
                Text(verbatim: unavailable)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("noteTypes.delete.unavailable")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private var device: String { NoteTypesStrings.device(locale: locale) }

    private var unavailable: String? {
        guard question.service == .meguriSync else { return nil }
        switch model.meguriSyncState {
        case .ready: return nil
        case .notSetUp: return NoteTypesStrings.text("noteTypes.save.notSetUp", locale: locale)
        case .busy: return NoteTypesStrings.text("noteTypes.save.syncBusy", locale: locale)
        case .uploadPending: return NoteTypesStrings.text("noteTypes.save.uploadPending", locale: locale)
        }
    }

    private var cancel: some View {
        Button(role: .cancel) { model.cancelQuestion() } label: { Text("noteTypes.save.cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .disabled(model.confirming)
            .accessibilityIdentifier("noteTypes.delete.cancel")
    }

    private var confirm: some View {
        Button(role: .destructive) { model.confirmDeletion() } label: {
            if question.service == .meguriSync, !model.meguriUnreachable {
                Text("noteTypes.delete.upload", bundle: .module)
            } else {
                Text("noteTypes.delete.confirm", bundle: .module)
            }
        }
        .disabled(!model.canConfirmDeletion)
        .help(Text(verbatim: unavailable ?? ""))
        .accessibilityIdentifier("noteTypes.delete.confirm")
    }
}

/// The same action on Mac and on compact/wide touch lists.
struct NoteTypeDeletionMenu: View {
    let model: NoteTypesModel
    let id: Int64

    var body: some View {
        Button(role: .destructive) { model.delete(id) } label: {
            Label { Text("noteTypes.delete.menu", bundle: .module) } icon: { Image(systemName: "trash") }
        }
        .disabled(!model.canDelete(id))
        .accessibilityIdentifier("noteTypes.list.delete")
    }
}
