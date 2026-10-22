import Foundation
import XCTest
#if os(macOS)
import AppKit
#else
import UIKit
import notify
#endif

/// Run in the VM through tools/mg ui-test --platform iphone|ipad|mac
/// --suite CollectionEntryJourneyTests. First-run picker hierarchy attachments
/// are required evidence; provider navigation uses visible system labels only.
@MainActor final class CollectionEntryJourneyTests: XCTestCase {
    func testPickerImportCreatesDurableReview() throws {
        try exercisePickedPackage(hasLocalCollection: false)
    }
    func testPickerReplacementCreatesDurableReview() throws {
        try exercisePickedPackage(hasLocalCollection: true)
    }
    private func exercisePickedPackage(hasLocalCollection: Bool) throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development source setup") }
        let source = build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/review.colpkg")
        let initial = hasLocalCollection
            ? build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1")
            : build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection")
        let app = XCUIApplication()
        #if os(iOS)
        let importSource: URL? = source
        #else
        let importSource: URL? = nil
        #endif
        app.launchArguments = MeguriLaunch(fixture: initial, importSource: importSource, now: "2030-01-15T10:00:00Z",
            timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        // Replacement may need RF1's scheduler admission before Home actions.
        let enable = element(app, "home.policy.enable")
        if hasLocalCollection && enable.waitForExistence(timeout: 5) { try click(enable, app, bundle) }
        let entry = element(app, "home.collection.import")
        XCTAssertTrue(entry.waitForExistence(timeout: 20))
        try click(entry, app, bundle)
        XCTAssertTrue(element(app, "entry.choose").waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "collection-only .colpkg")).firstMatch.exists)
        try click(element(app, "entry.choose"), app, bundle)
        try pick(source, app, bundle)
        let confirm = element(app, "entry.confirm")
        XCTAssertTrue(confirm.waitForExistence(timeout: 30), app.debugDescription)
        capture(app, "entry-preview-\(hasLocalCollection ? "replacement" : "first")")
        XCTAssertTrue(text(element(app, "entry.preview.counts")).contains("6 notes · 11 cards · 6 reviews"))
        let safety = text(element(app, "entry.preview.safety"))
        XCTAssertTrue(safety.contains(hasLocalCollection ? "safety backup" : "no local collection file"))
        XCTAssertEqual(confirm.label, hasLocalCollection ? "Replace Collection" : "Import Collection")
        #if os(iOS)
        notify_post("\(bundle).testhooks.toggleSizeClass")
        XCTAssertTrue(element(app, "entry.preview.counts").waitForExistence(timeout: 10))
        XCTAssertTrue(text(element(app, "entry.preview.counts")).contains("6 notes · 11 cards · 6 reviews"))
        capture(app, "entry-preview-adapted")
        #endif
        try click(confirm, app, bundle)
        // Real activation dismisses entry. FSRS policy is the production gate.
        let policyOrDeck = app.descendants(matching: .any).matching(NSPredicate(format:
            "identifier == %@ OR identifier BEGINSWITH %@", "home.policy.enable", "home.deck.row.")).firstMatch
        XCTAssertTrue(policyOrDeck.waitForExistence(timeout: 30), app.debugDescription)
        if enable.exists { try click(enable, app, bundle) }
        XCTAssertFalse(element(app, "entry.error").exists)
        XCTAssertFalse(element(app, "home.collection.error").exists)
        let deck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.row.")).firstMatch
        XCTAssertTrue(deck.waitForExistence(timeout: 20))
        capture(app, "entry-real-decks")
        try click(deck, app, bundle)
        let study = element(app, "home.deck.study")
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        let before = text(element(app, "home.deck.counts"))
        try click(study, app, bundle)
        let reveal = element(app, "review.session.reveal")
        XCTAssertTrue(reveal.waitForExistence(timeout: 20), "Public package must provide a real reviewable card: \(app.debugDescription)")
        try click(reveal, app, bundle)
        let easy = element(app, "review.session.rate.easy")
        XCTAssertTrue(easy.waitForExistence(timeout: 10))
        try click(easy, app, bundle)
        let settled = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["review.session.reveal", "review.session.refresh", "review.session.done"])).firstMatch
        XCTAssertTrue(settled.waitForExistence(timeout: 20))
        capture(app, "entry-real-answer")
        #if os(iOS)
        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackground, timeout: 10))
        app.activate()
        #else
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("h", modifierFlags: .command)
        #endif
        try app.bringToFront(bundleIdentifier: bundle)
        let close = element(app, "review.session.close")
        XCTAssertTrue(close.waitForExistence(timeout: 20))
        try click(close, app, bundle)
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"))
        let after = text(element(app, "home.deck.counts"))
        capture(app, "entry-saved-answer")
        app.terminate()
        // Reopen the installed collection without fixture/source/route setup.
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"))
        let restartedDeck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.row.")).firstMatch
        XCTAssertTrue(restartedDeck.waitForExistence(timeout: 20))
        try click(restartedDeck, app, bundle)
        XCTAssertTrue(waitForText(element(app, "home.deck.counts"), after))
        XCTAssertFalse(element(app, "home.collection.error").exists)
        capture(app, "entry-restart-durable-answer")
        XCTAssertFalse(before.isEmpty)
    }
    private func pick(_ source: URL, _ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        // Native Open panel, no file URL or app-private import route.
        let open = app.buttons["Open"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 10))
        capturePicker(app)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("g", modifierFlags: [.command, .shift])
        let path = app.textFields.firstMatch
        XCTAssertTrue(path.waitForExistence(timeout: 10))
        try click(path, app, bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        path.typeText(source.path)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        XCTAssertTrue(open.waitForExistence(timeout: 10))
        try click(open, app, bundle)
        #else
        let file = named(app, ["review", "review.colpkg"])
        let rememberedFile = file.waitForExistence(timeout: 2)
        capturePicker(app)
        // The picker remembers folders. iPad can keep its Browse sidebar up.
        if !rememberedFile {
            let dedicated = named(app, ["RF2 Import Tests"])
            if !dedicated.exists {
                let browse = app.buttons["Browse"].firstMatch
                if browse.exists { try click(browse, app, bundle) }
                let local = named(app, ["On My iPhone", "On My iPad"])
                if !local.exists {
                    let browseTab = app.tabBars.buttons["Browse"].firstMatch
                    if browseTab.exists { try click(browseTab, app, bundle) }
                }
                XCTAssertTrue(local.waitForExistence(timeout: 10), app.debugDescription)
                try click(local, app, bundle)
                let meguri = named(app, ["Meguri"])
                XCTAssertTrue(meguri.waitForExistence(timeout: 10), app.debugDescription)
                try click(meguri, app, bundle)
            }
            XCTAssertTrue(dedicated.waitForExistence(timeout: 10), app.debugDescription)
            try click(dedicated, app, bundle)
        }
        XCTAssertTrue(file.waitForExistence(timeout: 10), app.debugDescription)
        capturePicker(app)
        try click(file, app, bundle)
        #endif
    }
    private func named(_ app: XCUIApplication, _ labels: [String]) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@", labels)).firstMatch
    }
    private func click(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        target.clickOrTap()
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
    private func text(_ element: XCUIElement) -> String { (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label }
    private func waitForText(_ element: XCUIElement, _ expected: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format:
            "exists == true AND (label == %@ OR value == %@)", expected, expected), object: element)], timeout: 20) == .completed
    }
    private func capturePicker(_ app: XCUIApplication) {
        capture(app, "entry-system-picker")
        let hierarchy = XCTAttachment(string: app.debugDescription)
        hierarchy.name = "entry-system-picker-hierarchy"; hierarchy.lifetime = .keepAlways; add(hierarchy)
    }
    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
