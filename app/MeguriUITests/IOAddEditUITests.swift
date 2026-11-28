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
    private struct Observation {
        let passed: Bool
        let detail: String
        let file: StaticString
        let line: UInt
    }
    private var observations: [Observation] = []
    private var observationStage = ""

    // Evaluate against the live window now, never against a later draft or a
    // stale XCUIElement. Report every independent contract failure after the
    // production save/reopen journey and its durable screenshots have run.
    private func check(_ passed: Bool, _ detail: String,
                       file: StaticString = #filePath, line: UInt = #line) {
        observations.append(Observation(passed: passed, detail: "[\(observationStage)] \(detail)",
                                        file: file, line: line))
    }

    private func verifyObservations() {
        let facts = XCTAttachment(string: observations.map { "\($0.passed ? "PASS" : "FAIL") \($0.detail)" }
            .joined(separator: "\n"))
        facts.name = "image-occlusion-native-layout-and-ax-facts"
        facts.lifetime = .keepAlways
        add(facts)
        continueAfterFailure = true
        for observation in observations {
            XCTAssertTrue(observation.passed, observation.detail, file: observation.file, line: observation.line)
        }
    }

    func testNativeImageOcclusionAddAndEditPersistsDraggedMasks() throws {
        // Must precede launch, synthetic input, and all clipboard access.
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            throw XCTSkip("Image occlusion journey runs only in the Mac test VM")
        }
        continueAfterFailure = false
        observations = []
        defer { verifyObservations() }
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
        capture(addWindow, named: "image-occlusion-add-note-no-image-native-form-synthetic")
        recordForm(in: addWindow, hasImage: false, stage: "empty-picker")

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
        recordForm(in: addWindow, hasImage: true, stage: "add-image")
        recordFitDisplay(in: addWindow)

        // O4.2 opens Rectangle. These are real mouse drags over the two
        // diagram labels, expressed as fractions of the fitted source image.
        try drag(addCanvas, from: CGVector(dx: 0.05, dy: 0.50), to: CGVector(dx: 0.31, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-first-mask-immediately-after-input-synthetic")
        try requireMask(1, in: addWindow, stage: "add-first")
        let handles = addWindow.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@", "io.handle."))
        observationStage = "add-first-deselect"
        check(wait { handles.count > 0 }, "drawing selects the first mask and exposes native handles")
        try prepareInput()
        addCanvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.94)).click()
        check(wait { handles.count == 0 }, "bare blank-image click clears selection with Rectangle still armed")
        try drag(addCanvas, from: CGVector(dx: 0.69, dy: 0.50), to: CGVector(dx: 0.95, dy: 0.62))
        capture(addWindow, named: "image-occlusion-add-two-masks-immediately-after-input-synthetic")
        try requireMask(2, in: addWindow, stage: "add-second")
        observationStage = "add-second-deselect"
        check(wait { handles.count > 0 }, "the still-armed Rectangle draws and selects the second mask")
        try prepareInput()
        addCanvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.94)).click()
        check(wait { handles.count == 0 }, "bare blank-image click clears the second mask's native handles")
        recordNativeImage(addCanvas, in: addWindow, stage: "add-completed")
        let original = try maskBounds(in: addWindow, canvas: addCanvas)
        assertBounds(original, equalTo: [
            CGRect(x: 0.05, y: 0.50, width: 0.26, height: 0.12),
            CGRect(x: 0.69, y: 0.50, width: 0.26, height: 0.12)
        ])
        // Keep Rectangle armed: selecting mask 1 must not make a drag on mask
        // 2 move mask 1. Restore mask 2 so the saved/reopened geometry stays exact.
        observationStage = "add-select-first-drag-second"
        let addFirst = element(addWindow, "io.mask.1"), addSecond = element(addWindow, "io.mask.2")
        try click(addFirst)
        check(wait { addFirst.isSelected && !addSecond.isSelected }, "native selection identifies mask 1")
        try drag(addCanvas, from: CGVector(dx: 0.82, dy: 0.56), to: CGVector(dx: 0.86, dy: 0.70))
        let moved = try maskBounds(in: addWindow, canvas: addCanvas)
        check(abs(moved[0].minX - original[0].minX) <= 0.015 &&
              abs(moved[0].minY - original[0].minY) <= 0.015, "dragging mask 2 leaves mask 1 in place")
        check(abs(moved[1].minX - original[1].minX - 0.04) <= 0.015 &&
              abs(moved[1].minY - original[1].minY - 0.14) <= 0.015, "the first drag moves mask 2")
        try drag(addCanvas, from: CGVector(dx: 0.86, dy: 0.70), to: CGVector(dx: 0.82, dy: 0.56))
        assertBounds(try maskBounds(in: addWindow, canvas: addCanvas), equalTo: original)
        check(wait { addSecond.isSelected && !addFirst.isSelected }, "native selection identifies mask 2 before deselecting")
        try prepareInput()
        addCanvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.94)).click()
        check(wait { handles.count == 0 }, "bare blank-image click clears selection after moving mask 2")
        // Option B keeps ordinary fields and both native masks visible while
        // typing. Focus the actual editable role, never the Header label.
        let header = "Isosceles triangle"
        let tag = "geometry-native"
        let headerInput = try formField("Header", in: addWindow)
        try click(headerInput)
        try prepareInput()
        app.typeText(header)
        try prepareInput()
        app.typeKey(.tab, modifierFlags: [])
        let tagsInput = try tagsField(in: addWindow)
        try click(tagsInput)
        try prepareInput()
        app.typeText(tag)
        try prepareInput()
        app.typeKey(.tab, modifierFlags: [])
        capture(addWindow, named: "image-occlusion-add-note-two-drawn-masks-synthetic")
        recordTags(tag, in: addWindow)
        recordHeader(header, in: addWindow)
        recordForm(in: addWindow, hasImage: true)
        recordTwoCardFooter(in: addWindow)
        assertBounds(try maskBounds(in: addWindow, canvas: addCanvas), equalTo: original)
        try click(addWindow.buttons["editor.add.save"])
        capture(addWindow, named: "image-occlusion-add-immediately-after-save-input-synthetic")
        XCTAssertTrue(element(addWindow, "editor.add.result").waitForExistence(timeout: 20))
        XCTAssertFalse(element(addWindow, "editor.add.error").exists)
        XCTAssertTrue(paste.waitForExistence(timeout: 15), "the next draft has no image or masks")
        recordForm(in: addWindow, hasImage: false, stage: "next-empty-draft")
        try click(addWindow.buttons["editor.add.close"])
        XCTAssertTrue(addWindow.waitForNonExistence(timeout: 15))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 2, learning: 0, review: 0),
                      "two independent numbered masks generate exactly two cards in the empty deck")

        try click(page.buttons["home.deck.study"])
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        let editWindow = try openEditCurrent(app, bundle: bundle, imageSize: imageSize)
        let editCanvas = try readableCanvas(in: editWindow, imageSize: imageSize, stage: "edit")
        recordNativeImage(editCanvas, in: editWindow, stage: "edit")
        recordForm(in: editWindow, hasImage: true)
        recordHeader(header, in: editWindow)
        recordTags(tag, in: editWindow)
        recordTwoCardFooter(in: editWindow)
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
        observationStage = "edit-moved"
        let edited = try maskBounds(in: editWindow, canvas: editCanvas)
        XCTAssertEqual(edited[0].minX, original[0].minX + 0.04, accuracy: 0.015)
        XCTAssertEqual(edited[0].minY, original[0].minY + 0.14, accuracy: 0.015)
        assertBounds([edited[1]], equalTo: [original[1]])
        recordHeader(header, in: editWindow)
        recordTwoCardFooter(in: editWindow)
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
        recordNativeImage(reloadedCanvas, in: reopened, stage: "reopened")
        recordForm(in: reopened, hasImage: true)
        recordHeader(header, in: reopened)
        recordTags(tag, in: reopened)
        recordTwoCardFooter(in: reopened)
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
        observationStage = viaToolbar ? "reopened" : "edit"
        capture(window, named: "image-occlusion-\(observationStage)-note-immediately-after-opening-synthetic")
        // Fit uses the measured Image field viewport, with the other form rows
        // still visible. A restored window may already be at the reference size.
        let before = try settledPresentation(in: window, imageSize: imageSize)
        try sizeWindow(window)
        capture(window, named: "image-occlusion-\(viaToolbar ? "reopened" : "edit")-note-before-secondary-ax-synthetic")
        let after = try settledPresentation(in: window, imageSize: imageSize)
        recordResize(from: before, to: after, imageSize: imageSize)
        if !viaToolbar {
            try verifyResizing(window, imageSize: imageSize)
        }
        recordFitDisplay(in: window)
        return window
    }

    private func recordResize(from before: Presentation, to after: Presentation, imageSize: CGSize) {
        let ratio = after.image.width / before.image.width
        let fitRatio = min(after.viewport.width / imageSize.width, after.viewport.height / imageSize.height)
            / min(before.viewport.width / imageSize.width, before.viewport.height / imageSize.height)
        check(abs(ratio - fitRatio) <= 0.01,
              "native image follows real viewport resize: image ratio \(ratio), fit ratio \(fitRatio)")
        // Each displayed percentage is rounded to an integer. Propagate both
        // half-point rounding bounds through the measured scale change. An
        // already-sized restored window keeps the same numeric percentage.
        check(before.zoom != nil && after.zoom != nil, "resize exposes numeric Zoom values before and after")
        if let oldZoom = before.zoom, let newZoom = after.zoom {
            check(abs(newZoom - oldZoom * Double(ratio)) <= 0.5 * Double(ratio) + 0.5 + 0.01,
                  "resize fits locked reference size: zoom \(oldZoom) → \(newZoom), scale ratio \(ratio)")
        }
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
        check(abs(after.window.width - before.window.width) >= 20 ||
              abs(after.window.height - before.window.height) >= 20,
              "native resize changes actual window: \(before.window) → \(after.window)")
        check(abs(after.viewport.width - before.viewport.width) >= 10 ||
              abs(after.viewport.height - before.viewport.height) >= 10,
              "Image viewport responds to resize: \(before.viewport) → \(after.viewport)")
        recordResize(from: before, to: after, imageSize: imageSize)
        recordForm(in: window, hasImage: true)
        try sizeWindow(window)
        let restored = try settledPresentation(in: window, imageSize: imageSize)
        recordResize(from: after, to: restored, imageSize: imageSize)
    }

    private struct Presentation: Equatable {
        let image: CGRect
        let viewport: CGRect
        let window: CGRect
        let zoom: Double?
    }

    private func settledPresentation(in window: XCUIElement, imageSize: CGSize) throws -> Presentation {
        let surface = DrawingSurface(viewport: element(window, "io.canvas"),
            nativeImage: element(window, "io.editor.canvas"), imageSize: imageSize)
        let zoom = zoomControls(in: window).firstMatch
        var previous: Presentation?
        var stableSamples = 0
        // A usable, stable viewport is needed for real mouse input. Independent
        // image-role, fitted-layout and Zoom semantics are recorded below so a
        // failure there cannot suppress the native save/reopen evidence.
        let settled = wait {
            guard surface.viewport.exists,
                  surface.viewport.frame.width >= 44, surface.viewport.frame.height >= 44 else {
                previous = nil; stableSamples = 0; return false
            }
            let percent = zoom.exists ? (zoom.value as? String).flatMap(Self.zoomPercent) : nil
            let sample = Presentation(image: surface.frame, viewport: surface.viewport.frame,
                                      window: window.frame, zoom: percent)
            stableSamples = previous == sample ? stableSamples + 1 : 0
            previous = sample
            return stableSamples >= 3
        }
        XCTAssertTrue(settled, "the live drawing viewport must settle before mouse input")
        let sample = try XCTUnwrap(previous)
        check(sample.window.contains(sample.viewport), "capture includes full viewport: \(sample.viewport)")
        check(zoom.exists && (zoom.value as? String)?.contains("%") == true,
              "live Zoom retains its percent AX value")
        check(sample.zoom.map { $0 > 0 } == true, "live Zoom has a positive numeric percentage: \(String(describing: sample.zoom))")
        recordNativeImage(surface, in: window, stage: observationStage)
        return sample
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
        check(safe.insetBy(dx: -1, dy: -1).contains(actual),
              "entire editor window \(actual) fits native VM safe bounds \(safe)")
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
        observationStage = stage
        let imageField = element(window, "io.editor.imageField")
        check(imageField.exists, "Image field has its own native AX group")
        check(wait { imageField.exists && viewport.frame.height >= 279 && viewport.frame.width >= imageField.frame.width * 0.9 },
              "canvas fills Image field width and keeps roughly 280 pt minimum height")
        check(imageField.exists && imageField.frame.contains(viewport.frame), "Image contains full canvas viewport")
        check(window.frame.contains(viewport.frame), "capture includes full viewport")
        let result = DrawingSurface(viewport: viewport, nativeImage: element(window, "io.editor.canvas"), imageSize: imageSize)
        if !result.nativeImage.exists { captureToolbar(window, stage: "\(stage)-image-absent") }
        let zoom = zoomControls(in: window).firstMatch
        if !zoom.exists { captureToolbar(window, stage: "\(stage)-zoom-absent") }
        check(zoom.exists, "Zoom retains io.zoom identifier")
        _ = try settledPresentation(in: window, imageSize: imageSize)
        return result
    }

    private func recordNativeImage(_ surface: DrawingSurface, in window: XCUIElement, stage: String) {
        observationStage = stage
        if !surface.nativeImage.exists {
            captureToolbar(window, stage: stage)
            captureHierarchy(window, stage: stage)
        }
        check(surface.nativeImage.exists, "the fitted image must be a native AX image child")
        guard surface.nativeImage.exists else { return }
        check(surface.nativeImage.elementType == .image, "native fitted image role: \(surface.nativeImage.elementType)")
        let actual = surface.nativeImage.frame, fitted = surface.fittedFrame
        check(actual.width > 0 && actual.height > 0, "native fitted image bounds: \(actual)")
        check(window.frame.contains(actual), "window contains native fitted image \(actual)")
        check(abs(actual.minX - fitted.minX) <= 1, "image minX \(actual.minX), fitted \(fitted.minX)")
        check(abs(actual.minY - fitted.minY) <= 1, "image minY \(actual.minY), fitted \(fitted.minY)")
        check(abs(actual.width - fitted.width) <= 1, "image width \(actual.width), fitted \(fitted.width)")
        check(abs(actual.height - fitted.height) <= 1, "image height \(actual.height), fitted \(fitted.height)")
        check(abs(actual.width / actual.height - surface.imageSize.width / surface.imageSize.height) <= 0.02,
              "native fitted image preserves source aspect ratio")
    }

    private func fieldInputs(_ name: String, in window: XCUIElement) -> XCUIElementQuery {
        // Scope by native editable role and name so the static label cannot win.
        window.descendants(matching: .any).matching(NSPredicate(
            format: "(label == %@ OR label == %@) AND (elementType == %lu OR elementType == %lu)",
            name, name + ":", XCUIElement.ElementType.textField.rawValue, XCUIElement.ElementType.textView.rawValue))
    }

    private func tagsInputs(in window: XCUIElement) -> XCUIElementQuery {
        window.textFields.matching(NSPredicate(format: "label == %@", "Tags"))
    }

    private func formField(_ name: String, in window: XCUIElement) throws -> XCUIElement {
        let inputs = fieldInputs(name, in: window)
        XCTAssertTrue(wait { inputs.count == 1 }, "expected one native editable \(name) field")
        return inputs.firstMatch
    }

    private func tagsField(in window: XCUIElement) throws -> XCUIElement {
        let inputs = tagsInputs(in: window)
        XCTAssertTrue(wait { inputs.count == 1 }, "expected one native Tags text field, independently of its row ID")
        return inputs.firstMatch
    }

    private func recordField(_ name: String, in window: XCUIElement) -> CGRect {
        let inputs = name == "Tags" ? tagsInputs(in: window) : fieldInputs(name, in: window)
        check(inputs.count == 1, "expected one native editable \(name) field; found \(inputs.count)")
        guard inputs.count == 1 else { return .zero }
        let input = inputs.firstMatch, frame = input.frame
        check(input.isEnabled, "\(name) input is enabled")
        check(frame.width > 0 && frame.height > 0, "\(name) input bounds: \(frame)")
        check(window.frame.contains(frame), "the whole \(name) field stays visible with Image: \(frame)")
        if name == "Tags" {
            check(input.identifier == "io.editor.tags.input", "Tags keeps its native input ID: \(input.identifier)")
        }
        return frame
    }

    private func recordForm(in window: XCUIElement, hasImage: Bool, stage: String = "") {
        if !stage.isEmpty { observationStage = stage }
        let header = recordField("Header", in: window)
        let back = recordField("Back Extra", in: window)
        let comments = recordField("Comments", in: window)
        let tags = recordField("Tags", in: window)
        let field = element(window, "io.editor.imageField")
        check(field.exists, "Image has its own native AX group")
        let fieldFrame = field.exists ? field.frame : .zero
        check(fieldFrame.width > 0 && fieldFrame.height > 0, "Image field bounds: \(fieldFrame)")
        check(window.frame.contains(fieldFrame), "the whole Image field stays visible: \(fieldFrame)")
        check(header.maxY <= fieldFrame.minY + 1, "Header precedes Image")
        check(fieldFrame.maxY <= back.minY + 1, "Image precedes Back Extra")
        check(back.maxY <= comments.minY + 1, "Back Extra precedes Comments")
        check(comments.maxY <= tags.minY + 1, "Comments precedes Tags")
        let footer = element(window, "io.editor.imageFooter")
        if hasImage {
            check(footer.exists, "image footer has its own native AX group")
            let footerFrame = footer.exists ? footer.frame : .zero
            check(footerFrame.height > 0, "footer bounds: \(footerFrame)")
            check(fieldFrame.contains(footerFrame), "Image contains its footer")
            let canvas = element(window, "io.canvas")
            check(canvas.exists, "Image contains the native canvas")
            check(canvas.exists && canvas.frame.maxY <= footerFrame.minY + 1, "canvas precedes footer")
            let changes = footer.buttons.matching(identifier: "io.editor.changeImage")
            check(changes.count == 1, "footer retains one independent native Change Image button")
            if changes.count == 1 {
                let change = changes.firstMatch
                check(change.isEnabled && change.isHittable, "Change Image is enabled and hittable")
                check(change.label == "Change Image…", "Change Image retains its native label: \(change.label)")
                check(footerFrame.contains(change.frame), "Change Image belongs to the visible image footer")
            }
        } else {
            check(!footer.exists, "no-image form has no image footer")
            check(zoomControls(in: window).count == 0, "no-image form has no image toolbar")
            check(!element(window, "io.canvas").exists, "no-image form has no canvas")
            // AppKit may omit the picker's outer stack from AX. Its native
            // source buttons still belong to the meaningful Image field group.
            for caption in ["Choose File…", "Paste Image"] {
                let buttons = field.buttons.matching(NSPredicate(
                    format: "title == %@ OR label == %@", caption, caption))
                check(buttons.count == 1, "empty Image contains one native \(caption) picker button")
                guard buttons.count == 1 else { continue }
                let button = buttons.firstMatch, frame = button.frame
                check(button.isEnabled && button.isHittable, "\(caption) is enabled and hittable")
                check(frame.width > 0 && frame.height > 0, "\(caption) picker bounds: \(frame)")
                check(fieldFrame.contains(frame), "Image contains the whole \(caption) picker button")
                check(window.frame.contains(frame), "the whole \(caption) picker button stays visible")
            }
        }
    }

    private func recordHeader(_ expected: String, in window: XCUIElement) {
        let inputs = fieldInputs("Header", in: window)
        check(inputs.count == 1, "Header has one actual editable input")
        guard inputs.count == 1 else { return }
        let input = inputs.firstMatch
        let persisted = wait {
            guard let value = input.value as? String else { return false }
            return value == expected || value == expected + "\n"
        }
        check(persisted, "Header's actual input value survives save/reopening: \(String(describing: input.value))")
    }

    private func recordTags(_ expected: String, in window: XCUIElement) {
        let inputs = tagsInputs(in: window)
        check(inputs.count == 1, "Tags has one native TextField labelled Tags")
        guard inputs.count == 1 else { return }
        let input = inputs.firstMatch
        // NSTokenField can expose committed tokens as children rather than as
        // its editable string. Read only actual AX values, never draft hooks.
        let persisted = wait {
            if let value = input.value as? String,
               value.split(whereSeparator: \.isWhitespace).contains(Substring(expected)) { return true }
            return input.descendants(matching: .any).matching(NSPredicate(
                format: "label == %@ OR value == %@", expected, expected)).count > 0
        }
        check(persisted, "native Tags input commits and reloads \(expected); value: \(String(describing: input.value))")
    }

    private func recordTwoCardFooter(in window: XCUIElement) {
        let footer = element(window, "io.editor.imageFooter")
        check(footer.exists, "two-mask footer exists")
        guard footer.exists else { return }
        let summary = footer.descendants(matching: .staticText).matching(NSPredicate(
            format: "label CONTAINS %@ OR value CONTAINS %@", "2 masks make 2 cards", "2 masks make 2 cards"))
        // Native Label may combine filename and counts into one static text.
        check(wait { summary.count > 0 }, "footer exposes its count label separately from Change Image")
        check(footer.label.contains("2 masks make 2 cards") || summary.count > 0,
              "footer reports distinct card ordinals for both masks")
        check(window.frame.contains(footer.frame), "whole two-card footer remains visible")
        for text in summary.allElementsBoundByIndex {
            check(footer.frame.contains(text.frame), "footer contains count label \(text.frame)")
            check(window.frame.contains(text.frame), "window contains count label \(text.frame)")
        }
    }

    private func zoomControls(in window: XCUIElement) -> XCUIElementQuery {
        window.menuButtons.matching(identifier: "io.zoom")
    }

    private func recordFitDisplay(in window: XCUIElement) {
        let controls = zoomControls(in: window)
        check(controls.count == 1, "expected one native io.zoom menu button")
        guard controls.count == 1 else { return }
        let zoom = controls.firstMatch
        check(zoom.identifier == "io.zoom", "Zoom menu keeps its own identifier: \(zoom.identifier)")
        // AppKit can expose the accessibility label as the native menu title.
        check(wait { zoom.title == "Zoom, Fit" || zoom.label == "Zoom, Fit" },
              "native Zoom title or label exposes the current Fit state")
        let value = zoom.value as? String
        check(value?.contains("%") == true, "io.zoom keeps its numeric percent AX value: \(String(describing: value))")
        check(value.flatMap(Self.zoomPercent) != nil, "Zoom AX percentage parses as a number")
    }

    private static func zoomPercent(_ value: String) -> Double? {
        Double(value.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespacesAndNewlines))
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
            check(mask.elementType == .button, "mask \(number) has native button role: \(mask.elementType)")
            check(mask.label == "Rectangle \(number)", "saved mask numbers keep their identities: \(mask.label)")
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
