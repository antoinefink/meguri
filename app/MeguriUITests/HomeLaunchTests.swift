import Foundation
import XCTest

/// A cold start of the production Home (native-interface-contract N27): from its first
/// frame until the decks show, Home never draws the welcome or "No Decks". The app's own
/// trace (`-MeguriHomeTrace on`, `testhooks.home.phases`) lists every phase it drew, in
/// order, so a phase shorter than a hierarchy poll is still seen; screenshots are not used.
@MainActor
final class HomeLaunchTests: XCTestCase {
    /// First run with this collection: nothing kept, so the page waits in its own background.
    func testColdStartWithoutSnapshotDrawsNoWelcomeOrEmptyStateBeforeTheDecks() throws {
        let app = try launch(installingFixture: true, snapshot: false)
        let phases = try phasesOnceDecksShow(app)
        XCTAssertEqual(phases.first, "pending", "Nothing is known at launch: \(phases)")
        try assertNoFalseStateBeforeDecks(phases)
        XCTAssertFalse(app.descendants(matching: .any)["home.collection.pending"].exists, "The waiting page goes with the decks")
    }

    /// The next launch draws the kept decks in its first frame and replaces them in place.
    func testColdStartWithSnapshotDrawsTheKeptDecksFirst() throws {
        let first = try launch(installingFixture: true, snapshot: true)
        let row = try deckRow(first)
        let rowID = row.identifier
        // The snapshot is written off the main thread just after the read.
        Thread.sleep(forTimeInterval: 1)
        first.terminate()

        // Not -MeguriFixture again: it installs a new file, which no snapshot names.
        let app = try launch(installingFixture: false, snapshot: true)
        let phases = try phasesOnceDecksShow(app)
        XCTAssertEqual(phases.first, "kept", "The kept read is the first frame: \(phases)")
        XCTAssertFalse(phases.contains("pending"), "Nothing blank between the kept and the read decks: \(phases)")
        try assertNoFalseStateBeforeDecks(phases)
        // The same row, under the same identifier, before and after the read.
        XCTAssertTrue(app.buttons[rowID].exists)
        XCTAssertEqual(app.buttons.matching(identifier: rowID).count, 1)
    }

    // MARK: Support

    private func launch(installingFixture: Bool, snapshot: Bool) throws -> XCUIApplication {
        continueAfterFailure = false
        #if os(macOS)
        throw XCTSkip("The cold start is checked on the iPhone, in the test VM")
        #else
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let fixture = installingFixture ? build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1") : nil
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false,
            // The tab layout's list on every device class (an iPad's sidebar rows have other identifiers).
            extra: ["-MeguriSizeClass", "compact", "-MeguriHomeTrace", "on", "-MeguriHomeSnapshot", snapshot ? "on" : "off"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        return app
        #endif
    }

    private func deckRow(_ app: XCUIApplication) throws -> XCUIElement {
        let rows = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.row."))
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 20), "Stage the generated local-review-1 fixture in the VM")
        return rows.firstMatch
    }

    /// The phases the app drew, read once the read decks show.
    private func phasesOnceDecksShow(_ app: XCUIApplication) throws -> [String] {
        _ = try deckRow(app)
        let trace = app.descendants(matching: .any)["testhooks.home.phases"]
        XCTAssertTrue(trace.waitForExistence(timeout: 10))
        let read = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label ENDSWITH %@", "decks"), object: trace)
        XCTAssertEqual(XCTWaiter.wait(for: [read], timeout: 20), .completed, "The read decks never showed: \(trace.label)")
        let phases = trace.label.split(separator: ",").map(String.init)
        let attachment = XCTAttachment(string: phases.joined(separator: " → "))
        attachment.name = "home-phases"
        attachment.lifetime = .keepAlways
        add(attachment)
        return phases
    }

    private func assertNoFalseStateBeforeDecks(_ phases: [String]) throws {
        let decks = try XCTUnwrap(phases.firstIndex(of: "decks"), "\(phases)")
        for phase in phases[..<decks] {
            XCTAssertFalse(["welcome", "noDecks", "failed", "unavailable"].contains(phase), "\(phase) drawn before the decks: \(phases)")
        }
    }
}
