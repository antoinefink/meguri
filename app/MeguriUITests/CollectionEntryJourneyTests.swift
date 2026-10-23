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
    /// RF3 journeys use the same public source and real review path as RF2.
    /// Run on Mac, iPhone and iPad in the VM, with no cloud account required.
    func testNativeExportCancelSaveReplaceReimportAndDurableReview() throws {
        try exercisePickedPackage(hasLocalCollection: false)
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        let app = XCUIApplication()
        let settings = try XCTUnwrap(waitForShellSection(app, tabIdentifier: "shell.root.tab.settings",
            sidebarIdentifier: "shell.root.sidebar.settings", compactLabel: "Settings", timeout: 10), app.debugDescription)
        try click(settings, app, bundle)
        let settingsContent = element(app, "settings.root.content")
        XCTAssertTrue(settingsContent.waitForExistence(timeout: 10), "Native navigation must open Settings: \(app.debugDescription)")
        let export = app.buttons["settings.root.exportCollection"].firstMatch
        let exportReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            settingsContent.exists && export.exists && export.isEnabled && export.isHittable
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [exportReady], timeout: 10), .completed, app.debugDescription)
        capture(app, "export-native-settings-route")
        try click(export, app, bundle)
        let prepare = try XCTUnwrap(waitForHittable(app.buttons.matching(identifier: "settings.importExport.prepare")), app.debugDescription)
        try click(prepare, app, bundle)
        XCTAssertTrue(element(app, "settings.importExport.ready").waitForExistence(timeout: 30), app.debugDescription)
        try click(element(app, "settings.importExport.choose"), app, bundle)
        capturePicker(app)
        #if os(macOS)
        let savePanel = app.sheets.matching(identifier: "save-panel").firstMatch
        XCTAssertTrue(savePanel.waitForExistence(timeout: 10))
        try click(savePanel.buttons.matching(NSPredicate(format: "identifier == %@ OR title == %@", "CancelButton", "Cancel")).firstMatch, app, bundle)
        let folder = FileManager.default.temporaryDirectory.appending(path: "RF3-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let saved = folder.appending(path: "Meguri-Collection.colpkg")
        #else
        let cancel = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Cancel")).firstMatch
        let cancelReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            cancel.exists && cancel.isEnabled && cancel.isHittable
        }, object: nil)
        let cancelResult = XCTWaiter().wait(for: [cancelReady], timeout: 10)
        if cancelResult != .completed {
            capturePicker(app)
            func attributes(_ target: XCUIElement) -> String {
                "elementType=\(target.elementType) label=\(target.label.debugDescription) identifier=\(target.identifier.debugDescription) exists=\(target.exists) isEnabled=\(target.isEnabled) isHittable=\(target.isHittable) frame=\(target.frame)"
            }
            let cancels = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Cancel"))
            let buttons = app.buttons.matching(NSPredicate(format: "label IN %@ OR identifier IN %@",
                ["Cancel", "Close", "Save", "More"], ["Cancel", "Close", "Save", "More", "DOCPicker.actionButton"]))
            let cancelRows = cancels.allElementsBoundByIndex.map { "exact Cancel: \(attributes($0))" }
            let buttonRows = buttons.allElementsBoundByIndex.map { "native button: \(attributes($0))" }
            let rows = (cancelRows.isEmpty ? ["No exact Cancel candidates"] : cancelRows)
                + (buttonRows.isEmpty ? ["No named native button candidates"] : buttonRows)
            let diagnostic = XCTAttachment(string: rows.joined(separator: "\n"))
            diagnostic.name = "export-cancel-postwait-native-attributes"
            diagnostic.lifetime = .keepAlways
            add(diagnostic)
        }
        XCTAssertEqual(cancelResult, .completed, app.debugDescription)
        try click(cancel, app, bundle)
        // The native local folder is installed by the existing public source hook.
        let saved = URL(fileURLWithPath: "/RF2 Import Tests/Meguri-Collection.colpkg")
        #endif
        XCTAssertTrue(element(app, "settings.importExport.ready").waitForExistence(timeout: 10))
        for replacement in [false, true] {
            try click(element(app, "settings.importExport.choose"), app, bundle)
            #if os(macOS)
            XCTAssertTrue(savePanel.waitForExistence(timeout: 10))
            try app.bringToFront(bundleIdentifier: bundle)
            app.typeKey("g", modifierFlags: [.command, .shift])
            let go = savePanel.sheets.matching(identifier: "GoToWindow").firstMatch
            let path = go.textFields.matching(identifier: "PathTextField").firstMatch
            XCTAssertTrue(path.waitForExistence(timeout: 10), app.debugDescription)
            try app.bringToFront(bundleIdentifier: bundle); path.typeKey("a", modifierFlags: .command)
            try app.bringToFront(bundleIdentifier: bundle); path.typeText(folder.path)
            XCTAssertEqual(path.value as? String, folder.path, app.debugDescription)
            try app.bringToFront(bundleIdentifier: bundle); path.typeKey(.return, modifierFlags: [])
            let save = savePanel.buttons.matching(identifier: "OKButton").firstMatch
            let saveReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                !go.exists && save.exists && save.isEnabled && save.isHittable
            }, object: nil)
            XCTAssertEqual(XCTWaiter().wait(for: [saveReady], timeout: 10), .completed, app.debugDescription)
            try click(save, app, bundle)
            if replacement {
                let replace = app.buttons.matching(NSPredicate(format: "title == %@ OR label == %@", "Replace", "Replace")).firstMatch
                XCTAssertTrue(replace.waitForExistence(timeout: 10), app.debugDescription)
                try click(replace, app, bundle)
            }
            #else
            try selectLocalExportFolder(app, bundle)
            let save = app.buttons.matching(NSPredicate(format: "label IN %@", ["Export", "Save", "Move", "Copy"])).firstMatch
            XCTAssertTrue(save.waitForExistence(timeout: 10), app.debugDescription)
            capturePicker(app)
            try click(save, app, bundle)
            if replacement {
                let replace = app.buttons.matching(NSPredicate(format: "label == %@", "Replace")).firstMatch
                XCTAssertTrue(replace.waitForExistence(timeout: 10), app.debugDescription)
                try click(replace, app, bundle)
            }
            #endif
            XCTAssertTrue(element(app, "settings.importExport.saved").waitForExistence(timeout: 30), app.debugDescription)
            #if os(macOS)
            XCTAssertTrue(FileManager.default.fileExists(atPath: saved.path), "NSSavePanel must save the expected collection filename at \(saved.path)")
            #endif
        }
        let discard = app.buttons["settings.importExport.discard"].firstMatch
        try click(discard, app, bundle)
        let discarded = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            prepare.exists && prepare.isEnabled && prepare.isHittable && !discard.exists
        }, object: nil)
        let discardResult = XCTWaiter().wait(for: [discarded], timeout: 10)
        if discardResult != .completed { capturePicker(app) }
        XCTAssertEqual(discardResult, .completed, app.debugDescription)
        let done = waitForHittable(app.buttons.matching(identifier: "settings.importExport.close"))
        if done == nil { capturePicker(app) }
        try click(try XCTUnwrap(done, app.debugDescription), app, bundle)
        let exportContent = element(app, "settings.importExport.content")
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: exportContent)
        let dismissResult = XCTWaiter().wait(for: [dismissed], timeout: 10)
        if dismissResult != .completed { capturePicker(app) }
        XCTAssertEqual(dismissResult, .completed, app.debugDescription)
        let importCollection = app.buttons["settings.root.importCollection"].firstMatch
        let importReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            settingsContent.exists && importCollection.exists && importCollection.isEnabled && importCollection.isHittable
        }, object: nil)
        let importResult = XCTWaiter().wait(for: [importReady], timeout: 10)
        if importResult != .completed { capturePicker(app) }
        XCTAssertEqual(importResult, .completed, app.debugDescription)
        try click(importCollection, app, bundle)
        let chooseFile = app.buttons["onboarding.collectionImport.choose"].firstMatch
        let chooseReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            chooseFile.exists && chooseFile.isEnabled && chooseFile.isHittable
        }, object: nil)
        let chooseResult = XCTWaiter().wait(for: [chooseReady], timeout: 10)
        if chooseResult != .completed { capturePicker(app) }
        XCTAssertEqual(chooseResult, .completed, app.debugDescription)
        try click(chooseFile, app, bundle)
        try pick(saved, app, bundle)
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.counts"), "6 notes · 11 cards · 7 reviews"))
        try click(element(app, "onboarding.collectionImport.confirm"), app, bundle)
        let imported = app.buttons.matching(NSPredicate(format: "identifier == %@", "home.deck.row.1788249600212"))
        // Return to Home through the production shell after native replacement.
        let home = try XCTUnwrap(waitForShellSection(app, tabIdentifier: "shell.root.tab.decks",
            sidebarIdentifier: "shell.root.sidebar.home", compactLabel: "Decks", timeout: 30), app.debugDescription)
        try click(home, app, bundle)
        try waitForAdmission(imported, app, bundle)
        let deck = try XCTUnwrap(waitForHittable(imported)); try click(deck, app, bundle)
        try click(element(app, "home.deck.study"), app, bundle)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        try click(element(app, "review.session.reveal"), app, bundle)
        try click(element(app, "review.session.rate.easy"), app, bundle)
        XCTAssertTrue(element(app, "review.session.close").waitForExistence(timeout: 20))
        try click(element(app, "review.session.close"), app, bundle)
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"))
        app.terminate()
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answers recorded"))
        capture(app, "export-reimport-durable-review")
    }
    #if os(iOS)
    private func selectLocalExportFolder(_ app: XCUIApplication, _ bundle: String) throws {
        let folderActions = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].buttons
            .matching(NSPredicate(format: "label == %@", "RF2 Import Tests, Actions Menu")).firstMatch
        let isInFolder = { app.navigationBars["RF2 Import Tests"].exists || folderActions.exists }
        if isInFolder() { return }
        let folder = app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@ OR identifier == %@",
            ["RF2 Import Tests", "RF2 Import Tests, Folder"], "RF2 Import Tests, Folder")).firstMatch
        if !folder.waitForExistence(timeout: 2) {
            let local = app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@", ["On My iPhone", "On My iPad"])).firstMatch
            if !local.exists {
                let browse = app.buttons.matching(NSPredicate(format: "label IN %@", ["Browse", "Back"])).firstMatch
                if browse.exists { try click(browse, app, bundle) }
            }
            XCTAssertTrue(local.waitForExistence(timeout: 10), app.debugDescription); try click(local, app, bundle)
            let meguri = app.cells.containing(NSPredicate(format: "label == %@", "Meguri")).firstMatch
            if meguri.waitForExistence(timeout: 2) { try click(meguri, app, bundle) }
        }
        if folder.waitForExistence(timeout: 2) {
            let target = folder.images.firstMatch.exists ? folder.images.firstMatch : folder
            try click(target, app, bundle)
        }
        let folderReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in isInFolder() }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [folderReady], timeout: 10), .completed, app.debugDescription)
        capturePicker(app)
    }
    #endif
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
        XCTAssertTrue(app.descendants(matching: .any)["onboarding.collectionImport.content"].firstMatch.staticTexts.matching(NSPredicate(
            format: "label CONTAINS %@ OR value CONTAINS %@", "collection-only .colpkg", "collection-only .colpkg")).firstMatch.waitForExistence(timeout: 10))
        try click(element(app, "onboarding.collectionImport.choose"), app, bundle)
        try pick(source, app, bundle)
        let confirm = element(app, "onboarding.collectionImport.confirm")
        XCTAssertTrue(confirm.waitForExistence(timeout: 30), app.debugDescription)
        let entryContent = element(app, "onboarding.collectionImport.content")
        XCTAssertTrue(entryContent.exists)
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
            predicate: NSPredicate(format: "exists == false"), object: entryContent)], timeout: 30), .completed)
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
        let hidden = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            NSRunningApplication.runningApplications(withBundleIdentifier: bundle).contains {
                !$0.isTerminated && $0.isHidden
            }
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [hidden], timeout: 10), .completed)
        app.activate()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 10))
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
        let panel = app.sheets.matching(identifier: "open-panel").firstMatch
        guard panel.waitForExistence(timeout: 10) else {
            capturePicker(app)
            XCTFail("Expected the native open-panel sheet")
            return
        }
        capturePicker(app)
        guard FileManager.default.fileExists(atPath: source.path) else {
            capturePicker(app)
            XCTFail("Public collection package is missing from the VM at \(source.path)")
            return
        }
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("g", modifierFlags: [.command, .shift])
        let goToFolder = panel.sheets.matching(identifier: "GoToWindow").firstMatch
        let path = goToFolder.textFields.matching(identifier: "PathTextField").firstMatch
        guard goToFolder.waitForExistence(timeout: 10), path.waitForExistence(timeout: 10) else {
            capturePicker(app)
            XCTFail("Expected GoToWindow with its PathTextField in the native Open panel")
            return
        }
        try click(path, app, bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        path.typeKey("a", modifierFlags: .command)
        try app.bringToFront(bundleIdentifier: bundle)
        path.typeText(source.path)
        guard path.value as? String == source.path else {
            capturePicker(app)
            XCTFail("Go To Folder did not contain the exact public collection package path")
            return
        }
        try app.bringToFront(bundleIdentifier: bundle)
        path.typeKey(.return, modifierFlags: [])
        let open = panel.buttons.matching(identifier: "OKButton").firstMatch
        let resolved = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            !goToFolder.exists && open.exists && open.isEnabled && open.isHittable
        }, object: nil)
        guard XCTWaiter().wait(for: [resolved], timeout: 10) == .completed else {
            capturePicker(app)
            XCTFail("Public collection package path did not resolve to an enabled, hittable native Open button with GoToWindow dismissed")
            return
        }
        try click(open, app, bundle)
        #else
        let labels = [source.deletingPathExtension().lastPathComponent, source.lastPathComponent]
        let file = app.cells.containing(NSPredicate(format: "label IN %@", labels)).firstMatch
        let rememberedFile = file.waitForExistence(timeout: 2)
        capturePicker(app)
        // The picker remembers folders. iPad can keep its Browse sidebar up.
        if !rememberedFile {
            let dedicated = app.descendants(matching: .any).matching(NSPredicate(
                format: "identifier == %@ OR label == %@", "RF2 Import Tests, Folder", "RF2 Import Tests, Folder"))
            let appFolder = app.cells.containing(NSPredicate(format: "label == %@", "Meguri"))
            if waitForHittable(dedicated) == nil {
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
    private func waitForShellSection(_ app: XCUIApplication, tabIdentifier: String, sidebarIdentifier: String,
                                     compactLabel: String, timeout: TimeInterval) -> XCUIElement? {
        let identified = app.buttons.matching(NSPredicate(format: "identifier IN %@", [tabIdentifier, sidebarIdentifier]))
        #if os(iOS)
        let nativeTab = app.tabBars.buttons.matching(NSPredicate(format: "label == %@", compactLabel))
        #endif
        var target: XCUIElement?
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            target = identified.allElementsBoundByIndex.first { $0.exists && $0.isHittable }
            #if os(iOS)
            if target == nil {
                target = nativeTab.allElementsBoundByIndex.first { $0.exists && $0.isHittable }
            }
            #endif
            return target != nil
        }, object: nil)
        return XCTWaiter().wait(for: [ready], timeout: timeout) == .completed ? target : nil
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
