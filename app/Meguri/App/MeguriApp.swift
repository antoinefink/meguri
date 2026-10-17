import AppModel
import SwiftUI

/// The app entry (KTD10): one window, holding every feature (R6, R7, R110).
/// On the Mac a `Window` scene (no New Window, Window > Meguri reopens it)
/// with the app delegate that keeps Meguri running while an integration is
/// on (R11); on iPhone and iPad a `WindowGroup` limited to one scene by
/// Info-iOS.plist. There is no `Settings` scene: Settings is a section of
/// the window, and ⌘, selects it (U159).
///
/// Normal launches use the default launch configuration and restore the
/// device's UI state. In Debug and Profile builds the test hooks first
/// override them from the launch arguments (KTD18, R1963); this file is the
/// only place outside `TestHooks/` that names them.
@main
struct MeguriApp: App {
    #if os(macOS)
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    #endif
    private let configuration: LaunchConfiguration
    private let features: ShellFeatures
    private let uiStateStore: UIStateStore
    /// Above the layout switch, for the app's whole run (KTD10).
    @State private var navigation: AppNavigation
    /// MetricKit's reports, kept on the device for Copy Debug Info (R1867).
    private let fieldDiagnostics = FieldDiagnosticsSubscriber.start()
    #if MEGURI_TEST_HOOKS
    private let hooks: TestHooks.Session
    #endif

    init() {
        var configuration = LaunchConfiguration()
        var features = ShellFeatures.standard
        var uiStateStore = UIStateStore.standard
        #if MEGURI_TEST_HOOKS
        let hooks = TestHooks.prepare(&configuration)
        self.hooks = hooks
        if hooks.startsFresh {
            uiStateStore = UIStateStore(defaults: nil)
        }
        if let shell = hooks.shell {
            features = shell.features
        }
        #endif
        let navigation = uiStateStore.load().map(AppNavigation.init(restoring:)) ?? AppNavigation()
        #if MEGURI_TEST_HOOKS
        hooks.start(navigation)
        #endif
        self.configuration = configuration
        self.features = features
        self.uiStateStore = uiStateStore
        _navigation = State(initialValue: navigation)
    }

    var body: some Scene {
        #if os(macOS)
        Window("Meguri", id: WindowMetrics.mainWindowID) {
            root
        }
        .defaultSize(WindowMetrics.standard)
        .windowResizability(.contentMinSize)
        .commands {
            SidebarCommands()
            MainWindowCommands()
        }
        #else
        WindowGroup {
            root
        }
        .commands {
            SidebarCommands()
        }
        #endif
    }

    @ViewBuilder private var root: some View {
        #if MEGURI_TEST_HOOKS
        TestHooks.Root(session: hooks, navigation: navigation) {
            RootView(configuration: configuration, navigation: navigation, features: features, uiStateStore: uiStateStore)
        }
        #else
        RootView(configuration: configuration, navigation: navigation, features: features, uiStateStore: uiStateStore)
        #endif
    }
}
