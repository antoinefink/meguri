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
        var count: Int
        var p50Frames: Int
        var p95Frames: Int
        var p99Frames: Int
        var maxFrames: Int
        var p50Ms: Double
        var p95Ms: Double
        var p99Ms: Double
        var displayLinkP95Frames: Int
        var fromHandlerP95Frames: Int
        var fromHandlerP99Frames: Int
    }

    struct Burst: Decodable {
        var retaining: Bool
        var swaps: Int
        var footprintAt100KB: Int
        var footprintAt1000KB: Int
        /// -1 where the system does not tell (iOS).
        var webContentWarmKB: Int
        var webContentAt100KB: Int
        var webContentAt1000KB: Int
        var webContentClosedKB: Int
        var webContentNote: String
        var nodesAt100: Int
        var nodesAt1000: Int
        var pages: Int
        var windowMoves: [Int]
        var cardElementsRegistered: Int
        var cardElementsFinalized: Int
    }

    struct Report: Decodable {
        var ready: Bool
        var probe: String
        var periodMs: Double
        var inputs: Int
        var web: Stats
        var baseline: Stats
        var unsettled: Int
        var unpresented: Int
        var confirmed: Int
        var wrongCard: Int
        var unconfirmed: Int
        var confirmP95Ms: Double
        var inputToHandlerP95Ms: Double
        var frontRAFPerSecond: Int
        var beneathRAFPerSecond: Int
        var captured: Int
        var blank: Int
        var stale: Int
        var captureControls: [String]
        var capturedFrames: Int
        var framesWithoutDisplayTime: Int
        var lateWithinAFrame: Int
        var captureError: String
        var focused: String
        var messages: Int
        var lastConfirm: String
        var terminations: Int
        var burst: Burst?
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
        // Every input resolved: each swap has its frame (presented, or the
        // display link's estimate), and the confirmations are in. A run that
        // never gets there is read as it stands, and the gate names what is
        // missing.
        report = try waitForReport(app, orLatest: true) {
            $0.web.count + $0.baseline.count + $0.unsettled >= $0.inputs
                && $0.confirmed + $0.unconfirmed + $0.wrongCard + $0.unsettled >= inputs / 2 - 1
        }
        let mode = CardSwapTimingGate.current
        record(report, "S1(b), \(mode)")
        XCTAssertGreaterThanOrEqual(report.web.count, 200)
        XCTAssertGreaterThanOrEqual(report.baseline.count, 200)
        XCTAssertEqual(report.unsettled, 0, "inputs found the next card not yet settled beneath")
        XCTAssertEqual(report.wrongCard, 0)
        XCTAssertLessThanOrEqual(report.unconfirmed, 1, "only the last swap may still wait for its confirmation")
        XCTAssertGreaterThanOrEqual(report.confirmed, 199)
        // The frame gate's mode comes from where the run is, never from the
        // samples (CardSwapTimingGate): strict on a device or this Mac's own
        // screen, function only on the simulator and in the test VM, where
        // frames are display-link estimates (KTD19).
        for problem in CardSwapTimingGate.problems(report, mode: mode) {
            XCTFail("S1(b), \(mode): \(problem)")
        }
        XCTAssertEqual(report.terminations, 0)
    }

    // MARK: S1(c)

    /// S1(c): every page swap's frame shows the new card's marker, a band
    /// whose colour names the card, never one with no card's marker (white,
    /// the page's own background) or an older card's. On iOS the window is
    /// rendered at each swap's frame; on the Mac every frame of the app's own
    /// window is captured (ScreenCaptureKit). Controls first: the check, run
    /// on known screens before any swap, must name card 1 shown as the card,
    /// as stale where card 2 or card 3 is expected, and as blank once the
    /// front page's card is hidden; or it could not tell those frames apart.
    func testNoBlankOrStaleFrameAtThePageSwap() throws {
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap/capture"))
        var report = try waitForReport(app) { $0.ready }
        record(report, "S1(c) controls")
        XCTAssertEqual(report.captureError, "", "the window capture did not start")
        XCTAssertEqual(report.captureControls, ["card", "stale", "stale", "blank", "card"], "the check cannot tell a blank or stale frame from the card")
        input(app, count: Self.inputsPerRun, expectingTotal: Self.inputsPerRun)
        let swaps = Self.inputsPerRun / 2
        report = try waitForReport(app, orLatest: true) { $0.captured + $0.blank + $0.stale >= swaps && $0.web.count >= swaps }
        record(report, "S1(c)")
        XCTAssertEqual(report.captured, swaps, "page swaps whose new card showed")
        XCTAssertEqual(report.blank, 0)
        XCTAssertEqual(report.stale, 0)
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
    /// within R1853's 5 MB from swap 100 to 1,000; once the session closes,
    /// all but a few of the cards' elements have been collected. Web content
    /// memory is read where the app can read it (the iOS simulator, whose
    /// processes are the VM's; not a device, and the Mac's app sandbox
    /// refuses it) and held to R1853 as U245 reads it (KTD11's call-out):
    /// within 5 MB from swap 100 to 1,000, and back within 10 MB of the warm
    /// level once the session closed. S1(f) failed that reading on screen
    /// as off screen (decision record S1, escalated): it is an expected
    /// failure until the plan decides, and a run that meets it shows.
    func testAThousandSwapsKeepTheSamePagesAndFlatMemory() throws {
        let burst = try runBurst(trigger: "cardSwapBurst", title: "S1(f)")
        XCTAssertFalse(burst.retaining)
        XCTAssertEqual(burst.pages, 2)
        XCTAssertEqual(burst.windowMoves, [1, 1])
        XCTAssertEqual(burst.nodesAt1000, burst.nodesAt100)
        XCTAssertLessThan(burst.footprintAt1000KB - burst.footprintAt100KB, 5 * 1024)
        // The 1,000 cards prepared during the burst, each with five
        // elements (marker, heading, image, paragraph, script).
        XCTAssertEqual(burst.cardElementsRegistered, 5000)
        XCTAssertGreaterThanOrEqual(burst.cardElementsFinalized * 100, burst.cardElementsRegistered * 95, "old cards stayed alive in their pages")
        guard Self.webContentReadable(burst) else { return }
        let options = XCTExpectedFailure.Options()
        options.isStrict = false
        XCTExpectFailure("S1(f) failed for web content memory (decision record S1, escalated)", options: options) {
            XCTAssertLessThan(burst.webContentAt1000KB - burst.webContentAt100KB, 5 * 1024, "web content growth from swap 100 to 1,000")
            XCTAssertLessThan(burst.webContentClosedKB - burst.webContentWarmKB, 10 * 1024, "web content above warm after the session closed")
        }
    }

    /// The web content check's control: cards that each keep 64 KB in their
    /// page's script heap (57.6 MB from swap 100 to 1,000) keep the DOM the
    /// same size and the app's memory flat, but the web content check sees
    /// most of what they keep.
    func testTheMemoryCheckCatchesCardsThatRetainMemoryInTheirPages() throws {
        let burst = try runBurst(trigger: "cardSwapBurstRetaining", title: "S1(f) control, cards keeping 64 KB")
        XCTAssertTrue(burst.retaining)
        XCTAssertEqual(burst.nodesAt1000, burst.nodesAt100)
        guard Self.webContentReadable(burst) else { return }
        XCTAssertGreaterThanOrEqual(burst.webContentAt1000KB - burst.webContentAt100KB, 40 * 1024, "the web content check missed what the cards kept")
    }

    /// Launches the harness, starts a burst with `trigger` and returns it
    /// once 1,000 swaps are done.
    private func runBurst(trigger: String, title: String) throws -> Burst {
        let app = launch(MeguriLaunch(animations: false, route: "spike/card-swap"))
        _ = try waitForReport(app) { $0.ready }
        post("\(try BuildUnderTest.current().expectedBundleIdentifier).testhooks.\(trigger)")
        let report = try waitForReport(app, timeout: 600) { $0.burst?.swaps == 1000 }
        record(report, title)
        XCTAssertEqual(report.terminations, 0)
        return try XCTUnwrap(report.burst)
    }

    /// Whether the burst read the web content processes' memory; records why
    /// not when it did not.
    private static func webContentReadable(_ burst: Burst) -> Bool {
        guard burst.webContentAt100KB >= 0, burst.webContentAt1000KB >= 0, burst.webContentWarmKB >= 0, burst.webContentClosedKB >= 0 else {
            XCTContext.runActivity(named: "S1(f): web content memory not readable here: \(burst.webContentNote)") { _ in }
            return false
        }
        return true
    }

    // MARK: S2(a), S2(d)

    struct MediaOutcome: Decodable {
        let video: String
        let videoRanged: Int
        let videoLate: Int
        let audio: String
        let audioRanged: Int
        let audioLate: Int
        let badRanges: [String]
    }

    struct Played: Decodable {
        let seekedTo: Double?
        let playedTo: Double?
        let error: String?
    }

    /// S2(a) in the app: a muted H.264 video on the media origin seeks to
    /// 90 % and plays on, through ranged requests answered 206 with the
    /// `Content-Range` asked for. Audio is the next test's.
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
        XCTAssertEqual(outcome.badRanges, [])
    }

    /// S2(a), audio: an 8 MB silent MP3 in `<audio>` on the media origin
    /// seeks to 90 % and plays on, through ranged requests each answered 206
    /// with the `Content-Range` asked for. It needs real hardware: in a
    /// virtual machine no `<audio>` plays (no audio route), and an audio
    /// element left loading there stalled the app for a minute, so the test
    /// skips the test VM's simulators and Mac. Its runs are the iOS device
    /// check and Antoine's run on this Mac (decision record S2).
    func testAudioSeeksToNinetyPercentThroughRangeRequests() throws {
        #if os(macOS) || targetEnvironment(simulator)
        // A simulator shares its Mac's kernel, so this reads the Mac's.
        guard CardSwapTimingGate.hypervisorPresent() == false else {
            throw XCTSkip("not known to run on real hardware (kern.hv_vmm_present): no <audio> plays in a virtual machine")
        }
        #endif
        let app = launch(MeguriLaunch(animations: false, route: "spike/media-seek/audio"))
        let text = text(app.staticTexts["spike.media.result"], notStartingWith: "running", timeout: 120)
        XCTContext.runActivity(named: "S2(a) audio \(text)") { _ in }
        let outcome = try JSONDecoder().decode(MediaOutcome.self, from: Data(text.utf8))
        let audio = try JSONDecoder().decode(Played.self, from: Data(outcome.audio.utf8))
        XCTAssertNil(audio.error, outcome.audio)
        // 8 MB at 128 kbit/s is 524 s: 90 % is about 472 s.
        let seekedTo = try XCTUnwrap(audio.seekedTo), playedTo = try XCTUnwrap(audio.playedTo)
        XCTAssertGreaterThanOrEqual(seekedTo, 470, "seeked to \(seekedTo) s")
        XCTAssertGreaterThan(playedTo, seekedTo + 0.3)
        XCTAssertGreaterThan(outcome.audioRanged, 0, "no ranged request")
        XCTAssertGreaterThan(outcome.audioLate, 0, "no ranged request in the last fifth")
        XCTAssertEqual(outcome.badRanges, [])
    }

    /// S2(d): a value written to localStorage and IndexedDB on the media
    /// origin, in the persistent store with the fixed identifier, is read
    /// back after the app is relaunched (R502).
    func testStorageOnTheMediaOriginSurvivesARelaunch() throws {
        let store = UUID().uuidString
        let arguments = ["-CardRenderingTestStore", store]
        defer {
            XCUIApplication().terminate()
            let cleanup = launch(MeguriLaunch(animations: false, route: "spike/storage/read", extra: arguments + ["-CardRenderingTestCleanup", "YES"]))
            XCTAssertEqual(text(cleanup.staticTexts["spike.storage.result"], notEqualTo: "running", timeout: 30), "removed")
            cleanup.terminate()
        }
        let writer = launch(MeguriLaunch(animations: false, route: "spike/storage/write", extra: arguments))
        let written = text(writer.staticTexts["spike.storage.result"], notEqualTo: "running", timeout: 30)
        let value = try XCTUnwrap(written.wholeMatch(of: /written ([0-9A-F-]{36})/)?.1, written)
        writer.terminate()
        let reader = launch(MeguriLaunch(animations: false, route: "spike/storage/read", extra: arguments))
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

    /// The app's report once `done` accepts it. After `timeout`, the latest
    /// report if `orLatest`, else a failure.
    private func waitForReport(
        _ app: XCUIApplication, timeout: TimeInterval = 30, orLatest: Bool = false, until done: (Report) -> Bool
    ) throws -> Report {
        let element = app.staticTexts["spike.cardSwap.report"]
        let deadline = Date().addingTimeInterval(timeout)
        var last = ""
        var latest: Report?
        repeat {
            last = text(element, notEqualTo: "", timeout: 5)
            if let report = try? JSONDecoder().decode(Report.self, from: Data(last.utf8)) {
                lastReport = last
                latest = report
                if done(report) { return report }
            }
            Thread.sleep(forTimeInterval: 0.5)
        } while Date() < deadline
        if orLatest, let latest { return latest }
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
