import Foundation
import XCTest
#if os(macOS)

/// Out-of-process proof of D4 native web identity and the same AX link press.
@MainActor
final class MacReplayAccessibilityTests: XCTestCase {
    private var actionNumber = 0

    func testNativeReplayWaitObservesDisappearanceAndPublicationWithinTimeout() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        _ = try XCTUnwrap(build.configuration == .debug && bundle.hasSuffix(".dev") ? true : nil)
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, windowSize: "1000x760",
            route: "card/replay-accessibility").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)
        try wait(app) { $0["ready"] as? Bool == true }
        let native = try XCTUnwrap(HierarchyExport.tree(app).nativeWeb)
        let id = try XCTUnwrap(native.binding.generatedIDs.first)
        _ = try native.uniqueLink(id)
        // This fixture action schedules visibility only, before either wait.
        // Neither wait can press a link or use DOM facts as native proof.
        element(app, "testhooks.replay.cyclePublication").clickOrTap()
        var disappearance: [Bool] = []
        try NativeWebCapture.wait(id, in: app, binding: native.binding, timeout: 10, gone: true) {
            disappearance.append($0)
        }
        XCTAssertEqual(disappearance.first, true, "Gone wait must first observe the actual published link")
        XCTAssertEqual(disappearance.last, false, "Gone wait must observe actual native disappearance")
        var appearance: [Bool] = []
        try NativeWebCapture.wait(id, in: app, binding: native.binding, timeout: 10, gone: false) {
            appearance.append($0)
        }
        XCTAssertEqual(appearance.first, false, "Appearance wait must begin before real publication")
        XCTAssertEqual(appearance.last, true)
        let restored = try XCTUnwrap(HierarchyExport.tree(app).nativeWeb)
        XCTAssertEqual(restored.binding, native.binding)
        _ = try restored.uniqueLink(id)
        let observed = try report(app)
        XCTAssertEqual(observed["received"] as? [String], [])
        XCTAssertEqual(observed["commands"] as? [String], [])
        XCTAssertEqual(observed["actions"] as? [String], [])
    }

    func testGeneratedReplaysKeepNativeIdentityLocalizationAndExactCommands() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        let bundle = try build.bundleIdentifier
        _ = try XCTUnwrap(build.configuration == .debug && bundle.hasSuffix(".dev") ? true : nil,
            "Replay native proof requires the isolated Debug fixture")
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(locale: "en_US", animations: false, windowSize: "1000x760",
            route: "card/replay-accessibility").arguments
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        try app.bringToFront(bundleIdentifier: bundle)

        let front = ["play:q:00", "play:q:1", "play:q:00"]
        let back = front + front + ["play:a:000"]
        var commands: [String] = []
        var actions: [String] = []
        var checkpoint = 0
        for (stage, side) in ["question", "answer", "question", "answer"].enumerated() {
            let deadline = Date().addingTimeInterval(10)
            let card = stage < 2 ? 1 : 2
            if stage > 0 {
                try press(element(app, "testhooks.replay.\(side)"), app: app, bundle: bundle)
            }
            try wait(app, deadline: deadline) { $0["ready"] as? Bool == true && $0["side"] as? String == side && $0["card"] as? Int == card }
            let stageReport = try report(app)
            // The production CardBehavior catalog currently supplies English.
            // Use its actual localized value, not DOM copy, for every leaf.
            let label = try XCTUnwrap(stageReport["catalogLabel"] as? String)
            XCTAssertEqual(label, "Play audio")
            let currentCommands = side == "question" ? front : back
            let ids = currentCommands.indices.map { "review.card.replay.\(side == "question" ? "q\($0 + 1)" : "a\($0)")" }
            let web = app.webViews
            let nativeLinks = web.links
            let expectedIDs = ids + (side == "question" ? ["review.card.replay.q0"] : [])
            // Native publication shares the side's readiness budget. Do not
            // wait for IDs/names: a published leaf missing either must fail.
            let publication = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                web.count == 1 && nativeLinks.count == expectedIDs.count
            }, object: nil)
            let published = XCTWaiter.wait(for: [publication], timeout: max(0, deadline.timeIntervalSinceNow))
            let tree = try HierarchyExport.tree(app)
            try attach(tree, app: app, stage: stage)
            XCTAssertEqual(published, .completed, "Native links must publish within the side's 10-second budget")
            XCTAssertEqual(web.count, 1, "Idle page must not leak into the native tree")
            XCTAssertEqual(nativeLinks.count, expectedIDs.count, "Empty/partial native trees cannot establish replay proof")
            XCTAssertEqual(Set(expectedIDs).count, expectedIDs.count)
            let native = try XCTUnwrap(tree.nativeWeb)
            XCTAssertEqual(native.binding.generatedIDs, ids, "IDs must come from the current trusted payload")
            // Keep XCUI observations verbatim and check every published leaf's
            // visibility independently. They do not select native press targets.
            for link in nativeLinks.allElementsBoundByIndex {
                XCTAssertTrue(link.isEnabled)
                XCTAssertTrue(link.isHittable)
                XCTAssertFalse(link.frame.isEmpty)
                XCTAssertTrue(app.windows.firstMatch.frame.contains(link.frame))
            }
            if side == "question" {
                let authored = native.root.allNodes().filter { $0.type == "link" && $0.axDOMIdentifier == "review.card.replay.q0" }
                XCTAssertEqual(authored.count, 1)
                XCTAssertEqual(authored.first?.label, "Author & 猫", "An authored replay lookalike keeps its name")
            }
            for (id, command) in zip(ids, currentCommands) {
                let (_, leaf) = try native.uniqueLink(id)
                XCTAssertEqual(leaf.type, "link")
                XCTAssertEqual(leaf.axDOMIdentifier, id)
                XCTAssertEqual(leaf.label, label)
                XCTAssertTrue(leaf.enabled)
                XCTAssertNotNil(leaf.hitPoint)
                XCTAssertFalse(leaf.frame.isEmpty)
                XCTAssertTrue(app.windows.firstMatch.frame.contains(leaf.frame))
                let before = try HierarchyExport.tree(app)
                try attach(before, app: app, name: "replay-native-action-\(actionNumber)-before")
                try native.press(id, in: app)
                try attach(HierarchyExport.tree(app), app: app, name: "replay-native-action-\(actionNumber)-after")
                actionNumber += 1
                commands.append(command)
                let parts = command.split(separator: ":")
                let index = try XCTUnwrap(Int(parts[2]))
                actions.append("play:\(parts[1]):\(index)")
                // The checkpoint round trip uses the real isolated transport
                // after the native press; counts come only from actual messages
                // and the production bridge's admitted-action callback.
                try press(element(app, "testhooks.replay.checkpoint"), app: app, bundle: bundle)
                checkpoint += 1
                let expectedCheckpoint = checkpoint
                try wait(app) { $0["ready"] as? Bool == true && $0["checkpoint"] as? Int == expectedCheckpoint }
                let observed = try report(app)
                XCTAssertEqual(observed["received"] as? [String], commands, "Exactly one original transport command per native press")
                XCTAssertEqual(observed["commands"] as? [String], commands, "Exactly one validated original command, preserving leading zeroes")
                XCTAssertEqual(observed["actions"] as? [String], actions, "The admitted action must target the intended side and audio entry")
                XCTAssertEqual(observed["unmatchedActions"] as? Int, 0)
            }
        }
        XCTAssertEqual(commands.count, 20)
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).element(boundBy: 0)
    }

    private func press(_ target: XCUIElement, app: XCUIApplication, bundle: String) throws {
        try app.bringToFront(bundleIdentifier: bundle)
        XCTAssertTrue(target.isHittable)
        try attach(HierarchyExport.tree(app), app: app, name: "replay-native-action-\(actionNumber)-before")
        target.clickOrTap()
        // A side-changing action may temporarily have no ready host binding.
        // Capture the raw native/XCTest tree immediately, before waiting.
        try attach(HierarchyExport.tree(app, requireNativeBinding: false), app: app, name: "replay-native-action-\(actionNumber)-after")
        actionNumber += 1
    }

    private func report(_ app: XCUIApplication) throws -> [String: Any] {
        let target = element(app, "testhooks.replay.report")
        let json = try XCTUnwrap(target.value as? String)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    }

    private func wait(_ app: XCUIApplication, deadline: Date = Date().addingTimeInterval(10),
                      until condition: @escaping ([String: Any]) -> Bool) throws {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let value = try? self.report(app) else { return false }
            return condition(value)
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: max(0, deadline.timeIntervalSinceNow)), .completed,
            "Synthetic replay fixture must become ready within the existing 10-second bound")
        XCTAssertEqual(try report(app)["failure"] as? String, "")
    }

    private func attach(_ tree: HierarchyExport.Tree, app: XCUIApplication, stage: Int) throws {
        try attach(tree, app: app, name: "replay-native-\(stage)")
    }

    private func attach(_ tree: HierarchyExport.Tree, app: XCUIApplication, name: String) throws {
        let shot = app.windows.firstMatch.screenshot()
        let width = try XCTUnwrap(HierarchyExport.pngPixelWidth(shot.pngRepresentation))
        XCTAssertGreaterThan(tree.window.width, 0)
        let scale = Double(width) / Double(tree.window.width)
        let data = try HierarchyExport.json(tree, platform: "mac", screenshotOrigin: tree.window.origin, screenshotScale: scale)
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "\(name).json"
        attachment.lifetime = .keepAlways
        add(attachment)
        let screenshot = XCTAttachment(screenshot: shot)
        screenshot.name = name
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }
}
#endif
