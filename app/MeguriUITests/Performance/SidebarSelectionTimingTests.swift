import Foundation
import XCTest

/// Mac sidebar selection on the 150k fixture: from a row's mouse-down to the
/// window frame that first shows it selected, and to the destination's first
/// frame. The app posts each press to itself at its time
/// (`TestHooks.SidebarSelectionTiming`), so a press made while the main thread
/// is busy waits in the queue as a person's would; ScreenCaptureKit gives the
/// display time of every frame of the app's own window. Each run's numbers are
/// attached (`sidebar-timing-*`) and printed (`meguri.sidebarTiming`).
///
/// A harness check in `tools/mg perf-ui` (`MG_PERF_UI_CHECK_SUITES`): it
/// writes no XCTest metrics, and it needs Profile and the 150k collection.
final class SidebarSelectionTimingTests: PerfTestCase {
    override nonisolated class var fixture: String? { "150k" }

    #if os(macOS)
    private var app: XCUIApplication!
    private var bundle = ""
    private var report: [String] = []
    private var points: [String: String] = [:]

    func testSidebarSelectionTimings() throws {
        let build = try BuildUnderTest.current()
        bundle = build.expectedBundleIdentifier
        let fixture = try BrowserSearchPerfTests.fixture150k(build.repositoryRoot)
        app = launch(MeguriLaunch(fixture: fixture, windowSize: "large", extra: ["-MeguriStatsRollout", "on"]))
        // Every run reports, whichever assertion fails first.
        continueAfterFailure = true
        // The 150k collection keeps the main thread busy for a while after launch.
        waitUntilResponsive()
        let any = app.descendants(matching: .any)
        XCTAssert(any["shell.root.sidebar.stats"].waitForExistence(timeout: 180), "the shell did not show its Statistics row")
        let decks = any.matching(NSPredicate(format: "identifier BEGINSWITH 'home.sidebar.deck.'"))
        XCTAssert(decks.firstMatch.waitForExistence(timeout: 180), "the sidebar showed no deck rows")
        let deckRows = decks.allElementsBoundByIndex.prefix(2).map(\.identifier)
        guard deckRows.count == 2 else {
            XCTFail("the 150k fixture shows two decks, found \(deckRows.count)")
            return
        }
        let deck = deckRows.map { "deck:" + $0.replacingOccurrences(of: "home.sidebar.deck.", with: "") }
        // Every row's place, read once while the app is idle: later queries can
        // wait on a busy app, and the rows do not move (Browse's sidebar keeps
        // the section rows where the main list has them; the app checks a row
        // is under each point).
        for identifier in ["shell.root.sidebar.home", "shell.root.sidebar.stats", "shell.root.sidebar.browse",
                           "shell.root.sidebar.noteTypes"] + deckRows {
            let element = any[identifier].firstMatch
            XCTAssert(element.waitForExistence(timeout: 30), "no sidebar row \(identifier)")
            points[identifier] = "\(element.frame.midX),\(element.frame.midY)"
        }
        try app.bringToFront(bundleIdentifier: bundle)
        pause(3)

        // Cold: each destination's first visit, into and out of Browse.
        click("shell.root.sidebar.stats", "cold today>stats", expect: "stats")
        click("shell.root.sidebar.browse", "cold stats>browse", expect: "browse")
        click("shell.root.sidebar.noteTypes", "cold browse>noteTypes", expect: "noteTypes")
        click(deckRows[0], "cold noteTypes>deck1", expect: deck[0])
        click(deckRows[1], "cold deck1>deck2", expect: deck[1])
        click("shell.root.sidebar.home", "cold deck2>today", expect: "home")
        // Warm: the same destinations again.
        click("shell.root.sidebar.stats", "warm today>stats", expect: "stats")
        click("shell.root.sidebar.browse", "warm stats>browse", expect: "browse")
        click("shell.root.sidebar.home", "warm browse>today", expect: "home")
        click("shell.root.sidebar.noteTypes", "warm today>noteTypes", expect: "noteTypes")
        click("shell.root.sidebar.browse", "warm noteTypes>browse", expect: "browse")
        click("shell.root.sidebar.stats", "warm browse>stats", expect: "stats")
        click(deckRows[0], "warm stats>deck1", expect: deck[0])
        // A held press: the row is selected while the button is still down.
        click("shell.root.sidebar.home", "hold deck1>today", hold: 400, expect: "home")
        // Presses while the previous destination still loads.
        sequence([("shell.root.sidebar.stats", "rapid today>stats"), ("shell.root.sidebar.noteTypes", "rapid >noteTypes"),
                  (deckRows[1], "rapid >deck2")], expect: deck[1])
        sequence([("shell.root.sidebar.browse", "rapid deck2>browse"), ("shell.root.sidebar.home", "rapid >today")], expect: "home")

        let summary = XCTAttachment(string: report.joined(separator: "\n"))
        summary.name = "sidebar-timing-summary"
        summary.lifetime = .keepAlways
        add(summary)
        print("meguri.sidebarTiming summary\n" + report.joined(separator: "\n"))
    }

