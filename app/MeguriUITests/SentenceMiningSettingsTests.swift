import XCTest
#if os(macOS)
import AppKit
import Network
#endif

/// Run only through the isolated Dev Mac VM UI lane. Uses a synthetic collection
/// to exercise MeguriApp's public Settings scene and real listeners.
@MainActor final class SentenceMiningSettingsTests: XCTestCase {
    #if os(macOS)
    func testPublicSettingsIndependentCheckboxes() async throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks, try build.bundleIdentifier.hasSuffix(".dev") else {
            throw XCTSkip("Requires development hooks")
        }
        _ = try XCTUnwrap(CardSwapTimingGate.hypervisorPresent() == true && NSUserName() == "meguri" ? true : nil,
                          "Live fixed-port tests require the dedicated test VM")
        let bundle = try build.bundleIdentifier
        XCTAssertEqual(bundle, build.expectedBundleIdentifier)
        // No application bytes: an occupied port fails before we launch or click.
        for port in [UInt16(44432), 8765] {
            let open = try await miningPortIsOpen(port)
            _ = try XCTUnwrap(!open ? true : nil, "Port \(port) must be closed before this journey")
        }
        let app = XCUIApplication()
        // The existing owned launch copies a real synthetic collection into
        // disposable storage and uses normal production entry, with no route
        // or provider transport. It also supplies real destination choices.
        // MG discovers this recipe path and stages it in the isolated VM.
        // Omit port overrides to use the supported production listener defaults.
        app.launchArguments = MeguriLaunch(
            fixture: build.repositoryRoot.appending(path: "build/fixtures/recipes/local-review-1"),
            ownedProviderFixture: true, now: "2030-01-15T10:00:00+01:00", timeZone: "Europe/Paris",
            locale: "en_US", animations: false).arguments
        app.launch()
        defer { if app.state != .notRunning { app.terminate() } }
        let main = app.windows.matching(identifier: "main").firstMatch
        XCTAssertTrue(main.waitForExistence(timeout: 10))
        try app.bringToFront(bundleIdentifier: build.bundleIdentifier)
        let deck = main.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "home.sidebar.deck.")).firstMatch
        XCTAssertTrue(deck.waitForExistence(timeout: 20), "The owned synthetic collection must open through production entry")
        main.typeKey(",", modifierFlags: .command)
        let settings = app.windows.containing(NSPredicate(format: "identifier == %@", "settings.root.content")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        let row = settings.descendants(matching: .any).matching(identifier: "settings.root.integrations").firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.clickOrTap()
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
        let owner = try XCTUnwrap(running.count == 1 ? running.first : nil)
        let migaku = settings.checkBoxes["settings.integrations.migaku.enabled"]
        let ankiConnect = settings.checkBoxes["settings.integrations.ankiConnect.enabled"]
        XCTAssertTrue(migaku.waitForExistence(timeout: 5) && ankiConnect.exists)
        XCTAssertTrue(migaku.label.contains("44432") && ankiConnect.label.contains("8765"))
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 1
        configuration.timeoutIntervalForResource = 2
        let session = URLSession(configuration: configuration, delegate: MiningNoRedirects(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        func statusMatches(_ name: String, _ port: UInt16, _ enabled: Bool) -> Bool {
            let status = settings.descendants(matching: .any).matching(identifier: "settings.integrations.\(name).status").firstMatch
            guard status.exists else { return false }
            let expected = enabled ? "Listening on 127.0.0.1:\(port)" : "Off · Port \(port)"
            let allowed = enabled ? [expected, expected + " · Collection unavailable"] : [expected]
            return allowed.contains(status.label) || allowed.contains(status.value as? String ?? "")
                || status.staticTexts.matching(NSPredicate(format: "label IN %@", allowed)).count == 1
        }
        func checked(_ checkbox: XCUIElement, _ enabled: Bool) -> Bool {
            let value = checkbox.value
            return value as? String == (enabled ? "1" : "0")
                || value as? NSNumber == NSNumber(value: enabled)
        }
        func check(_ migakuOn: Bool, _ ankiConnectOn: Bool) async throws {
            let endpoints = [
                (migaku, "migaku", UInt16(44432), migakuOn), (ankiConnect, "ankiConnect", UInt16(8765), ankiConnectOn)
            ]
            for (checkbox, name, port, enabled) in endpoints {
                // Bind each request to one native control and its model's exact
                // status, rather than accepting another endpoint's status prefix.
                _ = try XCTUnwrap(settings.checkBoxes.matching(identifier: "settings.integrations.\(name).enabled").count == 1
                    && checkbox.identifier == "settings.integrations.\(name).enabled"
                    && checkbox.label == (name == "migaku" ? "Migaku (44432)" : "AnkiConnect (8765)") ? true : nil)
                let deadline = ContinuousClock.now + .seconds(5)
                while !checked(checkbox, enabled) || !statusMatches(name, port, enabled) {
                    _ = try XCTUnwrap(ContinuousClock.now < deadline ? true : nil,
                                      "\(name) must report checked=\(enabled) and its exact port status")
                    try await Task.sleep(for: .milliseconds(20))
                }
            }
            // Off is published before asynchronous socket retirement. Wait for
            // actual refusal before checking the retained sibling's live reply.
            for (_, name, port, enabled) in endpoints where !enabled {
                let deadline = ContinuousClock.now + .seconds(5)
                while try await miningPortIsOpen(port) {
                    _ = try XCTUnwrap(ContinuousClock.now < deadline ? true : nil, "\(name) listener did not close")
                    try await Task.sleep(for: .milliseconds(20))
                }
            }
            for (checkbox, name, port, enabled) in endpoints where enabled {
                let current = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
                _ = try XCTUnwrap(!owner.isTerminated && current.count == 1
                    && current.first?.processIdentifier == owner.processIdentifier
                    && owner.bundleURL?.resolvingSymlinksInPath() == build.appBundle.resolvingSymlinksInPath()
                    && checked(checkbox, true) && statusMatches(name, port, true) ? true : nil,
                    "The tested Dev app must own this control and listening status")
                // Listening is published only after the production exclusive
                // bind succeeds. Never send an RPC after Off or Port in use.
                var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/")!)
                request.httpMethod = "POST"
                request.httpBody = Data(#"{"action":"version","version":6}"#.utf8)
                request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                let (data, response) = try await session.data(for: request)
                XCTAssertEqual((response as? HTTPURLResponse)?.statusCode, 200)
                let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
                XCTAssertEqual(object["result"] as? Int, 6)
                XCTAssertTrue(object["error"] is NSNull)
            }
        }
        try await check(false, false)
        migaku.clickOrTap()
        try await check(true, false)
        ankiConnect.clickOrTap()
        try await check(true, true)
        migaku.clickOrTap()
        try await check(false, true)
        ankiConnect.clickOrTap()
        try await check(false, false)
        // Reverse the order to prove Migaku also survives its sibling's stop.
        ankiConnect.clickOrTap()
        try await check(false, true)
        migaku.clickOrTap()
        try await check(true, true)
        ankiConnect.clickOrTap()
        try await check(true, false)
        migaku.clickOrTap()
        try await check(false, false)
        XCTAssertFalse(settings.textFields["settings.integrations.port"].exists)
        settings.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10), "Native Quit must close the production owner and clean up owned storage")
    }
    #endif
}

