import Darwin
import Foundation
import Instrumentation
import XCTest
#if !os(macOS)
import notify
#endif

/// The base class of every UI performance test (R1863, R1866; U12).
///
/// - A subclass names the intervals it measures in `intervalNames`; each
///   must be in the registry (`IntervalNames`), or the test fails at setup.
/// - It runs only in the Profile configuration (Release optimization plus
///   the test hooks, app/AGENTS.md) and without a debugger, else it fails at
///   setup; the Performance plan and `tools/mg perf-ui` fix the rest of
///   R1866 (no coverage, sanitizers, screenshots or parallel testing).
/// - `measure(app:prepare:action:)` asks the app for a warmed web content
///   process and waits for it (R1866, `TestHooks.WebContentWarmup`), runs
///   the action once unmeasured (warm-up: the app's caches and the warmed
///   page), checks the web content process still answers, then runs the
///   action `iterations` times between `startMeasuring()` and
///   `stopMeasuring()`, with one `XCTOSSignpostMetric` per interval,
///   `XCTHitchMetric` and `XCTMemoryMetric`. Without a ready process it
///   throws before measuring.
/// - It also records the intervals it measures as an activity named
///   `declarationPrefix` + names: `tools/mg perf-ui` fails a test whose
///   declared interval has no measurements, and a performance suite with no
///   metrics (tools/scripts/perf-compare, which writes the perf result for
///   U241's gate).
///
/// A new subclass joins the UI plan's skipped tests (app/TestPlans).
@MainActor
class PerfTestCase: XCTestCase {
    /// The registered intervals the test measures, by name.
    nonisolated class var intervalNames: [String] { [] }

    /// Measured iterations, after the warm-up run.
    nonisolated class var iterations: Int { 5 }

    /// How long the app may take to report a warmed web content process.
    nonisolated class var webContentTimeout: TimeInterval { 30 }

    /// The title of the activity that declares a test's measured intervals,
    /// followed by their names separated by spaces (perf-compare reads it).
    nonisolated static let declarationPrefix = "meguri.perf.intervals:"

    /// The intervals of `intervalNames` (setup has checked they resolve).
    var intervals: [IntervalName] { (try? Self.registered(Self.intervalNames)) ?? [] }

    override func setUpWithError() throws {
        try super.setUpWithError()
        _ = try Self.registered(Self.intervalNames)
        try Self.requireProfile(try BuildUnderTest.current().configuration)
        try Self.requireNoDebugger()
    }

    // MARK: Preconditions

    struct PreconditionFailure: Error, CustomStringConvertible {
        let description: String
    }

    /// The registered intervals named `names`; throws on the first unknown
    /// one.
    nonisolated static func registered(_ names: [String]) throws -> [IntervalName] {
        try names.map { name in
            guard let interval = IntervalNames.named(name) else {
                throw PreconditionFailure(
                    description: "\(name) is not a registered interval: performance tests measure only IntervalNames (R1861)")
            }
            return interval
        }
    }

    nonisolated static func requireProfile(_ configuration: BuildUnderTest.Configuration) throws {
        guard configuration == .profile else {
            throw PreconditionFailure(
                description: "performance tests require Profile (R1866); this bundle was built in \(configuration.rawValue)")
        }
    }

    /// The test runner is not being debugged (R1866).
    nonisolated static func requireNoDebugger() throws {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, u_int(mib.count), &info, &size, nil, 0) == 0 else { return }
        if info.kp_proc.p_flag & P_TRACED != 0 {
            throw PreconditionFailure(description: "performance tests run without a debugger (R1866)")
        }
    }

    // MARK: Running

    /// Launches the app with test-hook options.
    func launch(_ options: MeguriLaunch) -> XCUIApplication {
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = options.arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    /// One signpost metric per interval, then hitches and memory of `app`.
    func metrics(for app: XCUIApplication) -> [any XCTMetric] {
        intervals.map { XCTOSSignpostMetric(subsystem: $0.subsystem, category: $0.category.rawValue, name: $0.name) }
            + [XCTHitchMetric(application: app), XCTMemoryMetric(application: app)]
    }

    /// Warms the app's web content process, runs `action` once unmeasured,
    /// checks the process is still ready, then runs `action` `iterations`
    /// measured times; `prepare` runs before each, outside the measurement.
    func measure(
        app: XCUIApplication,
        prepare: (XCUIApplication) -> Void = { _ in },
        action: (XCUIApplication) -> Void
    ) throws {
        try warmWebContent()
        prepare(app)
        action(app)
        try warmWebContent()
        XCTContext.runActivity(named: Self.declaration(intervals)) { _ in }
        let options = XCTMeasureOptions()
        options.invocationOptions = [.manuallyStart, .manuallyStop]
        options.iterationCount = Self.iterations
        measure(metrics: metrics(for: app), options: options) {
            prepare(app)
            startMeasuring()
            action(app)
            stopMeasuring()
        }
    }

    /// The activity title declaring `intervals`.
    nonisolated static func declaration(_ intervals: [IntervalName]) -> String {
        ([declarationPrefix] + intervals.map(\.name)).joined(separator: " ")
    }

    /// Asks the running app for a warmed web content process and waits until
    /// it reports one ready (R1866): the notification
    /// `<bundle id>.testhooks.warmWebContent`, answered by
    /// `<bundle id>.testhooks.webContentReady`. Throws when no answer comes.
    func warmWebContent() throws {
        let id = try BuildUnderTest.current().expectedBundleIdentifier
        let ready = "\(id).testhooks.webContentReady"
        #if os(macOS)
        let answer = XCTNSNotificationExpectation(
            name: Notification.Name(ready), object: nil, notificationCenter: DistributedNotificationCenter.default())
        #else
        let answer = XCTDarwinNotificationExpectation(notificationName: ready)
        #endif
        post("\(id).testhooks.warmWebContent")
        guard XCTWaiter().wait(for: [answer], timeout: Self.webContentTimeout) == .completed else {
            throw PreconditionFailure(
                description: "the app reported no warmed web content process within \(Self.webContentTimeout) s (R1866)")
        }
    }

    /// Measures launching the app with `options` (launch time, R1863), after
    /// one unmeasured launch.
    func measureLaunch(_ options: MeguriLaunch) {
        let app = XCUIApplication()
        app.launchArguments = options.arguments
        app.launch()
        app.terminate()
        let measure = XCTMeasureOptions()
        measure.iterationCount = Self.iterations
        self.measure(metrics: [XCTApplicationLaunchMetric(waitUntilResponsive: true)], options: measure) {
            app.launch()
            app.terminate()
        }
    }

    /// Posts a test-hook notification to the app: Darwin on iOS (the runner
    /// and the app share the simulator), distributed on the Mac, delivered
    /// at once (the app is usually not the active app).
    func post(_ name: String) {
        #if os(macOS)
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name(name), object: nil, userInfo: nil, deliverImmediately: true)
        #else
        notify_post(name)
        #endif
    }

    /// Waits until a static text reads `text` (its label on iOS, its value
    /// on the Mac).
    func waitForText(_ element: XCUIElement, _ text: String, timeout: TimeInterval = 10) -> Bool {
        let predicate = NSPredicate(format: "label == %@ OR value == %@", text, text)
        return XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: timeout) == .completed
    }
}
