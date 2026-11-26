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
        try sizeWindow(addWindow)
        try prepareInput()
        addWindow.popUpButtons["editor.add.type"].click()
        app.menuItems["Image Occlusion"].click()
        let paste = addWindow.buttons["Paste Image"]
        XCTAssertTrue(paste.waitForExistence(timeout: 15), "the on-demand stock type opens the image picker")
        try assertForm(in: addWindow, hasImage: false)
        capture(addWindow, named: "image-occlusion-add-note-no-image-native-form-synthetic")

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
        try assertForm(in: addWindow, hasImage: true)
        assertFitDisplay(in: addWindow)

        // O4.2 opens Rectangle. These are real mouse drags over the two
        // diagram labels, expressed as fractions of the fitted source image.
        try drag(addCanvas, from: CGVector(dx: 0.05, dy: 0.50), to: CGVector(dx: 0.31, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-first-mask-immediately-after-input-synthetic")
        try requireMask(1, in: addWindow, stage: "add-first")
        try drag(addCanvas, from: CGVector(dx: 0.69, dy: 0.50), to: CGVector(dx: 0.95, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-two-masks-immediately-after-input-synthetic")
        try requireMask(2, in: addWindow, stage: "add-second")
        try deselect(in: addCanvas, app: app)
        assertNativeImage(addCanvas, in: addWindow, stage: "add-completed")
        let original = try maskBounds(in: addWindow, canvas: addCanvas)
        assertBounds(original, equalTo: [
            CGRect(x: 0.05, y: 0.50, width: 0.26, height: 0.12),
            CGRect(x: 0.69, y: 0.50, width: 0.26, height: 0.12)
        ])
        // Option B keeps ordinary fields and both native masks visible while
        // typing. Focus the actual editable role, never the Header label.
        let header = "Isosceles triangle"
        let headerInput = try formField("Header", in: addWindow)
        try click(headerInput)
        try prepareInput()
        app.typeText(header)
        try prepareInput()
        app.typeKey(.tab, modifierFlags: [])
        try assertHeader(header, in: addWindow)
        try assertForm(in: addWindow, hasImage: true)
        assertTwoCardFooter(in: addWindow)
        assertBounds(try maskBounds(in: addWindow, canvas: addCanvas), equalTo: original)
        capture(addWindow, named: "image-occlusion-add-note-two-drawn-masks-synthetic")
        try click(addWindow.buttons["editor.add.save"])
        capture(addWindow, named: "image-occlusion-add-immediately-after-save-input-synthetic")
        XCTAssertTrue(element(addWindow, "editor.add.result").waitForExistence(timeout: 20))
        XCTAssertFalse(element(addWindow, "editor.add.error").exists)
        XCTAssertTrue(paste.waitForExistence(timeout: 15), "the next draft has no image or masks")
        try assertForm(in: addWindow, hasImage: false)
        try click(addWindow.buttons["editor.add.close"])
        XCTAssertTrue(addWindow.waitForNonExistence(timeout: 15))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 2, learning: 0, review: 0),
                      "two independent numbered masks generate exactly two cards in the empty deck")

        try click(page.buttons["home.deck.study"])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        let editWindow = try openEditCurrent(app, bundle: bundle, imageSize: imageSize)
        let editCanvas = try readableCanvas(in: editWindow, imageSize: imageSize, stage: "edit")
        assertNativeImage(editCanvas, in: editWindow, stage: "edit")
        try assertForm(in: editWindow, hasImage: true)
        try assertHeader(header, in: editWindow)
        assertTwoCardFooter(in: editWindow)
        assertBounds(try maskBounds(in: editWindow, canvas: editCanvas), equalTo: original)

        // Edit opens Select. Drag the first mask's interior, away from handles.
        let first = element(editWindow, "io.mask.1")
        try prepareInput()
        let start = first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.click(forDuration: 0.1, thenDragTo: start.withOffset(
            CGVector(dx: editCanvas.frame.width * 0.04, dy: editCanvas.frame.height * 0.14)))
        capture(editWindow, named: "image-occlusion-edit-moved-mask-immediately-after-input-synthetic")
        try deselect(in: editCanvas, app: app)
        XCTAssertTrue(wait {
            let frame = first.frame, image = editCanvas.frame
            return image.height > 0 && (frame.minY - image.minY) / image.height > original[0].minY + 0.10
        }, "the mouse drag must move the actual mask before capture")
        capture(editWindow, named: "image-occlusion-edit-note-moved-mask-synthetic")
        let edited = try maskBounds(in: editWindow, canvas: editCanvas)
        XCTAssertEqual(edited[0].minX, original[0].minX + 0.04, accuracy: 0.015)
        XCTAssertEqual(edited[0].minY, original[0].minY + 0.14, accuracy: 0.015)
        assertBounds([edited[1]], equalTo: [original[1]])
        try assertHeader(header, in: editWindow)
        assertTwoCardFooter(in: editWindow)
        try click(editWindow.buttons["review.editCurrent.done"])
        // Keep the actual workspace/window state even if closing or resuming
        // review fails. Capture before waiting for either state to settle.
        capture(app, named: "image-occlusion-post-save-workspace-synthetic")
        captureHierarchy(app, stage: "post-save-workspace")
        XCTAssertTrue(editWindow.waitForNonExistence(timeout: 20))

        // Reload through normal UI, with no fixture reinstallation or draft
        // reuse. This empty collection contains only our note's two cards, so
        // either card's Edit Note must reopen the same stored note.
        let reopened = try openEditCurrent(app, bundle: bundle, imageSize: imageSize, viaToolbar: true)
        let reloadedCanvas = try readableCanvas(in: reopened, imageSize: imageSize, stage: "reopened")
        capture(reopened, named: "image-occlusion-edit-note-persisted-after-reopen-synthetic")
        assertNativeImage(reloadedCanvas, in: reopened, stage: "reopened")
        try assertForm(in: reopened, hasImage: true)
        try assertHeader(header, in: reopened)
        assertTwoCardFooter(in: reopened)
        assertBounds(try maskBounds(in: reopened, canvas: reloadedCanvas), equalTo: edited)
        try click(reopened.buttons["review.editCurrent.cancel"])
        XCTAssertTrue(reopened.waitForNonExistence(timeout: 15))
    }

    private func openEditCurrent(_ app: XCUIApplication, bundle: String, imageSize: CGSize,
                                 viaToolbar: Bool = false) throws -> XCUIElement {
        try app.bringToFront(bundleIdentifier: bundle)
        let main = app.windows["main"]
        XCTAssertTrue(main.waitForExistence(timeout: 10))
        let reveal = element(main, "review.session.reveal")
        let ready = wait { reveal.exists && reveal.isEnabled && reveal.isHittable }
        if !ready { try attachReviewInputEvidence(bundle: bundle, stage: "io-review-unavailable") }
        XCTAssertTrue(ready, "Edit must return to the same usable, unanswered review")
        if viaToolbar {
            // An independent production entry proves the saved masks survive
            // a new editor load even if menu-bar registration regresses.
            let actions = app.menuButtons["review.session.actions"]
            let available = wait { actions.exists && actions.isEnabled && actions.isHittable }
            if !available { try attachReviewInputEvidence(bundle: bundle, stage: "io-toolbar-unavailable") }
            XCTAssertTrue(available)
            // Foreground was established before opening; activation while
            // tracking dismisses the native menu.
            actions.click()
            let edit = actions.menuItems["review.menu.editNote"]
            let enabled = wait { edit.exists && edit.isEnabled }
            if !enabled { try attachReviewInputEvidence(bundle: bundle, stage: "io-toolbar-edit-disabled") }
            XCTAssertTrue(enabled)
            edit.click()
        } else {
            let note = app.menuBars.menuBarItems["Note"]
            let command = note.menuItems.matching(NSPredicate(format: "title == %@", "Edit Note")).firstMatch
            let enabled = wait { command.exists && command.isEnabled }
            if !enabled { try attachReviewInputEvidence(bundle: bundle, stage: "io-menu-edit-disabled") }
            XCTAssertTrue(enabled)
            note.click()
            command.click()
        }
        let window = app.windows["review.editCurrent.window"]
        XCTAssertTrue(window.waitForExistence(timeout: 15))
        // Fit uses the measured Image field viewport, with the other form rows
        // still visible. A restored window may already be at the reference size.
        let before = try settledPresentation(in: window, imageSize: imageSize)
        try sizeWindow(window)
        let after = try settledPresentation(in: window, imageSize: imageSize)
        assertResize(from: before, to: after, imageSize: imageSize)
        if !viaToolbar {
            try verifyResizing(window, imageSize: imageSize)
        }
        assertFitDisplay(in: window)
        return window
    }

    private func assertResize(from before: Presentation, to after: Presentation, imageSize: CGSize) {
        let ratio = after.image.width / before.image.width
        let fitRatio = min(after.viewport.width / imageSize.width, after.viewport.height / imageSize.height)
            / min(before.viewport.width / imageSize.width, before.viewport.height / imageSize.height)
        XCTAssertEqual(ratio, fitRatio, accuracy: 0.01, "native image follows the real viewport resize")
        // Each displayed percentage is rounded to an integer. Propagate both
        // half-point rounding bounds through the measured scale change. An
        // already-sized restored window keeps the same numeric percentage.
        XCTAssertEqual(after.zoom, before.zoom * Double(ratio), accuracy: 0.5 * Double(ratio) + 0.5 + 0.01,
                       "resize fits the locked reference size rather than resetting zoom to 100%")
    }

    private func verifyResizing(_ window: XCUIElement, imageSize: CGSize) throws {
        let before = try settledPresentation(in: window, imageSize: imageSize)
        let bounds = try visibleFrame(around: window.frame).insetBy(dx: 8, dy: 8)
        // Prefer growing the canvas. A smaller VM display can instead exercise
        // native shrinking; AppKit retains the window's own minimum size.
        let canGrow = bounds.width - before.window.width >= 60 || bounds.height - before.window.height >= 60
        let target = canGrow
            ? CGSize(width: min(bounds.width, before.window.width + 120),
                     height: min(bounds.height, before.window.height + 80))
            : CGSize(width: before.window.width - 120, height: before.window.height - 80)
        try sizeWindow(window, target: target)
        let after = try settledPresentation(in: window, imageSize: imageSize)
        XCTAssertTrue(abs(after.window.width - before.window.width) >= 20 ||
                      abs(after.window.height - before.window.height) >= 20,
                      "the native resize border must change the actual window size")
        XCTAssertTrue(abs(after.viewport.width - before.viewport.width) >= 10 ||
                      abs(after.viewport.height - before.viewport.height) >= 10,
                      "Image's viewport must respond to native window resizing")
        assertResize(from: before, to: after, imageSize: imageSize)
        try assertForm(in: window, hasImage: true)
        try sizeWindow(window)
        let restored = try settledPresentation(in: window, imageSize: imageSize)
        assertResize(from: after, to: restored, imageSize: imageSize)
    }

    private struct Presentation: Equatable {
        let image: CGRect
        let viewport: CGRect
        let window: CGRect
        let zoom: Double
    }

    private func settledPresentation(in window: XCUIElement, imageSize: CGSize) throws -> Presentation {
        let surface = DrawingSurface(viewport: element(window, "io.canvas"),
            nativeImage: element(window, "io.editor.canvas"), imageSize: imageSize)
        let zoom = element(window, "io.zoom")
        var previous: Presentation?
        var stableSamples = 0
        // Wait only for current production AX measurements. Never require an
        // early tiny presentation or a diagnostic/synthetic data record.
        let settled = wait {
            guard surface.viewport.exists, surface.nativeImage.exists, zoom.exists,
                  let value = zoom.value as? String,
                  value.contains("%"),
                  let percent = Double(value.replacingOccurrences(of: "%", with: "")
                    .trimmingCharacters(in: .whitespacesAndNewlines)), percent > 0 else {
                previous = nil; stableSamples = 0; return false
            }
            let sample = Presentation(image: surface.nativeImage.frame, viewport: surface.viewport.frame,
                                      window: window.frame, zoom: percent)
            let fitted = surface.fittedFrame
            guard sample.viewport.width >= 44, sample.viewport.height >= 44,
                  sample.window.contains(sample.viewport),
                  abs(sample.image.minX - fitted.minX) <= 1, abs(sample.image.minY - fitted.minY) <= 1,
                  abs(sample.image.width - fitted.width) <= 1, abs(sample.image.height - fitted.height) <= 1 else {
                previous = nil; stableSamples = 0; return false
            }
            stableSamples = previous == sample ? stableSamples + 1 : 0
            previous = sample
            return stableSamples >= 3
        }
        XCTAssertTrue(settled, "the live native image, viewport and zoom must settle before measurement")
        return try XCTUnwrap(previous)
    }

    /// Native title-bar and resize-border drags keep screenshots readable. Only
    /// the VM's app windows receive input; no window-server manipulation hooks.
    private func sizeWindow(_ window: XCUIElement, target: CGSize = CGSize(width: 1040, height: 720)) throws {
        let safe = try visibleFrame(around: window.frame).insetBy(dx: 8, dy: 8)
        let requested = CGSize(width: min(target.width, safe.width), height: min(target.height, safe.height))
        let frame = window.frame
        let title = window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            .withOffset(CGVector(dx: 0, dy: 12))
        try prepareInput()
        title.click(forDuration: 0.1, thenDragTo: title.withOffset(CGVector(
            dx: safe.minX - frame.minX, dy: safe.minY - frame.minY)))
        _ = try settledWindowFrame(window)
        let edge = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
            .withOffset(CGVector(dx: -2, dy: -2))
        try prepareInput()
        edge.click(forDuration: 0.1, thenDragTo: edge.withOffset(CGVector(
            dx: requested.width - window.frame.width, dy: requested.height - window.frame.height)))
        let actual = try settledWindowFrame(window)
        XCTAssertTrue(safe.insetBy(dx: -1, dy: -1).contains(actual),
                      "the entire genuine editor window must fit inside the VM display's native safe bounds")
    }

    private func settledWindowFrame(_ window: XCUIElement) throws -> CGRect {
        var previous: CGRect?
        var stableSamples = 0
        XCTAssertTrue(wait {
            let frame = window.frame
            stableSamples = previous == frame ? stableSamples + 1 : 0
            previous = frame
            return stableSamples >= 3
        }, "native window geometry must settle after a drag")
        return try XCTUnwrap(previous)
    }

    /// AppKit uses a bottom-left origin; XCTest uses the main display's top-left.
    private func visibleFrame(around frame: CGRect) throws -> CGRect {
        let height = try XCTUnwrap(NSScreen.screens.first).frame.height
        func flipped(_ rect: CGRect) -> CGRect {
            CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height)
        }
        func area(_ rect: CGRect) -> CGFloat { rect.isNull ? 0 : rect.width * rect.height }
        let screen = try XCTUnwrap(NSScreen.screens.max {
            area(flipped($0.frame).intersection(frame)) < area(flipped($1.frame).intersection(frame))
        })
        return flipped(screen.visibleFrame)
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
        let imageField = element(window, "io.editor.imageField")
        XCTAssertTrue(imageField.exists)
        XCTAssertTrue(wait { viewport.frame.height >= 279 && viewport.frame.width >= imageField.frame.width * 0.9 },
                      "the canvas fills the Image field's width and keeps its roughly 280 pt minimum height")
        XCTAssertTrue(imageField.frame.contains(viewport.frame))
        XCTAssertTrue(window.frame.contains(viewport.frame), "capture includes the full viewport")
        let result = DrawingSurface(viewport: viewport, nativeImage: element(window, "io.editor.canvas"), imageSize: imageSize)
        if !result.nativeImage.exists { captureToolbar(window, stage: "\(stage)-image-absent") }
        let zoom = element(window, "io.zoom")
        if !zoom.exists { captureToolbar(window, stage: "\(stage)-zoom-absent") }
        XCTAssertTrue(zoom.exists)
        _ = try settledPresentation(in: window, imageSize: imageSize)
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
        XCTAssertGreaterThan(actual.width, 0)
        XCTAssertGreaterThan(actual.height, 0)
        XCTAssertTrue(window.frame.contains(actual))
        XCTAssertEqual(actual.minX, fitted.minX, accuracy: 1)
        XCTAssertEqual(actual.minY, fitted.minY, accuracy: 1)
        XCTAssertEqual(actual.width, fitted.width, accuracy: 1)
        XCTAssertEqual(actual.height, fitted.height, accuracy: 1)
        XCTAssertEqual(actual.width / actual.height, surface.imageSize.width / surface.imageSize.height, accuracy: 0.02)
    }

    private func formField(_ name: String, in window: XCUIElement) throws -> XCUIElement {
        // SwiftUI/AppKit single-line fields and WebKit's multiline textbox are
        // both native AX inputs. Scope by role and name so the label cannot win.
        let inputs = window.descendants(matching: .any).matching(NSPredicate(
            format: "(label == %@ OR label == %@) AND (elementType == %lu OR elementType == %lu)",
            name, name + ":", XCUIElement.ElementType.textField.rawValue, XCUIElement.ElementType.textView.rawValue))
        XCTAssertTrue(wait { inputs.count == 1 }, "expected one native editable \(name) field")
        let input = inputs.firstMatch
        XCTAssertTrue(input.isEnabled)
        XCTAssertGreaterThan(input.frame.width, 0)
        XCTAssertGreaterThan(input.frame.height, 0)
        XCTAssertTrue(window.frame.contains(input.frame), "the whole \(name) field stays visible with Image")
        return input
    }

    private func assertForm(in window: XCUIElement, hasImage: Bool) throws {
        let header = try formField("Header", in: window)
        let back = try formField("Back Extra", in: window)
        let comments = try formField("Comments", in: window)
        let tags = element(window, "io.editor.tags.input")
        XCTAssertTrue(tags.exists && tags.isEnabled)
        XCTAssertGreaterThan(tags.frame.width, 0)
        XCTAssertGreaterThan(tags.frame.height, 0)
        XCTAssertTrue(window.frame.contains(tags.frame))
        let field = element(window, "io.editor.imageField")
        XCTAssertTrue(field.exists)
        XCTAssertGreaterThan(field.frame.width, 0)
        XCTAssertGreaterThan(field.frame.height, 0)
        XCTAssertTrue(window.frame.contains(field.frame))
        XCTAssertLessThanOrEqual(header.frame.maxY, field.frame.minY + 1)
        XCTAssertLessThanOrEqual(field.frame.maxY, back.frame.minY + 1)
        XCTAssertLessThanOrEqual(back.frame.maxY, comments.frame.minY + 1)
        XCTAssertLessThanOrEqual(comments.frame.maxY, tags.frame.minY + 1)
        let footer = element(window, "io.editor.imageFooter")
        if hasImage {
            XCTAssertTrue(footer.exists)
            XCTAssertGreaterThan(footer.frame.height, 0)
            XCTAssertTrue(field.frame.contains(footer.frame))
            let canvas = element(window, "io.canvas")
            XCTAssertTrue(canvas.exists)
            XCTAssertLessThanOrEqual(canvas.frame.maxY, footer.frame.minY + 1)
            let change = element(footer, "io.editor.changeImage")
            XCTAssertTrue(change.exists && change.isEnabled && change.isHittable,
                          "Change Image belongs to the visible image footer")
        } else {
            XCTAssertFalse(footer.exists, "no-image form has no image footer")
            XCTAssertFalse(element(window, "io.zoom").exists, "no-image form has no image toolbar")
            XCTAssertFalse(element(window, "io.canvas").exists)
            let picker = element(window, "io.picker.container")
            XCTAssertTrue(picker.exists)
            XCTAssertTrue(field.frame.contains(picker.frame))
        }
    }

    private func assertHeader(_ expected: String, in window: XCUIElement) throws {
        let input = try formField("Header", in: window)
        XCTAssertTrue(wait {
            guard let value = input.value as? String else { return false }
            return value == expected || value == expected + "\n"
        }, "the Header's actual AX input value must survive save and reopening")
    }

    private func assertTwoCardFooter(in window: XCUIElement) {
        let footer = element(window, "io.editor.imageFooter")
        let summary = footer.descendants(matching: .staticText).matching(
            NSPredicate(format: "label CONTAINS %@", "2 masks make 2 cards")).firstMatch
        // A native footer may combine its text children into one AX element.
        XCTAssertTrue(wait { footer.label.contains("2 masks make 2 cards") || summary.exists },
                      "the footer reports distinct card ordinals for both masks")
        XCTAssertTrue(window.frame.contains(footer.frame))
        if summary.exists {
            XCTAssertTrue(footer.frame.contains(summary.frame))
            XCTAssertTrue(window.frame.contains(summary.frame))
        }
    }

    private func assertFitDisplay(in window: XCUIElement) {
        let zoom = element(window, "io.zoom")
        let fit = window.descendants(matching: .any).matching(identifier: "io.zoom")
            .matching(NSPredicate(format: "title == %@ OR label == %@", "Fit", "Fit")).firstMatch
        XCTAssertTrue(wait { fit.exists || zoom.staticTexts["Fit"].exists },
                      "the fitted image's native zoom menu displays Fit")
        let value = zoom.value as? String
        XCTAssertTrue(value?.contains("%") == true, "io.zoom keeps its numeric percent AX value")
        XCTAssertNotNil(value.flatMap {
            Double($0.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespacesAndNewlines))
        })
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
            XCTAssertEqual(mask.label, "Rectangle \(number)", "saved mask numbers must keep their identities")
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
