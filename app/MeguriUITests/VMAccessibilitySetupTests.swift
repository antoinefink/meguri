#if os(macOS)
import AppKit
@preconcurrency import ApplicationServices
import Darwin
import XCTest

/// Explicit VM provisioning only; skipped at runtime without the opt-in marker.
final class VMAccessibilitySetupTests: XCTestCase {
    @MainActor private var vmConsentValue: String?

    @MainActor
    func testInspectRemoteLoginSettings() throws {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-remote-login-inspect") else {
            throw XCTSkip("VM Remote Login inspection opt-in absent")
        }
        guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
            XCTFail("Remote Login inspection requires the dedicated VM's meguri account")
            return
        }

        let settings = XCUIApplication(bundleIdentifier: "com.apple.systempreferences")
        defer {
            if settings.state != .notRunning { settings.terminate() }
        }
        let url = URL(string: "x-apple.systempreferences:com.apple.Sharing-Settings.extension?RemoteLogin")!
        guard NSWorkspace.shared.open(url), settings.wait(for: .runningForeground, timeout: 5) else {
            XCTFail("Could not open System Settings Remote Login pane")
            return
        }
        do {
            try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences", timeout: 4)
        } catch {
            XCTFail("Could not bring System Settings forward for Remote Login inspection")
            return
        }
        let windows = settings.windows.allElementsBoundByAccessibilityElement
        guard (1...2).contains(windows.count) else {
            XCTFail("Expected one or two System Settings windows; inspection evidence withheld")
            return
        }

        let infoIdentifier = "Remote LoginToggle.infoButton"
        let infoButtons = settings.buttons.matching(identifier: infoIdentifier)
        guard infoButtons.firstMatch.waitForExistence(timeout: 5) else {
            XCTFail("Remote Login Info button did not appear; inspection evidence withheld")
            return
        }
        // Only the observed detail pane may scroll; never use the sidebar or toggle.
        for attempt in 0...3 {
            guard !settings.secureTextFields.firstMatch.exists,
                  settings.sheets.count == 0, infoButtons.count == 1 else {
                XCTFail("Remote Login navigation is ambiguous or has secure fields; inspection evidence withheld")
                return
            }
            let info = settings.buttons[infoIdentifier]
            guard info.label == "Show Detail", info.isEnabled else {
                XCTFail("Remote Login Info button did not match; inspection evidence withheld")
                return
            }
            if info.isHittable { break }
            guard attempt < 3 else {
                XCTFail("Remote Login Info button remained offscreen after three scrolls; inspection evidence withheld")
                return
            }
            let owners = windows.filter { $0.buttons.matching(identifier: infoIdentifier).count == 1 }
            guard owners.count == 1 else {
                XCTFail("Remote Login Info window is ambiguous; inspection evidence withheld")
                return
            }
            let window = owners[0]
            let buttonFrame = info.frame
            let panes = window.scrollViews.allElementsBoundByAccessibilityElement.filter { pane in
                let frame = pane.frame
                return !frame.isEmpty && window.frame.contains(frame)
                    && frame.midX > window.frame.midX
                    && frame.minX <= buttonFrame.minX && frame.maxX >= buttonFrame.maxX
                    && buttonFrame.minY >= frame.minY
            }
            guard panes.count == 1, buttonFrame.maxY > panes[0].frame.maxY else {
                XCTFail("Remote Login detail scroll view is ambiguous or button is not below it; inspection evidence withheld")
                return
            }
            panes[0].scroll(byDeltaX: 0, deltaY: -180)
        }
        guard !settings.secureTextFields.firstMatch.exists,
              settings.sheets.count == 0, infoButtons.count == 1 else {
            XCTFail("Remote Login Info navigation changed; inspection evidence withheld")
            return
        }
        let info = settings.buttons[infoIdentifier]
        guard info.label == "Show Detail", info.isEnabled, info.isHittable else {
            XCTFail("Exact Remote Login Info button is unavailable; inspection evidence withheld")
            return
        }
        info.click()
        let infoAppearance = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            settings.sheets.count > 0 || settings.windows.count > windows.count
        }, object: nil)
        let appeared = XCTWaiter().wait(for: [infoAppearance], timeout: 5) == .completed
        let inspectionWindows = settings.windows.allElementsBoundByAccessibilityElement
        let sheetCount = settings.sheets.count
        guard (1...2).contains(inspectionWindows.count),
              settings.windows.count == inspectionWindows.count,
              inspectionWindows.allSatisfy({ $0.exists }) else {
            XCTFail("Expected one or two stable System Settings windows after Info; inspection evidence withheld")
            return
        }

        var secureFieldsPresent = settings.secureTextFields.firstMatch.exists
        var windowEvidence: [XCTAttachment] = []
        for (index, window) in inspectionWindows.enumerated() {
            if secureFieldsPresent { break }
            guard window.exists, settings.windows.count == inspectionWindows.count else {
                XCTFail("System Settings windows changed during inspection; inspection evidence withheld")
                return
            }
            let outline = XCTAttachment(string: String(window.debugDescription.prefix(64_000)))
            outline.name = "VM Remote Login window \(index) tree (up to 64000 characters)"
            outline.lifetime = .keepAlways
            windowEvidence.append(outline)
            secureFieldsPresent = settings.secureTextFields.firstMatch.exists
            if secureFieldsPresent { break }
            let pixels = XCTAttachment(screenshot: window.screenshot())
            pixels.name = "VM Remote Login System Settings window \(index)"
            pixels.lifetime = .keepAlways
            windowEvidence.append(pixels)
            secureFieldsPresent = settings.secureTextFields.firstMatch.exists
        }
        // Hold all pixels and hierarchy until the final secure-field check has passed.
        secureFieldsPresent = settings.secureTextFields.firstMatch.exists || secureFieldsPresent
        let report: [String: Any] = ["windowCount": inspectionWindows.count,
                                   "sheetCount": sheetCount,
                                   "infoAppeared": appeared]
        let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        let diagnostic = XCTAttachment(string: String(decoding: data, as: UTF8.self))
        diagnostic.name = "VM Remote Login inspection status"
        diagnostic.lifetime = .keepAlways
        add(diagnostic)
        if !secureFieldsPresent {
            for attachment in windowEvidence { add(attachment) }
        }
        if !appeared {
            XCTFail("Remote Login Info sheet or added window did not appear within five seconds")
        }
    }

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

        let helperStaticTextCount = helper.staticTexts.count
        let helperButtonCount = helper.buttons.count
        let dialogs = (0..<dialogCount).map { helper.dialogs.element(boundBy: $0) }
        let dialogCounts = dialogs.map { (staticTexts: $0.staticTexts.count, buttons: $0.buttons.count) }
        guard dialogCounts.allSatisfy({ $0.staticTexts <= 8 && $0.buttons <= 8 }) else {
            XCTFail("Too many UserNotificationCenter dialog texts or buttons; sharing consent evidence withheld")
            return
        }
        var dialogFacts: [[String: Any]] = []
        for (index, dialog) in dialogs.enumerated() {
            guard !helper.secureTextFields.firstMatch.exists, dialog.exists else {
                XCTFail("Secure fields present or UserNotificationCenter dialog changed; sharing consent evidence withheld")
                return
            }
            let counts = dialogCounts[index]
            let staticTexts = (0..<counts.staticTexts).map { textIndex in
                let text = dialog.staticTexts.element(boundBy: textIndex)
                return ["label": text.label, "value": text.value as? String ?? "<non-string>"]
            }
            let buttons: [[String: Any]] = (0..<counts.buttons).map { buttonIndex in
                let button = dialog.buttons.element(boundBy: buttonIndex)
                return ["identifier": button.identifier, "label": button.label, "title": button.title,
                        "isEnabled": button.isEnabled, "isHittable": button.isHittable]
            }
            dialogFacts.append(["index": index, "staticTextCount": counts.staticTexts,
                                "buttonCount": counts.buttons, "staticTexts": staticTexts, "buttons": buttons])
        }
        let facts: [String: Any] = ["helperStaticTextCount": helperStaticTextCount,
                                   "helperButtonCount": helperButtonCount, "dialogCount": dialogCount,
                                   "dialogs": dialogFacts]
        let factsData = try JSONSerialization.data(withJSONObject: facts, options: [.prettyPrinted, .sortedKeys])
        let matchingFacts = XCTAttachment(string: String(decoding: factsData, as: UTF8.self))
        matchingFacts.name = "VM sharing consent matching facts"
        matchingFacts.lifetime = .keepAlways
        let outline = XCTAttachment(string: helper.debugDescription)
        outline.name = "VM sharing consent helper tree"
        outline.lifetime = .keepAlways
        var evidence = [outline, matchingFacts]
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
    func testApproveRecordedMeguriVersionSharing() throws {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-allow-recorded-meguri-version") else {
            throw XCTSkip("Recorded Meguri version sharing opt-in absent")
        }
        guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
            XCTFail("Recorded version sharing approval requires the dedicated VM's meguri account")
            return
        }
        continueAfterFailure = false

        let helper = XCUIApplication(bundleIdentifier: "com.apple.UserNotificationCenter")
        guard helper.state != .notRunning else {
            XCTFail("UserNotificationCenter is not already running")
            return
        }
        guard helper.dialogs.firstMatch.waitForExistence(timeout: 5),
              helper.dialogs.count == 1, helper.secureTextFields.count == 0 else {
            XCTFail("Expected one existing UserNotificationCenter dialog without secure fields; no input performed")
            return
        }
        let dialog = helper.dialogs.element(boundBy: 0)
        let heading = "“Meguri” differs from previously opened versions. Are you sure you want to open it?"
        let explanation = "Opening “Meguri” will allow it to access data from previously used versions of “Meguri”."
        let headingMatch = NSPredicate(format: "label == %@ OR value == %@", heading, heading)
        let explanationMatch = NSPredicate(format: "label == %@ OR value == %@", explanation, explanation)
        let openMatch = NSPredicate(format: "identifier == %@ AND title == %@", "action-button-1", "Open Anyway")
        let cancelMatch = NSPredicate(format: "identifier == %@ AND title == %@", "action-button-2", "Don’t Open")
        guard dialog.staticTexts.matching(headingMatch).count == 1,
              dialog.staticTexts.matching(explanationMatch).count == 1,
              helper.staticTexts.matching(headingMatch).count == 1,
              helper.staticTexts.matching(explanationMatch).count == 1,
              dialog.buttons.count == 2,
              dialog.buttons.matching(openMatch).count == 1,
              dialog.buttons.matching(cancelMatch).count == 1 else {
            XCTFail("Recorded Meguri version sharing text or buttons did not match uniquely; no input performed")
            return
        }
        let open = dialog.buttons.matching(openMatch).element(boundBy: 0)
        guard open.isEnabled, open.isHittable,
              helper.state != .notRunning, helper.dialogs.count == 1,
              helper.secureTextFields.count == 0 else {
            XCTFail("Recorded Meguri version sharing action is unavailable or ambiguous; no input performed")
            return
        }
        // This opt-in authorizes only this recorded prompt and a single native mouse click.
        open.click()

        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            helper.staticTexts.matching(headingMatch).count == 0
                && helper.staticTexts.matching(explanationMatch).count == 0
        }, object: nil)
        guard XCTWaiter().wait(for: [dismissed], timeout: 5) == .completed else {
            XCTFail("Recorded Meguri version sharing text did not disappear within five seconds; no further input performed")
            return
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
