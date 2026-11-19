import AppModel
import Home
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

    private var hostedSheet: SheetRoute? {
        guard isActive, navigation.sheetHost == host else { return nil }
        #if os(macOS)
        if features.nativeMacWorkspace, case .add = navigation.sheet { return nil }
        #endif
        return navigation.sheet
    }

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    /// N17.1: on an iPhone at regular width (the Duo opened wide) Add Note
    /// covers the screen instead of floating as a sheet.
    private var coversScreen: Bool {
        #if os(iOS)
        guard case .add = hostedSheet else { return false }
        return AddNotePresentation.coversScreen(phone: UIDevice.current.userInterfaceIdiom == .phone, regularWidth: sizeClass == .regular)
        #else
        false
        #endif
    }
    private func binding(cover: Bool) -> Binding<SheetRoute?> {
        Binding(get: { coversScreen == cover ? hostedSheet : nil },
                set: { if $0 == nil, hostedSheet != nil, coversScreen == cover { navigation.dismissSheet() } })
    }

    func body(content: Content) -> some View {
        content
            .sheet(item: binding(cover: false)) { sheet in presented(sheet) }
            #if os(iOS)
            .fullScreenCover(item: binding(cover: true)) { sheet in presented(sheet) }
            #endif
            .modifier(CollectionImportPicker(coordinator: features.collectionEntry, hosts: isActive && navigation.sheetHost == host))
    }

    private func presented(_ sheet: SheetRoute) -> some View {
        features.sheet(sheet, navigation)
            #if os(macOS)
            .accessibilityHidden(false)
            #endif
            // A Mac sheet takes its environment from its own window:
            // the launch values again.
            .modifier(LaunchEnvironment(configuration: configuration))
    }
}
