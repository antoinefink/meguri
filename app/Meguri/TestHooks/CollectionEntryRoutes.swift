#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Home
import Settings
import SwiftUI

extension TestHooks {
    enum CollectionEntryRoutes {
        static func register(in registry: inout RouteRegistry) {
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
            registry.register("settings/import-export/typical") { _ in .sheet { CollectionExportPreview.preview(.typical) } }
            registry.register("settings/import-export/error") { _ in .sheet { CollectionExportPreview.preview(.error) } }
            registry.register("settings/import-export/loading") { _ in .sheet { CollectionExportPreview.preview(.loading) } }
            registry.register("collection/import/empty") { _ in .sheet { CollectionEntryPreview.preview(.empty) } }
            registry.register("collection/import/typical") { _ in .sheet { CollectionEntryPreview.preview(.typical) } }
            registry.register("collection/import/overflow") { _ in .sheet { CollectionEntryPreview.preview(.overflow) } }
            registry.register("collection/import/error") { _ in .sheet { CollectionEntryPreview.preview(.error) } }
            registry.register("collection/import/loading") { _ in .sheet { CollectionEntryPreview.preview(.loading) } }
            registry.register("home/deck-list/empty") { _ in .screen { HomeDeckListPreview.preview(.empty) } }
            registry.register("home/deck-list/typical") { _ in .screen { HomeDeckListPreview.preview(.typical) } }
        }
    }
}
#endif
