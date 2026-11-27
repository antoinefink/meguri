#if MEGURI_TEST_HOOKS
#if os(macOS)
import AppKit
#endif
@_spi(MeguriTestHooks) import AppModel
import Settings
import SwiftUI

extension TestHooks {
    /// The round 2 Sync section (N15) in the states a capture needs. Each route
    /// draws the production views over an inert coordinator: no signed build,
    /// collection, account or server, and no action reaches one.
    enum SyncRoutes {
        /// `-MeguriSyncRender`: once the collection is open, the shell shows the
        /// synthetic status (the real coordinator keeps running unseen).
        @MainActor static func showRender(_ render: LaunchOptions.SyncRender, over collection: ReviewCollection,
                                          clock: AppClock) {
            Task { @MainActor in
                while collection.state != .ready { try? await Task.sleep(for: .milliseconds(100)) }
                collection.showSyncRenderFixtureForTestHooks(.renderFixture(render.state, clock: clock))
                if render.justFinished { collection.syncPresentation.buttonFeedback.holdAccentForTestHooks() }
            }
        }

        /// `-MeguriSyncRender` with a shell route: the shell's sync button over the
        /// synthetic status (its own inert collection; nothing syncs).
        @MainActor static func addRender(_ render: LaunchOptions.SyncRender, to features: inout ShellFeatures,
                                         configuration: LaunchConfiguration) {
            let collection = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
                sync: .renderFixture(render.state, clock: configuration.clock))
            if render.justFinished { collection.syncPresentation.buttonFeedback.holdAccentForTestHooks() }
            features.syncStatus = { AnyView(SyncStatusButton(collection: collection)) }
            #if os(iOS)
            features.syncPanelMeasure = { AnyView(SyncPanelMeasure(collection: collection)) }
            #endif
        }

