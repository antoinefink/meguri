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
            .background(SettingsToolbarStyle())
    }
}

/// N15.5 (resume fix): SwiftUI gives the Settings scene's window the preference
/// toolbar style, which centres the title, puts a pushed page's back button in
/// a row under it and, once a page was pushed, moves the pane's title from the
/// leading edge to the centre. The unified style is System Settings' own: the
/// back control at the leading edge of the title bar, the page title beside it.
private struct SettingsToolbarStyle: NSViewRepresentable {
    func makeNSView(context: Context) -> WindowView { WindowView() }
    func updateNSView(_ view: WindowView, context: Context) { view.apply() }
    final class WindowView: NSView {
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); apply() }
        func apply() {
            guard let window, window.toolbarStyle != .unified else { return }
            window.toolbarStyle = .unified
        }
    }
}
#endif
