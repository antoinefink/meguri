import Instrumentation
import XCTest

/// The performance smoke set (U12; U19 chooses what runs per change): a
/// registered interval with a known synthetic delay, from tap to label, and
/// the launch time. `tools/mg perf-ui` runs it on the iPhone 17 Pro
/// simulator in Profile and writes the perf result U241's gate reads.
final class SmokePerfTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] { [IntervalNames.probeSyntheticDelay.name] }

    /// `perf/probe` emits `probe.syntheticDelay` around a 30 ms delay
    /// (TestHooks.PerfProbe): the metric must report about 30 ms per run.
    /// The test starts each run with the probe's notification, not a tap:
    /// it needs no synthesized input, which macOS drops on this toolchain.
    func testProbeIntervalMeasuresItsSyntheticDelay() throws {
        let app = launch(MeguriLaunch(animations: false, route: "perf/probe"))
        let runs = app.staticTexts["perf.probe.runs"]
        XCTAssert(runs.waitForExistence(timeout: 15), "the probe screen is not on screen")
        let probe = "\(try BuildUnderTest.current().expectedBundleIdentifier).testhooks.perfProbe"
        var done = 0
        measure(app: app) { _ in
            done += 1
            post(probe)
            XCTAssert(waitForText(runs, "Runs: \(done)"), "probe run \(done) did not finish")
        }
    }

    /// Launch to the first screen, responsive (R1863's launch time).
    func testLaunch() throws {
        measureLaunch(MeguriLaunch(animations: false))
    }
}

/// The harness's own checks: they launch nothing and run in any
/// configuration.
@MainActor
final class PerfTestCaseTests: XCTestCase {
    func testAnIntervalNameThatIsNotRegisteredFailsAtSetUp() {
        let test = UnregisteredIntervalPerfTest(selector: #selector(UnregisteredIntervalPerfTest.probe))
        XCTAssertThrowsError(try test.setUpWithError()) { error in
            XCTAssert("\(error)".contains("review.answerToNext is not a registered interval"), "\(error)")
        }
    }

    func testAnyConfigurationButProfileIsRefused() {
        for configuration in [BuildUnderTest.Configuration.debug, .release, .appStore] {
            XCTAssertThrowsError(try PerfTestCase.requireProfile(configuration)) { error in
                XCTAssert("\(error)".contains("performance tests require Profile"), "\(error)")
            }
        }
        XCTAssertNoThrow(try PerfTestCase.requireProfile(.profile))
    }

    func testRegisteredNamesResolve() throws {
        XCTAssertEqual(try PerfTestCase.registered(["probe.syntheticDelay"]), [IntervalNames.probeSyntheticDelay])
    }
}

/// A performance test naming an interval that is not registered. It has no
/// `test` method of its own: only `PerfTestCaseTests` runs its setup.
final class UnregisteredIntervalPerfTest: PerfTestCase {
    override nonisolated class var intervalNames: [String] { ["review.answerToNext"] }

    @objc func probe() {}
}
