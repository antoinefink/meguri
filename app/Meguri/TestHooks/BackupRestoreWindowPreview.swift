#if MEGURI_TEST_HOOKS && os(macOS)
import AppModel
import DesignSystem
import Settings
import SwiftUI

extension TestHooks {
    /// Only the data is synthetic: route launches use the production Window scene.
    @MainActor enum BackupRestoreWindowPreview {
        static var state: PreviewState?

        static func presentation(_ state: PreviewState) -> InitialPresentation {
            self.state = state
            return .screen { Launcher() }
        }

        struct Launcher: View {
            @Environment(\.openWindow) private var openWindow
            var body: some View {
                Color.clear.task { openWindow(id: BackupRestoreWindowLayout.id) }
            }
        }

        struct Content: View {
            let state: PreviewState
            let configuration: LaunchConfiguration
            @Environment(\.dismissWindow) private var dismissWindow
            var body: some View {
                BackupsPreview.restore(state, cancel: { dismissWindow(id: BackupRestoreWindowLayout.id) })
                    .modifier(LaunchEnvironment(configuration: configuration))
            }
        }
    }
}
#endif
