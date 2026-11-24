import Foundation
import XCTest

/// Browse's column chooser and the inspector's Info tab on the Mac (U272,
/// U274; `browser/workspace` V2.3, V4.1, V13; N21.3, N21.4), over the
/// generated typical recipe on the real core: the header's context menu
/// with every column checked or not, a column added and one removed, a
/// header dragged to another place and an edge dragged, a sort by a header
/// click; Notes mode's own columns; the layout read back after a relaunch;
/// then the Info tab for a reviewed card, a new card and several rows.
/// Each step attaches a capture (the screen while a menu is open).
@MainActor
final class MacBrowseColumnsTests: XCTestCase {
    #if os(macOS)
    func testColumnsAndInfoLight() throws { try columnsAndInfo(appearance: "light", relaunch: true) }
    func testColumnsAndInfoDark() throws { try columnsAndInfo(appearance: "dark", relaunch: false) }

    private func columnsAndInfo(appearance: String, relaunch: Bool) throws {
        var (app, bundle) = try launch(appearance: appearance, fixture: true)
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 3 }, "rows: \(table.outlineRows.count)")
        XCTAssertTrue(waitUntil { self.columns(app).hasPrefix("model noteFld,template,deck,cardDue,cardMod") },
                      "the standard set: \(columns(app))")

        // V2.3: the header's context menu lists every column, the set's checked.
        try openHeaderMenu(app, bundle, table)
        let interval = columnItem(app, "cardIvl")
        XCTAssertTrue(interval.waitForExistence(timeout: 10), "the column chooser")
        XCTAssertTrue(app.menuItems["Size Columns to Fit"].exists && app.menuItems["Reset to Standard Columns"].exists)
        screen(app, "header-menu-\(appearance)")
        interval.click()
        XCTAssertTrue(waitUntil { self.columns(app).hasPrefix("model noteFld,template,deck,cardDue,cardMod,cardIvl") },
                      "Interval added at the end: \(columns(app))")
        try openHeaderMenu(app, bundle, table)
        columnItem(app, "deck").click()
        XCTAssertTrue(waitUntil { self.columns(app).hasPrefix("model noteFld,template,cardDue,cardMod,cardIvl") },
                      "Deck removed: \(columns(app))")

        // R1135: a header dragged before another reorders the set.
        try app.bringToFront(bundleIdentifier: bundle)
        let source = header(app, "Interval"), target = header(app, "Card Type")
        XCTAssertTrue(source.waitForExistence(timeout: 10) && target.exists, "the headers")
        source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .click(forDuration: 0.6, thenDragTo: target.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.5)))
        XCTAssertTrue(waitUntil(timeout: 15) { self.columns(app).hasPrefix("model noteFld,cardIvl,template,cardDue,cardMod") },
                      "Interval moved before Card Type: \(columns(app))")

        // V2.3: an edge dragged sets the width.
        try app.bringToFront(bundleIdentifier: bundle)
        let due = header(app, "Due")
        let before = due.frame.width
        due.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5)).withOffset(CGVector(dx: -1, dy: 0))
            .click(forDuration: 0.4, thenDragTo: due.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5))
                .withOffset(CGVector(dx: 60, dy: 0)))
        XCTAssertTrue(waitUntil { self.header(app, "Due").frame.width > before + 20 }, "Due wider: \(before) → \(header(app, "Due").frame.width)")

        // V4.1: a click on a sortable header sorts by it, in its first direction.
        try app.bringToFront(bundleIdentifier: bundle)
        header(app, "Due").coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.5)).click()
        XCTAssertTrue(waitUntil(timeout: 15) { self.columns(app).contains("sort cardDue descending") }, "sorted by Due: \(columns(app))")
        sleep(1)
        capture(app, "customised-columns-\(appearance)")

        // V2.3: Notes mode keeps its own list.
        try app.bringToFront(bundleIdentifier: bundle)
        modeButton(app, "Notes").click()
        XCTAssertTrue(waitUntil(timeout: 15) { self.columns(app).hasPrefix("model noteFld,note,template,noteTags,noteCrt") },
                      "Notes mode's columns: \(columns(app))")
        sleep(1)
        capture(app, "notes-columns-\(appearance)")
        modeButton(app, "Cards").click()
        XCTAssertTrue(waitUntil(timeout: 15) { self.columns(app).hasPrefix("model noteFld,cardIvl,template,cardDue,cardMod") },
                      "back in Cards mode with its own list: \(columns(app))")

        if relaunch {
            // V2.4: the set and its sort are the collection's: a relaunch reads them back.
            app.terminate()
            (app, bundle) = try launch(appearance: appearance, fixture: false)
            try openBrowse(app, bundle)
            XCTAssertTrue(waitUntil(timeout: 30) {
                self.columns(app).hasPrefix("model noteFld,cardIvl,template,cardDue,cardMod")
                    && self.columns(app).contains("sort cardDue descending")
            }, "the layout after a relaunch: \(columns(app))")
            capture(app, "after-relaunch-\(appearance)")
        }

        // V13: the Info tab of a reviewed card.
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "browse.sidebar.state.review").click()
        XCTAssertTrue(waitUntil(timeout: 20) { (self.element(app, "browse.status.text").value as? String ?? "").contains(" of ") })
        press(resultsTable(app).outlineRows.element(boundBy: 0))
        try app.bringToFront(bundleIdentifier: bundle)
        tab(app, "Info").click()
        XCTAssertTrue(element(app, "cardInfo.history").waitForExistence(timeout: 15), "a reviewed card's history")
        let reviews = element(app, "cardInfo.row.reviews")
        XCTAssertTrue(reviews.exists && reviews.label == "Reviews", "one element per row: \(reviews.label) \(reviews.value ?? "")")
        XCTAssertTrue(element(app, "cardInfo.row.card_id").exists && element(app, "cardInfo.row.deck").exists)
        sleep(1)
        capture(app, "info-reviewed-\(appearance)")

        // A new card: its rows, no history.
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "browse.sidebar.state.review").click()
        element(app, "browse.sidebar.state.new").click()
        sleep(2)
        press(resultsTable(app).outlineRows.element(boundBy: 0))
        XCTAssertTrue(waitUntil(timeout: 15) { self.element(app, "cardInfo.row.position").exists && !self.element(app, "cardInfo.history").exists },
                      "a new card: Position, no history")
        sleep(1)
        capture(app, "info-new-\(appearance)")

        // Several rows: the summary, as the Note tab shows it.
        try app.bringToFront(bundleIdentifier: bundle)
        XCUIElement.perform(withKeyModifiers: .shift) { press(resultsTable(app).outlineRows.element(boundBy: 3)) }
        XCTAssertTrue(element(app, "browse.inspector.summary.title").waitForExistence(timeout: 10), "V7's summary on the Info tab")
        sleep(1)
        capture(app, "info-several-\(appearance)")
    }

    // MARK: Helpers

    private func launch(appearance: String, fixture: Bool) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: fixture ? build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1", directoryHint: .isDirectory) : nil,
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            windowSize: "1440x900", appearance: appearance).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        XCTAssertTrue(element(app, "shell.root.sidebar.browse").waitForExistence(timeout: 30),
                      "Stage the generated typical-1 fixture through tools/mg")
        return (app, bundle)
    }

    private func openBrowse(_ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "shell.root.sidebar.browse").click()
        XCTAssertTrue(resultsTable(app).waitForExistence(timeout: 20), "Browse's table")
    }

    /// A secondary click on the column header, above the first row.
    private func openHeaderMenu(_ app: XCUIApplication, _ bundle: String, _ table: XCUIElement) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let sortField = header(app, "Sort Field")
        if sortField.waitForExistence(timeout: 5) {
            // XCTest reads a SwiftUI table's header buttons as not hittable: clicked at their point.
            sortField.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).rightClick()
        } else {
            let row = table.outlineRows.element(boundBy: 0)
            row.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0)).withOffset(CGVector(dx: 0, dy: -12)).rightClick()
        }
    }

    /// The header's column button with that title.
    private func header(_ app: XCUIApplication, _ title: String) -> XCUIElement {
        resultsTable(app).buttons.matching(NSPredicate(format: "label == %@ OR title == %@", title, title)).firstMatch
    }

    /// A segment of a segmented control, by its title.
    private func segment(_ app: XCUIApplication, _ control: String, _ title: String) -> XCUIElement {
        element(app, control).descendants(matching: .any).matching(NSPredicate(format: "label == %@", title)).firstMatch
    }

    private func tab(_ app: XCUIApplication, _ title: String) -> XCUIElement { segment(app, "browse.inspector.tabs", title) }

    private func modeButton(_ app: XCUIApplication, _ title: String) -> XCUIElement { segment(app, "browse.toolbar.mode", title) }

    /// The open context menu's item for a column: View › Columns holds items of
    /// the same identifiers, so the one on screen is chosen.
    private func columnItem(_ app: XCUIApplication, _ key: String) -> XCUIElement {
        let items = app.menuItems.matching(identifier: "browse.columns.column.\(key)")
        _ = items.firstMatch.waitForExistence(timeout: 10)
        return items.allElementsBoundByIndex.first { $0.isHittable } ?? items.firstMatch
    }

    /// The header's report: the model's columns and sort, and the table's.
    private func columns(_ app: XCUIApplication) -> String {
        let probe = element(app, "browse.debug.columns")
        return (probe.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? probe.label
    }

    private func resultsTable(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.results.table").firstMatch
    }

    private func press(_ row: XCUIElement) {
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// The VM's screen: an open menu is a window of its own.
    private func screen(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    private func waitUntil(timeout: TimeInterval = 10, _ condition: @escaping () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }
    #endif
}