        static func register(in registry: inout RouteRegistry) {
            #if os(macOS)
            registry.register("settings/integrations/listening") { _ in
                .screen { NavigationStack { SentenceMiningPane(model: .renderFixture()) } }
            }
            registry.register("settings/integrations/port-conflict") { _ in
                .screen { NavigationStack { SentenceMiningPane(model: .renderFixture(portConflict: true)) } }
            }
            #endif
            registry.register("settings/sync/off") { settings(.off, $0) }
            registry.register("settings/sync/meguri-syncing") { settings(.meguriSyncing, $0) }
            registry.register("settings/sync/meguri-applying") { settings(.meguriApplying, $0) }
            registry.register("settings/sync/meguri-checking-media") { settings(.meguriCheckingMedia, $0) }
            registry.register("settings/sync/meguri-syncing-media") { settings(.meguriSyncingMedia, $0) }
            registry.register("settings/sync/meguri-syncing-media-unknown") { settings(.meguriSyncingMediaUnknown, $0) }
            registry.register("settings/sync/meguri-connected") { settings(.meguriConnected, $0) }
            registry.register("settings/sync/meguri-sign-in") { settings(.meguriSignIn, $0) }
            registry.register("settings/sync/meguri-never-signed-in") { settings(.meguriNeverSignedIn, $0) }
            // MP6.5 (resume fix): setup's media step running, paused, and stopped after its automatic attempts.
            registry.register("settings/sync/meguri-setup-media") { settings(.meguriSetupMedia, $0) }
            registry.register("settings/sync/meguri-setup-media-paused") { settings(.meguriSetupMediaPaused, $0) }
            registry.register("settings/sync/meguri-setup-media-stopped") { settings(.meguriSetupMediaStopped, $0) }
            registry.register("settings/sync/meguri-needs-you") { settings(.meguriNeedsYou, $0) }
            // MP7 step 5: a note type structure save waits for Upload….
            registry.register("settings/sync/meguri-structure-upload") { settings(.meguriStructureUploadPending, $0) }
            registry.register("settings/sync/wide-meguri-connected") { context in
                .screen { SyncSettingsPreview.screen(.meguriConnected, page: .settingsInTabLayout, configuration: context.configuration) }
            }
            registry.register("settings/sync/troubleshooting-meguri") { context in
                .screen { SyncSettingsPreview.screen(.meguriConnected, page: .troubleshooting, configuration: context.configuration) }
            }
            registry.register("settings/sync/keep-collection") { context in
                .screen { SyncSettingsPreview.screen(.meguriNeedsYou, page: .keepCollection, configuration: context.configuration) }
            }
            registry.register("settings/sync/status-needs-you") { context in
                .screen { SyncSettingsPreview.screen(.meguriNeedsYou, page: .statusPopover, configuration: context.configuration) }
            }
            // N23 (round 10): the status panel in each state, drawn in the window.
            registry.register("settings/sync/panel/off") { panel(.off, $0) }
            registry.register("settings/sync/panel/meguri-up-to-date") { panel(.meguriConnected, $0) }
            registry.register("settings/sync/panel/meguri-applying") { panel(.meguriApplying, $0) }
            registry.register("settings/sync/panel/meguri-checking-media") { panel(.meguriCheckingMedia, $0) }
            registry.register("settings/sync/panel/meguri-syncing-media") { panel(.meguriSyncingMedia, $0) }
            registry.register("settings/sync/panel/meguri-syncing-media-unknown") { panel(.meguriSyncingMediaUnknown, $0) }
            registry.register("settings/sync/panel/meguri-syncing") { panel(.meguriSyncing, $0) }
            registry.register("settings/sync/panel/meguri-setup-media") { panel(.meguriSetupMedia, $0) }
            registry.register("settings/sync/panel/meguri-setup-media-paused") { panel(.meguriSetupMediaPaused, $0) }
            registry.register("settings/sync/panel/meguri-setup-media-stopped") { panel(.meguriSetupMediaStopped, $0) }
            registry.register("settings/sync/panel/meguri-preparing") { panel(.meguriPreparing, $0) }
            registry.register("settings/sync/panel/meguri-media-blocked") { panel(.meguriMediaBlocked, $0) }
            registry.register("settings/sync/panel/meguri-acceptance-unknown") { panel(.meguriAcceptanceUnknown, $0) }
            registry.register("settings/sync/panel/meguri-round-failed") { panel(.meguriRoundFailed, $0) }
            registry.register("settings/sync/panel/meguri-sign-in-refused") { panel(.meguriSignInRefused, $0) }
            registry.register("settings/sync/panel/meguri-choose-collection") { panel(.meguriNeedsYou, $0) }
            registry.register("settings/sync/panel/meguri-structure-upload") { panel(.meguriStructureUploadPending, $0) }
            registry.register("settings/sync/panel/meguri-blocked") { panel(.meguriBlocked, $0) }
            registry.register("settings/sync/panel/meguri-never-signed-in") { panel(.meguriNeverSignedIn, $0) }
            // The popover itself, opened from the toolbar (iPhone: the sheet; iPad: the popover).
            registry.register("settings/sync/status/meguri-setup-media") { context in
                .screen { SyncSettingsPreview.screen(.meguriSetupMedia, page: .statusPopover, configuration: context.configuration) }
            }
            registry.register("settings/sync/status/off") { context in
                .screen { SyncSettingsPreview.screen(.off, page: .statusPopover, configuration: context.configuration) }
            }
            registry.register("settings/sync/status/meguri-up-to-date") { context in
                .screen { SyncSettingsPreview.screen(.meguriConnected, page: .statusPopover, configuration: context.configuration) }
            }
            // Mac: the real Settings window (its scene, toolbar and split view) over a
            // synthetic coordinator, for the pane and its pushed Troubleshooting page.
            // Touch has no Settings window: the same state on the Settings page.
            // Literal paths: xcode-check reads routes from the source (app/AGENTS.md).
            registry.register("settings/sync/window/meguri-setup-media") { window(.meguriSetupMedia, $0) }
            registry.register("settings/sync/window/meguri-setup-media-paused") { window(.meguriSetupMediaPaused, $0) }
            registry.register("settings/sync/window/meguri-connected") { window(.meguriConnected, $0) }
            registry.register("settings/sync/window/meguri-never-signed-in") { window(.meguriNeverSignedIn, $0) }
            registry.register("settings/sync/window/meguri-sign-in") { window(.meguriSignIn, $0) }
            // T21.1: the service change confirmation, opened by the Service control
            // over the Settings window (Mac) or the Settings page (touch).
            registry.register("settings/sync/window/switch-meguri-off-unknown") { window(.meguriOffUnknown, $0) }
            registry.register("settings/sync/window/switch-meguri-off-known") { window(.meguriOffKnown, $0) }
        }
        #if os(macOS)
        /// The Settings scene shows this collection in place of the app's while a
        /// `settings/sync/window/…` route runs (`MeguriApp`'s Settings scene).
        @MainActor @Observable final class SettingsWindowPreview {
            static let shared = SettingsWindowPreview()
            var collection: ReviewCollection?
            struct Opener: View {
                let state: SyncCoordinator.RenderState
                var serviceSwitch: SyncCoordinator.RenderServiceSwitch? = nil
                let configuration: LaunchConfiguration
                @Environment(\.openSettings) private var openSettings
                /// The Settings window's resizing facts, for captures (N15.7).
                @State private var windowFacts = ""
                var body: some View {
                    Text(verbatim: "Settings window preview")
                        .accessibilityIdentifier("settings.sync.windowPreview")
                        .accessibilityValue(windowFacts)
                        .task {
                            // A hidden launch (xcode-check's probe, -MeguriBackgroundLaunch)
                            // stays accessory and must never come to the front: it
                            // checks the route launches and stays up, without the window.
                            guard NSApp.activationPolicy() == .regular else { return }
                            let preview = ReviewCollection(configuration: configuration,
                                navigation: AppNavigation(), sync: .renderFixture(state, clock: configuration.clock,
                                    serviceSwitch: serviceSwitch))
                            // Whatever section was last chosen, the window opens at Sync.
                            preview.syncPresentation.settingsRequest = UUID()
                            SettingsWindowPreview.shared.collection = preview
                            openSettings()
                            try? await Task.sleep(for: .seconds(1))
                            windowFacts = NSApp.windows.filter { $0.isVisible }.map { window in
                                let id = window.identifier?.rawValue ?? "none"
                                return "\(id): resizable=\(window.styleMask.contains(.resizable)) "
                                    + "min=\(Int(window.minSize.width))x\(Int(window.minSize.height)) "
                                    + "max=\(Int(min(window.maxSize.width, 99_999)))x\(Int(min(window.maxSize.height, 99_999))) "
                                    + "size=\(Int(window.frame.width))x\(Int(window.frame.height)) "
                                    + "autosave=\(window.frameAutosaveName.isEmpty ? "none" : window.frameAutosaveName)"
                            }.joined(separator: "; ")
                        }
                }
            }
        }
        #endif
        private static func window(_ state: SyncCoordinator.RenderState, _ context: RouteContext) -> InitialPresentation {
            #if os(macOS)
            .screen { SettingsWindowPreview.Opener(state: state, configuration: context.configuration) }
            #else
            settings(state, context)
            #endif
        }
        private static func window(_ example: SyncSettingsPreview.ServiceSwitchExample,
                                   _ context: RouteContext) -> InitialPresentation {
            #if os(macOS)
            .screen { SettingsWindowPreview.Opener(state: example.state, serviceSwitch: example.request,
                                                   configuration: context.configuration) }
            #else
            .screen { SyncSettingsPreview.screen(example.state, serviceSwitch: example.request,
                                                 configuration: context.configuration) }
            #endif
        }
        private static func panel(_ state: SyncCoordinator.RenderState, _ context: RouteContext) -> InitialPresentation {
            .screen { SyncSettingsPreview.screen(state, page: .statusPanel, configuration: context.configuration) }
        }
        private static func settings(_ state: SyncCoordinator.RenderState, _ context: RouteContext) -> InitialPresentation {
            .screen { SyncSettingsPreview.screen(state, configuration: context.configuration) }
        }
    }
}
#endif
