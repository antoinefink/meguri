import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Save sheet (`notetypes/draft-save` D4, N22.5): the changes the save
/// makes, the losses first and in red with their counts, the sync paragraph
/// when Meguri Sync's collection would be replaced, then Cancel and the
/// confirm button D3 names. A destructive confirm is never the default, so
/// Return does nothing destructive. A window sheet on the Mac, a form sheet
/// on iPad.
public struct NoteTypeSaveSheet: View {
    private let model: NoteTypesModel
    @Environment(\.locale) private var locale

    public init(model: NoteTypesModel) {
        self.model = model
    }

    public var body: some View {
        Group {
            if let question = model.question {
                content(question)
            }
        }
        // A save question ends only by Cancel or the confirm button.
        .interactiveDismissDisabled(true)
        #if os(iOS)
        .presentationSizing(.form)
        #endif
        .onDisappear { model.questionClosed() }
    }

    @ViewBuilder private func content(_ question: NoteTypesModel.Question) -> some View {
        #if os(macOS)
        VStack(alignment: .leading, spacing: Spacing.medium) {
            details(question)
            HStack(spacing: Spacing.small) {
                Spacer()
                cancel
                confirm(question)
            }
        }
        .padding(Spacing.large)
        .frame(width: ContentWidth.readable * 0.7)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.list.saveSheet")
        #else
        NavigationStack {
            ScrollView {
                details(question)
                    .padding(Spacing.large)
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { cancel }
                ToolbarItem(placement: question.decision.destructive ? .destructiveAction : .confirmationAction) { confirm(question) }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.list.saveSheet")
        #endif
    }

    private func details(_ question: NoteTypesModel.Question) -> some View {
        let device = NoteTypesStrings.device(locale: locale)
        return VStack(alignment: .leading, spacing: Spacing.medium) {
            Text(verbatim: NoteTypesStrings.saveTitle(question.name, locale: locale))
                .font(.headline)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("noteTypes.list.saveSheetTitle")
            GroupBox {
                VStack(alignment: .leading, spacing: Spacing.small) {
                    ForEach(Array(question.rows.enumerated()), id: \.offset) { index, row in
                        SaveRowView(row: row)
                            .accessibilityIdentifier("noteTypes.list.saveSheetRow.\(index)")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            if question.decision.syncParagraph {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    Text("\(Text("noteTypes.save.sync.title", bundle: .module).bold()) \(NoteTypesStrings.syncBody(device: device, locale: locale))")
                    Text("noteTypes.save.sync.others", bundle: .module)
                }
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("noteTypes.list.saveSheetSync")
            }
            Text(verbatim: NoteTypesStrings.backup(device: device, locale: locale))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("noteTypes.list.saveSheetBackup")
        }
    }

    private var cancel: some View {
        Button(role: .cancel) { model.cancelQuestion() } label: { Text("noteTypes.save.cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .accessibilityIdentifier("noteTypes.list.saveSheetCancel")
    }

    @ViewBuilder private func confirm(_ question: NoteTypesModel.Question) -> some View {
        let label = question.decision.confirm == .saveAndUpload
            ? Text("noteTypes.save.confirm.saveAndUpload", bundle: .module) : Text("noteTypes.save.confirm.save", bundle: .module)
        if question.decision.destructive {
            Button(role: .destructive) { model.confirm() } label: { label }
                .disabled(!model.canConfirm)
                .accessibilityIdentifier("noteTypes.list.saveSheetConfirm")
        } else {
            Button { model.confirm() } label: { label }
                .keyboardShortcut(.defaultAction)
                .disabled(!model.canConfirm)
                .accessibilityIdentifier("noteTypes.list.saveSheetConfirm")
        }
    }
}

/// One D4 row: a symbol, the change, and its detail lines (red when content is lost).
private struct SaveRowView: View {
    let row: NoteTypeSaveRow
    @Environment(\.locale) private var locale

    var body: some View {
        let line = NoteTypesStrings.line(row, locale: locale)
        Label {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(verbatim: line.title)
                ForEach(line.details, id: \.self) { detail in
                    Text(verbatim: detail)
                        .font(.callout)
                        .foregroundStyle(row.loses ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: symbol)
                .foregroundStyle(row.loses ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
                .accessibilityHidden(true)
        }
        .accessibilityElement(children: .combine)
    }

    private var symbol: String {
        switch row.kind {
        case .deleteCardType, .deleteField: "trash"
        case .addCardType, .addField: "plus.circle"
        case .reorderFields, .reorderCardTypes: "arrow.up.arrow.down"
        case .sortBy: "arrow.up.and.down.text.horizontal"
        }
    }
}
