#if MEGURI_TEST_HOOKS
import AppModel
import Settings
import SwiftUI

extension TestHooks {
    /// The round 2 Sync section (N15) in the states a capture needs. Each route
    /// draws the production views over an inert coordinator: no signed build,
    /// collection, account or server, and no action reaches one.
    enum SyncRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("settings/sync/off") { settings(.off, $0) }
            registry.register("settings/sync/meguri-connected") { settings(.meguriConnected, $0) }
            registry.register("settings/sync/meguri-sign-in") { settings(.meguriSignIn, $0) }
            registry.register("settings/sync/meguri-needs-you") { settings(.meguriNeedsYou, $0) }
            registry.register("settings/sync/icloud-selected") { settings(.iCloudSelected, $0) }
            registry.register("settings/sync/icloud-connected") { settings(.iCloudConnected, $0) }
            registry.register("settings/sync/icloud-needs-you") { settings(.iCloudNeedsYou, $0) }
            registry.register("settings/sync/icloud-issues") { settings(.iCloudIssues, $0) }
            registry.register("settings/sync/wide-meguri-connected") { context in
                .screen { SyncSettingsPreview.screen(.meguriConnected, page: .settingsInTabLayout, configuration: context.configuration) }
            }
            registry.register("settings/sync/wide-icloud-needs-you") { context in
                .screen { SyncSettingsPreview.screen(.iCloudNeedsYou, page: .settingsInTabLayout, configuration: context.configuration) }
            }
            registry.register("settings/sync/troubleshooting-icloud") { context in
                .screen { SyncSettingsPreview.screen(.iCloudIssues, page: .troubleshooting, configuration: context.configuration) }
            }
            registry.register("settings/sync/troubleshooting-meguri") { context in
                .screen { SyncSettingsPreview.screen(.meguriConnected, page: .troubleshooting, configuration: context.configuration) }
            }
            registry.register("settings/sync/issues-list") { context in
                .screen { SyncSettingsPreview.screen(.iCloudIssues, page: .issues, configuration: context.configuration) }
            }
            registry.register("settings/sync/keep-collection") { context in
                .screen { SyncSettingsPreview.screen(.meguriNeedsYou, page: .keepCollection, configuration: context.configuration) }
            }
            registry.register("settings/sync/status-needs-you") { context in
                .screen { SyncSettingsPreview.screen(.iCloudNeedsYou, page: .statusPopover, configuration: context.configuration) }
            }
        }
        private static func settings(_ state: SyncCoordinator.RenderState, _ context: RouteContext) -> InitialPresentation {
            .screen { SyncSettingsPreview.screen(state, configuration: context.configuration) }
        }
    }
}
#endif
