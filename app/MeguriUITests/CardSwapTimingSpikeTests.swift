import Foundation
import Instrumentation
import XCTest

/// The rendering spikes' in-app checks (U65, KTD11): what needs a real app
/// on screen, a relaunch or a simulator. Run them with
/// `tools/mg perf-ui --platform iphone|mac --suite CardSwapTimingSpikeTests`
/// (Profile, in the macOS test VM). The macOS host checks are
/// CardRenderingTests' spike suites; the results of both are in
/// docs/design/decisions/s1-occluded-page-swap.md and
/// s2-custom-scheme-media.md. The routes are `TestHooks.SpikeRoutes`.
///
/// On iOS the inputs are XCUITest taps (touch-down swaps); on the Mac,
/// where macOS drops synthesized input on this toolchain, the app posts
/// mouse-downs to itself on `testhooks.cardSwapDrive`.
final class CardSwapTimingSpikeTests: PerfTestCase {
    override nonisolated class var intervalNames: [String] {
        [IntervalNames.answerToNextVisible.name, IntervalNames.probeNativeSwap.name]
    }

    /// Ten measured runs of 40 inputs: 200 page swaps and 200 baseline swaps.
    override nonisolated class var iterations: Int { 10 }
    static let inputsPerRun = 40

    struct Stats: Decodable {
        let count: Int
        let p50Frames: Int
        let p95Frames: Int
        let p99Frames: Int
        let maxFrames: Int
        let p50Ms: Double
        let p95Ms: Double
        let p99Ms: Double
        let displayLinkP95Frames: Int
        let fromHandlerP95Frames: Int
        let fromHandlerP99Frames: Int
    }

    struct Burst: Decodable {
        let swaps: Int
        let footprintAt100KB: Int
        let footprintAt1000KB: Int
        let nodesAt100: Int
        let nodesAt1000: Int
        let pages: Int
        let windowMoves: [Int]
    }

    struct Report: Decodable {
        let ready: Bool
        let probe: String
        let periodMs: Double
        let inputs: Int
        let web: Stats
        let baseline: Stats
        let unsettled: Int
        let unpresented: Int
        let confirmed: Int
        let wrongCard: Int
        let unconfirmed: Int
        let confirmP95Ms: Double
        let inputToHandlerP95Ms: Double
        let frontRAFPerSecond: Int
        let beneathRAFPerSecond: Int
        let captured: Int
        let blank: Int
        let stale: Int
        let captureControl: String
        let focused: String
        let messages: Int
        let lastConfirm: String
        let terminations: Int
        let burst: Burst?
    }

    // MARK: S1(a), S1(b)

    /// S1(b): over 200 page swaps from touch-down, the p95 frame that shows
    /// the swap is the one a native view swap reaches from the same input
    /// path (R1807 as KTD11 reads it), and p99 at most one frame later; each
    /// swap's post-swap confirmation names its card (R1862). S1(a) on
    /// screen: the page beneath runs animation frames, and every next card
    /// settled (a double requestAnimationFrame beneath) before its input.
    func testPageSwapFromInputLandsInTheBaselineSwapsFrame() throws {
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap"))
        var report = try waitForReport(app) { $0.ready }
        record(report, "S1(a)")
        // As many frames beneath as in front: occlusion does not throttle.
        XCTAssertGreaterThanOrEqual(report.beneathRAFPerSecond, 20, "the page beneath ran \(report.beneathRAFPerSecond) frames a second")
        XCTAssertGreaterThanOrEqual(Double(report.beneathRAFPerSecond), Double(report.frontRAFPerSecond) * 0.8)
        var inputs = 0
        try measure(app: app) { app in
            inputs += Self.inputsPerRun
            input(app, count: Self.inputsPerRun, expectingTotal: inputs)
        }
        report = try waitForReport(app) {
            $0.web.count + $0.unsettled >= inputs / 2 && $0.confirmed + $0.unconfirmed + $0.wrongCard + $0.unsettled >= inputs / 2 - 1
        }
        record(report, "S1(b)")
        XCTAssertGreaterThanOrEqual(report.web.count, 200)
        XCTAssertGreaterThanOrEqual(report.baseline.count, 200)
        XCTAssertEqual(report.unsettled, 0, "inputs found the next card not yet settled beneath")
        XCTAssertEqual(report.wrongCard, 0)
        XCTAssertLessThanOrEqual(report.unconfirmed, 1, "only the last swap may still wait for its confirmation")
        XCTAssertGreaterThanOrEqual(report.confirmed, 199)
        // Frames are exact only where the probe saw every swap presented (a
        // device, this Mac's own screen): there the pages must land in the
        // baseline's frame at p95 and at most one frame later at p99
        // (R1807). Elsewhere they are display-link estimates, which count
        // the main-thread work after a page swap (its confirmation and the
        // next card) as lateness: the simulator and the test VM check
        // function only (KTD19), with one frame of slack at p95.
        let presented = report.probe == "metal" && report.unpresented == 0
        let slack = presented ? 0 : 1
        XCTAssertLessThanOrEqual(
            report.web.p95Frames, report.baseline.p95Frames + slack,
            "p95 frames: pages \(report.web.p95Frames), baseline \(report.baseline.p95Frames)")
        if presented {
            XCTAssertLessThanOrEqual(report.web.fromHandlerP95Frames, report.baseline.fromHandlerP95Frames)
            XCTAssertLessThanOrEqual(report.web.p99Frames, report.baseline.p99Frames + 1)
        }
        XCTAssertEqual(report.terminations, 0)
    }

