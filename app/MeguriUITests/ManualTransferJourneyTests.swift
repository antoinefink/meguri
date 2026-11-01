import Foundation
import XCTest
#if os(macOS)
import AppKit
#endif

/// VM-only, through tools/mg. The controlled rig is seeded with generated
/// reviewable content; no fixture installs its collection into the app.
/// Integrator stages controlled-rig.json through its controlled forwarding job.
@MainActor final class ManualTransferJourneyTests: XCTestCase {
    private var confirmDiagnosticTarget: XCUIElement?
    private var prepareGeometry: (id: String, lines: [String])?
    private struct Rig: Decodable {
        let endpoint: String
        let username: String
        let password: String
        let exportUsername: String
        let exportPassword: String
        let deckID: Int64
        let serverImportSHA256: String
        let serverExportSHA256: String
        let deviceSHA256: String
        let wireToken: String
        let nativeContext: String
    }
    #if os(macOS)
    func testNativeTransferSheetIdentityPreflight() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires Debug/test hooks") }
        let bundle = try build.bundleIdentifier
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false, extra: ["-ShowAnkiServerInterface", "YES"]).arguments
        addTeardownBlock { @MainActor in app.terminate() }
        app.launch()

        try selectImport(app, bundle)
        XCTAssertEqual(app.sheets.count, 1, "Import must open exactly one app-owned native sheet.")
        let sheets = app.sheets.containing(.any, identifier: "settings.anki.transferWorkflow")
        XCTAssertEqual(sheets.count, 1, "The workflow root must belong to exactly one native sheet.")
        let sheet = sheets.firstMatch
        XCTAssertEqual(sheet.descendants(matching: .any)
            .matching(identifier: "settings.anki.transferWorkflow").count, 1,
            "The native sheet must expose exactly one workflow root.")
        XCTAssertEqual(sheet.buttons.matching(identifier: "settings.anki.closeTransfer").count, 1,
                       "The native sheet must expose its Done control.")

        let prepareButtons = sheet.buttons.matching(identifier: "settings.anki.prepareImport")
        XCTAssertEqual(prepareButtons.count, 1, "The native sheet must expose exactly one Prepare Import control.")
        let prepare = prepareButtons.firstMatch
        try reveal(prepare, app, bundle)
        let workflow = sheet.descendants(matching: .any)
            .matching(identifier: "settings.anki.transferWorkflow").firstMatch
        var owners = [workflow.scrollViews, workflow.tables, workflow.collectionViews].flatMap {
            $0.containing(.any, identifier: "settings.anki.prepareImport").allElementsBoundByIndex
        }
        if workflow.exists, [.scrollView, .table, .collectionView].contains(workflow.elementType),
           workflow.descendants(matching: .any).matching(identifier: "settings.anki.prepareImport").firstMatch.exists {
            owners.append(workflow)
        }
        XCTAssertFalse(owners.isEmpty, "Prepare Import must have an identified owning scroll viewport.")
        let window = app.windows.containing(.any, identifier: "settings.anki.transferWorkflow").firstMatch
        XCTAssertTrue(owners.contains { owner in
            guard prepare.exists, owner.exists, window.exists else { return false }
            let frame = prepare.frame, viewport = owner.frame.intersection(window.frame)
            guard [frame, viewport].allSatisfy({
                !$0.isNull && !$0.isEmpty && [$0.origin.x, $0.origin.y, $0.width, $0.height].allSatisfy { $0.isFinite }
            }) else { return false }
            let center = CGPoint(x: frame.midX, y: frame.midY)
            return center.x.isFinite && center.y.isFinite && viewport.contains(center)
        }, "Revealed Prepare Import center must lie inside its owning scroll/window viewport.")

        try closeTransfer(app, bundle)
        XCTAssertFalse(element(app, "settings.anki.transferWorkflow").exists,
                       "Done must remove the workflow root.")
        XCTAssertEqual(app.sheets.count, 0, "Done must dismiss the native transfer sheet.")
    }
    func testSettingsScrollCarrierPreflight() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires Debug/test hooks") }
        let bundle = try build.bundleIdentifier
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false, extra: ["-ShowAnkiServerInterface", "YES"]).arguments
        addTeardownBlock { @MainActor in app.terminate() }
        app.launch()

        try openSettings(app, bundle)
        let endpoint = element(app, "settings.anki.endpoint")
        XCTAssertTrue(endpoint.waitForExistence(timeout: 30), "Settings must expose the endpoint before reveal.")
        attachGeometry(app, "rf4-preflight-before-bringToFront", bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        attachGeometry(app, "rf4-preflight-after-bringToFront", bundle)

        // Traverse Connection's existing details disclosure at its default size.
        // Reveal only: refreshing history or editing the endpoint is unnecessary.
        try showHistory(app, bundle)
        let history = element(app, "settings.anki.history")
        try reveal(history, app, bundle)
        XCTAssertEqual(app.descendants(matching: .any).matching(identifier: "settings.anki.details").count, 1)
        XCTAssertEqual(app.descendants(matching: .any).matching(identifier: "settings.anki.history").count, 1)
        XCTAssertTrue(history.exists, "Settings must expose the Local History refresh control.")
        XCTAssertEqual(history.elementType, .button, "Local History refresh must remain a native button.")
        XCTAssertTrue(history.exists && history.isEnabled && history.isHittable,
                      "Local History refresh must be enabled and hittable after reveal.")
        XCTAssertTrue(history.exists && history.isHittable, "Local History refresh must be reachable by scrolling down.")
        attachGeometry(app, "rf4-preflight-history-revealed", bundle)
        try reveal(endpoint, app, bundle)
        XCTAssertTrue(endpoint.exists, "Settings must retain the endpoint control.")
        XCTAssertTrue(endpoint.exists && endpoint.isHittable, "The endpoint must be reachable by scrolling back up.")
        attachGeometry(app, "rf4-preflight-endpoint-revealed", bundle)
    }
    #endif
    func testSameLoginReviewCycleThenCrossAccountMediaRecovery() async throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else {
            throw XCTSkip("Requires Debug/test hooks and a controlled VM rig route")
        }
        let manifest = build.repositoryRoot.appending(path: "app/Fixtures/ManualTransfer/controlled-rig.json")
        guard FileManager.default.fileExists(atPath: manifest.path) else {
            throw XCTSkip("Queued and unaccepted: the VM needs a controlled loopback rig and a manifest for two distinct synthetic accounts")
        }
        let rig = try JSONDecoder().decode(Rig.self, from: Data(contentsOf: manifest))
        try requireControlledEndpoint(rig.endpoint)
        let importUsername = rig.username.trimmingCharacters(in: .whitespacesAndNewlines)
        let exportUsername = rig.exportUsername.trimmingCharacters(in: .whitespacesAndNewlines)
        for (username, password) in [(importUsername, rig.password), (exportUsername, rig.exportPassword)] {
            XCTAssertFalse(username.isEmpty)
            XCTAssertFalse(password.isEmpty)
        }
        XCTAssertTrue(importUsername != exportUsername, "A and B must use distinct accounts after trimming usernames.")

        let app = XCUIApplication()
        let bundle = try build.bundleIdentifier
        // The leased-VM controller stages only its reserved media files. A
        // MeguriFixture launch would replace Collection, including activation.
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false, extra: ["-ShowAnkiServerInterface", "YES"]).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        #if os(iOS)
        // Exactly once, after the initial real launch and before login/Import.
        // Relaunch/recovery paths only read the frozen bootstrap identity.
        try await bootstrapMedia(rig)
        #endif

        // Import A through the same Settings controls used outside tests.
        do {
            try openSettings(app, bundle)
            let savedLogin = element(app, "settings.anki.logout")
            if savedLogin.waitForExistence(timeout: 2) { try click(savedLogin, app, bundle) }
            let passiveLaunch = try await wire(rig, "snapshot")
            XCTAssertEqual(passiveLaunch.collection, 0)
            XCTAssertEqual(passiveLaunch.media, 0)
            XCTAssertEqual(passiveLaunch.collectionTransfers, 0)
            let address = element(app, "settings.anki.endpoint")
            try click(address, app, bundle)
            replaceText(address, app: app, with: rig.endpoint)
            #if os(iOS)
            try dismissTransferKeyboardIfPresent(app, bundle)
            #endif
            let endpointExactMatch = (address.value as? String) == rig.endpoint
            let endpointState = XCTAttachment(string: "endpointExactMatch=\(endpointExactMatch)")
            endpointState.name = "rf4-endpoint-entry-state"; endpointState.lifetime = .keepAlways; add(endpointState)
            XCTAssertTrue(endpointExactMatch, "Endpoint entry must match the intended draft.")
            try login(app, bundle, username: importUsername, password: rig.password)
            let startup = try await wire(rig, "mark/startup")
            XCTAssertEqual(startup.media, 0, "Passive launch and login must send zero media requests.")
            XCTAssertEqual(startup.collectionTransfers, 0, "Direct URL Login must not transfer a collection.")
            XCTAssertTrue((address.value as? String) == rig.endpoint, "Login must preserve the exact entered URL.")
            try selectImport(app, bundle)
            try selectMediaScope(app, bundle)
            try click(element(app, "settings.anki.prepareImport"), app, bundle)
            try reveal(element(app, "settings.anki.confirm"), app, bundle)
            waitUntilReady(element(app, "settings.anki.confirm"), app)
            let baseline = try durableOperation(try await nativeState(rig), direction: "Import").successCooldown
            try click(element(app, "settings.anki.confirm"), app, bundle)
            try confirmNative(app, bundle)
            try completeOptionalFsrsAdmission(app, bundle)
            try openSettings(app, bundle)
            _ = try await allowAndFinishMedia(app, bundle, rig: rig, stage: "Import", baseline: baseline, name: "rf4-account-a-import")
        }

        let imported = try durableOperation(try await nativeState(rig), direction: "Import")
        let afterImport = try await wire(rig, "snapshot")

        // Save an answer in the imported deck before restarting the app.
        try openHome(app, bundle)
        try completeOptionalFsrsAdmission(app, bundle)
        for id in ["home.deck.row.\(rig.deckID)", "home.deck.study", "review.session.reveal",
                   "review.session.rate.easy", "review.session.close"] {
            try click(element(app, id), app, bundle)
        }
        try waitForText(element(app, "home.session.recorded"), "1 answers recorded", app, bundle)
        attachSuccess(app, "rf4-account-a-import-real-review-saved")
        app.terminate()
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false, extra: ["-ShowAnkiServerInterface", "YES"]).arguments
        app.launch()
        try waitForText(element(app, "home.session.recorded"), "1 answers recorded", app, bundle)
        attachSuccess(app, "rf4-account-a-review-durable-after-restart")

        // Restore the persisted answer and A's stored login with no passive work.
        try openSettings(app, bundle)
        try waitForText(element(app, "settings.anki.loginIdentity"), importUsername, app, bundle)
        waitUntilReady(element(app, "settings.anki.logout"), app)
        XCTAssertFalse(element(app, "settings.anki.username").exists)
        XCTAssertFalse(element(app, "settings.anki.confirm").exists)
        XCTAssertTrue((element(app, "settings.anki.endpoint").value as? String) == rig.endpoint)
        let restored = try await wire(rig, "snapshot")
        XCTAssertEqual(restored.media, afterImport.media)
        XCTAssertEqual(restored.collection, afterImport.collection)
        XCTAssertEqual(restored.collectionTransfers, afterImport.collectionTransfers)
        try selectExport(app, bundle)
        try selectMediaScope(app, bundle)
        try click(element(app, "settings.anki.prepareExport"), app, bundle)
        try reveal(element(app, "settings.anki.confirm"), app, bundle)
        waitUntilReady(element(app, "settings.anki.confirm"), app)
        let aPrepared = try durableOperation(try await nativeState(rig), direction: "AExport")
        XCTAssertTrue(aPrepared.receipt == nil && aPrepared.safetyBackupCount > 0)
        XCTAssertTrue(aPrepared.id != imported.id && aPrepared.account == imported.account &&
                      aPrepared.configuredEndpoint == imported.configuredEndpoint,
                      "The fresh Export must use A's restored login at the same endpoint.")
        try click(element(app, "settings.anki.confirm"), app, bundle)
        attachSuccess(app, "rf4-account-a-export-fresh-backup-preview")
        try confirmNative(app, bundle)
        try completeOptionalFsrsAdmission(app, bundle)
        try openSettings(app, bundle)
        let aExported = try await allowAndFinishMedia(app, bundle, rig: rig, stage: "AExport",
            baseline: aPrepared.successCooldown, name: "rf4-account-a-export")
        let importReceipt = try XCTUnwrap(imported.receipt)
        let exportReceipt = try XCTUnwrap(aExported.receipt)
        let elapsed = exportReceipt.completedAt - importReceipt.completedAt
        XCTAssertTrue(elapsed >= 0 && elapsed < 3_600_000,
                      "A's real review cycle must finish before one hour without changing clocks or policy.")

        // Independent B coverage follows A's completed cycle. Arm only B's real payload.
        _ = try await wire(rig, "arm-export")
        try logout(app, bundle)
        try login(app, bundle, username: exportUsername, password: rig.exportPassword)
        try selectExport(app, bundle)
        try selectMediaScope(app, bundle)
        try click(element(app, "settings.anki.prepareExport"), app, bundle)
        try reveal(element(app, "settings.anki.confirm"), app, bundle)
        waitUntilReady(element(app, "settings.anki.confirm"), app)
        let bPrepared = try durableOperation(try await nativeState(rig), direction: "Export")
        XCTAssertTrue(bPrepared.receipt == nil && bPrepared.safetyBackupCount > 0 && bPrepared.account != imported.account &&
                      bPrepared.configuredEndpoint == imported.configuredEndpoint)
        try click(element(app, "settings.anki.confirm"), app, bundle)
        attachSuccess(app, "rf4-account-b-export-backup-preview")
        try confirmNative(app, bundle)
        try completeOptionalFsrsAdmission(app, bundle)
        try openSettings(app, bundle)
        _ = try await allowAndFinishMedia(app, bundle, rig: rig, stage: "Export", baseline: bPrepared.successCooldown, name: "rf4-account-b-export")
        let retained = try await nativeState(rig)
        assertSameOperation(imported, try durableOperation(retained, direction: "Import"))
        assertSameOperation(aExported, try durableOperation(retained, direction: "AExport"))
        attachSuccess(app, "rf4-account-b-export-complete-local-activation")

        // The rig independently downloads A and B and compares the saved review
        // and scheduling with replay. Native state alone cannot establish that.
    }
    private func selectImport(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try openSettings(app, bundle)
        try click(element(app, "settings.root.collection"), app, bundle)
        try click(element(app, "settings.anki.openImport"), app, bundle)
        let workflowExists = element(app, "settings.anki.transferWorkflow").waitForExistence(timeout: 30)
        if !workflowExists { attachTransferWorkflowFailureFacts(app) }
        XCTAssertTrue(workflowExists)
        #endif
    }
    private func selectExport(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try openSettings(app, bundle)
        try click(element(app, "settings.root.collection"), app, bundle)
        try click(element(app, "settings.anki.openExport"), app, bundle)
        let workflowExists = element(app, "settings.anki.transferWorkflow").waitForExistence(timeout: 30)
        if !workflowExists { attachTransferWorkflowFailureFacts(app) }
        XCTAssertTrue(workflowExists)
        #else
        try click(element(app, "settings.anki.direction"), app, bundle)
        try click(app.buttons["Export to Anki"].firstMatch, app, bundle)
        #endif
    }
    #if os(macOS)
    private func attachTransferWorkflowFailureFacts(_ app: XCUIApplication) {
        let knownIDs = ["settings.anki.transferWorkflow", "settings.anki.closeTransfer",
                        "settings.anki.prepareImport", "settings.anki.prepareExport"]
        func counts(in owner: XCUIElement) -> [String: Int] {
            let descendants = owner.descendants(matching: .any)
            return Dictionary(uniqueKeysWithValues: knownIDs.map { id in
                (id, descendants.matching(NSPredicate(format: "identifier == %@", id)).count)
            })
        }
        func component(_ value: CGFloat) -> [String: Any] {
            ["value": value.isFinite ? Double(value) : 0, "isFinite": value.isFinite]
        }
        let sheets = app.sheets
        let sheetCount = sheets.count
        var sheetFacts: [[String: Any]] = []
        // Bound only this app's sheets, and do not enumerate when over the cap.
        if sheetCount <= 4 {
            for (index, sheet) in sheets.allElementsBoundByAccessibilityElement.prefix(4).enumerated() {
                let exists = sheet.exists
                let frame = exists ? sheet.frame : .zero
                sheetFacts.append([
                    "index": index,
                    "exists": exists,
                    "frame": ["x": component(frame.origin.x), "y": component(frame.origin.y),
                              "width": component(frame.width), "height": component(frame.height)],
                    "exactIDCounts": exists ? counts(in: sheet)
                        : Dictionary(uniqueKeysWithValues: knownIDs.map { ($0, 0) })
                ])
            }
        }
        let facts: [String: Any] = [
            "appWindowCount": app.windows.count,
            "appSheetCount": sheetCount,
            "exactIDCounts": counts(in: app),
            "sheetsTruncated": sheetCount > 4,
            "sheets": sheetFacts
        ]
        if let data = try? JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]) {
            let report = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
            report.name = "transfer-workflow-failure-facts"
            report.lifetime = .keepAlways
            add(report)
        }
    }
    #endif
    private func selectMediaScope(_ app: XCUIApplication, _ bundle: String) throws {
        let included = element(app, "settings.anki.mediaIncluded")
        try waitForText(included, "Imports and exports include collection and media.", app, bundle)
    }
    private func allowAndFinishMedia(_ app: XCUIApplication, _ bundle: String, rig: Rig, stage: String,
                                     baseline: DurableOperation.Receipt?, name: String) async throws -> DurableOperation {
        try continueTransfer(app, bundle)
        let importing = stage == "Import"
        let cancelledStage = stage != "AExport"
        let prefix = importing ? "" : "export-"
        let direction = stage == "AExport" ? "AExport" : (importing ? "Import" : "Export")
        let expectedHeld = importing ? 1 : 2
        // Collection activation/adoption must drain before this separate, real
        // both-side decision becomes enabled. No launch hook grants consent.
        let allow = element(app, "settings.anki.startMedia")
        try reveal(allow, app, bundle)
        waitUntilReady(allow, app)
        XCTAssertFalse(element(app, "settings.anki.confirm").exists)
        XCTAssertFalse(element(app, "settings.anki.progress").exists)
        let initial = try await nativeState(rig)
        if importing {
            XCTAssertFalse(try XCTUnwrap(initial.files["rf4-server.bin"]).exists,
                           "The server file must not arrive before media consent.")
        } else {
            try assertMediaHash(initial, name: "rf4-server.bin", expected: rig.serverImportSHA256, bytes: 4096)
        }
        try assertMediaHash(initial, name: "rf4-device.bin", expected: rig.deviceSHA256, bytes: 6144)
        attachSuccess(app, name + "-collection-complete-media-awaiting-consent")
        let before = try await wire(rig, importing ? "mark/before-consent" : (stage == "AExport" ? "mark/a-export-before-consent" : "mark/export-before-consent"))
        XCTAssertEqual(before.active, 0)
        if importing { XCTAssertEqual(before.media, 0, "Preparing the media decision must be offline.") }
        let original = try durableOperation(initial, direction: direction)
        XCTAssertTrue(original.collectionComplete && original.receipt == nil && original.media.stage == "AwaitingConsent")
        XCTAssertTrue(original.media.consent == nil, "Each stage requires fresh explicit consent.")
        assertHistoricalCooldown(original, baseline: baseline)
        try click(allow, app, bundle)
        if cancelledStage {
            // The observer holds the first real media file request before it
            // reaches the owned rig. Cancellation cannot race loopback completion.
            let held = try await waitForWire(rig) { $0.gate == "holding" && $0.held == expectedHeld }
            XCTAssertGreaterThan(held.media, before.media)
            try click(element(app, "settings.anki.cancel"), app, bundle)
            try waitForText(element(app, "settings.anki.result"), "operation is partial", app, bundle)
            XCTAssertFalse(element(app, "settings.anki.progress").exists)

            let cancelled = try await nativeState(rig)
            let partial = try durableOperation(cancelled, direction: direction)
            XCTAssertEqual(partial.media.stage, "Cancelled")
            assertSameOperation(original, partial)
            XCTAssertEqual(partial.phase, "MediaIncomplete")
            XCTAssertTrue(partial.collectionComplete)
            XCTAssertTrue(partial.receipt == nil)
            XCTAssertTrue(partial.media.remoteOutcomeUnknown)
            XCTAssertEqual(partial.media.counters.downloadedFiles, 0)
            XCTAssertEqual(partial.media.counters.uploadedFiles, 0)
            XCTAssertEqual(partial.media.counters.bytesDownloaded, 0)
            XCTAssertEqual(partial.media.counters.bytesUploaded, 0)
            assertHistoricalCooldown(partial, baseline: baseline)
            if importing { XCTAssertFalse(try XCTUnwrap(cancelled.files["rf4-server.bin"]).exists) }
            else { try assertMediaHash(cancelled, name: "rf4-server.bin", expected: rig.serverImportSHA256, bytes: 4096) }
            try assertMediaHash(cancelled, name: "rf4-device.bin", expected: rig.deviceSHA256, bytes: 6144)
            _ = try await wire(rig, "abort")
            _ = try await waitForWire(rig) { $0.active == 0 && $0.aborted == expectedHeld }
            let stopped = try await wire(rig, "mark/\(prefix)cancelled")
            XCTAssertEqual(stopped.collectionTransfers, before.collectionTransfers)
            try reveal(element(app, "settings.anki.history"), app, bundle)
            waitUntilReady(element(app, "settings.anki.history"), app)
            attachSuccess(app, name + "-native-cancel-drained-durable-partial-history-preserved")

            app.terminate()
            app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false, extra: ["-ShowAnkiServerInterface", "YES"]).arguments
            app.launch()
            try completeOptionalFsrsAdmission(app, bundle)
            try openSettings(app, bundle)
            try waitForText(element(app, "settings.anki.loginIdentity"), importing ? rig.username : rig.exportUsername, app, bundle)
            try showHistory(app, bundle)
            try click(element(app, "settings.anki.history"), app, bundle)
            waitUntilReady(element(app, "settings.anki.history"), app)
            XCTAssertFalse(element(app, "settings.anki.startMedia").exists)
            XCTAssertFalse(element(app, "settings.anki.progress").exists)
            let recovered = try durableOperation(try await nativeState(rig), direction: direction)
            assertSameOperation(partial, recovered)
            XCTAssertTrue(recovered.collectionComplete && recovered.receipt == nil)
            XCTAssertEqual(recovered.media.stage, "Cancelled")
            assertHistoricalCooldown(recovered, baseline: baseline)
            let passive = try await wire(rig, "mark/\(prefix)passive-history")
            XCTAssertEqual(passive.media, stopped.media, "Restart and offline history must issue zero additional media requests.")
            XCTAssertEqual(passive.collectionTransfers, stopped.collectionTransfers)
            XCTAssertEqual(passive.collection, stopped.collection)
            attachSuccess(app, name + "-passive-restart-no-media-requests")

            try click(element(app, "settings.anki.resumeMedia.\(original.id)"), app, bundle)
            try continueTransfer(app, bundle)
            try reveal(allow, app, bundle)
            waitUntilReady(allow, app)
            let decision = try durableOperation(try await nativeState(rig), direction: direction)
            assertSameOperation(original, decision)
            XCTAssertEqual(decision.media.stage, "AwaitingConsent")
            XCTAssertTrue(decision.media.consent == nil && decision.receipt == nil)
            assertHistoricalCooldown(decision, baseline: baseline)
            let prepared = try await wire(rig, "mark/\(prefix)resume-decision")
            XCTAssertEqual(prepared.media, passive.media, "Reviewing the saved media decision must remain offline.")
            XCTAssertEqual(prepared.collection, passive.collection)
            _ = try await wire(rig, "release")
            try click(allow, app, bundle) // fresh explicit both-side consent
        }
        try waitForText(element(app, "settings.anki.result"), "Collection and supported media complete", app, bundle)
        try reveal(element(app, "settings.anki.history"), app, bundle)
        waitUntilReady(element(app, "settings.anki.history"), app)
        XCTAssertFalse(element(app, "settings.anki.startMedia").exists)
        let final = try await nativeState(rig)
        try assertMediaHash(final, name: "rf4-server.bin", expected: stage == "Export" ? rig.serverExportSHA256 : rig.serverImportSHA256, bytes: stage == "Export" ? 8192 : 4096)
        try assertMediaHash(final, name: "rf4-device.bin", expected: rig.deviceSHA256, bytes: 6144)
        try assertMediaHash(final, name: "rf4-zero.bin", expected: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", bytes: 0)
        let finished = try durableOperation(final, direction: direction)
        assertSameOperation(original, finished)
        XCTAssertTrue(finished.phase == "Complete" && finished.media.stage == "Complete" && finished.receipt != nil)
        XCTAssertTrue(finished.media.consentBound && finished.media.exclusionsVerified)
        let ownReceipt = try XCTUnwrap(finished.receipt)
        XCTAssertEqual(ownReceipt.nextEligibleAt, ownReceipt.completedAt,
                       "A custom-server completion is immediately eligible.")
        XCTAssertEqual(finished.media.remoteOutcomeUnknown, cancelledStage,
                       "Completion must retain unknown effects of the cancelled attempt.")
        let counts = finished.media.counters
        XCTAssertEqual(counts.downloadedFiles, stage == "AExport" ? 0 : 1)
        XCTAssertEqual(counts.uploadedFiles, stage == "AExport" ? 0 : 1)
        XCTAssertEqual(counts.bytesDownloaded, stage == "AExport" ? 0 : (importing ? 4096 : 8192))
        XCTAssertEqual(counts.bytesUploaded, stage == "AExport" ? 0 : 6144)
        XCTAssertEqual(counts.downloadedDeletions, 0)
        XCTAssertEqual(counts.uploadedDeletions, 0)
        XCTAssertEqual(counts.awaiting, 0)
        XCTAssertTrue(counts.empty == ["rf4-zero.bin"] && counts.oversized.isEmpty)
        _ = try await waitForWire(rig) { $0.active == 0 }
        let completed = try await wire(rig, importing ? "mark/resumed" : (stage == "AExport" ? "mark/a-exported" : "mark/exported"))
        XCTAssertEqual(completed.collectionTransfers, before.collectionTransfers, "Media Resume must send zero collection uploads/downloads.")
        XCTAssertEqual(completed.collection, before.collection, "Media must never replace or resend the collection.")
        // Durable typed counters and actual app/server SHA256 values are also
        // checked by manual-transfer-data; this text alone cannot pass RF4.
        attachSuccess(app, name + "-supported-media-complete")
        return finished
    }

    private struct WireCounts: Decodable {
        let media, collection, collectionTransfers, active, held, aborted, errors: Int
        let gate: String
    }
    private enum EvidenceStage: String, Decodable {
        case resolver, context, binding, marker, media, read, schema, subprocess
        case transport, decoder, timeout, cancelled, stopped, busy, output, cleanup
    }
    private enum EvidenceError: Error, CustomStringConvertible {
        case unavailable
        case rpc(EvidenceStage, status: Int? = nil, childRC: Int? = nil,
                 timeoutMS: Int? = nil, scriptLine: Int? = nil)
        var description: String {
            switch self {
            case .unavailable: return "Controlled evidence unavailable"
            case let .rpc(stage, status, childRC, timeoutMS, scriptLine):
                return "Controlled evidence stage=\(stage.rawValue) status=\(status ?? 0) childRC=\(childRC ?? 0) timeoutMS=\(timeoutMS ?? 0) scriptLine=\(scriptLine ?? 0)"
            }
        }
    }
    private struct EvidenceFailure: Decodable {
        struct Detail: Decodable {
            let stage: EvidenceStage
            let child_rc, http_status, timeout_ms, script_line: Int?
        }
        let failure: Detail
    }
    private func safeFailure(_ data: Data, status: Int) -> EvidenceError {
        // Reject unknown keys/codes and out-of-range numbers; response text and
        // Foundation error descriptions never reach test output.
        guard data.count <= 2048,
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(root.keys) == Set(["failure"]),
              let detail = root["failure"] as? [String: Any],
              Set(detail.keys) == Set(["stage", "child_rc", "http_status", "timeout_ms", "script_line"]),
              let decoded = try? JSONDecoder().decode(EvidenceFailure.self, from: data) else {
            return .rpc(.schema, status: status)
        }
        for key in ["child_rc", "http_status", "timeout_ms", "script_line"] {
            if let raw = detail[key], !(raw is NSNull) {
                guard let number = raw as? NSNumber,
                      ["s", "i", "l", "q", "S", "I", "L", "Q"].contains(String(cString: number.objCType)) else {
                    return .rpc(.schema, status: status)
                }
            }
        }
        let value = decoded.failure
        func bounded(_ number: Int?, _ range: ClosedRange<Int>) -> Bool {
            number.map { range.contains($0) } ?? true
        }
        guard bounded(value.child_rc, -255...255), bounded(value.http_status, 100...599),
              bounded(value.timeout_ms, 0...120000), bounded(value.script_line, 1...100000),
              value.http_status == status else { return .rpc(.schema, status: status) }
        return .rpc(value.stage, status: status, childRC: value.child_rc,
                    timeoutMS: value.timeout_ms, scriptLine: value.script_line)
    }
    private func evidenceRPC(_ rig: Rig, _ action: String, limit: Int, timeout: TimeInterval) async throws -> Data {
        guard let url = URL(string: rig.endpoint + "__rf4/" + action) else {
            throw EvidenceError.rpc(.transport)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = Data()
        request.timeoutInterval = timeout
        request.setValue(rig.wireToken, forHTTPHeaderField: "X-RF4-Control")
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let status = (response as? HTTPURLResponse)?.statusCode, (100...599).contains(status) else {
                throw EvidenceError.rpc(.transport)
            }
            guard status == 200 else { throw safeFailure(data, status: status) }
            guard data.count <= limit else { throw EvidenceError.rpc(.output, status: status) }
            return data
        } catch let error as EvidenceError { throw error }
        catch is CancellationError { throw EvidenceError.rpc(.cancelled) }
        catch let error as URLError {
            throw EvidenceError.rpc(error.code == .timedOut ? .timeout :
                                    error.code == .cancelled ? .cancelled : .transport)
        } catch { throw EvidenceError.rpc(.transport) }
    }
    private func wire(_ rig: Rig, _ action: String) async throws -> WireCounts {
        let data = try await evidenceRPC(rig, action, limit: 16384, timeout: 5)
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let counts = try? decoder.decode(WireCounts.self, from: data) else {
            throw EvidenceError.rpc(.decoder, status: 200)
        }
        XCTAssertEqual(counts.errors, 0, "Fixture forwarding must remain error-free.")
        return counts
    }
    private func waitForWire(_ rig: Rig, until predicate: (WireCounts) -> Bool) async throws -> WireCounts {
        let end = ProcessInfo.processInfo.systemUptime + 30
        // Poll actual observed requests; no delay is used to make UI beat a transfer.
        while ProcessInfo.processInfo.systemUptime < end {
            let counts = try await wire(rig, "snapshot")
            if predicate(counts) { return counts }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw EvidenceError.unavailable
    }
    private struct DurableOperation: Decodable {
        struct Receipt: Decodable, Equatable { let completedAt, nextEligibleAt: Int64 }
        struct Media: Decodable {
            struct Counters: Decodable {
                let downloadedFiles, uploadedFiles, downloadedDeletions, uploadedDeletions: UInt64
                let bytesDownloaded, bytesUploaded, awaiting: UInt64
                let empty, oversized: [String]
            }
            struct Consent: Decodable { let categories: UInt32 }
            let stage: String
            let remoteOutcomeUnknown, consentBound, exclusionsVerified: Bool
            let counters: Counters
            let consent: Consent?
        }
        let id, direction, phase, authority, account, configuredEndpoint: String
        let collectionComplete: Bool
        let safetyBackupCount: UInt64
        let resultingPublication: String?
        let receipt, successCooldown: Receipt?
        let media: Media
    }
    private struct NativeState: Decodable {
        struct FileFact: Decodable {
            let exists: Bool
            let bytes: UInt64?
            let sha256: String?
        }
        let context: String
        let operations: [String: DurableOperation?]
        let files: [String: FileFact]
    }
    private struct BootstrapProof: Decodable, Equatable {
        let authority, context, marker, namespace, binding: String
    }
    private struct MobileBootstrap: Decodable {
        let selector: String
        let bootstrap: BootstrapProof
    }
    private struct MobileNativeState: Decodable {
        let selector: String
        let bootstrap: BootstrapProof
        let state: NativeState
    }
    private var nativeBoundContext: (selector: String, proof: BootstrapProof)?
    private var bootstrapAttempted = false
    private func bootstrapMedia(_ rig: Rig) async throws {
        guard !bootstrapAttempted, nativeBoundContext == nil else { throw EvidenceError.rpc(.binding) }
        bootstrapAttempted = true
        let data = try await evidenceRPC(rig, "bootstrap", limit: 2048, timeout: 12)
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(root.keys) == Set(["selector", "bootstrap"]),
              let raw = root["bootstrap"] as? [String: Any],
              Set(raw.keys) == Set(["authority", "context", "marker", "namespace", "binding"]),
              let response = try? JSONDecoder().decode(MobileBootstrap.self, from: data),
              response.selector == rig.nativeContext,
              response.bootstrap.authority == "rf4-mobile-pretransfer-media-v1",
              [response.selector, response.bootstrap.context, response.bootstrap.marker,
               response.bootstrap.namespace, response.bootstrap.binding].allSatisfy({
                  $0.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil
              }) else { throw EvidenceError.rpc(.binding, status: 200) }
        nativeBoundContext = (selector: response.selector, proof: response.bootstrap)
    }
    private func nativeState(_ rig: Rig) async throws -> NativeState {
        let data = try await evidenceRPC(rig, "native-state", limit: 65536, timeout: 12)
        // The observer retains strict full-schema and frozen operation validation. Swift separately
        // binds both the run selector and the actual postinstall fingerprint.
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        #if os(macOS)
        guard let state = try? decoder.decode(NativeState.self, from: data) else {
            throw EvidenceError.rpc(.decoder, status: 200)
        }
        guard state.context == rig.nativeContext else { throw EvidenceError.rpc(.context, status: 200) }
        #else
        guard let response = try? decoder.decode(MobileNativeState.self, from: data) else {
            throw EvidenceError.rpc(.decoder, status: 200)
        }
        let state = response.state
        guard response.selector == rig.nativeContext,
              state.context.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil else {
            throw EvidenceError.rpc(.binding, status: 200)
        }
        guard let bound = nativeBoundContext, bound.selector == rig.nativeContext,
              response.bootstrap == bound.proof, state.context == bound.proof.context else {
            throw EvidenceError.rpc(.context, status: 200)
        }
        #endif
        return state
    }
    private func durableOperation(_ state: NativeState, direction: String) throws -> DurableOperation {
        guard let candidate = state.operations[direction], let record = candidate,
              record.direction == (direction == "AExport" ? "Export" : direction),
              record.id.range(of: "^[0-9a-f]{16}-[0-9a-f]{16}$", options: .regularExpression) != nil else {
            throw EvidenceError.unavailable
        }
        return record
    }
    private func assertSameOperation(_ first: DurableOperation, _ second: DurableOperation) {
        // Compare opaque identities only in memory; never attach the sidecar.
        XCTAssertTrue(first.id == second.id && first.authority == second.authority && first.account == second.account && first.configuredEndpoint == second.configuredEndpoint)
        XCTAssertTrue(first.resultingPublication != nil && first.resultingPublication == second.resultingPublication,
                      "Media cancellation/recovery/resume must retain this collection activation.")
    }
    private func assertHistoricalCooldown(_ operation: DurableOperation, baseline: DurableOperation.Receipt?) {
        XCTAssertTrue(operation.receipt == nil, "A partial operation must have no own receipt.")
        XCTAssertTrue(operation.successCooldown == baseline,
                      "Partial media must preserve historical per-account success state.")
    }
    private func assertMediaHash(_ state: NativeState, name: String, expected: String, bytes: UInt64) throws {
        let fact = try XCTUnwrap(state.files[name])
        XCTAssertTrue(fact.exists, "Reserved synthetic media must exist.")
        XCTAssertTrue(fact.bytes == bytes && fact.sha256 == expected,
                      "Actual reserved synthetic media bytes and SHA256 must match.")
    }
    private func requireControlledEndpoint(_ text: String) throws {
        let endpoint = try XCTUnwrap(URLComponents(string: text))
        let loopbackHosts: Set<String> = ["127.0.0.1", "::1", "localhost"]
        XCTAssertTrue(loopbackHosts.contains(endpoint.host ?? ""), "Only the controlled server's loopback route is allowed.")
        XCTAssertTrue(["http", "https"].contains(endpoint.scheme ?? ""))
        XCTAssertTrue(endpoint.user == nil && endpoint.password == nil, "Enter credentials in the production login fields.")
        XCTAssertTrue(endpoint.query == nil && endpoint.fragment == nil, "The server URL must have no query or fragment.")
    }
    private func login(_ app: XCUIApplication, _ bundle: String, username: String, password: String) throws {
        let userField = element(app, "settings.anki.username")
        try click(userField, app, bundle)
        replaceText(userField, app: app, with: username)
        #if os(iOS)
        try dismissTransferKeyboardIfPresent(app, bundle)
        #endif
        let usernameExactMatch = (userField.value as? String) == username
        let usernameState = XCTAttachment(string: "usernameExactMatch=\(usernameExactMatch)")
        usernameState.name = "rf4-username-entry-state"; usernameState.lifetime = .keepAlways; add(usernameState)
        XCTAssertTrue(usernameExactMatch, "Username entry must match the intended account.")
        let passwordField = element(app, "settings.anki.password")
        try click(passwordField, app, bundle)
        passwordField.typeText(password)
        #if os(iOS)
        try dismissTransferKeyboardIfPresent(app, bundle)
        #endif
        try click(element(app, "settings.anki.login"), app, bundle)
        try waitForText(element(app, "settings.anki.result"), "Logged in", app, bundle)
        try waitForText(element(app, "settings.anki.loginIdentity"), username, app, bundle)
        waitUntilReady(element(app, "settings.anki.logout"), app)
        XCTAssertFalse(element(app, "settings.anki.confirm").exists, "Login must not prepare or transfer.")
    }
    #if os(iOS)
    private func dismissTransferKeyboardIfPresent(_ app: XCUIApplication, _ bundle: String) throws {
        let keyboard = app.keyboards.firstMatch
        guard keyboard.exists else { return }
        let done = element(app, "settings.anki.keyboardDone")
        try click(done, app, bundle)
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !keyboard.exists }, object: nil)
        let outcome = XCTWaiter().wait(for: [dismissed], timeout: 30)
        if outcome != .completed { attach(app, "rf4-keyboard-dismissal-failure", target: done) }
        XCTAssertEqual(outcome, .completed, "Done must dismiss the transfer field keyboard before the next action.")
    }
    #endif
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        func tracked(_ target: XCUIElement) -> XCUIElement {
            if id == "settings.anki.confirm" { confirmDiagnosticTarget = target }
            return target
        }
        #if os(macOS)
        let workflow = app.descendants(matching: .any).matching(identifier: "settings.anki.transferWorkflow").firstMatch
        if id != "settings.anki.transferWorkflow", id.hasPrefix("settings.anki."), workflow.exists {
            // Native toolbar items can be siblings of the identified content.
            let sheet = app.sheets.containing(.any, identifier: "settings.anki.transferWorkflow").firstMatch
            let scope = sheet.exists ? sheet : workflow
            return tracked(scope.descendants(matching: .any).matching(identifier: id).firstMatch)
        }
        #endif
        return tracked(app.descendants(matching: .any).matching(identifier: id).firstMatch)
    }
    private func click(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        try reveal(target, app, bundle)
        waitUntilReady(target, app)
        #if os(macOS)
        let id = target.identifier
        if id == "settings.anki.prepareImport" || id == "settings.anki.prepareExport" {
            prepareGeometry = (id, prepareGeometryFacts(app, id, target))
        }
        target.click()
        #else
        target.tap()
        #endif
    }
    private func reveal(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        _ = target.waitForExistence(timeout: 30)
        #if os(macOS)
        let workflow = element(app, "settings.anki.transferWorkflow")
        let settingsRoot = element(app, "settings.root.content")
        let surface = workflow.exists ? workflow : settingsRoot
        let containers = surface.exists
            ? [surface.scrollViews, surface.tables, surface.collectionViews]
            : [app.scrollViews, app.tables, app.collectionViews]
        let window = surface.exists
            ? app.windows.containing(.any, identifier: surface.identifier).firstMatch
            : app.windows.firstMatch
        func canScrollSettingsRoot(_ root: XCUIElement) -> Bool {
            guard app.state == .runningForeground, (!app.sheets.firstMatch.exists || workflow.exists), !app.alerts.firstMatch.exists,
                  root.exists, root.isEnabled, root.isHittable else { return false }
            guard window.exists else { return false }
            // macOS can expose a null application frame even for an active window.
            let viewport = root.frame.intersection(window.frame)
            return !viewport.isNull && !viewport.isEmpty
                && viewport.origin.x.isFinite && viewport.origin.y.isFinite
                && viewport.width.isFinite && viewport.height.isFinite
        }
        var usesSettingsRoot = false
        #else
        let containers = [app.collectionViews, app.tables, app.scrollViews]
        #endif
        func container(owning identifier: String) -> XCUIElement? {
            #if os(macOS)
            if surface.exists, [.scrollView, .table, .collectionView].contains(surface.elementType),
               surface.identifier == identifier || surface.descendants(matching: .any).matching(identifier: identifier).firstMatch.exists {
                return surface
            }
            #endif
            for query in containers {
                let identified = query.matching(identifier: identifier).firstMatch
                if identified.exists { return identified }
                let owner = query.containing(.any, identifier: identifier).firstMatch
                if owner.exists { return owner }
            }
            return nil
        }
        let targetIdentifier = target.exists ? target.identifier : ""
        let targetScroll = targetIdentifier.isEmpty ? nil : container(owning: targetIdentifier)
        func targetReady() -> Bool {
            guard target.exists && target.isHittable else { return false }
            #if os(macOS)
            if let owner = targetScroll {
                guard owner.exists, window.exists else { return false }
                let frame = target.frame, viewport = owner.frame.intersection(window.frame)
                guard [frame, viewport].allSatisfy({
                    !$0.isNull && !$0.isEmpty && [$0.origin.x, $0.origin.y, $0.width, $0.height].allSatisfy { $0.isFinite }
                }) else { return false }
                let center = CGPoint(x: frame.midX, y: frame.midY)
                return center.x.isFinite && center.y.isFinite && viewport.contains(center)
            }
            #endif
            return true
        }
        let scroll = targetScroll ?? { () -> XCUIElement? in
            let settingsIDs = ["settings.root.content", "settings.anki.endpoint", "settings.anki.loginIdentity"]
            for id in settingsIDs {
                if let detail = container(owning: id) { return detail }
            }
            #if os(macOS)
            let root = surface
            #else
            let root = element(app, "settings.root.content")
            #endif
            if root.exists {
                if let detail = [root.scrollViews, root.tables, root.collectionViews]
                    .map({ $0.firstMatch }).first(where: { $0.exists }) { return detail }
                #if os(macOS)
                // macOS may expose the visible Form only as its identified AX group.
                if canScrollSettingsRoot(root) {
                    usesSettingsRoot = true
                    return root
                }
                #endif
                return nil
            }
            // Identified detail content must never fall back to the sidebar.
            if settingsIDs.contains(where: { element(app, $0).exists }) { return nil }
            return containers.map { $0.firstMatch }.first(where: { $0.exists })
        }()
        var upwardSearchesRemaining = 8
        var searchesRemaining = 16
        for _ in 0..<24 {
            let targetExists = target.exists
            if targetReady() { break }
            guard let scroll, scroll.exists else { break }
            let searchUpward = !targetExists && upwardSearchesRemaining > 0
            if searchUpward {
                upwardSearchesRemaining -= 1
            } else {
                guard searchesRemaining > 0 else { break }
                searchesRemaining -= 1
            }
            #if os(macOS)
            if usesSettingsRoot && !canScrollSettingsRoot(scroll) { break }
            guard app.state == .runningForeground else { break }
            guard window.exists else { break }
            let viewport = scroll.frame.intersection(window.frame)
            guard !viewport.isNull, !viewport.isEmpty,
                  viewport.origin.x.isFinite, viewport.origin.y.isFinite,
                  viewport.width.isFinite, viewport.height.isFinite else { break }
            let towardsBottom = targetExists ? target.frame.maxY > viewport.maxY : !searchUpward
            scroll.scroll(byDeltaX: 0, deltaY: (towardsBottom ? -1 : 1) * viewport.height / 2)
            #else
            var viewport = scroll.frame.intersection(app.frame)
            let keyboard = app.keyboards.firstMatch
            if keyboard.exists, viewport.intersects(keyboard.frame) {
                viewport.size.height = max(0, min(viewport.maxY, keyboard.frame.minY) - viewport.minY)
            }
            guard !viewport.isNull, viewport.width > 0, viewport.height > 0 else { break }
            let towardsBottom = targetExists ? target.frame.maxY > viewport.maxY : !searchUpward
            // Keep the gesture inside the visible Form, above the keyboard.
            let origin = scroll.coordinate(withNormalizedOffset: .zero)
            let upper = origin.withOffset(CGVector(dx: viewport.midX - scroll.frame.minX,
                                                  dy: viewport.minY + viewport.height * 0.2 - scroll.frame.minY))
            let lower = origin.withOffset(CGVector(dx: viewport.midX - scroll.frame.minX,
                                                  dy: viewport.minY + viewport.height * 0.8 - scroll.frame.minY))
            let start = towardsBottom ? lower : upper
            let end = towardsBottom ? upper : lower
            start.press(forDuration: 0, thenDragTo: end)
            #endif
        }
        let visible = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in targetReady() }, object: nil)
        let outcome = XCTWaiter().wait(for: [visible], timeout: 30)
        if outcome != .completed {
            #if os(macOS)
            attachGeometry(app, "rf4-reveal-failure-geometry", bundle)
            #endif
            attach(app, "rf4-reveal-failure", target: target, scroll: scroll)
        }
        XCTAssertEqual(outcome, .completed, "Element not visible: \(targetIdentifier)")
    }
    private func waitUntilReady(_ target: XCUIElement, _ app: XCUIApplication) {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in target.exists && target.isEnabled }, object: nil)
        let outcome = XCTWaiter().wait(for: [ready], timeout: 30)
        if outcome != .completed { attach(app, "rf4-ready-failure", target: target) }
        XCTAssertEqual(outcome, .completed, "Action unavailable: \(target.exists ? target.identifier : "absent")")
    }
    private func replaceText(_ field: XCUIElement, app: XCUIApplication, with value: String) {
        #if os(macOS)
        field.typeKey("a", modifierFlags: .command); field.typeText(value)
        #else
        let count = (field.value as? String)?.count ?? 0
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + value)
        #endif
    }
    private func waitForText(_ target: XCUIElement, _ text: String, _ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        try reveal(target, app, bundle)
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            target.exists && (target.label + " " + String(describing: target.value ?? "")).contains(text)
        }, object: nil)
        let outcome = XCTWaiter().wait(for: [ready], timeout: 45)
        if outcome != .completed { attach(app, "rf4-text-wait-failure", target: target) }
        XCTAssertEqual(outcome, .completed)
    }
    private func completeOptionalFsrsAdmission(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        // Admission belongs to the main window, even when Settings initiated
        // replacement. Closing the workflow neither cancels nor grants consent.
        try closeSettings(app, bundle)
        #endif
        try app.bringToFront(bundleIdentifier: bundle)
        let admission = element(app, "home.policy.enable")
        if admission.waitForExistence(timeout: 2), admission.isEnabled {
            try click(admission, app, bundle)
        }
        // A disabled button means admission is already running; wait for that
        // job rather than clicking again or navigating behind its modal sheet.
        try app.bringToFront(bundleIdentifier: bundle)
        let sheet = element(app, "home.policy.sheet")
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !sheet.exists }, object: nil)
        let outcome = XCTWaiter().wait(for: [dismissed], timeout: 30)
        if outcome != .completed { attach(app, "rf4-policy-admission-failure") }
        XCTAssertEqual(outcome, .completed, "FSRS admission must finish before continuing.")
    }
    private func openSettings(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try closeTransfer(app, bundle)
        try app.bringToFront(bundleIdentifier: bundle)
        let settings = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        if !settings.exists {
            let main = try keyboardWindow(app.windows.matching(identifier: "main").firstMatch)
            main.typeKey(",", modifierFlags: .command)
        }
        let settingsOpened = element(app, "settings.root.content").waitForExistence(timeout: 30)
        if !settingsOpened {
            let notificationCenter = XCUIApplication(bundleIdentifier: "com.apple.UserNotificationCenter")
            let notificationCenterState = notificationCenter.state
            let notificationCenterRunning = notificationCenterState == .runningForeground
                || notificationCenterState == .runningBackground
            var facts: [String: Any] = [
                "appStateRawValue": app.state.rawValue,
                "appWindowCount": app.windows.count,
                "mainWindowCount": app.windows.matching(NSPredicate(format: "identifier == %@", "main")).count,
                "settingsRootContentCount": app.descendants(matching: .any)
                    .matching(NSPredicate(format: "identifier == %@", "settings.root.content")).count,
                "settingsRootConnectionCount": app.descendants(matching: .any)
                    .matching(NSPredicate(format: "identifier == %@", "settings.root.connection")).count,
                "settingsRootCollectionCount": app.descendants(matching: .any)
                    .matching(NSPredicate(format: "identifier == %@", "settings.root.collection")).count,
                "settingsAnkiEndpointCount": app.descendants(matching: .any)
                    .matching(NSPredicate(format: "identifier == %@", "settings.anki.endpoint")).count,
                "notificationCenterRunning": notificationCenterRunning
            ]
            if notificationCenterRunning {
                facts["notificationCenterDialogCount"] = notificationCenter.dialogs.count
            }
            if let data = try? JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]) {
                let report = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
                report.name = "settings-open-failure-facts"
                report.lifetime = .keepAlways
                add(report)
            }
        }
        XCTAssertTrue(settingsOpened,
                      "Command-comma must open the native Settings scene.")
        try click(element(app, "settings.root.connection"), app, bundle)
        #else
        try openSection(app, bundle, name: "settings", title: "Settings")
        #endif
    }
    private func openHome(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try closeSettings(app, bundle)
        #endif
        try openSection(app, bundle, name: "home", title: "Decks")
    }
    private func continueTransfer(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        if element(app, "settings.anki.transferWorkflow").exists { return }
        try openSettings(app, bundle)
        let resume = element(app, "settings.anki.continueTransfer")
        if resume.exists {
            try click(resume, app, bundle)
        } else {
            try click(element(app, "settings.root.collection"), app, bundle)
            try click(element(app, "settings.anki.transferDetails"), app, bundle)
        }
        XCTAssertTrue(element(app, "settings.anki.transferWorkflow").waitForExistence(timeout: 30),
                      "The retained transfer must open in the real workflow sheet.")
        #endif
    }
    private func showHistory(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        if !element(app, "settings.anki.transferWorkflow").exists {
            try click(element(app, "settings.root.connection"), app, bundle)
            let history = element(app, "settings.anki.history")
            if !history.exists { try clickSettingsDetailsHeader(app, bundle) }
        }
        #endif
        try reveal(element(app, "settings.anki.history"), app, bundle)
    }
    private func logout(_ app: XCUIApplication, _ bundle: String) throws {
        #if os(macOS)
        try openSettings(app, bundle)
        #endif
        try click(element(app, "settings.anki.logout"), app, bundle)
    }
    #if os(macOS)
    private func settingsDetailsHeader(_ app: XCUIApplication) -> (container: XCUIElement, header: XCUIElement)? {
        let containers = app.descendants(matching: .any).matching(identifier: "settings.anki.details")
        guard containers.count == 1, let container = containers.allElementsBoundByAccessibilityElement.first else { return nil }
        if container.elementType == .disclosureTriangle { return (container, container) }
        let headers = container.descendants(matching: .disclosureTriangle)
        guard headers.count == 1, let header = headers.allElementsBoundByAccessibilityElement.first else { return nil }
        return (container, header)
    }
    private func clickSettingsDetailsHeader(_ app: XCUIApplication, _ bundle: String) throws {
        guard CardSwapTimingGate.hypervisorPresent() == true else { throw EvidenceError.unavailable }
        try app.bringToFront(bundleIdentifier: bundle)
        let (container, header) = try XCTUnwrap(settingsDetailsHeader(app), "Expected one scoped native disclosure header.")
        try reveal(header, app, bundle)
        waitUntilReady(header, app)
        let build = try BuildUnderTest.current()
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
        let roots = app.descendants(matching: .any).matching(identifier: "settings.root.content")
        let windows = app.windows.containing(.any, identifier: "settings.anki.details")
        guard running.count == 1, let live = running.first, roots.count == 1, windows.count == 1,
              let root = roots.allElementsBoundByAccessibilityElement.first,
              let window = windows.allElementsBoundByAccessibilityElement.first else { throw EvidenceError.unavailable }
        let boundPID = live.processIdentifier
        let headerFrame = header.frame, rootFrame = root.frame, windowFrame = window.frame
        let containerFrame = container.frame, headerID = header.identifier
        func finite(_ frame: CGRect) -> Bool {
            !frame.isNull && !frame.isInfinite && !frame.isEmpty &&
                [frame.minX, frame.maxX, frame.minY, frame.maxY, frame.width, frame.height].allSatisfy { $0.isFinite }
        }
        func collapsed(_ value: Any?) -> Bool {
            if let number = value as? NSNumber { return number == 0 }
            if let text = value as? String { return text == "0" || text == "false" }
            return false
        }
        let viewport = rootFrame.intersection(windowFrame)
        guard [headerFrame, containerFrame, rootFrame, windowFrame, viewport].allSatisfy(finite) else { throw EvidenceError.unavailable }
        let x: CGFloat
        if abs(headerFrame.width - 147.5) <= 0.5 && abs(headerFrame.height - 16) <= 0.5 { x = 26 }
        else if (8...24).contains(headerFrame.width) && (8...24).contains(headerFrame.height) { x = headerFrame.width / 2 }
        else { attachGeometry(app, "rf4-header-unsupported-geometry", bundle); throw EvidenceError.unavailable }
        let point = CGPoint(x: headerFrame.minX + x, y: headerFrame.midY)
        let coordinate = header.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x, dy: headerFrame.height / 2))
        let actual = coordinate.screenPoint
        let currentRunning = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
        guard actual.x.isFinite, actual.y.isFinite, abs(actual.x - point.x) <= 0.5, abs(actual.y - point.y) <= 0.5,
              viewport.contains(headerFrame), viewport.contains(point),
              app.state == .runningForeground, live.processIdentifier > 0, live.processIdentifier == boundPID,
              !live.isTerminated, live.isActive, NSWorkspace.shared.frontmostApplication?.processIdentifier == boundPID,
              live.bundleURL?.resolvingSymlinksInPath() == build.appBundle.resolvingSymlinksInPath(),
              currentRunning.count == 1, currentRunning.first?.processIdentifier == boundPID,
              app.sheets.count == 0, app.alerts.count == 0, roots.count == 1, windows.count == 1,
              container.exists, container.identifier == "settings.anki.details", container.frame == containerFrame,
              settingsDetailsHeader(app) != nil, header.exists, header.elementType == .disclosureTriangle,
              header.identifier == headerID, header.frame == headerFrame, header.isEnabled, header.isHittable,
              root.exists, root.frame == rootFrame, window.exists, window.frame == windowFrame,
              root.descendants(matching: .any).matching(identifier: "settings.anki.details").count == 1,
              window.descendants(matching: .any).matching(identifier: "settings.root.content").count == 1,
              collapsed(header.value)
        else { attachGeometry(app, "rf4-header-final-guard-failure", bundle); throw EvidenceError.unavailable }
        coordinate.click()
    }
    private func keyboardWindow(_ window: XCUIElement) throws -> XCUIElement {
        let target = try XCTUnwrap(window.exists ? window : nil, "Native keyboard target must be an existing window.")
        // The application AX frame can be null; synthesize keys against a real window.
        let frame = target.frame
        return try XCTUnwrap(!frame.isNull && !frame.isEmpty
            && frame.origin.x.isFinite && frame.origin.y.isFinite
            && frame.width.isFinite && frame.height.isFinite ? target : nil,
            "Native keyboard target must have a finite, nonempty window frame.")
    }
    private func closeTransfer(_ app: XCUIApplication, _ bundle: String) throws {
        let workflow = element(app, "settings.anki.transferWorkflow")
        guard workflow.exists else { return }
        try click(element(app, "settings.anki.closeTransfer"), app, bundle)
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !workflow.exists }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [dismissed], timeout: 30), .completed,
                       "Done must close the transfer sheet before navigating Settings.")
    }
    private func closeSettings(_ app: XCUIApplication, _ bundle: String) throws {
        try closeTransfer(app, bundle)
        let settings = app.windows.containing(.any, identifier: "settings.root.content").firstMatch
        guard settings.exists else { return }
        try app.bringToFront(bundleIdentifier: bundle)
        let target = try keyboardWindow(settings)
        target.typeKey("w", modifierFlags: .command)
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !settings.exists }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [dismissed], timeout: 30), .completed,
                       "Settings must close before admission or real local reviews.")
    }
    #endif
    private func openSection(_ app: XCUIApplication, _ bundle: String, name: String, title: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        let root = element(app, name == "home" ? "home.collection.screen" : "settings.root.content")
        if root.exists && root.isHittable { return }
        let tab = name == "home" ? "decks" : name
        let match = app.descendants(matching: .any).matching(NSPredicate(format: "identifier IN %@", ["shell.root.tab.\(tab)", "shell.root.sidebar.\(name)"])).firstMatch
        if match.waitForExistence(timeout: 5), match.isHittable { try click(match, app, bundle) }
        else {
            let sidebar = app.buttons["Toggle Sidebar"].firstMatch
            if sidebar.exists && sidebar.isHittable { try click(sidebar, app, bundle) }
            try click(app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@ OR label == %@", "shell.root.sidebar.\(name)", title)).firstMatch, app, bundle)
        }
    }
    private func confirmNative(_ app: XCUIApplication, _ bundle: String) throws {
        let button = app.buttons.matching(NSPredicate(format: "label == %@", "Replace Collection")).firstMatch
        try click(button, app, bundle)
    }
    private func attachSuccess(_ app: XCUIApplication, _ name: String) {
        attach(app, name + "-state")
        // These existing checkpoints follow login. Never capture a login form
        // or any other secure field, even if a preceding action regresses.
        guard !element(app, "settings.anki.password").exists,
              !app.secureTextFields.firstMatch.exists else { return }
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
    }
    private func transferResultCategory(_ target: XCUIElement) -> String {
        guard target.exists else { return "absent" }
        // Compare whole fixed coordinator messages in memory. Never return AX text.
        for text in [target.label, target.value as? String ?? ""] {
            switch text {
            case "Logged in. Choose a direction, then prepare its preview.":
                return "logged-in"
            case "The server request failed. Check the endpoint and login, then inspect the recorded stages.":
                return "server-failed"
            case "The transfer could not finish. Inspect the recorded stages and retained copies below.",
                 "The operation could not finish. Local reviews and Collection Files remain available.",
                 "Couldn’t save this endpoint. Enter an http or https server URL with an optional path prefix, without credentials, a query or a fragment.":
                return "generic-failed"
            case "Saved server login is unavailable. Local reviews and Collection Files remain available.",
                 "Couldn’t remove the device login. Unlock this device and try again.",
                 "Local transfer history is unavailable. Reviews and Collection Files remain available.":
                return "keychain/unavailable"
            case "This login is cooling down after a completed transfer. Reviews and Collection Files remain available.":
                return "cooldown"
            case "A transfer or file operation needs attention. Inspect local history below.",
                 "Finish the current review or file operation first.",
                 "Finish the current review before preparing a server transfer.":
                return "busy"
            default: continue
            }
        }
        return "unknown"
    }
    #if os(macOS)
    private func prepareGeometryFacts(_ app: XCUIApplication, _ id: String, _ target: XCUIElement) -> [String] {
        guard id == "settings.anki.prepareImport" || id == "settings.anki.prepareExport" else { return [] }
        let controlID = NSPredicate(format: "identifier == %@", id)
        let rootID = NSPredicate(format: "identifier == %@", "settings.anki.transferWorkflow")
        let all = app.descendants(matching: .any), controls = all.matching(controlID), roots = all.matching(rootID)
        let owners = app.sheets.containing(rootID), windows = app.windows.containing(rootID)
        let exists = target.exists, targetFrame = exists ? target.frame : .zero
        let center = CGPoint(x: targetFrame.midX, y: targetFrame.midY)
        var lines = ["controlID=\(id) sheetCount=\(app.sheets.count) owningSheetCount=\(owners.count)",
                     "workflowRootCount=\(roots.count) controlCount=\(controls.count) nativeButtonCount=\(app.buttons.matching(controlID).count) owningWindowCount=\(windows.count)",
                     "targetExists=\(exists) targetRole=\(exists ? Int(target.elementType.rawValue) : -1) targetEnabled=\(exists && target.isEnabled) targetHittable=\(exists && target.isHittable)",
                     "targetCenter=[\(center.x),\(center.y)]"]
        func frame(_ value: CGRect, _ key: String) {
            lines.append("\(key)=[\(value.origin.x),\(value.origin.y),\(value.width),\(value.height)]")
            lines.append("\(key)CenterContained=\(exists && !value.isNull && !value.isEmpty && value.contains(center))")
        }
        frame(targetFrame, "targetFrame")
        lines.append("ambiguous=\(controls.count != 1 || roots.count != 1 || owners.count != 1 || windows.count != 1)")
        guard controls.count == 1, roots.count == 1, owners.count == 1, windows.count == 1 else { return lines }
        let owner = owners.firstMatch, root = roots.firstMatch
        let ownerRoots = owner.descendants(matching: .any).matching(rootID).count
        let ownerControls = owner.descendants(matching: .any).matching(controlID).count
        lines += ["ownerWorkflowRootCount=\(ownerRoots) ownerControlCount=\(ownerControls) ownerNativeButtonCount=\(owner.buttons.matching(controlID).count)",
                  "rootControlCount=\(root.descendants(matching: .any).matching(controlID).count)"]
        guard ownerRoots == 1, ownerControls == 1 else { return lines }
        let rootFrame = root.frame, sheetFrame = owner.frame, windowFrame = windows.firstMatch.frame
        frame(rootFrame, "workflowRootFrame"); frame(sheetFrame, "owningSheetFrame"); frame(windowFrame, "owningWindowFrame")
        frame(sheetFrame.intersection(windowFrame), "sheetWindowViewport")
        frame(rootFrame.intersection(sheetFrame).intersection(windowFrame), "rootSheetWindowViewport")
        let control = controls.firstMatch, children = control.children(matching: .any), childCount = children.count
        lines += ["nativeButtonDescendantCount=\(control.descendants(matching: .button).count) immediateChildCount=\(childCount) childrenTruncated=\(childCount > 8)"]
        for index in 0..<min(childCount, 8) {
            let child = children.element(boundBy: index)
            lines.append("child\(index)Role=\(child.elementType.rawValue)"); frame(child.frame, "child\(index)Frame")
        }
        let viewports = [owner.scrollViews, owner.tables, owner.collectionViews].map { $0.containing(controlID) }
        let viewportCount = viewports.reduce(0) { $0 + $1.count }; var emitted = 0
        lines += ["actualScrollOwnerCount=\(viewportCount)", "scrollOwnersTruncated=\(viewportCount > 4)"]
        for query in viewports {
            for index in 0..<min(query.count, 4 - emitted) {
                let viewport = query.element(boundBy: index)
                lines.append("scrollOwner\(emitted)Role=\(viewport.elementType.rawValue)"); frame(viewport.frame, "scrollOwner\(emitted)Frame")
                frame(viewport.frame.intersection(rootFrame).intersection(sheetFrame).intersection(windowFrame), "scrollOwner\(emitted)Viewport")
                emitted += 1
            }
        }
        return lines
    }
    private func attachGeometry(_ app: XCUIApplication, _ name: String, _ bundle: String) {
        // Geometry and closed disclosure facts only; never emit AX text or inspect account controls.
        let appState = app.state
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
        let appExists = app.exists
        let appFrame = appExists ? app.frame : nil
        let window = app.windows.firstMatch
        let windowExists = window.exists
        let windowFrame = windowExists ? window.frame : nil
        let root = element(app, "settings.root.content")
        let rootExists = root.exists
        let rootFrame = rootExists ? root.frame : nil
        var lines = [
            "appState=\(appState.rawValue)",
            "appForeground=\(appState == .runningForeground)",
            "runningApplicationCount=\(running.count)",
            "runningApplicationIsActive=\(running.map { $0.isActive })",
            "appExists=\(appExists)",
            "windowExists=\(windowExists)",
            "settingsRootExists=\(rootExists)",
            "sheetCount=\(app.sheets.count)",
            "alertCount=\(app.alerts.count)"
        ]
        func frame(_ value: CGRect, _ key: String) {
            lines.append("\(key)=[\(value.origin.x),\(value.origin.y),\(value.width),\(value.height)]")
            lines.append("\(key)IsNull=\(value.isNull)")
            lines.append("\(key)IsEmpty=\(value.isEmpty)")
        }
        if let appFrame { frame(appFrame, "appFrame") }
        if let windowFrame { frame(windowFrame, "windowFrame") }
        if let rootFrame {
            lines.append("settingsRootEnabled=\(root.isEnabled)")
            lines.append("settingsRootHittable=\(root.isHittable)")
            frame(rootFrame, "settingsRootFrame")
            if let windowFrame {
                let intersection = rootFrame.intersection(windowFrame)
                frame(intersection, "rootWindowIntersection")
                frame(intersection, "viewportIntersection")
            }
        }
        let descendants = app.descendants(matching: .any)
        let container = element(app, "settings.anki.details")
        let resolved = settingsDetailsHeader(app)
        let details = resolved?.header
        let detailsExists = details?.exists == true
        lines.append("detailsExactIDCount=\(descendants.matching(NSPredicate(format: "identifier == %@", "settings.anki.details")).count)")
        lines.append("detailsContainerExists=\(container.exists)")
        if container.exists { lines.append("detailsContainerElementType=\(container.elementType.rawValue)") }
        lines.append("detailsNativeHeaderResolved=\(resolved != nil)")
        lines.append("detailsExists=\(detailsExists)")
        var detailsValueCategory = "absent"
        if let details, detailsExists {
            lines.append("detailsEnabled=\(details.isEnabled)")
            lines.append("detailsHittable=\(details.isHittable)")
            lines.append("detailsElementType=\(details.elementType.rawValue)")
            let immediateChildren = details.children(matching: .any)
            let immediateChildCount = immediateChildren.count
            lines.append("detailsImmediateChildCount=\(immediateChildCount)")
            let immediateChildTypes = (0..<min(immediateChildCount, 8)).map {
                immediateChildren.element(boundBy: $0).elementType.rawValue
            }
            lines.append("detailsImmediateChildElementTypes=\(immediateChildTypes)")
            frame(details.frame, "detailsFrame")
            // Compare allowed values in memory; never stringify the AX value.
            switch details.value {
            case nil: break
            case let number as NSNumber:
                detailsValueCategory = number == 1 ? "expanded" : number == 0 ? "collapsed" : "other"
            case let text as String:
                switch text {
                case "1", "true": detailsValueCategory = "expanded"
                case "0", "false": detailsValueCategory = "collapsed"
                default: detailsValueCategory = "other"
                }
            default: detailsValueCategory = "other"
            }
        }
        lines.append("detailsValueCategory=\(detailsValueCategory)")
        lines.append("syncLogsLabelOrTitleCount=\(descendants.matching(NSPredicate(format: "label == %@ OR title == %@", "Sync Logs", "Sync Logs")).count)")
        lines.append("refreshLocalHistoryLabelOrTitleCount=\(descendants.matching(NSPredicate(format: "label == %@ OR title == %@", "Refresh Local History", "Refresh Local History")).count)")
        lines.append("historyExactIDCount=\(descendants.matching(NSPredicate(format: "identifier == %@", "settings.anki.history")).count)")
        let report = XCTAttachment(string: lines.joined(separator: "\n"))
        report.name = name; report.lifetime = .keepAlways; add(report)
        if name == "rf4-reveal-failure-geometry" {
            attachCollapsedSettingsDetailsHeader(app)
        }
    }
    private func attachCollapsedSettingsDetailsHeader(_ app: XCUIApplication) {
        // Failure-only crop of the fixed collapsed header, including its leading chevron.
        let detailsID = NSPredicate(format: "identifier == %@", "settings.anki.details")
        let rootID = NSPredicate(format: "identifier == %@", "settings.root.content")
        let detailsMatches = app.descendants(matching: .any).matching(detailsID)
        let windowMatches = app.windows.containing(detailsID)
        guard detailsMatches.count == 1, windowMatches.count == 1 else { return }
        let boundDetails = detailsMatches.allElementsBoundByAccessibilityElement
        let boundWindows = windowMatches.allElementsBoundByAccessibilityElement
        guard boundDetails.count == 1, boundWindows.count == 1,
              let details = boundDetails.first, let window = boundWindows.first,
              details.exists, window.exists else { return }
        let detailsFrame = details.frame
        let windowFrame = window.frame

        func validFrame(_ frame: CGRect) -> Bool {
            !frame.isNull && !frame.isInfinite && !frame.isEmpty &&
                frame.origin.x.isFinite && frame.origin.y.isFinite &&
                frame.width.isFinite && frame.height.isFinite &&
                frame.minX.isFinite && frame.maxX.isFinite &&
                frame.minY.isFinite && frame.maxY.isFinite &&
                frame.width > 0 && frame.height > 0
        }
        guard validFrame(detailsFrame), validFrame(windowFrame) else { return }
        let cropFrame = CGRect(x: detailsFrame.minX - 32, y: detailsFrame.minY - 8,
                               width: detailsFrame.width + 40, height: detailsFrame.height + 16)

        func safeHeader() -> Bool {
            guard app.state == .runningForeground, app.exists,
                  app.alerts.count == 0, app.sheets.count == 0,
                  app.descendants(matching: .secureTextField).count == 0,
                  detailsMatches.count == 1, windowMatches.count == 1,
                  app.windows.containing(rootID).count == 1,
                  details.exists, window.exists, window.elementType == .window,
                  details.identifier == "settings.anki.details",
                  window.descendants(matching: .any).matching(detailsID).count == 1,
                  window.descendants(matching: .any).matching(rootID).count == 1,
                  details.frame == detailsFrame, window.frame == windowFrame,
                  validFrame(details.frame), validFrame(window.frame),
                  validFrame(cropFrame), cropFrame.width <= 260, cropFrame.height <= 48,
                  windowFrame.contains(cropFrame) else { return false }
            switch details.value {
            case let number as NSNumber:
                guard number == 0 else { return false }
            case let text as String:
                guard text == "0" || text == "false" else { return false }
            default: return false
            }
            let fields = window.descendants(matching: .textField).allElementsBoundByAccessibilityElement +
                window.descendants(matching: .secureTextField).allElementsBoundByAccessibilityElement
            for field in fields {
                guard field.exists else { return false }
                let fieldFrame = field.frame
                guard validFrame(fieldFrame), !fieldFrame.intersects(cropFrame) else { return false }
            }
            return true
        }
        guard safeHeader() else { return }
        let windowImage = window.screenshot().image
        var proposedRect = CGRect(origin: .zero, size: windowImage.size)
        guard let source = windowImage.cgImage(forProposedRect: &proposedRect, context: nil, hints: nil),
              source.width > 0, source.height > 0 else { return }
        let scaleX = CGFloat(source.width) / windowFrame.width
        let scaleY = CGFloat(source.height) / windowFrame.height
        guard scaleX.isFinite, scaleY.isFinite, scaleX > 0, scaleY > 0 else { return }
        let relativeFrame = cropFrame.offsetBy(dx: -windowFrame.minX, dy: -windowFrame.minY)
        let minX = ceil(relativeFrame.minX * scaleX)
        let minY = ceil(relativeFrame.minY * scaleY)
        let maxX = floor(relativeFrame.maxX * scaleX)
        let maxY = floor(relativeFrame.maxY * scaleY)
        let pixelFrame = CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
        let imageBounds = CGRect(x: 0, y: 0, width: CGFloat(source.width), height: CGFloat(source.height))
        guard validFrame(relativeFrame), validFrame(pixelFrame), imageBounds.contains(pixelFrame),
              let cropped = source.cropping(to: pixelFrame) else { return }
        guard safeHeader() else { return }
        let attachment = XCTAttachment(image: NSImage(cgImage: cropped, size: cropFrame.size))
        attachment.name = "settings-details-collapsed-header"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
    #endif
    private func attach(_ app: XCUIApplication, _ name: String, target: XCUIElement? = nil, scroll: XCUIElement? = nil) {
        // Whitelist state only: no labels, identifiers read from elements, field
        // contents, screenshots or hierarchy (which can contain credentials).
        func state(_ item: XCUIElement, _ key: String, field: Bool = false) -> String {
            guard item.exists else { return "\(key): exists=false enabled=unavailable hittable=unavailable type=unavailable frame=unavailable valueLength=unavailable" }
            let frame = item.frame
            var line = "\(key): exists=true enabled=\(item.isEnabled) hittable=\(item.isHittable) type=\(item.elementType.rawValue) frame=[\(frame.origin.x),\(frame.origin.y),\(frame.width),\(frame.height)]"
            if field {
                // Secure fields may expose masking characters; this is AX value
                // length, not proof of the underlying password's exact length.
                let length = (item.value as? String).map { String($0.count) } ?? "unavailable"
                line += " valueLength=\(length)"
            }
            return line
        }
        var lines = [String]()
        #if os(macOS)
        if let target, target === confirmDiagnosticTarget, !target.exists, let prepareGeometry {
            lines += prepareGeometry.lines.map { "beforePrepare.\($0)" }
            lines += prepareGeometryFacts(app, prepareGeometry.id, element(app, prepareGeometry.id)).map { "afterMissingPreview.\($0)" }
        }
        #endif
        if let target { lines.append(state(target, "target")) }
        let result = element(app, "settings.anki.result")
        lines.append("resultCategory=\(transferResultCategory(result))")
        if let scroll { lines.append(state(scroll, "scrollContainer")) }
        #if !os(macOS)
        let keyboard = app.keyboards.firstMatch
        lines.append("keyboardPresent=\(keyboard.exists)")
        if keyboard.exists {
            let frame = keyboard.frame
            lines.append("keyboardFrame=[\(frame.origin.x),\(frame.origin.y),\(frame.width),\(frame.height)]")
        }
        #endif
        for id in ["settings.anki.endpoint", "settings.anki.saveEndpoint", "settings.anki.login",
                   "settings.anki.username", "settings.anki.password"] {
            lines.append(state(element(app, id), id, field: id == "settings.anki.username" || id == "settings.anki.password"))
        }
        let configured = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Configured endpoint")).firstMatch.exists
        lines.append("configuredEndpointRowPresent=\(configured)")
        lines.append("secureTextFieldPresent=\(app.secureTextFields.firstMatch.exists)")
        lines.append("operationProgressPresent=\(element(app, "settings.anki.progress").exists)")
        lines.append("progressIndicatorPresent=\(app.progressIndicators.firstMatch.exists)")
        lines.append("resultPresent=\(result.exists)")
        lines.append("loginIdentityPresent=\(element(app, "settings.anki.loginIdentity").exists)")
        lines.append("usernameFieldPresent=\(element(app, "settings.anki.username").exists)")
        lines.append("passwordFieldPresent=\(element(app, "settings.anki.password").exists)")
        lines.append("sheetCount=\(app.sheets.count)")
        lines.append("alertCount=\(app.alerts.count)")
        lines.append("home.policy.sheetPresent=\(element(app, "home.policy.sheet").exists)")
        let settingsRoot = element(app, "settings.root.content")
        let homeRoot = element(app, "home.collection.screen")
        lines.append(state(settingsRoot, "settingsRoot"))
        lines.append("settingsRootHittable=\(settingsRoot.exists && settingsRoot.isHittable)")
        lines.append("homeRootHittable=\(homeRoot.exists && homeRoot.isHittable)")
        lines.append("policyErrorPresent=\(element(app, "home.policy.error").exists)")
        let report = XCTAttachment(string: lines.joined(separator: "\n"))
        report.name = name; report.lifetime = .keepAlways; add(report)
    }
}
