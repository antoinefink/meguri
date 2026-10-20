import XCTest
#if os(macOS)
import AppKit
import Darwin
#endif

@MainActor
final class TestPlanEditorProbeTests: XCTestCase {
    func testInspectPlanEditor() throws {
        guard ProcessInfo.processInfo.environment["MEGURI_PLAN_EDITOR_PROBE"] == "1" else {
            throw XCTSkip("Test plan editor probe is opt-in")
        }
        #if os(macOS)
        var isVM: Int32 = 0
        var size = MemoryLayout<Int32>.size
        guard sysctlbyname("kern.hv_vmm_present", &isVM, &size, nil, 0) == 0,
              size == MemoryLayout<Int32>.size else {
            throw XCTSkip("VM detection is unsupported")
        }
        try XCTSkipUnless(isVM == 1, "Test plan editor probe requires a VM")
        continueAfterFailure = false

        let environment = ProcessInfo.processInfo.environment
        let notices = environment["MEGURI_PLAN_EDITOR_NOTICES"] == "1"
        let bundle = notices ? "com.apple.UserNotificationCenter" : "com.apple.dt.Xcode"
        let app = XCUIApplication(bundleIdentifier: bundle)
        guard app.state != .notRunning else { XCTFail("Target must already be running in the VM"); return }
        struct Action: Decodable {
            let action: String
            let type: String?
            let label: String?
            let identifier: String?
            let index: Int?
            let rootIndex: Int?
            let expectText: String?
            let value: String?
        }
        let types: [String: XCUIElement.ElementType] = [
            "any": .any, "radio": .radioButton,
            "button": .button, "popup": .popUpButton, "menuItem": .menuItem,
            "checkbox": .checkBox, "switch": .switch, "staticText": .staticText,
            "disclosure": .disclosureTriangle, "textField": .textField, "searchField": .searchField
        ]
        let data = Data((environment["MEGURI_PLAN_EDITOR_ACTIONS"] ?? "[]").utf8)
        let payload = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        let keys: Set<String> = ["action", "type", "label", "identifier", "index", "rootIndex", "expectText", "value"]
        for entry in payload { XCTAssertTrue(Set(entry.keys).isSubset(of: keys), "Unknown action field") }
        let actions = try JSONDecoder().decode([Action].self, from: data)
        XCTAssertLessThanOrEqual(actions.count, 12)
        for action in actions {
            XCTAssertTrue(["click", "text", "save", "key"].contains(action.action), "Unknown action")
            if action.action == "key" {
                XCTAssertFalse(notices, "Key actions are Xcode-only")
                XCTAssertTrue(action.type == nil && action.label == nil && action.identifier == nil
                              && action.index == nil, "Key accepts only value, scope and expectText")
                XCTAssertTrue(action.value == "return" || action.value == "escape", "Unsupported key")
                XCTAssertFalse(try XCTUnwrap(action.expectText).isEmpty, "Key requires nonempty expectText")
            } else if action.action != "save" {
                XCTAssertNotNil(action.type.flatMap { types[$0] }, "Unknown control type")
                XCTAssertTrue(action.label != nil || action.identifier != nil || action.index != nil,
                              "An explicit selector is required")
            } else {
                XCTAssertTrue(action.type == nil && action.label == nil && action.identifier == nil
                              && action.index == nil && action.value == nil, "Save accepts only scope and expectText")
            }
            if let index = action.index { XCTAssertGreaterThanOrEqual(index, 0) }
            if let index = action.rootIndex { XCTAssertGreaterThanOrEqual(index, 0) }
            if action.action == "text" {
                XCTAssertTrue(action.type == "textField" || action.type == "searchField")
                XCTAssertLessThanOrEqual(try XCTUnwrap(action.value).count, 3_000)
            } else if action.action != "key" {
                XCTAssertNil(action.value, "Only text and key actions accept a value")
            }
        }
        if !notices { app.activate() }
        func roots() -> [XCUIElement] {
            app.windows.allElementsBoundByIndex + app.dialogs.allElementsBoundByIndex
        }
        let dumpLimit = 60_000 / (1 + actions.count
            + (environment["MEGURI_PLAN_EDITOR_DISABLE_IOS_COMPONENT"] == "1" ? 1 : 0))
        func dumpTree() {
            let tree = roots().enumerated().map { "ROOT \($0.offset)\n\($0.element.debugDescription)" }
            // Printed menu metadata is diagnostic only; actions use concrete selectors and hittability checks.
            let menus = app.debugDescription.components(separatedBy: "\n").filter { line in
                let element = line.trimmingCharacters(in: .whitespaces)
                return ["Menu,", "MenuItem,", "MenuButton,", "PopUpButton,"].contains {
                    element.hasPrefix($0)
                } && line.contains("{{") && !line.contains(", {0.0, 0.0}}")
            }
            let output = ("MEGURI_PLAN_EDITOR_TREE\n" + (tree + menus).joined(separator: "\n"))
                .prefix(dumpLimit - 1)
            print(output)
        }
        func normalWindow() -> Bool { !notices && app.windows.count > 0 && app.dialogs.count == 0 }
        func focus(_ control: XCUIElement) throws {
            if normalWindow() { try app.bringToFront(bundleIdentifier: bundle) }
            else if !notices && app.state != .runningForeground { app.activate() }
            XCTAssertTrue(control.isHittable, "Control must be hittable before mutation")
        }
        dumpTree()
        if environment["MEGURI_PLAN_EDITOR_DISABLE_IOS_COMPONENT"] == "1" {
            XCTAssertFalse(notices, "The component toggle is Xcode-only")
            XCTAssertTrue(app.staticTexts["iOS 27.0"].exists)
            XCTAssertEqual(app.switches.count, 4)
            let firstSwitch = app.switches.firstMatch
            let switchValue = firstSwitch.value
            XCTAssertTrue(
                (switchValue as? String) == "1"
                    || (switchValue as? NSNumber) == NSNumber(value: 1)
            )
            try focus(firstSwitch)
            firstSwitch.click()
            dumpTree()
        }
        for action in actions {
            let root: XCUIElement
            if let index = action.rootIndex {
                let currentRoots = roots()
                XCTAssertTrue(currentRoots.indices.contains(index), "rootIndex is out of bounds")
                root = currentRoots[index]
            } else { root = app }
            if let expected = action.expectText {
                XCTAssertTrue(root.debugDescription.contains(expected), "Expected text is absent")
            }
            if action.action == "save" {
                XCTAssertTrue(normalWindow(), "Save requires a normal Xcode window")
                try app.bringToFront(bundleIdentifier: bundle)
                app.typeKey("s", modifierFlags: .command)
            } else if action.action == "key" {
                if normalWindow() { try app.bringToFront(bundleIdentifier: bundle) }
                else { app.activate() }
                XCTAssertEqual(NSWorkspace.shared.frontmostApplication?.bundleIdentifier, "com.apple.dt.Xcode")
                XCTAssertEqual(app.state, .runningForeground)
                app.typeKey(action.value == "return" ? XCUIKeyboardKey.return.rawValue : XCUIKeyboardKey.escape.rawValue,
                            modifierFlags: [])
            } else {
                let type = try XCTUnwrap(action.type.flatMap { types[$0] })
                var query = root.descendants(matching: type)
                if let label = action.label { query = query.matching(NSPredicate(format: "label == %@", label)) }
                else if let id = action.identifier { query = query.matching(NSPredicate(format: "identifier == %@", id)) }
                let matches = query.allElementsBoundByIndex.filter { $0.isHittable }
                let index = action.index ?? 0
                if action.index == nil { XCTAssertEqual(matches.count, 1, "Selector must be unique") }
                XCTAssertTrue(matches.indices.contains(index), "Control index is out of bounds")
                let control = matches[index]
                try focus(control)
                control.click()
                if action.action == "text" {
                    control.typeKey("a", modifierFlags: .command)
                    control.typeText(try XCTUnwrap(action.value))
                }
            }
            dumpTree()
        }
        #else
        throw XCTSkip("Test plan editor probe requires macOS")
        #endif
    }
}
