import XCTest
#if os(macOS)
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

        let xcode = XCUIApplication(bundleIdentifier: "com.apple.dt.Xcode")
        guard xcode.state != .notRunning else {
            XCTFail("Xcode must already be running in the VM")
            return
        }
        xcode.activate()
        func dumpTree() {
            print("MEGURI_PLAN_EDITOR_WINDOW_COUNT \(xcode.windows.count)")
            let roots = (xcode.windows.allElementsBoundByIndex + xcode.dialogs.allElementsBoundByIndex)
                .prefix(3).map { $0.debugDescription }
            let popUpMenus = xcode.popUpButtons.allElementsBoundByIndex
                .filter { $0.isHittable }.map { $0.debugDescription }
            let tree = (roots + popUpMenus).joined(separator: "\n")
            print("MEGURI_PLAN_EDITOR_TREE\n\(tree.prefix(60_000))")
        }
        dumpTree()

        if ProcessInfo.processInfo.environment["MEGURI_PLAN_EDITOR_DISABLE_IOS_COMPONENT"] == "1" {
            XCTAssertTrue(xcode.staticTexts["iOS 27.0"].exists)
            XCTAssertEqual(xcode.switches.count, 4)
            let firstSwitch = xcode.switches.firstMatch
            XCTAssertEqual(firstSwitch.value as? String, "1")
            try xcode.bringToFront(bundleIdentifier: "com.apple.dt.Xcode")
            firstSwitch.click()
            dumpTree()
        }

        if let label = ProcessInfo.processInfo.environment["MEGURI_PLAN_EDITOR_CLICK"] {
            let predicate = NSPredicate(format: "label == %@", label)
            let matches = (xcode.buttons.matching(predicate).allElementsBoundByIndex
                + xcode.popUpButtons.matching(predicate).allElementsBoundByIndex
                + xcode.menuItems.matching(predicate).allElementsBoundByIndex)
                .filter { $0.isHittable }
            XCTAssertEqual(matches.count, 1, "Expected exactly one matching Xcode control")
            let control = try XCTUnwrap(matches.first)
            XCTAssertTrue(control.exists)
            try xcode.bringToFront(bundleIdentifier: "com.apple.dt.Xcode")
            control.click()
            dumpTree()
        }
        #else
        throw XCTSkip("Test plan editor probe requires macOS")
        #endif
    }
}
