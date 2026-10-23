#if os(macOS)
import AppKit
@preconcurrency import ApplicationServices
import Darwin
import XCTest

/// Explicit VM provisioning only; skipped at runtime without the opt-in marker.
final class VMAccessibilitySetupTests: XCTestCase {
    @MainActor private var vmConsentValue: String?

    @MainActor
    func testInspectSandboxSharingConsent() throws {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-sharing-consent-inspect") else {
            throw XCTSkip("VM sharing consent inspection opt-in absent")
        }
        guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
            XCTFail("Sharing consent inspection requires the dedicated VM's meguri account")
            return
        }

        let helper = XCUIApplication(bundleIdentifier: "com.apple.UserNotificationCenter")
        guard helper.state != .notRunning else {
            XCTFail("UserNotificationCenter is not already running")
            return
        }
        let appeared = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            helper.windows.firstMatch.exists || helper.dialogs.firstMatch.exists
        }, object: nil)
        let result = XCTWaiter().wait(for: [appeared], timeout: 5)
        guard !helper.secureTextFields.firstMatch.exists else {
            XCTFail("Secure text fields present; sharing consent evidence withheld")
            return
        }
        let windowCount = helper.windows.count
        let dialogCount = helper.dialogs.count
        guard windowCount <= 4, dialogCount <= 4 else {
            XCTFail("Too many UserNotificationCenter surfaces; sharing consent evidence withheld")
            return
        }

        let outline = XCTAttachment(string: helper.debugDescription)
        outline.name = "VM sharing consent helper tree"
        outline.lifetime = .keepAlways
        var evidence = [outline]
        let surfaces = windowCount > 0 ? helper.windows : helper.dialogs
        let surfaceCount = windowCount > 0 ? windowCount : dialogCount
        for index in 0..<surfaceCount {
            let surface = surfaces.element(boundBy: index)
            guard !helper.secureTextFields.firstMatch.exists else {
                XCTFail("Secure text fields present; sharing consent evidence withheld")
                return
            }
            guard surface.exists else {
                XCTFail("UserNotificationCenter surface changed; sharing consent evidence withheld")
                return
            }
            let pixels = XCTAttachment(screenshot: surface.screenshot())
            pixels.name = "VM sharing consent system window"
            pixels.lifetime = .keepAlways
            evidence.append(pixels)
        }
        // Hold all evidence until the final secure-field check has passed.
        guard !helper.secureTextFields.firstMatch.exists else {
            XCTFail("Secure text fields present; sharing consent evidence withheld")
            return
        }
        for attachment in evidence { add(attachment) }
        guard result == .completed, surfaceCount > 0 else {
            XCTFail("No existing UserNotificationCenter window or dialog appeared within five seconds")
            return
        }
        if surfaceCount > 1 {
            XCTFail("Multiple UserNotificationCenter surfaces require manual inspection")
        }
    }

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
        defer {
            if settings.state != .notRunning {
                attachSetupEvidence(settings, stage: "final")
                settings.terminate()
            }
            vmConsentValue = nil
        }
        let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
        guard NSWorkspace.shared.open(url), settings.wait(for: .runningForeground, timeout: 10) else {
            reportSetupFailure(settings, "Could not open Settings Accessibility privacy pane; inspect the VM attachments")
            return
        }

        let runnerNames = ["MeguriUITests-Runner", "MeguriUITests Runner"]
        let exactName = NSPredicate(format: "label IN %@ OR value IN %@", runnerNames, runnerNames)
        // Allow the pane a bounded time to populate, without navigating other settings.
        _ = settings.staticTexts.matching(exactName).firstMatch.waitForExistence(timeout: 10)
        guard authorizeSettingsIfNeeded(settings) else { return }
        attachSetupEvidence(settings, stage: "before")
        var rows = runnerRows(settings, predicate: exactName)
        if rows.isEmpty {
            guard addCurrentRunner(settings) else { return }
            let appeared = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                !self.runnerRows(settings, predicate: exactName).isEmpty
            }, object: nil)
            guard XCTWaiter().wait(for: [appeared], timeout: 10) == .completed else {
                reportSetupFailure(settings, "The exact runner row did not appear after Add; no permission was toggled")
                return
            }
            rows = runnerRows(settings, predicate: exactName)
        }
        guard rows.count == 1 else {
            reportSetupFailure(settings, "Expected exactly one MeguriUITests runner row, found \(rows.count). Inspect Settings hierarchy; no permission was toggled")
            return
        }
        let controls = rows[0].switches.allElementsBoundByIndex + rows[0].checkBoxes.allElementsBoundByIndex
        guard controls.count == 1 else {
            reportSetupFailure(settings, "Expected one switch/checkbox in the exact runner row, found \(controls.count); inspect Settings hierarchy")
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
            reportSetupFailure(settings, "Runner permission control has an unknown value: \(String(describing: control.value)); no click performed")
            return
        }
        if !enabled {
            guard click(control, in: settings, description: "exact runner switch") else { return }
        }

        let trusted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in AXIsProcessTrusted() }, object: nil)
        guard XCTWaiter().wait(for: [trusted], timeout: 15) == .completed else {
            reportSetupFailure(settings, "Runner AX trust is still false. Inspect VM Settings attachments; credentials may require manual VM authorization, or the runner may need relaunching. Remove the authorization marker after setup")
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
            reportSetupFailure(settings, "Refusing Add: Bundle.main is not the expected runner .app")
            return false
        }
        // Authorizing a stale Settings sheet can already have opened the original Add picker.
        if settings.descendants(matching: .sheet).count > 0 || settings.descendants(matching: .dialog).count > 0 {
            guard let picker = pickerContainer(settings), isAppPicker(picker) else {
                reportSetupFailure(settings, "Expected one normal app picker before resuming Add")
                return false
            }
        } else {
            let addButtons = settings.buttons.matching(NSPredicate(format: "label == %@", "Add"))
            guard addButtons.count == 1 else {
                reportSetupFailure(settings, "Expected exactly one Settings Add button, found \(addButtons.count)")
                return false
            }
            guard click(addButtons.element(boundBy: 0), in: settings, description: "Settings Add") else { return false }
        }
        guard waitForPicker(settings) != nil else { return false }
        guard bringSettingsForward(settings) else { return false }
        settings.typeKey("g", modifierFlags: [.command, .shift])

        let goToFolder = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isGoToFolder(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [goToFolder], timeout: 5) == .completed,
              let folderPicker = pickerContainer(settings), isGoToFolder(folderPicker) else {
            reportSetupFailure(settings, "Expected the unique Go To Folder text field in the picker; no path entered")
            return false
        }
        let field = folderPicker.textFields.matching(identifier: "PathTextField").element(boundBy: 0)
        guard click(field, in: settings, description: "Go To Folder text field") else { return false }
        guard bringSettingsForward(settings) else { return false }
        field.typeKey("a", modifierFlags: .command)
        guard bringSettingsForward(settings) else { return false }
        field.typeText(runnerURL.path)
        guard field.value as? String == runnerURL.path else {
            reportSetupFailure(settings, "Go To Folder did not contain the exact current runner bundle path")
            return false
        }
        guard bringSettingsForward(settings) else { return false }
        field.typeKey(.return, modifierFlags: [])

        let resolved = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker) && !self.isGoToFolder(picker)
                && picker.buttons.matching(identifier: "OKButton").element(boundBy: 0).isEnabled
        }, object: nil)
        guard XCTWaiter().wait(for: [resolved], timeout: 5) == .completed,
              let picker = pickerContainer(settings), isAppPicker(picker) else {
            reportSetupFailure(settings, "Runner path did not resolve to a picker with one Open/Add control; inspect picker/auth UI")
            return false
        }
        return click(picker.buttons.matching(identifier: "OKButton").element(boundBy: 0), in: settings, description: "runner picker Open/Add")
    }

    @MainActor
    private func isGoToFolder(_ picker: XCUIElement) -> Bool {
        picker.elementType == .sheet && picker.identifier == "GoToWindow"
            && picker.textFields.count == 1 && picker.secureTextFields.count == 0
            && picker.textFields.matching(identifier: "PathTextField").count == 1
    }

    @MainActor
    private func isAppPicker(_ picker: XCUIElement) -> Bool {
        picker.secureTextFields.count == 0
            && picker.buttons.matching(identifier: "OKButton").count == 1
            && picker.buttons.matching(identifier: "CancelButton").count == 1
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
            if self.hasAuthorizationPrompt(settings) { return true }
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [appeared], timeout: 5) == .completed else {
            reportSetupFailure(settings, "Expected one normal app picker sheet/dialog after Settings Add")
            return nil
        }
        guard authorizeSettingsIfNeeded(settings) else { return nil }
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [ready], timeout: 5) == .completed,
              let picker = pickerContainer(settings), isAppPicker(picker) else {
            reportSetupFailure(settings, "Expected one normal app picker sheet/dialog after Settings authorization")
            return nil
        }
        return picker
    }

    @MainActor
    private func hasAuthorizationPrompt(_ settings: XCUIApplication) -> Bool {
        settings.secureTextFields.count > 0
            || settings.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).count > 0
    }

    @MainActor
    private func isSettingsAuthorization(_ sheet: XCUIElement) -> Bool {
        guard sheet.elementType == .sheet, sheet.textFields.count == 1, sheet.secureTextFields.count == 1 else { return false }
        let username = sheet.textFields.element(boundBy: 0)
        guard username.placeholderValue == "Username", let account = username.value as? String,
              account.caseInsensitiveCompare(NSUserName()) == .orderedSame else { return false }
        return sheet.secureTextFields.element(boundBy: 0).label == "Password"
            && sheet.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).count == 1
            && sheet.buttons.matching(NSPredicate(format: "label == %@", "Cancel")).count == 1
            && sheet.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@", "Privacy & Security is trying to modify your system settings.", "Privacy & Security is trying to modify your system settings.")).count == 1
    }

    @MainActor
    private func authorizeSettingsIfNeeded(_ settings: XCUIApplication) -> Bool {
        guard hasAuthorizationPrompt(settings) else { return true }
        // Repeat the provisioning boundary here, before any credential-file access or input.
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-ax-enable"),
              CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri",
              let sheet = pickerContainer(settings), isSettingsAuthorization(sheet),
              settings.secureTextFields.count == 1,
              settings.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).count == 1 else {
            reportSetupFailure(settings, "Refusing unexpected or ambiguous Settings authorization UI")
            return false
        }
        let passwordField = sheet.secureTextFields.element(boundBy: 0)
        guard click(passwordField, in: settings, description: "Settings secure password field"),
              bringSettingsForward(settings) else { return false }
        guard let password = readVMConsentValue() else {
            reportSetupFailure(settings, "VM authorization credential is unavailable or invalid")
            return false
        }
        vmConsentValue = password
        passwordField.typeKey("a", modifierFlags: .command)
        let pasted: Bool = {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            defer { pasteboard.clearContents() }
            guard pasteboard.setString(password, forType: .string) else { return false }
            passwordField.typeKey("v", modifierFlags: .command)
            return true
        }()
        guard pasted, isSettingsAuthorization(sheet) else {
            reportSetupFailure(settings, "Could not fill the expected Settings secure field")
            return false
        }
        let modify = sheet.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).element(boundBy: 0)
        guard click(modify, in: settings, description: "Settings authorization Modify Settings") else { return false }
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard !self.hasAuthorizationPrompt(settings) else { return false }
            if settings.descendants(matching: .sheet).count == 0 && settings.descendants(matching: .dialog).count == 0 { return true }
            guard let picker = self.pickerContainer(settings) else { return false }
            return self.isAppPicker(picker)
        }, object: nil)
        guard XCTWaiter().wait(for: [dismissed], timeout: 10) == .completed else {
            reportSetupFailure(settings, "Settings authorization was refused, ambiguous, or did not complete")
            return false
        }
        return true
    }

    /// Open without following links; validate the opened file rather than its path metadata.
    private func readVMConsentValue() -> String? {
        let descriptor = Darwin.open("/tmp/mg-vm-ax-password", O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard descriptor >= 0 else { return nil }
        defer { Darwin.close(descriptor) }
        var metadata = stat()
        guard fstat(descriptor, &metadata) == 0,
              metadata.st_mode & mode_t(S_IFMT) == mode_t(S_IFREG),
              metadata.st_uid == getuid(), metadata.st_mode & 0o077 == 0,
              metadata.st_nlink == 1, metadata.st_size > 0, metadata.st_size <= 4096 else { return nil }
        var bytes = [UInt8](repeating: 0, count: Int(metadata.st_size) + 1)
        let count = bytes.withUnsafeMutableBytes { Darwin.read(descriptor, $0.baseAddress, $0.count) }
        guard count == Int(metadata.st_size), let decoded = String(bytes: bytes.prefix(count), encoding: .utf8) else { return nil }
        var password = decoded
        if password.hasSuffix("\n") {
            password.removeLast()
            if password.hasSuffix("\r") { password.removeLast() }
        }
        guard !password.isEmpty, !password.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return nil }
        return password
    }

    @MainActor
    private func click(_ element: XCUIElement, in settings: XCUIApplication, description: String) -> Bool {
        guard element.isEnabled, element.isHittable else {
            reportSetupFailure(settings, "Unavailable \(description); inspect picker/auth UI")
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
            reportSetupFailure(settings, "Could not bring Settings forward")
            return false
        }
    }

    @MainActor
    private func reportSetupFailure(_ settings: XCUIApplication, _ reason: String, file: StaticString = #filePath, line: UInt = #line) {
        // Save evidence and close Settings before XCTest can stop this case.
        attachSetupEvidence(settings, stage: "failure")
        if settings.state != .notRunning { settings.terminate() }
        XCTFail(safeDiagnostic(reason), file: file, line: line)
    }

    @MainActor
    private func attachSetupEvidence(_ settings: XCUIApplication, stage: String) {
        let outline = XCTAttachment(string: safeDiagnostic(settings.debugDescription))
        outline.name = "VM setup tree: \(stage)"
        outline.lifetime = .keepAlways
        add(outline)
        // Loading the credential permanently ends pixel capture for this run.
        guard vmConsentValue == nil else { return }
        let pixels = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        pixels.name = "VM setup image: \(stage)"
        pixels.lifetime = .keepAlways
        add(pixels)
    }

    @MainActor
    private func safeDiagnostic(_ message: String) -> String {
        guard let vmConsentValue else { return message }
        let maskedLiteral = String(String(decoding: try! JSONEncoder().encode(vmConsentValue), as: UTF8.self).dropFirst().dropLast())
        return message.replacingOccurrences(of: maskedLiteral, with: "[MASKED]")
            .replacingOccurrences(of: vmConsentValue, with: "[MASKED]")
    }
}
#endif
