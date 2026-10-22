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
        if hasLocalCollection {
            let localDeck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.row."))
            try waitForAdmission(localDeck, app, bundle)
        }
        let entry = element(app, "home.collection.import")
        XCTAssertTrue(entry.waitForExistence(timeout: 20))
        try click(entry, app, bundle)
        XCTAssertTrue(element(app, "onboarding.collectionImport.choose").waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "collection-only .colpkg")).firstMatch.exists)
        try click(element(app, "onboarding.collectionImport.choose"), app, bundle)
        try pick(source, app, bundle)
        let confirm = element(app, "onboarding.collectionImport.confirm")
        XCTAssertTrue(confirm.waitForExistence(timeout: 30), app.debugDescription)
        let entrySheet = element(app, "onboarding.collectionImport.sheet")
        XCTAssertTrue(entrySheet.exists)
        assertPreview(app, hasLocalCollection: hasLocalCollection)
        capture(app, "entry-preview-\(hasLocalCollection ? "replacement" : "first")")
        #if os(iOS)
        let adapted = try toggleSizeClass(bundle)
        assertPreview(app, hasLocalCollection: hasLocalCollection)
        capture(app, "entry-preview-adapted")
        let restored = try toggleSizeClass(bundle)
        XCTAssertEqual(restored.before, adapted.after)
        XCTAssertEqual(restored.after, adapted.before)
        assertPreview(app, hasLocalCollection: hasLocalCollection)
        capture(app, "entry-preview-restored")
        #endif
        try click(confirm, app, bundle)
        // Real activation dismisses entry. FSRS policy is the production gate.
        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: entrySheet)], timeout: 30), .completed)
        let importedDeck = app.buttons.matching(NSPredicate(format: "identifier == %@", "home.deck.row.1788249600212"))
        try waitForAdmission(importedDeck, app, bundle)
        XCTAssertFalse(element(app, "onboarding.collectionImport.error").exists)
        XCTAssertFalse(element(app, "home.collection.error").exists)
        let deck = try XCTUnwrap(waitForHittable(importedDeck), app.debugDescription)
        let deckIdentifier = deck.identifier
        let deckID = try XCTUnwrap(Int64(deckIdentifier.dropFirst("home.deck.row.".count)))
        let deckPage = element(app, "home.deck.page.\(deckID)")
        capture(app, "entry-real-decks")
        try click(deck, app, bundle)
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        let study = deckPage.descendants(matching: .any)["home.deck.study"].firstMatch
        XCTAssertTrue(study.waitForExistence(timeout: 10))
        let counts = deckPage.descendants(matching: .any)["home.deck.counts"].firstMatch
        let before = text(counts)
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
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        let after = text(counts)
        capture(app, "entry-saved-answer")
        app.terminate()
        // Reopen the installed collection without fixture/source/route setup.
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"))
        let retainedDeck = app.buttons[deckIdentifier].firstMatch
        let restartReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            (deckPage.exists && study.isHittable) || (retainedDeck.exists && retainedDeck.isHittable)
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [restartReady], timeout: 20), .completed)
        if !(deckPage.exists && study.isHittable) {
            try click(retainedDeck, app, bundle)
        }
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForText(counts, after))
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
        let labels = ["review", "review.colpkg"]
        let file = app.cells.containing(NSPredicate(format: "label IN %@", labels)).firstMatch
        let rememberedFile = file.waitForExistence(timeout: 2)
        capturePicker(app)
        // The picker remembers folders. iPad can keep its Browse sidebar up.
        if !rememberedFile {
            let dedicated = app.descendants(matching: .any).matching(NSPredicate(
                format: "identifier == %@ OR label == %@", "RF2 Import Tests, Folder", "RF2 Import Tests, Folder"))
            let appFolder = app.cells.containing(NSPredicate(format: "label == %@", "Meguri"))
            if waitForHittable(dedicated, timeout: 2) == nil {
                let local = app.descendants(matching: .any).matching(NSPredicate(
                    format: "label IN %@", ["On My iPhone", "On My iPad"]))
                if !local.firstMatch.exists {
                    let browseTab = try XCTUnwrap(waitForHittable(
                        app.tabBars["DOC.browsingModeTabBar"].buttons.matching(NSPredicate(format: "label == %@", "Browse"))), app.debugDescription)
                    try click(browseTab, app, bundle)
                    capturePicker(app)
                }
                let location = try XCTUnwrap(waitForHittable(local), app.debugDescription)
                try click(location, app, bundle)
                capturePicker(app)
                // On My Device may open Meguri's folder directly. Only a native
                // file cell with the exact caption can select the app folder.
                let contents = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                    dedicated.allElementsBoundByIndex.contains { $0.exists && $0.isHittable }
                        || appFolder.allElementsBoundByIndex.contains { $0.exists && $0.isHittable }
                }, object: nil)
                XCTAssertEqual(XCTWaiter().wait(for: [contents], timeout: 20), .completed, app.debugDescription)
                capturePicker(app)
                if !dedicated.allElementsBoundByIndex.contains(where: { $0.exists && $0.isHittable }) {
                    let meguri = try XCTUnwrap(waitForHittable(appFolder), app.debugDescription)
                    try click(meguri, app, bundle)
                    capturePicker(app)
                }
            }
            let folder = try XCTUnwrap(waitForHittable(dedicated), app.debugDescription)
            capturePicker(app)
            let folderImage = folder.images.firstMatch
            if folderImage.waitForExistence(timeout: 10) {
                let image = try XCTUnwrap(waitForHittable(folder.images), app.debugDescription)
                try click(image, app, bundle)
            } else {
                try click(folder, app, bundle)
            }
        }
        XCTAssertTrue(file.waitForExistence(timeout: 10), app.debugDescription)
        capturePicker(app)
        let thumbnail = file.images.firstMatch
        if thumbnail.waitForExistence(timeout: 2) {
            try click(thumbnail, app, bundle)
        } else {
            try click(file, app, bundle)
        }
        #endif
    }
    private func click(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        target.clickOrTap()
    }
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
    private func text(_ element: XCUIElement) -> String { (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label }
    #if os(iOS)
    private func toggleSizeClass(_ bundle: String) throws -> (before: UIUserInterfaceSizeClass, after: UIUserInterfaceSizeClass) {
        var token = NOTIFY_TOKEN_INVALID
        XCTAssertEqual(notify_register_check("\(bundle).testhooks.sizeClassApplied", &token), UInt32(NOTIFY_STATUS_OK))
        defer { notify_cancel(token) }
        // Consume registration's initial signal before requesting a transition.
        var changed: Int32 = 0
        XCTAssertEqual(notify_check(token, &changed), UInt32(NOTIFY_STATUS_OK))
        var state: UInt64 = 0
        let applied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            var changed: Int32 = 0
            guard notify_check(token, &changed) == NOTIFY_STATUS_OK, changed != 0 else { return false }
            return notify_get_state(token, &state) == NOTIFY_STATUS_OK
        }, object: nil)
        XCTAssertEqual(notify_post("\(bundle).testhooks.toggleSizeClass"), UInt32(NOTIFY_STATUS_OK))
        XCTAssertEqual(XCTWaiter().wait(for: [applied], timeout: 10), .completed, "No applied window size-class transition")
        let before = try XCTUnwrap(UIUserInterfaceSizeClass(rawValue: Int(state >> 32)))
        let after = try XCTUnwrap(UIUserInterfaceSizeClass(rawValue: Int(state & 0xffff_ffff)))
        XCTAssertTrue(before == .compact || before == .regular)
        XCTAssertEqual(after, before == .compact ? .regular : .compact)
        return (before, after)
    }
    #endif
    private func assertPreview(_ app: XCUIApplication, hasLocalCollection: Bool) {
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.counts"), "6 notes · 11 cards · 6 reviews"))
        let safety = hasLocalCollection
            ? "This will replace the local collection on this device. Meguri must finish a safety backup of your current collection, including saved answers, before replacing it. If that backup fails, import stops."
            : "There is no local collection file to replace. A pre-import safety backup is not needed."
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.safety"), safety))
        XCTAssertEqual(element(app, "onboarding.collectionImport.confirm").label, hasLocalCollection ? "Replace Collection" : "Import Collection")
    }
    private func waitForAdmission(_ query: XCUIElementQuery, _ app: XCUIApplication, _ bundle: String) throws {
        let enable = element(app, "home.policy.enable")
        let policy = element(app, "home.policy.sheet")
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            enable.isHittable || (query.allElementsBoundByIndex.contains { $0.exists && $0.isHittable } && !policy.exists)
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed, app.debugDescription)
        if enable.isHittable {
            try click(enable, app, bundle)
            let admitted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                !policy.exists && query.allElementsBoundByIndex.contains { $0.exists && $0.isHittable }
            }, object: nil)
            XCTAssertEqual(XCTWaiter().wait(for: [admitted], timeout: 30), .completed, app.debugDescription)
        }
    }
    private func waitForHittable(_ query: XCUIElementQuery, timeout: TimeInterval = 20) -> XCUIElement? {
        var target: XCUIElement?
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            target = query.allElementsBoundByIndex.first { $0.exists && $0.isHittable }
            return target != nil
        }, object: nil)
        return XCTWaiter().wait(for: [ready], timeout: timeout) == .completed ? target : nil
    }
    private func waitForText(_ element: XCUIElement, _ expected: String) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format:
            "exists == true AND (label == %@ OR value == %@)", expected, expected), object: element)], timeout: 20) == .completed
    }
    private func capturePicker(_ app: XCUIApplication) {
        capture(app, "entry-system-picker")
        let hierarchy = XCTAttachment(string: app.debugDescription)
        hierarchy.name = "entry-system-picker-hierarchy"; hierarchy.lifetime = .keepAlways; add(hierarchy)
        #if os(iOS)
        // Files' remote provider can be absent from the app snapshot. Query
        // only the fixture/navigation targets live, with a fixed output cap.
        func attributes(_ target: XCUIElement) -> String {
            guard target.exists else { return "no longer exists" }
            return "type=\(target.elementType) id=\(target.identifier.debugDescription) label=\(target.label.debugDescription) hittable=\(target.isHittable)"
        }
        let targets = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier IN %@ OR label IN %@",
            ["RF2 Import Tests, Folder", "DOC.sidebar.item.On My iPad"],
            ["On My iPhone", "On My iPad", "Meguri", "RF2 Import Tests", "RF2 Import Tests, Folder", "review", "review.colpkg"]))
        var rows = targets.allElementsBoundByIndex.prefix(12).map { attributes($0) }
        let owners = app.cells.containing(NSPredicate(format: "label IN %@", ["Meguri", "review", "review.colpkg"]))
        for owner in owners.allElementsBoundByIndex.prefix(6) {
            rows.append("owning cell: \(attributes(owner))")
            for image in owner.images.allElementsBoundByIndex.prefix(1) {
                rows.append("image child: \(attributes(image))")
            }
        }
        let live = XCTAttachment(string: rows.isEmpty ? "No fixture/navigation query matches" : rows.joined(separator: "\n"))
        live.name = "entry-system-picker-live-targets"; live.lifetime = .keepAlways; add(live)
        #endif
    }
    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
