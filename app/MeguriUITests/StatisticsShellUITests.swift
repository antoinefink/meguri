import Foundation
import XCTest

/// The Statistics page's shell (plan U296; `stats/statistics-page` S13, S14; contract N25.1,
/// N25.11), behind `StatsRollout`: with the flag off nothing reaches it; with it on the sidebar's
/// Statistics row is second under Today and View › Statistics (⇧⌘S) opens the page; Tab walks the
/// toolbar's two controls, then each box's header and switches; a loading phase never unpins a
/// pinned selection (deferred from the chart system, U295).
@MainActor
final class StatisticsShellUITests: XCTestCase {
    // MARK: Flag

    #if os(macOS)
    func testFlagOffOffersNoStatistics() throws {
        let (app, _) = try launchFixture(rollout: nil)
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 20))
        XCTAssertFalse(element(app, "shell.root.sidebar.stats").exists, "No Statistics row while the flag is off")
        let view = app.menuBars.menuBarItems["View"]
        XCTAssertTrue(view.waitForExistence(timeout: 10))
        XCTAssertFalse(app.menuBars.menuItems["Statistics"].exists, "No View › Statistics while the flag is off")
    }

    func testStatisticsRowIsSecondAndShiftCommandSOpensIt() throws {
        let (app, bundle) = try launchFixture(rollout: "on")
        let today = element(app, "shell.root.sidebar.home")
        let stats = element(app, "shell.root.sidebar.stats")
        let browse = element(app, "shell.root.sidebar.browse")
        XCTAssertTrue(stats.waitForExistence(timeout: 20), "The Statistics row behind the flag")
        XCTAssertLessThan(today.frame.minY, stats.frame.minY, "Statistics under Today (N25.1)")
        XCTAssertLessThan(stats.frame.minY, browse.frame.minY, "Statistics above Browse (N25.1)")
        XCTAssertFalse(element(app, "stats.page.content").exists)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("s", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 30), "⇧⌘S shows the page")
        // The first time on a device: All Decks and All (S3.3).
        XCTAssertEqual(element(app, "stats.toolbar.deck").value as? String, "All Decks")
        let all = element(app, "stats.toolbar.period.all")
        XCTAssertTrue(all.waitForExistence(timeout: 10))
        // A segment reads as selected, or (the Mac's radio button) with the value 1.
        XCTAssertTrue(all.isSelected || (all.value as? Int) == 1 || (all.value as? String) == "1", "All is the first period")
        for section in ["knowledge", "reviews", "cards", "comingUp", "answering", "memory", "decks"] {
            XCTAssertTrue(element(app, "stats.section.\(section)").exists, "Section \(section)")
        }
    }

    /// U295's deferred case: a pinned selection stays pinned while its box shows a loading phase
    /// (the plot loses focus to it), and still shows once the box is ready again.
    func testLoadingPhaseKeepsAPinnedSelection() throws {
        continueAfterFailure = false
        let app = XCUIApplication()
        let bundle = try BuildUnderTest.current().bundleIdentifier
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, windowSize: "large",
                                           route: "stats/page/typical").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let box = element(app, "stats.box.knowledgeOverTime")
        XCTAssertTrue(box.waitForExistence(timeout: 20))
        // Every box filled: the strip's Mature figure has its value.
        XCTAssertTrue(waitForValue(element(app, "stats.strip.mature")))
        try app.bringToFront(bundleIdentifier: bundle)
        // A click in the plot pins the bar under it (S8.3).
        let plot = element(app, "stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(plot.waitForExistence(timeout: 10))
        plot.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.5)).click()
        XCTAssertTrue(pinned(bundle), "The click pinned a selection")
        // The kept results go (as on CollectionReplaced): the box reads again, loading.
        post("\(bundle).testhooks.statsReload")
        let mature = element(app, "stats.strip.mature")
        XCTAssertTrue(XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == ''"), object: mature)],
                                       timeout: 10) == .completed, "The boxes read again: a loading phase")
        XCTAssertTrue(waitForValue(mature), "The box filled again")
        XCTAssertTrue(pinned(bundle), "The selection stayed pinned through the loading phase")
    }

    /// Asks the app whether a box holds a pinned selection (`statsQueryPin`, answered `statsPinned`).
    private func pinned(_ bundle: String) -> Bool {
        let answer = XCTNSNotificationExpectation(name: Notification.Name("\(bundle).testhooks.statsPinned"), object: nil,
                                                  notificationCenter: DistributedNotificationCenter.default())
        post("\(bundle).testhooks.statsQueryPin")
        return XCTWaiter().wait(for: [answer], timeout: 3) == .completed
    }
    #endif

    // MARK: Keyboard

    /// S14: Tab moves through the deck pop-up and the period control, then box by box: the
    /// header, then its switches. XCUITest reports keyboard focus on iOS only (app/AGENTS.md).
    func testTabWalksTheToolbarThenTheBoxes() throws {
        #if os(macOS)
        throw XCTSkip("XCUITest reports keyboard focus on iOS only")
        #else
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("The sidebar page is the iPad's (U304 has the iPhone's)") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "stats/page/typical").arguments
            + ["-MeguriSizeClass", "regular"]
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "stats.box.knowledgeOverTime.header").waitForExistence(timeout: 20))
        var order: [String] = []
        for _ in 0..<6 {
            app.typeKey("\t", modifierFlags: [])
            let focused = app.descendants(matching: .any).matching(NSPredicate(format: "hasFocus == true")).firstMatch
            order.append(focused.exists ? focused.identifier : "")
        }
        let expected = ["stats.toolbar.deck", "stats.toolbar.period", "stats.box.knowledgeOverTime.header",
                        "stats.box.retention.header", "stats.boxSwitch.retentionKind", "stats.box.forgotten.header"]
        let tail = Array(order.drop { $0 != "stats.toolbar.deck" })
        XCTAssertGreaterThanOrEqual(tail.count, 4, "Tab reached the deck pop-up and went on: \(order)")
        XCTAssertEqual(Array(tail.prefix(min(tail.count, expected.count))), Array(expected.prefix(tail.count)), "Tab order \(order)")
        #endif
    }

    // MARK: Support

    #if os(macOS)
    private func launchFixture(rollout: String?) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: rollout.map { ["-MeguriStatsRollout", $0] } ?? []).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.home").waitForExistence(timeout: 30),
                      "Stage the generated local-review-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func post(_ name: String) {
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name(name), object: nil, userInfo: nil, deliverImmediately: true)
    }
    #endif

    private func waitForValue(_ target: XCUIElement) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND value != ''"),
                                                         object: target)], timeout: 30) == .completed
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
}
