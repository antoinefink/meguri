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
        try measure(app: app) { _ in
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
            XCTAssertThrowsError(try PerfPreconditions.requireProfile(configuration)) { error in
                XCTAssert("\(error)".contains("performance tests require Profile"), "\(error)")
            }
        }
        XCTAssertNoThrow(try PerfPreconditions.requireProfile(.profile))
    }

    func testAnAttachedDebuggerIsRefused() {
        XCTAssertThrowsError(try PerfPreconditions.requireNoDebugger(traced: true)) { error in
            XCTAssert("\(error)".contains("without a debugger"), "\(error)")
        }
        XCTAssertNoThrow(try PerfPreconditions.requireNoDebugger(traced: false))
    }

    func testCoverageAndSanitizerRuntimesInTheAppAreRefused() {
        let plain = Data("__TEXT __text _main".utf8)
        XCTAssertNoThrow(try PerfPreconditions.requireUninstrumented([("Meguri", plain)]))
        for (marker, expected) in [("__llvm_prf_cnts", "code coverage"), ("___llvm_profile_runtime", "code coverage"),
                                   ("@rpath/libclang_rt.asan_iossim_dynamic.dylib", "sanitizer"),
                                   ("@rpath/libclang_rt.tsan_osx_dynamic.dylib", "sanitizer"),
                                   ("@rpath/libclang_rt.ubsan_iossim_dynamic.dylib", "sanitizer")] {
            let binary = plain + Data(marker.utf8) + plain
            XCTAssertThrowsError(try PerfPreconditions.requireUninstrumented([("Meguri", binary)]), marker) { error in
                XCTAssert("\(error)".contains(expected) && "\(error)".contains("R1866"), "\(error)")
            }
        }
    }

    func testAnInstrumentedRunnerIsRefused() {
        XCTAssertNoThrow(try PerfPreconditions.requireUninstrumented(environment: ["HOME": "/tmp"]))
        XCTAssertThrowsError(try PerfPreconditions.requireUninstrumented(
            environment: ["DYLD_INSERT_LIBRARIES": "/x/libclang_rt.asan_iossim_dynamic.dylib"]))
        // Xcode sets it for every test run, coverage or not: not evidence.
        XCTAssertNoThrow(try PerfPreconditions.requireUninstrumented(environment: ["LLVM_PROFILE_FILE": "/x/%p.profraw"]))
    }

    func testAParallelTestingCloneIsRefused() {
        XCTAssertNoThrow(try PerfPreconditions.requireSerialTesting(["SIMULATOR_DEVICE_NAME": "Meguri-Pool-iphone-1"]))
        XCTAssertNoThrow(try PerfPreconditions.requireSerialTesting([:]))
        XCTAssertThrowsError(try PerfPreconditions.requireSerialTesting(
            ["SIMULATOR_DEVICE_NAME": "Clone 1 of Meguri-Pool-iphone-1"])) { error in
            XCTAssert("\(error)".contains("parallel"), "\(error)")
        }
    }

    /// The app this run tests carries no coverage or sanitizer runtime, and
    /// the runner runs serially and uninstrumented, whatever the
    /// configuration (the checks performance tests make at setup, Profile
    /// aside): the checks do not refuse a clean run.
    func testTheBuildUnderTestIsUninstrumented() throws {
        let binaries = try PerfPreconditions.binaries(try BuildUnderTest.current())
        XCTAssertFalse(binaries.isEmpty, "no executable in the app under test")
        XCTAssertNoThrow(try PerfPreconditions.requireUninstrumented(binaries))
        XCTAssertNoThrow(try PerfPreconditions.requireUninstrumented(environment: ProcessInfo.processInfo.environment))
        XCTAssertNoThrow(try PerfPreconditions.requireSerialTesting(ProcessInfo.processInfo.environment))
    }

    func testRegisteredNamesResolve() throws {
        XCTAssertEqual(try PerfTestCase.registered(["probe.syntheticDelay"]), [IntervalNames.probeSyntheticDelay])
    }

    func testTheDeclarationNamesTheMeasuredIntervals() throws {
        XCTAssertEqual(
            PerfTestCase.declaration(try PerfTestCase.registered(["probe.syntheticDelay", "review.answerToNextVisible"])),
            "meguri.perf.intervals: probe.syntheticDelay review.answerToNextVisible")
        XCTAssertEqual(PerfTestCase.declaration([]), "meguri.perf.intervals:")
    }

    func testTheFixtureDeclarationNamesTheFixture() {
        XCTAssertEqual(PerfTestCase.fixtureDeclaration("150k"), "meguri.perf.fixture: 150k")
        XCTAssertNil(SmokePerfTests.fixture)
    }
}

/// A performance test naming an interval that is not registered. It has no
/// `test` method of its own: only `PerfTestCaseTests` runs its setup.
final class UnregisteredIntervalPerfTest: PerfTestCase {
    override nonisolated class var intervalNames: [String] { ["review.answerToNext"] }

    @objc func probe() {}
}
