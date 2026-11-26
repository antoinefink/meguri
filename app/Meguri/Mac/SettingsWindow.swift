#if os(macOS)
import AppModel
import Settings
import SwiftUI

/// Owns only this window's presentation surface; collection/navigation stay app-owned.
struct SettingsWindow: View {
    let collection: ReviewCollection
    let features: ShellFeatures
    let configuration: LaunchConfiguration
    @Environment(\.scenePhase) private var scenePhase
    @State private var collectionSceneID = UUID()
    private var context: NativeCommandContext {
        NativeCommandContext(collection: collection, host: .settings).observingReviewAvailability()
    }
    var body: some View {
        // N15.7: as System Settings, a fixed width and a height the person can
        // change. The Settings scene's resizability (`contentSize`, its default)
        // takes the window's minimum and maximum from this frame.
        MacSettingsView(collection: collection, present: context.present)
            .frame(width: SettingsWindowSize.width)
            .frame(minHeight: SettingsWindowSize.minimumHeight, maxHeight: .infinity)
            .focusedSceneValue(\.nativeCommandContext, context)
            .modifier(PresentationHost(navigation: collection.navigation, features: features,
                configuration: configuration, isActive: true, host: .settings))
            .modifier(LaunchEnvironment(configuration: configuration))
            .background(SettingsToolbarStyle())
            .onChange(of: scenePhase, initial: true) { _, phase in
                if collection.updateSceneActivity(collectionSceneID, active: phase == .active, background: phase == .background) {
                    collection.setForeground(false)
                }
            }
            .onDisappear {
                if collection.removeSceneActivity(collectionSceneID) { collection.setForeground(false) }
            }
    }
}

/// N15.7's sizes: the width is fixed; the default height holds the Sync
/// section signed in or out.
enum SettingsWindowSize {
    static let width: CGFloat = 760
    static let minimumHeight: CGFloat = 480
    static let defaultHeight: CGFloat = 640
}

/// N15.5 (resume fix), a documented exception: SwiftUI gives the Settings
/// scene's window the preference toolbar style, which centres the title, puts
/// a pushed page's back button in a row under it and, once a page was pushed,
/// moves the pane's title from the leading edge to the centre. The unified
/// style is System Settings' own: the back control at the leading edge of the
/// title bar, the page title beside it. `.windowToolbarStyle(.unified)` on the
/// Settings scene has no effect, so the window's style is set here, and set
/// again if SwiftUI resets it (verified in the test VM, `settings/sync/window/…`).
private struct SettingsToolbarStyle: NSViewRepresentable {
    func makeNSView(context: Context) -> WindowView { WindowView() }
    func updateNSView(_ view: WindowView, context: Context) { view.apply() }
    final class WindowView: NSView {
        private var observation: NSKeyValueObservation?
        private var styleObservation: NSKeyValueObservation?
        private var sized = false
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            observation = window?.observe(\.toolbarStyle) { window, _ in
                MainActor.assumeIsolated { if window.toolbarStyle != .unified { window.toolbarStyle = .unified } }
            }
            styleObservation = window?.observe(\.styleMask) { window, _ in
                MainActor.assumeIsolated { if !window.styleMask.contains(.resizable) { window.styleMask.insert(.resizable) } }
            }
            apply()
        }
        func apply() {
            guard let window else { return }
            if window.toolbarStyle != .unified { window.toolbarStyle = .unified }
            // N15.7, a documented exception: SwiftUI builds the Settings window
            // without a drag-to-resize edge, whatever the content allows. Add it.
            // AppKit then saves the frame under the window's autosave name. The
            // first open after this change gets the default height once, since a
            // frame saved earlier is the old fixed size.
            if !window.styleMask.contains(.resizable) { window.styleMask.insert(.resizable) }
            guard !sized else { return }
            sized = true
            let key = "meguri.settingsWindow.defaultHeight"
            guard !UserDefaults.standard.bool(forKey: key) else { return }
            // After SwiftUI has sized and restored the window: done earlier, it puts the old frame back.
            Task { @MainActor [weak window] in
                try? await Task.sleep(for: .milliseconds(300))
                guard let window, let screen = window.screen ?? NSScreen.main else { return }
                UserDefaults.standard.set(true, forKey: key)
                let content = window.contentRect(forFrameRect: window.frame).height
                let chrome = window.frame.height - content
                let height = min(SettingsWindowSize.defaultHeight, screen.visibleFrame.height - chrome)
                guard height > content else { return }
                // Grow downward from the same top edge, kept within the visible screen.
                var frame = window.frame
                frame.origin.y -= height - content
                frame.size.height = height + chrome
                frame.origin.y = max(frame.origin.y, screen.visibleFrame.minY)
                window.setFrame(frame, display: true)
            }
        }
    }
}
#endif
