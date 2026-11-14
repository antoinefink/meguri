#if MEGURI_TEST_HOOKS
import AppModel
import Review
import SwiftUI

extension TestHooks {
    /// Fixed production terminal content; these routes own no collection/session.
    enum ReviewRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("review/congratulations/waiting-unavailable") { _ in
                .screen { ReviewFinishedPreview.preview(.waitingUnavailable) }
            }
            registry.register("review/congratulations/ready-to-continue") { _ in
                .screen { ReviewFinishedPreview.preview(.readyToContinue) }
            }
            registry.register("review/congratulations/finished") { _ in
                .screen { ReviewFinishedPreview.preview(ReviewFinishedPreview.State.finished) }
            }
            registry.register("review/congratulations/finished-with-new-work") { _ in
                .screen { ReviewFinishedPreview.preview(.finishedWithNewWork) }
            }
        }
    }
}
#endif
