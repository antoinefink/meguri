#if os(macOS)
import AppKit
import Foundation
import XCTest

/// VM-only, production UI and core; no editor routes or injected masks.
/// Integration AX contract: io.editor.canvas bounds are the fitted image,
/// without letterboxing; io.mask.1 / io.mask.2 expose each mask's drawn bounds
/// (editor/image-occlusion-strings S7). Paste Image uses its native caption.
@MainActor final class IOAddEditUITests: XCTestCase {
    private var inputApplication: XCUIApplication?
    private var inputBundleIdentifier = ""

    func testNativeImageOcclusionAddAndEditPersistsDraggedMasks() throws {
        // Must precede launch, synthetic input, and all clipboard access.
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            throw XCTSkip("Image occlusion journey runs only in the Mac test VM")
        }
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else {
            throw XCTSkip("Requires disposable development collection hooks")
        }
        let app = XCUIApplication()
        inputApplication = app
        inputBundleIdentifier = bundle
        // The installer copies this public empty directory into the Dev
        // container. All collection/media writes belong to that installed copy.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US",
            animations: false, windowSize: "1440x900", appearance: "light").arguments
        app.launch()
        defer { app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        try click(element(app, "home.collection.startEmpty"))
        XCTAssertTrue(wait { HomeControls.hittable(HomeControls.deckRows(app, deckID: "1")) != nil })
        try click(try XCTUnwrap(HomeControls.hittable(HomeControls.deckRows(app, deckID: "1"))))
        let page = element(app, "home.deck.page.1")
        XCTAssertTrue(page.waitForExistence(timeout: 20))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 0))

        // File > New Note, not a direct presentation hook.
        try prepareInput()
        app.typeKey("n", modifierFlags: .command)
        let addWindow = app.windows["editor.add.window"]
        XCTAssertTrue(addWindow.waitForExistence(timeout: 15))
        try enlarge(addWindow, in: app)
        try click(addWindow.popUpButtons["editor.add.type"])
        try click(app.menuItems["Image Occlusion"])
        let paste = addWindow.buttons["Paste Image"]
        XCTAssertTrue(paste.waitForExistence(timeout: 15), "the on-demand stock type opens the image picker")

        // Never read or save the workstation clipboard. Only generated pixels
        // enter the VM clipboard, which is cleared even after a failed assertion.
        let png = try geometryDiagram()
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        defer { pasteboard.clearContents() }
        XCTAssertTrue(pasteboard.setData(png, forType: .png))
        try click(paste)
        let addCanvas = element(addWindow, "io.editor.canvas")
        try assertReadableCanvas(addCanvas, window: addWindow, stage: "add")

        // O4.2 opens Rectangle. These are real mouse drags over the two
        // diagram labels, expressed as fractions of the fitted source image.
        try drag(addCanvas, from: CGVector(dx: 0.05, dy: 0.50), to: CGVector(dx: 0.31, dy: 0.62))
        XCTAssertTrue(element(addWindow, "io.mask.1").waitForExistence(timeout: 10))
        try drag(addCanvas, from: CGVector(dx: 0.69, dy: 0.50), to: CGVector(dx: 0.95, dy: 0.62))
        XCTAssertTrue(element(addWindow, "io.mask.2").waitForExistence(timeout: 10))
        try deselect(in: addCanvas, app: app)
        let original = try maskBounds(in: addWindow, canvas: addCanvas)
        assertBounds(original, equalTo: [
            CGRect(x: 0.05, y: 0.50, width: 0.26, height: 0.12),
            CGRect(x: 0.69, y: 0.50, width: 0.26, height: 0.12)
        ])
        capture(addWindow, named: "image-occlusion-add-note-two-drawn-masks-synthetic")
        try click(addWindow.buttons["editor.add.save"])
        XCTAssertTrue(element(addWindow, "editor.add.result").waitForExistence(timeout: 20))
        XCTAssertFalse(element(addWindow, "editor.add.error").exists)
        XCTAssertTrue(paste.waitForExistence(timeout: 15), "the next draft has no image or masks")
        try click(addWindow.buttons["editor.add.close"])
        XCTAssertTrue(addWindow.waitForNonExistence(timeout: 15))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 2, learning: 0, review: 0),
                      "two independent numbered masks generate exactly two cards in the empty deck")

        try click(page.buttons["home.deck.study"])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        let editWindow = try openEditCurrent(app, bundle: bundle)
        let editCanvas = element(editWindow, "io.editor.canvas")
        try assertReadableCanvas(editCanvas, window: editWindow, stage: "edit")
        assertBounds(try maskBounds(in: editWindow, canvas: editCanvas), equalTo: original)

        // Edit opens Select. Drag the first mask's interior, away from handles.
        let first = element(editWindow, "io.mask.1")
        try prepareInput()
        let start = first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.click(forDuration: 0.1, thenDragTo: start.withOffset(
            CGVector(dx: editCanvas.frame.width * 0.04, dy: editCanvas.frame.height * 0.14)))
        try deselect(in: editCanvas, app: app)
        XCTAssertTrue(wait {
            let frame = first.frame, image = editCanvas.frame
            return image.height > 0 && (frame.minY - image.minY) / image.height > original[0].minY + 0.10
        }, "the mouse drag must move the actual mask before capture")
        let edited = try maskBounds(in: editWindow, canvas: editCanvas)
        XCTAssertEqual(edited[0].minX, original[0].minX + 0.04, accuracy: 0.015)
        XCTAssertEqual(edited[0].minY, original[0].minY + 0.14, accuracy: 0.015)
        assertBounds([edited[1]], equalTo: [original[1]])
        capture(editWindow, named: "image-occlusion-edit-note-moved-mask-synthetic")
        try click(editWindow.buttons["review.editCurrent.done"])
        XCTAssertTrue(editWindow.waitForNonExistence(timeout: 20))

        // Reload the stored note through the same normal entry, with no fixture
        // reinstallation or draft reuse. Both coordinates and sizes must persist.
        let reopened = try openEditCurrent(app, bundle: bundle)
        let reloadedCanvas = element(reopened, "io.editor.canvas")
        try assertReadableCanvas(reloadedCanvas, window: reopened, stage: "reopened")
        assertBounds(try maskBounds(in: reopened, canvas: reloadedCanvas), equalTo: edited)
        capture(reopened, named: "image-occlusion-edit-note-persisted-after-reopen-synthetic")
        try click(reopened.buttons["review.editCurrent.cancel"])
        XCTAssertTrue(reopened.waitForNonExistence(timeout: 15))
    }

    private func openEditCurrent(_ app: XCUIApplication, bundle: String) throws -> XCUIElement {
        try app.bringToFront(bundleIdentifier: bundle)
        let command = app.menuBars.menuBarItems["Note"].menuItems.matching(
            NSPredicate(format: "title == %@", "Edit Note")).firstMatch
        XCTAssertTrue(wait { command.exists && command.isEnabled })
        try prepareInput()
        app.typeKey("e", modifierFlags: .command)
        let window = app.windows["review.editCurrent.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 15))
        try enlarge(window, in: app)
        return window
    }

    /// Native title-bar and resize-border drags keep screenshots readable. Only
    /// the VM's app windows receive input; no window-server manipulation hooks.
    private func enlarge(_ window: XCUIElement, in app: XCUIApplication) throws {
        let workspace = app.windows["main"].frame
        XCTAssertGreaterThan(workspace.width, 1100)
        XCTAssertGreaterThan(workspace.height, 780)
        let frame = window.frame
        let title = window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            .withOffset(CGVector(dx: 0, dy: 12))
        try prepareInput()
        title.click(forDuration: 0.1, thenDragTo: title.withOffset(CGVector(
            dx: workspace.minX + 30 - frame.minX, dy: workspace.minY + 30 - frame.minY)))
        let edge = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
            .withOffset(CGVector(dx: -2, dy: -2))
        try prepareInput()
        edge.click(forDuration: 0.1, thenDragTo: edge.withOffset(CGVector(
            dx: 1040 - window.frame.width, dy: 720 - window.frame.height)))
        XCTAssertTrue(wait { window.frame.width >= 1000 && window.frame.height >= 680 },
                      "the complete native editor window must be large enough to read")
    }

    private func assertReadableCanvas(_ canvas: XCUIElement, window: XCUIElement, stage: String) throws {
        let appeared = canvas.waitForExistence(timeout: 20)
        // Keep the real editor window and its global AX frames even when the
        // first geometry assertion stops this journey. No desktop/menu capture.
        capture(window, named: "image-occlusion-\(stage)-authoring-surface-synthetic")
        let hierarchy = XCTAttachment(string: window.debugDescription)
        hierarchy.name = "image-occlusion-\(stage)-authoring-surface-ax"
        hierarchy.lifetime = .keepAlways
        add(hierarchy)
        XCTAssertTrue(appeared)
        XCTAssertTrue(wait { canvas.frame.width >= 600 && canvas.frame.height >= 330 })
        XCTAssertTrue(window.frame.contains(canvas.frame), "capture includes the full image")
        XCTAssertEqual(canvas.frame.width / canvas.frame.height, 960.0 / 540.0, accuracy: 0.02,
                       "canvas AX bounds must follow the fitted image, excluding letterboxing")
    }

    private func drag(_ canvas: XCUIElement, from: CGVector, to: CGVector) throws {
        try prepareInput()
        canvas.coordinate(withNormalizedOffset: from).click(forDuration: 0.1,
            thenDragTo: canvas.coordinate(withNormalizedOffset: to))
    }

    private func deselect(in canvas: XCUIElement, app: XCUIApplication) throws {
        try prepareInput()
        app.typeKey("s", modifierFlags: [])
        try prepareInput()
        canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.94)).click()
    }

    private func maskBounds(in window: XCUIElement, canvas: XCUIElement) throws -> [CGRect] {
        let image = canvas.frame
        XCTAssertGreaterThan(image.width, 0)
        XCTAssertGreaterThan(image.height, 0)
        return (1...2).map { number in
            let mask = element(window, "io.mask.\(number)")
            XCTAssertTrue(mask.waitForExistence(timeout: 10))
            let rect = mask.frame
            XCTAssertGreaterThan(rect.width, 10)
            XCTAssertGreaterThan(rect.height, 10)
            XCTAssertTrue(image.contains(rect))
            return CGRect(x: (rect.minX - image.minX) / image.width,
                          y: (rect.minY - image.minY) / image.height,
                          width: rect.width / image.width, height: rect.height / image.height)
        }
    }

    private func assertBounds(_ actual: [CGRect], equalTo expected: [CGRect]) {
        XCTAssertEqual(actual.count, expected.count)
        for (a, e) in zip(actual, expected) {
            XCTAssertEqual(a.minX, e.minX, accuracy: 0.015)
            XCTAssertEqual(a.minY, e.minY, accuracy: 0.015)
            XCTAssertEqual(a.width, e.width, accuracy: 0.015)
            XCTAssertEqual(a.height, e.height, accuracy: 0.015)
        }
    }

    private func capture(_ window: XCUIElement, named name: String) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func click(_ target: XCUIElement) throws {
        XCTAssertTrue(wait { target.exists && target.isEnabled && target.isHittable })
        try prepareInput()
        target.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
    }

    private func prepareInput() throws {
        let app = try XCTUnwrap(inputApplication)
        try app.bringToFront(bundleIdentifier: inputBundleIdentifier)
    }

    private func element(_ root: XCUIElement, _ identifier: String) -> XCUIElement {
        root.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func wait(_ condition: () -> Bool) -> Bool {
        let end = Date().addingTimeInterval(20)
        while Date() < end {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return condition()
    }

    /// Original educational asset, drawn directly into pixels in the test
    /// process. This image is pasted into the real editor, never used as a UI
    /// screenshot or a replacement for an attachment of the running app.
    private func geometryDiagram() throws -> Data {
        let bitmap = try XCTUnwrap(NSBitmapImageRep(bitmapDataPlanes: nil,
            pixelsWide: 960, pixelsHigh: 540, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 0))
        let context = try XCTUnwrap(NSGraphicsContext(bitmapImageRep: bitmap))
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = context
        // Use top-left coordinates to match the image-fraction input above.
        context.cgContext.translateBy(x: 0, y: 540)
        context.cgContext.scaleBy(x: 1, y: -1)
        NSColor.white.setFill()
        NSBezierPath(rect: NSRect(x: 0, y: 0, width: 960, height: 540)).fill()
        func text(_ value: String, x: CGFloat, y: CGFloat, size: CGFloat, bold: Bool = false) {
            let font = bold ? NSFont.boldSystemFont(ofSize: size) : NSFont.systemFont(ofSize: size)
            (value as NSString).draw(at: NSPoint(x: x, y: y), withAttributes: [
                .font: font, .foregroundColor: NSColor.black])
        }
        // AppKit text needs a flipped graphics context as well as the transform.
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context.cgContext, flipped: true)
        text("ISOSCELES TRIANGLE", x: 245, y: 28, size: 36, bold: true)
        text("Equal sides meet at vertex A", x: 279, y: 82, size: 25)
        let triangle = NSBezierPath()
        triangle.move(to: NSPoint(x: 480, y: 165))
        triangle.line(to: NSPoint(x: 300, y: 420))
        triangle.line(to: NSPoint(x: 660, y: 420))
        triangle.close()
        NSColor(srgbRed: 0.88, green: 0.94, blue: 1, alpha: 1).setFill()
        triangle.fill()
        NSColor(srgbRed: 0.08, green: 0.25, blue: 0.45, alpha: 1).setStroke()
        triangle.lineWidth = 4
        triangle.stroke()
        text("A", x: 470, y: 124, size: 30, bold: true)
        text("B", x: 265, y: 412, size: 30, bold: true)
        text("C", x: 678, y: 412, size: 30, bold: true)
        text("Side a: 5 cm", x: 60, y: 280, size: 30, bold: true)
        text("Side b: 5 cm", x: 670, y: 280, size: 30, bold: true)
        text("Base c: 6 cm", x: 375, y: 445, size: 28)
        text("Recall both equal side lengths", x: 288, y: 497, size: 24)
        context.cgContext.flush()
        return try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
    }
}
#endif
