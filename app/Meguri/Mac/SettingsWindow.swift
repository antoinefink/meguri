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
            .frame(minWidth: WindowMetrics.minimum.width, minHeight: WindowMetrics.minimum.height)
            .focusedSceneValue(\.nativeCommandContext, context)
            .modifier(PresentationHost(navigation: collection.navigation, features: features,
                configuration: configuration, isActive: true, host: .settings))
            .modifier(LaunchEnvironment(configuration: configuration))
    }
}
#endif
