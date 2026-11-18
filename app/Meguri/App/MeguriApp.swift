import AppModel
import SwiftUI
#if os(macOS)
import AppKit
#elseif os(iOS)
import UIKit
#endif

/// The app entry (KTD10): one window, holding every feature (R6, R7, R110).
/// On the Mac a `Window` scene (no New Window, Window > Meguri reopens it)
/// with the app delegate that keeps Meguri running while an integration is
/// on (R11); on iPhone and iPad a `WindowGroup` limited to one scene by
/// Info-iOS.plist. The native Mac Settings scene shares the app-owned
/// collection (native interface contract N5, Antoine's override of R6/R110).
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
    private let collection: ReviewCollection?
    private let uiStateStore: UIStateStore
    /// Above the layout switch, for the app's whole run (KTD10).
    @State private var navigation: AppNavigation
    /// MetricKit's reports, kept on the device for Copy Debug Info (R1867).
    private let fieldDiagnostics: FieldDiagnosticsSubscriber?
    #if MEGURI_TEST_HOOKS
    private let hooks: TestHooks.Session
    #endif

    init() {
        #if MEGURI_TEST_HOOKS && DEBUG && os(macOS)
        // Before anything else: a test host or a hidden tools/mg launch
        // never shows in the Dock or comes to the front (U8d).
        TestHooks.BackgroundLaunch.apply()
        #endif
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
        var startsProduction = configuration.initialPresentation == nil && ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil
        #if MEGURI_TEST_HOOKS
        startsProduction = startsProduction && !hooks.setupOnly && hooks.shell == nil && hooks.error == nil && hooks.options.route == nil
        #endif
        var recordsFieldDiagnostics = startsProduction
        #if MEGURI_TEST_HOOKS
        recordsFieldDiagnostics = recordsFieldDiagnostics && hooks.ownedProvider == nil
        #endif
        if recordsFieldDiagnostics {
            let store = FieldDiagnosticsStore.standard
            configuration.recordDiagnostic = { store.enqueue($0) }
            configuration.fieldDiagnostics = FieldDiagnosticsCallbacks(
                snapshot: { await store.timeline() }, safeExport: { try await store.debugInfo() },
                writeClipboard: { text in
                    #if os(macOS)
                    NSPasteboard.general.clearContents()
                    return NSPasteboard.general.setString(text, forType: .string)
                    #elseif os(iOS)
                    UIPasteboard.general.string = text
                    return true
                    #else
                    return false
                    #endif
                })
            let subscriber = FieldDiagnosticsSubscriber(store: store)
            subscriber.start()
            fieldDiagnostics = subscriber
        } else {
            fieldDiagnostics = nil
        }
        let collection: ReviewCollection?
        #if MEGURI_TEST_HOOKS && os(macOS)
        if startsProduction, let ownedProvider = hooks.ownedProvider {
            let sync = SyncCoordinator(directory: configuration.collectionLocation.directory,
                clock: configuration.clock, defaults: nil, recordDiagnostic: configuration.recordDiagnostic)
            collection = ReviewCollection(configuration: configuration, navigation: navigation,
                sync: sync, providerDirectory: ownedProvider.installationDirectory)
        } else {
            collection = startsProduction ? ReviewCollection(configuration: configuration, navigation: navigation) : nil
        }
        #else
        collection = startsProduction ? ReviewCollection(configuration: configuration, navigation: navigation) : nil
        #endif
        #if MEGURI_TEST_HOOKS
        hooks.start(navigation, collection: collection)
        #endif
        if let collection {
            features = .standard(collection: collection)
            // Preserve navigation/window restoration while unavailable areas stay absent.
            navigation.show(.home)
            #if os(macOS)
            #if MEGURI_TEST_HOOKS
            if let ownedProvider = hooks.ownedProvider {
                AppDelegate.closeCollection = {
                    await ownedProvider.transport.cancelAndJoin()
                    let closed = await collection.closeForQuit()
                    return ownedProvider.cleanup(afterClosing: closed)
                }
            } else {
                AppDelegate.closeCollection = { await collection.closeForQuit() }
            }
            #else
            AppDelegate.closeCollection = { await collection.closeForQuit() }
            #endif
            #endif
        }
        self.collection = collection
        self.configuration = configuration
        self.features = features
        self.uiStateStore = uiStateStore
        _navigation = State(initialValue: navigation)
    }

    // Read-only composition seam for the app-hosted test. Inspecting this does
    // not create a store, subscribe, or touch either platform clipboard.
    var diagnosticComposition: (configuration: LaunchConfiguration, collection: ReviewCollection?, subscriber: FieldDiagnosticsSubscriber?) {
        (configuration, collection, fieldDiagnostics)
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
            StudyCommands()
            CloudSyncCommands(collection: collection)
        }
        SwiftUI.Settings {
            if let collection {
                #if MEGURI_TEST_HOOKS
                if let ownedProvider = hooks.ownedProvider {
                    SettingsWindow(collection: collection, features: features, configuration: configuration)
                        .defaultAppStorage(ownedProvider.defaults)
                        .modifier(TestHooks.OwnedProviderTransportGate(transport: ownedProvider.transport))
                } else {
                    SettingsWindow(collection: collection, features: features, configuration: configuration)
                }
                #else
                SettingsWindow(collection: collection, features: features, configuration: configuration)
                #endif
            }
        }
        #else
        WindowGroup {
            root
        }
        .commands {
            SidebarCommands()
            CloudSyncCommands(collection: collection)
        }
        #endif
    }

    @ViewBuilder private var root: some View {
        #if MEGURI_TEST_HOOKS
        TestHooks.Root(session: hooks, configuration: configuration, navigation: navigation) {
            if let ownedProvider = hooks.ownedProvider {
                RootView(configuration: configuration, navigation: navigation, features: features, uiStateStore: uiStateStore, collection: collection)
                    .defaultAppStorage(ownedProvider.defaults)
                    #if os(macOS)
                    .modifier(TestHooks.OwnedProviderTransportGate(transport: ownedProvider.transport))
                    #endif
            } else {
                RootView(configuration: configuration, navigation: navigation, features: features, uiStateStore: uiStateStore, collection: collection)
            }
        }
        #else
        RootView(configuration: configuration, navigation: navigation, features: features, uiStateStore: uiStateStore, collection: collection)
        #endif
    }
}
