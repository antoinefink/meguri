import Foundation
import XCTest

/// Checks of S1(b)'s frame gate (`CardSwapTimingGate`): they launch nothing
/// and measure nothing, so `tools/mg perf-ui` lists the class in
/// `MG_PERF_UI_CHECK_SUITES` and the UI plan skips it.
final class CardSwapTimingGateTests: XCTestCase {
    /// A report that passes the strict gate: every input presented by the
    /// Metal probe, the pages in the baseline's frames.
    static func passing() throws -> CardSwapTimingSpikeTests.Report {
        let stats = #"{"count": 200, "p50Frames": 2, "p95Frames": 3, "p99Frames": 4, "maxFrames": 5, "p50Ms": 20, "p95Ms": 28, "p99Ms": 36, "displayLinkP95Frames": 3, "fromHandlerP95Frames": 2, "fromHandlerP99Frames": 3}"#
        let json = """
            {"ready": true, "probe": "metal", "periodMs": 8.33, "inputs": 400, "web": \(stats), "baseline": \(stats),
             "unsettled": 0, "unpresented": 0, "confirmed": 200, "wrongCard": 0, "unconfirmed": 0, "confirmP95Ms": 20,
             "inputToHandlerP95Ms": 1, "frontRAFPerSecond": 120, "beneathRAFPerSecond": 120, "captured": 0, "blank": 0,
             "stale": 0, "captureControls": [], "capturedFrames": 0, "framesWithoutDisplayTime": 0, "lateWithinAFrame": 0, "captureError": "", "focused": "", "messages": 200, "lastConfirm": "posted", "terminations": 0}
            """
        return try JSONDecoder().decode(CardSwapTimingSpikeTests.Report.self, from: Data(json.utf8))
    }

    static let functionOnly = CardSwapTimingGate.Mode.functionOnly("a virtual machine")

    func testThePassingReportPassesBothModes() throws {
        XCTAssertEqual(CardSwapTimingGate.problems(try Self.passing(), mode: .strict), [])
        XCTAssertEqual(CardSwapTimingGate.problems(try Self.passing(), mode: Self.functionOnly), [])
    }

    // MARK: The mode

    func testTheModeComesFromTheEnvironment() {
        XCTAssertEqual(CardSwapTimingGate.mode(simulator: false, virtualMachine: false), .strict)
        XCTAssertEqual(CardSwapTimingGate.mode(simulator: true, virtualMachine: nil), .functionOnly("the iOS simulator"))
        XCTAssertEqual(CardSwapTimingGate.mode(simulator: true, virtualMachine: true), .functionOnly("the iOS simulator"))
        XCTAssertEqual(CardSwapTimingGate.mode(simulator: false, virtualMachine: true), .functionOnly("a virtual machine"))
    }

    /// A Mac whose `kern.hv_vmm_present` cannot be read is held to the
    /// strict gate: not knowing is no reason to relax it.
    func testAnUnknownMachineIsHeldToTheStrictGate() {
        XCTAssertEqual(CardSwapTimingGate.mode(simulator: false, virtualMachine: nil), .strict)
    }

    @MainActor
    func testThisRunnersModeMatchesItsEnvironment() {
        #if targetEnvironment(simulator)
        XCTAssertEqual(CardSwapTimingGate.current, .functionOnly("the iOS simulator"))
        #elseif os(macOS)
        let virtual = CardSwapTimingGate.hypervisorPresent()
        XCTContext.runActivity(named: "kern.hv_vmm_present: \(virtual.map(String.init) ?? "unreadable")") { _ in }
        XCTAssertEqual(CardSwapTimingGate.current, virtual == true ? .functionOnly("a virtual machine") : .strict)
        #else
        XCTAssertEqual(CardSwapTimingGate.current, .strict)
        #endif
    }

    // MARK: Strict

    /// The review finding this gate answers: one swap without a presented
    /// time once relaxed the gate to function only. Strict stays strict and
    /// fails, even when the frames themselves would pass.
    func testASwapWithoutAPresentedTimeFailsTheStrictGate() throws {
        var report = try Self.passing()
        report.unpresented = 1
        let problems = CardSwapTimingGate.problems(report, mode: .strict)
        XCTAssert(problems.contains { $0.contains("no presented time") }, "\(problems)")
    }

    /// Strict keeps gating p99 and the handler's p95 when a sample has no
    /// presented time: a failed probe must not hide a late page.
    func testAFailedProbeDoesNotSkipTheP99AndHandlerGates() throws {
        var report = try Self.passing()
        report.unpresented = 1
        report.web.p95Frames = report.baseline.p95Frames + 1
        report.web.p99Frames = report.baseline.p99Frames + 2
        report.web.fromHandlerP95Frames = report.baseline.fromHandlerP95Frames + 1
        let problems = CardSwapTimingGate.problems(report, mode: .strict)
        XCTAssert(problems.contains { $0.hasPrefix("p95 frames:") }, "\(problems)")
        XCTAssert(problems.contains { $0.hasPrefix("p99 frames:") }, "\(problems)")
        XCTAssert(problems.contains { $0.hasPrefix("p95 frames from the handler:") }, "\(problems)")
    }

    func testStrictNeedsTheMetalProbe() throws {
        var report = try Self.passing()
        report.probe = "displayLink"
        let problems = CardSwapTimingGate.problems(report, mode: .strict)
        XCTAssert(problems.contains { $0.contains("probe") }, "\(problems)")
    }

    func testStrictNeedsEveryInputResolved() throws {
        var report = try Self.passing()
        report.web.count -= 1
        let problems = CardSwapTimingGate.problems(report, mode: .strict)
        XCTAssert(problems.contains { $0.contains("399 of 400 inputs") }, "\(problems)")
    }

    func testStrictGatesEachFrameRule() throws {
        var late = try Self.passing()
        late.web.p95Frames += 1
        XCTAssertEqual(CardSwapTimingGate.problems(late, mode: .strict), ["p95 frames: pages 4, baseline 3"])
        late = try Self.passing()
        late.web.fromHandlerP95Frames += 1
        XCTAssertEqual(CardSwapTimingGate.problems(late, mode: .strict), ["p95 frames from the handler: pages 3, baseline 2"])
        late = try Self.passing()
        late.web.p99Frames += 2
        XCTAssertEqual(CardSwapTimingGate.problems(late, mode: .strict), ["p99 frames: pages 6, baseline 4"])
        // p99 may be one frame later than the baseline's.
        late = try Self.passing()
        late.web.p99Frames += 1
        XCTAssertEqual(CardSwapTimingGate.problems(late, mode: .strict), [])
    }

    // MARK: Function only

    /// Where frames are estimates: one frame of slack at p95, nothing else
    /// about frames.
    func testFunctionOnlyAllowsOneFrameOfSlackAtP95() throws {
        var report = try Self.passing()
        report.probe = "displayLink"
        report.unpresented = 400
        report.web.p95Frames = report.baseline.p95Frames + 1
        report.web.p99Frames = report.baseline.p99Frames + 30
        report.web.fromHandlerP95Frames = report.baseline.fromHandlerP95Frames + 3
        XCTAssertEqual(CardSwapTimingGate.problems(report, mode: Self.functionOnly), [])
        report.web.p95Frames += 1
        XCTAssertEqual(CardSwapTimingGate.problems(report, mode: Self.functionOnly), ["p95 frames: pages 5, baseline 3"])
    }
}
