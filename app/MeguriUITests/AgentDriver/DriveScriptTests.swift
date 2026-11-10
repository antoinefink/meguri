import Foundation
import XCTest

#if os(macOS)
/// Exercise the production polling and candidate classifier with a controlled
/// publication clock. These are observations, never activation retries.
@MainActor
final class NativeReplayWaitTests: XCTestCase {
    private let id = "review.card.replay.q1"
    private var binding: NativeWebCapture.Binding {
        .init(page: "review.card.page.a", document: "synthetic-document", card: 1,
              generation: 2, side: "question", root: "meguri-media://media/",
              generatedIDs: [id])
    }

    private func link(type: String = "link", error: Int = 0, wrongType: Bool = false,
                      domID: String? = "review.card.replay.q1") -> HierarchyExport.Node {
        var node = HierarchyExport.Node(type: type, identifier: "", label: "Play audio", value: nil,
            frame: CGRect(x: 10, y: 10, width: 44, height: 44), hitPoint: CGPoint(x: 32, y: 32),
            enabled: true, selected: false, focused: false, children: [])
        node.nativeAX = true
        node.axDOMIdentifier = domID
        node.axDOMIdentifierError = error
        node.axDOMIdentifierWrongType = wrongType
        return node
    }

    func testGoneWaitObservesPresentThenActualRemoval() throws {
        let start = Date(timeIntervalSince1970: 100)
        var now = start
        var nodes = [link()]
        var observations: [Bool] = []
        try NativeReplayWait.until(id: id, timeout: 0.3, gone: true, now: { now }, pause: { delay in
            now = now.addingTimeInterval(delay)
            if now.timeIntervalSince(start) >= 0.09 { nodes.removeAll() }
        }) { deadline in
            XCTAssertEqual(deadline, start.addingTimeInterval(0.3))
            let present = try NativeWebCapture.publishedLinkIndex(self.id, binding: self.binding, nodes: nodes) != nil
            observations.append(present)
            return present
        }
        XCTAssertEqual(observations.first, true)
        XCTAssertEqual(observations.last, false)
        XCTAssertGreaterThan(observations.count, 1)
        XCTAssertLessThan(now.timeIntervalSince(start), 0.3)
    }

    func testAppearanceWaitObservesPublicationWithinRequestedTimeout() throws {
        let start = Date(timeIntervalSince1970: 100)
        var now = start
        var nodes: [HierarchyExport.Node] = []
        var observations: [Bool] = []
        try NativeReplayWait.until(id: id, timeout: 0.3, gone: false, now: { now }, pause: { delay in
            now = now.addingTimeInterval(delay)
            if now.timeIntervalSince(start) >= 0.09 { nodes = [self.link()] }
        }) { deadline in
            XCTAssertEqual(deadline, start.addingTimeInterval(0.3))
            let present = try NativeWebCapture.publishedLinkIndex(self.id, binding: self.binding, nodes: nodes) != nil
            observations.append(present)
            return present
        }
        XCTAssertEqual(observations.first, false)
        XCTAssertEqual(observations.last, true)
        XCTAssertGreaterThan(observations.count, 1)
        XCTAssertLessThan(now.timeIntervalSince(start), 0.3)
    }

    func testBothWaitDirectionsStopAtTheirRequestedDeadline() {
        for gone in [false, true] {
            let start = Date(timeIntervalSince1970: 100)
            var now = start
            var observations = 0
            XCTAssertThrowsError(try NativeReplayWait.until(id: id, timeout: 0.12, gone: gone,
                now: { now }, pause: { now = now.addingTimeInterval($0) }) { deadline in
                XCTAssertEqual(deadline, start.addingTimeInterval(0.12))
                XCTAssertLessThan(now, deadline)
                observations += 1
                return gone
            })
            XCTAssertGreaterThan(observations, 1)
            XCTAssertEqual(now, start.addingTimeInterval(0.12))
        }
    }

