import Foundation
import XCTest

/// The narrow Statistics page (plan U304; `stats/statistics-page` S5, S6, S8.3, S12.1, S13;
/// contract N25.1, N25.2, N25.6) on the iPhone, over the sample collection's routes: the tab
/// between Decks and Settings (Browse has no compact layout yet), no tab on first run, the inline
/// title with the deck pop-up, the pinned period, the Jump menu, the four strip tiles, Count and
/// Time in Reviews Over Time's menu, a short hold that selects while a swipe still scrolls, and
/// the no-collection state with its controls disabled; then the launch option that opens the
/// page over a generated collection.
@MainActor
final class StatisticsCompactUITests: XCTestCase {
    override func setUpWithError() throws {
        #if os(macOS)
        throw XCTSkip("The narrow page is the iPhone's and the narrow iPad's")
        #else
        guard UIDevice.current.userInterfaceIdiom == .phone else { throw XCTSkip("Run on an iPhone (N25.6)") }
        // The sample routes are the test hooks' (Debug and Profile only).
        guard try BuildUnderTest.current().hasHooks else { throw XCTSkip("The stats/… routes need a build with test hooks") }
        continueAfterFailure = false
        #endif
    }

    #if os(iOS)
    // MARK: The tab (N25.1, S13)

    func testTheStatisticsTabSitsAfterDecksAndOpensThePage() throws {
        let app = launch(route: "stats/page/typical")
        let tabBar = app.tabBars.firstMatch
        XCTAssertTrue(tabBar.waitForExistence(timeout: 20), "No tab bar")
        // The tab bar's own buttons carry no identifier on iOS 27; the elements that do are read
        // in their order across the bar.
        let tabs = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'shell.root.tab.'"))
        func ordered() -> [XCUIElement] { tabs.allElementsBoundByIndex.sorted { $0.frame.minX < $1.frame.minX } }
        let identifiers = ["decks", "stats", "settings"].map { "shell.root.tab.\($0)" }
        XCTAssertTrue(waitUntil { ordered().map(\.identifier) == identifiers },
                      "Decks, Statistics, Settings (Browse has no compact layout): \(ordered().map(\.identifier))")
        XCTAssertEqual(ordered().map(\.label), ["Decks", "Statistics", "Settings"])
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20), "The route reaches the page on the iPhone")
        XCTAssertTrue(element(app, "shell.root.tab.stats").isSelected, "The Statistics tab is selected")
        // A second tab and back keeps the page (its own stack).
        element(app, "shell.root.tab.decks").tap()
        element(app, "shell.root.tab.stats").tap()
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 10))
    }

    func testFirstRunOffersNoStatisticsTab() throws {
        let app = launch(route: "stats/page/first-run")
        let tabBar = app.tabBars.firstMatch
        XCTAssertTrue(tabBar.waitForExistence(timeout: 20), "No tab bar")
        XCTAssertTrue(waitUntil { tabBar.buttons.count == 2 }, "\(tabBar.buttons.allElementsBoundByIndex.map(\.identifier))")
        XCTAssertFalse(element(app, "shell.root.tab.stats").exists, "No Statistics tab with no collection on the device")
        XCTAssertFalse(element(app, "stats.page.content").exists)
        XCTAssertTrue(element(app, "shell.root.tab.decks").isSelected, "The shell keeps Decks")
    }

    // MARK: The bar (N25.2)

    func testTheBarHasTheDeckPopUpThePinnedPeriodAndTheJumpMenu() throws {
        let app = launch(route: "stats/page/typical")
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20))
        let deck = element(app, "stats.toolbar.deck")
        let period = element(app, "stats.toolbar.period")
        let jump = element(app, "stats.toolbar.jump")
        XCTAssertTrue(deck.waitForExistence(timeout: 10) && period.exists && jump.exists, "The deck pop-up, the period and the Jump menu")
        XCTAssertEqual(deck.value as? String, "All Decks", "The first time: All Decks (S3.3)")
        XCTAssertTrue(element(app, "stats.toolbar.period.all").isSelected, "The first time: All (S3.3)")
        let navigationBar = app.navigationBars.firstMatch
        XCTAssertLessThan(abs(deck.frame.midY - navigationBar.frame.midY), navigationBar.frame.height, "The deck pop-up is in the bar")
        let top = period.frame.minY
        // The period stays under the bar while the page scrolls (N25.2).
        element(app, "stats.page.content").swipeUp(velocity: .fast)
        element(app, "stats.page.content").swipeUp(velocity: .fast)
        XCTAssertTrue(period.isHittable, "The period control is still on screen after scrolling")
        XCTAssertEqual(period.frame.minY, top, accuracy: 2, "The period control is pinned")
        XCTAssertTrue(deck.isHittable, "The deck's name shows at every scroll position")
        // Choosing a period works from the pinned control.
        element(app, "stats.toolbar.period.1m").tap()
        XCTAssertTrue(waitUntil { self.element(app, "stats.toolbar.period.1m").isSelected }, "1M chosen")
    }

    func testTheJumpMenuScrollsASectionsHeadingToTheTop() throws {
        let app = launch(route: "stats/page/typical")
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20))
        let heading = element(app, "stats.section.memory.heading")
        element(app, "stats.toolbar.jump").tap()
        let item = element(app, "stats.toolbar.jump.memory")
        XCTAssertTrue(item.waitForExistence(timeout: 5), "The Jump menu lists the sections")
        for section in ["knowledge", "reviews", "cards", "comingUp", "answering", "decks"] {
            XCTAssertTrue(element(app, "stats.toolbar.jump.\(section)").exists, "Jump menu item \(section)")
        }
        item.tap()
        let period = element(app, "stats.toolbar.period")
        XCTAssertTrue(waitUntil { heading.exists && heading.isHittable && abs(heading.frame.minY - period.frame.maxY) < 60 },
                      "Memory's heading at the top, under the period: \(heading.frame) under \(period.frame)")
    }

    // MARK: The page (S6, N25.6)

    func testTheStripHasFourTilesAndCountAndTimeAreInTheBoxsMenu() throws {
        let app = launch(route: "stats/page/typical")
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20))
        let tiles = ["studiedToday", "retentionToday", "remainingToday", "streak"].map { element(app, "stats.strip.\($0)") }
        XCTAssertTrue(tiles[0].waitForExistence(timeout: 10))
        XCTAssertTrue(tiles.allSatisfy(\.exists), "Four tiles")
        XCTAssertFalse(element(app, "stats.strip.mature").exists, "Mature moves to Card Counts (S6)")
        let rows = Set(tiles.map { Int($0.frame.minY.rounded()) })
        XCTAssertEqual(rows.count, 1, "Four in a row at the default text size: \(tiles.map(\.frame))")
        // Reviews Over Time: By Type | By Answer in the header, Count | Time in the box's menu.
        element(app, "stats.toolbar.jump").tap()
        element(app, "stats.toolbar.jump.reviews").tap()
        let menu = element(app, "stats.box.reviewsOverTime.menu")
        XCTAssertTrue(menu.waitForExistence(timeout: 10), "The box's menu")
        XCTAssertTrue(element(app, "stats.boxSwitch.reviewsMode").exists, "By Type | By Answer stays in the header")
        XCTAssertEqual(menu.value as? String, "Count")
        menu.tap()
        // A picker's options in a menu carry no identifier on iOS: the menu's items by their
        // titles (the launch is en_US).
        let items = app.collectionViews.buttons
        let time = items["Time"]
        XCTAssertTrue(time.waitForExistence(timeout: 5) && items["Count"].exists, "Count and Time in the menu")
        XCTAssertTrue(items["Count"].isSelected, "Count is the chosen one")
        time.tap()
        XCTAssertTrue(waitUntil { (menu.value as? String) == "Time" }, "Time chosen from the menu")
    }

    /// S8.3 on touch: a short hold selects the bar under the finger and keeps it after the finger
    /// lifts; a plain vertical swipe over the same plot scrolls the page and selects nothing.
    func testAHoldSelectsAndASwipeStillScrolls() throws {
        let app = launch(route: "stats/page/typical")
        let plot = element(app, "stats.box.knowledgeOverTime.plot")
        XCTAssertTrue(waitFor(plot, NSPredicate(format: "exists == true AND label BEGINSWITH 'Estimated knowledge'"), 40), "Filled")
        XCTAssertEqual(plot.value as? String ?? "", "", "Nothing pinned yet")
        let before = plot.frame.minY
        plot.swipeUp(velocity: .slow)
        XCTAssertLessThan(plot.frame.minY, before - 40, "A swipe over the plot scrolls the page")
        XCTAssertEqual(plot.value as? String ?? "", "", "A swipe selects nothing")
        plot.swipeDown(velocity: .slow)
        plot.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.6)
        XCTAssertTrue(waitUntil { !((plot.value as? String) ?? "").isEmpty }, "A hold pins the bar under the finger")
    }

    /// Activity's recent lines (the coordinator's decision of 2026-11-23): Show All Years grows the
    /// grid above today's line in place, the control staying where it is on the screen; Show Less
    /// brings the recent lines back. The figures stay in both states.
    func testShowAllYearsExpandsActivityInPlace() throws {
        let app = launch(route: "stats/page/typical")
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20))
        element(app, "stats.toolbar.jump").tap()
        element(app, "stats.toolbar.jump.reviews").tap()
        let plot = element(app, "stats.box.activity.plot")
        let toggle = element(app, "stats.box.activity.showAllYears")
        XCTAssertTrue(waitFor(toggle, NSPredicate(format: "exists == true AND label == 'Show All Years'"), 20),
                      "Three years of history: the recent lines and Show All Years")
        // Bring the control into the upper half of the screen, a short drag at a time, then expand.
        let window = app.windows.firstMatch.frame
        for _ in 0..<8 where !(toggle.isHittable && toggle.frame.maxY < window.midY) {
            let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.7))
            start.press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.45)))
        }
        XCTAssertTrue(toggle.isHittable, "Show All Years on screen: \(toggle.frame) in \(window)")
        let before = toggle.frame.minY
        let shortGrid = plot.frame.height
        toggle.tap()
        XCTAssertTrue(waitFor(toggle, NSPredicate(format: "label == 'Show Less'"), 5), "Show Less after expanding")
        XCTAssertGreaterThan(plot.frame.height, shortGrid + 100, "Every year is shown")
        XCTAssertEqual(toggle.frame.minY, before, accuracy: 3, "The control and today's line stay where they were")
        XCTAssertTrue(element(app, "stats.box.activity.plot").exists, "The grid stays")
        toggle.tap()
        XCTAssertTrue(waitFor(toggle, NSPredicate(format: "label == 'Show All Years'"), 5), "Show All Years after Show Less")
        XCTAssertEqual(plot.frame.height, shortGrid, accuracy: 1, "The recent lines again")
        XCTAssertEqual(toggle.frame.minY, before, accuracy: 3, "Still in place")
    }

    // MARK: States (S12)

    func testNoCollectionShowsItsViewWithTheControlsDisabled() throws {
        let app = launch(route: "stats/page/no-collection")
        XCTAssertTrue(element(app, "stats.page.noCollection").waitForExistence(timeout: 20), "No Collection (S12.1)")
        XCTAssertFalse(element(app, "stats.toolbar.deck").isEnabled, "The deck pop-up is disabled")
        XCTAssertFalse(element(app, "stats.toolbar.period").isEnabled, "The period is disabled")
        XCTAssertFalse(element(app, "stats.toolbar.jump").isEnabled, "The Jump menu is disabled")
    }

    func testADeletedDeckOffersAllDecks() throws {
        let app = launch(route: "stats/page/deleted-deck")
        let show = element(app, "stats.page.showAllDecks")
        XCTAssertTrue(show.waitForExistence(timeout: 20), "This deck no longer exists, with Show All Decks (S12.7)")
        show.tap()
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 20), "All Decks shows the page")
        XCTAssertEqual(element(app, "stats.toolbar.deck").value as? String, "All Decks")
    }

    // MARK: The launch option over a collection (S13)

    /// `-MeguriStatsRollout open` over a generated collection reaches the Statistics tab on the
    /// iPhone and fills the page from the real core.
    func testTheLaunchOptionOpensStatisticsOverACollection() throws {
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1", directoryHint: .isDirectory),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriStatsRollout", "open"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "stats.page.content").waitForExistence(timeout: 60),
                      "The page over the fixture (stage local-review-1 through tools/mg)")
        XCTAssertTrue(element(app, "shell.root.tab.stats").isSelected, "On the Statistics tab")
        XCTAssertTrue(waitFor(element(app, "stats.strip.studiedToday"), NSPredicate(format: "exists == true AND value != ''"), 60),
                      "Studied Today filled from the core")
    }

    // MARK: Support

    private func launch(route: String) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func waitFor(_ target: XCUIElement, _ predicate: NSPredicate, _ timeout: TimeInterval) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: target)], timeout: timeout) == .completed
    }

    private func waitUntil(timeout: TimeInterval = 10, _ condition: @escaping () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return condition()
    }
    #endif
}
