#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Foundation
import Home
import Settings
import SwiftUI

extension TestHooks {
    enum CollectionEntryRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("settings/account-sync/setup-needed") { context in
                .screen { AccountSyncInspection(configuration: context.configuration) }
            }
            registry.register("settings/anki-transfer/empty") { _ in .screen { ManualTransferSettingsPreview.preview(.empty) } }
            registry.register("settings/anki-transfer/typical") { _ in .screen { ManualTransferSettingsPreview.preview(.typical) } }
            registry.register("settings/anki-transfer/error") { _ in .screen { ManualTransferSettingsPreview.preview(.error) } }
            registry.register("settings/anki-transfer/loading") { _ in .screen { ManualTransferSettingsPreview.preview(.loading) } }
            registry.register("settings/anki-transfer/overflow") { _ in .screen { ManualTransferSettingsPreview.preview(.overflow) } }
            registry.register("settings/anki-transfer/media-consent") { _ in .screen { ManualTransferSettingsPreview.mediaFixture(.mediaConsent) } }
            registry.register("settings/anki-transfer/media-progress") { _ in .screen { ManualTransferSettingsPreview.mediaFixture(.mediaProgress) } }
            registry.register("settings/anki-transfer/media-partial") { _ in .screen { ManualTransferSettingsPreview.mediaFixture(.mediaPartial) } }
            registry.register("settings/anki-transfer/media-complete") { _ in .screen { ManualTransferSettingsPreview.mediaFixture(.mediaComplete) } }
            registry.register("settings/anki-transfer/media-unknown") { _ in .screen { ManualTransferSettingsPreview.mediaFixture(.mediaUnknown) } }
            registry.register("settings/backups/empty") { _ in .screen { BackupsPreview.settings(.empty) } }
            registry.register("settings/backups/typical") { _ in .screen { BackupsPreview.settings(.typical) } }
            registry.register("settings/backups/overflow") { _ in .screen { BackupsPreview.settings(.overflow) } }
            registry.register("settings/backups/error") { _ in .screen { BackupsPreview.settings(.error) } }
            registry.register("settings/backups/loading") { _ in .screen { BackupsPreview.settings(.loading) } }
            registry.register("settings/backups/restore/empty") { _ in .sheet { BackupsPreview.restore(.empty) } }
            registry.register("settings/backups/restore/typical") { _ in .sheet { BackupsPreview.restore(.typical) } }
            registry.register("settings/backups/restore/overflow") { _ in .sheet { BackupsPreview.restore(.overflow) } }
            registry.register("settings/backups/restore/error") { _ in .sheet { BackupsPreview.restore(.error) } }
            registry.register("settings/backups/restore/loading") { _ in .sheet { BackupsPreview.restore(.loading) } }
            registry.register("settings/import-export/typical") { _ in .sheet { CollectionExportPreview.preview(.typical) } }
            registry.register("settings/import-export/error") { _ in .sheet { CollectionExportPreview.preview(.error) } }
            registry.register("settings/import-export/loading") { _ in .sheet { CollectionExportPreview.preview(.loading) } }
            registry.register("collection/import/empty") { _ in .sheet { CollectionEntryPreview.preview(.empty) } }
            registry.register("collection/import/typical") { _ in .sheet { CollectionEntryPreview.preview(.typical) } }
            registry.register("collection/import/overflow") { _ in .sheet { CollectionEntryPreview.preview(.overflow) } }
            registry.register("collection/import/error") { _ in .sheet { CollectionEntryPreview.preview(.error) } }
            registry.register("collection/import/loading") { _ in .sheet { CollectionEntryPreview.preview(.loading) } }
            // N14.10: the list an unfolded iPhone shows beside the detail, which no simulator here can be opened to.
            registry.register("home/deck-list/beside-deck") { _ in .screen { HomeDeckListPreview.besideDetail(deck: 2) } }
            registry.register("home/deck-list/beside-today") { _ in .screen { HomeDeckListPreview.besideDetail(deck: nil) } }
            registry.register("home/deck-list/empty") { _ in .screen { HomeDeckListPreview.preview(.empty) } }
            // N18.3: Start Empty disabled while sync work runs, with its reason line.
            registry.register("home/deck-list/start-empty-waits") { _ in .screen { HomeDeckListPreview.welcomeWaitingForSync() } }
            // N14.11, N24.6: the tab layout's list with its heatmap after the rows.
            registry.register("home/deck-list/typical") { _ in .screen { HomeDeckListPreview.typicalWithActivity() } }
            // N24.4: the production sidebar's rows over sample decks, Today's summary beside them.
            registry.register("home/sidebar/typical") { _ in .screen { SidebarPreview() } }
            // N24.2: Today's summary beside the deck list (Mac, iPad, the Duo held wide), over sample values.
            registry.register("home/today/summary") { _ in .screen { HomeTodayPreview.preview(.summary) } }
            registry.register("home/today/nothing-waiting") { _ in .screen { HomeTodayPreview.preview(.nothingWaiting) } }
            registry.register("home/today/after-walk") { _ in .screen { HomeTodayPreview.preview(.afterWalk) } }
        }
    }

    /// The shell's own sidebar list (`SidebarView`) with Home's deck rows over sample decks
    /// (`HomeSidebarPreview`) and Today's summary beside it: the badges, help tags and the
    /// quieter nothing-waiting names, with no collection.
    struct SidebarPreview: View {
        @State private var navigation = AppNavigation()

        var body: some View {
            NavigationSplitView {
                SidebarView(navigation: navigation, features: features)
            } detail: {
                HomeTodayPreview.preview(.summary)
            }
        }

        private var features: ShellFeatures {
            var features = ShellFeatures.standard
            features.todayRoot = true
            #if os(macOS)
            features.nativeMacWorkspace = true
            features.availableSections = [.home]
            #else
            features.availableSections = [.home, .settings]
            #endif
            features.sidebarDecks = { navigation in AnyView(HomeSidebarPreview.decks(selection: navigation.sidebarSelection)) }
            return features
        }
    }

    /// Production form and Settings composition, using a real generated local
    /// collection. Explicitly absent CloudKit setup admits no native account read.
    struct AccountSyncInspection: View {
        private let configuration: LaunchConfiguration
        private let preferences: UserDefaults
        @State private var collection: ReviewCollection

        init(configuration: LaunchConfiguration) {
            self.configuration = configuration
            let preferences = UserDefaults(suiteName: "meguri.testhooks.accountSyncInspection") ?? .standard
            preferences.set("accountSync", forKey: "settings.selectedSection")
            self.preferences = preferences
            let sync = SyncCoordinator(directory: configuration.collectionLocation.directory,
                clock: configuration.clock, configuration: nil, defaults: nil)
            _collection = State(initialValue: ReviewCollection(configuration: configuration,
                navigation: AppNavigation(), sync: sync))
        }
        var body: some View {
            Group {
                #if os(macOS)
                SettingsWindow(collection: collection, features: .standard(collection: collection), configuration: configuration)
                #else
                NavigationStack { AccountSyncView(collection: collection) }
                #endif
            }
            .defaultAppStorage(preferences)
            .task {
                #if os(macOS)
                AppDelegate.closeCollection = { await collection.closeForQuit() }
                #endif
                await collection.start()
            }
        }
    }
}
#endif
