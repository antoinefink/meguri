import Foundation
import XCTest

/// `sync/status-strings` T22.3 and native-interface-contract N23.6, on the iPhone (and the
/// folded Duo, which runs the same compact layout): the sync button is the first item of the
/// trailing group with search and Add on the deck list and a deck page, at the top and after
/// scrolling to the bottom; no bottom bar or empty item appears; a tap syncs without opening
/// anything in the ordinary states and opens the panel when the state needs the person.
/// Over the generated typical recipe, with the status drawn by `-MeguriSyncRender`.
@MainActor
final class SyncButtonPlacementTests: XCTestCase {
    func testButtonStaysInTheTrailingGroupOnTheListAndADeckPage() throws {
        let app = try launch(state: "meguriConnected")
        let sync = app.buttons["sync.status.button"]
        XCTAssertTrue(sync.waitForExistence(timeout: 30), "Stage the generated typical-1 fixture in the VM")
        let search = app.buttons["home.collection.search"]
        let add = app.buttons["home.collection.add"]
        try assertGroup([sync, search, add], "the deck list at the top")
        assertNoBottomBarOrEmptyItem(app, "the deck list at the top")

        app.otherElements["home.collection.screen"].firstMatch.swipeUp()
        app.otherElements["home.collection.screen"].firstMatch.swipeUp()
        XCTAssertTrue(sync.waitForExistence(timeout: 5))
        try assertGroup([sync, search, add], "the deck list scrolled to the bottom")
        assertNoBottomBarOrEmptyItem(app, "the deck list scrolled to the bottom")

        app.otherElements["home.collection.screen"].firstMatch.swipeDown()
        app.otherElements["home.collection.screen"].firstMatch.swipeDown()
        let languages = app.buttons["home.deck.row.1884308400001"]
        XCTAssertTrue(languages.waitForExistence(timeout: 10))
        languages.tap()
        let addNote = app.buttons["home.deck.addNote"]
        XCTAssertTrue(addNote.waitForExistence(timeout: 10), "The deck page opened")
        XCTAssertTrue(sync.waitForExistence(timeout: 5), "A deck page carries the sync button too")
        try assertGroup([sync, addNote], "a deck page")
        assertNoBottomBarOrEmptyItem(app, "a deck page")
    }

    /// T22.1: up to date, a tap syncs and opens nothing; needs you, a tap opens the panel.
    func testTapSyncsOrOpensThePanelByState() throws {
        let app = try launch(state: "meguriConnected")
        let sync = app.buttons["sync.status.button"]
        XCTAssertTrue(sync.waitForExistence(timeout: 30), "Stage the generated typical-1 fixture in the VM")
        XCTAssertEqual(sync.label, "Sync")
        sync.tap()
        XCTAssertFalse(app.otherElements["sync.status.popover"].waitForExistence(timeout: 3),
            "A tap in the ordinary states opens nothing")
        app.terminate()

        let needsYou = try launch(state: "meguriNeedsYou")
        let button = needsYou.buttons["sync.status.button"]
        XCTAssertTrue(button.waitForExistence(timeout: 30))
        button.tap()
        XCTAssertTrue(needsYou.otherElements["sync.status.popover"].waitForExistence(timeout: 10),
            "A state that needs the person opens the panel")
    }

    /// The items sit side by side in one group, in this order: one row (iPhone) or one
    /// column (the folded Duo's vertical capsule), each next to the one before.
    private func assertGroup(_ items: [XCUIElement], _ place: String) throws {
        let frames = items.map(\.frame)
        for (item, frame) in zip(items, frames) {
            XCTAssertTrue(item.isHittable, "\(item.identifier) is on screen on \(place)")
            XCTAssertFalse(frame.isEmpty, "\(item.identifier) has a frame on \(place)")
        }
        let row = frames.allSatisfy { abs($0.midY - frames[0].midY) < 4 }
        let column = frames.allSatisfy { abs($0.midX - frames[0].midX) < 4 }
        XCTAssertTrue(row || column, "The sync button shares the row or column of its group on \(place): \(frames)")
        for (before, after) in zip(frames, frames.dropFirst()) {
            let gap = row ? after.minX - before.maxX : after.minY - before.maxY
            XCTAssertGreaterThanOrEqual(gap, -1, "In order, the sync button first, on \(place): \(frames)")
            XCTAssertLessThan(gap, max(before.width, before.height), "Next to each other, one group, on \(place): \(frames)")
        }
    }

    /// The empty capsule Antoine saw was a bottom bar holding the moved item: the Decks
    /// stack has no bottom toolbar, and every navigation bar button says what it is.
    private func assertNoBottomBarOrEmptyItem(_ app: XCUIApplication, _ place: String) {
        XCTAssertEqual(app.toolbars.count, 0, "No bottom bar on \(place)")
        for button in app.navigationBars.buttons.allElementsBoundByIndex where button.exists {
            XCTAssertFalse(button.label.isEmpty && button.identifier.isEmpty, "An empty bar item on \(place): \(button.frame)")
        }
    }

    private func launch(state: String) throws -> XCUIApplication {
        continueAfterFailure = false
        #if os(macOS)
        throw XCTSkip("The tab layout's Decks stack is an iOS layout")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Test hooks and an isolated development test container are required")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", "compact", "-MeguriSyncRender", state]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
        #endif
    }
}
