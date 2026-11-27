import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The line under the table (`browser/workspace` V11, N21.3): the count of
/// rows with the first number bold, "Searching…" while a search runs past
/// half a second, the selection, and the column set at the trailing edge
/// (V2.2).
struct BrowseStatusLine: View {
    let model: BrowseModel
    let editNoteType: (Int64) -> Void
    @Environment(\.locale) private var locale

    var body: some View {
        let status = BrowseStatus(results: model.results, query: model.query)
        HStack(spacing: Spacing.small) {
            if model.results.showsProgress {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel(Text("Searching Browse", bundle: .module))
                    .accessibilityIdentifier("browse.navigation.progress")
            }
            Text(BrowseStrings.status(status, locale: locale))
                .monospacedDigit()
                .lineLimit(1)
                .accessibilityIdentifier("browse.status.text")
            Spacer(minLength: Spacing.small)
            columnSet
        }
        .font(.footnote)
        .foregroundStyle(.secondary)
        .padding(.horizontal, Spacing.small)
        .padding(.vertical, Spacing.xxSmall)
    }

    /// V2.2, V11: the set's name with a chevron, as a menu: in a note
    /// type's own set Use Standard Columns and Edit Note Type…; in the
    /// standard set Reset to Standard Columns (U272), enabled once the set
    /// differs from V2.3's default columns.
    @ViewBuilder private var columnSet: some View {
        let set = model.results.columnSet
        let own: Int64? = if let set, case .notetype(let id) = set.set, set.own { id } else { nil }
        Menu {
            if let own {
                Button { model.results.useStandardColumns() } label: { Text("browse.columns.useStandard", bundle: .module) }
                    .accessibilityIdentifier("browse.status.useStandard")
                Button { editNoteType(own) } label: { Text("browse.columns.editNoteType", bundle: .module) }
                    .accessibilityIdentifier("browse.status.editNoteType")
            } else {
                Button { model.results.resetColumns() } label: { Text("browse.columns.reset", bundle: .module) }
                    .disabled(!model.results.canResetColumns)
                    .accessibilityIdentifier("browse.status.resetColumns")
            }
        } label: {
            if let own {
                Text(BrowseStrings.ownColumns(model.notetypes.first { $0.id == own }?.name ?? "", locale: locale))
            } else {
                Text("browse.columns.standard", bundle: .module)
            }
        }
        .menuStyle(.button)
        .buttonStyle(.borderless)
        .fixedSize()
        .disabled(set == nil)
        .accessibilityIdentifier("browse.status.columnSet")
    }
}
