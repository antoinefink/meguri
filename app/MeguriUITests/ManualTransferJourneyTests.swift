import Foundation
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
    }
    func testDistinctAccountRoundTripPreservesImportCooldownAfterRestart() throws {
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
            fixture: build.repositoryRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"),
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
            try click(element(app, "settings.anki.saveEndpoint"), app, bundle)
            try login(app, bundle, username: importUsername, password: rig.password)
            for id in ["settings.anki.prepareImport", "settings.anki.confirm"] {
                try click(element(app, id), app, bundle)
            }
            try confirmNative(app, bundle)
            let admission = element(app, "home.policy.enable")
            if admission.waitForExistence(timeout: 2) { try click(admission, app, bundle) }
            try openSettings(app, bundle)
            waitForText(element(app, "settings.anki.result"), "Collection transfer finished")
            attachSuccess(app, "rf4-account-a-import-finished")
        }

        // Save an answer in the imported deck before restarting the app.
        try openHome(app, bundle)
        let admission = element(app, "home.policy.enable")
        if admission.waitForExistence(timeout: 2) { try click(admission, app, bundle) }
        for id in ["home.deck.row.\(rig.deckID)", "home.deck.study", "review.session.reveal",
                   "review.session.rate.easy", "review.session.close"] {
            try click(element(app, id), app, bundle)
        }
        waitForText(element(app, "home.session.recorded"), "1 answers recorded")
        attachSuccess(app, "rf4-account-a-import-real-review-saved")
        app.terminate()
        app.launchArguments = MeguriLaunch(timeZone: "UTC", locale: "en_US", animations: false).arguments
        app.launch()
        waitForText(element(app, "home.session.recorded"), "1 answers recorded")
        attachSuccess(app, "rf4-account-a-review-durable-after-restart")

        // A's stored login must load without starting work. The next explicit
        // preparation uses system time and must meet A's existing cooldown.
        try openSettings(app, bundle)
        waitForText(element(app, "settings.anki.loginIdentity"), importUsername)
        waitUntilReady(element(app, "settings.anki.logout"), app)
        XCTAssertFalse(element(app, "settings.anki.username").exists, "Restore A's saved login.")
        XCTAssertFalse(element(app, "settings.anki.confirm").exists, "Loading a login must leave preparation untouched.")
        try click(element(app, "settings.anki.prepareImport"), app, bundle)
        let result = element(app, "settings.anki.result")
        waitForText(result, "cooling down")
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
        for id in ["settings.anki.prepareExport", "settings.anki.confirm"] {
            try click(element(app, id), app, bundle)
        }
        attachSuccess(app, "rf4-account-b-export-backup-preview")
        try confirmNative(app, bundle)
        waitForText(element(app, "settings.anki.result"), "Collection transfer finished")
        attachSuccess(app, "rf4-account-b-export-complete-local-activation")

        // Evidence covers two accounts: A is still blocked and B completes.
        // A later successful transfer by A is outside this run. Acceptance also
        // needs the rig's comparison of B's server and peer content with the
        // reviewed collection, including the answer saved above.
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
        let passwordField = element(app, "settings.anki.password")
        try click(passwordField, app, bundle)
        passwordField.typeText(password)
        try click(element(app, "settings.anki.login"), app, bundle)
        waitForText(element(app, "settings.anki.result"), "Logged in")
        waitForText(element(app, "settings.anki.loginIdentity"), username)
        waitUntilReady(element(app, "settings.anki.logout"), app)
        XCTAssertFalse(element(app, "settings.anki.confirm").exists, "Login must not prepare or transfer.")
    }
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
        #else
        let containers = [app.collectionViews, app.tables, app.scrollViews]
        #endif
        let scroll = containers.map { $0.containing(.any, identifier: target.identifier).firstMatch }
            .first(where: { !target.identifier.isEmpty && $0.exists })
            ?? containers.map { $0.firstMatch }.first(where: { $0.exists })
        for _ in 0..<16 {
            if target.exists && target.isHittable { break }
            guard let scroll, scroll.exists else { break }
            #if os(macOS)
            let towardsBottom = !target.exists || target.frame.maxY > scroll.frame.maxY
            scroll.scroll(byDeltaX: 0, deltaY: (towardsBottom ? -1 : 1) * scroll.frame.height / 2)
            #else
            var viewport = scroll.frame.intersection(app.frame)
            let keyboard = app.keyboards.firstMatch
            if keyboard.exists, viewport.intersects(keyboard.frame) {
                viewport.size.height = max(0, min(viewport.maxY, keyboard.frame.minY) - viewport.minY)
            }
            guard !viewport.isNull, viewport.width > 0, viewport.height > 0 else { break }
            let towardsBottom = !target.exists || target.frame.maxY > viewport.maxY
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
        XCTAssertEqual(outcome, .completed, "Element not visible: \(target.identifier)")
    }
    private func waitUntilReady(_ target: XCUIElement, _ app: XCUIApplication) {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in target.exists && target.isEnabled }, object: nil)
        let outcome = XCTWaiter().wait(for: [ready], timeout: 30)
        if outcome != .completed { attach(app, "rf4-ready-failure", target: target) }
        XCTAssertEqual(outcome, .completed, "Action unavailable: \(target.identifier)")
    }
    private func replaceText(_ field: XCUIElement, app: XCUIApplication, with value: String) {
        #if os(macOS)
        field.typeKey("a", modifierFlags: .command); field.typeText(value)
        #else
        let count = (field.value as? String)?.count ?? 0
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + value)
        #endif
    }
    private func waitForText(_ target: XCUIElement, _ text: String) {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            target.exists && (target.label + " " + String(describing: target.value ?? "")).contains(text)
        }, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [ready], timeout: 45), .completed)
    }
    private func openSettings(_ app: XCUIApplication, _ bundle: String) throws {
        try openSection(app, bundle, name: "settings", title: "Settings")
    }
    private func openHome(_ app: XCUIApplication, _ bundle: String) throws {
        try openSection(app, bundle, name: "home", title: "Decks")
    }
    private func openSection(_ app: XCUIApplication, _ bundle: String, name: String, title: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
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
        lines.append("resultPresent=\(element(app, "settings.anki.result").exists)")
        let report = XCTAttachment(string: lines.joined(separator: "\n"))
        report.name = name; report.lifetime = .keepAlways; add(report)
    }
}
