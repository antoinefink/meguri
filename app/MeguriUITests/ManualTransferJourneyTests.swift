import Foundation
import CryptoKit
import XCTest

/// VM-only, through tools/mg. The controlled rig is seeded with generated
/// reviewable content; no fixture installs its collection into the app.
/// Integrator stages controlled-rig.json through its controlled forwarding job.
@MainActor final class ManualTransferJourneyTests: XCTestCase {
    private struct Rig: Decodable {
        let endpoint: String
        let username: String
        let password: String
        let exportUsername: String
        let exportPassword: String
        let deckID: Int64
        let fixturePath: String
        let supportPath: String
        let serverImportSHA256: String
        let serverExportSHA256: String
        let deviceSHA256: String
        let wireToken: String
    }
    func testDistinctAccountRoundTripPreservesImportCooldownAfterRestart() async throws {
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
        XCTAssertNotEqual(importUsername, exportUsername, "Import and Export must use separate accounts after trimming usernames.")

        let app = XCUIApplication()
        let bundle = try build.bundleIdentifier
        app.launchArguments = MeguriLaunch(
            fixture: URL(filePath: rig.fixturePath),
            timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }

        // Import A through the same Settings controls used outside tests.
        do {
            try openSettings(app, bundle)
            let savedLogin = element(app, "settings.anki.logout")
            if savedLogin.waitForExistence(timeout: 2) { try click(savedLogin, app, bundle) }
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
            try click(element(app, "settings.anki.saveEndpoint"), app, bundle)
            try login(app, bundle, username: importUsername, password: rig.password)
            let startup = try await wire(rig, "mark/startup")
            XCTAssertEqual(startup.media, 0, "Passive launch and login must send zero media requests.")
            try selectMediaScope(app, bundle)
            for id in ["settings.anki.prepareImport", "settings.anki.confirm"] {
                try click(element(app, id), app, bundle)
            }
            try confirmNative(app, bundle)
            try completeOptionalFsrsAdmission(app, bundle)
            try openSettings(app, bundle)
            try await allowAndFinishMedia(app, bundle, rig: rig, importing: true, name: "rf4-account-a-import")
        }

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
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        try waitForText(element(app, "home.session.recorded"), "1 answers recorded", app, bundle)
        attachSuccess(app, "rf4-account-a-review-durable-after-restart")

        // A's stored login must load without starting work. The next explicit
        // preparation uses system time and must meet A's existing cooldown.
        try openSettings(app, bundle)
        try waitForText(element(app, "settings.anki.loginIdentity"), importUsername, app, bundle)
        waitUntilReady(element(app, "settings.anki.logout"), app)
        XCTAssertFalse(element(app, "settings.anki.username").exists, "Restore A's saved login.")
        XCTAssertFalse(element(app, "settings.anki.confirm").exists, "Loading a login must leave preparation untouched.")
        try click(element(app, "settings.anki.prepareImport"), app, bundle)
        let result = element(app, "settings.anki.result")
        try waitForText(result, "cooling down", app, bundle)
        XCTAssertTrue(element(app, "settings.anki.nextEligible").waitForExistence(timeout: 5), "Record the deadline returned with the cooldown.")
        waitUntilReady(element(app, "settings.anki.prepareImport"), app)
        for (id, explanation) in [
            ("settings.anki.progress", "Wait for preparation to finish before checking its outcome."),
            ("settings.anki.confirm", "A's cooldown must leave no replacement preview.")
        ] {
            XCTAssertFalse(element(app, id).exists, explanation)
        }
        try reveal(result, app)
        attachSuccess(app, "rf4-account-a-restored-login-cooldown-enforced")

        // Use B for Export at the unchanged endpoint; A's saved history and
        // deadline remain in place while credentials change.
        try click(element(app, "settings.anki.logout"), app, bundle)
        try login(app, bundle, username: exportUsername, password: rig.exportPassword)
        try click(element(app, "settings.anki.direction"), app, bundle)
        #if os(macOS)
        try click(app.menuItems["Export to Anki"].firstMatch, app, bundle)
        #else
        try click(app.buttons["Export to Anki"].firstMatch, app, bundle)
        #endif
        try selectMediaScope(app, bundle)
        for id in ["settings.anki.prepareExport", "settings.anki.confirm"] {
            try click(element(app, id), app, bundle)
        }
        attachSuccess(app, "rf4-account-b-export-backup-preview")
        try confirmNative(app, bundle)
        try completeOptionalFsrsAdmission(app, bundle)
        try openSettings(app, bundle)
        try await allowAndFinishMedia(app, bundle, rig: rig, importing: false, name: "rf4-account-b-export")
        attachSuccess(app, "rf4-account-b-export-complete-local-activation")

        // Evidence covers two accounts: A is still blocked and B completes.
        // A later successful transfer by A is outside this run. Acceptance also
        // needs the rig's comparison of B's server and peer content with the
        // reviewed collection, including the answer saved above.
    }
    private func selectMediaScope(_ app: XCUIApplication, _ bundle: String) throws {
        try click(element(app, "settings.anki.scope"), app, bundle)
        #if os(macOS)
        try click(app.menuItems["Collection and media"].firstMatch, app, bundle)
        #else
        try click(app.buttons["Collection and media"].firstMatch, app, bundle)
        #endif
    }
    private func allowAndFinishMedia(_ app: XCUIApplication, _ bundle: String, rig: Rig, importing: Bool, name: String) async throws {
        // Collection activation/adoption must drain before this separate, real
        // both-side decision becomes enabled. No launch hook grants consent.
        let allow = element(app, "settings.anki.startMedia")
        try reveal(allow, app)
        waitUntilReady(allow, app)
        XCTAssertFalse(element(app, "settings.anki.confirm").exists)
        XCTAssertFalse(element(app, "settings.anki.progress").exists)
        let media = URL(filePath: rig.supportPath).appending(path: "Collection/collection.media")
        if importing {
            XCTAssertFalse(FileManager.default.fileExists(atPath: media.appending(path: "rf4-server.bin").path),
                           "The server file must not arrive before media consent.")
        } else {
            try assertMediaHash(media, name: "rf4-server.bin", expected: rig.serverImportSHA256)
        }
        try assertMediaHash(media, name: "rf4-device.bin", expected: rig.deviceSHA256)
        attachSuccess(app, name + "-collection-complete-media-awaiting-consent")
        let before = try await wire(rig, importing ? "mark/before-consent" : "mark/export-before-consent")
        XCTAssertEqual(before.active, 0)
        if importing { XCTAssertEqual(before.media, 0, "Preparing the media decision must be offline.") }
        let original = try durableOperation(rig, direction: importing ? "Import" : "Export")
        XCTAssertTrue(original.collectionComplete && original.receipt == nil && original.media.stage == "AwaitingConsent")
        try click(allow, app, bundle)
        if importing {
            // The observer holds the first real media file request before it
            // reaches the owned rig. Cancellation cannot race loopback completion.
            let held = try await waitForWire(rig) { $0.gate == "holding" && $0.held == 1 }
            XCTAssertGreaterThan(held.media, before.media)
            try click(element(app, "settings.anki.cancel"), app, bundle)
            try waitForText(element(app, "settings.anki.result"), "operation is partial", app, bundle)
            waitUntilReady(element(app, "settings.anki.history"), app)
            XCTAssertFalse(element(app, "settings.anki.progress").exists)
            XCTAssertFalse(element(app, "settings.anki.nextEligible").exists)

            let partial = try durableOperation(rig, direction: "Import")
            XCTAssertEqual(partial.media.stage, "Cancelled")
            assertSameOperation(original, partial)
            XCTAssertEqual(partial.phase, "MediaIncomplete")
            XCTAssertTrue(partial.collectionComplete)
            XCTAssertTrue(partial.receipt == nil)
            XCTAssertTrue(partial.media.remoteOutcomeUnknown)
            XCTAssertEqual(partial.media.counters.downloadedFiles, 0)
            XCTAssertEqual(partial.media.counters.uploadedFiles, 0)
            try assertNoSuccessCooldown(rig, operation: partial)
            _ = try await wire(rig, "abort")
            _ = try await waitForWire(rig) { $0.active == 0 && $0.aborted == 1 }
            let stopped = try await wire(rig, "mark/cancelled")
            XCTAssertEqual(stopped.collectionTransfers, before.collectionTransfers)
            attachSuccess(app, name + "-native-cancel-drained-durable-partial-no-cooldown")

            app.terminate()
            app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false).arguments
            app.launch()
            try completeOptionalFsrsAdmission(app, bundle)
            try openSettings(app, bundle)
            try waitForText(element(app, "settings.anki.loginIdentity"), rig.username, app, bundle)
            try click(element(app, "settings.anki.history"), app, bundle)
            waitUntilReady(element(app, "settings.anki.history"), app)
            XCTAssertFalse(element(app, "settings.anki.startMedia").exists)
            XCTAssertFalse(element(app, "settings.anki.progress").exists)
            let recovered = try durableOperation(rig, direction: "Import")
            assertSameOperation(partial, recovered)
            XCTAssertTrue(recovered.collectionComplete && recovered.receipt == nil)
            XCTAssertEqual(recovered.media.stage, "Cancelled")
            try assertNoSuccessCooldown(rig, operation: recovered)
            let passive = try await wire(rig, "mark/passive-history")
            XCTAssertEqual(passive.media, stopped.media, "Restart and offline history must issue zero additional media requests.")
            XCTAssertEqual(passive.collectionTransfers, stopped.collectionTransfers)
            attachSuccess(app, name + "-passive-restart-no-media-requests")

            try click(element(app, "settings.anki.resumeMedia.\(original.id)"), app, bundle)
            try reveal(allow, app)
            waitUntilReady(allow, app)
            let decision = try durableOperation(rig, direction: "Import")
            assertSameOperation(original, decision)
            XCTAssertEqual(decision.media.stage, "AwaitingConsent")
            XCTAssertTrue(decision.media.consent == nil && decision.receipt == nil)
            let prepared = try await wire(rig, "mark/resume-decision")
            XCTAssertEqual(prepared.media, passive.media, "Reviewing the saved media decision must remain offline.")
            XCTAssertEqual(prepared.collection, passive.collection)
            _ = try await wire(rig, "release")
            try click(allow, app, bundle) // fresh explicit both-side consent
        }
        try waitForText(element(app, "settings.anki.result"), "Collection and supported media complete", app, bundle)
        waitUntilReady(element(app, "settings.anki.history"), app)
        XCTAssertFalse(element(app, "settings.anki.startMedia").exists)
        XCTAssertTrue(element(app, "settings.anki.nextEligible").exists)
        try assertMediaHash(media, name: "rf4-server.bin", expected: importing ? rig.serverImportSHA256 : rig.serverExportSHA256)
        try assertMediaHash(media, name: "rf4-device.bin", expected: rig.deviceSHA256)
        XCTAssertEqual(try Data(contentsOf: media.appending(path: "rf4-zero.bin")).count, 0)
        let finished = try durableOperation(rig, direction: importing ? "Import" : "Export")
        assertSameOperation(original, finished)
        XCTAssertTrue(finished.phase == "Complete" && finished.media.stage == "Complete" && finished.receipt != nil)
        XCTAssertEqual(finished.media.remoteOutcomeUnknown, importing,
                       "A completed resume cannot erase unknown prior effects; fresh Export has none.")
        _ = try await waitForWire(rig) { $0.active == 0 }
        let completed = try await wire(rig, importing ? "mark/resumed" : "mark/exported")
        XCTAssertEqual(completed.collectionTransfers, before.collectionTransfers, "Media Resume must send zero collection uploads/downloads.")
        XCTAssertEqual(completed.collection, before.collection, "Media must never replace or resend the collection.")
        // Durable typed counters and actual app/server SHA256 values are also
        // checked by manual-transfer-data; this text alone cannot pass RF4.
        attachSuccess(app, name + "-supported-media-complete")
    }

    private struct WireCounts: Decodable {
        let media, collection, collectionTransfers, active, held, aborted, errors: Int
        let gate: String
    }
    private enum EvidenceError: Error { case unavailable }
    private func wire(_ rig: Rig, _ action: String) async throws -> WireCounts {
        var request = URLRequest(url: try XCTUnwrap(URL(string: rig.endpoint + "__rf4/" + action)))
        request.httpMethod = "POST"
        request.httpBody = Data()
        request.timeoutInterval = 5
        request.setValue(rig.wireToken, forHTTPHeaderField: "X-RF4-Control")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200, data.count <= 16384 else {
            throw EvidenceError.unavailable
        }
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        let counts = try decoder.decode(WireCounts.self, from: data)
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
        struct Receipt: Decodable { let completedAt, nextEligibleAt: Int64 }
        struct Media: Decodable {
            struct Counters: Decodable { let downloadedFiles, uploadedFiles: UInt64 }
            struct Consent: Decodable { let categories: UInt32 }
            let stage: String
            let remoteOutcomeUnknown: Bool
            let counters: Counters
            let consent: Consent?
        }
        let id, direction, phase, account, configuredEndpoint: String
        let collectionComplete: Bool
        let resultingPublication: [UInt64]?
        let receipt: Receipt?
        let media: Media
    }
    private struct DurableHistory: Decodable {
        struct Transfers: Decodable {
            let operations: [String: DurableOperation]
            let cooldowns: [String: DurableOperation.Receipt]
        }
        let manualTransfers: Transfers
    }
    private func durableHistory(_ rig: Rig) throws -> DurableHistory.Transfers {
        do {
            let url = URL(filePath: rig.supportPath).appending(path: "ManualTransfers/manual-transfers.json")
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(DurableHistory.self, from: Data(contentsOf: url)).manualTransfers
        } catch { throw EvidenceError.unavailable }
    }
    private func durableOperation(_ rig: Rig, direction: String) throws -> DurableOperation {
        let candidates = try durableHistory(rig).operations.values.filter { $0.direction == direction }
        // Operation IDs begin with a monotonically increasing fixed-width serial.
        guard let record = candidates.max(by: { $0.id < $1.id }),
              record.id.range(of: "^[0-9a-f]{16}-[0-9a-f]{16}$", options: .regularExpression) != nil else {
            throw EvidenceError.unavailable
        }
        return record
    }
    private func assertSameOperation(_ first: DurableOperation, _ second: DurableOperation) {
        // Compare opaque identities only in memory; never attach the sidecar.
        XCTAssertTrue(first.id == second.id && first.account == second.account && first.configuredEndpoint == second.configuredEndpoint)
        XCTAssertTrue(first.resultingPublication != nil && first.resultingPublication == second.resultingPublication,
                      "Media cancellation/recovery/resume must retain this collection activation.")
    }
    private func assertNoSuccessCooldown(_ rig: Rig, operation: DurableOperation) throws {
        XCTAssertTrue(try durableHistory(rig).cooldowns[operation.account] == nil,
                      "No whole-operation receipt or success cooldown exists for partial media.")
    }
    private func assertMediaHash(_ directory: URL, name: String, expected: String) throws {
        let bytes = try Data(contentsOf: directory.appending(path: name))
        let actual = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(actual, expected, "Actual app bytes differ for reserved synthetic media: \(name)")
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
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func click(_ target: XCUIElement, _ app: XCUIApplication, _ bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        try reveal(target, app)
        waitUntilReady(target, app)
        #if os(macOS)
        target.click()
        #else
        target.tap()
        #endif
    }
    private func reveal(_ target: XCUIElement, _ app: XCUIApplication) throws {
        _ = target.waitForExistence(timeout: 30)
        #if os(macOS)
        let containers = [app.scrollViews, app.tables, app.collectionViews]
        func canScrollSettingsRoot(_ root: XCUIElement) -> Bool {
            guard app.state == .runningForeground, !app.sheets.firstMatch.exists, !app.alerts.firstMatch.exists,
                  root.exists, root.isEnabled, root.isHittable else { return false }
            let window = app.windows.firstMatch
            guard window.exists else { return false }
            let viewport = root.frame.intersection(window.frame).intersection(app.frame)
            return !viewport.isNull && viewport.width > 0 && viewport.height > 0
        }
        var usesSettingsRoot = false
        #else
        let containers = [app.collectionViews, app.tables, app.scrollViews]
        #endif
        func container(owning identifier: String) -> XCUIElement? {
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
        let scroll = targetScroll ?? { () -> XCUIElement? in
            let settingsIDs = ["settings.root.content", "settings.anki.endpoint", "settings.anki.loginIdentity"]
            for id in settingsIDs {
                if let detail = container(owning: id) { return detail }
            }
            let root = element(app, "settings.root.content")
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
            if targetExists && target.isHittable { break }
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
            let window = app.windows.firstMatch
            guard window.exists else { break }
            let viewport = scroll.frame.intersection(window.frame).intersection(app.frame)
            guard !viewport.isNull, viewport.width > 0, viewport.height > 0 else { break }
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
        let visible = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in target.exists && target.isHittable }, object: nil)
        let outcome = XCTWaiter().wait(for: [visible], timeout: 30)
        if outcome != .completed { attach(app, "rf4-reveal-failure", target: target, scroll: scroll) }
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
        try reveal(target, app)
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            target.exists && (target.label + " " + String(describing: target.value ?? "")).contains(text)
        }, object: nil)
        let outcome = XCTWaiter().wait(for: [ready], timeout: 45)
        if outcome != .completed { attach(app, "rf4-text-wait-failure", target: target) }
        XCTAssertEqual(outcome, .completed)
    }
    private func completeOptionalFsrsAdmission(_ app: XCUIApplication, _ bundle: String) throws {
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
        try openSection(app, bundle, name: "settings", title: "Settings")
    }
    private func openHome(_ app: XCUIApplication, _ bundle: String) throws {
        try openSection(app, bundle, name: "home", title: "Decks")
    }
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
