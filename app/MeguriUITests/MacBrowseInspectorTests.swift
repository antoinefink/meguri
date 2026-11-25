import Foundation
import XCTest
#if os(macOS)
import AppKit
#endif

/// Browse's inspector on the Mac (U273, restyled as Option A; `browser/workspace`
/// V7, V12; N21.3, N21.4, N21.5, N21.8), over the generated typical recipe on the real core:
/// the toggle last in the toolbar; one card with its preview, Deck and Type,
/// fields and tags; the empty fields opened; the formatting bar in use; an
/// edit saved when another row is chosen, as Edit › Undo's "Undo Edit Note";
/// a long note scrolling in the pane; a card with an image; several rows'
/// summary; no selection; the pane hidden in a narrow window and shown over
/// the table by Return. Each step attaches its window as a capture.
@MainActor
final class MacBrowseInspectorTests: XCTestCase {
    #if os(macOS)
    func testInspectorLight() throws { try inspector(appearance: "light") }
    func testInspectorDark() throws { try inspector(appearance: "dark") }

    func testTagsAutocompleteLight() throws { try tagsAutocomplete(appearance: "light") }
    func testTagsAutocompleteDark() throws { try tagsAutocomplete(appearance: "dark") }

    /// Production inspector + real core, with system suggestions in the VM.
    /// Paste seeds committed tags; unfinished queries always use real typing.
    private func tagsAutocomplete(appearance: String) throws {
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            throw XCTSkip("Browse tag completion runs only in the Mac test VM")
        }
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        try showDeck(app, bundle, containing: "Español")
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 1 })
        press(table.outlineRows.element(boundBy: 0))
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(shows(fields, "palabra"))
        let firstFront = try XCTUnwrap(fields.element(boundBy: 0).value as? String)
        let paths = ["Adjectives", "Adverbs", "Articles", "Conditionals", "Nouns", "Prepositions", "Pronouns", "Tenses", "Verbs"]
            .map { "Spanish::Grammar::" + $0 }
        let tags = element(app, "browse.inspector.tags")
        func hasTag(_ name: String) -> Bool {
            // Match in the remote AX process, rather than fetching every child's
            // label and value separately on every poll.
            tags.descendants(matching: .any).matching(
                NSPredicate(format: "label == %@ OR value == %@", name, name)).firstMatch.exists
        }
        func suggestion(_ name: String) -> XCUIElement {
            app.descendants(matching: .any).matching(
                NSPredicate(format: "label == %@ OR value == %@", name, name)).firstMatch
        }
        // This test is VM-only. Never read or retain the workstation clipboard.
        let pasteboard = NSPasteboard.general
        defer { pasteboard.clearContents() }
        func seed(_ names: [String]) {
            tags.click()
            app.typeKey("a", modifierFlags: [.command])
            pasteboard.clearContents()
            XCTAssertTrue(pasteboard.setString(names.joined(separator: " ") + " ", forType: .string))
            app.typeKey("v", modifierFlags: [.command])
        }
        func typeQuery(_ query: String, showing name: String) -> XCUIElement {
            app.typeText(query)
            XCTAssertTrue(waitUntil { (tags.value as? String ?? "").contains(query) },
                          "the unfinished query is editable text, not a pasted token")
            let choice = suggestion(name)
            XCTAssertTrue(choice.waitForExistence(timeout: 10), "native suggestion: \(name)")
            XCTAssertTrue(choice.isHittable, "the specific suggestion is visible before capture or selection")
            return choice
        }
        func afterAcceptance(_ name: String, capture label: String) {
            XCTAssertTrue(waitUntil { hasTag(name) }, "accepted tag: \(name)")
            XCTAssertTrue(hasTag(paths[0]) && hasTag("Kept::Token"), "adjacent tokens survive")
            screen(app, "browse-tags-\(label)-\(appearance)-synthetic")
        }

        seed(paths)
        app.typeKey(.tab, modifierFlags: [])
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil {
            guard fields.count > 0, let front = fields.element(boundBy: 0).value as? String else { return false }
            return front.contains("palabra") && front != firstFront
        })
        let secondFront = try XCTUnwrap(fields.element(boundBy: 0).value as? String)
        seed([paths[0], "Kept::Token"])
        let prefix = "spanish::grammar::"
        _ = typeQuery(prefix, showing: paths[1])
        screen(app, "browse-tags-expanded-\(appearance)-synthetic")
        app.typeKey(.downArrow, modifierFlags: [])
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertTrue((tags.value as? String ?? "").contains(prefix), "Escape preserves typed text")
        for name in paths.dropFirst() { XCTAssertFalse(hasTag(name), "Escape accepts nothing: \(name)") }
        for _ in prefix { app.typeKey(.delete, modifierFlags: []) }
        let keyboardQuery = "SPANISH::GRAMMAR::TEN"
        let keyboardChoice = typeQuery(keyboardQuery, showing: paths[7])
        app.typeKey(.downArrow, modifierFlags: [])
        XCTAssertTrue(keyboardChoice.isHittable)
        XCTAssertTrue((tags.value as? String ?? "").contains(keyboardQuery), "highlighting preserves typed text")
        screen(app, "browse-tags-keyboard-highlighted-\(appearance)-synthetic")
        app.typeKey(.return, modifierFlags: [])
        afterAcceptance(paths[7], capture: "after-keyboard-accept")
        XCTAssertFalse(hasTag(keyboardQuery), "acceptance replaces the unfinished token")

        // No click or focus command: typing the next query proves keyboard focus
        // survives acceptance, and the held tokens must survive that typing too.
        app.typeText(" ")
        let choice = typeQuery("ver", showing: paths[8])
        XCTAssertTrue(hasTag(paths[7]), "typing after acceptance preserves the accepted token")
        screen(app, "browse-tags-pointer-expanded-\(appearance)-synthetic")
        choice.click()
        afterAcceptance(paths[8], capture: "after-pointer-accept")
        XCTAssertFalse(hasTag("ver"), "pointer acceptance replaces the unfinished token")
        app.typeText(" Fresh::Created ")
        XCTAssertTrue(waitUntil { hasTag("Fresh::Created") }, "typing immediately after pointer acceptance retains focus")
        app.typeKey(.tab, modifierFlags: [])

        // Browse's real editing-end save, then a reload of each note.
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, firstFront))
        XCTAssertFalse(hasTag("Fresh::Created"))
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(shows(fields, secondFront))
        for name in [paths[0], paths[7], paths[8], "Kept::Token", "Fresh::Created"] {
            XCTAssertTrue(waitUntil { hasTag(name) }, "persisted tag: \(name)")
        }
        for name in paths[1..<7] {
            XCTAssertFalse(hasTag(name), "dismissed suggestion is never saved: \(name)")
        }
        for query in [prefix, "SPANISH::GRAMMAR::TEN", "ver"] {
            XCTAssertFalse(hasTag(query), "unfinished query is not saved as another token: \(query)")
        }

        // A highlighted offer is not acceptance. Both ordinary blur and a note
        // switch commit the typed token, never the highlighted full name.
        for (query, offered, switchNote) in [("Spanish::Grammar::NOU", paths[4], false),
                                             ("Spanish::Grammar::PRO", paths[6], true)] {
            tags.click()
            app.typeKey(.downArrow, modifierFlags: [.command])
            app.typeText(" ")
            _ = typeQuery(query, showing: offered)
            app.typeKey(.downArrow, modifierFlags: [])
            if !switchNote {
                app.typeKey(.tab, modifierFlags: [])
                XCTAssertTrue(waitUntil { hasTag(query) }, "blur preserves the typed token")
                XCTAssertFalse(hasTag(offered), "blur never accepts a highlight")
            }
            press(table.outlineRows.element(boundBy: 0))
            XCTAssertTrue(shows(fields, firstFront))
            press(table.outlineRows.element(boundBy: 1))
            XCTAssertTrue(shows(fields, secondFront))
            XCTAssertTrue(waitUntil { hasTag(query) }, "typed token persists after switching notes")
            XCTAssertFalse(hasTag(offered), "switching notes never accepts a highlight")
            XCTAssertTrue(hasTag(paths[0]) && hasTag(paths[7]) && hasTag(paths[8]) &&
                          hasTag("Kept::Token") && hasTag("Fresh::Created"), "existing tokens remain saved")
        }
    }

    /// H4 and Browse M4: bare Return belongs to the field, including consecutive
    /// Returns at an empty line. Use app-level keys to exercise shell routing.
    func testConsecutiveReturnsPersist() throws {
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            throw XCTSkip("Browse keyboard regression runs only in the Mac test VM")
        }
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        try showDeck(app, bundle, containing: "Español")
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 1 })
        press(table.outlineRows.element(boundBy: 0))
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(shows(fields, "palabra"))
        let originalFront = try XCTUnwrap(fields.element(boundBy: 0).value as? String)
        let field = fields.element(boundBy: 1)
        field.click()
        app.typeKey("a", modifierFlags: [.command])
        app.typeText("returnbeforereturnafter")
        for _ in "returnafter" { app.typeKey(.leftArrow, modifierFlags: []) }
        for _ in 0..<2 { app.typeKey(.return, modifierFlags: []) }
        app.typeText("returnmiddle")
        for _ in 0..<3 { app.typeKey(.return, modifierFlags: []) }
        app.typeText("returnlast")

        let expected = "returnbefore<div><br></div><div>returnmiddle</div>" +
            "<div><br></div><div><br></div><div>returnlastreturnafter</div>"
        let html = element(app, "editor.field.1.htmlToggle")
        XCTAssertTrue(html.isEnabled, "the field retains focus after every bare Return")
        html.click()
        XCTAssertTrue(waitUntil { field.value as? String == expected },
                      "two then three Returns preserve intervening blank lines and text on both sides: \(field.value ?? "")")
        html.click()
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil {
            guard fields.count > 1, let front = fields.element(boundBy: 0).value as? String else { return false }
            return front.contains("palabra") && front != originalFront
        }, "the other note has loaded before returning")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, originalFront))
        field.click()
        XCTAssertTrue(html.isEnabled)
        html.click()
        XCTAssertTrue(waitUntil { field.value as? String == expected }, "all blank lines survive the row-switch save")
    }

    /// P7/P11/P16: a real system-clipboard bitmap is inserted by Cmd-V at the
    /// caret, stored as media, and decoded again after loading another note.
    func testCommandVPastesBitmapAtCaretAndPersists() throws {
        // Check before launch or any pasteboard access; never inspect/save the
        // workstation's clipboard. The only data written here is synthetic.
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            throw XCTSkip("System clipboard regression runs only in the Mac test VM")
        }
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        try showDeck(app, bundle, containing: "Español")
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 1 })
        press(table.outlineRows.element(boundBy: 0))
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(shows(fields, "palabra"))
        let originalFront = try XCTUnwrap(fields.element(boundBy: 0).value as? String)
        let field = fields.element(boundBy: 1)
        field.click()
        app.typeKey("a", modifierFlags: [.command])
        app.typeText("picturebeforepictureafter")
        for _ in "pictureafter" { app.typeKey(.leftArrow, modifierFlags: []) }

        // Deliberately nonsquare and below the inspector's thumbnail limits.
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil,
            pixelsWide: 24, pixelsHigh: 12, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        let red = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)
        let blue = NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1)
        for y in 0..<12 {
            for x in 0..<24 { bitmap.setColor(x < 12 ? red : blue, atX: x, y: y) }
        }
        let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        defer { pasteboard.clearContents() }
        XCTAssertTrue(pasteboard.setData(png, forType: .png))
        app.typeKey("v", modifierFlags: [.command])
        let image = field.images.firstMatch
        func assertRenderedBitmap(_ name: String) throws {
            XCTAssertTrue(waitUntil(timeout: 20) { image.exists }, "the field exposes the pasted image")
            XCTAssertEqual(field.images.count, 1)
            let frame = image.frame
            XCTAssertTrue(field.frame.contains(frame), "the actual image is inside the edited field")
            // AX encloses fractional WebKit geometry by rounding outwards:
            // an exact 24 × 12 image can occupy a 25 × 13 AX rectangle.
            XCTAssertTrue([CGFloat(24), 25].contains(frame.width), "24 pt nominal width, outward-rounded: \(frame)")
            XCTAssertTrue([CGFloat(12), 13].contains(frame.height), "12 pt nominal height, outward-rounded: \(frame)")

            // Capture only the actual image element, never the fixture bitmap.
            let shot = image.screenshot()
            let attachment = XCTAttachment(screenshot: shot)
            attachment.name = name
            attachment.lifetime = .keepAlways
            add(attachment)
            let cgImage = try XCTUnwrap(shot.image.cgImage(forProposedRect: nil, context: nil, hints: nil))
            let rendered = NSBitmapImageRep(cgImage: cgImage)
            XCTAssertGreaterThanOrEqual(rendered.pixelsWide, 24)
            XCTAssertGreaterThanOrEqual(rendered.pixelsHigh, 12)
            // Interior 3 × 3 patches at 1/4 and 3/4 width avoid the caret,
            // rounded crop edges and the JPEG seam. Allow lossy encoding,
            // but require every sampled pixel to have the fixture's color.
            for (fraction, isRed) in [(0.25, true), (0.75, false)] {
                let x = Int(Double(rendered.pixelsWide) * fraction)
                let y = rendered.pixelsHigh / 2
                for dy in -1...1 {
                    for dx in -1...1 {
                        let color = try XCTUnwrap(rendered.colorAt(x: x + dx, y: y + dy)?.usingColorSpace(.sRGB))
                        let dominant = isRed ? color.redComponent : color.blueComponent
                        let opposite = isRed ? color.blueComponent : color.redComponent
                        XCTAssertGreaterThan(dominant, 0.65, "\(name): \(color)")
                        XCTAssertLessThan(max(color.greenComponent, opposite), 0.3, "\(name): \(color)")
                        XCTAssertGreaterThan(dominant, 2 * max(color.greenComponent, opposite), "\(name): \(color)")
                    }
                }
            }
        }
        try assertRenderedBitmap("browse-pasted-bitmap-at-caret-synthetic")

        let html = element(app, "editor.field.1.htmlToggle")
        XCTAssertTrue(html.isEnabled)
        html.click()
        let pattern = #"^picturebefore<img src="paste-[a-f0-9]{40}\.(jpg|png)">pictureafter$"#
        XCTAssertTrue(waitUntil {
            (field.value as? String ?? "").range(of: pattern, options: .regularExpression) != nil
        }, "paste stores a media reference exactly between the text around the caret")
        let savedHTML = try XCTUnwrap(field.value as? String)
        html.click()
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil {
            guard fields.count > 1, let front = fields.element(boundBy: 0).value as? String else { return false }
            return front.contains("palabra") && front != originalFront
        }, "the other note has loaded before returning")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, originalFront))
        try assertRenderedBitmap("browse-pasted-bitmap-after-row-switch-synthetic")
        field.click()
        html.click()
        XCTAssertTrue(waitUntil { field.value as? String == savedHTML }, "the pasted image and surrounding text persist exactly")
    }

    /// Synthetic typical-1 collection only; run in the Mac test VM.
    func testPinnedPreviewInteractions() throws {
        let (app, bundle) = try launch(appearance: "light")
        try openBrowse(app, bundle)
        try showDeck(app, bundle, containing: "Español")
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 1 })
        press(table.outlineRows.element(boundBy: 0))
        let page = element(app, "editor.fields"), fields = page.textViews
        XCTAssertTrue(shows(fields, "palabra"))
        let preview = element(app, "browse.inspector.preview")
        let picker = element(app, "browse.inspector.preview.side")
        XCTAssertTrue(preview.waitForExistence(timeout: 20) && picker.exists)
        let front = picker.radioButtons["Front"], back = picker.radioButtons["Back"]
        XCTAssertTrue(waitUntil { front.isEnabled && back.isEnabled }, "native accessible segments are ready")
        // typical.yaml uses "word {n}"; recipe_note appends image and sound
        // references to that field. Only its human-readable token is card text.
        let questionField = try XCTUnwrap(fields.element(boundBy: 0).value as? String)
        let questionRange = try XCTUnwrap(questionField.range(of: #"^palabra [0-9]+\b"#, options: .regularExpression))
        let question = String(questionField[questionRange])
        let answerField = try XCTUnwrap(fields.element(boundBy: 1).value as? String)
        let answerRange = try XCTUnwrap(answerField.range(of: #"^word [0-9]+\b"#, options: .regularExpression))
        let answer = String(answerField[answerRange])
        func cardText(_ text: String) -> XCUIElement {
            // CardPagePair exposes only the current webview in this AX subtree.
            // MATCHES consumes the whole string: e.g. (?s).*\bword 1\b.*
            // accepts WebKit's trailing space. Rendered text may be below the viewport.
            let pattern = #"(?s).*\b"# + NSRegularExpression.escapedPattern(for: text) + #"\b.*"#
            return preview.staticTexts.matching(NSPredicate(
                format: "label MATCHES %@ OR value MATCHES %@", pattern, pattern)).firstMatch
        }
        func answerRendered() -> Bool {
            cardText(answer).exists
        }
        func backRendered() -> Bool {
            back.value as? Int == 1 && answerRendered()
        }
        func frontRendered() -> Bool {
            front.value as? Int == 1 && cardText(question).exists && !answerRendered()
        }
        XCTAssertTrue(waitUntil { frontRendered() }, "Front's field content is rendered in the current card")
        capture(app, "browse-note-pinned-preview-initial-front-synthetic")
        back.click()
        let renderedBack = waitUntil { backRendered() }
        // Retain the natural selected note even when the rendered-side check fails.
        capture(app, "browse-note-pinned-preview-before-resize-synthetic")
        XCTAssertTrue(renderedBack, "Back's field content is rendered in the card")

        let count = fields.count
        let bar = element(app, "browse.inspector.formatting")
        let initialHeight = preview.frame.height, barBottom = bar.frame.maxY
        func dividerStart() -> XCUICoordinate {
            // Native VM evidence places the separator immediately below the preview.
            // Resolve a physical point afresh: a preview-relative saved coordinate
            // would move when a previous drag changes the preview's height.
            let window = app.windows.containing(.outline, identifier: "browse.results.table").firstMatch
            let frame = preview.frame, origin = window.frame.origin
            return window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: frame.midX - origin.x, dy: frame.maxY + 0.5 - origin.y))
        }
        try app.bringToFront(bundleIdentifier: bundle)
        let enlargeStart = dividerStart()
        enlargeStart.click(forDuration: 0.1, thenDragTo: enlargeStart.withOffset(CGVector(dx: 0, dy: 160)))
        XCTAssertTrue(waitUntil { preview.frame.height > initialHeight + 130 }, "dragging resizes the preview")
        XCTAssertGreaterThanOrEqual(bar.frame.maxY - preview.frame.maxY, 179, "the editor remains useful")
        XCTAssertEqual(bar.frame.maxY, barBottom, accuracy: 2, "formatting stays pinned")
        // Capture a normal selected note before editing or folding anything.
        XCTAssertTrue(waitUntil { backRendered() })
        capture(app, "browse-note-pinned-preview-synthetic")

        // Click-to-flip updates the same segmented picker.
        preview.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.9)).click()
        XCTAssertTrue(waitUntil { frontRendered() }, "Front is rendered before selecting Back again")
        back.click()
        XCTAssertTrue(waitUntil { backRendered() })

        fields.element(boundBy: 0).click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        app.typeText(" inspectionmarker")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])

        let editingHeight = preview.frame.height
        try app.bringToFront(bundleIdentifier: bundle)
        let editingStart = dividerStart()
        editingStart.click(forDuration: 0.1, thenDragTo: editingStart.withOffset(CGVector(dx: 0, dy: -30)))
        XCTAssertTrue(waitUntil { preview.frame.height < editingHeight - 15 })
        XCTAssertEqual(fields.count, count)
        XCTAssertEqual(bar.frame.maxY, barBottom, accuracy: 2, "formatting stays pinned")
        let bold = element(app, "browse.inspector.format.bold")
        XCTAssertTrue(bold.isEnabled, "dragging retains the editing selection")
        bold.click()
        // Browse uses the host's focused-field identifier; formatting emits <b>.
        let html = bar.descendants(matching: .any).matching(identifier: "editor.field.0.htmlToggle").firstMatch
        XCTAssertTrue(html.isEnabled)
        html.click()
        XCTAssertTrue(waitUntil { (fields.element(boundBy: 0).value as? String ?? "").contains("<b>inspectionmarker</b>") },
                      "formatting still acts on the selected text after the divider drag")
        html.click()

        let header = element(app, "browse.inspector.preview.header")
        header.click()
        XCTAssertTrue(waitUntil { !preview.exists })
        XCTAssertEqual(fields.count, count, "folding keeps the editor")
        header.click()
        XCTAssertTrue(preview.waitForExistence(timeout: 10))
        XCTAssertTrue(waitUntil { backRendered() && cardText("inspectionmarker").exists },
                      "folding and a blur-save preserve rendered Back with the edited FrontSide")
        // Drag to both bounds: both regions remain useful.
        for shrink in [true, false] {
            try app.bringToFront(bundleIdentifier: bundle)
            let height = preview.frame.height
            let target = shrink ? header.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 1)) :
                bar.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            let edge = dividerStart()
            edge.click(forDuration: 0.1, thenDragTo: target)
            XCTAssertTrue(waitUntil {
                shrink ? preview.frame.height < height - 15 : preview.frame.height > height + 30
            }, "each bounds drag changes the actual preview height")
            XCTAssertGreaterThanOrEqual(preview.frame.height, 95)
            XCTAssertGreaterThanOrEqual(bar.frame.maxY - preview.frame.maxY, 179)
            XCTAssertEqual(bar.frame.maxY, barBottom, accuracy: 2, "formatting stays pinned")
        }
        try app.bringToFront(bundleIdentifier: bundle)
        let edge = dividerStart()
        edge.click(forDuration: 0.1, thenDragTo: edge.withOffset(CGVector(dx: 0, dy: initialHeight - preview.frame.height)))
        XCTAssertTrue(waitUntil { abs(preview.frame.height - initialHeight) <= 2 }, "the preview returns to its initial height")
        XCTAssertEqual(bar.frame.maxY, barBottom, accuracy: 2, "formatting stays pinned")

        // Empty Fields is still the separate disclosure; an opened empty field stays open on clicks.
        fields.element(boundBy: 1).click()
        app.typeKey("a", modifierFlags: [.command])
        app.typeKey(.delete, modifierFlags: [])
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil { fields.count > 1 && !(fields.element(boundBy: 1).value as? String ?? "").isEmpty })
        press(table.outlineRows.element(boundBy: 0))
        let empty = element(app, "browse.inspector.emptyFields")
        XCTAssertTrue(empty.waitForExistence(timeout: 10))
        empty.click()
        XCTAssertTrue(waitUntil { fields.count == count })
        fields.element(boundBy: 1).click()
        XCTAssertEqual(fields.count, count)
        // Narrow overlay: the picker and card navigation must remain separate and reachable.
        // Español uses Basic; the Japanese reversed note actually has card navigation.
        try showDeck(app, bundle, containing: "語彙")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, "単語"))
        let nextCard = element(app, "browse.inspector.preview.nextCard")
        XCTAssertTrue(nextCard.waitForExistence(timeout: 10))
        try resize(app, bundle: bundle, to: "820x700")
        XCTAssertTrue(waitUntil { !preview.exists }, "the narrow inspector hides before reopening")
        try app.bringToFront(bundleIdentifier: bundle)
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).doubleClick()
        XCTAssertTrue(picker.waitForExistence(timeout: 10) && front.isHittable && back.isHittable)
        XCTAssertTrue(nextCard.isHittable, "card navigation remains reachable")
        XCTAssertFalse(picker.frame.intersects(header.frame), "Preview and side selection never overlap")
        XCTAssertFalse(picker.frame.intersects(nextCard.frame), "side selection and card navigation never overlap")
        capture(app, "browse-note-pinned-preview-narrow-synthetic")
    }

    private func inspector(appearance: String) throws {
        let (app, bundle) = try launch(appearance: appearance)
        try openBrowse(app, bundle)
        let table = resultsTable(app)
        XCTAssertTrue(waitUntil { table.outlineRows.count > 3 }, "rows: \(table.outlineRows.count)")

        // N21.3: the toggle is the toolbar's last item, after the Add menu.
        let toggle = element(app, "browse.toolbar.inspector"), add = element(app, "home.collection.add")
        XCTAssertTrue(toggle.waitForExistence(timeout: 10) && add.exists)
        XCTAssertLessThan(add.frame.maxX, toggle.frame.minX + 1, "the inspector toggle comes last: \(add.frame) \(toggle.frame)")

        // No Selection.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        XCUIElement.perform(withKeyModifiers: .command) { press(table.outlineRows.element(boundBy: 0)) }
        XCTAssertTrue(element(app, "browse.inspector.empty").waitForExistence(timeout: 10), "No Selection")
        capture(app, "no-selection-\(appearance)")

        // Several rows: the summary and the commands.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        XCUIElement.perform(withKeyModifiers: .shift) { press(table.outlineRows.element(boundBy: 3)) }
        XCTAssertTrue(element(app, "browse.inspector.summary.title").waitForExistence(timeout: 10), "V7's summary")
        sleep(1)
        capture(app, "several-rows-\(appearance)")

        // A card with a sound and an image (the Español deck's notes).
        try showDeck(app, bundle, containing: "Español")
        press(table.outlineRows.element(boundBy: 0))
        // The Note tab: its Deck pop-up, and the fields' page (the tab's own container is
        // folded into the pane's scroll view in the accessibility tree).
        let note = element(app, "browse.inspector.deck")
        XCTAssertTrue(note.waitForExistence(timeout: 10), "the Note tab")
        XCTAssertTrue(element(app, "browse.inspector.deck").exists && element(app, "browse.inspector.type").exists)
        XCTAssertTrue(element(app, "browse.inspector.tags").exists && element(app, "browse.inspector.formatting").exists)
        let fields = element(app, "editor.fields").textViews
        XCTAssertTrue(shows(fields, "palabra"), "the selected row's note")
        XCTAssertFalse(element(app, "browse.inspector.format.bold").isEnabled, "Option A: the bar is dimmed until a field has the focus")
        sleep(2)
        capture(app, "sound-and-image-\(appearance)")

        // Right to left, then CJK with a sound.
        try showDeck(app, bundle, containing: "العربية")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, "كلمة"), "the selected row's note")
        sleep(1)
        capture(app, "right-to-left-\(appearance)")
        // V12: a field emptied, then behind its row on the note's next showing, then shown.
        try app.bringToFront(bundleIdentifier: bundle)
        fields.element(boundBy: 1).click()
        app.typeKey("a", modifierFlags: [.command])
        app.typeKey(.delete, modifierFlags: [])
        press(table.outlineRows.element(boundBy: 1))
        XCTAssertTrue(waitUntil(timeout: 20) { fields.count > 1 && !(fields.element(boundBy: 1).value as? String ?? "").isEmpty },
                      "the next row's note, whose second field is filled")
        press(table.outlineRows.element(boundBy: 0))
        let empty = element(app, "browse.inspector.emptyFields")
        XCTAssertTrue(empty.waitForExistence(timeout: 10), "the emptied field hides behind its row")
        let shown = fields.count
        try app.bringToFront(bundleIdentifier: bundle)
        empty.click()
        XCTAssertTrue(waitUntil { fields.count > shown }, "every field shows: \(fields.count) after \(shown)")
        sleep(1)
        capture(app, "empty-fields-open-\(appearance)")
        empty.click()
        try showDeck(app, bundle, containing: "語彙")
        press(table.outlineRows.element(boundBy: 0))
        XCTAssertTrue(shows(fields, "単語"), "the selected row's note")
        sleep(2)
        capture(app, "cjk-with-sound-\(appearance)")

        // A field being edited, the formatting bar in use and ⌘I (F14) reaching the field.
        try app.bringToFront(bundleIdentifier: bundle)
        let first = fields.element(boundBy: 1)
        first.click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        app.typeText(" kerplunk")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        let bold = element(app, "browse.inspector.format.bold")
        XCTAssertTrue(waitUntil { bold.isEnabled }, "Bold acts on the focused field")
        bold.click()
        app.typeKey(.rightArrow, modifierFlags: [])
        app.typeText(" vintor")
        app.typeKey(.leftArrow, modifierFlags: [.option, .shift])
        app.typeKey("i", modifierFlags: [.command])
        capture(app, "field-being-edited-\(appearance)")

        // Many tags, wrapping in their section.
        try app.bringToFront(bundleIdentifier: bundle)
        element(app, "browse.inspector.tags").click()
        app.typeText("alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima ")
        app.typeKey(.tab, modifierFlags: [])
        sleep(1)
        capture(app, "many-tags-\(appearance)")

        // V12, W2: choosing another row saves the edit first, one "Edit Note" step.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 1))
        try app.bringToFront(bundleIdentifier: bundle)
        app.menuBars.menuBarItems["Edit"].click()
        XCTAssertTrue(app.menuItems["Undo Edit Note"].waitForExistence(timeout: 10), "V8.3: the save's step")
        app.typeKey(.escape, modifierFlags: [])

        // A long note: many lines typed; one scroll view for the whole pane.
        try app.bringToFront(bundleIdentifier: bundle)
        fields.element(boundBy: 0).click()
        app.typeKey(.downArrow, modifierFlags: [.command])
        for line in 1...14 { app.typeText("\rline \(line) of a long field that wraps across the pane") }
        capture(app, "long-note-\(appearance)")
        let bar = element(app, "browse.inspector.formatting")
        XCTAssertTrue(bar.frame.maxY <= app.windows.firstMatch.frame.maxY + 1, "the bar stays at the pane's bottom edge")

        // N21.3: under about 900 points the pane hides; a double click shows it over the
        // table, the toolbar's toggle hides it, and Return on the selected row shows it again.
        try app.bringToFront(bundleIdentifier: bundle)
        press(table.outlineRows.element(boundBy: 0))
        try resize(app, bundle: bundle, to: "820x700")
        XCTAssertTrue(waitUntil { !note.exists }, "the pane hid itself")
        try app.bringToFront(bundleIdentifier: bundle)
        table.outlineRows.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).doubleClick()
        XCTAssertTrue(note.waitForExistence(timeout: 10), "a double click shows it over the table")
        sleep(1)
        capture(app, "narrow-over-table-\(appearance)")
        element(app, "browse.toolbar.inspector").click()
        XCTAssertTrue(waitUntil { !note.exists }, "the toolbar's toggle hides it")
        press(table.outlineRows.element(boundBy: 0))
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(note.waitForExistence(timeout: 10), "Return shows it over the table")
    }

    /// Browse's sidebar: the deck row whose name holds `name` sets the Deck token. The
    /// table has that deck's results once the status line counts what the row counts
    /// (the previous results stay shown while a search runs).
    private func showDeck(_ app: XCUIApplication, _ bundle: String, containing name: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let row = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "browse.sidebar.deck.", name)).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the deck row \(name)")
        // "90 cards" on the row; "90 of 415 cards · 1 selected" on the status line.
        let count = (row.value as? String ?? "").split(separator: " ").first.map(String.init) ?? ""
        row.click()
        let status = element(app, "browse.status.text")
        XCTAssertTrue(waitUntil(timeout: 30) { !count.isEmpty && (status.value as? String ?? "").hasPrefix(count + " ") },
                      "the deck's own results: \(name) \(count) \(status.value as? String ?? "")")
        try app.bringToFront(bundleIdentifier: bundle)
    }

    private func launch(appearance: String) throws -> (XCUIApplication, String) {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("The generated recipe requires an isolated development test container")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1", directoryHint: .isDirectory),
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

    private func resize(_ app: XCUIApplication, bundle: String, to size: String) throws {
        let resized = expectation(description: "The main window resized")
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name(bundle + ".testhooks.windowResized"),
            object: nil, queue: .main) { _ in resized.fulfill() }
        defer { center.removeObserver(observer) }
        center.postNotificationName(Notification.Name(bundle + ".testhooks.resizeWindow"), object: size,
            userInfo: nil, deliverImmediately: true)
        wait(for: [resized], timeout: 10)
    }

    /// The inspector shows the selected row's note: its first field holds `text` (the
    /// previous note's page stays shown while the next one is read).
    private func shows(_ fields: XCUIElementQuery, _ text: String) -> Bool {
        waitUntil(timeout: 20) { fields.count > 0 && (fields.element(boundBy: 0).value as? String ?? "").contains(text) }
    }

    /// SwiftUI's Table is an outline in the Mac's accessibility tree.
    private func resultsTable(_ app: XCUIApplication) -> XCUIElement {
        app.outlines.matching(identifier: "browse.results.table").firstMatch
    }

    /// A row is clicked at its point: XCTest's hit test reads a SwiftUI table's rows as not hittable.
    private func press(_ row: XCUIElement) {
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5)).click()
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        let window = app.windows.containing(.outline, identifier: "browse.results.table").firstMatch
        XCTAssertTrue(window.exists, "capture the Browse window")
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

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
