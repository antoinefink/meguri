import Foundation
import XCTest
#if os(macOS)

/// Production Mac outlines over the public generated recipe, with no screen route.
@MainActor
final class MacHomeHierarchyTests: XCTestCase {
    func testNormalMeguriSetupControlsAndCancelledServiceSwitch() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        XCTAssertTrue(build.hasHooks && bundle.hasSuffix(".dev"), "This case requires the actual hook-enabled .dev app")
        guard build.hasHooks, bundle.hasSuffix(".dev") else { return }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1", directoryHint: .isDirectory),
            ownedProviderFixture: true, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
        app.launch()
        // Forced termination is failure cleanup only; success requires native Quit below.
        addTeardownBlock { @MainActor in if app.state != .notRunning { app.terminate() } }
        let homeMatches = app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "shell.root.sidebar.home"))
        let home = homeMatches.element(boundBy: 0)
        XCTAssertTrue(home.waitForExistence(timeout: 20))
        XCTAssertEqual(homeMatches.count, 1)
        try click(home, app: app, bundle: bundle)
        let homeScreens = app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "home.collection.screen"))
        XCTAssertTrue(homeScreens.element(boundBy: 0).waitForExistence(timeout: 20),
            "Stage the generated local-review-1 fixture through tools/mg")
        XCTAssertEqual(homeScreens.count, 1)
        _ = namedRow(app, prefix: "home.deck.row.", path: "Local Review::Cards")
        XCTAssertFalse(app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "home.policy.enable"))
            .element(boundBy: 0).exists, "The generated recipe must already enable FSRS")
        XCTAssertEqual(app.sheets.count, 0, "Unexpected admission sheets are fixture prerequisite failures")

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
        _ = unique(settings, "settings.root.accountSync")
        let picker = unique(settings, "settings.accountSync.service")
        XCTAssertEqual(picker.elementType, .popUpButton)
        actionable(picker)

        func choose(_ caption: String) throws {
            actionable(picker)
            try click(picker, app: app, bundle: bundle)
            let items = picker.menuItems
            let optionExists = items.matching(NSPredicate(format: "label == %@", "Meguri Sync"))
                .element(boundBy: 0).waitForExistence(timeout: 10)
            if !optionExists {
                let scopes: [(code: String, menus: XCUIElementQuery, items: XCUIElementQuery)] = [
                    ("picker", picker.menus, picker.menuItems),
                    ("settings", settings.menus, settings.menuItems),
                    ("app", app.menus, app.menuItems)
                ]
                let menuItemType = NSNumber(value: XCUIElement.ElementType.menuItem.rawValue)
                for scope in scopes {
                    let labelICloud = scope.items.matching(NSPredicate(format: "label == %@", "iCloud Sync")).count
                    let labelMeguri = scope.items.matching(NSPredicate(format: "label == %@", "Meguri Sync")).count
                    let titleICloud = scope.items.matching(NSPredicate(format: "title == %@", "iCloud Sync")).count
                    let titleMeguri = scope.items.matching(NSPredicate(format: "title == %@", "Meguri Sync")).count
                    let valueICloud = scope.items.matching(NSPredicate(format: "value == %@", "iCloud Sync")).count
                    let valueMeguri = scope.items.matching(NSPredicate(format: "value == %@", "Meguri Sync")).count
                    print("ownedPopupScope: \(scope.code) menuTotal=\(scope.menus.count) itemsTotal=\(scope.items.count) "
                        + "labelICloud=\(labelICloud) labelMeguri=\(labelMeguri) "
                        + "titleICloud=\(titleICloud) titleMeguri=\(titleMeguri) "
                        + "valueICloud=\(valueICloud) valueMeguri=\(valueMeguri)")
                    for attribute in ["label", "title", "value"] {
                        let candidates = scope.menus.containing(NSPredicate(
                            format: "elementType == %@ AND %K == %@", menuItemType, attribute, "iCloud Sync"))
                            .containing(NSPredicate(
                                format: "elementType == %@ AND %K == %@", menuItemType, attribute, "Meguri Sync"))
                        let candidateCount = candidates.count
                        let totalItems = candidateCount == 1 ? candidates.element(boundBy: 0).menuItems.count : -1
                        print("ownedPopupMenuCandidate: \(scope.code) attribute=\(attribute) "
                            + "candidateCount=\(candidateCount) totalItems=\(totalItems)")
                    }
                }
            }
            XCTAssertTrue(optionExists)
            XCTAssertEqual(items.count, 2, "LocalOnly is a separate Stop action, never a third provider")
            XCTAssertEqual(Set(items.allElementsBoundByIndex.map(\.label)), Set(["iCloud Sync", "Meguri Sync"]),
                "Require the full native menu's actual captions")
            for title in ["iCloud Sync", "Meguri Sync"] {
                let options = items.matching(NSPredicate(format: "label == %@", title))
                XCTAssertEqual(options.count, 1, "Require the exact native service caption: \(title)")
                XCTAssertEqual(options.element(boundBy: 0).elementType, .menuItem)
                actionable(options.element(boundBy: 0))
            }
            let intended = items.matching(NSPredicate(format: "label == %@", caption))
            XCTAssertEqual(intended.count, 1)
            let option = intended.element(boundBy: 0)
            XCTAssertEqual(option.label, caption)
            XCTAssertEqual(option.elementType, .menuItem)
            actionable(option)
            try click(option, app: app, bundle: bundle)
        }
        func meguriForm() {
            waitFor({ picker.value as? String == "Meguri Sync" }, "The real Picker must retain Meguri selection")
            actionable(picker)
            for name in ["address", "username", "password"] {
                let field = unique(settings, "settings.accountSync.meguri.\(name)")
                XCTAssertEqual(field.elementType, name == "password" ? .secureTextField : .textField)
                actionable(field)
            }
            let secure = settings.secureTextFields.matching(NSPredicate(format: "identifier == %@", "settings.accountSync.meguri.password"))
            XCTAssertEqual(secure.count, 1, "Password must be an actual native secure field")
            let signIn = unique(settings, "settings.accountSync.meguri.signIn")
            XCTAssertEqual(signIn.elementType, .button)
            XCTAssertFalse(signIn.isEnabled, "The empty setup form cannot sign in")
            for id in ["iCloudHeading", "connect", "automatic", "media"] {
                XCTAssertEqual(settings.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@", "settings.accountSync.\(id)")).count, 0)
            }
        }
        try choose("Meguri Sync")
        meguriForm()
        try choose("iCloud Sync")
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
            _ = unique(settings, "settings.accountSync.meguri.\(id)")
        }
        actionable(cancel)
        try click(cancel, app: app, bundle: bundle)
        XCTAssertTrue(sheet.waitForNonExistence(timeout: 10))
        XCTAssertEqual(settings.sheets.count, 0)
        meguriForm()
        try app.bringToFront(bundleIdentifier: bundle)
        settings.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10), "Native Quit must finish actual close and owned cleanup")
    }

    func testExpandedTreesKeepUniqueIDsNativeDisclosureSelectionAndChildMenus() throws {
        let (app, bundle) = try launchFixture()
        let parent = namedRow(app, prefix: "home.deck.row.", path: "Local Review")
        let child = namedRow(app, prefix: "home.deck.row.", path: "Local Review::Cards")
        let parentID = String(parent.identifier.dropFirst("home.deck.row.".count))
        let childID = String(child.identifier.dropFirst("home.deck.row.".count))
        let sidebarParent = element(app, "home.sidebar.deck.\(parentID)")
        let sidebarChild = element(app, "home.sidebar.deck.\(childID)")
        XCTAssertTrue(sidebarParent.waitForExistence(timeout: 10))
        XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)

        // Each click hits the real AppKit triangle in this deck's native row.
        // Both outlines share collection-owned study collapse, but not selection.
        for rowID in [parent.identifier, sidebarParent.identifier] {
            try click(triangle(app, rowID: rowID), app: app, bundle: bundle)
            XCTAssertTrue(child.waitForNonExistence(timeout: 10))
            XCTAssertTrue(sidebarChild.waitForNonExistence(timeout: 10))
            assertExpansionState(app, parentID: parentID, expanded: false)
            XCTAssertTrue(parent.exists)
            XCTAssertTrue(sidebarParent.exists)
            assertDecksRoot(app)
            try assertUniqueIDs(app)

            try click(triangle(app, rowID: rowID), app: app, bundle: bundle)
            XCTAssertTrue(child.waitForExistence(timeout: 10))
            XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
            assertExpansionState(app, parentID: parentID, expanded: true)
            assertDecksRoot(app)
            try assertExpandedHierarchy(app, parentID: parentID, childID: childID)
        }

        try click(parent, app: app, bundle: bundle)
        assertPage(app, deckID: parentID)
        try showDecksRoot(app, bundle: bundle)
        XCTAssertTrue(child.waitForExistence(timeout: 10), "Selecting a parent must not collapse it")
        try openChildMenu(child, deckID: childID, app: app, bundle: bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(menuItem(app, action: "overview", deckID: childID).waitForNonExistence(timeout: 10))
        try openChildMenu(child, deckID: childID, app: app, bundle: bundle)
        try click(menuItem(app, action: "overview", deckID: childID), app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        try showDecksRoot(app, bundle: bundle)
        XCTAssertTrue(child.waitForExistence(timeout: 10))
        try click(child, app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        assertExpansionState(app, parentID: parentID, expanded: true, sidebarOnly: true)

        // Collapsing a sidebar ancestor must leave the selected child's page alone.
        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForNonExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: false, sidebarOnly: true)
        assertPage(app, deckID: childID)
        try click(triangle(app, rowID: sidebarParent.identifier), app: app, bundle: bundle)
        XCTAssertTrue(sidebarChild.waitForExistence(timeout: 10))
        assertExpansionState(app, parentID: parentID, expanded: true, sidebarOnly: true)
        try click(sidebarParent, app: app, bundle: bundle)
        assertPage(app, deckID: parentID)
        XCTAssertTrue(sidebarChild.exists, "Selecting a parent must not collapse it")
        try click(sidebarChild, app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        try openChildMenu(sidebarChild, deckID: childID, app: app, bundle: bundle)
        try click(menuItem(app, action: "overview", deckID: childID), app: app, bundle: bundle)
        assertPage(app, deckID: childID)
        try showDecksRoot(app, bundle: bundle)
        try assertExpandedHierarchy(app, parentID: parentID, childID: childID)
    }

    func testSavedStudyCollapseSurvivesRestartWithoutReseeding() throws {
        let (app, bundle) = try launchFixture()
        let parent = namedRow(app, prefix: "home.deck.row.", path: "Local Review")
        let child = namedRow(app, prefix: "home.deck.row.", path: "Local Review::Cards")
        let parentID = String(parent.identifier.dropFirst("home.deck.row.".count))
        let childID = String(child.identifier.dropFirst("home.deck.row.".count))
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
        XCTAssertTrue(element(app, "home.deck.row.\(parentID)").waitForExistence(timeout: 20))
        XCTAssertFalse(element(app, "home.deck.row.\(childID)").exists)
        XCTAssertFalse(element(app, "home.sidebar.deck.\(childID)").exists)
        assertExpansionState(app, parentID: parentID, expanded: false)
        assertDecksRoot(app)
        try assertUniqueIDs(app)
        _ = triangle(app, rowID: "home.deck.row.\(parentID)")
        let sidebarRowID = "home.sidebar.deck.\(parentID)"
        try click(triangle(app, rowID: sidebarRowID), app: app, bundle: bundle)
        XCTAssertTrue(element(app, "home.deck.row.\(childID)").waitForExistence(timeout: 10))
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
        let enable = element(app, "home.policy.enable")
        if enable.waitForExistence(timeout: 5) { try click(enable, app: app, bundle: bundle) }
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

    private func assertExpansionState(_ app: XCUIApplication, parentID: String, expanded: Bool, sidebarOnly: Bool = false) {
        for prefix in sidebarOnly ? ["home.sidebar.deck"] : ["home.deck.row", "home.sidebar.deck"] {
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

    private func assertDecksRoot(_ app: XCUIApplication) {
        XCTAssertTrue(element(app, "home.collection.screen").exists)
        XCTAssertFalse(app.buttons["home.deck.study"].exists, "Disclosure must not select a deck or start Study")
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
        let actions = [(action: "study", title: "Study"), (action: "overview", title: "Overview"),
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
            (id: "home.deck.row.1884308400001", type: "staticText", label: "Local Review", value: "0 new · 0 learning · 4 due"),
            (id: "home.deck.row.1884308400002", type: "button", label: "Local Review::Cards", value: "0 new · 0 learning · 4 due"),
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
        let home = tree.root.allNodes().filter { $0.identifier.hasPrefix("home.deck.") || $0.identifier.hasPrefix("home.sidebar.") }
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
