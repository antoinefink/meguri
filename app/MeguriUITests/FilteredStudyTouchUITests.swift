#if os(iOS)
import Foundation
import UIKit
import XCTest

/// Production pages, row menus and forms over the same MG recipe as the Mac.
/// Run compact on iPhone and iPad; the second case covers the wide iPad sidebar.
@MainActor final class FilteredStudyTouchUITests: XCTestCase {
    func testCompactDeckActionsAndOperableStudyForms() throws {
        try journey(sizeClass: "compact")
    }

    func testIPadSidebarDeckActionsAndOperableStudyForms() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("Wide iPad sidebar") }
        XCUIDevice.shared.orientation = .landscapeLeft
        defer { XCUIDevice.shared.orientation = .portrait }
        try journey(sizeClass: "regular")
    }

    private func journey(sizeClass: String) throws {
        let app = try launch(sizeClass: sizeClass)
        defer { app.terminate() }
        let prefix = "filtered-touch-" + sizeClass
        let normalID = try selectDeck("Preview Source", app: app)
        try tap("home.deck.customStudy", app: app)
        XCTAssertTrue(element(app, "home.customStudy.available").waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.customStudy.state").exists, "Only relevant inputs are exposed")
        try app.performAccessibilityAudit(for: [.trait, .sufficientElementDescription])
        try replace("home.customStudy.amount", with: "invalid", app: app)
        XCTAssertFalse(element(app, "home.customStudy.save").isEnabled)
        try replace("home.customStudy.amount", with: "0", app: app)
        capture(app, prefix + "-custom-study-page-entry-and-editable-amount")
        try tap("home.customStudy.save", app: app)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 15))
        XCTAssertTrue(element(app, "home.deck.page." + normalID).exists, "Daily adjustment keeps the normal deck")

        try openDeckMenu(normalID, app: app)
        assertUnavailableBrowse(normalID, app: app)
        XCTAssertTrue(element(app, "home.deckMenu.customStudy." + normalID).isEnabled)
        XCTAssertFalse(element(app, "home.filteredDeck.edit." + normalID).exists)
        try tap("home.deckMenu.customStudy." + normalID, app: app)
        try chooseOption("home.customStudy.mode", index: 5, count: 6, app: app)
        try chooseOption("home.customStudy.state", index: 3, count: 4, app: app)
        try replace("home.customStudy.amount", with: "2", app: app)
        try replace("home.customStudy.includeTags", with: "filtered-ui", app: app)
        try replace("home.customStudy.excludeTags", with: "exclude::tag*", app: app)
        capture(app, prefix + "-custom-study-state-and-literal-tags-form")
        XCTAssertTrue(element(app, "home.customStudy.save").isEnabled)
        try tap("home.customStudy.cancel", app: app)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 10))
        try openDeckMenu(normalID, app: app)
        try tap("home.deckMenu.createFilteredDeck." + normalID, app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.name").waitForExistence(timeout: 10))
        XCTAssertTrue((element(app, "home.filteredStudy.first.search").value as? String ?? "").contains("is:due"))
        try replace("home.filteredStudy.name", with: "Touch Preview", app: app)
        try replace("home.filteredStudy.first.limit", with: "1", app: app)
        try tap("home.filteredStudy.secondEnabled", app: app)
        try replace("home.filteredStudy.second.search", with: "tag:filtered-ui", app: app)
        try replace("home.filteredStudy.second.limit", with: "5", app: app)
        try tap("home.filteredStudy.reschedule", app: app)
        try replace("home.filteredStudy.again", with: "60", app: app)
        try replace("home.filteredStudy.hard", with: "invalid", app: app)
        XCTAssertFalse(element(app, "home.filteredStudy.save").isEnabled, "Native Hard binding validates typed text")
        try replace("home.filteredStudy.hard", with: "120", app: app)
        try replace("home.filteredStudy.good", with: "0", app: app)
        capture(app, prefix + "-two-search-preview-form-with-edited-delays")
        try tap("home.filteredStudy.save", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 20))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2),
                      "Overlap gathers two distinct eligible cards and excludes the suspended match")
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 15))
        let filteredID = String(page.identifier.dropFirst("home.deck.page.".count))
        XCTAssertNotNil(Int64(filteredID), "Filtered deck page has a deck ID")
        XCTAssertNotEqual(filteredID, normalID, "Creation selects the new filtered deck")
        XCTAssertFalse(element(app, "home.deck.addNote").exists)
        XCTAssertFalse(element(app, "home.deck.customStudy").exists)
        try tap("home.filteredDeck.edit." + filteredID, app: app)
        for (field, value) in [("name", "Touch Preview"), ("again", "60"), ("hard", "120"), ("good", "0")] {
            let target = try visible("home.filteredStudy." + field, app: app)
            XCTAssertEqual(target.value as? String, value, "Saved native text binding: " + field)
            XCTAssertFalse(target.label.isEmpty, "Accessible field name: " + field)
        }
        try replace("home.filteredStudy.good", with: "30", app: app)
        try tap("home.filteredStudy.save", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 20))
        try tap("home.filteredDeck.edit." + filteredID, app: app)
        XCTAssertEqual(try visible("home.filteredStudy.good", app: app).value as? String, "30")
        try app.performAccessibilityAudit(for: [.trait, .sufficientElementDescription])
        capture(app, prefix + "-reopened-native-editor-retains-hard-and-good")
        try tap("home.filteredStudy.cancel", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 10))

        try openDeckMenu(filteredID, app: app)
        assertUnavailableBrowse(filteredID, app: app)
        for action in ["addNote", "subdeck", "customStudy", "options"] {
            XCTAssertFalse(element(app, "home.deckMenu.\(action).\(filteredID)").exists, action)
        }
        for action in ["edit", "empty", "rebuild"] {
            XCTAssertTrue(element(app, "home.filteredDeck.\(action).\(filteredID)").isEnabled, action)
        }
        capture(app, prefix + "-filtered-deck-native-menu")
        try tap("home.filteredDeck.empty." + filteredID, app: app)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "home.deck.study").exists)
        try openDeckMenu(filteredID, app: app)
        XCTAssertFalse(element(app, "home.deckMenu.study." + filteredID).isEnabled, "Empty deck cannot start study")
        try tap("home.filteredDeck.rebuild." + filteredID, app: app)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        try tap("home.filteredDeck.empty." + filteredID, app: app)
        try tap("home.deck.rebuild", app: app)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        capture(app, prefix + "-rebuilt-deck-page-with-real-study-entry")
        try tap("home.deck.study", app: app)
        try tap("review.session.reveal", app: app)
        XCTAssertTrue(try visible("review.session.rate.again", app: app).isEnabled)
        try tap("review.session.close", app: app)
    }

    /// Touch Browse is intentionally unavailable until it has a destination.
    /// The lifecycle fix may omit the item or leave a disabled native item.
    private func assertUnavailableBrowse(_ deck: String, app: XCUIApplication) {
        let browse = element(app, "home.deckMenu.browse." + deck)
        XCTAssertTrue(!browse.exists || !browse.isEnabled, "No enabled Browse action without a touch destination")
    }

    private func openDeckMenu(_ id: String, app: XCUIApplication) throws {
        let pageMenu = element(app, "home.deck.actions")
        if pageMenu.exists {
            try tap("home.deck.actions", app: app)
        } else {
            let rows = HomeControls.deckRows(app, deckID: id)
            XCTAssertTrue(wait { HomeControls.hittable(rows) != nil })
            try XCTUnwrap(HomeControls.hittable(rows)).press(forDuration: 1.2)
        }
        XCTAssertTrue(element(app, "home.deckMenu.study." + id).waitForExistence(timeout: 10))
    }

    private func selectDeck(_ name: String, app: XCUIApplication) throws -> String {
        // Follow the native Decks tab / Today sidebar entry, including a sidebar
        // the split view has hidden. Do not navigate through a test-hook route.
        let home = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["shell.root.tab.decks", "shell.root.sidebar.home"]))
        let decksTab = app.tabBars.buttons["Decks"].firstMatch
        if HomeControls.hittable(home) == nil && !decksTab.isHittable {
            let toggle = app.buttons["Toggle Sidebar"].firstMatch
            if toggle.exists && toggle.isHittable { toggle.tap() }
        }
        XCTAssertTrue(wait { HomeControls.hittable(home) != nil || decksTab.isHittable }, deckRowDiagnostics(app))
        let entry = try XCTUnwrap(HomeControls.hittable(home) ?? (decksTab.isHittable ? decksTab : nil))
        entry.tap()

        // SwiftUI may expose the same sidebar row on several element types.
        // Accept appended count labels, but exclude subdecks and name lookalikes.
        let rows = HomeControls.deckRows(app, labelContaining: name).matching(NSPredicate(
            format: "label == %@ OR label BEGINSWITH %@", name, name + ","))
        XCTAssertTrue(wait { HomeControls.hittable(rows) != nil }, deckRowDiagnostics(app))
        let ids = Set(rows.allElementsBoundByIndex.compactMap { HomeControls.deckID($0.identifier) })
        XCTAssertEqual(ids.count, 1, "Expected one synthetic deck named \(name). \(deckRowDiagnostics(app))")
        let row = try XCTUnwrap(HomeControls.hittable(rows), deckRowDiagnostics(app))
        let id = try XCTUnwrap(HomeControls.deckID(row.identifier))
        row.tap()
        XCTAssertTrue(element(app, "home.deck.page." + id).waitForExistence(timeout: 15))
        // Compact navigation removes the row from the tree; retain its ID now.
        return id
    }

    private func deckRowDiagnostics(_ app: XCUIApplication) -> String {
        let rows = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@",
            HomeControls.rowPrefix, HomeControls.sidebarPrefix)).allElementsBoundByIndex
        return "Deck rows: " + rows.map {
            "type=\($0.elementType) label=\($0.label.debugDescription) id=\($0.identifier.debugDescription) hittable=\($0.isHittable)"
        }.joined(separator: "\n")
    }

    private func visible(_ id: String, app: XCUIApplication) throws -> XCUIElement {
        let hideKeyboard = app.keyboards.buttons.matching(NSPredicate(
            format: "label IN %@", ["Hide keyboard", "Hide Keyboard"])).firstMatch
        if hideKeyboard.exists && hideKeyboard.isHittable {
            hideKeyboard.tap()
            XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5))
        }
        let target = element(app, id)
        // Forms are ordinary scrollable controls; find the field in either direction
        // when revisiting a sheet whose first search is already above the viewport.
        let filtered = element(app, "home.filteredStudy.sheet")
        let custom = element(app, "home.customStudy.sheet")
        let scroll = filtered.exists ? filtered : custom.exists ? custom : app
        for _ in 0..<8 {
            if target.exists && target.isHittable { break }
            try scrollForm(scroll, up: true, app: app)
        }
        for _ in 0..<8 {
            if target.exists && target.isHittable { break }
            try scrollForm(scroll, up: false, app: app)
        }
        XCTAssertTrue(wait { target.exists && target.isHittable }, id)
        return target
    }

    private func scrollForm(_ container: XCUIElement, up: Bool, app: XCUIApplication) throws {
        // As in the native transfer journey, keep the gesture in the Form's
        // viewport. A sheet-centred swipe can land on the software keyboard.
        let form = container.collectionViews.firstMatch
        var viewport = (form.exists ? form.frame : container.frame).intersection(app.frame)
        let keyboard = app.keyboards.firstMatch
        if keyboard.exists && viewport.intersects(keyboard.frame) {
            viewport.size.height = max(0, min(viewport.maxY, keyboard.frame.minY) - viewport.minY)
        }
        XCTAssertFalse(viewport.isNull || viewport.isEmpty, "Visible native form viewport")
        let origin = app.coordinate(withNormalizedOffset: .zero)
        let top = origin.withOffset(CGVector(dx: viewport.midX - app.frame.minX,
            dy: viewport.minY - app.frame.minY + viewport.height * 0.2))
        let bottom = origin.withOffset(CGVector(dx: viewport.midX - app.frame.minX,
            dy: viewport.minY - app.frame.minY + viewport.height * 0.8))
        (up ? bottom : top).press(forDuration: 0.05, thenDragTo: up ? top : bottom)
    }

    private func tap(_ id: String, app: XCUIApplication) throws {
        let target = try visible(id, app: app)
        XCTAssertTrue(wait { target.isEnabled }, id)
        XCTAssertFalse(target.label.isEmpty, "Named native action: " + id)
        target.tap()
    }

    private func replace(_ id: String, with text: String, app: XCUIApplication) throws {
        let target = try visible(id, app: app)
        XCTAssertTrue(target.isEnabled, id)
        target.tap()
        let previous = target.value as? String ?? ""
        target.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count) + text)
        XCTAssertEqual(target.value as? String, text, id)
    }

    /// iOS does not preserve identifiers on SwiftUI Picker options. Select
    /// within the native menu by the form's declared enum order, with its exact
    /// option count asserted; no translated menu captions or Mac menu bar.
    private func chooseOption(_ id: String, index: Int, count: Int, app: XCUIApplication) throws {
        try tap(id, app: app)
        var options: XCUIElementQuery?
        XCTAssertTrue(wait {
            options = app.collectionViews.allElementsBoundByIndex.first { menu in
                let items = menu.buttons.allElementsBoundByIndex
                return items.count == count && items.contains { $0.isSelected } && items.allSatisfy { $0.identifier.isEmpty }
            }?.buttons
            return options != nil
        }, id + " native option count and selection")
        let choice = try XCTUnwrap(options).element(boundBy: index)
        XCTAssertTrue(choice.isEnabled && choice.isHittable)
        choice.tap()
    }

    private func launch(sizeClass: String) throws -> XCUIApplication {
        #if !targetEnvironment(simulator)
        throw XCTSkip("MG stages the generated recipe in the simulator VM")
        #endif
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("Synthetic development container required") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/filtered-study-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", sizeClass]).arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 25))
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        // Filtered actions share ids between the page and its native menu.
        // Scope actions exclusively to the native menu or collection view
        // containing that deck's Study item, even when the action is absent.
        if let deck = id.split(separator: ".").last, Int64(deck) != nil {
            for containers in [app.menus, app.collectionViews] {
                let menu = containers.containing(.any, identifier: "home.deckMenu.study." + String(deck)).firstMatch
                if menu.exists {
                    return menu.descendants(matching: .any).matching(identifier: id).firstMatch
                }
            }
        }
        let matches = app.descendants(matching: .any).matching(identifier: id)
        return HomeControls.hittable(matches) ?? matches.firstMatch
    }

    private func wait(_ condition: @MainActor () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(15)
        while !condition() {
            guard Date() < deadline else { return false }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return true
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
