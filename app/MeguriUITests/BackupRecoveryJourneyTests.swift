import Foundation
import XCTest
#if os(iOS)
import UIKit
#endif

/// R1–R4 / U4 of the native backups plan. Run sequentially in the test VM:
/// tools/mg ui-test --platform iphone|ipad|mac --config Debug --suite BackupRecoveryJourneyTests
/// Uses the committed empty fixture, then only native collection operations.
@MainActor
final class BackupRecoveryJourneyTests: XCTestCase {
    private let originalFront = "Synthetic recovery question QZ71"
    private let originalBack = "Synthetic recovery answer QZ83"
    private let editMarker = "postbackupQZ92"
    private let entryPrefix = "settings.backups.entry."
    private let runID = UUID()
    private var launchedMinutes: Set<Int> = []
    private var bundle = ""

    func testManualBackupCancelRestoreAndRecoverSafetyCopyAcrossRelaunch() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        bundle = try build.bundleIdentifier
        guard build.configuration == .debug, bundle == "com.antoinefink.meguri.dev" else {
            throw XCTSkip("Requires Debug Meguri Dev in the test VM")
        }
        let app = XCUIApplication(bundleIdentifier: bundle)
        app.launchArguments = arguments(minute: 0, fixture: build.repositoryRoot.appending(
            path: "app/Fixtures/CollectionEntry/EmptyCollection"))
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        try act(element(app, "home.collection.startEmpty"), app)
        try openDeck(app)
        try act(element(app, "home.deck.empty.addNote"), app)
        let editor = element(app, "editor.add")
        try enter(originalFront, field: "Front", editor: editor, app: app)
        try enter(originalBack, field: "Back", editor: editor, app: app)
        try act(element(app, "editor.add.save"), app)
        #if os(macOS)
        XCTAssertTrue(wait { self.text(self.element(app, "editor.add.result")) == "Added to Default" })
        #else
        XCTAssertTrue(app.navigationBars.staticTexts["Added to Default"].firstMatch.waitForExistence(timeout: 20))
        #endif
        try act(element(app, "editor.add.close"), app)
        XCTAssertTrue(editor.waitForNonExistence(timeout: 20))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 1, learning: 0, review: 0))
        try inspectCard(app, front: originalFront)

        try openBackups(app)
        let older = try createBackup(app)
        try openHistory(app)
        XCTAssertTrue(wait { self.entryNames(app).contains(older) }, "The manual result must be in real history")
        try assertRunHistory(app)
        try cancelHistory(app)
        try capture(app, anchor: "settings.backups.create", state: "settings-manual-backup-created")

        // Separate fixed instants avoid same-second archive-name collisions;
        // every later launch reuses the run ID without reinstalling the fixture.
        try relaunch(app, minute: 1)
        try startReview(app)
        try assertRendered(app, originalFront)
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("e", modifierFlags: .command)
        #else
        try act(element(app, "review.session.more"), app)
        try act(app.buttons["Edit Note"], app)
        #endif
        let edit = element(app, "review.editCurrent")
        let front = field("Front", editor: edit)
        XCTAssertTrue(wait { front.exists && self.fieldValue(front) == self.originalFront })
        // Insertion position is a native text-system choice. Record the exact
        // committed result, then demand those bytes through cancel and recovery.
        try enter(editMarker, field: "Front", editor: edit, app: app, replacing: false)
        let changedFront = try XCTUnwrap(fieldValue(front))
        XCTAssertTrue(changedFront.contains(editMarker))
        XCTAssertNotEqual(changedFront, originalFront)
        try act(element(app, "review.editCurrent.done"), app)
        XCTAssertTrue(edit.waitForNonExistence(timeout: 20))
        try assertRendered(app, changedFront)
        try act(element(app, "review.session.close"), app)

        #if os(macOS)
        // Exercise both focused-main-window File commands, in addition to the
        // Settings route used for the first backup and the confirmed recovery.
        try fileCommand("Back Up Now", app)
        try openBackups(app)
        let newer = try createdFilename(app)
        XCTAssertNotEqual(newer, older)
        try closeSettings(app)
        try fileCommand("Restore from Backup…", app)
        try waitForHistory(app)
        #else
        try openBackups(app)
        let newer = try createBackup(app)
        XCTAssertNotEqual(newer, older)
        try openHistory(app)
        #endif
        XCTAssertTrue(wait { self.entryNames(app).isSuperset(of: [older, newer]) })
        let beforeCancel = entryNames(app)
        try select(older, app)
        try capture(app, anchor: "settings.backups.list", state: "history-older-backup-selected")
        let historyWindow = try captureWindow(app, anchor: "settings.backups.list")
        let outside = try confirmationDismissalPoint(historyWindow)
        let confirmation = try advanceToConfirmation(app)
        capture(historyWindow, state: "confirmation-restore-older-backup")
        try cancelConfirmation(confirmation, app, outside: outside)
        XCTAssertTrue(element(app, "settings.backups.cancel").waitForExistence(timeout: 10), "Cancellation keeps the restore sheet open")
        XCTAssertTrue(element(app, "settings.backups.list").waitForExistence(timeout: 10))
        XCTAssertTrue(wait { self.element(app, self.entryPrefix + older).isSelected },
            "Cancellation keeps the specifically selected older entry")
        XCTAssertEqual(entryNames(app), beforeCancel, "Cancelling confirmation must not create a safety backup")
        try cancelHistory(app)
        try returnToDecks(app)
        try inspectCard(app, front: changedFront)

        // Clean UI launch, no fixture reinstallation and no simulated restore.
        try relaunch(app, minute: 2)
        try openBackups(app)
        try openHistory(app)
        let beforeRestore = entryNames(app)
        XCTAssertTrue(beforeRestore.isSuperset(of: [older, newer]))
        // B4 allocates the first free second at the fixed restore instant.
        // A later Opened/CollectionReplaced automatic archive may also appear;
        // its name must never be mistaken for the pre-replacement safety copy.
        let safety = try XCTUnwrap(backupNames(minute: 2).first { !beforeRestore.contains($0) })
        try restore(older, app)
        try returnToDecks(app)
        try inspectCard(app, front: originalFront)
        try openBackups(app)
        try openHistory(app)
        XCTAssertTrue(wait { self.entryNames(app).contains(safety) },
            "Confirmed restore must publish its specifically named safety archive")
        XCTAssertEqual(entryNames(app).subtracting(beforeRestore).sorted().first, safety,
            "The safety copy precedes any automatic archive of the restored collection")
        XCTAssertNotEqual(safety, older)
        XCTAssertNotEqual(safety, newer)
        try cancelHistory(app)

        try relaunch(app, minute: 3)
        try inspectCard(app, front: originalFront)
        try openBackups(app)
        try openHistory(app)
        XCTAssertTrue(entryNames(app).isSuperset(of: [older, newer, safety]),
            "The original archive and the pre-restore safety copy must survive process termination")
        try capture(app, anchor: "settings.backups.list", state: "history-safety-copy-after-relaunch")
        // Restore the newly identified safety archive, not the later manual
        // archive: recovering the edit proves the predecessor was preserved.
        try restore(safety, app)
        try returnToDecks(app)
        try inspectCard(app, front: changedFront)
        try relaunch(app, minute: 4)
        try inspectCard(app, front: changedFront)
        try capture(app, anchor: "home.deck.page.1", state: "safety-copy-recovered-after-clean-relaunch")
    }

    private func arguments(minute: Int, fixture: URL? = nil) -> [String] {
        launchedMinutes.insert(minute)
        return MeguriLaunch(fixture: fixture, fixtureRunID: runID, now: "2030-01-15T10:0\(minute):00Z", timeZone: "UTC",
            locale: "en_US", animations: false).arguments
    }

    private func relaunch(_ app: XCUIApplication, minute: Int) throws {
        app.terminate()
        app.launchArguments = arguments(minute: minute)
        app.launch()
        XCTAssertTrue(HomeControls.deckRows(app, deckID: "1").firstMatch.waitForExistence(timeout: 20))
    }

    private func openDeck(_ app: XCUIApplication) throws {
        if element(app, "home.deck.page.1").exists { return }
        let rows = HomeControls.deckRows(app, deckID: "1")
        XCTAssertTrue(wait { HomeControls.hittable(rows) != nil }, "Default must be selectable in the native deck tree")
        try act(try XCTUnwrap(HomeControls.hittable(rows)), app)
        XCTAssertTrue(element(app, "home.deck.page.1").waitForExistence(timeout: 20))
    }

    private func startReview(_ app: XCUIApplication) throws {
        try openDeck(app)
        try act(element(app, "home.deck.study"), app)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
    }

    private func inspectCard(_ app: XCUIApplication, front: String) throws {
        try startReview(app)
        try assertRendered(app, front)
        try act(element(app, "review.session.reveal"), app)
        try assertRendered(app, originalBack)
        try act(element(app, "review.session.close"), app)
        XCTAssertTrue(element(app, "review.session.screen").waitForNonExistence(timeout: 20))
    }

    private func assertRendered(_ app: XCUIApplication, _ content: String) throws {
        let rendered = app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", content, content)).firstMatch
        XCTAssertTrue(rendered.waitForExistence(timeout: 20), "The real study card must render: \(content)")
    }

    private func field(_ name: String, editor: XCUIElement) -> XCUIElement {
        editor.descendants(matching: .any).matching(identifier: "editor.fields").firstMatch
            .descendants(matching: .textView).matching(NSPredicate(format: "label == %@", name)).firstMatch
    }

    private func enter(_ content: String, field name: String, editor: XCUIElement,
                       app: XCUIApplication, replacing: Bool = true) throws {
        let input = field(name, editor: editor)
        try act(input, app)
        #if os(macOS)
        if replacing { app.typeKey("a", modifierFlags: .command) }
        for character in content {
            app.typeKey(String(character).lowercased(), modifierFlags: character.isUppercase ? .shift : [])
        }
        app.typeKey(.tab, modifierFlags: [])
        let nextMode = editor.descendants(matching: .any).matching(identifier:
            name == "Front" ? "editor.field.1.htmlToggle" : "editor.add.htmlToggle").firstMatch
        XCTAssertTrue(wait { nextMode.exists && (name == "Front" ? nextMode.isEnabled : !nextMode.isEnabled) },
            "The editor must process the native blur before reading the committed value")
        #else
        input.typeText(content)
        // Move focus to the other native field so WebKit commits the edit.
        try act(field(name == "Front" ? "Back" : "Front", editor: editor), app)
        #endif
        XCTAssertTrue(wait {
            guard let value = self.fieldValue(input) else { return false }
            return replacing ? value == content : value.contains(content)
        }, "Native typing and blur must commit the expected field text")
    }

    private func fieldValue(_ field: XCUIElement) -> String? {
        guard let value = field.value as? String else { return nil }
        // WebKit's multiline AX representation may add one terminal newline.
        return value.hasSuffix("\n") ? String(value.dropLast()) : value
    }

    private func openBackups(_ app: XCUIApplication) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        if !element(app, "settings.root.content").exists {
            app.windows["main"].typeKey(",", modifierFlags: .command)
        }
        XCTAssertTrue(element(app, "settings.root.content").waitForExistence(timeout: 20))
        #else
        let sections = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["shell.root.tab.settings", "shell.root.sidebar.settings"]))
        XCTAssertTrue(wait { HomeControls.hittable(sections) != nil })
        try act(try XCTUnwrap(HomeControls.hittable(sections)), app)
        #endif
        if !element(app, "settings.backups.create").exists {
            try revealAndAct("settings.backups.open", app)
        }
        XCTAssertTrue(element(app, "settings.backups.create").waitForExistence(timeout: 20))
    }

    private func returnToDecks(_ app: XCUIApplication) throws {
        #if os(macOS)
        try closeSettings(app)
        #else
        let sections = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["shell.root.tab.decks", "shell.root.sidebar.home"]))
        XCTAssertTrue(wait { HomeControls.hittable(sections) != nil })
        try act(try XCTUnwrap(HomeControls.hittable(sections)), app)
        #endif
    }

    #if os(macOS)
    private func closeSettings(_ app: XCUIApplication) throws {
        let settings = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        if settings.exists {
            settings.typeKey("w", modifierFlags: .command)
            XCTAssertTrue(settings.waitForNonExistence(timeout: 10))
        }
    }

    private func fileCommand(_ title: String, _ app: XCUIApplication) throws {
        try closeSettings(app)
        try app.bringToFront(bundleIdentifier: bundle)
        let main = app.windows["main"]
        XCTAssertTrue(main.waitForExistence(timeout: 20))
        main.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.02)).click()
        try act(app.menuBars.menuBarItems["File"], app)
        let command = app.menuItems.matching(NSPredicate(format: "title == %@ OR label == %@", title, title)).firstMatch
        try act(command, app)
    }
    #endif

    private func createBackup(_ app: XCUIApplication) throws -> String {
        try revealAndAct("settings.backups.create", app)
        return try createdFilename(app)
    }

    private func createdFilename(_ app: XCUIApplication) throws -> String {
        var filename: String?
        XCTAssertTrue(wait {
            let results = app.staticTexts.allElementsBoundByIndex.map { self.text($0) }
            filename = results.compactMap { value in
                value.firstMatch(of: /Created (backup-[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{2}\.[0-9]{2}\.[0-9]{2}\.colpkg)/).map { String($0.1) }
            }.first
            return filename != nil && self.element(app, "settings.backups.create").isEnabled
        }, "Back Up Now must report the actual Created filename and finish")
        let created = try XCTUnwrap(filename)
        XCTAssertTrue(allowedRunNames.contains(created), "The Created result must belong to this run's clock")
        return created
    }

    private func openHistory(_ app: XCUIApplication) throws {
        try revealAndAct("settings.backups.restore", app)
        try waitForHistory(app)
    }

    private func waitForHistory(_ app: XCUIApplication) throws {
        XCTAssertTrue(element(app, "settings.backups.list").waitForExistence(timeout: 20))
        XCTAssertTrue(wait { !self.entryNames(app).isEmpty && self.element(app, "settings.backups.apply").isEnabled },
            "Real history must load an eligible, preselected backup")
        try assertRunHistory(app)
    }

    private func backupNames(minute: Int) -> [String] {
        (0...60).map { offset in
            String(format: "backup-2030-01-15-10.%02d.%02d.colpkg", minute + offset / 60, offset % 60)
        }
    }

    private var allowedRunNames: Set<String> { Set(launchedMinutes.flatMap { backupNames(minute: $0) }) }

    private func assertRunHistory(_ app: XCUIApplication) throws {
        let names = entryNames(app)
        XCTAssertFalse(names.isEmpty)
        // The hook exclusively created empty Collection/Backups siblings for
        // this UUID. Accept automatic as well as manual archives, but reject
        // every filename outside the instants actually launched in this run.
        XCTAssertTrue(names.isSubset(of: allowedRunNames), "Every displayed archive must be synthetic and produced in this run: \(names)")
    }

    private func entryNames(_ app: XCUIApplication) -> Set<String> {
        Set(element(app, "settings.backups.list").descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", entryPrefix))
            .allElementsBoundByIndex.map { String($0.identifier.dropFirst(entryPrefix.count)) })
    }

    private func select(_ filename: String, _ app: XCUIApplication) throws {
        let row = element(app, entryPrefix + filename)
        try act(row, app)
        XCTAssertTrue(element(app, "settings.backups.apply").isEnabled)
        XCTAssertTrue(wait { row.isSelected }, "The exact archive must be selected before confirmation")
    }

    private func advanceToConfirmation(_ app: XCUIApplication) throws -> XCUIElement {
        try act(element(app, "settings.backups.apply"), app)
        // System confirmation buttons lose app identifiers (app/AGENTS.md).
        let contexts = app.descendants(matching: .any).matching(NSPredicate(format: "elementType IN %@", [
            XCUIElement.ElementType.popover.rawValue, XCUIElement.ElementType.sheet.rawValue,
            XCUIElement.ElementType.alert.rawValue, XCUIElement.ElementType.dialog.rawValue,
        ])).containing(.button, identifier: "Restore")
        var found: XCUIElement?
        XCTAssertTrue(wait {
            // Exclude the presenting restore sheet if it contains the native
            // prompt as a descendant; prefer the innermost native surface.
            found = contexts.allElementsBoundByIndex.filter {
                self.destructiveRestore($0).exists
                    && !$0.buttons.matching(identifier: "settings.backups.cancel").firstMatch.exists
            }.min { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }
            return found != nil
        })
        let confirmation = try XCTUnwrap(found, "A native confirmation must own the destructive Restore button")
        // Exact caption + confirmation context exclude the parent's Restore…
        // (settings.backups.apply), even when the parent sheet is also visible.
        XCTAssertTrue(destructiveRestore(confirmation).isEnabled)
        return confirmation
    }

    private func destructiveRestore(_ confirmation: XCUIElement) -> XCUIElement {
        confirmation.buttons.matching(NSPredicate(format: "label == %@ AND identifier != %@",
            "Restore", "settings.backups.apply")).firstMatch
    }

    private func confirmationDismissalPoint(_ window: XCUIElement) throws -> XCUICoordinate? {
        #if os(iOS)
        // Capture geometry before the modal tree hides the presenting controls.
        let bar = window.navigationBars.firstMatch
        XCTAssertTrue(bar.exists)
        return window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: bar.frame.midX - window.frame.minX, dy: bar.frame.midY - window.frame.minY))
        #else
        return nil
        #endif
    }

    private func cancelConfirmation(_ confirmation: XCUIElement, _ app: XCUIApplication, outside: XCUICoordinate?) throws {
        let cancel = confirmation.buttons.matching(NSPredicate(format: "label == %@ AND identifier != %@",
            "Cancel", "settings.backups.cancel")).firstMatch
        if cancel.exists && cancel.isHittable {
            try act(cancel, app)
        } else {
            #if os(macOS)
            XCTFail("The native Mac confirmation must offer Cancel")
            #else
            // iOS 27 can hide cancel-role buttons in a popover. A tap in
            // the presenting sheet's empty navigation-bar middle dismisses it.
            let point = try XCTUnwrap(outside)
            XCTAssertFalse(confirmation.frame.contains(point.screenPoint), "Dismiss outside the native confirmation")
            point.tap()
            #endif
        }
        XCTAssertTrue(destructiveRestore(confirmation).waitForNonExistence(timeout: 10))
    }

    private func restore(_ filename: String, _ app: XCUIApplication) throws {
        try select(filename, app)
        let confirmation = try advanceToConfirmation(app)
        try act(destructiveRestore(confirmation), app)
        XCTAssertTrue(destructiveRestore(confirmation).waitForNonExistence(timeout: 10))
        // The list disappears as soon as progress replaces it. Only the whole
        // sheet closing after terminal delivery establishes durable completion.
        XCTAssertTrue(element(app, "settings.backups.cancel").waitForNonExistence(timeout: 30),
            "The sheet's toolbar Cancel must disappear after durable replacement")
        XCTAssertFalse(element(app, "settings.backups.list").exists)
    }

    private func cancelHistory(_ app: XCUIApplication) throws {
        try act(element(app, "settings.backups.cancel"), app)
        XCTAssertTrue(element(app, "settings.backups.cancel").waitForNonExistence(timeout: 10))
    }

    private func revealAndAct(_ identifier: String, _ app: XCUIApplication) throws {
        let target = element(app, identifier)
        XCTAssertTrue(target.waitForExistence(timeout: 20))
        // Bounded scrolling is a normal user operation, not a timing retry.
        let window = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        for _ in 0..<6 where !target.isHittable {
            #if os(macOS)
            window.scrollViews.firstMatch.scroll(byDeltaX: 0, deltaY: -350)
            #else
            window.swipeUp()
            #endif
        }
        try act(target, app)
    }

    private func act(_ target: XCUIElement, _ app: XCUIApplication) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(wait { target.exists && target.isEnabled && target.isHittable },
            "Expected an actionable native control: \(target.identifier)")
        target.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).clickOrTap()
    }

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func text(_ element: XCUIElement) -> String {
        (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label
    }

    private func wait(_ condition: @escaping () -> Bool) -> Bool {
        XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)],
            timeout: 20) == .completed
    }

    private func capture(_ app: XCUIApplication, anchor: String, state: String) throws {
        if element(app, "settings.backups.list").exists { try assertRunHistory(app) }
        capture(try captureWindow(app, anchor: anchor), state: state)
    }

    private func captureWindow(_ app: XCUIApplication, anchor: String) throws -> XCUIElement {
        XCTAssertTrue(element(app, anchor).waitForExistence(timeout: 10))
        // Bind by window index before presenting a system confirmation: its
        // modal accessibility tree may hide the presenting sheet's controls.
        let index = try XCTUnwrap(app.windows.allElementsBoundByIndex.firstIndex {
            $0.descendants(matching: .any).matching(identifier: anchor).firstMatch.exists
        }, "The captured surface must belong to Meguri Dev, never the desktop")
        return app.windows.element(boundBy: index)
    }

    private func capture(_ window: XCUIElement, state: String) {
        #if os(macOS)
        let platform = "mac"
        #else
        let platform = UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
        #endif
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "native-backups-\(platform)-\(state)-synthetic"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
