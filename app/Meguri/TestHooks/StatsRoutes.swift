#if MEGURI_TEST_HOOKS
import AppModel
import Stats
import SwiftUI

extension TestHooks {
    /// The Statistics page's routes. U295 adds the chart system's gallery on
    /// SAMPLE data; U296 adds the page's own routes (`stats`, `stats/deck/<id>`,
    /// `stats/chart/<box>`) and the `StatsRollout` flag here.
    ///
    /// - `stats/charts/<page>`: the gallery's page (`marks`: every mark kind;
    ///   `states`: loading, empty, failed, sparse and separated series;
    ///   `selection`: pinned selections with their callouts), All's monthly bins.
    /// - `…/<period>` (`1m`, `3m`, `1y`): the same with that period's bins.
    /// - `…/contrast`: Increase Contrast; `…/ax5`: the largest text size. The
    ///   appearance is the device's (`-MeguriAppearance` on the Mac).
    enum StatsRoutes {
        static let pages: [(StatsChartGallery.Page, [String])] = [
            (.marks, ["", "1m", "3m", "1y", "contrast", "ax5"]),
            (.states, ["", "1m", "contrast", "ax5"]),
            (.selection, ["", "1m", "contrast", "ax5"]),
        ]

        static func register(in registry: inout RouteRegistry) {
            for (page, options) in pages {
                for option in options {
                    let path = option.isEmpty ? "stats/charts/\(page.rawValue)" : "stats/charts/\(page.rawValue)/\(option)"
                    registry.register(path) { _ in
                        .screen { gallery(page, option: option) }
                    }
                }
            }
        }

        @MainActor @ViewBuilder
        static func gallery(_ page: StatsChartGallery.Page, option: String) -> some View {
            let period = StatsSamplePeriod(rawValue: option) ?? .all
            let gallery = NavigationStack {
                StatsChartGallery(page: page, period: period)
                    .environment(\._colorSchemeContrast, option == "contrast" ? .increased : .standard)
            }
            if option == "ax5" {
                gallery.dynamicTypeSize(.accessibility5)
            } else {
                gallery
            }
        }
    }
}
#endif
