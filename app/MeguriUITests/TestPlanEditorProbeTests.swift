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
        print("MEGURI_PLAN_EDITOR_TREE\n\(xcode.windows.debugDescription.prefix(60_000))")

        if let label = ProcessInfo.processInfo.environment["MEGURI_PLAN_EDITOR_CLICK"] {
            let predicate = NSPredicate(format: "label == %@", label)
            let matches = xcode.windows.buttons.matching(predicate).allElementsBoundByIndex
                + xcode.windows.popUpButtons.matching(predicate).allElementsBoundByIndex
            XCTAssertEqual(matches.count, 1, "Expected exactly one matching Xcode control")
            let control = try XCTUnwrap(matches.first)
            XCTAssertTrue(control.exists)
            xcode.activate()
            control.click()
            print("MEGURI_PLAN_EDITOR_TREE\n\(xcode.windows.debugDescription.prefix(60_000))")
        }
        #else
        throw XCTSkip("Test plan editor probe requires macOS")
        #endif
    }
}
