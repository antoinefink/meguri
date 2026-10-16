import Foundation
import XCTest
#if os(macOS)
import AppKit
#else
import UIKit
import notify
#endif

/// The app shell (U150): the tab and sidebar layouts, switching between
/// them at run time, the review and sheet presentations, deep links, the
/// Mac window's lifetime and the registered URL schemes (R2–R8, R11,
/// R17–R23, R25, R319–R321).
///
/// The shell routes (`shell/root/…`, TestHooks/ShellRoutes.swift) give the
/// shell sample content: decks, scrolling roots, a deck page with Study and a
/// review stand-in that keeps its card and side. `-MeguriSizeClass` forces a
/// layout and the `toggleSizeClass` command switches it without a relaunch.
/// The probe `testhooks.shell.state` prints the navigation's state.
///
/// On the Mac, synthesized input does not reach the app on this toolchain
/// (app/AGENTS.md), so tests that tap run on iOS only; the Mac runs the
/// ones that only query and post commands.
@MainActor
final class NavigationLayoutTests: XCTestCase {
    private let spanishMining = "1700000000001"
    /// The app's bundle id, which names the test-hook commands.
    private var bundleIdentifier = ""

    // MARK: Layouts (R2–R4, R17, R18)

    func testTheTabLayoutHasDecksBrowseStatsAndSettings() throws {
        #if os(macOS)
        throw XCTSkip("the Mac always shows the sidebar layout")
        #else
        let app = try launch(route: "shell/root/typical", sizeClass: "compact")
        let tabBar = app.tabBars.firstMatch
        XCTAssert(tabBar.waitForExistence(timeout: 15), "no tab bar at compact width")
        XCTAssertEqual(tabBar.buttons.allElementsBoundByIndex.map(\.label), ["Decks", "Browse", "Stats", "Settings"])
        // Each tab button has its own identifier (R1964), so tests and agents
        // reach a tab whatever the language. SwiftUI sets them a moment
        // after launch.
        let identifiers = ["decks", "browse", "stats", "settings"].map { "shell.root.tab.\($0)" }
        XCTAssert(
            waitUntil { tabBar.buttons.allElementsBoundByIndex.map(\.identifier) == identifiers },
            "\(tabBar.buttons.allElementsBoundByIndex.map(\.identifier))"
        )
        XCTAssertFalse(element(app, "shell.root.sidebar.home").exists, "the sidebar shows at compact width")
        #endif
    }

    func testTheSidebarListsTheSectionsThenTheDecks() throws {
        let app = try launch(route: "shell/root/typical", sizeClass: "regular")
        let rows = ["home", "browse", "stats", "noteTypes", "settings"].map { element(app, "shell.root.sidebar.\($0)") }
        XCTAssert(rows[0].waitForExistence(timeout: 15), "no sidebar at regular width")
        let decksHeader = element(app, "shell.root.sidebarDecks")
        let firstDeck = element(app, "testhooks.shell.deck.1")
        for item in rows + [decksHeader, firstDeck] {
            XCTAssert(item.exists, "\(item) is missing")
        }
        let tops = (rows + [decksHeader, firstDeck]).map(\.frame.minY)
        XCTAssertEqual(tops, tops.sorted(), "the sidebar is not Home, Browse, Stats, Note Types, Settings, then Decks: \(tops)")
        XCTAssertFalse(app.tabBars.firstMatch.exists, "a tab bar shows in the sidebar layout")
    }

    // MARK: Tab reselection (R19)

    func testReselectingTheBrowseTabPopsItThenScrollsToTheTop() throws {
        #if os(macOS)
        throw XCTSkip("taps do not reach the Mac app on this toolchain; AppNavigationTests covers the logic")
        #else
        let app = try launch(route: "shell/root/browse-pushed", sizeClass: "compact")
        XCTAssert(element(app, "testhooks.shell.noteTypes.row.1").waitForExistence(timeout: 15), "Note Types is not pushed on Browse")
        let browseTab = element(app, "shell.root.tab.browse")
        XCTAssert(browseTab.waitForExistence(timeout: 5), "no Browse tab")
        browseTab.tap()
        XCTAssert(element(app, "testhooks.shell.browse.row.20").waitForExistence(timeout: 5), "the first reselect did not pop to Browse")
        keepScreen(app, "reselect-first")
        XCTAssertTrue(probe(app).contains("browse= "), probe(app))
        XCTAssertFalse(element(app, "testhooks.shell.browse.row.1").isHittable, "the first reselect also scrolled to the top")
        browseTab.tap()
        XCTAssert(waitUntil { self.element(app, "testhooks.shell.browse.row.1").isHittable }, "the second reselect did not scroll to the top")
        #endif
    }

