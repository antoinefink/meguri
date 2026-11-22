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

    /// V2.2: the set's name; in a note type's own set, its menu.
    @ViewBuilder private var columnSet: some View {
        let set = model.results.columnSet
        if let set, case .notetype(let id) = set.set, set.own {
            let name = model.notetypes.first { $0.id == id }?.name ?? ""
            Menu {
                Button { model.results.useStandardColumns() } label: { Text("browse.columns.useStandard", bundle: .module) }
                    .accessibilityIdentifier("browse.status.useStandard")
                Button { editNoteType(id) } label: { Text("browse.columns.editNoteType", bundle: .module) }
                    .accessibilityIdentifier("browse.status.editNoteType")
            } label: {
                Text(BrowseStrings.ownColumns(name, locale: locale))
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .fixedSize()
            .accessibilityIdentifier("browse.status.columnSet")
        } else {
            Text("browse.columns.standard", bundle: .module)
                .accessibilityIdentifier("browse.status.columnSet")
        }
    }
}
