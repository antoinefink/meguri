import AppModel
import SwiftUI

/// Presents `AppNavigation.sheet` over the current screen (R25): Add, deck
/// dialogs, import and export, login. Only its recorded window host presents.
/// Settings and the main window share navigation and collection coordinators;
/// adaptive review still presents over its full-screen cover.
struct PresentationHost: ViewModifier {
    let navigation: AppNavigation
    let features: ShellFeatures
    let configuration: LaunchConfiguration
    let isActive: Bool
    var host: AppNavigation.SheetHost = .main

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(get: { isActive && navigation.sheetHost == host ? navigation.sheet : nil }, set: { if $0 == nil, isActive, navigation.sheetHost == host { navigation.dismissSheet() } })) { sheet in
                features.sheet(sheet, navigation)
                    #if os(macOS)
                    .accessibilityHidden(false)
                    #endif
                    // A Mac sheet takes its environment from its own window:
                    // the launch values again.
                    .modifier(LaunchEnvironment(configuration: configuration))
            }
    }
}
