#if os(macOS)
import AppModel
import AppKit
import Settings
import SwiftUI

/// The shell's collection and navigation own the task; this scene owns only its window.
struct BackupRestoreWindow: View {
    let collection: ReviewCollection
    let configuration: LaunchConfiguration
    @Environment(\.dismissWindow) private var dismissWindow
    @Environment(\.scenePhase) private var scenePhase
    @State private var collectionSceneID = UUID()

    var body: some View {
        BackupRestoreSheet(collection: collection)
            .modifier(LaunchEnvironment(configuration: configuration))
            .background(RestoreWindowCloseObserver(close: close))
            .onChange(of: collection.navigation.sheet) { _, route in
                if route != .backupRestore { dismissWindow(id: BackupRestoreWindowLayout.id) }
            }
            .onChange(of: scenePhase, initial: true) { _, phase in
                if collection.updateSceneActivity(collectionSceneID, active: phase == .active, background: phase == .background) {
                    collection.setForeground(false)
                }
            }
            .onDisappear {
                close()
                if collection.removeSceneActivity(collectionSceneID) { collection.setForeground(false) }
            }
    }

    private func close() {
        // Standard Close, red control, Cmd-W and Cancel end the intent.
        // Only the explicit selected confirmation starts a restore.
        guard collection.navigation.sheet == .backupRestore else { return }
        if collection.backups.work == .restore { collection.backups.cancel() }
        collection.navigation.dismissSheet()
    }
}

/// Observe the real window: SwiftUI may retain a Window scene's content after Close.
/// Leave the system delegate, titlebar, controls and focus handling untouched.
private struct RestoreWindowCloseObserver: NSViewRepresentable {
    let close: () -> Void
    func makeNSView(context: Context) -> WindowView { WindowView() }
    func updateNSView(_ view: WindowView, context: Context) { view.close = close }
    final class WindowView: NSView {
        var close: (() -> Void)?
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            NotificationCenter.default.removeObserver(self, name: NSWindow.willCloseNotification, object: nil)
            if let window {
                NotificationCenter.default.addObserver(self, selector: #selector(windowClosing),
                    name: NSWindow.willCloseNotification, object: window)
            }
        }
        @objc private func windowClosing(_ notification: Notification) { close?() }
    }
}
#endif
