import Foundation
import Instrumentation
import XCTest

/// The Statistics page on the 150k fixture (plan U296; `stats/statistics-page` S19, R2076 to
/// R2079, R1838's scope case): the app opens the generated 150k collection (`tools/mg fixtures
/// 150k`, which `tools/mg perf-ui` makes) with `StatsRollout` on, and the page is driven with no
/// synthesized input, through the hooks' commands (`showStats`, `showToday`, `statsPeriod…`,
/// `statsScroll`); the app answers `statsFilled` once every box shows its value.
///
/// Each test declares only the intervals it measures (`intervals`), so a result row never lacks
/// one. The Mac only in this version: the iPad's and the iPhone's runs come with U304 and U307.
final class StatsPerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] {
        ["stats.keptOpen", "stats.coldOpen", "stats.scopeChange", "stats.selection", "stats.scrollDuringRead"]
    }
    override nonisolated class var fixture: String? { "150k" }
    override nonisolated class var iterations: Int { 3 }

    /// The intervals this test measures, from its name.
    override var intervals: [IntervalName] {
        let names: [String] = switch invocation?.selector.description ?? name {
        case let test where test.contains("testKeptOpen"): ["stats.keptOpen"]
        case let test where test.contains("testColdOpen"): ["stats.coldOpen"]
        case let test where test.contains("testScopeChange"): ["stats.scopeChange"]
        case let test where test.contains("testSelection"): ["stats.selection"]
        case let test where test.contains("testScrollDuringRead"): ["stats.scrollDuringRead"]
        default: Self.intervalNames
        }
        return (try? Self.registered(names)) ?? []
    }

    /// R2076: the page with a kept result fills every box in its first frame.
    func testKeptOpen() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats()
        try measure(app: app, prepare: { _ in
            show(today: true, bundle: bundle)
        }, action: { _ in
            showAndWaitFilled(bundle: bundle)
        })
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    /// R2077: nothing kept and the caches cold: each run launches the app again on a fresh copy of
    /// the fixture and opens the page.
    func testColdOpen() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats(open: false)
        try measure(app: app, prepare: { app in
            app.terminate()
            app.launch()
            XCTAssert(app.descendants(matching: .any)["shell.root.sidebar.stats"].waitForExistence(timeout: 180),
                      "the shell did not show its Statistics row")
        }, action: { _ in
            showAndWaitFilled(bundle: bundle, timeout: 300)
        })
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    /// R1838: a change to or from All is a new refresh, measured to the last review-log box.
    func testScopeChange() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats()
        showAndWaitFilled(bundle: bundle, timeout: 300)
        var all = true
        try measure(app: app, prepare: { _ in
            all.toggle()
        }, action: { _ in
            waitFilled(after: all ? "statsPeriodAll" : "statsPeriodYear", bundle: bundle)
        })
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    /// R2079: a period within the year re-bins in one frame, with no core call.
    func testSelection() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats()
        showAndWaitFilled(bundle: bundle, timeout: 300)
        waitFilled(after: "statsPeriodYear", bundle: bundle, timeout: 300)
        var half = false
        try measure(app: app, prepare: { _ in
            half.toggle()
        }, action: { _ in
            post("\(bundle).testhooks.\(half ? "statsPeriodHalfYear" : "statsPeriodYear")")
            // One frame and the next: the re-bin is drawn well within it.
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
        })
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    /// R2078: scrolling while a refresh reads keeps the hitch time ratio under 5 ms/s.
    func testScrollDuringRead() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats()
        showAndWaitFilled(bundle: bundle, timeout: 300)
        let done = "\(bundle).testhooks.statsScrolled"
        try measure(app: app) { _ in
            let finished = expectation(forNotification: Notification.Name(done), object: nil,
                                       notificationCenter: DistributedNotificationCenter.default())
            post("\(bundle).testhooks.statsScroll")
            wait(for: [finished], timeout: 60)
        }
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    // MARK: Support

    #if os(macOS)
    /// The 150k collection with Statistics behind its flag, on Today.
    private func launchStats(open: Bool = false) throws -> (XCUIApplication, String) {
        let build = try BuildUnderTest.current()
        let fixture = try BrowserSearchPerfTests.fixture150k(build.repositoryRoot)
        let app = launch(MeguriLaunch(fixture: fixture, animations: false, windowSize: "large",
                                      extra: ["-MeguriStatsRollout", open ? "open" : "on"]))
        XCTAssert(app.descendants(matching: .any)["shell.root.sidebar.stats"].waitForExistence(timeout: 180),
                  "the shell did not show its Statistics row")
        return (app, build.expectedBundleIdentifier)
    }

    private func show(today: Bool, bundle: String) {
        post("\(bundle).testhooks.\(today ? "showToday" : "showStats")")
        RunLoop.current.run(until: Date().addingTimeInterval(0.5))
    }

    private func showAndWaitFilled(bundle: String, timeout: TimeInterval = 120) {
        waitFilled(after: "showStats", bundle: bundle, timeout: timeout)
    }

    /// Posts `command` and waits until the app reports every box filled.
    private func waitFilled(after command: String, bundle: String, timeout: TimeInterval = 120) {
        let filled = expectation(forNotification: Notification.Name("\(bundle).testhooks.statsFilled"), object: nil,
                                 notificationCenter: DistributedNotificationCenter.default())
        post("\(bundle).testhooks.\(command)")
        wait(for: [filled], timeout: timeout)
    }
    #endif
}
