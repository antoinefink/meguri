import Foundation
import CryptoKit
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
    #if os(iOS)
    func testSettingsAccountSyncReopensWithMeguriSelected() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        XCTAssertTrue(build.hasHooks && bundle.hasSuffix(".dev"), "Requires the development fixture installer")
        guard build.hasHooks, bundle.hasSuffix(".dev") else { return }
        let appInfo = try build.appInfo
        try XCTSkipIf(appInfo["MeguriCloudSchemaConfigured"] as? Bool != true,
            "Requires team signing with the cloud schema configured")
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        XCTAssertTrue(app.staticTexts["Welcome to Meguri"].waitForExistence(timeout: 10))
        let settings = try XCTUnwrap(waitForShellSection(app, tabIdentifier: "shell.root.tab.settings",
            sidebarIdentifier: "shell.root.sidebar.settings", compactLabel: "Settings", timeout: 10), app.debugDescription)
        try click(settings, app, bundle)
        let accountRows = app.buttons.matching(identifier: "settings.root.accountSync")
        let account = try XCTUnwrap(waitForHittable(accountRows), app.debugDescription)
        XCTAssertEqual(accountRows.count, 1)
        try click(account, app, bundle)
        XCTAssertTrue(app.staticTexts["Choose a service to sync this collection."].waitForExistence(timeout: 10),
            "Requires fresh synthetic local-only provider state")
        let services = app.buttons.matching(identifier: "settings.accountSync.service")
        let service = try XCTUnwrap(waitForHittable(services), app.debugDescription)
        XCTAssertEqual(services.count, 1)
        XCTAssertTrue(service.isEnabled)
        try click(service, app, bundle)
        // The standard iOS Picker exposes menu choices as native buttons.
        let options = app.buttons.matching(NSPredicate(format: "label == %@", "Meguri Sync"))
        let meguri = try XCTUnwrap(waitForHittable(options), app.debugDescription)
        XCTAssertEqual(options.count, 1)
        try click(meguri, app, bundle)

        func checkFormAndGoBack() throws {
            let serviceCommitted = XCTWaiter().wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format:
                "exists == true AND (label == %@ OR value == %@ OR label == %@)",
                "Meguri Sync", "Meguri Sync", "Sync Service, Meguri Sync"), object: service)], timeout: 20) == .completed
            if !serviceCommitted {
                // Fixed synthetic targets only; never read credential field values.
                let identifiers = ["settings.root.accountSync", "settings.accountSync.content",
                    "settings.accountSync.service", "settings.accountSync.meguri.address",
                    "settings.accountSync.meguri.username", "settings.accountSync.meguri.password",
                    "settings.accountSync.meguri.signIn", "sync.serviceSwitch.confirm"]
                var context = identifiers.map { identifier in
                    "\(identifier): count=\(app.descendants(matching: .any).matching(identifier: identifier).count)"
                }
                if service.exists {
                    context.append("service: label=\(service.label.debugDescription) value=\(String(describing: service.value)) enabled=\(service.isEnabled) hittable=\(service.isHittable)")
                }
                context.append("Account & Sync navigation bar: \(app.navigationBars["Account & Sync"].exists)")
                context.append("Settings navigation bar: \(app.navigationBars["Settings"].exists)")
                let attachment = XCTAttachment(string: context.joined(separator: "\n"))
                attachment.name = "settings-account-sync-unsettled"
                attachment.lifetime = .keepAlways
                add(attachment)
            }
            XCTAssertTrue(serviceCommitted, "The committed service must survive reopening")
            XCTAssertTrue(element(app, "settings.accountSync.content").exists)
            for name in ["address", "username", "password"] {
                let fields = name == "password"
                    ? app.secureTextFields.matching(identifier: "settings.accountSync.meguri.password")
                    : app.textFields.matching(identifier: "settings.accountSync.meguri.\(name)")
                let field = try XCTUnwrap(waitForHittable(fields), "Actionable native \(name) field required")
                XCTAssertEqual(fields.count, 1)
                XCTAssertTrue(field.isEnabled)
            }
            let signIns = app.buttons.matching(identifier: "settings.accountSync.meguri.signIn")
            let signIn = signIns.firstMatch
            XCTAssertTrue(signIn.waitForExistence(timeout: 20), app.debugDescription)
            XCTAssertEqual(signIns.count, 1)
            XCTAssertEqual(signIn.label, "Sign In")
            XCTAssertFalse(signIn.isEnabled, "Empty credentials must not submit")
            XCTAssertFalse(app.buttons["settings.accountSync.done"].exists, "Use the pushed Settings page")
            // Exercise real secure-field focus without credentials or submission.
            let password = app.secureTextFields["settings.accountSync.meguri.password"]
            try click(password, app, bundle)
            XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
            XCTAssertTrue(password.exists && password.isEnabled && password.isHittable)
            let back = try XCTUnwrap(waitForHittable(app.navigationBars["Account & Sync"].buttons
                .matching(NSPredicate(format: "label == %@", "Settings"))), app.debugDescription)
            try click(back, app, bundle)
            _ = try XCTUnwrap(waitForHittable(accountRows), "One Back must return to the Settings root")
            XCTAssertTrue(app.navigationBars["Settings"].exists)
            XCTAssertFalse(element(app, "settings.accountSync.content").exists)
            XCTAssertFalse(app.secureTextFields["settings.accountSync.meguri.password"].exists)
            XCTAssertTrue(app.buttons["settings.root.collection"].exists)
        }

        try checkFormAndGoBack()
        let reopen = try XCTUnwrap(waitForHittable(accountRows), app.debugDescription)
        try click(reopen, app, bundle)
        try checkFormAndGoBack()
    }
    #endif

    func testNoFileHomeAccountSyncActionReturnsToCollectionChoices() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development source setup") }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        let accountSyncButtons = app.buttons.matching(identifier: "home.collection.accountSync")
        let accountSync = try XCTUnwrap(waitForHittable(accountSyncButtons), app.debugDescription)
        XCTAssertEqual(accountSyncButtons.count, 1)
        // N18.3: one row for sync, which opens Account & Sync.
        XCTAssertEqual(accountSync.label, "Set Up Sync")
        XCTAssertTrue(accountSync.isEnabled)
        XCTAssertTrue(app.staticTexts["Welcome to Meguri"].exists)
        XCTAssertTrue(app.staticTexts["Choose how to start. You can change this later in Settings."].exists)
        XCTAssertFalse(app.staticTexts["No Collection"].exists)
        let startEmptyButtons = app.buttons.matching(identifier: "home.collection.startEmpty")
        let startEmpty = try XCTUnwrap(waitForHittable(startEmptyButtons), app.debugDescription)
        XCTAssertEqual(startEmptyButtons.count, 1)
        XCTAssertEqual(startEmpty.label, "Start Empty")
        XCTAssertTrue(startEmpty.isEnabled)
        let importCollection = app.buttons["home.collection.import"].firstMatch
        XCTAssertTrue(importCollection.exists && importCollection.isEnabled && importCollection.isHittable)
        XCTAssertEqual(importCollection.label, "Import a Collection File")
        // The former second route to sync is gone.
        XCTAssertFalse(element(app, "home.collection.connect").exists)
        let choices = ["home.collection.accountSync", "home.collection.import", "home.collection.startEmpty"]
            .map { app.buttons[$0].firstMatch.frame.minY }
        XCTAssertEqual(choices, choices.sorted(), "Sync, then the file, then the empty collection")
        XCTAssertFalse(element(app, "settings.accountSync.content").exists)
        try click(accountSync, app, bundle)

        #if os(macOS)
        let settings = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 10), app.debugDescription)
        let root = settings.descendants(matching: .any).matching(identifier: "settings.root.content")
        XCTAssertEqual(root.count, 1, "Account & Sync must open in the native Settings window")
        let contents = settings.descendants(matching: .any).matching(identifier: "settings.accountSync.content")
        let content = contents.firstMatch
        XCTAssertTrue(content.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertEqual(contents.count, 1)
        #else
        let content = element(app, "settings.accountSync.content")
        XCTAssertTrue(content.waitForExistence(timeout: 10), app.debugDescription)
        #endif
        let service = content.descendants(matching: .any).matching(identifier: "settings.accountSync.service")
        XCTAssertTrue(service.firstMatch.waitForExistence(timeout: 10))
        XCTAssertEqual(service.count, 1, "The standard form must contain its existing Sync Service picker")
        #if os(macOS)
        XCTAssertEqual(service.firstMatch.elementType, .popUpButton)
        try app.bringToFront(bundleIdentifier: bundle)
        settings.typeKey("w", modifierFlags: .command)
        XCTAssertTrue(settings.waitForNonExistence(timeout: 10), "Close the actual Settings window")
        #else
        let doneButtons = app.buttons.matching(identifier: "settings.accountSync.done")
        let done = try XCTUnwrap(waitForHittable(doneButtons), app.debugDescription)
        XCTAssertEqual(doneButtons.count, 1)
        try click(done, app, bundle)
        #endif

        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: content)], timeout: 10), .completed)
        _ = try XCTUnwrap(waitForHittable(accountSyncButtons), app.debugDescription)
        XCTAssertTrue(element(app, "home.collection.screen").exists)
        XCTAssertTrue(app.staticTexts["Welcome to Meguri"].exists, "Opening Account & Sync must leave Home without a collection")
        _ = try XCTUnwrap(waitForHittable(startEmptyButtons), app.debugDescription)
        XCTAssertEqual(startEmptyButtons.count, 1)
        XCTAssertEqual(startEmpty.label, "Start Empty")
        XCTAssertTrue(startEmpty.isEnabled)
        XCTAssertFalse(app.staticTexts["No Decks"].exists)
        XCTAssertTrue(importCollection.exists && importCollection.isEnabled && importCollection.isHittable)
        XCTAssertFalse(element(app, "home.collection.connect").exists)
        XCTAssertFalse(element(app, "settings.accountSync.done").exists)
    }

    func testStartEmptyAddNoteAndStudyGeneratedCard() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development fixture hooks") }
        let app = XCUIApplication()
        // The existing installer copies this public empty directory into the
        // development container; all writes belong to that installed copy.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        let startEmpty = try XCTUnwrap(waitForHittable(app.buttons.matching(identifier: "home.collection.startEmpty")),
            app.debugDescription)
        XCTAssertTrue(startEmpty.isEnabled)
        try click(startEmpty, app, bundle)
        // First-local creation persists Default with ID 1, without cards.
        let defaultDeck = deckRows(app, identifier: "home.deck.row.1")
        try waitForAdmission(defaultDeck, app, bundle)
        let deck = try XCTUnwrap(waitForHittable(defaultDeck), app.debugDescription)
        try click(deck, app, bundle)
        let deckPage = element(app, "home.deck.page.1")
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        // N14.3: an empty deck shows "Nothing to study right now", not three zero counts.
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: 0, learning: 0, review: 0), app.debugDescription)
        XCTAssertFalse(deckPage.buttons["home.deck.study"].firstMatch.exists, "Add Note takes Study's place while nothing waits")
        let emptyAddButtons = deckPage.buttons.matching(identifier: "home.deck.empty.addNote")
        let emptyDeckAdd = try XCTUnwrap(waitForHittable(emptyAddButtons), app.debugDescription)
        XCTAssertEqual(emptyAddButtons.count, 1)
        XCTAssertEqual(emptyDeckAdd.label, "Add Note")
        XCTAssertTrue(emptyDeckAdd.isEnabled)
        try click(emptyDeckAdd, app, bundle)

        let editor = element(app, "editor.add")
        XCTAssertTrue(editor.waitForExistence(timeout: 10))
        let fields = editor.descendants(matching: .any)["editor.fields"].firstMatch
        XCTAssertTrue(fields.waitForExistence(timeout: 10))
        let front = "Meguri empty deck native question QZ71"
        let back = "Meguri empty deck native answer QZ83"
        // N17.1: Deck then Type as menu rows; Add waits for the first field.
        XCTAssertTrue(element(app, "editor.add.deck").waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(element(app, "editor.add.type").exists, app.debugDescription)
        let emptyAdd = editor.buttons["editor.add.save"].firstMatch
        XCTAssertTrue(emptyAdd.waitForExistence(timeout: 10))
        XCTAssertFalse(emptyAdd.isEnabled, "Add stays disabled until the first field has content")
        XCTAssertFalse(element(app, "editor.add.fieldStatus").exists)
        for (name, content) in [("Front", front), ("Back", back)] {
            // Production declares a named multiline textbox. Require its
            // native AX input, rather than the field's header.
            let inputs = fields.descendants(matching: .textView).matching(NSPredicate(format: "label == %@", name))
            let input = try XCTUnwrap(waitForHittable(inputs), "Expected an actionable \(name) multiline text view")
            XCTAssertEqual(inputs.count, 1)
            XCTAssertTrue(input.isEnabled)
            try click(input, app, bundle)
            try app.bringToFront(bundleIdentifier: bundle)
            input.typeText(content)
            // Require the live native AX String, allowing exactly one terminal LF.
            var characterCount = -1
            var exactWithTerminalLF = false
            let typed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                guard input.exists, let value = input.value as? String else {
                    characterCount = -1; exactWithTerminalLF = false
                    return false
                }
                characterCount = min(value.count, 128)
                exactWithTerminalLF = value == content + "\n"
                return value == content || exactWithTerminalLF
            }, object: nil)
            XCTAssertEqual(XCTWaiter().wait(for: [typed], timeout: 20), .completed,
                "Native typing must reach \(name); AX String character count (capped at 128)=\(characterCount), exactWithTerminalLF=\(exactWithTerminalLF)")
        }
        let add = editor.buttons["editor.add.save"].firstMatch
        let addReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            add.exists && add.isEnabled && add.isHittable
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [addReady], timeout: 20), .completed,
            "The real field-change events must make Add actionable")
        XCTAssertEqual(add.label, "Add")
        try click(add, app, bundle)
        // N17.1: the confirmation names the deck; the sheet stays open on a cleared form.
        #if os(macOS)
        XCTAssertTrue(waitForText(element(app, "editor.add.result"), "Added to Default"))
        #else
        // On touch the confirmation is the navigation bar's subtitle.
        XCTAssertTrue(app.navigationBars.staticTexts["Added to Default"].firstMatch.waitForExistence(timeout: 20), app.debugDescription)
        #endif
        XCTAssertTrue(editor.exists)
        XCTAssertFalse(element(app, "editor.add.error").exists)
        let closeEditor = try XCTUnwrap(waitForHittable(editor.buttons.matching(identifier: "editor.add.close")),
            app.debugDescription)
        XCTAssertTrue(closeEditor.isEnabled)
        // The ordinary Close runs the held-flush guard on the post-save draft.
        try click(closeEditor, app, bundle)
        XCTAssertTrue(editor.waitForNonExistence(timeout: 20))
        let addedToDeck = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            deckPage.exists && HomeControls.deckCounts(app) == [1, 0, 0] && !emptyAddButtons.firstMatch.exists
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [addedToDeck], timeout: 20), .completed,
            "The saved card must refresh Default and remove its empty-deck Add CTA")

        let study = deckPage.buttons["home.deck.study"].firstMatch
        try waitForStudyReady(study, app)
        try click(study, app, bundle)
        let renderedFront = app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", front, front)).firstMatch
        let renderedBack = app.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", back, back)).firstMatch
        XCTAssertTrue(renderedFront.waitForExistence(timeout: 20), "Study must render the newly typed question")
        XCTAssertFalse(renderedBack.exists)
        let reveal = try XCTUnwrap(waitForHittable(app.buttons.matching(identifier: "review.session.reveal")),
            app.debugDescription)
        XCTAssertTrue(reveal.isEnabled)
        try click(reveal, app, bundle)
        XCTAssertTrue(renderedBack.waitForExistence(timeout: 20), "Show Answer must render the newly typed answer")
        let closeReview = try XCTUnwrap(waitForHittable(app.buttons.matching(identifier: "review.session.close")),
            app.debugDescription)
        XCTAssertTrue(closeReview.isEnabled)
        try click(closeReview, app, bundle)
        try waitForStudyReady(study, app)
    }

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
        // N17.3: a finished save closes the export by itself; nothing is left to discard.
        let saved = try exerciseNativeExport(app, bundle, destinationFolder: folder)
        let settingsContent = element(app, "settings.root.content")
        XCTAssertFalse(element(app, "settings.importExport.content").exists, app.debugDescription)
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
        // N17.2: Import opens the system picker directly, with no sheet before it.
        try click(importCollection, app, bundle)
        try pick(saved, app, bundle)
        // The saved review travels in the file: the summary's value carries its count.
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.counts"), "7 reviews"))
        try click(element(app, "onboarding.collectionImport.confirm"), app, bundle)
        let imported = deckRows(app, identifier: "home.deck.row.1788249600212")
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
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
        app.terminate()
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
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
        let recordPath = try XCTUnwrap(ProcessInfo.processInfo.environment["MEGURI_NATIVE_MEDIA_FIXTURE_RECORD"],
            "Native media capture requires the internal prepared fixture record")
        let recordURL = URL(fileURLWithPath: recordPath)
        let runURL = recordURL.deletingLastPathComponent()
        let runsURL = build.repositoryRoot.appending(path: "build/mg/ui-test", directoryHint: .isDirectory)
        guard recordURL.lastPathComponent == "expected-source.json",
              recordURL.standardizedFileURL.path == recordPath,
              recordURL.resolvingSymlinksInPath().path == recordPath,
              runURL.deletingLastPathComponent().path == runsURL.path,
              runURL.lastPathComponent.range(of: #"^[A-Za-z0-9][A-Za-z0-9._-]*$"#, options: .regularExpression) != nil else {
            XCTFail("Native media capture requires its own run's bound fixture")
            throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
        }
        let record = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: recordURL)) as? [String: Any])
        let fixtureURL = runURL.appending(path: "native-media-baseline/fixture", directoryHint: .isDirectory)
        let relativeFixture = "build/mg/ui-test/\(runURL.lastPathComponent)/native-media-baseline/fixture"
        XCTAssertEqual(record["native_filename"] as? String, filename)
        XCTAssertEqual(record["fixture"] as? String, relativeFixture)
        XCTAssertEqual(record["clock"] as? [String: String],
                       ["now": "2030-01-15T10:00:00+01:00", "tz": "Europe/Paris"])
        XCTAssertEqual(fixtureURL.resolvingSymlinksInPath().path, fixtureURL.path)
        let app = XCUIApplication()
        #if os(iOS)
        // Installs the local Files folder only; this source is never imported.
        let importSource: URL? = build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/review.colpkg")
        #else
        let importSource: URL? = nil
        #endif
        app.launchArguments = MeguriLaunch(
            fixture: fixtureURL,
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

    /// F11.2: one frozen input, real picker/review, finalized close, and an
    /// ordinary no-reset relaunch. F11.1 remains its separate export-only case.
    func testNativeMediaImportReviewReopenAndExport() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current(), app = XCUIApplication()
        let bundle = try build.bundleIdentifier
        guard build.hasHooks, bundle.hasSuffix(".dev") else { throw XCTSkip("Requires development hooks") }
        #if os(iOS) && !targetEnvironment(simulator)
        throw XCTSkip("F11.2 capture requires the leased test VM")
        #else
        let env = ProcessInfo.processInfo.environment
        let caseName = try XCTUnwrap(env["MEGURI_NATIVE_REVIEW_CASE"])
        XCTAssertNotNil(caseName.range(of: #"^[A-Za-z0-9][A-Za-z0-9._-]{0,119}$"#, options: .regularExpression))
        let platform = try XCTUnwrap(env["MEGURI_NATIVE_REVIEW_PLATFORM"])
        #if os(macOS)
        XCTAssertEqual(platform, "mac")
        #else
        XCTAssertEqual(platform, UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone")
        #endif
        let sha = try XCTUnwrap(env["MEGURI_NATIVE_REVIEW_FREEZE_SHA256"])
        let bytes = try XCTUnwrap(env["MEGURI_NATIVE_REVIEW_FREEZE_BYTES"])
        XCTAssertNotNil(sha.range(of: #"^[a-f0-9]{64}$"#, options: .regularExpression))
        XCTAssertGreaterThan(try XCTUnwrap(UInt64(bytes)), 0)
        let run = build.repositoryRoot.appending(path: "build/mg/ui-test/\(caseName)")
        func object(_ url: URL) throws -> [String: Any] {
            XCTAssertEqual(url.resolvingSymlinksInPath().path, url.path)
            return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        }
        let binding = try object(run.appending(path: "native-review-binding.json"))
        XCTAssertEqual(binding["case"] as? String, caseName)
        XCTAssertEqual(binding["platform"] as? String, platform)
        XCTAssertEqual(binding["configuration"] as? String, build.configuration.rawValue)
        let freeze = try XCTUnwrap(binding["freeze"] as? [String: Any])
        XCTAssertEqual(freeze["sha256"] as? String, sha)
        XCTAssertEqual((freeze["bytes"] as? NSNumber)?.stringValue, bytes)
        let input = try object(run.appending(path: "expected-replay/input.json"))
        let chosen = try XCTUnwrap(input["chosen"] as? [String: Any])
        let deckID = try XCTUnwrap(chosen["deck_id"] as? Int64)
        _ = try XCTUnwrap(chosen["card_id"] as? Int64)
        let preparation = try object(run.appending(path: "source-preparation/preparation.json"))
        let facts = try XCTUnwrap(preparation["source_facts"] as? [String: Any])
        let counts = try XCTUnwrap(facts["row_counts"] as? [String: Int])
        let reviews = try XCTUnwrap(counts["revlog"])
        let filename = try XCTUnwrap(binding["native_filename"] as? String)
        XCTAssertNotNil(filename.range(of: #"^Meguri-Media-[a-f0-9]{32}\.colpkg$"#, options: .regularExpression))
        var loadedCode: [String: [String: Any]] = [:]
        for executableURL in try build.machOFiles {
            let bytesOnDisk = try Data(contentsOf: executableURL)
            loadedCode[executableURL.lastPathComponent] = [
                "sha256": SHA256.hash(data: bytesOnDisk).map { String(format: "%02x", $0) }.joined(),
                "bytes": bytesOnDisk.count]
        }
        XCTAssertTrue(NSDictionary(dictionary: loadedCode).isEqual(to: try XCTUnwrap(binding["mach_o"] as? [String: Any])))
        let codeRecord = XCTAttachment(data: try JSONSerialization.data(withJSONObject: binding, options: [.sortedKeys]),
            uniformTypeIdentifier: "public.json")
        codeRecord.name = "native-review-app.json"
        codeRecord.lifetime = .keepAlways
        add(codeRecord)
        let native = ["-MeguriNativeReviewRoot", build.repositoryRoot.path, "-MeguriNativeReviewCase", caseName,
            "-MeguriNativeReviewPlatform", platform, "-MeguriNativeReviewFreezeSHA256", sha, "-MeguriNativeReviewFreezeBytes", bytes]
        let source = run.appending(path: "expected-replay/input.colpkg")
        #if os(iOS)
        let importSource: URL? = source
        #else
        let importSource: URL? = nil
        #endif
        app.launchArguments = MeguriLaunch(fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
            importSource: importSource, locale: "en_US", animations: false, extra: native).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        let entry = element(app, "home.collection.import")
        let launchError = element(app, "testhooks.error.message")
        XCTAssertTrue(entry.waitForExistence(timeout: 20),
            "Native first launch must expose Import Collection: \(launchError.exists ? text(launchError) : app.debugDescription)")
        try click(entry, app, bundle)
        try pick(source, app, bundle)
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.counts"), reviews == 1 ? "1 review" : "\(reviews) reviews"))
        capture(app, "native-review-import-preview")
        try click(element(app, "onboarding.collectionImport.confirm"), app, bundle)
        let deckQuery = deckRows(app, identifier: "home.deck.row.\(deckID)")
        try waitForAdmission(deckQuery, app, bundle)
        let importedEvidence = try waitNativeReview("imported", app, bundle)
        try click(try XCTUnwrap(waitForHittable(deckQuery)), app, bundle)
        let study = element(app, "home.deck.study")
        try waitForStudyReady(study, app)
        try click(study, app, bundle)
        XCTAssertTrue(element(app, "review.session.reveal").waitForExistence(timeout: 20))
        try click(element(app, "review.session.reveal"), app, bundle)
        _ = try waitNativeReview("media-ready", app, bundle)
        capture(app, "native-review-actual-image-and-finished-audio")
        try click(element(app, "review.session.rate.easy"), app, bundle)
        _ = try waitNativeReview("accepted", app, bundle)
        try click(element(app, "review.session.close"), app, bundle)
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
        _ = try waitNativeReview("ended", app, bundle)
        capture(app, "native-review-one-answer-recorded")
        try closeNativeReview(bundle)
        app.terminate()
        // Same five bound inputs; no fixture, import source, route or clock.
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, extra: native).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
        let reopenedEvidence = try waitNativeReview("reopened", app, bundle)
        capture(app, "native-review-reopened-one-answer")
        #if os(macOS)
        let owner = try XCTUnwrap(reopenedEvidence.owner)
        XCTAssertNotEqual(importedEvidence.owner, owner, "Reopen must retire the first transport owner")
        let inputBytes = try Data(contentsOf: run.appending(path: "expected-replay/input.json"))
        let transfer = NativeReviewPhaseReceiver()
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name("\(bundle).testhooks.nativeReviewPhase"),
            object: nil, queue: .main) { [weak transfer] notification in
                let object: String? = notification.object as? String
                let hasUserInfo: Bool = notification.userInfo != nil
                MainActor.assumeIsolated { transfer?.receive(object, hasUserInfo: hasUserInfo) }
            }
        defer {
            XCTAssertNil(transfer.failure, "Late/duplicate native phase reply")
            transfer.cancel(); center.removeObserver(observer)
        }
        let fixed = ["version": "1", "case": caseName, "platform": platform, "freezeSHA256": sha, "freezeBytes": bytes,
            "inputSHA256": SHA256.hash(data: inputBytes).map { String(format: "%02x", $0) }.joined(),
            "inputBytes": String(inputBytes.count), "owner": owner]
        for phase in ["import", "accepted", "closed", "reopened"] {
            var request = fixed; request["phase"] = phase; request["nonce"] = UUID().uuidString
            try transfer.begin(request)
            let data = try JSONSerialization.data(withJSONObject: request, options: [.sortedKeys, .withoutEscapingSlashes])
            XCTAssertLessThanOrEqual(data.count, 180 * 1024)
            center.postNotificationName(Notification.Name("\(bundle).testhooks.readNativeReviewPhase"),
                object: String(decoding: data, as: UTF8.self), userInfo: nil, deliverImmediately: true)
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                transfer.failure != nil || transfer.bytes[phase] != nil
            }, object: nil)
            let result = XCTWaiter().wait(for: [ready], timeout: 30)
            XCTAssertEqual(result, .completed, "Missing native phase response: \(phase)")
            XCTAssertNil(transfer.failure)
            _ = try XCTUnwrap(transfer.bytes[phase], "Missing native phase bytes")
        }
        for phase in ["import", "accepted", "closed", "reopened"] {
            let data = try XCTUnwrap(transfer.bytes[phase])
            let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
            attachment.name = "native-review-\(phase).json"; attachment.lifetime = .keepAlways; add(attachment)
        }
        let folder = FileManager.default.temporaryDirectory.appending(path: "RF3-Review-\(filename.dropFirst(13).prefix(32))")
        XCTAssertFalse(FileManager.default.fileExists(atPath: folder.path))
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: folder) }
        #else
        _ = importedEvidence; _ = reopenedEvidence
        let folder: URL? = nil
        #endif
        let saved = try exerciseNativeExport(app, bundle, destinationFolder: folder, destinationFilename: filename)
        #if os(macOS)
        let attachment = XCTAttachment(contentsOfFile: saved)
        attachment.name = filename; attachment.lifetime = .keepAlways; add(attachment)
        XCTAssertNil(transfer.failure, "Late/duplicate native phase reply")
        #endif
        capture(app, "native-review-export-all-media")
        #endif
    }

    #if os(macOS)
    /// A four-phase exchange only. Any extra, mismatched or repeated response
    /// poisons the case, including responses delivered during native export.
    @MainActor private final class NativeReviewPhaseReceiver {
        private var expected: [String: String]?
        private var active = true
        private var nonces: Set<String> = []
        private(set) var bytes: [String: Data] = [:]
        private(set) var failure: String?

        func begin(_ request: [String: String]) throws {
            guard active, failure == nil, nonces.count < 4,
                  let phase = request["phase"], ["import", "accepted", "closed", "reopened"].contains(phase),
                  bytes[phase] == nil, let nonce = request["nonce"], nonces.insert(nonce).inserted,
                  let owner = request["owner"], UUID(uuidString: owner)?.uuidString == owner,
                  expected == nil || bytes[expected!["phase"]!] != nil else {
                throw NSError(domain: "NativeReviewPhase", code: 1)
            }
            expected = request
        }
        func receive(_ object: String?, hasUserInfo: Bool) {
            guard active, failure == nil else { return }
            do {
                guard !hasUserInfo, let object,
                      object.utf8.count <= 180 * 1024, let expected,
                      let fields = try JSONSerialization.jsonObject(with: Data(object.utf8)) as? [String: String],
                      Set(fields.keys) == Set(expected.keys).union(["data"]),
                      try JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys, .withoutEscapingSlashes]) == Data(object.utf8)
                else { throw NSError(domain: "NativeReviewPhase", code: 2) }
                var binding = fields; binding.removeValue(forKey: "data")
                guard binding == expected, let phase = expected["phase"], bytes[phase] == nil,
                      let encoded = fields["data"], encoded.utf8.count <= ((128 * 1024 + 2) / 3) * 4,
                      let data = Data(base64Encoded: encoded), !data.isEmpty, data.count <= 128 * 1024,
                      data.base64EncodedString() == encoded else { throw NSError(domain: "NativeReviewPhase", code: 3) }
                // Only syntax is inspected here. Historical owner/admission
                // validation remains in the unchanged independent package proof.
                _ = try JSONSerialization.jsonObject(with: data)
                bytes[phase] = data
            } catch { failure = "Refused native phase response" }
        }
        func cancel() { active = false; expected = nil }
    }
    #endif

    @MainActor private final class NativeReviewAcknowledgement {
        var phase = ""
        var directory: String?
        var owner: String?
        var diagnostic = ""
    }
    /// Flush/query only the requested dev owner. No visible test controls.
    private func waitNativeReview(_ phase: String, _ app: XCUIApplication, _ bundle: String) throws -> NativeReviewAcknowledgement {
        let acknowledgement = NativeReviewAcknowledgement()
        #if os(macOS)
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: Notification.Name("\(bundle).testhooks.nativeReviewStatus"), object: nil, queue: .main) { notification in
            let fields: [String: String]? = {
                guard notification.userInfo == nil, let value = notification.object as? String,
                      value.utf8.count <= 4096, let data = value.data(using: .utf8) else { return nil }
                return (try? JSONSerialization.jsonObject(with: data)) as? [String: String]
            }()
            MainActor.assumeIsolated {
                guard let fields, (fields["diagnostic"] ?? "").utf8.count <= 900 else { return }
                acknowledgement.phase = fields["status"] ?? ""; acknowledgement.directory = fields["directory"]
                acknowledgement.owner = fields["owner"]; acknowledgement.diagnostic = fields["diagnostic"] ?? ""
            }
        }
        defer { center.removeObserver(observer) }
        #else
        let states = ["imported": UInt64(1), "media-ready": 2, "accepted": 3, "ended": 4, "reopened": 5]
        var token = NOTIFY_TOKEN_INVALID
        XCTAssertEqual(notify_register_check("\(bundle).testhooks.nativeReviewStatus", &token), UInt32(NOTIFY_STATUS_OK))
        defer { notify_cancel(token) }
        var initial: Int32 = 0; notify_check(token, &initial)
        #endif
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            #if os(macOS)
            center.postNotificationName(Notification.Name("\(bundle).testhooks.flushNativeReview"), object: nil, userInfo: nil, deliverImmediately: true)
            #else
            notify_post("\(bundle).testhooks.flushNativeReview")
            var changed: Int32 = 0, state: UInt64 = 0
            if notify_check(token, &changed) == NOTIFY_STATUS_OK, changed != 0, notify_get_state(token, &state) == NOTIFY_STATUS_OK {
                acknowledgement.phase = state == 9 ? "failed" : states.first(where: { $0.value == state })?.key ?? "pending"
            }
            #endif
            return acknowledgement.phase == phase || acknowledgement.phase == "failed"
        }, object: nil)
        let result = XCTWaiter().wait(for: [ready], timeout: 30)
        var diagnostics = ""
        if result != .completed || acknowledgement.phase != phase {
            let notice = element(app, "review.session.notice")
            diagnostics = "last native phase=\(acknowledgement.phase.debugDescription); native diagnostic=\(acknowledgement.diagnostic.debugDescription); review.session.notice=\(notice.exists ? text(notice).debugDescription : "<absent>")"
            if !notice.exists { diagnostics += "\n\(app.debugDescription)" }
        }
        XCTAssertEqual(result, .completed, "Native evidence did not reach \(phase); \(diagnostics)")
        XCTAssertEqual(acknowledgement.phase, phase, "Native evidence refused the case; \(diagnostics)")
        return acknowledgement
    }
    private func closeNativeReview(_ bundle: String) throws {
        #if os(macOS)
        let center = DistributedNotificationCenter.default()
        let acknowledgement = NativeReviewAcknowledgement()
        let observer = center.addObserver(forName: Notification.Name("\(bundle).testhooks.nativeReviewClosed"), object: nil, queue: .main) { notification in
            let result = notification.object as? String
            MainActor.assumeIsolated { acknowledgement.phase = result ?? "failed" }
        }
        defer { center.removeObserver(observer) }
        center.postNotificationName(Notification.Name("\(bundle).testhooks.closeNativeReview"), object: nil, userInfo: nil, deliverImmediately: true)
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !acknowledgement.phase.isEmpty }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed)
        XCTAssertEqual(acknowledgement.phase, "saved")
        #else
        var token = NOTIFY_TOKEN_INVALID, changed: Int32 = 0
        XCTAssertEqual(notify_register_check("\(bundle).testhooks.nativeReviewClosed", &token), UInt32(NOTIFY_STATUS_OK))
        defer { notify_cancel(token) }
        notify_check(token, &changed)
        var state: UInt64 = 0
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            var changed: Int32 = 0
            return notify_check(token, &changed) == NOTIFY_STATUS_OK && changed != 0 && notify_get_state(token, &state) == NOTIFY_STATUS_OK
        }, object: nil)
        XCTAssertEqual(notify_post("\(bundle).testhooks.closeNativeReview"), UInt32(NOTIFY_STATUS_OK))
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed)
        XCTAssertEqual(state, 1, "Actual collection close and saved receipt required before termination")
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
        // N17.3: Export Collection prepares at once, then the system save dialog opens.
        // Cancelling or finishing there closes the export and leaves Settings.
        let exportContent = element(app, "settings.importExport.content")
        func exportClosed() -> Bool {
            let closed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                !exportContent.exists && !self.element(app, "settings.importExport.progress").exists
                    && settingsContent.exists && export.exists && export.isEnabled
            }, object: nil)
            return XCTWaiter().wait(for: [closed], timeout: 30) == .completed
        }
        func startExport() throws {
            #if os(iOS)
            try revealSettingsControl("settings.root.exportCollection", app)
            #endif
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                settingsContent.exists && export.exists && export.isEnabled && export.isHittable
            }, object: nil)
            XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed, app.debugDescription)
            try click(export, app, bundle)
        }
        try click(export, app, bundle)
        for removed in ["prepare", "choose", "discard", "ready", "explanation"] {
            XCTAssertFalse(element(app, "settings.importExport.\(removed)").exists, removed)
        }
        capturePicker(app)
        #if os(macOS)
        let savePanel = app.sheets.matching(identifier: "save-panel").firstMatch
        XCTAssertTrue(savePanel.waitForExistence(timeout: 10))
        try click(savePanel.buttons.matching(NSPredicate(format: "identifier == %@ OR title == %@", "CancelButton", "Cancel")).firstMatch, app, bundle)
        let folder = try XCTUnwrap(destinationFolder)
        let saved = folder.appending(path: destinationFilename ?? "Meguri-Collection.colpkg")
        #else
        try cancelNativeExportPicker(app, bundle)
        // The native local folder is installed by the existing public source hook.
        let root = URL(fileURLWithPath: "/RF2 Import Tests")
        let folder = destinationFilename.map { root.appending(path: String($0.dropLast(".colpkg".count))) } ?? root
        let saved = folder.appending(path: "Meguri-Collection.colpkg")
        #endif
        guard exportClosed() else {
            capturePicker(app)
            XCTFail("Cancelling the save dialog must discard the export and return to Settings: \(app.debugDescription)")
            throw NSError(domain: "CollectionEntryJourneyTests", code: 2)
        }
        for replacement in [false, true] {
            try startExport()
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
            if !exportClosed() { capturePicker(app) }
            XCTAssertFalse(element(app, "settings.importExport.retryCleanup").exists)
            XCTAssertFalse(exportContent.exists, "The native destination and export owner must finish and close: \(app.debugDescription)")
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
        enum NameAdmissionStage { case idle, context, roots, editors, editorExists, editorEnabled, editorHittable, ready }
        var admission = (stage: NameAdmissionStage.idle, providers: 0, roots: 0,
                         directBars: 0, genericBars: 0, actions: 0, editors: 0,
                         enabled: false, hittable: false)
        func actionable(_ target: XCUIElement) -> Bool {
            target.exists && target.isEnabled && target.isHittable
        }
        func waitForState(_ message: String, _ condition: @escaping () -> Bool) throws {
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
            guard XCTWaiter().wait(for: [ready], timeout: 10) == .completed else {
                capturePicker(app)
                let diagnostic = "stage=\(admission.stage) providers=\(admission.providers) roots=\(admission.roots) directBars=\(admission.directBars) genericBars=\(admission.genericBars) actions=\(admission.actions) editors=\(admission.editors) enabled=\(admission.enabled) hittable=\(admission.hittable)"
                XCTFail("\(message); \(diagnostic.prefix(512))")
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
            let browserBar = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].firstMatch
            let folderBar = browserBar.exists ? browserBar : app.navigationBars["RF2 Import Tests"].firstMatch
            let creationName = NSPredicate(
                format: "label ==[c] %@ OR label ==[c] %@", "New Folder", "Create New Folder")
            let creationButtons = folderBar.buttons.matching(creationName)
            func captureCreationFailure(_ reason: String) {
                capturePicker(app)
                let buttons = folderBar.buttons.allElementsBoundByIndex
                let menuControls = app.buttons.matching(creationName).allElementsBoundByIndex
                    + app.menuItems.allElementsBoundByIndex
                let metadata = (Array(buttons.prefix(12)) + Array(menuControls.prefix(12))).map { control in
                    "type=\(control.elementType) label=\(String(control.label.prefix(120)).debugDescription) identifier=\(String(control.identifier.prefix(120)).debugDescription) isEnabled=\(control.isEnabled) isHittable=\(control.isHittable) frame=\(control.frame)"
                }
                let diagnostic = XCTAttachment(string: ([
                    reason, "Native toolbar buttons=\(buttons.count); menu controls=\(menuControls.count)"
                ] + metadata).joined(separator: "\n"))
                diagnostic.name = "export-new-folder-loaded-toolbar-controls"
                diagnostic.lifetime = .keepAlways
                add(diagnostic)
            }
            var candidates = creationButtons.allElementsBoundByIndex.filter { actionable($0) }
            if candidates.isEmpty {
                // The observed Files More control is separate from the folder title's info menu.
                let moreButtons = folderBar.buttons.matching(NSPredicate(
                    format: "identifier == %@ AND label ==[c] %@", "OverflowBarButtonItem", "More"))
                let moreCandidates = moreButtons.allElementsBoundByIndex
                guard moreCandidates.count == 1, let more = moreCandidates.first, actionable(more) else {
                    captureCreationFailure("The loaded native toolbar must offer one enabled, hittable More button")
                    XCTFail("The native folder creation menu requires the observed unique More control")
                    throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
                }
                try click(more, app, bundle)
                let offered = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                    candidates = (app.buttons.matching(creationName).allElementsBoundByIndex
                        + app.menuItems.matching(creationName).allElementsBoundByIndex).filter { actionable($0) }
                    return candidates.count == 1
                }, object: nil)
                guard XCTWaiter().wait(for: [offered], timeout: 10) == .completed else {
                    captureCreationFailure("Native More menu actionable creation candidates=\(candidates.count)")
                    XCTFail("The visible native More menu must offer exactly one enabled, hittable New Folder or Create New Folder button or item")
                    throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
                }
            }
            guard candidates.count == 1, let newFolder = candidates.first, actionable(newFolder) else {
                captureCreationFailure("Actionable creation candidates=\(candidates.count)")
                XCTFail("Native folder creation requires exactly one enabled, hittable New Folder or Create New Folder control")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
            try click(newFolder, app, bundle)
            if UIDevice.current.userInterfaceIdiom == .pad {
                let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
                screenshot.name = "native-ipad-new-folder"
                screenshot.lifetime = .keepAlways
                add(screenshot)
                func attachState(_ label: String, _ fact: String) {
                    let attachment = XCTAttachment(string: fact)
                    attachment.name = "native-ipad-new-folder-state-\(label)"
                    attachment.lifetime = .keepAlways
                    add(attachment)
                }
                let roots = app.descendants(matching: .any).matching(identifier:
                    "DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: RF2 Import Tests")
                attachState("root-count", "Exact local Files root identifier count=\(roots.count)")
                let editors = app.descendants(matching: .any).matching(identifier: "DOC.inlineRenameField")
                let editorCount = editors.count
                attachState("editor-count", "Exact DOC.inlineRenameField count=\(editorCount)")
                attachState("keyboard", "Software keyboard exists=\(app.keyboards.firstMatch.exists)")
                for index in 0..<min(editorCount, 2) {
                    let editor = editors.element(boundBy: index)
                    attachState("editor-\(index)-attributes",
                        "role=\(editor.elementType) identifier=\(editor.identifier.debugDescription) value=\(String(describing: editor.value).prefix(256)) frame=\(editor.frame)")
                    attachState("editor-\(index)-enabled", "isEnabled=\(editor.isEnabled)")
                    attachState("editor-\(index)-hittable", "isHittable=\(editor.isHittable)")
                    attachState("editor-\(index)-description", String(editor.debugDescription.prefix(2000)))
                }
                attachState("alerts", "Alert count=\(app.alerts.count)")
                attachState("sheets", "Sheet count=\(app.sheets.count)")
            }
            let commitName = NSPredicate(format: "label IN %@", ["Create", "Done"])
            func nameDialogs() -> [XCUIElement] {
                (app.alerts.allElementsBoundByIndex + app.sheets.allElementsBoundByIndex).filter { dialog in
                    dialog.exists && dialog.textFields.count == 1
                        && (dialog.label == "New Folder" || dialog.staticTexts["New Folder"].exists
                            || dialog.navigationBars["New Folder"].exists)
                        && dialog.buttons.matching(commitName).count == 1
                }
            }
            func inlineNameTextView() -> XCUIElement? {
                admission = (.context, 0, 0, 0, 0, 0, 0, false, false)
                guard self.nativeExportFolderIsOpen("RF2 Import Tests", app),
                      !app.alerts.firstMatch.exists,
                      nameDialogs().isEmpty else { return nil }
                let providers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@",
                    "DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: "))
                    .allElementsBoundByIndex
                admission.stage = .roots
                admission.providers = providers.count
                let roots = providers.filter { root in
                        guard root.exists else { return false }
                        let folderBars = root.navigationBars.matching(identifier: "RF2 Import Tests")
                            .allElementsBoundByIndex
                        let browserBars = root.navigationBars.matching(identifier: "FullDocumentManagerViewControllerNavigationBar")
                            .allElementsBoundByIndex
                        admission.directBars += folderBars.count
                        admission.genericBars += browserBars.count
                        guard folderBars.count <= 1, browserBars.count <= 1 else { return false }
                        let actions = browserBars.first?.buttons
                            .matching(NSPredicate(format: "label == %@", "RF2 Import Tests, Actions Menu"))
                            .allElementsBoundByIndex ?? []
                        admission.actions += actions.count
                        guard actions.count <= 1 else { return false }
                        return (folderBars.count == 1 && folderBars[0].exists)
                            || (browserBars.count == 1 && browserBars[0].exists
                                && actions.count == 1 && actions[0].exists)
                    }
                admission.roots = roots.count
                guard roots.count == 1, let root = roots.first, root.exists else { return nil }
                admission.stage = .editors
                let textViews = root.descendants(matching: .textView).matching(identifier: "DOC.inlineRenameField")
                    .allElementsBoundByIndex
                admission.editors = textViews.count
                guard textViews.count == 1, let textView = textViews.first else { return nil }
                admission.stage = .editorExists
                guard textView.exists else { return nil }
                admission.stage = .editorEnabled
                admission.enabled = textView.isEnabled
                guard admission.enabled else { return nil }
                admission.stage = .editorHittable
                admission.hittable = textView.isHittable
                guard admission.hittable else { return nil }
                admission.stage = .ready
                return textView
            }
            try waitForState("New Folder must offer one known name dialog or the unique inline Files name editor", {
                let dialogs = nameDialogs()
                return (dialogs.count == 1 && actionable(dialogs[0].textFields.firstMatch))
                    || (dialogs.isEmpty && inlineNameTextView() != nil)
            })
            let dialogs = nameDialogs()
            if dialogs.count == 1, let dialog = dialogs.first,
               actionable(dialog.textFields.firstMatch) {
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
            } else if dialogs.isEmpty, let textView = inlineNameTextView() {
                // Native New Folder has already focused and selected the entire inline name.
                textView.typeText(stem)
                try waitForState("The inline Files name must equal the exact safe nonce stem before Return", {
                    inlineNameTextView() != nil && textView.value as? String == stem
                })
                textView.typeText("\n")
                try waitForState("Return must finish inline naming and reveal the exact native child", {
                    !textView.exists && (self.nativeExportFolderIsOpen(stem, app)
                        || (self.nativeExportFolderIsOpen("RF2 Import Tests", app) && childTarget() != nil))
                })
            } else {
                capturePicker(app)
                XCTFail("The loaded native new-folder naming mode must remain unique and actionable")
                throw NSError(domain: "CollectionEntryJourneyTests", code: 5)
            }
        }
        if !nativeExportFolderIsOpen(stem, app) {
            try waitForState("RF2 Import Tests must offer the exact existing nonce child", {
                self.nativeExportFolderIsOpen("RF2 Import Tests", app) && childTarget() != nil
            })
            guard nativeExportFolderIsOpen("RF2 Import Tests", app), let target = childTarget() else {
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
        func actionable(_ target: XCUIElement) -> Bool {
            target.exists && target.isEnabled && target.isHittable
        }
        func actionableCancel() -> XCUIElement? {
            if actionable(cancelButton) { return cancelButton }
            if actionable(cancelMenuItem) { return cancelMenuItem }
            return actionable(cancelOther) ? cancelOther : nil
        }
        func completedRoot() -> Bool {
            !back.exists && actionable(more)
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
                "More: \(attributes(more))"]
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
            #if os(macOS)
            let localDeck = deckRows(app, identifier: "home.deck.row.1884308400001")
            #else
            let localDeck = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.row."))
            #endif
            try waitForAdmission(localDeck, app, bundle)
        }
        let entry = element(app, "home.collection.import")
        XCTAssertTrue(entry.waitForExistence(timeout: 20))
        // N17.2: the system picker opens directly; the one sheet follows the choice.
        try click(entry, app, bundle)
        XCTAssertFalse(element(app, "onboarding.collectionImport.content").exists)
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
        // Real activation dismisses entry; nothing asks after it (N18.1).
        XCTAssertEqual(XCTWaiter().wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: entryContent)], timeout: 30), .completed)
        let importedDeck = deckRows(app, identifier: "home.deck.row.1788249600212")
        try waitForAdmission(importedDeck, app, bundle)
        XCTAssertFalse(element(app, "onboarding.collectionImport.error").exists)
        XCTAssertFalse(element(app, "home.collection.error").exists)
        let deck = try XCTUnwrap(waitForHittable(importedDeck), app.debugDescription)
        let deckIdentifier = deck.identifier
        let deckID = try XCTUnwrap(HomeControls.deckID(deckIdentifier).flatMap { Int64($0) })
        let deckPage = element(app, "home.deck.page.\(deckID)")
        capture(app, "entry-real-decks")
        try click(deck, app, bundle)
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        let study = deckPage.buttons["home.deck.study"].firstMatch
        try waitForStudyReady(study, app)
        let before = try XCTUnwrap(HomeControls.deckCounts(app), app.debugDescription)
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
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        let after = try XCTUnwrap(HomeControls.deckCounts(app), app.debugDescription)
        capture(app, "entry-saved-answer")
        app.terminate()
        // Reopen the installed collection without fixture/source/route setup.
        app.launchArguments = MeguriLaunch(now: "2030-01-15T10:00:00Z", timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        XCTAssertTrue(waitForText(element(app, "home.session.recorded"), "1 answer recorded"))
        #if os(macOS)
        let retainedDeck = deckRows(app, identifier: deckIdentifier).element
        #else
        let retainedRows = deckRows(app, identifier: deckIdentifier)
        var retainedDeck: XCUIElement { HomeControls.hittable(retainedRows) ?? retainedRows.firstMatch }
        #endif
        let restartReady = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            (deckPage.exists && study.isHittable) || (retainedDeck.exists && retainedDeck.isHittable)
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [restartReady], timeout: 20), .completed)
        if !(deckPage.exists && study.isHittable) {
            try click(retainedDeck, app, bundle)
        }
        XCTAssertTrue(deckPage.waitForExistence(timeout: 10))
        XCTAssertTrue(HomeControls.waitForDeckCounts(app, new: after[0], learning: after[1], review: after[2]),
            "The reopened deck page must show the counts saved before termination: \(after)")
        XCTAssertFalse(element(app, "home.collection.error").exists)
        capture(app, "entry-restart-durable-answer")
        XCTAssertEqual(before.count, 3, "The deck page must have shown its three counts before Study")
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
        // ImportSource publishes every admitted package under this fixed Files name.
        let labels = ["review", "review.colpkg"]
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
    private func deckRows(_ app: XCUIApplication, identifier: String) -> XCUIElementQuery {
        // N14: the Mac's only deck outline is the sidebar's; its rows are
        // `home.sidebar.deck.<id>`, as static text or a button.
        let sidebar = identifier.replacingOccurrences(of: HomeControls.rowPrefix, with: HomeControls.sidebarPrefix)
        #if os(macOS)
        return app.outlines.outlineRows.descendants(matching: .any).matching(NSPredicate(
            format: "identifier == %@ AND elementType IN %@", sidebar,
            [XCUIElement.ElementType.staticText.rawValue, XCUIElement.ElementType.button.rawValue]))
        #else
        // The tab layout's deck list row, or the iPad sidebar's row where the detail shows Today.
        return app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@", [identifier, sidebar]))
        #endif
    }
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
        // N17.2: one confirmation: the file's name, its counts, one consequence line.
        XCTAssertTrue(waitForText(element(app, "onboarding.collectionImport.preview.heading"), "Import “review.colpkg”?"))
        let counts = element(app, "onboarding.collectionImport.preview.counts")
        XCTAssertTrue(waitForText(counts, "6 reviews"))
        XCTAssertEqual(counts.label, "6 notes · 11 cards · 0 media files")
        let safety = element(app, "onboarding.collectionImport.preview.safety")
        if hasLocalCollection {
            #if os(macOS)
            let device = "Mac"
            #else
            let device = UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
            #endif
            XCTAssertTrue(waitForText(safety, "This replaces the collection on this \(device). Meguri keeps a backup of the current one."))
        } else {
            XCTAssertFalse(safety.exists, "Nothing is replaced, so no consequence line shows")
        }
        XCTAssertEqual(element(app, "onboarding.collectionImport.confirm").label, "Import")
        for removed in ["preview.destination", "preview.media.files", "preview.media.bytes", "preview.media.retention", "explanation", "choose"] {
            XCTAssertFalse(element(app, "onboarding.collectionImport.\(removed)").exists, removed)
        }
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
        // The collection is presented as soon as it is open: no sheet, no prompt (N18.1).
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            #if os(macOS)
            guard query.count == 1 else { return false }
            #endif
            return query.allElementsBoundByIndex.contains { $0.exists && $0.isHittable }
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 30), .completed, app.debugDescription)
        XCTAssertEqual(app.sheets.count, 0, "Nothing asks after a collection opens")
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
