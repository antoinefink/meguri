import Foundation
import Instrumentation
import XCTest
#if os(iOS)
import notify
#endif

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
        ["stats.keptOpen", "stats.coldOpen", "stats.scopeChange", "stats.selection", "stats.scrollDuringRead",
         "stats.scrollSettled"]
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
        case let test where test.contains("testScrollSettled"): ["stats.scrollSettled"]
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
        let (app, bundle) = try launchStats(animations: true)
        showAndWaitFilled(bundle: bundle, timeout: 300)
        var pacing: [StatsFramePacingReading] = []
        try measure(app: app) { _ in pacing.append(scroll("statsScroll", bundle: bundle)) }
        StatsFramePacingReading.check(pacing)
        #else
        throw XCTSkip("The Mac's page in this version (U304 and U307 run the touch layouts)")
        #endif
    }

    /// R2078's settled case (the owner's report of 2026-11-23: scrolling "isn't very smooth" once
    /// the page is drawn): the filled page scrolled with nothing reading, the pointer resting over
    /// it, so the boxes pass under it as they do under a trackpad scroll.
    func testScrollSettled() throws {
        #if os(macOS)
        let (app, bundle) = try launchStats(animations: true)
        showAndWaitFilled(bundle: bundle, timeout: 300)
        let content = app.descendants(matching: .any)["stats.page.content"]
        XCTAssert(content.waitForExistence(timeout: 30), "the page's scroll view is not on screen")
        content.hover()
        var pacing: [StatsFramePacingReading] = []
        try measure(app: app) { _ in pacing.append(scroll("statsScrollSettled", bundle: bundle)) }
        StatsFramePacingReading.check(pacing)
        #else
        throw XCTSkip("The Mac's page on the 150k fixture; StatsTouchPerfTests scrolls the iPad's")
        #endif
    }

    // MARK: Support

    #if os(macOS)
    /// The 150k collection with Statistics behind its flag, on Today.
    /// The scroll tests launch with animations, so the page glides from one heading to the next.
    private func launchStats(open: Bool = false, animations: Bool = false) throws -> (XCUIApplication, String) {
        let build = try BuildUnderTest.current()
        let fixture = try BrowserSearchPerfTests.fixture150k(build.repositoryRoot)
        let app = launch(MeguriLaunch(fixture: fixture, animations: animations ? nil : false, windowSize: "large",
                                      extra: ["-MeguriStatsRollout", open ? "open" : "on"]))
        XCTAssert(app.descendants(matching: .any)["shell.root.sidebar.stats"].waitForExistence(timeout: 180),
                  "the shell did not show its Statistics row")
        return (app, build.expectedBundleIdentifier)
    }

    /// Posts a scroll command and returns the frame pacing the app reports with `statsScrolled`.
    private func scroll(_ command: String, bundle: String) -> StatsFramePacingReading {
        let reading = StatsFramePacingText()
        let finished = expectation(forNotification: Notification.Name("\(bundle).testhooks.statsScrolled"), object: nil,
                                   notificationCenter: DistributedNotificationCenter.default()) { notification in
            reading.text = notification.object as? String ?? ""
            return true
        }
        post("\(bundle).testhooks.\(command)")
        wait(for: [finished], timeout: 60)
        return StatsFramePacingReading(text: reading.text)
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

/// The iPad's page scrolled (R2078 on touch, the owner's report of 2026-11-23): the sample
/// collection of the `stats/page/typical` route (`StatsSampleService`, about three years), since
/// the 150k fixture reaches the test VM only for the Mac. Each run is the hooks' section-by-section
/// scroll; the settled one also swipes the page by touch, as a finger does.
final class StatsTouchPerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] { ["stats.scrollDuringRead", "stats.scrollSettled"] }
    override nonisolated class var iterations: Int { 3 }

    override var intervals: [IntervalName] {
        let names: [String] = switch invocation?.selector.description ?? name {
        case let test where test.contains("testScrollDuringRead"): ["stats.scrollDuringRead"]
        case let test where test.contains("testScrollSettled"): ["stats.scrollSettled"]
        default: Self.intervalNames
        }
        return (try? Self.registered(names)) ?? []
    }

    func testScrollSettled() throws {
        #if os(iOS)
        let (app, bundle) = try launchSample()
        var pacing: [StatsFramePacingReading] = []
        try measure(app: app) { app in
            pacing.append(scroll("statsScrollSettled", bundle: bundle))
            let page = app.descendants(matching: .any)["stats.page.content"]
            for _ in 0..<3 { page.swipeUp(velocity: .fast) }
            for _ in 0..<3 { page.swipeDown(velocity: .fast) }
        }
        StatsFramePacingReading.check(pacing)
        #else
        throw XCTSkip("Touch: StatsPerfTests measures the Mac")
        #endif
    }

    func testScrollDuringRead() throws {
        #if os(iOS)
        let (app, bundle) = try launchSample()
        var pacing: [StatsFramePacingReading] = []
        try measure(app: app) { _ in pacing.append(scroll("statsScroll", bundle: bundle)) }
        StatsFramePacingReading.check(pacing)
        #else
        throw XCTSkip("Touch: StatsPerfTests measures the Mac")
        #endif
    }

    #if os(iOS)
    private func launchSample() throws -> (XCUIApplication, String) {
        let bundle = try BuildUnderTest.current().expectedBundleIdentifier
        let filled = XCTDarwinNotificationExpectation(notificationName: "\(bundle).testhooks.statsFilled")
        let app = launch(MeguriLaunch(route: "stats/page/typical"))
        wait(for: [filled], timeout: 120)
        XCTAssert(app.descendants(matching: .any)["stats.page.content"].waitForExistence(timeout: 30),
                  "the page's scroll view is not on screen")
        return (app, bundle)
    }

    /// Posts a scroll command, waits for the page to report it done and reads the hitch time
    /// ratio it left as the notification's state (thousandths of a ms/s).
    private func scroll(_ command: String, bundle: String) -> StatsFramePacingReading {
        let name = "\(bundle).testhooks.statsScrolled"
        let finished = XCTDarwinNotificationExpectation(notificationName: name)
        post("\(bundle).testhooks.\(command)")
        wait(for: [finished], timeout: 60)
        var token: Int32 = 0
        var state: UInt64 = 0
        if notify_register_check(name, &token) == NOTIFY_STATUS_OK {
            notify_get_state(token, &state)
            notify_cancel(token)
        }
        return StatsFramePacingReading(text: "ratio=\(Double(state) / 1_000)")
    }
    #endif
}