    func testPublishedInvalidCandidatesFailImmediatelyInBothDirections() {
        let cases = [[link(domID: nil)], [link(error: -25205)], [link(wrongType: true)],
                     [link(type: "button")], [link(), link()]]
        for gone in [false, true] {
            for nodes in cases {
                var pauses = 0
                var observations = 0
                XCTAssertThrowsError(try NativeReplayWait.until(id: id, timeout: 2, gone: gone,
                    pause: { _ in pauses += 1 }) { _ in
                    observations += 1
                    return try NativeWebCapture.publishedLinkIndex(self.id, binding: self.binding, nodes: nodes) != nil
                })
                XCTAssertEqual(observations, 1)
                XCTAssertEqual(pauses, 0)
            }
        }
    }

    func testStaleObservationFailsWithoutWaitingOrRetry() {
        for gone in [false, true] {
            var pauses = 0
            var observations = 0
            XCTAssertThrowsError(try NativeReplayWait.until(id: id, timeout: 2, gone: gone,
                pause: { _ in pauses += 1 }) { _ in
                observations += 1
                throw NativeWebCapture.Failure("Stale native replay wait binding")
            })
            XCTAssertEqual(observations, 1)
            XCTAssertEqual(pauses, 0)
        }
    }
}
#endif

/// The drive script format (tools/drive-scripts/AGENTS.md). These tests only
/// parse JSON; they launch no app.
final class DriveScriptTests: XCTestCase {
    private func parse(_ json: String) throws(DriveScriptError) -> DriveScript {
        try DriveScript(json: Data(json.utf8))
    }

    private func error(_ json: String) -> String {
        do {
            _ = try parse(json)
            return "<parsed>"
        } catch {
            return error.message
        }
    }

    func testEveryActionParses() throws {
        let script = try parse("""
        {"name": "all", "description": "every action", "platforms": ["ios", "mac"], "steps": [
          {"launch": {"route": "sample/sheet", "fixture": "app/Fixtures/TestHooks/directory", "now": "2030-01-15T10:00:00Z",
                      "timeZone": "Asia/Tokyo", "locale": "en_US", "animations": false, "reduceMotion": true,
                      "reduceTransparency": false, "windowSize": "minimum", "ankiConnectPort": 18765,
                      "migakuPort": 54432, "arguments": ["-Extra", "1"]}},
          {"tap": "sample.sheet.done", "transition": "dismiss-sheet"},
          {"type": {"id": "editor.field.front", "text": ""}},
          {"swipe": {"id": "review.card.face", "direction": "left"}},
          {"scroll": {"id": "browser.list.rows", "direction": "down"}},
          {"wait": "home.placeholder.title"},
          {"wait": {"id": "sample.sheet.title", "timeout": 2.5, "gone": true}},
          {"snapshot": "home-after-dismiss"},
          {"resize": "1000x700"}
        ]}
        """)
        var launch = DriveScript.Launch()
        launch.route = "sample/sheet"
        launch.fixture = "app/Fixtures/TestHooks/directory"
        launch.now = "2030-01-15T10:00:00Z"
        launch.timeZone = "Asia/Tokyo"
        launch.locale = "en_US"
        launch.animations = false
        launch.reduceMotion = true
        launch.reduceTransparency = false
        launch.windowSize = "minimum"
        launch.ankiConnectPort = 18765
        launch.migakuPort = 54432
        launch.arguments = ["-Extra", "1"]
        XCTAssertEqual(script, DriveScript(name: "all", steps: [
            .init(action: .launch(launch)),
            .init(action: .tap(id: "sample.sheet.done"), transition: "dismiss-sheet"),
            .init(action: .type(id: "editor.field.front", text: "")),
            .init(action: .swipe(id: "review.card.face", direction: .left)),
            .init(action: .scroll(id: "browser.list.rows", direction: .down)),
            .init(action: .wait(id: "home.placeholder.title", timeout: 10, gone: false)),
            .init(action: .wait(id: "sample.sheet.title", timeout: 2.5, gone: true)),
            .init(action: .snapshot(name: "home-after-dismiss")),
            .init(action: .resize(size: "1000x700")),
        ]))
        XCTAssertEqual(script.steps.map(\.action.kind), ["launch", "tap", "type", "swipe", "scroll", "wait", "wait", "snapshot", "resize"])
    }