    // MARK: Support

    /// Until the app's main thread has answered ten pings in a row, a second
    /// apart, each within half a second, and at least `settle` seconds passed.
    private func waitUntilResponsive(settle: TimeInterval = 30, timeout: TimeInterval = 900) {
        let start = Date()
        let clock = ContinuousClock()
        let deadline = clock.now + .seconds(timeout)
        func left() -> TimeInterval {
            let remaining = clock.now.duration(to: deadline).components
            return max(0, Double(remaining.seconds) + Double(remaining.attoseconds) / 1e18)
        }
        var quick = 0
        var slowest: TimeInterval = 0
        let spans = ResultText()
        while quick < 10 || Date().timeIntervalSince(start) < settle, left() > 0 {
            let sent = Date()
            let pong = XCTNSNotificationExpectation(name: Notification.Name("\(bundle).testhooks.sidebarTimingPong"), object: nil,
                                                    notificationCenter: DistributedNotificationCenter.default())
            pong.handler = { notification in
                spans.value = notification.object as? String ?? ""
                return true
            }
            DistributedNotificationCenter.default().postNotificationName(
                Notification.Name("\(bundle).testhooks.sidebarTimingPing"), object: nil, userInfo: nil, deliverImmediately: true)
            let answered = XCTWaiter().wait(for: [pong], timeout: min(300, max(0.1, left()))) == .completed
            let took = Date().timeIntervalSince(sent)
            slowest = max(slowest, took)
            quick = answered && took < 0.5 ? quick + 1 : 0
            pause(min(1, left()))
        }
        let line = "launch: responsive after \(Int(Date().timeIntervalSince(start))) s (slowest ping \(Int(slowest * 1000)) ms;"
            + " main-thread spans of 0.5 s or more [s after launch, duration]: \(spans.value))"
        report.append(line)
        print("meguri.sidebarTiming " + line)
        XCTAssertGreaterThanOrEqual(quick, 10, "the app's main thread stayed busy")
    }

