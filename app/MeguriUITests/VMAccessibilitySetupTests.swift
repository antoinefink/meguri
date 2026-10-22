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
            fail(settings, "Could not open Settings Accessibility privacy pane; inspect the VM attachments")
            return
        }

        let runnerNames = ["MeguriUITests-Runner", "MeguriUITests Runner"]
        let exactName = NSPredicate(format: "label IN %@ OR value IN %@", runnerNames, runnerNames)
        // Allow the pane a bounded time to populate, without navigating other settings.
        _ = settings.staticTexts.matching(exactName).firstMatch.waitForExistence(timeout: 10)
        capture(settings, phase: "before")
        var rows = runnerRows(settings, predicate: exactName)
        if rows.isEmpty {
            guard addCurrentRunner(settings) else { return }
            let appeared = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                !self.runnerRows(settings, predicate: exactName).isEmpty
            }, object: nil)
            guard XCTWaiter().wait(for: [appeared], timeout: 10) == .completed else {
                fail(settings, "The exact runner row did not appear after Add; no permission was toggled")
                return
            }
            rows = runnerRows(settings, predicate: exactName)
        }
        guard rows.count == 1 else {
            fail(settings, "Expected exactly one MeguriUITests runner row, found \(rows.count). Inspect Settings hierarchy; no permission was toggled")
            return
        }
        let controls = rows[0].switches.allElementsBoundByIndex + rows[0].checkBoxes.allElementsBoundByIndex
        guard controls.count == 1 else {
            fail(settings, "Expected one switch/checkbox in the exact runner row, found \(controls.count); inspect Settings hierarchy")
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
            fail(settings, "Runner permission control has an unknown value: \(String(describing: control.value)); no click performed")
            return
        }
        if !enabled {
            guard click(control, in: settings, description: "exact runner switch") else { return }
        }

        let trusted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in AXIsProcessTrusted() }, object: nil)
        guard XCTWaiter().wait(for: [trusted], timeout: 15) == .completed else {
            fail(settings, "Runner AX trust is still false. Inspect VM Settings attachments; credentials may require manual VM authorization, or the runner may need relaunching. Remove the authorization marker after setup")
            return
        }
    }

    @MainActor
    private func runnerRows(_ settings: XCUIApplication, predicate: NSPredicate) -> [XCUIElement] {
        settings.outlineRows.containing(predicate).allElementsBoundByIndex
            + settings.tableRows.containing(predicate).allElementsBoundByIndex
    }

    @MainActor
    private func addCurrentRunner(_ settings: XCUIApplication) -> Bool {
        let runnerURL = Bundle.main.bundleURL
        guard runnerURL.isFileURL, runnerURL.pathExtension == "app",
              Bundle.main.bundleIdentifier == "com.antoinefink.meguri.dev.uitests.xctrunner",
              Bundle(url: runnerURL)?.bundleIdentifier == "com.antoinefink.meguri.dev.uitests.xctrunner" else {
            fail(settings, "Refusing Add: Bundle.main is not the expected runner .app")
            return false
        }
        let addButtons = settings.buttons.matching(NSPredicate(format: "label == %@", "Add"))
        guard addButtons.count == 1 else {
            fail(settings, "Expected exactly one Settings Add button, found \(addButtons.count)")
            return false
        }
        guard click(addButtons.element(boundBy: 0), in: settings, description: "Settings Add") else { return false }
        guard waitForPicker(settings) != nil else { return false }
        guard bringSettingsForward(settings) else { return false }
        settings.typeKey("g", modifierFlags: [.command, .shift])

        let goToFolder = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isGoToFolder(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [goToFolder], timeout: 5) == .completed,
              let folderPicker = pickerContainer(settings), isGoToFolder(folderPicker) else {
            fail(settings, "Expected the unique Go To Folder text field in the picker; no path entered")
            return false
        }
        let field = folderPicker.textFields.element(boundBy: 0)
        guard click(field, in: settings, description: "Go To Folder text field") else { return false }
        guard bringSettingsForward(settings) else { return false }
        field.typeKey("a", modifierFlags: .command)
        guard bringSettingsForward(settings) else { return false }
        field.typeText(runnerURL.path)
        guard field.value as? String == runnerURL.path else {
            fail(settings, "Go To Folder did not contain the exact current runner bundle path")
            return false
        }
        guard bringSettingsForward(settings) else { return false }
        field.typeKey(.return, modifierFlags: [])

        let chooseName = NSPredicate(format: "label IN %@", ["Open", "Add"])
        let resolved = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker) && !self.isGoToFolder(picker)
                && picker.buttons.matching(chooseName).element(boundBy: 0).isEnabled
        }, object: nil)
        guard XCTWaiter().wait(for: [resolved], timeout: 5) == .completed,
              let picker = pickerContainer(settings), isAppPicker(picker) else {
            fail(settings, "Runner path did not resolve to a picker with one Open/Add control; inspect picker/auth UI")
            return false
        }
        return click(picker.buttons.matching(chooseName).element(boundBy: 0), in: settings, description: "runner picker Open/Add")
    }

    @MainActor
    private func isGoToFolder(_ picker: XCUIElement) -> Bool {
        let names = ["Go to the folder:", "Go to Folder:", "Go to Folder"]
        let exactName = NSPredicate(format: "label IN %@ OR value IN %@", names, names)
        return picker.textFields.count == 1 && picker.secureTextFields.count == 0
            && (names.contains(picker.label) || picker.staticTexts.matching(exactName).count == 1
                || picker.textFields.matching(exactName).count == 1)
    }

    @MainActor
    private func isAppPicker(_ picker: XCUIElement) -> Bool {
        picker.secureTextFields.count == 0
            && picker.buttons.matching(NSPredicate(format: "label IN %@", ["Open", "Add"])).count == 1
            && picker.buttons.matching(NSPredicate(format: "label == %@", "Cancel")).count == 1
    }

    /// Use the innermost sheet/dialog, refusing multiple independent picker or auth surfaces.
    @MainActor
    private func pickerContainer(_ settings: XCUIApplication) -> XCUIElement? {
        let containers = settings.descendants(matching: .sheet).allElementsBoundByIndex
            + settings.descendants(matching: .dialog).allElementsBoundByIndex
        let leaves = containers.filter {
            $0.descendants(matching: .sheet).count == 0 && $0.descendants(matching: .dialog).count == 0
        }
        return leaves.count == 1 ? leaves[0] : nil
    }

    @MainActor
    private func waitForPicker(_ settings: XCUIApplication) -> XCUIElement? {
        let appeared = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [appeared], timeout: 5) == .completed,
              let picker = pickerContainer(settings), isAppPicker(picker) else {
            fail(settings, "Expected one normal app picker sheet/dialog after Settings Add")
            return nil
        }
        return picker
    }

    @MainActor
    private func click(_ element: XCUIElement, in settings: XCUIApplication, description: String) -> Bool {
        guard element.isEnabled, element.isHittable else {
            fail(settings, "Unavailable \(description); inspect picker/auth UI")
            return false
        }
        guard bringSettingsForward(settings) else { return false }
        element.click()
        return true
    }

    @MainActor
    private func bringSettingsForward(_ settings: XCUIApplication) -> Bool {
        do {
            try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences")
            return true
        } catch {
            fail(settings, "Could not bring Settings forward: \(error)")
            return false
        }
    }

    @MainActor
    private func fail(_ settings: XCUIApplication, _ message: String, file: StaticString = #filePath, line: UInt = #line) {
        // XCTFail may unwind immediately with continueAfterFailure=false, before defer runs.
        capture(settings, phase: "failure")
        XCTFail(message, file: file, line: line)
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
