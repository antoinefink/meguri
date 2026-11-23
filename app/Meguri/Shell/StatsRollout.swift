import AppModel
#if os(iOS)
import UIKit
#endif

/// Whether the Statistics page is reachable (`stats/statistics-page` S13). On by default where
/// its page has a layout, the Mac and the iPad (`AppSection.statisticsShown`; decided by the
/// coordinator under Antoine's delegation, 2026-11-23, part of plan U307): the sidebar row, View ›
/// Statistics (⇧⌘S) and View › Go to Section. Off on the iPhone and the Duo until U304 builds
/// their layout: no tab, no row, no command, and no statistics work starts. The test hooks set
/// it either way before the shell is made (`-MeguriStatsRollout on|open|off`, the `stats/…`
/// routes).
enum StatsRollout {
    static var enabled = AppSection.statisticsShown(on: .current)
}

extension DeviceFamily {
    /// This device's family.
    static var current: DeviceFamily {
        #if os(macOS)
        .mac
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? .pad : .phone
        #endif
    }
}
