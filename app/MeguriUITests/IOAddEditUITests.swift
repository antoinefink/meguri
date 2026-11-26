#if os(macOS)
import AppKit
import Foundation
import XCTest

/// VM-only, production UI and core; no editor routes or injected masks.
/// Integration AX contract: native io.editor.canvas bounds are the fitted image;
/// io.canvas is the viewport. io.mask.1 / io.mask.2 expose each mask's drawn bounds
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
        let pixels = try XCTUnwrap(NSBitmapImageRep(data: png))
        let imageSize = CGSize(width: pixels.pixelsWide, height: pixels.pixelsHigh)
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        defer { pasteboard.clearContents() }
        XCTAssertTrue(pasteboard.setData(png, forType: .png))
        try click(paste)
        let addCanvas = try readableCanvas(in: addWindow, imageSize: imageSize, stage: "add")

        // O4.2 opens Rectangle. These are real mouse drags over the two
        // diagram labels, expressed as fractions of the fitted source image.
        try drag(addCanvas, from: CGVector(dx: 0.05, dy: 0.50), to: CGVector(dx: 0.31, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-first-mask-immediately-after-input-synthetic")
        try requireMask(1, in: addWindow, stage: "add-first")
        try drag(addCanvas, from: CGVector(dx: 0.69, dy: 0.50), to: CGVector(dx: 0.95, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-two-masks-immediately-after-input-synthetic")
        try requireMask(2, in: addWindow, stage: "add-second")
        try deselect(in: addCanvas, app: app)
        capture(addWindow, named: "image-occlusion-add-note-two-drawn-masks-synthetic")
        assertNativeImage(addCanvas, in: addWindow, stage: "add-completed")
        let original = try maskBounds(in: addWindow, canvas: addCanvas)
        assertBounds(original, equalTo: [
            CGRect(x: 0.05, y: 0.50, width: 0.26, height: 0.12),
            CGRect(x: 0.69, y: 0.50, width: 0.26, height: 0.12)
        ])
        // The actual fields/masks composition must hide the native subtree,
        // restore it, and preserve every mask without an intervening save.
        try click(addWindow.buttons["io.maskEditor.toggle"])
        capture(addWindow, named: "image-occlusion-add-fields-after-toggle-synthetic")
        let hidden = addCanvas.viewport.waitForNonExistence(timeout: 10)
        if !hidden { captureToolbar(addWindow, stage: "fields-canvas-unexpectedly-visible") }
        XCTAssertTrue(hidden)
        XCTAssertFalse(element(addWindow, "io.mask.1").exists)
        XCTAssertFalse(element(addWindow, "io.mask.2").exists)
        try click(addWindow.buttons["io.maskEditor.toggle"])
        let restored = try readableCanvas(in: addWindow, imageSize: imageSize, stage: "restored-masks")
        assertNativeImage(restored, in: addWindow, stage: "restored-masks")
        assertBounds(try maskBounds(in: addWindow, canvas: restored), equalTo: original)
        try click(addWindow.buttons["editor.add.save"])
        capture(addWindow, named: "image-occlusion-add-immediately-after-save-input-synthetic")
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
        let editCanvas = try readableCanvas(in: editWindow, imageSize: imageSize, stage: "edit")
        assertNativeImage(editCanvas, in: editWindow, stage: "edit")
        assertBounds(try maskBounds(in: editWindow, canvas: editCanvas), equalTo: original)

        // Edit opens Select. Drag the first mask's interior, away from handles.
        let first = element(editWindow, "io.mask.1")
        try prepareInput()
        let start = first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.click(forDuration: 0.1, thenDragTo: start.withOffset(
            CGVector(dx: editCanvas.frame.width * 0.04, dy: editCanvas.frame.height * 0.14)))
        capture(editWindow, named: "image-occlusion-edit-moved-mask-immediately-after-input-synthetic")
        try deselect(in: editCanvas, app: app)
        capture(editWindow, named: "image-occlusion-edit-note-moved-mask-synthetic")
        XCTAssertTrue(wait {
            let frame = first.frame, image = editCanvas.frame
            return image.height > 0 && (frame.minY - image.minY) / image.height > original[0].minY + 0.10
        }, "the mouse drag must move the actual mask before capture")
        let edited = try maskBounds(in: editWindow, canvas: editCanvas)
        XCTAssertEqual(edited[0].minX, original[0].minX + 0.04, accuracy: 0.015)
        XCTAssertEqual(edited[0].minY, original[0].minY + 0.14, accuracy: 0.015)
        assertBounds([edited[1]], equalTo: [original[1]])
        try click(editWindow.buttons["review.editCurrent.done"])
        XCTAssertTrue(editWindow.waitForNonExistence(timeout: 20))

        // Reload the stored note through the same normal entry, with no fixture
        // reinstallation or draft reuse. Both coordinates and sizes must persist.
        let reopened = try openEditCurrent(app, bundle: bundle)
        let reloadedCanvas = try readableCanvas(in: reopened, imageSize: imageSize, stage: "reopened")
        capture(reopened, named: "image-occlusion-edit-note-persisted-after-reopen-synthetic")
        assertNativeImage(reloadedCanvas, in: reopened, stage: "reopened")
        assertBounds(try maskBounds(in: reopened, canvas: reloadedCanvas), equalTo: edited)
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

    /// Input can still complete and produce evidence if the fitted child is
    /// unexpectedly absent. Only the real AX viewport is awaited; measured
    /// source pixels determine its letterboxing. Native-image absence fails the
    /// completed-state contract below, after the immediate input captures.
    @MainActor private struct DrawingSurface {
        let viewport: XCUIElement
        let nativeImage: XCUIElement
        let imageSize: CGSize
        var fittedFrame: CGRect {
            let area = viewport.frame
            let scale = min(area.width / imageSize.width, area.height / imageSize.height)
            let size = CGSize(width: imageSize.width * scale, height: imageSize.height * scale)
            return CGRect(x: area.midX - size.width / 2, y: area.midY - size.height / 2,
                          width: size.width, height: size.height)
        }
        var frame: CGRect { nativeImage.exists ? nativeImage.frame : fittedFrame }
        func coordinate(withNormalizedOffset point: CGVector) -> XCUICoordinate {
            if nativeImage.exists { return nativeImage.coordinate(withNormalizedOffset: point) }
            let image = fittedFrame, area = viewport.frame
            return viewport.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: image.minX - area.minX + point.dx * image.width,
                dy: image.minY - area.minY + point.dy * image.height))
        }
    }

    private func readableCanvas(in window: XCUIElement, imageSize: CGSize, stage: String) throws -> DrawingSurface {
        let viewport = element(window, "io.canvas")
        let appeared = viewport.waitForExistence(timeout: 20)
        capture(window, named: "image-occlusion-\(stage)-authoring-surface-synthetic")
        captureHierarchy(window, stage: stage)
        if !appeared { captureToolbar(window, stage: stage) }
        XCTAssertTrue(appeared)
        XCTAssertTrue(wait { viewport.frame.width >= 600 && viewport.frame.height >= 330 })
        XCTAssertTrue(window.frame.contains(viewport.frame), "capture includes the full viewport")
        let result = DrawingSurface(viewport: viewport, nativeImage: element(window, "io.editor.canvas"), imageSize: imageSize)
        if !result.nativeImage.exists { captureToolbar(window, stage: "\(stage)-image-absent") }
        let zoom = element(window, "io.zoom")
        if !zoom.exists { captureToolbar(window, stage: "\(stage)-zoom-absent") }
        XCTAssertTrue(zoom.exists)
        XCTAssertEqual(zoom.value as? String, "100%", "the first usable image layout opens at 100% within pixel rounding")
        return result
    }

    private func assertNativeImage(_ surface: DrawingSurface, in window: XCUIElement, stage: String) {
        if !surface.nativeImage.exists {
            captureToolbar(window, stage: stage)
            captureHierarchy(window, stage: stage)
        }
        XCTAssertTrue(surface.nativeImage.exists, "the fitted image must be a native AX image child")
        XCTAssertEqual(surface.nativeImage.elementType, .image)
        let actual = surface.nativeImage.frame, fitted = surface.fittedFrame
        XCTAssertGreaterThan(actual.width, 600)
        XCTAssertGreaterThan(actual.height, 330)
        XCTAssertTrue(window.frame.contains(actual))
        XCTAssertEqual(actual.minX, fitted.minX, accuracy: 1)
        XCTAssertEqual(actual.minY, fitted.minY, accuracy: 1)
        XCTAssertEqual(actual.width, fitted.width, accuracy: 1)
        XCTAssertEqual(actual.height, fitted.height, accuracy: 1)
        XCTAssertEqual(actual.width / actual.height, surface.imageSize.width / surface.imageSize.height, accuracy: 0.02)
    }

    private func requireMask(_ number: Int, in window: XCUIElement, stage: String) throws {
        let present = element(window, "io.mask.\(number)").waitForExistence(timeout: 10)
        if !present {
            capture(window, named: "image-occlusion-\(stage)-unexpected-mask-absence-synthetic")
            captureToolbar(window, stage: stage)
            captureHierarchy(window, stage: stage)
        }
        XCTAssertTrue(present, "real input must create the native mask AX element")
    }

    private func captureHierarchy(_ window: XCUIElement, stage: String) {
        let hierarchy = XCTAttachment(string: window.debugDescription)
        hierarchy.name = "image-occlusion-\(stage)-authoring-surface-ax"
        hierarchy.lifetime = .keepAlways
        add(hierarchy)
    }

    private func captureToolbar(_ window: XCUIElement, stage: String) {
        // The mask tools are in the window content, rather than NSToolbar.
        // Keep both its pixels and AX snapshot after any unexpected absence.
        capture(window, named: "image-occlusion-\(stage)-toolbar-diagnostic-synthetic")
        captureHierarchy(window, stage: "\(stage)-toolbar")
    }

    private func drag(_ canvas: DrawingSurface, from: CGVector, to: CGVector) throws {
        try prepareInput()
        canvas.coordinate(withNormalizedOffset: from).click(forDuration: 0.1,
            thenDragTo: canvas.coordinate(withNormalizedOffset: to))
    }

    private func deselect(in canvas: DrawingSurface, app: XCUIApplication) throws {
        try prepareInput()
        app.typeKey("s", modifierFlags: [])
        try prepareInput()
        canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.94)).click()
    }

    private func maskBounds(in window: XCUIElement, canvas: DrawingSurface) throws -> [CGRect] {
        let image = canvas.frame
        XCTAssertGreaterThan(image.width, 0)
        XCTAssertGreaterThan(image.height, 0)
        return try (1...2).map { number in
            try requireMask(number, in: window, stage: "geometry-\(number)")
            let mask = element(window, "io.mask.\(number)")
            XCTAssertEqual(mask.elementType, .button)
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
