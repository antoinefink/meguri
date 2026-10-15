import AppModel
import SwiftUI

/// The app entry (KTD10). Normal launches use the default launch
/// configuration. In Debug and Profile builds the test hooks first override it
/// from the launch arguments (KTD18, R1963); this file is the only place
/// outside `TestHooks/` that names them.
@main
struct MeguriApp: App {
    private let configuration: LaunchConfiguration
    /// MetricKit's reports, kept on the device for Copy Debug Info (R1867).
    private let fieldDiagnostics = FieldDiagnosticsSubscriber.start()
    #if MEGURI_TEST_HOOKS
    private let hooks: TestHooks.Session
    #endif

    init() {
        var configuration = LaunchConfiguration()
        #if MEGURI_TEST_HOOKS
        hooks = TestHooks.prepare(&configuration)
        #endif
        self.configuration = configuration
    }

    var body: some Scene {
        #if os(macOS)
        Window("Meguri", id: "main") {
            root
        }
        .defaultSize(WindowMetrics.standard)
        #else
        WindowGroup {
            root
        }
        #endif
    }

    @ViewBuilder private var root: some View {
        #if MEGURI_TEST_HOOKS
        TestHooks.Root(session: hooks) {
            RootView(configuration: configuration)
        }
        #else
        RootView(configuration: configuration)
        #endif
    }
}