    // MARK: S1(c)

    /// S1(c), iOS: at every page swap's presented frame the screen shows the
    /// new card's colour, never white or the previous card. The control is a
    /// capture of the first card before any swap.
    func testNoBlankOrStaleFrameAtThePageSwap() throws {
        #if os(macOS)
        throw XCTSkip("the Mac has no in-process capture of web content; S1(c) on the Mac is the device session's (decision record)")
        #else
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap/capture"))
        var report = try waitForReport(app) { $0.ready }
        XCTAssertEqual(report.captureControl, "255,239,213", "the capture does not show web content; S1(c) cannot be checked this way")
        input(app, count: Self.inputsPerRun, expectingTotal: Self.inputsPerRun)
        report = try waitForReport(app) { $0.captured >= Self.inputsPerRun / 2 }
        record(report, "S1(c)")
        XCTAssertEqual(report.blank, 0)
        XCTAssertEqual(report.stale, 0)
        #endif
    }

    // MARK: S1(e)

    /// S1(e), iOS: `#typeans.focus()` from page script right after a swap
    /// focuses the field; whether the software keyboard comes up without a
    /// tap is recorded (R534). The control is a native field: a simulator
    /// with a hardware keyboard attached raises none at all.
    func testFocusInTheNewlyFrontPageAndTheKeyboard() throws {
        #if os(macOS)
        throw XCTSkip("the software keyboard is iOS only")
        #else
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap/typein"))
        _ = try waitForReport(app) { $0.ready }
        input(app, count: 1, expectingTotal: 1)
        let report = try waitForReport(app) { !$0.focused.isEmpty }
        let pageKeyboard = app.keyboards.firstMatch.waitForExistence(timeout: 3)
        record(report, "S1(e) keyboard after page focus: \(pageKeyboard)")
        XCTAssertEqual(report.focused, "typeans")
        let control = app.textFields["spike.cardSwap.control"]
        control.tap()
        let controlKeyboard = app.keyboards.firstMatch.waitForExistence(timeout: 3)
        XCTContext.runActivity(named: "S1(e) keyboard after a tap on a native field: \(controlKeyboard)") { _ in }
        guard controlKeyboard else {
            throw XCTSkip("this simulator shows no software keyboard even for a native field (hardware keyboard attached)")
        }
        XCTAssertEqual(pageKeyboard, Self.pageFocusRaisesKeyboard, "the decision record says \(Self.pageFocusRaisesKeyboard)")
        #endif
    }

    /// What S1(e) found on the iPhone 17 Pro simulator: the newly front page
    /// made first responder in the swap's turn, then `focus()` from page
    /// script after the commit, raises the keyboard. The device check is
    /// U256's (decision record).
    static let pageFocusRaisesKeyboard = true

    // MARK: S1(f)

    /// S1(f) on screen: 1,000 page swaps with no input keep the same two
    /// pages in their window, the DOM the same size, and the app's memory
    /// within R1853's 5 MB from swap 100 to 1,000.
    func testAThousandSwapsKeepTheSamePagesAndFlatMemory() throws {
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap"))
        _ = try waitForReport(app) { $0.ready }
        post("\(try BuildUnderTest.current().expectedBundleIdentifier).testhooks.cardSwapBurst")
        let report = try waitForReport(app, timeout: 600) { $0.burst?.swaps == 1000 }
        record(report, "S1(f)")
        let burst = try XCTUnwrap(report.burst)
        XCTAssertEqual(burst.pages, 2)
        XCTAssertEqual(burst.windowMoves, [1, 1])
        XCTAssertEqual(burst.nodesAt1000, burst.nodesAt100)
        XCTAssertLessThan(burst.footprintAt1000KB - burst.footprintAt100KB, 5 * 1024)
        XCTAssertEqual(report.terminations, 0)
    }

    // MARK: S2(a), S2(d)

    struct MediaOutcome: Decodable {
        let video: String
        let videoRanged: Int
        let videoLate: Int
    }

    struct Played: Decodable {
        let seekedTo: Double?
        let playedTo: Double?
        let error: String?
    }

