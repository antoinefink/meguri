import Foundation
import XCTest
#if os(macOS)

/// Production Mac outlines over the public generated recipe, with no screen route.
@MainActor
final class MacHomeHierarchyTests: XCTestCase {
    @MainActor
    private final class JourneyCleanupState {
        var pending = false
    }

    func testNormalMeguriSetupControlsAndCancelledServiceSwitch() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let journey: OwnedProviderJourneyInput?
        do { journey = try OwnedProviderJourneyInput.admit(repositoryRoot: build.repositoryRoot) }
        catch {
            XCTFail("Owned provider journey input refused")
            return
        }
        let bundle = try build.bundleIdentifier
        XCTAssertTrue(build.hasHooks && bundle.hasSuffix(".dev"), "This case requires the actual hook-enabled .dev app")
        guard build.hasHooks, bundle.hasSuffix(".dev") else { return }
        let appInfo = try build.appInfo
        XCTAssertEqual(appInfo["MeguriCloudSchemaConfigured"] as? Bool, true,
            "This real provider journey requires MEGURI_APP_SIGNING=team")
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: journey == nil
                ? "build/fixtures/recipes/local-review-1" : "build/fixtures/recipes/owned-sync-review-1", directoryHint: .isDirectory),
            ownedProviderFixture: true, now: journey == nil ? "2030-01-15T10:00:00+01:00" : nil, timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
        if let journey {
            app.launchEnvironment["MEGURI_OWNED_PROVIDER_TRANSPORT_RUN_ID"] = journey.runID
            app.launchEnvironment["MEGURI_OWNED_PROVIDER_TRANSPORT_MANIFEST_SHA256"] = journey.transportManifestSHA256
        }
        app.launch()
        // Forced termination is failure cleanup only; success requires native Quit below.
        addTeardownBlock { @MainActor in if app.state != .notRunning { app.terminate() } }
        let journeyCleanup = JourneyCleanupState()
        if journey != nil {
            addTeardownBlock { @MainActor in
                if journeyCleanup.pending { XCTFail("Owned provider journey cleanup-pending") }
            }
            let ready = app.descendants(matching: .any).matching(identifier: "testhooks.ownedProvider.transport.ready")
            guard ready.element(boundBy: 0).waitForExistence(timeout: 30) else {
                XCTFail("Owned provider transport prerequisite failed")
                return
            }
            XCTAssertEqual(ready.count, 1, "Require actual transport installation before service actions")
        }
        let homeMatches = app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "shell.root.sidebar.home"))
        let home = homeMatches.element(boundBy: 0)
        XCTAssertTrue(home.waitForExistence(timeout: 20))
        XCTAssertEqual(homeMatches.count, 1)
        try click(home, app: app, bundle: bundle)
        let homeScreens = app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "home.collection.screen"))
        XCTAssertTrue(homeScreens.element(boundBy: 0).waitForExistence(timeout: 20),
            journey == nil ? "Stage the generated local-review-1 fixture through tools/mg"
                : "Stage the generated owned-sync-review-1 fixture through tools/mg")
        XCTAssertEqual(homeScreens.count, 1)
        // N14.1: the sidebar is the Mac's only deck outline.
        _ = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review::Cards")
        XCTAssertEqual(app.sheets.count, 0, "Nothing asks before Home (N18.1)")

        func waitFor(_ predicate: @escaping () -> Bool, _ message: String) {
            let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 10), .completed, message)
        }
        func unique(_ scope: XCUIElement, _ id: String) -> XCUIElement {
            let matches = scope.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", id))
            let target = matches.element(boundBy: 0)
            XCTAssertTrue(target.waitForExistence(timeout: 10), id)
            XCTAssertEqual(matches.count, 1, id)
            return target
        }
        func actionable(_ target: XCUIElement) {
            waitFor({ target.exists && target.isEnabled && target.isHittable }, "Native action must be enabled and hittable: \(target.identifier)")
        }
        func finiteWindow(_ window: XCUIElement) {
            let frame = window.frame
            XCTAssertTrue(!frame.isEmpty && [frame.minX, frame.minY, frame.width, frame.height].allSatisfy(\.isFinite))
        }
        let mainWindows = app.windows.containing(NSPredicate(format: "identifier == %@", "home.collection.screen"))
        XCTAssertEqual(mainWindows.count, 1)
        let main = mainWindows.element(boundBy: 0)
        finiteWindow(main)
        try app.bringToFront(bundleIdentifier: bundle)
        main.typeKey(",", modifierFlags: .command)

        let settingsWindows = app.windows.containing(NSPredicate(format: "identifier == %@", "settings.root.content"))
        let settings = settingsWindows.element(boundBy: 0)
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        XCTAssertEqual(settingsWindows.count, 1)
        finiteWindow(settings)
        // N15.1: the list has Sync and Collection; the Sync pane holds the Service control.
        _ = unique(settings, "settings.root.sync")
        _ = unique(settings, "settings.root.collection")
        let picker = unique(settings, "settings.sync.service")
        XCTAssertEqual(picker.elementType, .popUpButton)
        actionable(picker)
        // T19.2: Off, Meguri Sync, iCloud, in that order; iCloud carries its Beta label.
        let captions = ["Off", "Meguri Sync", "iCloud (Beta)"]

        func choose(_ caption: String) throws {
            actionable(picker)
            try click(picker, app: app, bundle: bundle)
            let items = picker.menuItems
            XCTAssertTrue(items.matching(NSPredicate(format: "title == %@", "Meguri Sync"))
                .element(boundBy: 0).waitForExistence(timeout: 10))
            XCTAssertEqual(items.count, 3, "Off is the first value of the one Service control")
            XCTAssertEqual(items.allElementsBoundByIndex.map(\.title), captions,
                "Require the full native menu's actual captions, in order")
            for title in captions {
                let options = items.matching(NSPredicate(format: "title == %@", title))
                XCTAssertEqual(options.count, 1, "Require the exact native service caption: \(title)")
                XCTAssertEqual(options.element(boundBy: 0).elementType, .menuItem)
                actionable(options.element(boundBy: 0))
            }
            let intended = items.matching(NSPredicate(format: "title == %@", caption))
            XCTAssertEqual(intended.count, 1)
            let option = intended.element(boundBy: 0)
            XCTAssertEqual(option.title, caption)
            XCTAssertEqual(option.elementType, .menuItem)
            actionable(option)
            try click(option, app: app, bundle: bundle)
        }
        func count(_ id: String) -> Int {
            settings.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", id)).count
        }
        func meguriForm() {
            waitFor({ picker.value as? String == "Meguri Sync" }, "The real Picker must retain Meguri selection")
            actionable(picker)
            for name in ["address", "username", "password"] {
                let field = unique(settings, "settings.sync.meguri.\(name)")
                XCTAssertEqual(field.elementType, name == "password" ? .secureTextField : .textField)
                actionable(field)
            }
            let secure = settings.secureTextFields.matching(NSPredicate(format: "identifier == %@", "settings.sync.meguri.password"))
            XCTAssertEqual(secure.count, 1, "Password must be an actual native secure field")
            let signIn = unique(settings, "settings.sync.meguri.signIn")
            XCTAssertEqual(signIn.elementType, .button)
            XCTAssertFalse(signIn.isEnabled, "The empty setup form cannot sign in")
            // P25.3: selected but not connected, the switch shows its stored value and is disabled.
            let automatic = unique(settings, "settings.sync.automatic")
            XCTAssertFalse(automatic.isEnabled, "Sync Automatically has no effect before the service is connected")
            // T19.6: no media toggle, Pause, Sign Out, Stop Using or Connect.
            for id in ["settings.sync.media", "settings.sync.pause", "settings.sync.stop", "settings.sync.connect",
                       "settings.sync.meguri.signOut", "settings.sync.privacyFooter"] {
                XCTAssertEqual(count(id), 0, id)
            }
        }
        // A choice made while the value is Off opens no confirmation (T19.2).
        try choose("Meguri Sync")
        XCTAssertEqual(settings.sheets.count, 0)
        meguriForm()
        try choose("iCloud (Beta)")
        let sheets = settings.sheets.containing(NSPredicate(format: "identifier == %@", "sync.serviceSwitch.cancel"))
        let sheet = sheets.element(boundBy: 0)
        XCTAssertTrue(sheet.waitForExistence(timeout: 10))
        XCTAssertEqual(sheets.count, 1)
        let cancel = unique(sheet, "sync.serviceSwitch.cancel")
        let confirm = unique(sheet, "sync.serviceSwitch.confirm")
        XCTAssertEqual(cancel.elementType, .button)
        XCTAssertEqual(confirm.elementType, .button)
        actionable(cancel)
        actionable(confirm)
        try app.bringToFront(bundleIdentifier: bundle)
        sheet.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        XCTAssertTrue(sheet.exists, "Return must not activate the destructive confirmation")
        XCTAssertEqual(sheets.count, 1)
        XCTAssertEqual(picker.value as? String, "Meguri Sync")
        for id in ["address", "username", "password", "signIn"] {
            _ = unique(settings, "settings.sync.meguri.\(id)")
        }
        actionable(cancel)
        try click(cancel, app: app, bundle: bundle)
        XCTAssertTrue(sheet.waitForNonExistence(timeout: 10))
        XCTAssertEqual(settings.sheets.count, 0)
        meguriForm()
        if let journey {
            // Input actions may be recorded by XCTest; MG retains this mode's
            // entire raw result privately. SecureField is not a redaction claim.
            func enter(_ text: String, id: String) throws {
                let field = unique(settings, id)
                actionable(field)
                try click(field, app: app, bundle: bundle)
                try app.bringToFront(bundleIdentifier: bundle)
                field.typeKey("a", modifierFlags: .command)
                try app.bringToFront(bundleIdentifier: bundle)
                field.typeText(text)
            }
            func waitForJourney(_ predicate: @escaping () -> Bool, _ message: String) {
                let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
                XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 60), .completed, message)
            }
            // Connected and settled: Sync Now can run, no status action waits on the person.
            func availableSync() -> Bool {
                let button = settings.buttons.matching(identifier: "settings.sync.syncNow").element(boundBy: 0)
                return button.exists && button.isEnabled && button.isHittable && settings.sheets.count == 0
                    && ["settings.sync.statusAction", "settings.sync.issues", "settings.sync.meguri.signIn"].allSatisfy {
                        settings.descendants(matching: .any).matching(identifier: $0).count == 0
                    }
            }
            try enter(journey.address, id: "settings.sync.meguri.address")
            try enter(journey.username, id: "settings.sync.meguri.username")
            try enter(journey.password, id: "settings.sync.meguri.password")
            let signIn = unique(settings, "settings.sync.meguri.signIn")
            actionable(signIn)
            journeyCleanup.pending = true
            try click(signIn, app: app, bundle: bundle)
            // MP6.3: signing in is the consent to set the service up. Both sides hold a
            // collection, so Meguri asks which one to keep (N15.3). Only the sheet's
            // current identifiers are asserted: its content is still being built.
            let keeps = settings.sheets.containing(NSPredicate(format: "identifier == %@", "sync.keepCollection.keepDevice"))
            let keep = keeps.element(boundBy: 0)
            XCTAssertTrue(keep.waitForExistence(timeout: 60), "Require the keep-collection sheet after sign-in")
            XCTAssertEqual(keeps.count, 1)
            XCTAssertEqual(settings.sheets.count, 1)
            XCTAssertEqual(count("settings.sync.meguri.signIn"), 0, "The sign-in fields give way to the Account row")
            for id in ["keepDevice", "keepService", "cancel"] {
                let control = unique(keep, "sync.keepCollection.\(id)")
                XCTAssertEqual(control.elementType, .button)
                actionable(control)
            }
            try app.bringToFront(bundleIdentifier: bundle)
            keep.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
            XCTAssertTrue(keep.exists, "Return must confirm neither destructive choice")
            let keepDevice = unique(keep, "sync.keepCollection.keepDevice")
            try click(keepDevice, app: app, bundle: bundle)
            let previews = settings.sheets.containing(NSPredicate(format: "identifier == %@", "sync.meguriSetup.confirm"))
            let preview = previews.element(boundBy: 0)
            XCTAssertTrue(preview.waitForExistence(timeout: 60), "Require the actual setup preview")
            XCTAssertEqual(previews.count, 1)
            XCTAssertEqual(settings.sheets.count, 1)
            let previewCancel = unique(preview, "sync.meguriSetup.cancel")
            let previewConfirm = unique(preview, "sync.meguriSetup.confirm")
            for control in [previewCancel, previewConfirm] {
                XCTAssertEqual(control.elementType, .button)
                actionable(control)
            }
            for caption in [
                "The server’s collection will be replaced with this device’s collection. Meguri first backs up this device’s collection and keeps a copy of the server’s collection.",
                "Stop syncing on your other devices until this finishes. Changes they send to the server meanwhile can be replaced.",
                "Media then syncs both ways. On either side, files can be added, replaced or deleted. Files that no card uses are kept.",
                "After setup, Meguri Sync syncs this collection and its media automatically."
            ] {
                let content = preview.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", caption, caption))
                XCTAssertTrue(content.element(boundBy: 0).waitForExistence(timeout: 10), "Require ordinary setup consequence content")
                XCTAssertEqual(content.count, 1)
            }
            try click(previewConfirm, app: app, bundle: bundle)
            waitForJourney({ availableSync() }, "Setup must finish with available native Sync Now and no unresolved setup sheet")
            XCTAssertEqual(unique(settings, "settings.sync.syncNow").elementType, .button)
            // N15.2 row 2: once signed in, one Account row takes the fields' place.
            _ = unique(settings, "settings.sync.meguri.account")
            _ = unique(settings, "settings.sync.troubleshooting")

            waitForJourney({ availableSync() }, "Native Sync Now must be available before the daily action")
            let syncNow = unique(settings, "settings.sync.syncNow")
            actionable(syncNow)
            func successSequence() -> UInt64? {
                let prefix = "meguri-round-success-sequence:"
                guard let value = syncNow.value as? String, value.hasPrefix(prefix) else { return nil }
                let digits = value.dropFirst(prefix.count)
                guard !digits.isEmpty, digits.utf8.allSatisfy({ (48...57).contains($0) }),
                      let sequence = UInt64(digits), String(sequence) == String(digits) else { return nil }
                return sequence
            }
            try app.bringToFront(bundleIdentifier: bundle)
            let baseline = try XCTUnwrap(successSequence(), "Require the exact numeric Meguri round success observation")
            syncNow.clickOrTap()
            waitForJourney({
                guard let sequence = successSequence(), sequence > baseline else { return false }
                return availableSync()
            }, "Require a fresh successful collection/media round and available controls before review")
            let automatic = unique(settings, "settings.sync.automatic")
            XCTAssertTrue(automatic.isEnabled, "Require the ready binding's native Sync Automatically control")
            // P25.3: with no stored value Sync Automatically reads on from the first completed setup.
            XCTAssertTrue(automatic.value as? String == "1" || automatic.value as? NSNumber == NSNumber(value: 1),
                "Sync Automatically must report the exact native ON value after bootstrap")

            actionable(automatic)
            try click(automatic, app: app, bundle: bundle)
            try app.bringToFront(bundleIdentifier: bundle)
            waitForJourney({
                availableSync() && automatic.exists && automatic.isEnabled && automatic.isHittable
                    && (automatic.value as? String == "0" || automatic.value as? NSNumber == NSNumber(value: 0))
            }, "Require settled native OFF and available controls before closing Settings")
            XCTAssertEqual(picker.value as? String, "Meguri Sync")
            let offSequence = try XCTUnwrap(successSequence(), "Require the exact settled OFF success sequence before review")
            settings.typeKey("w", modifierFlags: .command)
            XCTAssertTrue(settings.waitForNonExistence(timeout: 10), "Close the actual Settings window")
            let deck = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review::Cards")
            actionable(deck)
            try click(deck, app: app, bundle: bundle)
            let study = unique(main, "home.deck.study")
            actionable(study)
            try click(study, app: app, bundle: bundle)
            // The native Mac workspace replaces Home during a review; its
            // window must be scoped by the real review subtree at this phase.
            let reviewWindows = app.windows.containing(NSPredicate(format: "identifier == %@", "review.session.screen"))
            let review = reviewWindows.element(boundBy: 0)
            XCTAssertTrue(review.waitForExistence(timeout: 10))
            XCTAssertEqual(reviewWindows.count, 1)
            _ = unique(review, "review.session.screen")
            let reveal = unique(review, "review.session.reveal")
            actionable(reveal)
            try click(reveal, app: app, bundle: bundle)
            let good = unique(review, "review.session.rate.good")
            actionable(good)
            try click(good, app: app, bundle: bundle) // Exactly one native answer.
            let close = unique(review, "review.session.close")
            actionable(close)
            try click(close, app: app, bundle: bundle)
            XCTAssertTrue(review.waitForNonExistence(timeout: 10))
            XCTAssertTrue(homeScreens.element(boundBy: 0).waitForExistence(timeout: 10))
            XCTAssertEqual(homeScreens.count, 1)
            let recorded = unique(main, "home.session.recorded")
            let expected = "1 answer recorded"
            waitFor({ recorded.label == expected || recorded.value as? String == expected }, "Require exactly the existing one-answer presentation")

            try app.bringToFront(bundleIdentifier: bundle)
            main.typeKey(",", modifierFlags: .command)
            XCTAssertTrue(settings.waitForExistence(timeout: 10))
            XCTAssertEqual(settingsWindows.count, 1)
            waitForJourney({
                availableSync() && automatic.exists && automatic.isEnabled && automatic.isHittable
                    && (automatic.value as? String == "0" || automatic.value as? NSNumber == NSNumber(value: 0))
            }, "Require native OFF retained with available controls after review")
            XCTAssertEqual(picker.value as? String, "Meguri Sync")
            // This observes no completed round before re-enabling, not zero HTTP requests.
            XCTAssertEqual(try XCTUnwrap(successSequence()), offSequence,
                "The successful-round sequence must remain at the settled OFF value")
            actionable(automatic)
            try click(automatic, app: app, bundle: bundle)
            // A higher sequence attributes neither a trigger nor a request count;
            // MG separately checks the exact persisted Good without a manual sync.
            waitForJourney({
                guard let sequence = successSequence(), sequence > offSequence else { return false }
                return availableSync() && automatic.exists && automatic.isEnabled && automatic.isHittable
                    && (automatic.value as? String == "1" || automatic.value as? NSNumber == NSNumber(value: 1))
            }, "Require a fresh successful collection/media round and available controls before turning sync off")
            // V27.5, T19.2: Off is the one way out. It asks once, names the removed
            // sign-in, and its destructive button is not the default.
            try choose("Off")
            let offSheets = settings.sheets.containing(NSPredicate(format: "identifier == %@", "sync.serviceSwitch.confirm"))
            let offSheet = offSheets.element(boundBy: 0)
            XCTAssertTrue(offSheet.waitForExistence(timeout: 10))
            XCTAssertEqual(offSheets.count, 1)
            XCTAssertEqual(settings.sheets.count, 1)
            let offConfirm = unique(offSheet, "sync.serviceSwitch.confirm")
            XCTAssertEqual(offConfirm.elementType, .button)
            XCTAssertTrue(offConfirm.label == "Turn Off Sync" || offConfirm.title == "Turn Off Sync")
            try app.bringToFront(bundleIdentifier: bundle)
            offSheet.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
            XCTAssertTrue(offSheet.exists, "Return must not turn sync off")
            XCTAssertEqual(picker.value as? String, "Meguri Sync")
            actionable(offConfirm)
            try click(offConfirm, app: app, bundle: bundle)
            waitForJourney({
                settings.sheets.count == 0 && picker.exists && picker.isEnabled && picker.value as? String == "Off"
                    && settings.staticTexts["Choose a service to sync this collection."].exists
                    && ["settings.sync.meguri.signIn", "settings.sync.meguri.account", "settings.sync.syncNow",
                        "settings.sync.status", "settings.sync.automatic", "settings.sync.troubleshooting"].allSatisfy {
                        settings.descendants(matching: .any).matching(identifier: $0).count == 0
                    }
            }, "Owned provider journey cleanup-pending")
            journeyCleanup.pending = false
        }
        try app.bringToFront(bundleIdentifier: bundle)
        settings.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10), "Native Quit must finish actual close and owned cleanup")
    }

    func testExpandedTreesKeepUniqueIDsNativeDisclosureSelectionAndChildMenus() throws {
        let (app, bundle) = try launchFixture()
        // N14.1: the sidebar is the Mac's only deck outline; with no deck selected the
        // The detail column shows the Today page and no second deck list.
        let sidebarParent = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review")
        let sidebarChild = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review::Cards")
        let parentID = String(sidebarParent.identifier.dropFirst("home.sidebar.deck.".count))
        let childID = String(sidebarChild.identifier.dropFirst("home.sidebar.deck.".count))
        assertTodayRoot(app, readyDeckID: childID)
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)

        // Each click hits the real AppKit triangle in this deck's native row.
        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForNonExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: false)
        XCTAssertTrue(sidebarParent.exists)
        // Today lists what waits whatever the outline shows.
        assertTodayRoot(app, readyDeckID: childID)
        try assertUniqueIDs(app)

        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: true)
        assertTodayRoot(app, readyDeckID: childID)
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)

        try click(sidebarParent, app: app, bundle: bundle)
        assertPage(app, deckID: parentID)
        XCTAssertTrue(sidebarChild.exists, "Selecting a parent must not collapse it")
        try openChildMenu(sidebarChild, deckID: childID, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(menuItem(app, action: "overview", deckID: childID).waitForNonExistence(timeout: 10))
        try openChildMenu(sidebarChild, deckID: childID, app: app, bundle: bundle)
        try click(menuItem(app, action: "overview", deckID: childID), app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        try showDecksRoot(app, bundle: bundle)
        assertTodayRoot(app, readyDeckID: childID)
        XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
        try click(sidebarChild, app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        assertExpansionState(app, parentID: parentID, expanded: true)

        // Collapsing a sidebar ancestor must leave the selected child's page alone.
        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForNonExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: false)
        assertPage(app, deckID: childID)
        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: true)
        try click(sidebarParent, app: app, bundle: bundle)
        assertPage(app, deckID: parentID)
        XCTAssertTrue(sidebarChild.exists, "Selecting a parent must not collapse it")
        try click(sidebarChild, app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        try showDecksRoot(app, bundle: bundle)
        assertTodayRoot(app, readyDeckID: childID)
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)
    }

    func testSavedStudyCollapseSurvivesRestartWithoutReseeding() throws {
        let (app, bundle) = try launchFixture()
        let parent = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review")
        let child = namedRow(app, prefix: "home.sidebar.deck.", path: "Local Review::Cards")
        let parentID = String(parent.identifier.dropFirst("home.sidebar.deck.".count))
        let childID = String(child.identifier.dropFirst("home.sidebar.deck.".count))
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)
        try click(triangle(app, rowID: parent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(child.waitForNonExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: false)
        // Native Quit awaits ReviewCollection.close and its pending collapse write.
        // Relaunch only after that graceful termination, without replacing the DB.
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10))
        app.launchArguments = launchArguments()
        app.launch()
        try showDecksRoot(app, bundle: bundle)
        let sidebarRowID = "home.sidebar.deck.\(parentID)"
        XCTAssertTrue(element(app, sidebarRowID).waitForExistence(timeout: 20))
        XCTAssertFalse(element(app, "home.sidebar.deck.\(childID)").exists)
        assertExpansionState(app, parentID: parentID, expanded: false)
        assertTodayRoot(app, readyDeckID: childID)
        try assertUniqueIDs(app)
        try click(triangle(app, rowID: sidebarRowID), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.sidebar.deck.\(childID)").waitForExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: true)
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)
    }

    private func launchFixture() throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = launchArguments(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"))
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try showDecksRoot(app, bundle: bundle)
        return (app, bundle)
    }

    private func launchArguments(fixture: URL? = nil) -> [String] {
        // In particular, the restart omits -MeguriFixture, which replaces the DB.
        MeguriLaunch(fixture: fixture, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
    }

    private func showDecksRoot(_ app: XCUIApplication, bundle: String) throws {
        let home = element(app, "shell.root.sidebar.home")
        XCTAssertTrue(home.waitForExistence(timeout: 20))
        try click(home, app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.collection.screen").waitForExistence(timeout: 20),
            "Stage the generated local-review-1 fixture through tools/mg")
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).element(boundBy: 0)
    }

    private func namedRow(_ app: XCUIApplication, prefix: String, path: String) -> XCUIElement {
        let rows = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", prefix, path))
        XCTAssertTrue(rows.element(boundBy: 0).waitForExistence(timeout: 20))
        XCTAssertEqual(rows.count, 1, "Each deck retains its own selection identifier")
        return rows.element(boundBy: 0)
    }

    private func triangle(_ app: XCUIApplication, rowID: String) -> XCUIElement {
        let outlines = app.outlines.containing(.any, identifier: rowID)
        XCTAssertEqual(outlines.count, 1)
        let rows = outlines.element(boundBy: 0).outlineRows.containing(.any, identifier: rowID)
        XCTAssertEqual(rows.count, 1)
        let triangles = rows.element(boundBy: 0).cells.disclosureTriangles
            .matching(identifier: "NSOutlineViewDisclosureButtonKey")
        XCTAssertEqual(triangles.count, 1, "Scope repeated system IDs to the deck's native outline row/cell")
        let target = triangles.element(boundBy: 0)
        XCTAssertEqual(target.elementType, .disclosureTriangle)
        XCTAssertTrue(target.isEnabled)
        XCTAssertTrue(target.isHittable)
        return target
    }

    private func click(_ target: XCUIElement, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        target.clickOrTap()
    }

    private func assertExpansionState(_ app: XCUIApplication, parentID: String, expanded: Bool) {
        for prefix in ["home.sidebar.deck"] {
            let target = triangle(app, rowID: "\(prefix).\(parentID)")
            let state = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                target.exists && Self.nativeExpansionValue(target.value) == expanded
            }, object: nil)
            XCTAssertEqual(XCTWaiter.wait(for: [state], timeout: 10), .completed,
                "The row's native triangle must report exactly 0 or 1 for the expected expansion state")
            XCTAssertEqual(Self.nativeExpansionValue(target.value), expanded)
        }
    }

    private nonisolated static func nativeExpansionValue(_ value: Any?) -> Bool? {
        if let value = value as? String {
            switch value {
            case "0": return false
            case "1": return true
            default: return nil
            }
        }
        if let value = value as? NSNumber {
            if value == NSNumber(value: 0) { return false }
            if value == NSNumber(value: 1) { return true }
        }
        return nil
    }

    /// N14.1: with no deck selected the detail is Today: the decks with cards
    /// waiting, each with its own Study, and no second deck outline.
    private func assertTodayRoot(_ app: XCUIApplication, readyDeckID: String) {
        XCTAssertTrue(element(app, "home.collection.screen").exists)
        XCTAssertFalse(app.buttons["home.deck.study"].exists, "Disclosure must not select a deck or start Study")
        XCTAssertTrue(element(app, "home.today.deck.\(readyDeckID)").waitForExistence(timeout: 10),
            "Today must list the deck whose cards wait")
        let study = app.buttons.matching(identifier: "home.today.study.\(readyDeckID)")
        XCTAssertEqual(study.count, 1)
        XCTAssertTrue(study.element(boundBy: 0).isEnabled)
        XCTAssertEqual(app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "home.deck.row.")).count, 0, "The sidebar is the only deck outline")
        XCTAssertFalse(element(app, "review.session.screen").exists)
    }

    private func assertPage(_ app: XCUIApplication, deckID: String) {
        XCTAssertTrue(element(app, "home.deck.page.\(deckID)").waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["home.deck.study"].exists)
        XCTAssertFalse(element(app, "review.session.screen").exists, "Selection must not start Study")
    }

    private func menuItem(_ app: XCUIApplication, action: String, deckID: String) -> XCUIElement {
        app.menuItems.matching(identifier: "home.deckMenu.\(action).\(deckID)").element(boundBy: 0)
    }

    private func openChildMenu(_ row: XCUIElement, deckID: String, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        row.rightClick()
        let actions = [(action: "study", title: "Study"), (action: "addNote", title: "Add Note"), (action: "overview", title: "Overview"),
            (action: "rename", title: "Rename…"), (action: "subdeck", title: "New Subdeck…")]
        for (action, title) in actions {
            let matches = app.menuItems.matching(identifier: "home.deckMenu.\(action).\(deckID)")
            XCTAssertTrue(matches.element(boundBy: 0).waitForExistence(timeout: 10))
            XCTAssertEqual(matches.count, 1)
            XCTAssertTrue(matches.element(boundBy: 0).isEnabled)
            XCTAssertTrue(matches[title].exists, "The owned action must have its native caption: \(title)")
        }
        let ownedItems = app.menuItems.matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@", "home.deckMenu.", ".\(deckID)"))
        XCTAssertEqual(ownedItems.allElementsBoundByIndex.map(\.identifier),
            actions.map { "home.deckMenu.\($0.action).\(deckID)" }, "Owned actions must retain their menu order")
    }

    private func assertExpandedHierarchy(_ app: XCUIApplication, parentID: String, childID: String) throws {
        XCTAssertEqual(parentID, "1884308400001")
        XCTAssertEqual(childID, "1884308400002")
        assertExpansionState(app, parentID: parentID, expanded: true)
        let tree = try HierarchyExport.tree(app)
        // These are the generated fixture's exported selection controls, including
        // AppKit's staticText parent labels and numeric sidebar badge values.
        let expectedRows = [
            (id: "home.sidebar.deck.1884308400001", type: "staticText", label: "Local Review", value: "4"),
            (id: "home.sidebar.deck.1884308400002", type: "button", label: "Local Review::Cards", value: "4")
        ]
        for expected in expectedRows {
            let rows = tree.elements(expected.id)
            XCTAssertEqual(rows.count, 1, "Each deck must expose exactly one selection control: \(expected.id)")
            let row = try XCTUnwrap(rows.first)
            XCTAssertEqual(row.type, expected.type, expected.id)
            XCTAssertEqual(row.label, expected.label, expected.id)
            XCTAssertEqual(row.value, expected.value, expected.id)
            XCTAssertTrue(row.enabled, expected.id)
        }
        try assertUniqueIDs(app)
    }

    private func assertUniqueIDs(_ app: XCUIApplication) throws {
        let tree = try HierarchyExport.tree(app)
        let home = tree.root.allNodes().filter { $0.identifier.hasPrefix("home.deck.") || $0.identifier.hasPrefix("home.sidebar.") || $0.identifier.hasPrefix("home.today.") }
        let identifiers = home.map(\.identifier)
        XCTAssertEqual(Set(identifiers).count, identifiers.count, "Own Home/sidebar IDs must be unique in the real native tree")
        let attachment = XCTAttachment(data: try HierarchyExport.json(tree, platform: "mac",
            screenshotOrigin: tree.window.origin, screenshotScale: 1), uniformTypeIdentifier: "public.json")
        attachment.name = "production-mac-home-hierarchy"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
