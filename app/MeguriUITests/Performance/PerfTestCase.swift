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
/// - `measure(app:prepare:action:)` runs the action once unmeasured
///   (warm-up: the app's caches and, once review pages exist, a warmed web
///   content process), then `iterations` times between `startMeasuring()`
///   and `stopMeasuring()`, with one `XCTOSSignpostMetric` per interval,
///   `XCTHitchMetric` and `XCTMemoryMetric`.
/// - `tools/mg perf-ui` turns the result bundle's metrics into a perf result
///   (tools/scripts/perf-compare) for U241's gate.
///
/// A new subclass joins the Performance plan's selected tests and the UI
/// plan's skipped tests (app/TestPlans).
@MainActor
class PerfTestCase: XCTestCase {
    /// The registered intervals the test measures, by name.
    nonisolated class var intervalNames: [String] { [] }

    /// Measured iterations, after the warm-up run.
    nonisolated class var iterations: Int { 5 }

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

    /// Runs `action` once unmeasured, then `iterations` measured times;
    /// `prepare` runs before each, outside the measurement.
    func measure(
        app: XCUIApplication,
        prepare: (XCUIApplication) -> Void = { _ in },
        action: (XCUIApplication) -> Void
    ) {
        prepare(app)
        action(app)
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
