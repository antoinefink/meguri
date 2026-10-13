import SwiftUI

extension View {
    /// Asks before a destructive action (delete a deck, empty cards): the
    /// system confirmation dialog with the title shown, the action in the
    /// destructive role and Cancel (R1995). `action` runs only on
    /// confirmation.
    public func destructiveConfirmation(
        _ title: Text,
        isPresented: Binding<Bool>,
        actionTitle: Text,
        message: Text? = nil,
        action: @escaping () -> Void
    ) -> some View {
        confirmationDialog(title, isPresented: isPresented, titleVisibility: .visible) {
            Button(role: .destructive, action: action) { actionTitle }
            Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
        } message: {
            if let message { message }
        }
    }
}