    // MARK: Layout switches (R5)

    /// Selected deck Spanish::Mining, then Browse with Note Types pushed and
    /// its root at row 20: compact → regular → compact keeps all of it.
    func testSwitchingLayoutsKeepsTheSectionDeckStackAndScrollPosition() throws {
        #if os(macOS)
        throw XCTSkip("the Mac has no size class")
        #else
        let app = try launch(route: "shell/root/browse-pushed", sizeClass: "compact")
        XCTAssert(element(app, "testhooks.shell.noteTypes.row.1").waitForExistence(timeout: 15))
        let before = navigationState(app)
        XCTAssertTrue(before.contains("section=browse deck=\(spanishMining) home= browse=noteTypes"), before)
        XCTAssertTrue(before.contains("scroll.browse=row-20"), before)

        toggleSizeClass()
        XCTAssert(waitUntil { self.probe(app).hasPrefix("layout=sidebar") }, "the sidebar layout did not take over: \(probe(app))")
        keepScreen(app, "switch-regular")
        XCTAssert(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 10), "no sidebar")
        XCTAssert(element(app, "testhooks.shell.noteTypes.row.1").waitForExistence(timeout: 5), "Note Types is no longer pushed on Browse")
        XCTAssertEqual(navigationState(app), before)
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").isSelected, "the sidebar does not show Browse selected")

        toggleSizeClass()
        XCTAssert(waitUntil { self.probe(app).hasPrefix("layout=compact") }, "the tab layout did not come back: \(probe(app))")
        XCTAssert(app.tabBars.firstMatch.waitForExistence(timeout: 10), "no tab bar")
        XCTAssert(element(app, "testhooks.shell.noteTypes.row.1").waitForExistence(timeout: 5))
        XCTAssertEqual(navigationState(app), before)
        let browseTab = element(app, "shell.root.tab.browse")
        XCTAssert(waitUntil { browseTab.exists && browseTab.isSelected }, "the Browse tab is not selected")