    /// S2(a) in the app: a muted H.264 video on the media origin seeks to
    /// 90 % and plays on, through ranged requests answered 206. Audio is the
    /// macOS host's check (CustomSchemeMediaSpikeTests) and the device
    /// session's: no `<audio>` plays in the test VM.
    func testVideoSeeksToNinetyPercentThroughRangeRequests() throws {
        let app = launch(MeguriLaunch(animations: false, route: "spike/media-seek"))
        let text = text(app.staticTexts["spike.media.result"], notStartingWith: "running", timeout: 120)
        XCTContext.runActivity(named: "S2(a) \(text)") { _ in }
        let outcome = try JSONDecoder().decode(MediaOutcome.self, from: Data(text.utf8))
        let video = try JSONDecoder().decode(Played.self, from: Data(outcome.video.utf8))
        XCTAssertNil(video.error, outcome.video)
        let seekedTo = try XCTUnwrap(video.seekedTo), playedTo = try XCTUnwrap(video.playedTo)
        XCTAssertGreaterThanOrEqual(seekedTo, 17.9, "seeked to \(seekedTo) of 20 s")
        XCTAssertGreaterThan(playedTo, seekedTo + 0.3)
        XCTAssertGreaterThan(outcome.videoLate, 0, "no ranged request in the last fifth")
    }

    /// S2(d): a value written to localStorage and IndexedDB on the media
    /// origin, in the persistent store with the fixed identifier, is read
    /// back after the app is relaunched (R502).
    func testStorageOnTheMediaOriginSurvivesARelaunch() throws {
        let writer = launch(MeguriLaunch(animations: false, route: "spike/storage/write"))
        let written = text(writer.staticTexts["spike.storage.result"], notEqualTo: "running", timeout: 30)
        let value = try XCTUnwrap(written.wholeMatch(of: /written ([0-9A-F-]{36})/)?.1, written)
        writer.terminate()
        let reader = launch(MeguriLaunch(animations: false, route: "spike/storage/read"))
        let read = text(reader.staticTexts["spike.storage.result"], notEqualTo: "running", timeout: 30)
        XCTContext.runActivity(named: "S2(d) wrote \(value), read \(read)") { _ in }
        XCTAssertEqual(read, "read \(value) \(value)")
    }

    // MARK: Helpers

    /// Sends `count` inputs: taps on iOS, the app's own mouse-downs on the
    /// Mac; waits until the swap count reaches `total`.
    private func input(_ app: XCUIApplication, count: Int, expectingTotal total: Int) {
        let counter = app.staticTexts["spike.cardSwap.count"]
        #if os(macOS)
        precondition(count % 40 == 0 || count == 1, "the Mac drive posts 40 inputs")
        for _ in 0..<max(count / 40, 1) {
            post("\((try? BuildUnderTest.current().expectedBundleIdentifier) ?? "").testhooks.cardSwapDrive")
        }
        #else
        let surface = app.otherElements["spike.cardSwap.input"]
        XCTAssert(surface.waitForExistence(timeout: 10), "no input surface")
        let point = surface.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6))
        for _ in 0..<count { point.tap() }
        #endif
        XCTAssert(waitForText(counter, "Swaps: \(total)", timeout: 60), "the swap count did not reach \(total)")
    }

    private func waitForReport(_ app: XCUIApplication, timeout: TimeInterval = 30, until done: (Report) -> Bool) throws -> Report {
        let element = app.staticTexts["spike.cardSwap.report"]
        let deadline = Date().addingTimeInterval(timeout)
        var last = ""
        repeat {
            last = text(element, notEqualTo: "", timeout: 5)
            if let report = try? JSONDecoder().decode(Report.self, from: Data(last.utf8)), done(report) {
                lastReport = last
                return report
            }
            Thread.sleep(forTimeInterval: 0.5)
        } while Date() < deadline
        throw PreconditionFailure(description: "the spike's report did not reach the expected state: \(last)")
    }

    private func text(_ element: XCUIElement, notEqualTo placeholder: String, timeout: TimeInterval) -> String {
        text(element, timeout: timeout) { $0 != placeholder }
    }

    private func text(_ element: XCUIElement, notStartingWith prefix: String, timeout: TimeInterval) -> String {
        text(element, timeout: timeout) { !$0.hasPrefix(prefix) }
    }

    /// The element's text (its label on iOS, its value on the Mac) once
    /// `done` accepts it, or the last one read after `timeout`.
    private func text(_ element: XCUIElement, timeout: TimeInterval, until done: (String) -> Bool) -> String {
        guard element.waitForExistence(timeout: timeout) else { return "" }
        let deadline = Date().addingTimeInterval(timeout)
        var current = ""
        repeat {
            current = element.label.isEmpty ? (element.value as? String ?? "") : element.label
            if done(current) { return current }
            Thread.sleep(forTimeInterval: 0.3)
        } while Date() < deadline
        return current
    }

    /// Keeps the report with the result bundle (the UI plan keeps no system
    /// attachments) and in an activity's title, which perf-ui exports.
    private func record(_ report: Report, _ title: String) {
        XCTContext.runActivity(named: "\(title): \(lastReport)") { activity in
            let attachment = XCTAttachment(string: lastReport)
            attachment.name = "\(title).json"
            attachment.lifetime = .keepAlways
            activity.add(attachment)
        }
    }

    /// The report text `waitForReport` last read.
    private var lastReport = ""
}
