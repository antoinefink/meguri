#if os(macOS)
import DesignSystem
import SwiftUI

/// The same columns as the field page: Add supplies native pickers and Edit
/// supplies read-only values. Both native windows share the form's layout.
struct NoteFormSelectionRows<NoteType: View, Deck: View>: View {
    @ViewBuilder let noteType: () -> NoteType
    @ViewBuilder let deck: () -> Deck
    var body: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: Spacing.xSmall, verticalSpacing: Spacing.xSmall) {
            GridRow {
                Text("Note type:", bundle: .module).frame(width: 140, alignment: .trailing)
                noteType().frame(maxWidth: .infinity, alignment: .leading)
            }
            GridRow {
                Text("Deck:", bundle: .module).frame(width: 140, alignment: .trailing)
                deck().frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Spacing.medium)
    }
}
/// Native rows inserted among the retained rich fields use the same label
/// column and gap as the selection rows and the Mac field page.
struct NoteFormFieldRow<Label: View, Content: View>: View {
    @ViewBuilder let label: () -> Label
    @ViewBuilder let content: () -> Content
    var body: some View {
        Grid(alignment: .topLeading, horizontalSpacing: Spacing.xSmall) {
            GridRow {
                label().frame(width: 140, alignment: .trailing).padding(.top, Spacing.xxSmall)
                content().frame(maxWidth: .infinity, alignment: .topLeading)
            }
        }
    }
}
#endif
