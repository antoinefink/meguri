#if MEGURI_TEST_HOOKS
import AppModel
import Stats
import SwiftUI

extension TestHooks {
    /// The Statistics page's routes. U295 adds the chart system's gallery on
    /// SAMPLE data; U296 adds the page's own routes (`stats`, `stats/deck/<id>`,
    /// `stats/chart/<box>`) and the `StatsRollout` flag here.
    ///
    /// - `stats/charts/<page>`: the gallery's page (`bars`, `lines`, `areas`:
    ///   every mark kind; `states`: loading, empty and series drawn apart; `edges`: a
    ///   failed part, a sparse chart and empty lines; `selection`: pinned
    ///   selections with their callouts), All's monthly bins.
    /// - `…/<period>` (`1m`, `3m`, `1y`): the same with that period's bins.
    /// - `…/contrast`: Increase Contrast; `…/ax5`: the largest text size. The
    ///   appearance is the device's (`-MeguriAppearance` on the Mac).
    enum StatsRoutes {
        // Literal paths: `tools/mg xcode-check` reads the routes from their registrations.
        static func register(in registry: inout RouteRegistry) {
            registry.register("stats/charts/bars") { _ in .screen { gallery(.bars, option: "") } }
            registry.register("stats/charts/bars/1m") { _ in .screen { gallery(.bars, option: "1m") } }
            registry.register("stats/charts/bars/3m") { _ in .screen { gallery(.bars, option: "3m") } }
            registry.register("stats/charts/bars/1y") { _ in .screen { gallery(.bars, option: "1y") } }
            registry.register("stats/charts/bars/contrast") { _ in .screen { gallery(.bars, option: "contrast") } }
            registry.register("stats/charts/bars/ax5") { _ in .screen { gallery(.bars, option: "ax5") } }
            registry.register("stats/charts/lines") { _ in .screen { gallery(.lines, option: "") } }
            registry.register("stats/charts/lines/1y") { _ in .screen { gallery(.lines, option: "1y") } }
            registry.register("stats/charts/lines/contrast") { _ in .screen { gallery(.lines, option: "contrast") } }
            registry.register("stats/charts/lines/ax5") { _ in .screen { gallery(.lines, option: "ax5") } }
            registry.register("stats/charts/areas") { _ in .screen { gallery(.areas, option: "") } }
            registry.register("stats/charts/areas/1m") { _ in .screen { gallery(.areas, option: "1m") } }
            registry.register("stats/charts/areas/contrast") { _ in .screen { gallery(.areas, option: "contrast") } }
            registry.register("stats/charts/areas/ax5") { _ in .screen { gallery(.areas, option: "ax5") } }
            registry.register("stats/charts/states") { _ in .screen { gallery(.states, option: "") } }
            registry.register("stats/charts/states/contrast") { _ in .screen { gallery(.states, option: "contrast") } }
            registry.register("stats/charts/states/ax5") { _ in .screen { gallery(.states, option: "ax5") } }
            registry.register("stats/charts/edges") { _ in .screen { gallery(.edges, option: "") } }
            registry.register("stats/charts/edges/contrast") { _ in .screen { gallery(.edges, option: "contrast") } }
            registry.register("stats/charts/selection") { _ in .screen { gallery(.selection, option: "") } }
            registry.register("stats/charts/selection/1m") { _ in .screen { gallery(.selection, option: "1m") } }
            registry.register("stats/charts/selection/contrast") { _ in .screen { gallery(.selection, option: "contrast") } }
            registry.register("stats/charts/selection/ax5") { _ in .screen { gallery(.selection, option: "ax5") } }
        }

        @MainActor @ViewBuilder
        static func gallery(_ page: StatsChartGallery.Page, option: String) -> some View {
            let period = StatsSamplePeriod(rawValue: option) ?? .all
            let gallery = NavigationStack { StatsChartGallery(page: page, period: period) }
            switch option {
            case "contrast": gallery.environment(\._colorSchemeContrast, .increased)
            case "ax5": gallery.dynamicTypeSize(.accessibility5)
            default: gallery
            }
        }
    }
}
#endif
