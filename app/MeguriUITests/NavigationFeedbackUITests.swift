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
    private let basic = "noteTypes.list.row.1700000100001"
    private let gates = ["home", "deck", "browse", "list", "record", "stats"]

    private func launchApp(route: String = "navigation/feedback", readyElement: String = "shell.root.sidebar.home") throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundleIdentifier = try build.bundleIdentifier
        guard build.hasHooks, bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Navigation feedback requires an isolated development hook build")
        }
        bundle = bundleIdentifier
        app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, windowSize: "large",
            appearance: "light", route: route).arguments
        app.launch()
        XCTAssertTrue(element(readyElement).waitForExistence(timeout: 20))
        try app.bringToFront(bundleIdentifier: bundle)
    }

    private func launchFeedback(holding held: Set<String> = []) throws {
        try launchApp()
        for gate in gates where !held.contains(gate) { try command("release.\(gate)") }
        if !held.contains("home") {
            XCTAssertTrue(element("home.today.studyAll").waitForExistence(timeout: 10))
        }
    }

    func testTodayLoadingScreenshot() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["home"])
        // The first tree is genuinely unknown: neither No Decks nor the welcome flashes.
        try clickSection("browse")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND label CONTAINS '1,204'"), element("browse.status.text")))
        try clickSection("home")
        captureLoading("navigation-loading-today-and-deck-list", label: "Loading Today", title: "Today", section: "home")
        XCTAssertFalse(element("home.sidebar.guidance").exists)
        try command("release.home")
        XCTAssertTrue(element("home.today.studyAll").waitForExistence(timeout: 10))
    }

    func testDeckLoadingScreenshotAndCommit() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["deck"])
        try clickDeck(languages)
        captureLoading("navigation-loading-deck", label: "Loading Deck", title: "Languages", deck: languages)
        try command("release.deck")
        try command("settle.deck")
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 10))
        assertTitle("Languages")
        assertSelected("home.sidebar.deck.\(languages)")
        XCTAssertFalse(element("shell.navigation.pendingBack").exists)
        XCTAssertFalse(progress("Loading Deck").exists)
    }

    func testStatisticsLoadingScreenshot() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["stats"])
        try clickSection("stats")
        captureLoading("navigation-loading-statistics", label: "Loading Statistics", title: "Statistics", section: "stats")
        try command("release.stats")
        try waitForState("filled=true")
        XCTAssertTrue(element("stats.box.knowledgeOverTime.plot").waitForExistence(timeout: 10))
    }

    func testBrowseLoadingScreenshot() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["browse"])
        try clickSection("browse")
        captureLoading("navigation-loading-browse", label: "Searching Browse", title: "Browse", section: "browse")
        XCTAssertEqual(progress("Searching Browse").identifier, "browse.navigation.progress")
        try command("release.browse")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND label CONTAINS '1,204'"), element("browse.status.text")))
    }

    func testNoteTypesListLoadingScreenshot() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["list"])
        try clickSection("noteTypes")
        captureLoading("navigation-loading-note-types-list", label: "Loading Note Types", title: "Note Types", section: "noteTypes")
        XCTAssertFalse(element(basic).exists)
        XCTAssertFalse(element("noteTypes.fields.table").exists)
        try command("release.list")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 10))
        assertTitle("Basic")
        assertSelected(basic)
    }

    func testNoteTypesDetailLoadingScreenshot() throws {
        defer { app?.terminate() }
        try launchFeedback(holding: ["record"])
        try clickSection("noteTypes")
        XCTAssertTrue(element(basic).waitForExistence(timeout: 10))
        // The product selects Basic and names it before its first record arrives.
        captureLoading("navigation-loading-note-types-detail", label: "Loading Note Types", title: "Basic", section: "noteTypes")
        assertSelected(basic)
        XCTAssertFalse(element("noteTypes.fields.table").exists)
        try command("release.record")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 10))
    }

    func testWarmNavigationKeepsLoadedContent() throws {
        defer { app?.terminate() }
        try launchFeedback()
        try clickSection("stats")
        try waitForState("filled=true")
        let knowledge = element("stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(knowledge.waitForExistence(timeout: 10))
        let loadedKnowledge = knowledge.label
        try clickSection("browse")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND label CONTAINS '1,204'"), element("browse.status.text")))
        let browseStatus = element("browse.status.text").label
        try clickSection("noteTypes")
        XCTAssertTrue(element("noteTypes.fields.table").waitForExistence(timeout: 10))

        // Keep loaded content while refreshes are explicitly held. A repeat visit
        // must not re-open Browse's session or reread Note Types' names/records.
        let before = try command("query")
        try command("hold.list")
        try command("refresh.list")
        XCTAssertTrue(element(basic).exists)
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

    func testPendingDeckBackRestoresPreviousPageAndIgnoresCompletion() throws {
        defer { app?.terminate() }
        try launchFeedback()
        try clickDeck(languages)
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 10))
        try command("hold.deck")
        try clickDeck(science)
        captureLoading("navigation-loading-deck-before-back", label: "Loading Deck", title: "Science", deck: science)
        let back = element("shell.navigation.pendingBack")
        XCTAssertTrue(back.isEnabled && back.isHittable)
        back.click()
        assertTitle("Languages")
        assertSelected("home.sidebar.deck.\(languages)")
        XCTAssertTrue(element("home.deck.page.\(languages)").exists)
        try command("release.deck")
        try command("settle.deck")
        assertTitle("Languages")
        XCTAssertFalse(element("home.deck.page.\(science)").exists)

        // The same Back action remains available through its native shortcut.
        try command("hold.deck")
        try clickDeck(science)
        XCTAssertTrue(progress("Loading Deck").waitForExistence(timeout: 5))
        app.typeKey("[", modifierFlags: .command)
        assertTitle("Languages")
        try command("release.deck")
        try command("settle.deck")
        assertSelected("home.sidebar.deck.\(languages)")
    }

    func testFailedDeckSaveRestoresPreviousPageAndCanBeRetried() throws {
        defer { app?.terminate() }
        try launchFeedback()
        try clickDeck(languages)
        XCTAssertTrue(element("home.deck.page.\(languages)").waitForExistence(timeout: 10))
        try command("hold.deck")
        try command("fail.deck")
        try clickDeck(science)
        captureLoading("navigation-loading-deck-before-failure", label: "Loading Deck", title: "Science", deck: science)
        try command("release.deck")
        try command("settle.deck")
        assertTitle("Languages")
        assertSelected("home.sidebar.deck.\(languages)")
        XCTAssertTrue(element("home.deck.page.\(languages)").exists)
        XCTAssertFalse(element("shell.navigation.pendingBack").exists)
        XCTAssertFalse(progress("Loading Deck").exists)
        XCTAssertFalse(element("home.deck.page.\(science)").exists)
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND (label == %@ OR value == %@)",
            "Couldn’t select this deck. Try again.", "Couldn’t select this deck. Try again."), element("home.collection.error")))
        try command("succeed.deck")
        try clickDeck(science)
        try command("settle.deck")
        XCTAssertTrue(element("home.deck.page.\(science)").waitForExistence(timeout: 10))
        assertTitle("Science")
        assertSelected("home.sidebar.deck.\(science)")
        XCTAssertFalse(element("home.collection.error").exists)
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
        try command("settle.deck")
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
        try command("settle.deck")
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
        assertTitle("Statistics – 4,210 cards in 7 decks · Updating…", exact: true)
        assertSelected("shell.root.sidebar.stats")

        // This existing route holds the production Knowledge plot at 40%; it
        // never releases data on a timer. Scroll the native page if needed.
        let plot = element("stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(plot.waitForExistence(timeout: 10))
        let indicator = plot.descendants(matching: .progressIndicator).firstMatch
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND value == 0.4"), indicator))
        let viewport = element("stats.page.content")
        // The scroll view's AX frame can span the whole document. Clip to the
        // actual window; neither the document nor an inert plot must be hittable.
        let visible = app.windows.firstMatch.frame.intersection(viewport.frame)
        XCTAssertFalse(visible.isEmpty)
        if plot.frame.maxY > visible.maxY {
            viewport.scroll(byDeltaX: 0, deltaY: -(plot.frame.maxY - visible.maxY))
        }
        XCTAssertTrue(wait(NSPredicate { [self] _, _ in
            let visible = app.windows.firstMatch.frame.intersection(viewport.frame)
            return !plot.frame.isEmpty && visible.contains(plot.frame)
                && !indicator.frame.isEmpty && plot.frame.contains(indicator.frame)
        }, plot), "The actual plot and its 40% indicator are fully on screen")
        XCTAssertTrue(wait(NSPredicate(format: "exists == true AND value == 0.4"), indicator))
        assertTitle("Statistics – 4,210 cards in 7 decks · Updating…", exact: true)
        assertSelected("shell.root.sidebar.stats")
        captureWindow("navigation-loading-statistics-progress")
    }

    private func element(_ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func progress(_ label: String) -> XCUIElement {
        app.activityIndicators.matching(NSPredicate(format: "label == %@", label)).firstMatch
    }
    private func clickSection(_ section: String) throws {
        let row = element("shell.root.sidebar.\(section)")
        XCTAssertTrue(row.waitForExistence(timeout: 3) && row.isHittable)
        row.click()
    }
    private func clickDeck(_ id: Int64) throws {
        let identifier = "home.sidebar.deck.\(id)"
        XCTAssertTrue(element(identifier).waitForExistence(timeout: 3))
        // Parent deck labels are static text or Other, not native buttons.
        let text = app.staticTexts.matching(identifier: identifier).firstMatch
        let row = text.exists ? text : app.otherElements.matching(identifier: identifier).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 3) && row.isHittable)
        row.click()
    }
    private func assertTitle(_ title: String, exact: Bool = false) {
        // macOS puts title + subtitle on XCUIWindow, sometimes as label,
        // sometimes as title. Visible title text can instead expose its value.
        let fullTitle = NSPredicate(format: "label == %@ OR title == %@", title, title)
        let windowTitle = exact ? fullTitle : NSCompoundPredicate(orPredicateWithSubpredicates: [fullTitle,
            NSPredicate(format: "label BEGINSWITH %@ OR title BEGINSWITH %@", title + " – ", title + " – ")])
        let textTitle = NSPredicate(format: "value == %@", title)
        XCTAssertTrue(wait(NSPredicate { [self] _, _ in
            if app.windows.matching(windowTitle).firstMatch.exists { return true }
            // A matching sidebar/list label cannot witness the window title.
            // The native title text sits above the first navigation row.
            let titleBottom = element("shell.root.sidebar.home").frame.minY
            return app.windows.firstMatch.staticTexts.matching(textTitle).allElementsBoundByIndex.contains {
                onScreen($0) && $0.frame.maxY <= titleBottom
            }
        }, app.windows.firstMatch), "Native title: \(title)")
    }
    private func assertSelected(_ id: String) {
        let control = element(id)
        let row = app.outlineRows.containing(.any, identifier: id).firstMatch
        XCTAssertTrue(wait(NSPredicate { _, _ in
            (control.exists && control.isSelected) || (row.exists && row.isSelected)
        }, control), "Native destination selection: \(id)")
    }
    private func onScreen(_ element: XCUIElement) -> Bool {
        element.exists && !element.frame.isEmpty && app.windows.firstMatch.frame.contains(element.frame)
    }
    private func captureLoading(_ name: String, label: String, title: String, section: String? = nil, deck: Int64? = nil) {
        assertTitle(title)
        let indicator = progress(label)
        XCTAssertTrue(indicator.waitForExistence(timeout: 5), "Actual unknown wait has native feedback: \(label)")
        XCTAssertEqual(indicator.label, label)
        XCTAssertTrue(onScreen(indicator), "The labelled native activity indicator is on screen")
        let id = section.map { "shell.root.sidebar.\($0)" } ?? "home.sidebar.deck.\(deck!)"
        assertSelected(id)
        XCTAssertTrue(onScreen(element(id)), "The selected destination label is on screen")
        XCTAssertTrue(element("shell.root.sidebar.home").isHittable, "Other navigation stays usable")
        captureWindow(name)
    }
    private func captureWindow(_ name: String) {
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
