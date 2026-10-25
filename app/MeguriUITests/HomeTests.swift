import Foundation
import XCTest

/// Production Home and the generated, public local-review recipe. No screen route
/// or preview DTO stands in for the collection-owned disclosure/save actions.
@MainActor
final class HomeTests: XCTestCase {
    func testNativeDeckDisclosureAndSelectionStayIndependent() throws {
        let app = try launchFixture()
        let parent = try row(app, named: "Local Review")
        let child = try row(app, named: "Local Review::Cards")
        let parentID = parent.identifier
        let childID = child.identifier
        XCTAssertNotEqual(parentID, childID)
        assertCounts(parent)
        assertCounts(child)
        try assertHierarchy(app)

        let disclosure = try disclosure(app, rowIdentifier: parentID)
        XCTAssertEqual(disclosure.label, "Local Review")
        XCTAssertEqual(disclosure.value as? String, "Expanded")
        disclosure.tap()
        XCTAssertTrue(child.waitForNonExistence(timeout: 10))
        XCTAssertTrue(parent.exists, "Collapsing must retain the parent's selection action")
        XCTAssertFalse(app.buttons["home.deck.study"].exists, "Disclosure must not select a deck")
        assertSettled(disclosure, value: "Collapsed")
        try assertHierarchy(app)

        disclosure.tap()
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        XCTAssertEqual(child.identifier, childID)
        assertSettled(disclosure, value: "Expanded")
        child.tap()
        try assertPage(app, rowIdentifier: childID)
        try backToDecks(app)
        XCTAssertTrue(child.waitForExistence(timeout: 10), "Selection must not collapse the parent")
        XCTAssertEqual(disclosure.value as? String, "Expanded")
        parent.tap()
        try assertPage(app, rowIdentifier: parentID)
        try backToDecks(app)
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        try assertHierarchy(app)
    }

    func testStudyCollapseSurvivesRestartWithoutReseeding() throws {
        let app = try launchFixture()
        let parent = try row(app, named: "Local Review")
        let child = try row(app, named: "Local Review::Cards")
        let parentID = parent.identifier
        let childID = child.identifier
        let parentDisclosure = try disclosure(app, rowIdentifier: parentID)
        parentDisclosure.tap()
        // Child visibility is optimistic; only the settled value proves completion.
        XCTAssertTrue(child.waitForNonExistence(timeout: 10))
        assertSettled(parentDisclosure, value: "Collapsed")
        app.terminate()

        // In particular, do not pass -MeguriFixture again: it replaces the database.
        app.launchArguments = launchArguments()
        app.launch()
        let restoredParent = app.buttons[parentID]
        XCTAssertTrue(restoredParent.waitForExistence(timeout: 20))
        XCTAssertEqual(app.buttons.matching(identifier: parentID).count, 1)
        XCTAssertFalse(app.buttons[childID].exists, "The saved collapse must survive process restart")
        XCTAssertFalse(app.buttons["home.deck.study"].exists)
        let restoredDisclosure = try disclosure(app, rowIdentifier: parentID)
        XCTAssertEqual(restoredDisclosure.value as? String, "Collapsed")
        try assertHierarchy(app)
        restoredDisclosure.tap()
        let restoredChild = app.buttons[childID]
        XCTAssertTrue(restoredChild.waitForExistence(timeout: 10))
        XCTAssertEqual(restoredChild.label, "Local Review::Cards")
        assertCounts(restoredParent)
        assertCounts(restoredChild)
        try assertHierarchy(app)
        restoredChild.tap()
        try assertPage(app, rowIdentifier: childID)
        // Browser flags are covered by SavedDeckRestartTests and the core's
        // collapse_scope_selection_and_not_found; Home's AX tree cannot prove them.
    }

    private func launchFixture() throws -> XCUIApplication {
        continueAfterFailure = false
        #if os(macOS)
        throw XCTSkip("This regression exercises the native iPhone/iPad disclosure hierarchy")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = launchArguments(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"))
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let enable = app.buttons["home.policy.enable"]
        if enable.waitForExistence(timeout: 5) { enable.tap() }
        return app
        #endif
    }

    private func launchArguments(fixture: URL? = nil) -> [String] {
        // Exercise the root List on both device classes, including narrow iPad windows.
        MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false, extra: ["-MeguriSizeClass", "compact"]).arguments
    }

    private func row(_ app: XCUIApplication, named name: String) throws -> XCUIElement {
        // Deliberately the same identifier family as LocalReviewJourneyTests.openDeck.
        let matches = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "home.deck.row.", name))
        XCTAssertTrue(matches.firstMatch.waitForExistence(timeout: 20), "Stage the generated local-review-1 fixture in the VM")
        XCTAssertEqual(matches.count, 1, "Each deck must have one distinct selection action")
        return matches.element
    }

    private func disclosure(_ app: XCUIApplication, rowIdentifier: String) throws -> XCUIElement {
        let identifier = rowIdentifier.replacingOccurrences(of: "home.deck.row.", with: "home.deck.disclosure.")
        let matches = app.buttons.matching(identifier: identifier)
        XCTAssertTrue(matches.firstMatch.waitForExistence(timeout: 10), "The native disclosure must be independently actionable")
        XCTAssertEqual(matches.count, 1)
        XCTAssertTrue(matches.element.isEnabled)
        XCTAssertTrue(matches.element.isHittable)
        return matches.element
    }

    private func assertCounts(_ row: XCUIElement) {
        XCTAssertEqual(row.value as? String, "0 new · 0 learning · 4 due")
    }

    private func assertSettled(_ disclosure: XCUIElement, value: String) {
        let settled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", value), object: disclosure)
        XCTAssertEqual(XCTWaiter.wait(for: [settled], timeout: 10), .completed,
            "Wait for the collection-owned expansion write before continuing")
    }

    private func assertPage(_ app: XCUIApplication, rowIdentifier: String) throws {
        let page = rowIdentifier.replacingOccurrences(of: "home.deck.row.", with: "home.deck.page.")
        XCTAssertTrue(app.descendants(matching: .any)[page].waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["home.deck.study"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["review.session.screen"].exists, "Selection must not start Study")
    }

    private func backToDecks(_ app: XCUIApplication) throws {
        let back = app.buttons["BackButton"]
        XCTAssertTrue(back.waitForExistence(timeout: 10))
        back.tap()
        XCTAssertTrue(app.staticTexts["home.deck.header"].waitForExistence(timeout: 10))
    }

    private func assertHierarchy(_ app: XCUIApplication) throws {
        let tree = try HierarchyExport.tree(app)
        let nodes = tree.root.allNodes()
        let home = nodes.filter { $0.identifier.hasPrefix("home.deck.") }
        let identifiers = home.map(\.identifier)
        XCTAssertEqual(Set(identifiers).count, identifiers.count, "Home identifiers must be unique across the actual native hierarchy")
        XCTAssertEqual(home.filter { $0.identifier == "home.deck.header" }.count, 1)
        // Inspect every native control under each disclosure, including the native
        // chevron. An identified outer element does not excuse an unnamed button.
        for branch in home where branch.identifier.hasPrefix("home.deck.disclosure.") {
            for control in branch.allNodes() where ["button", "disclosureTriangle"].contains(control.type) {
                XCTAssertFalse(control.identifier.isEmpty, "Unidentified native disclosure control")
                XCTAssertFalse(control.label.isEmpty, "Unnamed native disclosure control: \(control.identifier)")
            }
        }
        let attachment = XCTAttachment(string: app.debugDescription)
        attachment.name = "production-home-hierarchy"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
