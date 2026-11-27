#if os(iOS)
import Foundation
import UIKit
import XCTest

/// Production pages, row menus and forms over the same MG recipe as the Mac.
/// Run compact on iPhone and iPad; the second case covers the wide iPad sidebar.
@MainActor final class FilteredStudyTouchUITests: XCTestCase {
    func testCompactDeckActionsAndOperableStudyForms() throws {
        try journey(sizeClass: "compact")
    }

    func testIPadSidebarDeckActionsAndOperableStudyForms() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else { throw XCTSkip("Wide iPad sidebar") }
        XCUIDevice.shared.orientation = .landscapeLeft
        defer { XCUIDevice.shared.orientation = .portrait }
        try journey(sizeClass: "regular")
    }

    func testIPhoneLargestTextCustomStudyAmountRemainsOperable() throws {
        guard UIDevice.current.userInterfaceIdiom == .phone else { throw XCTSkip("iPhone largest text") }
        let app = try launch(sizeClass: "compact", extra: AccessibilityAudit.Variant(largestText: true, dark: false).launchArguments)
        defer { app.terminate() }
        let deck = try selectDeck("Preview Source", app: app)
        try openDeckMenu(deck, app: app)
        try tap("home.deckMenu.customStudy." + deck, app: app)
        _ = try visible("home.customStudy.available", app: app)
        _ = try visible("home.customStudy.amount", app: app)
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        capture(app, "filtered-touch-iphone-largest-text-custom-study-before-editing")
        let fields = app.textFields.matching(identifier: "home.customStudy.amount")
        XCTAssertEqual(fields.count, 1, "One native editable amount field at largest text")
        let amount = fields.firstMatch
        XCTAssertTrue(amount.isEnabled && amount.isHittable)
        XCTAssertEqual(amount.label, "Change in today’s limit")
        let windowFrame = app.windows.firstMatch.frame
        let availableFrame = element(app, "home.customStudy.available").frame
        XCTAssertGreaterThanOrEqual(amount.frame.minX, windowFrame.minX, "The native amount stays inside the window at largest text")
        XCTAssertLessThanOrEqual(amount.frame.maxX, windowFrame.maxX, "The native amount stays inside the window at largest text")
        // Native LabeledContent stacks its read-only count at the leading content inset at AX5.
        let leadingInset = availableFrame.minX - windowFrame.minX
        XCTAssertEqual(amount.frame.maxX, windowFrame.maxX - leadingInset, accuracy: 1,
                       "The editor reaches the trailing content edge at largest text")
        try replace("home.customStudy.amount", with: "-99999", app: app)
        capture(app, "filtered-touch-iphone-largest-text-custom-study-signed-boundary")
        _ = try visible("home.customStudy.amount", app: app)
        XCTAssertTrue(amount.isEnabled && amount.isHittable)
        XCTAssertTrue(element(app, "home.customStudy.save").isEnabled)
        try tap("home.customStudy.save", app: app)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 15))
        XCTAssertTrue(element(app, "home.deck.page." + deck).exists, "Daily adjustment keeps the normal deck")
    }

    private func journey(sizeClass: String) throws {
        let app = try launch(sizeClass: sizeClass)
        defer { app.terminate() }
        let prefix = "filtered-touch-" + sizeClass
        let normalID = try selectDeck("Preview Source", app: app)
        XCTAssertTrue(element(app, "home.deck.study").waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.deck.customStudy").exists, "Cards waiting: Study alone (N14.3)")
        try openDeckMenu(normalID, app: app)
        XCTAssertFalse(element(app, "home.deckMenu.createFilteredDeck." + normalID).exists,
                       "The deck menu holds actions on this deck only (N20.2)")
        try tap("home.deckMenu.customStudy." + normalID, app: app)
        XCTAssertTrue(element(app, "home.customStudy.available").waitForExistence(timeout: 10))
        XCTAssertFalse(element(app, "home.customStudy.state").exists, "Only relevant inputs are exposed")
        let amount = app.textFields.matching(identifier: "home.customStudy.amount").firstMatch
        XCTAssertTrue(amount.waitForExistence(timeout: 10), "The amount remains a native editable text field")
        XCTAssertEqual(amount.label, "Change in today’s limit")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        capture(app, prefix + "-custom-study-before-editing")
        let windowFrame = app.windows.firstMatch.frame
        let availableFrame = element(app, "home.customStudy.available").frame
        XCTAssertGreaterThan(amount.frame.minX, windowFrame.midX, "The amount occupies the trailing half of the window")
        XCTAssertEqual(amount.frame.maxX, availableFrame.maxX, accuracy: 1, "The editor aligns with the available-card count")
        try app.performAccessibilityAudit(for: [.trait, .sufficientElementDescription])
        try replace("home.customStudy.amount", with: "invalid", app: app)
        XCTAssertFalse(element(app, "home.customStudy.save").isEnabled)
        for signedAmount in ["-99999", "99999"] {
            try replace("home.customStudy.amount", with: signedAmount, app: app)
            XCTAssertTrue(element(app, "home.customStudy.save").isEnabled, signedAmount)
        }
        try replace("home.customStudy.amount", with: "-100000", app: app)
        XCTAssertFalse(element(app, "home.customStudy.save").isEnabled)
        try replace("home.customStudy.amount", with: "0", app: app)
        capture(app, prefix + "-custom-study-page-entry-and-editable-amount")
        try tap("home.customStudy.save", app: app)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 15))
        XCTAssertTrue(element(app, "home.deck.page." + normalID).exists, "Daily adjustment keeps the normal deck")

        try openDeckMenu(normalID, app: app)
        assertUnavailableBrowse(normalID, app: app)
        XCTAssertTrue(element(app, "home.deckMenu.customStudy." + normalID).isEnabled)
        XCTAssertFalse(element(app, "home.filteredDeck.edit." + normalID).exists)
        try tap("home.deckMenu.customStudy." + normalID, app: app)
        try chooseOption("home.customStudy.mode", index: 5, count: 6, app: app)
        try chooseOption("home.customStudy.state", index: 3, count: 4, app: app)
        try replace("home.customStudy.amount", with: "2", app: app)
        try replace("home.customStudy.includeTags", with: "filtered-ui", app: app)
        try replace("home.customStudy.excludeTags", with: "exclude::tag*", app: app)
        capture(app, prefix + "-custom-study-state-and-literal-tags-form")
        XCTAssertTrue(element(app, "home.customStudy.save").isEnabled)
        try tap("home.customStudy.cancel", app: app)
        XCTAssertTrue(element(app, "home.customStudy.sheet").waitForNonExistence(timeout: 10))
        try openCreateFilteredDeck(app: app)
        try replace("home.filteredStudy.name", with: "Touch Preview", app: app)
        try replace("home.filteredStudy.first.limit", with: "1", app: app)
        finishCardLimitEditing(app)
        try setSwitch("home.filteredStudy.secondEnabled", to: true, app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.second.search").waitForExistence(timeout: 5),
                      "Enabling the second search exposes its native fields")
        try replace("home.filteredStudy.second.search", with: "tag:filtered-ui", app: app)
        try replace("home.filteredStudy.second.limit", with: "5", app: app)
        finishCardLimitEditing(app)
        try setSwitch("home.filteredStudy.reschedule", to: false, app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.again").waitForExistence(timeout: 5),
                      "Disabling rescheduling exposes the preview delays")
        try replace("home.filteredStudy.again", with: "60", app: app)
        try replace("home.filteredStudy.hard", with: "invalid", app: app)
        XCTAssertFalse(element(app, "home.filteredStudy.save").isEnabled, "Native Hard binding validates typed text")
        try replace("home.filteredStudy.hard", with: "120", app: app)
        try replace("home.filteredStudy.good", with: "0", app: app)
        capture(app, prefix + "-two-search-preview-form-with-edited-delays")
        try tap("home.filteredStudy.save", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 20))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2),
                      "Overlap gathers two distinct eligible cards and excludes the suspended match")
        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page.")).firstMatch
        XCTAssertTrue(page.waitForExistence(timeout: 15))
        let filteredID = String(page.identifier.dropFirst("home.deck.page.".count))
        XCTAssertNotNil(Int64(filteredID), "Filtered deck page has a deck ID")
        XCTAssertNotEqual(filteredID, normalID, "Creation selects the new filtered deck")
        XCTAssertFalse(element(app, "home.deck.addNote").exists)
        XCTAssertFalse(element(app, "home.deck.customStudy").exists)
        try tap("home.filteredDeck.edit." + filteredID, app: app)
        for (field, value) in [("name", "Touch Preview"), ("again", "60"), ("hard", "120"), ("good", "0")] {
            let target = try visible("home.filteredStudy." + field, app: app)
            XCTAssertEqual(target.value as? String, value, "Saved native text binding: " + field)
            XCTAssertFalse(target.label.isEmpty, "Accessible field name: " + field)
        }
        try replace("home.filteredStudy.good", with: "30", app: app)
        try tap("home.filteredStudy.save", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 20))
        try tap("home.filteredDeck.edit." + filteredID, app: app)
        XCTAssertEqual(try visible("home.filteredStudy.good", app: app).value as? String, "30")
        try app.performAccessibilityAudit(for: [.trait, .sufficientElementDescription])
        capture(app, prefix + "-reopened-native-editor-retains-hard-and-good")
        try tap("home.filteredStudy.cancel", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.sheet").waitForNonExistence(timeout: 10))

        try openDeckMenu(filteredID, app: app)
        assertUnavailableBrowse(filteredID, app: app)
        for action in ["addNote", "subdeck", "customStudy", "options"] {
            XCTAssertFalse(element(app, "home.deckMenu.\(action).\(filteredID)").exists, action)
        }
        for action in ["edit", "empty", "rebuild"] {
            XCTAssertTrue(element(app, "home.filteredDeck.\(action).\(filteredID)").isEnabled, action)
        }
        capture(app, prefix + "-filtered-deck-native-menu")
        try tap("home.filteredDeck.empty." + filteredID, app: app)
        XCTAssertTrue(element(app, "home.deck.rebuild").waitForExistence(timeout: 15))
        XCTAssertFalse(element(app, "home.deck.study").exists)
        try openDeckMenu(filteredID, app: app)
        XCTAssertFalse(element(app, "home.deckMenu.study." + filteredID).isEnabled, "Empty deck cannot start study")
        try tap("home.filteredDeck.rebuild." + filteredID, app: app)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        try tap("home.filteredDeck.empty." + filteredID, app: app)
        try tap("home.deck.rebuild", app: app)
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 2))
        capture(app, prefix + "-rebuilt-deck-page-with-real-study-entry")
        try tap("home.deck.study", app: app)
        try tap("review.session.reveal", app: app)
        XCTAssertTrue(try visible("review.session.rate.again", app: app).isEnabled)
        try tap("review.session.close", app: app)
    }

    /// Touch Browse is intentionally unavailable until it has a destination.
    /// The lifecycle fix may omit the item or leave a disabled native item.
    private func assertUnavailableBrowse(_ deck: String, app: XCUIApplication) {
        let browse = element(app, "home.deckMenu.browse." + deck)
        XCTAssertTrue(!browse.exists || !browse.isEnabled, "No enabled Browse action without a touch destination")
    }

    /// N20.1: Create Filtered Deck is in the Add menu. The deck page's presets the deck's due
    /// search; the tab layout's deck page has Add Note there, so the list's opens a blank one.
    private func openCreateFilteredDeck(app: XCUIApplication) throws {
        let add = element(app, "home.collection.add")
        let onDeckPage = add.exists && add.isHittable
        if !onDeckPage {
            let back = app.buttons.matching(identifier: "BackButton").firstMatch
            XCTAssertTrue(back.waitForExistence(timeout: 10))
            back.tap()
            XCTAssertTrue(add.waitForExistence(timeout: 10))
        }
        try tap("home.collection.add", app: app)
        try tap("home.collection.createFilteredDeck", app: app)
        XCTAssertTrue(element(app, "home.filteredStudy.name").waitForExistence(timeout: 10))
        let search = element(app, "home.filteredStudy.first.search")
        if onDeckPage {
            XCTAssertTrue((search.value as? String ?? "").contains("is:due"))
        } else {
            try replace("home.filteredStudy.first.search", with: "deck:\"Preview Source\" is:due", app: app)
        }
    }

    private func openDeckMenu(_ id: String, app: XCUIApplication) throws {
        let pageMenu = element(app, "home.deck.actions")
        if pageMenu.exists {
            try tap("home.deck.actions", app: app)
        } else {
            let rows = HomeControls.deckRows(app, deckID: id)
            XCTAssertTrue(wait { HomeControls.hittable(rows) != nil })
            try XCTUnwrap(HomeControls.hittable(rows)).press(forDuration: 1.2)
        }
        XCTAssertTrue(element(app, "home.deckMenu.study." + id).waitForExistence(timeout: 10))
    }

    private func selectDeck(_ name: String, app: XCUIApplication) throws -> String {
        // Follow the native Decks tab / Today sidebar entry, including a sidebar
        // the split view has hidden. Do not navigate through a test-hook route.
        let home = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@",
            ["shell.root.tab.decks", "shell.root.sidebar.home"]))
        let decksTab = app.tabBars.buttons["Decks"].firstMatch
        if HomeControls.hittable(home) == nil && !decksTab.isHittable {
            let toggle = app.buttons["Toggle Sidebar"].firstMatch
            if toggle.exists && toggle.isHittable { toggle.tap() }
        }
        XCTAssertTrue(wait { HomeControls.hittable(home) != nil || decksTab.isHittable }, deckRowDiagnostics(app))
        let entry = try XCTUnwrap(HomeControls.hittable(home) ?? (decksTab.isHittable ? decksTab : nil))
        entry.tap()

        // SwiftUI may expose the same sidebar row on several element types.
        // Accept appended count labels, but exclude subdecks and name lookalikes.
        let rows = HomeControls.deckRows(app, labelContaining: name).matching(NSPredicate(
            format: "label == %@ OR label BEGINSWITH %@", name, name + ","))
        XCTAssertTrue(wait { HomeControls.hittable(rows) != nil }, deckRowDiagnostics(app))
        let ids = Set(rows.allElementsBoundByIndex.compactMap { HomeControls.deckID($0.identifier) })
        XCTAssertEqual(ids.count, 1, "Expected one synthetic deck named \(name). \(deckRowDiagnostics(app))")
        let row = try XCTUnwrap(HomeControls.hittable(rows), deckRowDiagnostics(app))
        let id = try XCTUnwrap(HomeControls.deckID(row.identifier))
        row.tap()
        XCTAssertTrue(element(app, "home.deck.page." + id).waitForExistence(timeout: 15))
        // Compact navigation removes the row from the tree; retain its ID now.
        return id
    }

    private func deckRowDiagnostics(_ app: XCUIApplication) -> String {
        let rows = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@",
            HomeControls.rowPrefix, HomeControls.sidebarPrefix)).allElementsBoundByIndex
        return "Deck rows: " + rows.map {
            "type=\($0.elementType) label=\($0.label.debugDescription) id=\($0.identifier.debugDescription) hittable=\($0.isHittable)"
        }.joined(separator: "\n")
    }

    private func visible(_ id: String, app: XCUIApplication) throws -> XCUIElement {
        let target = element(app, id)
        // An operable control needs no keyboard dismissal, including the field
        // that is already being edited. Scroll only for an offscreen target.
        if target.exists && target.isHittable { return target }
        // Forms are ordinary scrollable controls; find the field in either direction
        // when revisiting a sheet whose first search is already above the viewport.
        if let form = studyForm(app) {
            for up in [true, false] {
                for _ in 0..<8 {
                    if target.exists && target.isHittable { break }
                    if !(try scrollForm(form, up: up, app: app)) { break }
                }
            }
        }
        let ready = wait { target.exists && target.isHittable }
        if !ready { attachState(app, "reveal-failed-" + id) }
        XCTAssertTrue(ready, id)
        return target
    }

    private func studyForm(_ app: XCUIApplication) -> XCUIElement? {
        for id in ["home.filteredStudy.sheet", "home.customStudy.sheet"] {
            let sheet = element(app, id)
            guard sheet.exists else { continue }
            if [.collectionView, .table, .scrollView].contains(sheet.elementType) { return sheet }
            return [sheet.collectionViews, sheet.tables, sheet.scrollViews]
                .map { $0.firstMatch }.first { $0.exists }
        }
        return nil
    }

    private func scrollForm(_ form: XCUIElement, up: Bool, app: XCUIApplication) throws -> Bool {
        // Never fall back to the sheet: its pan recognizer can dismiss it.
        XCTAssertTrue([.collectionView, .table, .scrollView].contains(form.elementType))
        let window = app.windows.firstMatch
        var viewport = form.frame.intersection(app.frame)
        if window.exists { viewport = viewport.intersection(window.frame) }
        let keyboard = app.keyboards.firstMatch
        if keyboard.exists && viewport.intersects(keyboard.frame) {
            viewport.size.height = max(0, min(viewport.maxY, keyboard.frame.minY) - viewport.minY)
        }
        XCTAssertTrue(!viewport.isNull && !viewport.isEmpty && !viewport.isInfinite
                      && [viewport.minX, viewport.minY, viewport.width, viewport.height].allSatisfy { $0.isFinite },
                      "Visible native form viewport")
        let before = try form.snapshot()
        var distance = viewport.height * 0.3
        if !up {
            // Do not pull down at the top, or drag farther than the content above
            // the viewport. A slow drag prevents momentum handing off to the sheet.
            let firstCell = form.cells.firstMatch
            guard firstCell.exists else { return false }
            let firstFrame = firstCell.frame
            guard !firstFrame.isEmpty && !firstFrame.isNull && !firstFrame.isInfinite else { return false }
            distance = min(distance, viewport.minY - firstFrame.minY)
            guard distance > 1 else { return false }
        }
        let origin = form.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: viewport.midX - form.frame.minX,
            dy: viewport.midY - form.frame.minY + (up ? distance / 2 : -distance / 2)))
        let end = start.withOffset(CGVector(dx: 0, dy: up ? -distance : distance))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0)
        XCTAssertTrue(form.exists, "Scrolling must retain the study Form")
        return contentPosition(before) != contentPosition(try form.snapshot())
    }

    private func contentPosition(_ node: any XCUIElementSnapshot) -> [String] {
        if node.elementType == .cell { return [node.identifier + node.label + NSCoder.string(for: node.frame)] }
        return node.children.flatMap { contentPosition($0) }
    }

    private func finishCardLimitEditing(_ app: XCUIApplication) {
        // Only these single-line limit edits precede switches. Do not submit
        // or dismiss the keyboard while editing multiline literal tags.
        let keyboard = app.keyboards.firstMatch
        guard keyboard.exists else { return }
        let action: XCUIElement
        if UIDevice.current.userInterfaceIdiom == .pad {
            action = app.buttons["Hide keyboard"].firstMatch
        } else {
            action = keyboard.buttons.matching(NSPredicate(
                format: "identifier == %@ OR label == %@", "Return", "return")).firstMatch
        }
        guard action.exists && action.isEnabled && action.isHittable else { return }
        action.tap()
        XCTAssertTrue(keyboard.waitForNonExistence(timeout: 5), "Native card-limit keyboard dismissal")
    }

    private func setSwitch(_ id: String, to enabled: Bool, app: XCUIApplication) throws {
        _ = try visible(id, app: app)
        let matches = app.switches.matching(identifier: id)
        XCTAssertEqual(matches.count, 1, "One identified native switch: " + id)
        let control = matches.firstMatch
        // SwiftUI may expose a row-sized Switch wrapping a smaller native one.
        let descendants = control.switches
        XCTAssertLessThanOrEqual(descendants.count, 1, "Unambiguous native switch descendant: " + id)
        let thumb = descendants.count == 1 ? descendants.firstMatch : control
        attachState(app, "before-switch-" + id)
        func diagnostics() -> String {
            "\(id); requested=\(enabled); keyboardPresent=\(app.keyboards.firstMatch.exists); "
                + "sheet=\(element(app, "home.filteredStudy.sheet").frame); "
                + "row(value=\(String(reflecting: control.value)), frame=\(control.frame), "
                + "enabled=\(control.isEnabled), hittable=\(control.isHittable)); "
                + "native(value=\(String(reflecting: thumb.value)), frame=\(thumb.frame), "
                + "enabled=\(thumb.isEnabled), hittable=\(thumb.isHittable))"
        }
        XCTAssertTrue(control.isEnabled && thumb.isEnabled && thumb.isHittable, diagnostics())
        XCTAssertFalse(control.label.isEmpty, "Named native action: " + id)
        let oldValue = switchValue(control)
        if oldValue != !enabled {
            captureSwitchFailure(app, "unexpected-old-value-" + id, details: diagnostics())
        }
        XCTAssertEqual(oldValue, !enabled, "Old switch value: " + diagnostics())
        let frame = thumb.frame
        XCTAssertTrue(frame.width > 0 && frame.height > 0)
        let nativeTap = descendants.count == 1 || frame.width <= frame.height * 3
        if nativeTap {
            // After finishCardLimitEditing, let XCTest choose the exposed native
            // switch's hittable point on either iPhone or iPad.
            thumb.tap()
        } else {
            // When AX exposes only a wide row, its centre can focus the preceding
            // field. Use the upper trailing portion of the native control.
            let offset = CGVector(dx: frame.width - min(30, frame.width / 2), dy: frame.height / 4)
            let point = CGPoint(x: frame.minX + offset.dx, y: frame.minY + offset.dy)
            let sheet = element(app, "home.filteredStudy.sheet")
            let keyboardCoversPoint = app.keyboards.allElementsBoundByIndex.contains {
                $0.exists && $0.isHittable && $0.frame.intersects(sheet.frame) && $0.frame.contains(point)
            }
            let pointIsVisible = app.frame.contains(point) && sheet.frame.contains(point) && !keyboardCoversPoint
            let details = "point=\(point); keyboardCoversPoint=\(keyboardCoversPoint); " + diagnostics()
            if !pointIsVisible { captureSwitchFailure(app, "switch-point-obscured-" + id, details: details) }
            // Background toolbars in other presentation layers cannot establish
            // occlusion of this foreground sheet; XCTest geometry is not a hit test.
            XCTAssertTrue(pointIsVisible, "Native switch point must be visible in the sheet: " + details)
            thumb.coordinate(withNormalizedOffset: .zero).withOffset(offset).tap()
        }
        let changed = wait { switchValue(control) == enabled }
        attachState(app, "after-switch-" + id)
        let newValue = switchValue(control)
        let details = "old=\(String(describing: oldValue)); new=\(String(describing: newValue)); "
            + "tap=\(nativeTap ? "native default" : "upper trailing point"); " + diagnostics()
        if !changed || newValue != enabled {
            captureSwitchFailure(app, "unexpected-new-value-" + id, details: details)
        }
        XCTAssertTrue(changed, "Native switch must change: " + details)
        XCTAssertEqual(newValue, enabled, "New switch value: " + details)
    }

    private func captureSwitchFailure(_ app: XCUIApplication, _ name: String, details: String) {
        capture(app, name)
        let attachment = XCTAttachment(string: details + "\n" + app.debugDescription)
        attachment.name = name + "-hierarchy"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func switchValue(_ control: XCUIElement) -> Bool? {
        guard control.exists else { return nil }
        if let value = control.value as? String, ["0", "1"].contains(value) { return value == "1" }
        if let value = control.value as? NSNumber {
            if value == NSNumber(value: 0) { return false }
            if value == NSNumber(value: 1) { return true }
        }
        return nil
    }

    private func attachState(_ app: XCUIApplication, _ name: String) {
        var lines = ["keyboardPresent=\(app.keyboards.firstMatch.exists)"]
        func walk(_ node: any XCUIElementSnapshot) {
            if node.identifier.hasPrefix("home.filteredStudy.") || node.identifier.hasPrefix("home.customStudy.")
                || [.switch, .keyboard, .collectionView, .table, .scrollView].contains(node.elementType) {
                lines.append("type=\(node.elementType) id=\(node.identifier) label=\(node.label.debugDescription) "
                    + "value=\(String(describing: node.value)) enabled=\(node.isEnabled) frame=\(node.frame)")
            }
            node.children.forEach { walk($0) }
        }
        if let snapshot = try? app.snapshot() { walk(snapshot) }
        let attachment = XCTAttachment(string: lines.joined(separator: "\n"))
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func tap(_ id: String, app: XCUIApplication) throws {
        let target = try visible(id, app: app)
        XCTAssertTrue(wait { target.isEnabled }, id)
        XCTAssertFalse(target.label.isEmpty, "Named native action: " + id)
        target.tap()
    }

    private func replace(_ id: String, with text: String, app: XCUIApplication) throws {
        let target = try visible(id, app: app)
        XCTAssertTrue(target.isEnabled, id)
        XCTAssertTrue([.textField, .textView, .searchField].contains(target.elementType), id)
        var fieldSnapshot: (any XCUIElementSnapshot)?
        func fieldText() throws -> String {
            let snapshot = try target.snapshot()
            fieldSnapshot = snapshot
            // Only an absent AX value means empty; preserve all native text.
            guard let value = snapshot.value else { return "" }
            if let text = value as? NSAttributedString { return text.string }
            return try XCTUnwrap(value as? String,
                "Unsupported native AX field value type \(String(reflecting: type(of: value))): " + diagnostics())
        }
        func diagnostics() -> String {
            guard let snapshot = fieldSnapshot else { return id + "; field snapshot unavailable" }
            return "\(id); observed=\(String(reflecting: snapshot.value)); "
                + "placeholder=\(String(reflecting: snapshot.placeholderValue)); frame=\(snapshot.frame)"
        }
        target.tap()
        let previous = try fieldText()
        let count = previous.utf16.count
        if count > 0 {
            // Actual right-arrow events reach the end from any caret or
            // selection. UTF-16 length bounds both movement and deletion.
            for _ in 0..<count {
                target.typeKey(.rightArrow, modifierFlags: [])
            }
            target.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count))
        }
        guard try wait({ try fieldText() == "" }) else {
            attachState(app, "replace-not-empty-" + id)
            XCTFail("Native field must be empty before replacement: " + diagnostics())
            return
        }
        target.typeText(text)
        let matched = try wait { try fieldText() == text }
        if !matched { attachState(app, "replace-final-mismatch-" + id) }
        XCTAssertTrue(matched, "Native field must equal replacement: " + diagnostics())
    }

    /// iOS does not preserve identifiers on SwiftUI Picker options. Select
    /// within the native menu by the form's declared enum order, with its exact
    /// option count asserted; no translated menu captions or Mac menu bar.
    private func chooseOption(_ id: String, index: Int, count: Int, app: XCUIApplication) throws {
        try tap(id, app: app)
        var options: XCUIElementQuery?
        XCTAssertTrue(wait {
            options = app.collectionViews.allElementsBoundByIndex.first { menu in
                let items = menu.buttons.allElementsBoundByIndex
                return items.count == count && items.contains { $0.isSelected } && items.allSatisfy { $0.identifier.isEmpty }
            }?.buttons
            return options != nil
        }, id + " native option count and selection")
        let choice = try XCTUnwrap(options).element(boundBy: index)
        XCTAssertTrue(choice.isEnabled && choice.isHittable)
        choice.tap()
    }

    private func launch(sizeClass: String, extra: [String] = []) throws -> XCUIApplication {
        #if !targetEnvironment(simulator)
        throw XCTSkip("MG stages the generated recipe in the simulator VM")
        #endif
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("Synthetic development container required") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/filtered-study-1"),
            now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris", locale: "en_US", animations: false,
            extra: ["-MeguriSizeClass", sizeClass] + extra).arguments
        app.launch()
        XCTAssertTrue(element(app, "home.collection.add").waitForExistence(timeout: 25))
        return app
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        // Filtered actions share ids between the page and its native menu.
        // Scope actions exclusively to the native menu or collection view
        // containing that deck's Study item, even when the action is absent.
        if let deck = id.split(separator: ".").last, Int64(deck) != nil {
            for containers in [app.menus, app.collectionViews] {
                let menu = containers.containing(.any, identifier: "home.deckMenu.study." + String(deck)).firstMatch
                if menu.exists {
                    return menu.descendants(matching: .any).matching(identifier: id).firstMatch
                }
            }
        }
        let matches = app.descendants(matching: .any).matching(identifier: id)
        return HomeControls.hittable(matches) ?? matches.firstMatch
    }

    private func wait(_ condition: @MainActor () throws -> Bool) rethrows -> Bool {
        let deadline = Date().addingTimeInterval(15)
        while try !condition() {
            guard Date() < deadline else { return false }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return true
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