        // Back on Browse's root: still at row 20.
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssert(element(app, "testhooks.shell.browse.row.20").waitForExistence(timeout: 5))
        XCTAssertTrue(element(app, "testhooks.shell.browse.row.20").isHittable)
        XCTAssertFalse(element(app, "testhooks.shell.browse.row.1").isHittable, "Browse's root lost its scroll position")
        #endif
    }

    /// An open review on its answer side: switching layouts keeps the card,
    /// its side and the answers (none is recorded by the switch).
    func testSwitchingLayoutsKeepsTheReviewOnTheSameCardAndSide() throws {
        #if os(macOS)
        throw XCTSkip("the Mac has no size class")
        #else
        let app = try launch(route: "shell/root/review-answer", sizeClass: "compact")
        assertReview(app, card: "Card 1001", side: "Answer", answers: "Answers: 0")
        XCTAssertFalse(app.tabBars.firstMatch.isHittable, "the tab bar shows during the review (R23)")

        toggleSizeClass()
        XCTAssert(waitUntil { self.probe(app).hasPrefix("layout=sidebar") }, "the sidebar layout did not take over: \(probe(app))")
        assertReview(app, card: "Card 1001", side: "Answer", answers: "Answers: 0")
        XCTAssertTrue(probe(app).contains("review=\(spanishMining)"), probe(app))
        XCTAssertTrue(probe(app).contains("columns=detailOnly"), "the sidebar shows during the review (R21): \(probe(app))")

        toggleSizeClass()
        assertReview(app, card: "Card 1001", side: "Answer", answers: "Answers: 0")

        // The stand-in is live: an answer moves it on, and a switch keeps that.
        element(app, "testhooks.shell.review.good").tap()
        assertReview(app, card: "Card 1002", side: "Question", answers: "Answers: 1")
        toggleSizeClass()
        assertReview(app, card: "Card 1002", side: "Question", answers: "Answers: 1")
        #endif
    }

    // MARK: Review in the sidebar layout (R21)

    func testAReviewHidesTheSidebarAndItsEndRestoresIt() throws {
        #if os(macOS)
        throw XCTSkip("taps do not reach the Mac app on this toolchain; AppNavigationTests covers the logic")
        #else
        guard UIDevice.current.userInterfaceIdiom == .pad else {
            throw XCTSkip("at iPhone widths the sidebar overlays the detail column; the iPad shows it beside the review's column")
        }
        let app = try launch(route: "shell/root/deck-page", sizeClass: "regular")
        let study = element(app, "testhooks.shell.deckPage.study")
        XCTAssert(study.waitForExistence(timeout: 15))
        XCTAssertTrue(element(app, "shell.root.sidebar.home").isHittable, "the sidebar is hidden before the review")
        study.tap()
        assertReview(app, card: "Card 1001", side: "Question", answers: "Answers: 0")
        XCTAssert(waitUntil { !self.element(app, "shell.root.sidebar.home").isHittable }, "the sidebar stayed during the review")
        XCTAssertTrue(probe(app).contains("columns=detailOnly"), probe(app))

        element(app, "testhooks.shell.review.close").tap()
        XCTAssert(element(app, "testhooks.shell.deckPage.title").waitForExistence(timeout: 5), "the deck page did not come back")
        XCTAssert(waitUntil { self.element(app, "shell.root.sidebar.home").isHittable }, "the sidebar was not restored")
        XCTAssertTrue(probe(app).contains("review=none"), probe(app))
        XCTAssertTrue(probe(app).contains("columns=all"), probe(app))
        #endif
    }

    // MARK: Sheets (R25)

    func testAddOpensOverTheDeckPageAndClosingItKeepsItsScrollPosition() throws {
        #if os(macOS)
        throw XCTSkip("taps do not reach the Mac app on this toolchain; AppNavigationTests covers the logic")
        #else
        let app = try launch(route: "shell/root/deck-page", sizeClass: "compact")
        XCTAssert(element(app, "testhooks.shell.deckPage.title").waitForExistence(timeout: 15))
        app.swipeUp()
        app.swipeUp()
        let row = try XCTUnwrap(
            (1...40).lazy.map { self.element(app, "testhooks.shell.deckPage.row.\($0)") }.first { $0.exists && $0.isHittable && $0.frame.minY > 200 },
            "no deck page row on screen after scrolling"
        )
        let frame = row.frame
        let state = probe(app)
        // The navigation holds the position the user scrolled to (R5).
        XCTAssertFalse(state.contains("scroll.deckPage=none"), "the deck page's scroll position is not kept: \(state)")

        post("presentAdd")
        let cancel = element(app, "shell.sheet.cancel")
        XCTAssert(cancel.waitForExistence(timeout: 10), "Add did not open")
        cancel.tap()
        XCTAssert(cancel.waitForNonExistence(timeout: 5), "Add did not close")
        XCTAssertTrue(element(app, "testhooks.shell.deckPage.title").exists || row.exists, "the deck page is gone")
        XCTAssertEqual(row.frame.minY, frame.minY, accuracy: 1, "the deck page did not keep its scroll position")
        XCTAssertEqual(probe(app), state)
        #endif
    }

    // MARK: Deep links (R319–R321)

    /// From Browse (Note Types pushed, Spanish::Mining selected): a deck
    /// link opens Japanese's page, a Browse link opens Browse's root with the
    /// search, and a link to a missing deck changes nothing.
    func testDeepLinksOpenADeckOrBrowseAndRefuseAMissingDeck() throws {
        let japanese = "1700000000002"
        // Each device's own layout: on an iPhone forced to regular width the
        // sidebar covers the screen and hides the probe.
        let app = try launch(route: "shell/root/browse-pushed")
        XCTAssert(element(app, "testhooks.shell.state").waitForExistence(timeout: 15))

        try openLink("meguri://deck?id=\(japanese)")
        XCTAssert(waitUntil { self.probe(app).contains("section=home deck=\(japanese)") }, probe(app))
        XCTAssert(element(app, "testhooks.shell.deckPage.title").waitForExistence(timeout: 5), "the deck page is not shown")
        XCTAssertEqual(text(element(app, "testhooks.shell.deckPage.title")), "Japanese")
        XCTAssertTrue(probe(app).contains("review=none"), "a link started a review: \(probe(app))")

        try openLink("meguri://browse?search=deck:Japanese")
        XCTAssert(waitUntil { self.probe(app).contains("section=browse") }, probe(app))
        XCTAssertTrue(probe(app).contains("browse= "), "Browse did not open at its root: \(probe(app))")
        XCTAssertTrue(probe(app).contains("search=deck:Japanese"), probe(app))
        XCTAssert(element(app, "testhooks.shell.browseSearch").waitForExistence(timeout: 5))

        #if os(iOS)
        // Dismissing the message needs a tap, which the Mac does not get.
        let state = probe(app)
        try openLink("meguri://deck?id=999")
        let alert = app.alerts.firstMatch
        XCTAssert(alert.waitForExistence(timeout: 10), "no message for a missing deck (R320)")
        XCTAssertTrue(alert.staticTexts["No deck has the ID 999."].exists, alert.debugDescription)
        alert.buttons["OK"].tap()
        XCTAssert(alert.waitForNonExistence(timeout: 5))
        XCTAssertEqual(probe(app), state, "a link to a missing deck changed the navigation")
        #endif
    }

    func testOnlyTheMeguriSchemeIsRegistered() throws {
        let build = try BuildUnderTest.current()
        let info = try build.appInfo
        let types = try XCTUnwrap(info["CFBundleURLTypes"] as? [[String: Any]], "no URL types")
        let schemes = types.flatMap { $0["CFBundleURLSchemes"] as? [String] ?? [] }
        XCTAssertEqual(schemes, ["meguri"], "R319, R321: meguri only, never anki")
        #if os(iOS)
        // One scene (R7), resizable windows in every orientation (R7, R8).
        let manifest = try XCTUnwrap(info["UIApplicationSceneManifest"] as? [String: Any])
        XCTAssertEqual(manifest["UIApplicationSupportsMultipleScenes"] as? Bool, false)
        XCTAssertNil(info["UIRequiresFullScreen"], "UIRequiresFullScreen stops windows from resizing")
        let iphone = try XCTUnwrap(info["UISupportedInterfaceOrientations~iphone"] as? [String])
        XCTAssertTrue(Set(iphone).isSuperset(of: ["UIInterfaceOrientationPortrait", "UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight"]))
        let ipad = try XCTUnwrap(info["UISupportedInterfaceOrientations~ipad"] as? [String])
        XCTAssertEqual(Set(ipad).count, 4)
        #endif
    }

    // MARK: The Mac window's lifetime (R11)

    func testClosingTheMacWindowQuitsWithoutIntegrations() throws {
        #if os(macOS)
        let app = try launch(route: "shell/root/typical", extra: ["-MeguriIntegrations", "off"])
        XCTAssert(element(app, "shell.root.sidebar.home").waitForExistence(timeout: 15))
        post("closeWindow")
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15), "the app kept running without a window and without integrations")
        #else
        throw XCTSkip("the window lifetime is the Mac's")
        #endif
    }

    func testClosingTheMacWindowKeepsTheAppWithIntegrationsAndReopeningShowsIt() throws {
        #if os(macOS)
        let build = try BuildUnderTest.current()
        let app = try launch(route: "shell/root/deck-page", extra: ["-MeguriIntegrations", "on"])
        XCTAssert(element(app, "testhooks.shell.deckPage.title").waitForExistence(timeout: 15))
        post("closeWindow")
        XCTAssert(waitUntil { app.windows.count == 0 }, "the window did not close")
        XCTAssertFalse(app.wait(for: .notRunning, timeout: 5), "the app quit although an integration is on")
        // Window > Meguri opens it again (clicking it needs input the Mac
        // does not get on this toolchain).
        let windowMenu = app.menuBars.menuBarItems["Window"]
        XCTAssertTrue(windowMenu.menuItems["Meguri"].exists, "no Window > Meguri: \(windowMenu.menuItems.allElementsBoundByIndex.map(\.title))")

        // What a click on the Dock icon sends: open the running app again.
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        let opened = expectation(description: "reopen")
        NSWorkspace.shared.openApplication(at: build.appBundle, configuration: configuration) { _, _ in opened.fulfill() }
        wait(for: [opened], timeout: 15)
        XCTAssert(element(app, "testhooks.shell.deckPage.title").waitForExistence(timeout: 15), "reopening did not show the window on the same deck page")
        XCTAssertEqual(app.windows.count, 1)
        #else
        throw XCTSkip("the window lifetime is the Mac's")
        #endif
    }

    // MARK: Helpers

    private func launch(route: String, sizeClass: String? = nil, extra: [String] = []) throws -> XCUIApplication {
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("\(build.configuration.rawValue) has no test hooks") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("never launch \(try build.bundleIdentifier) on the Mac: it would replace the daily-driver app")
        }
        #endif
        continueAfterFailure = false
        bundleIdentifier = try build.bundleIdentifier
        var arguments = extra
        #if os(iOS)
        if let sizeClass { arguments += ["-MeguriSizeClass", sizeClass] }
        #endif
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: route, extra: arguments).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any)[identifier].firstMatch
    }

    /// The navigation state probe's text (its label on iOS, its value on the Mac).
    private func probe(_ app: XCUIApplication) -> String {
        let probe = element(app, "testhooks.shell.state")
        guard probe.waitForExistence(timeout: 5) else { return "<no probe>" }
        return probe.label.isEmpty ? (probe.value as? String ?? "") : probe.label
    }

    /// The probe without its first field, the layout: the navigation state
    /// both layouts share.
    private func navigationState(_ app: XCUIApplication) -> String {
        probe(app).split(separator: " ").dropFirst().joined(separator: " ")
    }

    private func text(_ element: XCUIElement) -> String {
        element.label.isEmpty ? (element.value as? String ?? "") : element.label
    }

    private func assertReview(_ app: XCUIApplication, card: String, side: String, answers: String, line: UInt = #line) {
        let cardText = element(app, "testhooks.shell.review.card")
        XCTAssert(waitUntil { cardText.exists && self.text(cardText) == card }, "card: \(text(cardText)), expected \(card)", line: line)
        XCTAssertEqual(text(element(app, "testhooks.shell.review.side")), side, line: line)
        XCTAssertEqual(text(element(app, "testhooks.shell.review.answers")), answers, line: line)
    }

    /// Opens a link the way another app does: the system hands it to the
    /// running app (`XCUIApplication.open` relaunches the app first).
    private func openLink(_ link: String) throws {
        let url = try XCTUnwrap(URL(string: link))
        #if os(macOS)
        let opened = expectation(description: link)
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        NSWorkspace.shared.open([url], withApplicationAt: try BuildUnderTest.current().appBundle, configuration: configuration) { _, _ in
            opened.fulfill()
        }
        wait(for: [opened], timeout: 15)
        #else
        XCUIDevice.shared.system.open(url)
        #endif
    }

    /// Keeps a screenshot of the app in the result bundle.
    private func keepScreen(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func toggleSizeClass() {
        post("toggleSizeClass")
    }

    /// Posts a test-hook command (`TestHooks.ShellCommands`).
    private func post(_ command: String) {
        let name = "\(bundleIdentifier).testhooks.\(command)"
        #if os(macOS)
        DistributedNotificationCenter.default().postNotificationName(Notification.Name(name), object: nil, userInfo: nil, deliverImmediately: true)
        #else
        notify_post(name)
        #endif
    }

    private func waitUntil(timeout: TimeInterval = 10, _ condition: @escaping () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            guard Date() < deadline else { return false }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return true
    }
}