/// The frame pacing the app measured over one hooks' scroll (`TestHooks.StatsFramePacing`):
/// R2078's hitch time ratio, measured on the main thread's display link in the test VM, where
/// XCTest's own hitch metric reports nothing. Each test records its readings as an activity and
/// fails above R2078's 5 ms/s.
struct StatsFramePacingReading {
    let text: String

    /// The late time per second of the scroll, in ms/s; nil when the app sent none.
    var ratio: Double? {
        text.split(separator: ";").first { $0.hasPrefix("ratio=") }.flatMap { Double($0.dropFirst("ratio=".count)) }
    }

    /// R2078: below 5 ms/s on every measured run (VM-measured).
    @MainActor static func check(_ readings: [StatsFramePacingReading]) {
        XCTContext.runActivity(named: "meguri.stats.framePacing: " + readings.map(\.text).joined(separator: " | ")) { _ in }
        for reading in readings {
            guard let ratio = reading.ratio else {
                XCTFail("the app reported no frame pacing for a scroll")
                continue
            }
            XCTAssertLessThan(ratio, 5, "R2078: a hitch time ratio of \(ratio) ms/s while scrolling (\(reading.text))")
        }
    }
}

/// The text a notification handler hands back.
private final class StatsFramePacingText: @unchecked Sendable {
    var text = ""
}
