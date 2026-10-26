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
        #if os(macOS)
        let folder = FileManager.default.temporaryDirectory.appending(path: "RF3-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        #else
        let folder: URL? = nil
        #endif
        let saved = try exerciseNativeExport(app, bundle, destinationFolder: folder)
        let settingsContent = element(app, "settings.root.content")
        let prepare = app.buttons.matching(identifier: "settings.importExport.prepare").firstMatch
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
        #if os(iOS)
        try revealSettingsControl("settings.root.importCollection", app)
        #endif
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
        let study = app.buttons["home.deck.study"].firstMatch
        try waitForStudyReady(study, app)
        try click(study, app, bundle)
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
    /// Destination proof only: generated media and its existing review history
    /// are exported without reviews, database edits or media-file reimport.
    func testNativeMediaExportCancelSaveReplace() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development fixture hooks") }
        #if os(iOS) && !targetEnvironment(simulator)
        throw XCTSkip("Native media export capture requires the leased test VM")
        #else
        let filename = try XCTUnwrap(ProcessInfo.processInfo.environment["MEGURI_NATIVE_MEDIA_EXPORT_NAME"],
            "Run this capture through tools/mg ui-test")
        guard filename.range(of: #"^Meguri-Media-[a-f0-9]{32}\.colpkg$"#, options: .regularExpression)?.lowerBound == filename.startIndex,
              filename.utf8.count == 52 else {
            XCTFail("Native media capture requires its own safe per-run filename")
            throw NSError(domain: "CollectionEntryJourneyTests", code: 4)
        }
        let app = XCUIApplication()
        #if os(iOS)
        // Installs the local Files folder only; this source is never imported.
        let importSource: URL? = build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/review.colpkg")
        #else
        let importSource: URL? = nil
        #endif
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/typical-1"),
            importSource: importSource, now: "2030-01-15T10:00:00+01:00",
            timeZone: "Europe/Paris", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        #if os(macOS)
        let folder = FileManager.default.temporaryDirectory.appending(
            path: "RF3-Media-\(filename.dropFirst(13).prefix(32))", directoryHint: .isDirectory)
        // Retain the actual native destination in XCTest before normal cleanup.
        XCTAssertNil(try? FileManager.default.attributesOfItem(atPath: folder.path),
                     "Native media capture requires a fresh owned folder")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: folder) }
        #else
        let folder: URL? = nil
        #endif
        let saved = try exerciseNativeExport(app, bundle, destinationFolder: folder, destinationFilename: filename)
        #if os(macOS)
        XCTAssertEqual(saved, folder.appending(path: filename))
        XCTAssertTrue(FileManager.default.fileExists(atPath: saved.path))
        let attachment = XCTAttachment(contentsOfFile: saved)
        attachment.name = filename
        attachment.lifetime = .keepAlways
        add(attachment)
        #else
        _ = saved
        #endif
        capture(app, "media-export-native-destination-received")
        #endif
    }

    private func openCollectionSettings(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try app.bringToFront(bundleIdentifier: bundle)
        let settings = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        if !settings.exists {
            let main = app.windows.matching(identifier: "main").firstMatch
            XCTAssertTrue(main.waitForExistence(timeout: 10))
            let frame = main.frame
            let keyboardTarget = try XCTUnwrap(!frame.isNull && !frame.isEmpty
                && frame.origin.x.isFinite && frame.origin.y.isFinite
                && frame.width.isFinite && frame.height.isFinite ? main : nil,
                "Native keyboard target must have a finite, nonempty window frame")
            keyboardTarget.typeKey(",", modifierFlags: .command)
        }
        XCTAssertTrue(element(app, "settings.root.content").waitForExistence(timeout: 30),
            "Command-comma must open the native Settings scene")
        try click(element(app, "settings.root.collection"), app, bundle)
        #else
        let settings = try XCTUnwrap(waitForShellSection(app, tabIdentifier: "shell.root.tab.settings",
            sidebarIdentifier: "shell.root.sidebar.settings", compactLabel: "Settings", timeout: 10), app.debugDescription)
        try click(settings, app, bundle)
        #endif
    }

    #if os(iOS)
    private func revealSettingsControl(_ identifier: String, _ app: XCUIApplication) throws {
        let root = element(app, "settings.root.content")
        let target = root.buttons.matching(identifier: identifier).firstMatch
        let window = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        let scroll: XCUIElement? = {
            guard root.exists else { return nil }
            if [.scrollView, .table, .collectionView].contains(root.elementType) { return root }
            for query in [root.collectionViews, root.tables, root.scrollViews] {
                if query.firstMatch.exists { return query.firstMatch }
            }
            for query in [app.collectionViews, app.tables, app.scrollViews] {
                let owner = query.containing(.any, identifier: "settings.root.content").firstMatch
                if owner.exists { return owner }
            }
            return nil
        }()
        func finite(_ frame: CGRect) -> Bool {
            !frame.isNull && !frame.isInfinite && !frame.isEmpty
                && [frame.minX, frame.minY, frame.width, frame.height].allSatisfy { $0.isFinite }
        }
        // Search the owning Form even when its lazy rows have no AX element yet.
        // As in ManualTransferJourneyTests.reveal, search above, then below.
        for attempt in 0..<24 {
            guard app.state == .runningForeground, !app.sheets.firstMatch.exists,
                  !app.alerts.firstMatch.exists, !app.keyboards.firstMatch.exists,
                  root.exists, window.exists, let scroll, scroll.exists,
                  scroll.isEnabled, scroll.isHittable else { break }
            let scrollFrame = scroll.frame
            let viewport = scrollFrame.intersection(root.frame).intersection(window.frame).intersection(app.frame)
            guard finite(scrollFrame), finite(viewport) else { break }
            if target.exists && target.isEnabled && target.isHittable { return }
            let towardsBottom = target.exists ? target.frame.maxY > viewport.maxY : attempt >= 8
            let origin = scroll.coordinate(withNormalizedOffset: .zero)
            let upper = origin.withOffset(CGVector(dx: viewport.midX - scrollFrame.minX,
                                                  dy: viewport.minY + viewport.height * 0.2 - scrollFrame.minY))
            let lower = origin.withOffset(CGVector(dx: viewport.midX - scrollFrame.minX,
                                                  dy: viewport.minY + viewport.height * 0.8 - scrollFrame.minY))
            let start = towardsBottom ? lower : upper
            let end = towardsBottom ? upper : lower
            start.press(forDuration: 0, thenDragTo: end)
        }
        guard target.exists && target.isEnabled && target.isHittable else {
            XCTFail("Settings Form must reveal an existing, enabled, hittable \(identifier) within 24 native gestures")
            throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
        }
    }
    #endif

    private func exerciseNativeExport(_ app: XCUIApplication, _ bundle: String,
                                      destinationFolder: URL?, destinationFilename: String? = nil) throws -> URL {
        try openCollectionSettings(app, bundle)
        let settingsContent = element(app, "settings.root.content")
        XCTAssertTrue(settingsContent.waitForExistence(timeout: 10), "Native navigation must open Settings: \(app.debugDescription)")
        let export = app.buttons["settings.root.exportCollection"].firstMatch
        #if os(iOS)
        try revealSettingsControl("settings.root.exportCollection", app)
        #endif
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
        let folder = try XCTUnwrap(destinationFolder)
        let saved = folder.appending(path: destinationFilename ?? "Meguri-Collection.colpkg")
        #else
        try cancelNativeExportPicker(app, bundle)
        let delegateReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            let choose = self.element(app, "settings.importExport.choose")
            return self.element(app, "settings.importExport.ready").exists
                && choose.exists && choose.isEnabled && choose.isHittable
        }, object: nil)
        let delegateResult = XCTWaiter().wait(for: [delegateReady], timeout: 10)
        if delegateResult != .completed { capturePicker(app) }
        XCTAssertEqual(delegateResult, .completed, "Native cancellation must return the export to Ready: \(app.debugDescription)")
        // The native local folder is installed by the existing public source hook.
        let root = URL(fileURLWithPath: "/RF2 Import Tests")
        let folder = destinationFilename.map { root.appending(path: String($0.dropLast(".colpkg".count))) } ?? root
        let saved = folder.appending(path: "Meguri-Collection.colpkg")
        #endif
        XCTAssertTrue(element(app, "settings.importExport.ready").waitForExistence(timeout: 10))
        for replacement in [false, true] {
            let choose = app.buttons.matching(identifier: "settings.importExport.choose").firstMatch
            let chooseReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                choose.exists && choose.isEnabled && choose.isHittable
            }, object: nil)
            guard XCTWaiter().wait(for: [chooseReady], timeout: 30) == .completed else {
                capture(app, "export-native-choose-not-ready")
                XCTFail("Native export Choose must be enabled and hittable: \(app.debugDescription)")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 2)
            }
            try click(choose, app, bundle)
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
            if let destinationFilename {
                try selectNativeExportFilename(savePanel.textFields, destinationFilename, app, bundle)
            }
            try click(save, app, bundle)
            if replacement {
                let replace = app.buttons.matching(NSPredicate(format: "title == %@ OR label == %@", "Replace", "Replace")).firstMatch
                XCTAssertTrue(replace.waitForExistence(timeout: 10), app.debugDescription)
                try click(replace, app, bundle)
            }
            #else
            if let destinationFilename {
                try selectNativeExportChild(String(destinationFilename.dropLast(".colpkg".count)),
                                            create: !replacement, app, bundle)
            } else {
                try selectLocalExportFolder(app, bundle)
            }
            let save = app.buttons.matching(identifier: "DOCPicker.actionButton").firstMatch
            let folderName = destinationFilename.map { String($0.dropLast(".colpkg".count)) } ?? "RF2 Import Tests"
            let saveReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                self.nativeExportFolderIsOpen(folderName, app)
                    && save.exists && save.label == "Save" && save.isEnabled && save.isHittable
            }, object: nil)
            guard XCTWaiter().wait(for: [saveReady], timeout: 10) == .completed else {
                capturePicker(app)
                XCTFail("The exact destination folder must offer an enabled, hittable native Save action")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
            capture(app, "export-native-save-ready")
            try click(save, app, bundle)
            if replacement {
                guard confirmNativeExportReplacement(app, expectedFilename: destinationFilename == nil ? nil : "Meguri-Collection.colpkg") else {
                    throw NSError(domain: "CollectionEntryJourneyTests", code: 3)
                }
            }
            #endif
            XCTAssertTrue(element(app, "settings.importExport.saved").waitForExistence(timeout: 30), app.debugDescription)
            let ownerCompleted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                let choose = self.element(app, "settings.importExport.choose")
                let discard = self.element(app, "settings.importExport.discard")
                return choose.exists && choose.isEnabled && choose.isHittable
                    && discard.exists && discard.isEnabled && discard.isHittable
                    && !self.element(app, "settings.importExport.progress").exists
                    && !self.element(app, "settings.importExport.retryCleanup").exists
            }, object: nil)
            XCTAssertEqual(XCTWaiter().wait(for: [ownerCompleted], timeout: 10), .completed,
                "The native destination and export owner must finish before continuing")
            #if os(macOS)
            XCTAssertTrue(FileManager.default.fileExists(atPath: saved.path), "NSSavePanel must save the expected collection filename at \(saved.path)")
            #endif
        }
        return saved
    }
    #if os(macOS)
    // NSSavePanel exposes a filename editor. Touch Files keeps the product's
    // filename and instead creates a fresh child through its native controls.
    private func selectNativeExportFilename(_ fields: XCUIElementQuery, _ filename: String,
                                            _ app: XCUIApplication, _ bundle: String) throws {
        let stem = String(filename.dropLast(".colpkg".count))
        let names = ["Meguri-Collection", "Meguri-Collection.colpkg", stem, filename]
        let candidates = fields.matching(NSPredicate(format: "value IN %@", names))
        let field = try XCTUnwrap(waitForHittable(candidates), app.debugDescription)
        XCTAssertEqual(candidates.count, 1, "The native save picker must expose one filename field")
        let previous = try XCTUnwrap(field.value as? String)
        let entered = previous.hasSuffix(".colpkg") ? filename : stem
        try click(field, app, bundle)
        field.typeKey("a", modifierFlags: .command)
        field.typeText(entered)
        XCTAssertEqual(field.value as? String, entered, "The native destination must use this run's exact filename")
    }
    #endif
    #if os(iOS)
    private func nativeExportFolderIsOpen(_ name: String, _ app: XCUIApplication) -> Bool {
        let actions = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].buttons
            .matching(NSPredicate(format: "label == %@", "\(name), Actions Menu")).firstMatch
        return app.navigationBars[name].firstMatch.exists || actions.exists
    }

    private func selectNativeExportChild(_ stem: String, create: Bool,
                                         _ app: XCUIApplication, _ bundle: String) throws {
        func actionable(_ target: XCUIElement) -> Bool {
            target.exists && target.isEnabled && target.isHittable
        }
        func waitForState(_ message: String, _ condition: @escaping () -> Bool) throws {
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
            guard XCTWaiter().wait(for: [ready], timeout: 10) == .completed else {
                capturePicker(app)
                XCTFail(message)
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
        }
        let childCell = app.cells.containing(NSPredicate(format: "label IN %@",
            [stem, "\(stem), Folder"])).firstMatch
        let childFolder = app.descendants(matching: .any).matching(NSPredicate(
            format: "label == %@ OR identifier == %@", "\(stem), Folder", "\(stem), Folder")).firstMatch
        func childTarget() -> XCUIElement? {
            let child = childCell.exists ? childCell : childFolder
            guard child.exists else { return nil }
            let image = child.images.firstMatch
            let target = image.exists ? image : child
            return actionable(target) ? target : nil
        }
        if !create {
            // Admit a remembered child only by its actual native navigation bar.
            // Otherwise use the existing local-root navigation before selecting it.
            let local = app.descendants(matching: .any).matching(NSPredicate(
                format: "label IN %@", ["On My iPhone", "On My iPad"])).firstMatch
            let root = app.descendants(matching: .any).matching(NSPredicate(
                format: "label IN %@", ["RF2 Import Tests", "RF2 Import Tests, Folder"])).firstMatch
            let meguri = app.cells.containing(NSPredicate(format: "label == %@", "Meguri")).firstMatch
            try waitForState("Files must load the owned child or a known local folder location", {
                self.nativeExportFolderIsOpen(stem, app) || self.nativeExportFolderIsOpen("RF2 Import Tests", app)
                    || actionable(root) || actionable(meguri) || actionable(local)
            })
            if nativeExportFolderIsOpen(stem, app) { return }
        }
        try selectLocalExportFolder(app, bundle)
        try waitForState("The native RF2 Import Tests navigation bar must precede child selection", {
            self.nativeExportFolderIsOpen("RF2 Import Tests", app)
        })
        if create {
            guard !childCell.exists && !childFolder.exists else {
                capturePicker(app)
                XCTFail("The per-run native child folder must be fresh")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
            let actions = app.navigationBars.buttons.matching(NSPredicate(
                format: "label == %@", "RF2 Import Tests, Actions Menu")).firstMatch
            try waitForState("The loaded root folder must offer its native Actions Menu", { actionable(actions) })
            try click(actions, app, bundle)
            let newFolderButton = app.buttons.matching(NSPredicate(format: "label == %@", "New Folder")).firstMatch
            let newFolderItem = app.menuItems.matching(NSPredicate(format: "label == %@", "New Folder")).firstMatch
            try waitForState("The native folder Actions Menu must offer New Folder", {
                actionable(newFolderButton) || actionable(newFolderItem)
            })
            try click(actionable(newFolderButton) ? newFolderButton : newFolderItem, app, bundle)
            let commitName = NSPredicate(format: "label IN %@", ["Create", "Done"])
            func nameDialog() -> XCUIElement? {
                (app.alerts.allElementsBoundByIndex + app.sheets.allElementsBoundByIndex).first { dialog in
                    dialog.exists && dialog.textFields.count == 1
                        && (dialog.label == "New Folder" || dialog.staticTexts["New Folder"].exists
                            || dialog.navigationBars["New Folder"].exists)
                        && dialog.buttons.matching(commitName).count == 1
                }
            }
            try waitForState("New Folder must present its native name dialog and Create or Done", {
                guard let dialog = nameDialog() else { return false }
                return actionable(dialog.textFields.firstMatch)
            })
            guard let dialog = nameDialog() else {
                capturePicker(app)
                XCTFail("The loaded native new-folder name dialog must remain available")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
            let field = dialog.textFields.firstMatch
            let previous = field.value as? String ?? ""
            try click(field, app, bundle)
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count) + stem)
            try waitForState("The native new-folder dialog must contain the exact safe nonce stem", {
                field.value as? String == stem
            })
            let commit = dialog.buttons.matching(commitName).firstMatch
            try waitForState("The native new-folder dialog must enable Create or Done", { actionable(commit) })
            try click(commit, app, bundle)
            try waitForState("Creating the child must dismiss the dialog and reveal that exact folder", {
                !dialog.exists && (self.nativeExportFolderIsOpen(stem, app)
                    || (self.nativeExportFolderIsOpen("RF2 Import Tests", app) && childTarget() != nil))
            })
        }
        if !nativeExportFolderIsOpen(stem, app) {
            try waitForState("RF2 Import Tests must offer the exact existing nonce child", { childTarget() != nil })
            guard let target = childTarget() else {
                capturePicker(app)
                XCTFail("The exact native nonce child must remain actionable")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
            try click(target, app, bundle)
        }
        try waitForState("The actual native navigation bar must name this run's exact child folder", {
            self.nativeExportFolderIsOpen(stem, app)
        })
    }

    private func confirmNativeExportReplacement(_ app: XCUIApplication, expectedFilename: String? = nil) -> Bool {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let exactReplace = NSPredicate(format: "label == %@", "Replace")
        let namedFile = NSPredicate(format: "label CONTAINS %@", expectedFilename ?? "")
        let appAlerts = expectedFilename == nil ? app.alerts : app.alerts.containing(namedFile)
        let systemAlerts = expectedFilename == nil ? springboard.alerts : springboard.alerts.containing(namedFile)
        let appAlertReplace = appAlerts.buttons.matching(exactReplace)
        let appReplace = app.buttons.matching(exactReplace)
        let systemReplace = systemAlerts.buttons.matching(exactReplace)
        func actionable(_ query: XCUIElementQuery) -> XCUIElement? {
            query.allElementsBoundByIndex.first { $0.exists && $0.isEnabled && $0.isHittable }
        }
        var replace: XCUIElement?
        let offered = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            replace = actionable(appAlertReplace) ?? (expectedFilename == nil ? actionable(appReplace) : nil)
                ?? actionable(systemReplace)
            return replace != nil
        }, object: nil)
        guard XCTWaiter().wait(for: [offered], timeout: 30) == .completed, let replace else {
            capturePicker(app)
            #if targetEnvironment(simulator)
            // Public synthetic picker evidence only; never capture a host screen.
            func captureWindowAndHierarchy(_ owner: XCUIApplication, _ name: String) {
                guard owner.exists else { return }
                let window = owner.windows.firstMatch
                if window.exists {
                    let shot = XCTAttachment(screenshot: window.screenshot())
                    shot.name = "export-replace-timeout-\(name)-window"
                    shot.lifetime = .keepAlways
                    add(shot)
                }
                let hierarchy = XCTAttachment(string: owner.debugDescription)
                hierarchy.name = "export-replace-timeout-\(name)-hierarchy"
                hierarchy.lifetime = .keepAlways
                add(hierarchy)
            }
            captureWindowAndHierarchy(app, "app")
            captureWindowAndHierarchy(springboard, "springboard")
            func attributes(_ query: XCUIElementQuery) -> String {
                let targets = query.allElementsBoundByIndex
                guard !targets.isEmpty else { return "exists=false isHittable=false" }
                return targets.map { target in
                    guard target.exists else { return "exists=false isHittable=false" }
                    return "exists=true isEnabled=\(target.isEnabled) isHittable=\(target.isHittable) label=\(target.label.debugDescription) identifier=\(target.identifier.debugDescription) frame=\(target.frame)"
                }.joined(separator: "\n")
            }
            let exactSave = NSPredicate(format: "label == %@", "Save")
            let rows = [
                "App exact Save Buttons: \(attributes(app.buttons.matching(exactSave)))",
                "App picker Save Button: \(attributes(app.buttons.matching(identifier: "DOCPicker.actionButton")))",
                "App alert exact Replace Buttons: \(attributes(appAlertReplace))",
                "App exact Replace Buttons: \(attributes(appReplace))",
                "App activityIndicators: \(attributes(app.activityIndicators))",
                "SpringBoard alert exact Save Buttons: \(attributes(springboard.alerts.buttons.matching(exactSave)))",
                "SpringBoard alert exact Replace Buttons: \(attributes(systemReplace))",
                "SpringBoard activityIndicators: \(attributes(springboard.activityIndicators))"
            ]
            let diagnostic = XCTAttachment(string: rows.joined(separator: "\n"))
            diagnostic.name = "export-replace-timeout-native-attributes"
            diagnostic.lifetime = .keepAlways
            add(diagnostic)
            #endif
            XCTFail("Native replacement must offer an existing, enabled, hittable exact Replace Button in Meguri or a SpringBoard alert within 30 seconds, naming \(expectedFilename ?? "the existing export")")
            return false
        }
        replace.tap()
        return true
    }

    private func cancelNativeExportPicker(_ app: XCUIApplication, _ bundle: String) throws {
        let exactCancel = NSPredicate(format: "label == %@", "Cancel")
        let cancelButton = app.buttons.matching(exactCancel).firstMatch
        let cancelMenuItem = app.menuItems.matching(exactCancel).firstMatch
        let cancelOther = app.otherElements.matching(exactCancel).firstMatch
        let back = app.buttons.matching(identifier: "BackButton").firstMatch
        let more = app.buttons.matching(identifier: "OverflowBarButtonItem").firstMatch
        let save = app.buttons.matching(identifier: "DOCPicker.actionButton").firstMatch
        func actionable(_ target: XCUIElement) -> Bool {
            target.exists && target.isEnabled && target.isHittable
        }
        func actionableCancel() -> XCUIElement? {
            if actionable(cancelButton) { return cancelButton }
            if actionable(cancelMenuItem) { return cancelMenuItem }
            return actionable(cancelOther) ? cancelOther : nil
        }
        func completedRoot() -> Bool {
            !back.exists && (actionable(save) || actionable(more))
        }
        func captureFailure(_ reason: String) {
            capturePicker(app)
            func attributes(_ target: XCUIElement) -> String {
                guard target.exists else { return "exists=false" }
                return "elementType=\(target.elementType) label=\(target.label.debugDescription) identifier=\(target.identifier.debugDescription) exists=true isEnabled=\(target.isEnabled) isHittable=\(target.isHittable) frame=\(target.frame)"
            }
            let rows = [reason,
                "Cancel Button: \(attributes(cancelButton))",
                "Cancel MenuItem: \(attributes(cancelMenuItem))",
                "Cancel Other: \(attributes(cancelOther))",
                "Back: \(attributes(back))",
                "More: \(attributes(more))",
                "Save: \(attributes(save))"]
            let diagnostic = XCTAttachment(string: rows.joined(separator: "\n"))
            diagnostic.name = "export-cancel-postwait-native-attributes"
            diagnostic.lifetime = .keepAlways
            add(diagnostic)
        }
        let loaded = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            actionableCancel() != nil || actionable(back) || completedRoot()
        }, object: nil)
        guard XCTWaiter().wait(for: [loaded], timeout: 10) == .completed else {
            captureFailure("No actionable native Cancel, Back or More after loading")
            XCTFail("Native export picker did not load cancellation navigation")
            return
        }
        // Files may replace Cancel with Back inside RF2 Import Tests → Meguri
        // → On My Device → Browse. Each tap must produce an observed transition.
        var steps = 0
        var visited: Set<String> = []
        while actionableCancel() == nil, actionable(back), steps < 4 {
            let label = back.label
            guard ["Meguri", "On My iPhone", "On My iPad", "Browse"].contains(label) else {
                captureFailure("Unexpected native Back destination: \(label)")
                XCTFail("Native export picker left the known local folder hierarchy")
                return
            }
            guard visited.insert(label).inserted else {
                captureFailure("Native Back returned to an unchanged navigation state")
                XCTFail("Native export picker must not retry the same Back state")
                return
            }
            try click(back, app, bundle)
            steps += 1
            let navigated = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                actionableCancel() != nil
                    || (actionable(back) && back.label != label)
                    || completedRoot()
            }, object: nil)
            guard XCTWaiter().wait(for: [navigated], timeout: 10) == .completed else {
                captureFailure("Native Back did not reveal Cancel, change its actionable label or reach the root at step \(steps)")
                XCTFail("Native export picker Back navigation did not advance")
                return
            }
        }
        if let cancel = actionableCancel() {
            try click(cancel, app, bundle)
            return
        }
        // Only inspect the native overflow once, after Back navigation ends.
        if !back.exists, actionable(more) {
            try click(more, app, bundle)
            let offered = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in actionableCancel() != nil }, object: nil)
            if XCTWaiter().wait(for: [offered], timeout: 5) == .completed, let cancel = actionableCancel() {
                try click(cancel, app, bundle)
                return
            }
        }
        captureFailure("No visible native Cancel after \(steps) Back steps and at most one overflow inspection")
        XCTFail("Native export picker must offer a hittable Cancel")
    }

    private func selectLocalExportFolder(_ app: XCUIApplication, _ bundle: String) throws {
        let save = app.buttons.matching(identifier: "DOCPicker.actionButton").firstMatch
        let back = app.buttons.matching(identifier: "BackButton").firstMatch
        let local = app.descendants(matching: .any).matching(NSPredicate(
            format: "label IN %@", ["On My iPhone", "On My iPad"])).firstMatch
        let folderBar = app.navigationBars["RF2 Import Tests"].firstMatch
        let folderActions = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].buttons
            .matching(NSPredicate(format: "label == %@", "RF2 Import Tests, Actions Menu")).firstMatch
        let folder = app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@ OR identifier == %@",
            ["RF2 Import Tests", "RF2 Import Tests, Folder"], "RF2 Import Tests, Folder")).firstMatch
        let meguri = app.cells.containing(NSPredicate(format: "label == %@", "Meguri")).firstMatch
        func actionable(_ target: XCUIElement) -> Bool {
            target.exists && target.isEnabled && target.isHittable
        }
        func isInFolder() -> Bool {
            folderBar.exists || actionable(folderActions)
        }
        func folderTarget() -> XCUIElement? {
            guard folder.exists else { return nil }
            // Tap the known folder's image when Files exposes one: tapping its
            // caption can edit the name instead of opening the folder.
            let image = folder.images.firstMatch
            if image.exists { return actionable(image) ? image : nil }
            return actionable(folder) ? folder : nil
        }
        func waitForState(_ message: String, _ condition: @escaping () -> Bool) -> Bool {
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
            guard XCTWaiter().wait(for: [ready], timeout: 10) == .completed else {
                capturePicker(app)
                let diagnostic = XCTAttachment(string: message)
                diagnostic.name = "export-folder-navigation-state"
                diagnostic.lifetime = .keepAlways
                add(diagnostic)
                XCTFail(message)
                return false
            }
            return true
        }
        // First let the newly presented native picker load. Then wait for any
        // known visible folder state, rather than timing out each deeper path.
        guard waitForState("Native Save, Back or local location must become actionable", {
            actionable(save) || actionable(back) || actionable(local)
        }) else { return }
        guard waitForState("Files must load the remembered folder or a known local folder location", {
            isInFolder() || folderTarget() != nil || actionable(meguri) || actionable(local)
        }) else { return }
        if isInFolder() { return }
        if folderTarget() == nil && !actionable(meguri) {
            guard actionable(local) else {
                capturePicker(app)
                XCTFail("Files must offer the native On My iPhone or On My iPad location")
                return
            }
            try click(local, app, bundle)
            guard waitForState("The local location must reveal Meguri or RF2 Import Tests", {
                isInFolder() || folderTarget() != nil || actionable(meguri)
            }) else { return }
        }
        if isInFolder() { return }
        if folderTarget() == nil {
            guard actionable(meguri) else {
                capturePicker(app)
                XCTFail("The local location must offer the native Meguri container")
                return
            }
            try click(meguri, app, bundle)
            guard waitForState("Meguri must reveal the actionable RF2 Import Tests folder", {
                folderTarget() != nil
            }) else { return }
        }
        if folderTarget() == nil { capturePicker(app) }
        let target = try XCTUnwrap(folderTarget(), "RF2 Import Tests must have a hittable native folder target")
        try click(target, app, bundle)
        _ = waitForState("Opening RF2 Import Tests must show its native folder navigation title", {
            isInFolder()
        })
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
        let study = deckPage.buttons["home.deck.study"].firstMatch
        try waitForStudyReady(study, app)
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
    private func waitForStudyReady(_ study: XCUIElement, _ app: XCUIApplication) throws {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            study.exists && study.elementType == .button && study.isEnabled && study.isHittable
        }, object: nil)
        guard XCTWaiter().wait(for: [ready], timeout: 30) == .completed else {
            capture(app, "entry-study-not-ready")
            XCTFail("Study Now must be an existing, enabled, hittable Button after collection admission: \(app.debugDescription)")
            throw NSError(domain: "CollectionEntryJourneyTests", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Study Now did not become ready within 30 seconds"])
        }
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
