#if os(macOS)
import AppKit
@preconcurrency import ApplicationServices
import XCTest

/// Explicit VM provisioning only; skipped at runtime without the opt-in marker.
final class VMAccessibilitySetupTests: XCTestCase {
    @MainActor
    func testEnableRunnerAccessibility() throws {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-ax-enable") else {
            throw XCTSkip("VM setup opt-in absent")
        }
        continueAfterFailure = false
        guard CardSwapTimingGate.hypervisorPresent() == true else {
            XCTFail("Accessibility setup requires kern.hv_vmm_present == true")
            return
        }
        guard NSUserName() == "meguri" else {
            XCTFail("Accessibility setup requires the dedicated VM's meguri account")
            return
        }
        if AXIsProcessTrusted() { return }

        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        if AXIsProcessTrustedWithOptions(options) { return }

        let settings = XCUIApplication(bundleIdentifier: "com.apple.systempreferences")
        defer { capture(settings, phase: "final") }
        let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
        guard NSWorkspace.shared.open(url), settings.wait(for: .runningForeground, timeout: 10) else {
            XCTFail("Could not open Settings Accessibility privacy pane; inspect the VM attachments")
            return
        }

        let runnerNames = ["MeguriUITests-Runner", "MeguriUITests Runner"]
        let exactName = NSPredicate(format: "label IN %@", runnerNames)
        // Allow the pane a bounded time to populate, without navigating other settings.
        _ = settings.staticTexts.matching(exactName).firstMatch.waitForExistence(timeout: 10)
        capture(settings, phase: "before")
        let rows = settings.tableRows.allElementsBoundByIndex.filter { row in
            runnerNames.contains(row.label) || row.staticTexts.matching(exactName).count == 1
        }
        guard rows.count == 1 else {
            XCTFail("Expected exactly one MeguriUITests runner row, found \(rows.count). Inspect Settings hierarchy; no permission was toggled")
            return
        }
        let controls = rows[0].switches.allElementsBoundByIndex + rows[0].checkBoxes.allElementsBoundByIndex
        guard controls.count == 1 else {
            XCTFail("Expected one switch/checkbox in the exact runner row, found \(controls.count); inspect Settings hierarchy")
            return
        }
        let control = controls[0]
        let enabled: Bool
        switch control.value {
        case let value as NSNumber where value == 0 || value == 1:
            enabled = value == 1
        case let value as String where value == "0" || value == "1":
            enabled = value == "1"
        default:
            XCTFail("Runner permission control has an unknown value: \(String(describing: control.value)); no click performed")
            return
        }
        if !enabled {
            guard control.isEnabled, control.isHittable else {
                XCTFail("Runner switch unavailable")
                return
            }
            try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences")
            control.click()
        }

        let trusted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in AXIsProcessTrusted() }, object: nil)
        guard XCTWaiter().wait(for: [trusted], timeout: 15) == .completed else {
            XCTFail("Runner AX trust is still false. Inspect VM Settings attachments; credentials may require manual VM authorization, or the runner may need relaunching. Remove the authorization marker after setup")
            return
        }
    }

    @MainActor
    private func capture(_ settings: XCUIApplication, phase: String) {
        let hierarchy = XCTAttachment(string: settings.debugDescription)
        hierarchy.name = "VM Accessibility Settings hierarchy (\(phase))"
        hierarchy.lifetime = .keepAlways
        add(hierarchy)
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        screenshot.name = "VM Accessibility Settings screenshot (\(phase))"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }
}
#endif
