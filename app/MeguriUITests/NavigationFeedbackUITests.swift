#if os(macOS)
import Foundation
import XCTest

/// Run in the macOS test VM. Every image is a window screenshot reached by
/// production sidebar navigation, with an explicitly held synthetic read.
@MainActor final class NavigationFeedbackUITests: XCTestCase {
    private var app: XCUIApplication!
    private var bundle = ""
    private let languages: Int64 = 1_700_000_000_101
    private let science: Int64 = 1_700_000_000_104

    private func launchApp(route: String = "navigation/feedback", readyElement: String = "shell.root.sidebar.home") throws {
        continueAfterFailure = false
        bundle = try BuildUnderTest.current().bundleIdentifier
        app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, windowSize: "large",
            appearance: "light", route: route).arguments
        app.launch()
        XCTAssertTrue(element(readyElement).waitForExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
    }

    func testFirstAndWarmNavigationFeedback() throws {
        defer { app?.terminate() }
        try launchApp()
        // The first tree is genuinely unknown: neither No Decks nor the welcome flashes.
        // Browse stays held too, preserving its later cold capture.
        try clickSection("browse")
        XCTAssertTrue(progress("Searching Browse").waitForExistence(timeout: 5))
        try clickSection("home")
        captureLoading("navigation-loading-today-and-deck-list", label: "Loading Today", title: "Today", section: "home")
        XCTAssertFalse(element("home.sidebar.guidance").exists)
        try command("release.home")
        XCTAssertTrue(element("home.today.studyAll").waitForExistence(timeout: 10))

        try clickDeck(languages)
        captureLoading("navigation-loading-deck", label: "Loading Deck", title: "Languages", deck: languages)
        try command("release.deck")
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 10))

        try clickSection("stats")
        captureLoading("navigation-loading-statistics", label: "Loading Statistics", title: "Statistics", section: "stats")
        try command("release.stats")
        try waitForState("filled=true")
        let knowledge = element("stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(knowledge.waitForExistence(timeout: 10))
        let loadedKnowledge = knowledge.label

        try clickSection("browse")
        captureLoading("navigation-loading-browse", label: "Searching Browse", title: "Browse", section: "browse")
        try command("release.browse")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND label CONTAINS '1,204'"), element("browse.status.text")))
        let browseStatus = element("browse.status.text").label

        try clickSection("noteTypes")
        captureLoading("navigation-loading-note-types-list", label: "Loading Note Types", title: "Note Types", section: "noteTypes")
        XCTAssertFalse(element("noteTypes.list.row.1700000100001").exists)
        XCTAssertFalse(element("noteTypes.fields.table").exists)
        try command("release.list")
        XCTAssertTrue(element("noteTypes.list.row.1700000100001").waitForExistence(timeout: 10))
        captureLoading("navigation-loading-note-types-detail", label: "Loading Note Types", title: "Basic", section: "noteTypes")
        try command("release.record")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 10))

        // Keep loaded content while refreshes are explicitly held. A repeat visit
        // must not re-open Browse's session or reread Note Types' names/records.
        let before = try command("query")
        try command("hold.list")
        try command("refresh.list")
        XCTAssertTrue(element("noteTypes.list.row.1700000100001").exists)
        try clickSection("home")
        try command("hold.home")
        try command("refresh.home")
        XCTAssertTrue(element("home.today.studyAll").exists)
        assertNoProgress("Loading Today")
        try command("hold.stats")
        try clickSection("stats")
        try waitForState("filled=false")
        XCTAssertTrue(knowledge.waitForExistence(timeout: 5))
        XCTAssertEqual(knowledge.label, loadedKnowledge, "Kept chart remains during an unfinished refresh")
        assertNoProgress("Loading Statistics")
        try clickSection("browse")
        XCTAssertEqual(element("browse.status.text").label, browseStatus)
        assertNoProgress("Searching Browse")
        try clickSection("noteTypes")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 5))
        assertNoProgress("Loading Note Types")
        let after = try command("query")
        for key in ["browse", "record"] {
            XCTAssertEqual(value(key, in: before), value(key, in: after), "Warm navigation reuses \(key)")
        }
        try command("release.list")
        try command("release.home")
        try command("release.stats")
    }

    func testRapidSwitchingAndLeavingPendingRoutes() throws {
        defer { app?.terminate() }
        try launchApp()
        try command("release.home")
        XCTAssertTrue(element("home.today.studyAll").waitForExistence(timeout: 10))
        // A/B/A before either save finishes: selection follows each click;
        // a completion from B cannot replace A or pull us back after departure.
        try clickDeck(languages)
        XCTAssertTrue(element("home.collection.pending").waitForExistence(timeout: 3))
        try clickDeck(science)
        assertTitle("Science")
        try clickDeck(languages)
        captureLoading("navigation-loading-deck-rapid-return", label: "Loading Deck", title: "Languages", deck: languages)
        try clickSection("home")
        try command("release.deck")
        try waitForState("pending=0")
        XCTAssertTrue(element("home.today.studyAll").exists)
        XCTAssertFalse(element("home.deck.page.\(languages)").exists)

        try clickDeck(languages)
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 10))
        // A loaded A, pending B, then A: no empty-page flash for loaded A.
        try command("hold.deck")
        try clickDeck(science)
        XCTAssertTrue(element("home.collection.pending").waitForExistence(timeout: 3))
        try clickDeck(languages)
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 3))
        XCTAssertFalse(progress("Loading Deck").exists)
        try clickSection("stats")
        XCTAssertTrue(progress("Loading Statistics").waitForExistence(timeout: 5))
        try clickSection("browse")
        XCTAssertTrue(progress("Searching Browse").waitForExistence(timeout: 5))
        try clickSection("noteTypes")
        XCTAssertTrue(progress("Loading Note Types").waitForExistence(timeout: 5))
        try command("release.list")
        XCTAssertTrue(element("noteTypes.list.row.1700000100001").waitForExistence(timeout: 5))
        XCTAssertTrue(progress("Loading Note Types").waitForExistence(timeout: 5))
        XCTAssertFalse(element("noteTypes.fields.table").exists)
        try clickSection("home")
        for key in ["deck", "stats", "browse", "list", "record"] { try command("release.\(key)") }
        XCTAssertTrue(element("home.today.studyAll").waitForExistence(timeout: 5))
        try waitForState("section=home")
        XCTAssertEqual(value("pending", in: try command("query")), "0")
        // Returning to routes whose read completed while hidden keeps or finishes
        // their loaded state; an older task never selects its own destination.
        try clickSection("noteTypes")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 10))
        try clickSection("browse")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND label CONTAINS '1,204'"), element("browse.status.text")))
        try clickSection("stats")
        try waitForState("filled=true")
        XCTAssertFalse(progress("Loading Statistics").exists)
    }

    func testDeterminateStatisticsLoadingProgress() throws {
        defer { app?.terminate() }
        try launchApp(route: "stats/page/loading-knowledge", readyElement: "stats.page.content")
        assertTitle("Statistics")
        XCTAssertTrue(element("shell.root.sidebar.stats").isSelected)

        // This existing route holds the production Knowledge plot at 40%; it
        // never releases data on a timer. Scroll the native page if needed.
        let plot = element("stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(plot.waitForExistence(timeout: 10))
        let indicator = plot.descendants(matching: .progressIndicator).firstMatch
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND (value == %@ OR value == 0.4 OR value == 40)", "40%"), indicator))
        let viewport = element("stats.page.content")
        if plot.frame.maxY > viewport.frame.maxY {
            viewport.scroll(byDeltaX: 0, deltaY: -(plot.frame.maxY - viewport.frame.maxY + 8))
        }
        XCTAssertTrue(plot.isHittable && viewport.frame.contains(plot.frame), "The entire loading plot is in view")
        XCTAssertTrue(indicator.isHittable)
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "navigation-loading-statistics-progress"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func element(_ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func progress(_ label: String) -> XCUIElement {
        app.progressIndicators.matching(NSPredicate(format: "label == %@", label)).firstMatch
    }
    private func clickSection(_ section: String) throws {
        let row = element("shell.root.sidebar.\(section)")
        XCTAssertTrue(row.waitForExistence(timeout: 3) && row.isHittable)
        row.click()
    }
    private func clickDeck(_ id: Int64) throws {
        let row = element("home.sidebar.deck.\(id)")
        XCTAssertTrue(row.waitForExistence(timeout: 3) && row.isHittable)
        row.click()
    }
    private func assertTitle(_ title: String) {
        let heading = app.toolbars.staticTexts.matching(NSPredicate(format: "label == %@ OR title == %@", title, title)).firstMatch
        XCTAssertTrue(heading.waitForExistence(timeout: 3), "Native title: \(title)")
    }
    private func captureLoading(_ name: String, label: String, title: String, section: String? = nil, deck: Int64? = nil) {
        assertTitle(title)
        XCTAssertTrue(progress(label).waitForExistence(timeout: 5), "Actual unknown wait has native feedback")
        let id = section.map { "shell.root.sidebar.\($0)" } ?? "home.sidebar.deck.\(deck!)"
        XCTAssertTrue(element(id).isSelected, "Destination is selected: \(id)")
        XCTAssertTrue(element("shell.root.sidebar.home").isHittable, "Other navigation stays usable")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
    private func assertNoProgress(_ label: String) {
        let appeared = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true"), object: progress(label))
        appeared.isInverted = true
        XCTAssertEqual(XCTWaiter().wait(for: [appeared], timeout: 0.8), .completed,
            "Fast/kept navigation never flashes progress: \(label)")
    }
    private func wait(_ predicate: NSPredicate, _ element: XCUIElement) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 10) == .completed
    }
    @discardableResult private func command(_ command: String) throws -> String {
        final class Reply: @unchecked Sendable { var state = "" }
        let reply = Reply()
        let center = DistributedNotificationCenter.default()
        let ack = XCTNSNotificationExpectation(name: Notification.Name("\(bundle).testhooks.navigation.ack"),
            object: nil, notificationCenter: center)
        ack.handler = { notification in
            guard let text = notification.object as? String, text.hasPrefix(command + "|") else { return false }
            reply.state = String(text.dropFirst(command.count + 1))
            return true
        }
        center.postNotificationName(Notification.Name("\(bundle).testhooks.navigation.command"), object: command,
            userInfo: nil, deliverImmediately: true)
        XCTAssertEqual(XCTWaiter().wait(for: [ack], timeout: 5), .completed, "Hook acknowledged \(command)")
        return reply.state
    }
    private func value(_ key: String, in state: String) -> String? {
        state.split(separator: ",").first { $0.hasPrefix(key + "=") }.map { String($0.dropFirst(key.count + 1)) }
    }
    private func waitForState(_ expected: String) throws {
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            if try command("query").split(separator: ",").contains(Substring(expected)) { return }
            // Yield the XCTest run loop; no product timing controls or retries.
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        XCTFail("State never reached \(expected)")
    }
}
#endif
