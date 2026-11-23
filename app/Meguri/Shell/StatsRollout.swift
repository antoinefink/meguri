import AppModel
#if os(iOS)
import UIKit
#endif

/// Whether the Statistics page is reachable (`stats/statistics-page` S13). On by default where
/// its page has a layout (`AppSection.statisticsShown`; decided by the coordinator under
/// Antoine's delegation, 2026-11-23, part of plan U307): the Mac and the iPad (the sidebar row,
/// View › Statistics ⇧⌘S, View › Go to Section) and, since U304's narrow page, the iPhone and the
/// Duo (the Statistics tab). Off, nothing reaches it and no statistics work starts. The test
/// hooks set it either way before the shell is made (`-MeguriStatsRollout on|open|off`, the
/// `stats/…` routes).
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
