#if os(macOS)
import AppModel
import Settings
import SwiftUI

/// Owns only this window's presentation surface; collection/navigation stay app-owned.
struct SettingsWindow: View {
    let collection: ReviewCollection
    let features: ShellFeatures
    let configuration: LaunchConfiguration
    private var context: NativeCommandContext { .init(collection: collection, host: .settings) }
    var body: some View {
        MacSettingsView(collection: collection, present: context.present)
            .frame(minWidth: 680, idealWidth: 760, minHeight: 480, idealHeight: 560)
            .focusedSceneValue(\.nativeCommandContext, context)
            .modifier(PresentationHost(navigation: collection.navigation, features: features,
                configuration: configuration, isActive: true, host: .settings))
            .modifier(LaunchEnvironment(configuration: configuration))
    }
}
#endif
