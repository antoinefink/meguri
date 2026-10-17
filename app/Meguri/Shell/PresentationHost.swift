import AppModel
import SwiftUI

/// Presents `AppNavigation.sheet` over the current screen (R25): Add, deck
/// dialogs, import and export, login. The screen underneath stays as it is,
/// so closing the sheet returns to exactly that screen; nothing opens a
/// window (R6). Only the active host presents: the root's, or, while the tab
/// layout's review covers the screen, the review's (a covered view cannot
/// present).
struct PresentationHost: ViewModifier {
    let navigation: AppNavigation
    let features: ShellFeatures
    let configuration: LaunchConfiguration
    let isActive: Bool

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(get: { isActive ? navigation.sheet : nil }, set: { if $0 == nil { navigation.dismissSheet() } })) { sheet in
                features.sheet(sheet, navigation)
                    // A Mac sheet takes its environment from its own window:
                    // the launch values again.
                    .modifier(LaunchEnvironment(configuration: configuration))
            }
    }
}
