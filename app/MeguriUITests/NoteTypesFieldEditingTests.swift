import Foundation
import XCTest

/// `notetypes/workspace` W4 in the running app: text typed in a field's Name
/// or Placeholder and not committed stays with that field when the selected
/// field changes, and the newly selected field's values are untouched. The
/// Mac edits in the inspector beside the table; iPad in the row's popover.
/// Over the test hooks' made-up note types (`notetypes/fields/typical`).
@MainActor
final class NoteTypesFieldEditingTests: XCTestCase {
    func testTypedTextStaysWithItsFieldWhenTheSelectionChanges() throws {
        let app = try launch()
        // Name: Reading gets "x" typed, then Meaning is selected.
        try open(app, row: 1)
        let name = element(app, "noteTypes.fields.inspector.name")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        XCTAssertEqual(name.value as? String, "Reading")
        try type(app, "x", into: name)
        try open(app, row: 2)
        XCTAssertTrue(waitForValue(name, "Meaning"), "the new field shows its own name: \(name.value ?? "nil")")
        XCTAssertTrue(waitForLabel(element(app, "noteTypes.fields.row.1"), "Readingx"), "the typed name landed on Reading")
        XCTAssertTrue(waitForLabel(element(app, "noteTypes.fields.row.2"), "Meaning"), "Meaning keeps its name")

        // Placeholder: Meaning gets "Hint" typed, then Word is selected.
        let placeholder = element(app, "noteTypes.fields.inspector.placeholder")
        XCTAssertTrue(placeholder.waitForExistence(timeout: 10))
        try type(app, "Hint", into: placeholder)
        try open(app, row: 0)
        XCTAssertTrue(waitForValue(name, "Word"))
        XCTAssertNotEqual(placeholder.value as? String, "Hint", "Word's placeholder is untouched")
        try open(app, row: 2)
        XCTAssertTrue(waitForValue(placeholder, "Hint"), "the typed placeholder landed on Meaning: \(placeholder.value ?? "nil")")
    }

    // MARK: Helpers

    private func launch() throws -> XCUIApplication {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("The routes need a hook build") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, route: "notetypes/fields/typical").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "noteTypes.fields.table").waitForExistence(timeout: 20))
        bundle = try build.bundleIdentifier
        return app
    }

    private var bundle = ""

    /// Selects a field: a table row on the Mac; on iPad, the popover closes
    /// (a tap outside it) and the row's own popover opens.
    private func open(_ app: XCUIApplication, row: Int) throws {
        let target = element(app, "noteTypes.fields.row.\(row)")
        XCTAssertTrue(target.waitForExistence(timeout: 10))
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        target.click()
        #else
        if element(app, "noteTypes.fields.inspector.name").exists {
            // A tap on the column header, outside the popover and on nothing that
            // acts, closes it and commits what it held.
            element(app, "noteTypes.fields.header").tap()
            XCTAssertTrue(element(app, "noteTypes.fields.inspector.name").waitForNonExistence(timeout: 10))
        }
        target.tap()
        XCTAssertTrue(element(app, "noteTypes.fields.inspector.name").waitForExistence(timeout: 10))
        #endif
    }

    private func type(_ app: XCUIApplication, _ text: String, into field: XCUIElement) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        field.click()
        app.typeKey(.rightArrow, modifierFlags: .command)
        #else
        field.tap()
        #endif
        field.typeText(text)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func waitForValue(_ element: XCUIElement, _ value: String, timeout: TimeInterval = 10) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", value), object: element)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }

    private func waitForLabel(_ element: XCUIElement, _ label: String, timeout: TimeInterval = 10) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", label), object: element)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }
}
