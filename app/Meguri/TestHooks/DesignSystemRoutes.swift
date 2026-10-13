#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import SwiftUI

extension TestHooks {
    /// The component gallery (U9, R1999): every DesignSystem component in
    /// every state, for agents, UI tests and U10's snapshots.
    enum DesignSystemRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("designsystem/gallery") { _ in
                .screen {
                    ComponentGallery()
                }
            }
        }
    }
}
#endif
