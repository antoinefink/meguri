#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Home
import SwiftUI

extension TestHooks {
    enum CollectionEntryRoutes {
        static func register(in registry: inout RouteRegistry) {
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
