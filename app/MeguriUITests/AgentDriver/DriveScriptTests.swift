import Foundation
import XCTest

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
            ("{\"name\": \"x\", \"steps\": [\(launch), {\"type\": {\"id\": \"a.b.c\"}}]}", "step 2: type needs text"),
        ]
        for (json, expected) in cases {
            let message = error(json)
            XCTAssertTrue(message.hasPrefix(expected), "\(json)\n  got: \(message)\n  expected prefix: \(expected)")
        }
    }

    func testPNGWidthComesFromTheHeader() {
        // The 24 bytes up to IHDR's width of a 1206 x 2622 PNG.
        var header: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]
        header += [0x00, 0x00, 0x04, 0xB6, 0x00, 0x00, 0x0A, 0x3E]
        XCTAssertEqual(HierarchyExport.pngPixelWidth(Data(header)), 1206)
        XCTAssertNil(HierarchyExport.pngPixelWidth(Data("not a png at all, really".utf8)))
    }
}