#if os(macOS)
/// A TCP probe only: unknown listeners never receive application bytes.
/// Stream completion also releases cancellation/deadline waits in Swift 6.
private nonisolated func miningPortIsOpen(_ port: UInt16) async throws -> Bool {
    let connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
    let reply = AsyncThrowingStream<Bool, any Error>.makeStream()
    connection.stateUpdateHandler = { state in
        switch state {
        case .ready: reply.continuation.yield(true); reply.continuation.finish()
        case .waiting(let error), .failed(let error):
            if case .posix(.ECONNREFUSED) = error {
                reply.continuation.yield(false); reply.continuation.finish()
            } else { reply.continuation.finish(throwing: error) }
        case .cancelled: reply.continuation.finish(throwing: CancellationError())
        default: break
        }
    }
    connection.start(queue: DispatchQueue(label: "mining.ui.probe"))
    let deadline = Task {
        do { try await Task.sleep(for: .seconds(1)) } catch { return }
        reply.continuation.finish(throwing: URLError(.timedOut))
        connection.cancel()
    }
    defer { deadline.cancel(); connection.cancel(); reply.continuation.finish() }
    return try await withTaskCancellationHandler {
        try Task.checkCancellation()
        for try await open in reply.stream { return open }
        throw CancellationError()
    } onCancel: {
        reply.continuation.finish(throwing: CancellationError())
        connection.cancel()
    }
}

private nonisolated final class MiningNoRedirects: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                    completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
#endif
