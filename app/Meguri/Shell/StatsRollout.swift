/// Statistics stays out of the product until its page is built (`stats/statistics-page` S13;
/// plan U296, shown by U307). With the flag off nothing reaches it: no sidebar row, tab, View
/// command or route, and no statistics work starts. The test hooks turn it on
/// (`-MeguriStatsRollout`, the `stats/…` routes), before the shell is made.
enum StatsRollout {
    static var enabled = false
}
