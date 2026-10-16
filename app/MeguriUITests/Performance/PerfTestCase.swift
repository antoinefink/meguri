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
///   It names the collection it measures on in `fixture` (the sizes and
///   named fixtures of tools/perf/budgets.yaml), so U241's gate applies
///   R1803's size rules to its results.
/// - It fails at setup unless `PerfPreconditions` hold (R1866): a Profile
///   build (Release optimization plus the test hooks, app/AGENTS.md), no
///   debugger, no coverage or sanitizers, no parallel testing; the
///   Performance plan and `tools/mg perf-ui` fix the same options.
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
///   U241's gate). Its fixture goes into an activity named
///   `fixturePrefix` + fixture, which perf-compare puts on its rows.
///
/// A new subclass joins the UI plan's skipped tests (app/TestPlans).
@MainActor
class PerfTestCase: XCTestCase {
    /// The registered intervals the test measures, by name.
    nonisolated class var intervalNames: [String] { [] }

    /// The collection the test measures on (150k, 500k, review-corpus, ...),
    /// as tools/perf/budgets.yaml names it; nil when it loads none.
    nonisolated class var fixture: String? { nil }

    /// Measured iterations, after the warm-up run.
    nonisolated class var iterations: Int { 5 }

    /// How long the app may take to report a warmed web content process.
    nonisolated class var webContentTimeout: TimeInterval { 30 }

    /// The title of the activity that declares a test's measured intervals,
    /// followed by their names separated by spaces (perf-compare reads it).
    nonisolated static let declarationPrefix = "meguri.perf.intervals:"

    /// The title of the activity that declares a test's fixture, followed by
    /// it (perf-compare reads it).
    nonisolated static let fixturePrefix = "meguri.perf.fixture:"

    /// The intervals of `intervalNames` (setup has checked they resolve).
    var intervals: [IntervalName] { (try? Self.registered(Self.intervalNames)) ?? [] }

    override func setUpWithError() throws {
        try super.setUpWithError()
        _ = try Self.registered(Self.intervalNames)
        try PerfPreconditions.check(try BuildUnderTest.current())
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
        declareFixture()
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

    /// The activity title declaring `fixture`.
    nonisolated static func fixtureDeclaration(_ fixture: String) -> String {
        "\(fixturePrefix) \(fixture)"
    }

    /// Records the fixture as an activity, for perf-compare.
    func declareFixture() {
        if let fixture = Self.fixture {
            XCTContext.runActivity(named: Self.fixtureDeclaration(fixture)) { _ in }
        }
    }

    /// Asks the running app for a warmed web content process and waits until
    /// it reports one ready (R1866, `PerfPreconditions`). Throws when no
    /// answer comes.
    func warmWebContent() throws {
        try PerfPreconditions.requireWarmWebContent(
            bundleIdentifier: try BuildUnderTest.current().expectedBundleIdentifier,
            timeout: Self.webContentTimeout, post: post)
    }

    /// Measures launching the app with `options` (launch time, R1863), after
    /// one unmeasured launch.
    func measureLaunch(_ options: MeguriLaunch) {
        let app = XCUIApplication()
        app.launchArguments = options.arguments
        app.launch()
        app.terminate()
        // A launch measures no interval: only the fixture is declared.
        declareFixture()
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