    func testTheExampleScriptsParse() throws {
        let build = try BuildUnderTest.current()
        let directory = build.repositoryRoot.appending(path: "tools/drive-scripts/examples", directoryHint: .isDirectory)
        let names = try FileManager.default.contentsOfDirectory(atPath: directory.path(percentEncoded: false))
            .filter { $0.hasSuffix(".json") }
        XCTAssertFalse(names.isEmpty, "no example scripts in \(directory.path(percentEncoded: false))")
        for name in names {
            XCTAssertNoThrow(try DriveScript(contentsOf: directory.appending(path: name)), name)
        }
    }

    func testMalformedScriptsAreRejectedWithTheReason() {
        let launch = #"{"launch": {}}"#
        let cases: [(String, String)] = [
            ("[]", "the script must be a JSON object"),
            ("{\"name\": \"x\"}", "\"steps\" must be a non-empty array"),
            ("{\"name\": \"\", \"steps\": [\(launch)]}", "\"name\" must be a non-empty string"),
            ("{\"name\": \"x\", \"steps\": [\(launch)], \"step\": []}", "unknown top-level key(s): step"),
            ("{\"name\": \"x\", \"steps\": [{\"tap\": \"a.b.c\"}]}", "step 1 must be a launch step, found tap"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"tap\": \"a.b.c\", \"wait\": \"a.b.c\"}]}", "step 2: needs exactly one action key"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"click\": \"a.b.c\"}]}", "step 2: unknown key(s) click"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"tap\": 3}]}", "step 2: tap takes an accessibility identifier string"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"swipe\": {\"id\": \"a.b.c\", \"direction\": \"north\"}}]}",
             "step 2: swipe.direction must be one of up, down, left, right, got 'north'"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"wait\": {\"id\": \"a.b.c\", \"timeout\": 0}}]}", "step 2: wait.timeout must be positive"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"wait\": {\"id\": \"a.b.c\", \"gone\": 1}}]}", "step 2: wait.gone must be true or false"),
            ("{\"name\": \"x\", \"steps\": [{\"launch\": {\"animations\": 0}}]}", "step 1: launch.animations must be true or false"),
            ("{\"name\": \"x\", \"steps\": [{\"launch\": {\"migakuPort\": 1.5}}]}", "step 1: launch.migakuPort must be a whole number"),
            ("{\"name\": \"x\", \"steps\": [{\"launch\": {\"routes\": \"a/b\"}}]}", "step 1: launch has unknown key(s) routes"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"snapshot\": \"Sheet Open\"}]}", "step 2: snapshot names use lowercase letters"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"resize\": \"huge\"}]}", "step 2: resize takes minimum, default, large or <width>x<height>, got 'huge'"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"tap\": \"a.b.c\", \"transition\": \"\"}]}", "step 2: \"transition\" must be a non-empty string"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"tap\": \"a.b.c\", \"transition\": \"Dismiss Sheet\"}]}",
             "step 2: transition names use lowercase letters, digits and '-', got 'Dismiss Sheet'"),
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"type\": {\"id\": \"a.b.c\"}}]}", "step 2: type needs text"),
        ]
        for (json, expected) in cases {
            let message = error(json)
            XCTAssertTrue(message.hasPrefix(expected), "\(json)\n  got: \(message)\n  expected prefix: \(expected)")
        }
    }

    private func inline(_ data: Data) -> [String: String] {
        ["MEGURI_DRIVE_SCRIPT_BASE64": data.base64EncodedString()]
    }

    private func refused(_ environment: [String: String], _ expected: DriveScriptInput.Failure,
                         file: StaticString = #filePath, line: UInt = #line) {
        do {
            _ = try DriveScriptInput.load(environment, physical: true)
            XCTFail("private input was accepted", file: file, line: line)
        } catch {
            XCTAssertEqual(error as? DriveScriptInput.Failure, expected, file: file, line: line)
            XCTAssertEqual(String(describing: error), expected.rawValue, file: file, line: line)
        }
    }

    func testPhysicalInlineUsesTheExistingParserAndPathTransportStaysAvailable() throws {
        let data = Data(#"{"name":"inspect","steps":[{"launch":{}},{"snapshot":"initial"}]}"#.utf8)
        guard case let .inline(script)? = try DriveScriptInput.load(inline(data), physical: true) else {
            return XCTFail("inline script missing")
        }
        XCTAssertEqual(script, try DriveScript(json: data))
        guard case let .path(url)? = try DriveScriptInput.load(["MEGURI_DRIVE_SCRIPT": "/synthetic/script.json"], physical: false) else {
            return XCTFail("existing path missing")
        }
        XCTAssertEqual(url.path, "/synthetic/script.json")
        XCTAssertNil(try DriveScriptInput.load([:], physical: false))
    }

    func testPhysicalInlineRejectsSourcesEncodingSizeAndJSONWithFixedCategories() {
        let valid = #"{"name":"inspect","steps":[{"launch":{}}]}"#
        refused([:], .missing)
        refused(["MEGURI_DRIVE_REQUIRED": "1"], .missing)
        refused(["MEGURI_DRIVE_SCRIPT_BASE64": ""], .missing)
        refused(["MEGURI_DRIVE_SCRIPT": "/private/path"], .policy)
        refused(["MEGURI_DRIVE_SCRIPT": "", "MEGURI_DRIVE_SCRIPT_BASE64": Data(valid.utf8).base64EncodedString()], .conflicting)
        refused(["MEGURI_DRIVE_SCRIPT_BASE64": "PRIVATE_TEXT!"], .encoding)
        refused(["MEGURI_DRIVE_SCRIPT_BASE64": String(repeating: "A", count: 21_849)], .oversized)
        refused(inline(Data(repeating: 32, count: 16_385)), .oversized)
        refused(inline(Data([0xff])), .encoding)
        refused(inline(Data("PRIVATE_TEXT".utf8)), .json)
        refused(inline(Data(#"{"name":"x","steps":[{"launch":{}},{"snapshot":"PRIVATE_TEXT"}]}"#.utf8)), .grammar)
        refused(inline(Data(#"{"name":"x","PRIVATE_TEXT":null,"steps":[{"launch":{}}]}"#.utf8)), .grammar)
        refused(inline(Data(#"{"name":"x","steps":[{"launch":{}},{"swipe":{"id":"a","direction":"PRIVATE_TEXT"}}]}"#.utf8)), .grammar)
    }

    func testPhysicalInlineInspectsEveryRawLaunchBeforeDefaultsAndRefusesMotion() throws {
        let overrides = [#"null"#, #"[]"#, #"{"route":""}"#, #"{"arguments":[]}"#,
                         #"{"animations":false}"#, #"{"fixture":null}"#, #"{"reset":false}"#,
                         #"{"import":"PRIVATE_TEXT"}"#, #"{"PRIVATE_TEXT":null}"#]
        for launch in overrides {
            let json = "{\"name\":\"x\",\"steps\":[{\"launch\":{}},{\"launch\":\(launch)}]}"
            refused(inline(Data(json.utf8)), .policy)
        }
        for step in [#"{"resize":"default"}"#, #"{"snapshot":"x","transition":null}"#,
                     #"{"snapshot":"x","transition":"motion"}"#] {
            let json = "{\"name\":\"x\",\"steps\":[{\"launch\":{}},\(step)]}"
            refused(inline(Data(json.utf8)), .policy)
        }
        let valid = #"{"name":"x","steps":[{"launch":{}}]}"#
        var environment = inline(Data(valid.utf8))
        environment["MEGURI_DRIVE_FRAMES"] = "1"
        refused(environment, .policy)
        refused(inline(Data(#"{"name":"x","steps":[{"snapshot":"x"}]}"#.utf8)), .grammar)
        // The inclusive decoded limit, including whitespace, remains valid.
        let padded = valid + String(repeating: " ", count: 16_384 - valid.utf8.count)
        guard case .inline? = try DriveScriptInput.load(inline(Data(padded.utf8)), physical: true) else {
            return XCTFail("bounded inline script missing")
        }
    }

    func testPNGWidthComesFromTheHeader() {
        // The 24 bytes up to IHDR's width of a 1206 x 2622 PNG.
        var header: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]
        header += [0x00, 0x00, 0x04, 0xB6, 0x00, 0x00, 0x0A, 0x3E]
        XCTAssertEqual(HierarchyExport.pngPixelWidth(Data(header)), 1206)
        XCTAssertNil(HierarchyExport.pngPixelWidth(Data("not a png at all, really".utf8)))
    }

    func testPreservePolicyRequiresPhysicalDebugDevelopmentProduct() throws {
        let environment = ["MEGURI_DRIVE_KEEP_APP_RUNNING": "1"]
        XCTAssertTrue(try DriveAppLifecycle.preserve(environment, physical: true, configuration: .debug,
                                                     bundleIdentifier: "com.antoinefink.meguri.dev"))
        for configuration in BuildUnderTest.Configuration.allCases where configuration != .debug {
            XCTAssertThrowsError(try DriveAppLifecycle.preserve(environment, physical: true,
                configuration: configuration, bundleIdentifier: "com.antoinefink.meguri.dev"))
        }
        XCTAssertThrowsError(try DriveAppLifecycle.preserve(environment, physical: false, configuration: .debug,
                                                            bundleIdentifier: "com.antoinefink.meguri.dev"))
        XCTAssertThrowsError(try DriveAppLifecycle.preserve(environment, physical: true, configuration: .debug,
                                                            bundleIdentifier: "com.antoinefink.meguri"))
        XCTAssertFalse(try DriveAppLifecycle.preserve([:], physical: true, configuration: .debug,
                                                      bundleIdentifier: "com.antoinefink.meguri.dev"))
        refused(["MEGURI_DRIVE_KEEP_APP_RUNNING": "1"], .policy)
        refused(["MEGURI_DRIVE_KEEP_APP_RUNNING": "unknown"], .policy)
        var nonphysical = inline(Data(#"{"name":"x","steps":[{"launch":{}}]}"#.utf8))
        nonphysical["MEGURI_DRIVE_KEEP_APP_RUNNING"] = "1"
        XCTAssertThrowsError(try DriveScriptInput.load(nonphysical, physical: false))
    }

    func testPreserveEveryLaunchActivatesWithoutStateOrLaunchFallback() {
        var calls: [String] = []
        // Includes first/later launches and absent/unknown state: state is not
        // an input to this policy and cannot select a destructive fallback.
        for _ in 0..<4 {
            DriveAppLifecycle.start(preserve: true, activate: { calls.append("activate") }, launch: { calls.append("launch") })
        }
        XCTAssertEqual(calls, Array(repeating: "activate", count: 4))
        DriveAppLifecycle.start(preserve: false, activate: { calls.append("activate") }, launch: { calls.append("launch") })
        XCTAssertEqual(calls.last, "launch")
    }

    func testPreserveCleanupRetainsTargetOnSuccessStepAndCaptureFailure() {
        enum SyntheticFailure: Error { case step, capture }
        for preserve in [false, true] {
            for failure in [nil, SyntheticFailure.step, .capture] {
                var terminations = 0
                func complete() throws {
                    defer { DriveAppLifecycle.finish(preserve: preserve) { terminations += 1 } }
                    if let failure { throw failure }
                }
                if failure != nil {
                    XCTAssertThrowsError(try complete())
                } else {
                    XCTAssertNoThrow(try complete())
                }
                XCTAssertEqual(terminations, preserve ? 0 : 1)
            }
        }
    }

    @MainActor
    func testPreservationIntentCannotClaimActivationOrContinuity() throws {
        let log = AgentDriver.Log(script: "physical-drive", path: "", platform: "ios", kept_running: true)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(log)) as? [String: Any])
        XCTAssertEqual(object["kept_running"] as? Bool, true)
        XCTAssertEqual(object["activated_existing"] as? Bool, false)
        XCTAssertEqual(object["activation_completed"] as? Bool, false)
        var activated = log
        activated.activation_completed = true
        let observation = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(activated)) as? [String: Any])
        XCTAssertEqual(observation["activation_completed"] as? Bool, true)
        XCTAssertEqual(observation["activated_existing"] as? Bool, false)
        XCTAssertNil(object["continuity"])
    }
}