    private func pause(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    /// The centre of a sidebar row, in screen points from the top left.
    private func point(_ identifier: String) -> String {
        guard let point = points[identifier] else {
            XCTFail("no place for \(identifier)")
            return "0,0"
        }
        return point
    }

    private func click(_ identifier: String, _ label: String, hold: Int = 60, expect: String) {
        let result = run(["\(point(identifier)),\(hold),0,\(label)"], label: label)
        check(result, expect: expect)
        guard let step = (result["steps"] as? [[String: Any]])?.first else {
            XCTFail("\(label): the run reported no step")
            return
        }
        XCTAssertNotNil(step["rowMs"] as? Double, "\(label): no captured frame showed the row selected")
        XCTAssertEqual(step["commit"] as? String, expect, "\(label): the press's own route was not shown")
        XCTAssertEqual(step["missingSamples"] as? Int, 0, "\(label): frames did not cover the row or the detail column")
        // The row is selected in the list, after Core Animation's commit, within two
        // display frames of its mouse-down: the press neither waits for its release
        // nor redraws anything but the sidebar. (Pixels in the test VM's capture
        // trail this by its compositing delay; `rowMs` reports them.)
        let selected = step["rowStateMs"] as? Double
        XCTAssertNotNil(selected, "\(label): the row was never selected")
        if let selected {
            XCTAssertLessThanOrEqual(selected, 50, "\(label): the row was selected \(selected) ms after its mouse-down")
            if hold >= 200 {
                XCTAssertLessThan(selected, step["holdMs"] as? Double ?? 0, "\(label): the row waited for the mouse-up")
            }
        }
        // The row stays selected until its route is shown (the list may be replaced then).
        if let lost = step["rowLostMs"] as? Double {
            XCTAssertGreaterThanOrEqual(lost, step["commitMs"] as? Double ?? .infinity, "\(label): the row lost its selection before its route changed")
        }
        // Nothing of the outgoing page changes before the route does: no dimming,
        // no relayout of its columns.
        // At least the row's own frame precedes the commit: no frame is no evidence.
        XCTAssertGreaterThan(step["framesBeforeCommit"] as? Int ?? 0, 0, "\(label): no displayed frame before the route changed")
        XCTAssertEqual(step["detailBeforeCommitFrames"] as? Int, 0, "\(label): the outgoing page changed before the new one")
    }

    /// Presses each row 40 ms after the previous release, whatever the app is doing.
    private func sequence(_ rows: [(String, String)], expect: String) {
        let steps = rows.map { "\(point($0.0)),50,40,\($0.1)" }
        let result = run(steps, label: rows.map(\.1).joined(separator: " "))
        check(result, expect: expect)
    }

    private func check(_ result: [String: Any], expect: String) {
        XCTAssertNil(result["error"], "the timing run failed: \(result["error"] ?? "")")
        // Timings need frames with a display time; a run without them measured nothing.
        XCTAssertGreaterThan(result["displayedFrames"] as? Int ?? 0, 0, "the run captured no displayed frame")
        let commits = (result["changes"] as? [[String: Any]] ?? []).filter { $0["kind"] as? String == "selection" }
        XCTAssertEqual(commits.last?["value"] as? String, expect, "the run ended on another route")
    }

    private func run(_ steps: [String], label: String) -> [String: Any] {
        pause(2)
        let text = ResultText()
        let done = expectation(forNotification: Notification.Name("\(bundle).testhooks.sidebarTimingResult"), object: nil,
                               notificationCenter: DistributedNotificationCenter.default()) { notification in
            text.value = notification.object as? String ?? ""
            return true
        }
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name("\(bundle).testhooks.sidebarTimingRun"), object: steps.joined(separator: "|"), userInfo: nil,
            deliverImmediately: true)
        wait(for: [done], timeout: 90)
        let attachment = XCTAttachment(string: text.value)
        attachment.name = "sidebar-timing-\(label)"
        attachment.lifetime = .keepAlways
        add(attachment)
        let object = (try? JSONSerialization.jsonObject(with: Data(text.value.utf8))) as? [String: Any] ?? ["error": "unreadable: \(text.value)"]
        for step in object["steps"] as? [[String: Any]] ?? [] {
            let line = ["label", "holdMs", "rowStateMs", "rowLostMs", "rowMs", "rowMax", "commitMs", "contentMs", "lastChangeMs", "detailBeforeCommitMax",
                        "detailBeforeCommitFrames", "downDeliveredMs", "upDeliveredMs", "busy"]
                .map { "\($0)=\(step[$0].map { "\($0)" } ?? "-")" }.joined(separator: " ")
            report.append(line)
        }
        print("meguri.sidebarTiming \(label): \(text.value)")
        return object
    }
    #else
    /// The probe is the Mac's: elsewhere the suite must not pass by running nothing.
    func testSidebarSelectionTimings() throws {
        XCTFail("SidebarSelectionTimingTests measures the Mac sidebar; run it with --platform mac")
    }
    #endif
}

/// The text a notification handler hands back.
private final class ResultText: @unchecked Sendable {
    var value = ""
}
