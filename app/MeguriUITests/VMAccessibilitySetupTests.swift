#if os(macOS)
import AppKit
@preconcurrency import ApplicationServices
import CryptoKit
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
        guard let inspection = openRemoteLoginInfo(settings) else { return }
        let appeared = inspection.appeared
        let inspectionWindows = inspection.windows
        let sheetCount = settings.sheets.count

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
    func testEnableRemoteLoginFullDiskAccess() throws {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-remote-login-fda-enable") else {
            throw XCTSkip("VM Remote Login full disk access opt-in absent")
        }
        guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
            XCTFail("Remote Login setup requires the dedicated VM's meguri account")
            return
        }
        continueAfterFailure = false
        let settings = XCUIApplication(bundleIdentifier: "com.apple.systempreferences")
        defer {
            if vmConsentValue != nil { NSPasteboard.general.clearContents() }
            if settings.state != .notRunning { settings.terminate() }
            vmConsentValue = nil
        }
        // A previous single FDA click may have left its normal OS authorization open.
        let resumedAuthorization = remoteLoginAuthorizationSurface(settings) != nil
        let resumedAuthority: RemoteLoginInfoAuthority?
        if resumedAuthorization {
            guard let underlying = remoteLoginUnderlyingInfoFacts(settings) else {
                reportRemoteLoginFailure(settings, "Exact underlying Remote Login Info authority is absent or ambiguous; no credential read or input performed")
                return
            }
            resumedAuthority = underlying.authority
            guard authorizeRemoteLoginIfNeeded(settings, authority: underlying.authority) else { return }
        } else {
            resumedAuthority = nil
            if remoteLoginFullDiskAccessState(settings) == nil {
                guard let inspection = openRemoteLoginInfo(settings, onFailure: {
                    self.reportRemoteLoginFailure(settings, $0)
                }) else { return }
                guard inspection.appeared else {
                    reportRemoteLoginFailure(settings, "Remote Login Info did not appear within five seconds; no permission input performed")
                    return
                }
            }
        }
        guard remoteLoginAuthorizationSurface(settings) == nil else {
            reportRemoteLoginFailure(settings, "Unexpected Remote Login authorization before the FDA action; no permission input performed")
            return
        }
        guard let initial = remoteLoginFullDiskAccessState(settings),
              resumedAuthority == nil || resumedAuthority == initial.authority,
              let alreadyEnabled = remoteLoginBoolean(initial.control) else {
            reportRemoteLoginFailure(settings, "Remote Login Info did not match the exact recorded settings; no permission input performed")
            return
        }
        if !alreadyEnabled {
            guard !resumedAuthorization else {
                reportRemoteLoginFailure(settings, "The previous FDA authorization did not enable full disk access; no retry performed")
                return
            }
            do {
                try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences", timeout: 4)
            } catch {
                reportRemoteLoginFailure(settings, "Could not bring Settings forward; no permission input performed")
                return
            }
            guard remoteLoginAuthorizationSurface(settings) == nil,
                  let current = remoteLoginFullDiskAccessState(settings),
                  current.authority == initial.authority,
                  remoteLoginBoolean(current.control) == false,
                  current.control.isEnabled, current.control.isHittable else {
                reportRemoteLoginFailure(settings, "Exact Remote Login full disk access switch changed or is unavailable; no permission input performed")
                return
            }
            // The single FDA permission input: the unique, exact FDA row's switch, once.
            current.control.click()
            let enabledOrAuthorization = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                if self.remoteLoginAuthorizationSurface(settings) != nil { return true }
                guard let state = self.remoteLoginFullDiskAccessState(settings) else { return false }
                return self.remoteLoginBoolean(state.control) == true
            }, object: nil)
            let completed = XCTWaiter().wait(for: [enabledOrAuthorization], timeout: 10) == .completed
            guard completed else {
                reportRemoteLoginFailure(settings, "Remote Login full disk access did not become enabled within ten seconds; no further permission input performed")
                return
            }
            guard authorizeRemoteLoginIfNeeded(settings, authority: initial.authority) else { return }
        }
        guard let final = remoteLoginFullDiskAccessState(settings),
              remoteLoginBoolean(final.control) == true,
              final.authority == initial.authority else {
            reportRemoteLoginFailure(settings, "Remote Login full disk access or the unchanged Remote Login/allowed-user settings could not be verified")
            return
        }
        do {
            try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences", timeout: 4)
        } catch {
            reportRemoteLoginFailure(settings, "Could not bring Settings forward to finish Remote Login setup")
            return
        }
        guard remoteLoginAuthorizationSurface(settings) == nil,
              let verified = remoteLoginFullDiskAccessState(settings),
              remoteLoginBoolean(verified.control) == true,
              verified.authority == initial.authority else {
            reportRemoteLoginFailure(settings, "Remote Login settings changed before Done; no further input performed")
            return
        }
        let doneButtons = settings.sheets.element(boundBy: 0).buttons.matching(NSPredicate(format: "label == %@", "Done"))
        guard doneButtons.count == 1 else {
            reportRemoteLoginFailure(settings, "Remote Login Info Done button was not unique; no further input performed")
            return
        }
        let done = doneButtons.element(boundBy: 0)
        guard done.isEnabled, done.isHittable else {
            reportRemoteLoginFailure(settings, "Remote Login Info Done button was unavailable; no further input performed")
            return
        }
        done.click()
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            settings.sheets.count == 0 || self.remoteLoginAuthorizationSurface(settings) != nil
        }, object: nil)
        guard XCTWaiter().wait(for: [dismissed], timeout: 5) == .completed,
              remoteLoginAuthorizationSurface(settings) == nil, settings.sheets.count == 0 else {
            reportRemoteLoginFailure(settings, "Remote Login Info did not close normally after Done; no further input performed")
            return
        }
        settings.terminate()
        let status: [String: Bool] = ["fullDiskAccessEnabled": true, "remoteLoginOn": true,
                                    "allowedUsersUnchanged": true, "alreadyEnabled": alreadyEnabled,
                                    "doneClicked": true, "settingsClosed": settings.state == .notRunning]
        let data = try JSONSerialization.data(withJSONObject: status, options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: data, as: UTF8.self))
        evidence.name = "VM Remote Login safe settings status"
        evidence.lifetime = .keepAlways
        add(evidence)
        if settings.state != .notRunning { reportRemoteLoginFailure(settings, "Settings did not close after Remote Login setup") }
    }

    @MainActor
    private func remoteLoginBoolean(_ control: XCUIElement) -> Bool? {
        switch control.value {
        case let value as NSNumber where value == 0 || value == 1: return value == 1
        case let value as String where value == "0" || value == "1": return value == "1"
        default: return nil
        }
    }

    private struct RemoteLoginInfoAuthority: Equatable {
        let allowedUsers: [String]
        let remoteLoginOn: Bool
    }

    @MainActor
    private func remoteLoginFullDiskAccessState(_ settings: XCUIApplication) -> (control: XCUIElement, authority: RemoteLoginInfoAuthority)? {
        guard settings.state != .notRunning, settings.windows.count == 1, settings.sheets.count == 1,
              settings.dialogs.count == 0, settings.alerts.count == 0,
              settings.secureTextFields.count == 0 else { return nil }
        guard let facts = remoteLoginUnderlyingInfoFacts(settings) else { return nil }
        return (facts.control, facts.authority)
    }

    /// Read only recorded Info facts, including beneath a separately matched authorization overlay.
    /// Query static text, switches, popup and outline only; never secure-field values.
    @MainActor
    private func remoteLoginUnderlyingInfoFacts(_ settings: XCUIApplication) -> (control: XCUIElement, authority: RemoteLoginInfoAuthority)? {
        guard settings.state != .notRunning, settings.windows.count == 1 else { return nil }
        let fda = NSPredicate(format: "label == %@ OR value == %@",
                              "Allow full disk access for remote users", "Allow full disk access for remote users")
        let on = NSPredicate(format: "label == %@ OR value == %@", "Remote Login: On", "Remote Login: On")
        let sheets = settings.sheets.allElementsBoundByAccessibilityElement.filter {
            $0.staticTexts.matching(fda).count == 1 && $0.staticTexts.matching(on).count == 1
        }
        guard sheets.count == 1 else { return nil }
        let sheet = sheets[0]
        let groups = sheet.descendants(matching: .group).allElementsBoundByAccessibilityElement
        func groupsWithDirectText(_ text: String) -> [XCUIElement] {
            let exact = NSPredicate(format: "label == %@ OR value == %@", text, text)
            return groups.filter { $0.children(matching: .staticText).matching(exact).count == 1 }
        }
        let rows = groupsWithDirectText("Allow full disk access for remote users")
        let headers = groupsWithDirectText("Remote Login: On")
        guard rows.count == 1, headers.count == 1,
              sheet.staticTexts.matching(NSPredicate(format: "label == %@ OR value == %@",
                                                     "Allow access for", "Allow access for")).count == 1,
              rows[0].switches.count == 1, headers[0].switches.count == 1,
              sheet.switches.count == 2,
              let remoteLoginOn = remoteLoginBoolean(headers[0].switches.element(boundBy: 0)), remoteLoginOn,
              sheet.popUpButtons.count == 1,
              sheet.popUpButtons.element(boundBy: 0).value as? String == "Only these users",
              sheet.outlines.count == 1 else { return nil }
        let outline = sheet.outlines.element(boundBy: 0)
        let administrators = NSPredicate(format: "label == %@ OR value == %@", "Administrators", "Administrators")
        guard outline.staticTexts.matching(administrators).count == 1 else { return nil }
        // Compare internally only: never attach account names or any other Settings values.
        let allowedUsers = outline.staticTexts.allElementsBoundByAccessibilityElement.map {
            [$0.label, $0.value as? String ?? ""]
        }.map { $0.joined(separator: "\u{0}") }.sorted()
        return (rows[0].switches.element(boundBy: 0), RemoteLoginInfoAuthority(allowedUsers: allowedUsers, remoteLoginOn: remoteLoginOn))
    }

    private typealias RemoteLoginAuthorization = (owner: XCUIApplication, bundleIdentifier: String, surface: XCUIElement)

    /// Enumerate only trusted OS owners; never launch SecurityAgent.
    @MainActor
    private func remoteLoginAuthorizationSurfaces(_ settings: XCUIApplication) -> [RemoteLoginAuthorization] {
        var surfaces: [RemoteLoginAuthorization] = []
        let agent = XCUIApplication(bundleIdentifier: "com.apple.SecurityAgent")
        if agent.state != .notRunning {
            if agent.windows.count > 0 {
                surfaces += agent.windows.allElementsBoundByIndex.map { (agent, "com.apple.SecurityAgent", $0) }
            } else if agent.secureTextFields.count > 0 || agent.dialogs.count > 0 || agent.alerts.count > 0
                        || agent.sheets.count > 0 {
                surfaces.append((agent, "com.apple.SecurityAgent", agent))
            }
        }
        guard settings.state != .notRunning else { return surfaces }
        let containers = settings.descendants(matching: .sheet).allElementsBoundByIndex
            + settings.descendants(matching: .dialog).allElementsBoundByIndex
            + settings.descendants(matching: .alert).allElementsBoundByIndex
        let leaves = containers.filter {
            $0.descendants(matching: .sheet).count == 0 && $0.descendants(matching: .dialog).count == 0
                && $0.descendants(matching: .alert).count == 0
        }
        let infoRow = NSPredicate(format: "label == %@ OR value == %@",
                                  "Allow full disk access for remote users", "Allow full disk access for remote users")
        let settingsSurfaces = leaves.filter {
            $0.elementType == .dialog || $0.elementType == .alert
                || $0.secureTextFields.count > 0 || $0.staticTexts.matching(infoRow).count == 0
        }
        surfaces += settingsSurfaces.map { (settings, "com.apple.systempreferences", $0) }
        if settingsSurfaces.isEmpty,
           settings.secureTextFields.count > 0
            || settings.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).count > 0 {
            surfaces.append((settings, "com.apple.systempreferences", settings))
        }
        return surfaces
    }

    @MainActor
    private func remoteLoginAuthorizationSurface(_ settings: XCUIApplication) -> XCUIElement? {
        remoteLoginAuthorizationSurfaces(settings).first?.surface
    }

    @MainActor
    private func matchedRemoteLoginAuthorization(_ settings: XCUIApplication, authority: RemoteLoginInfoAuthority) -> RemoteLoginAuthorization? {
        guard remoteLoginUnderlyingInfoFacts(settings)?.authority == authority else { return nil }
        let surfaces = remoteLoginAuthorizationSurfaces(settings)
        guard surfaces.count == 1 else { return nil }
        let authorization = surfaces[0]
        let owner = authorization.owner
        let surface = authorization.surface
        let modify = NSPredicate(format: "label == %@", "Modify Settings")
        let cancel = NSPredicate(format: "label == %@", "Cancel")
        let purpose = NSPredicate(format: "label == %@ OR value == %@",
                                  "Remote Login is trying to modify system settings.",
                                  "Remote Login is trying to modify system settings.")
        let sharing = NSPredicate(format: "label == %@ OR value == %@",
                                  "Enter your password to continue with Sharing.",
                                  "Enter your password to continue with Sharing.")
        guard settings.state != .notRunning, settings.windows.count == 1,
              owner.state != .notRunning, owner.windows.count == 1,
              owner.secureTextFields.count == 1, surface.secureTextFields.count == 1,
              owner.textFields.count == 1, surface.textFields.count == 1,
              surface.secureTextFields.element(boundBy: 0).label == "Password",
              surface.buttons.count == 2,
              surface.buttons.matching(modify).count == 1, owner.buttons.matching(modify).count == 1,
              surface.buttons.matching(cancel).count == 1, owner.buttons.matching(cancel).count == 1,
              surface.staticTexts.matching(purpose).count == 1,
              surface.staticTexts.matching(sharing).count == 1 else { return nil }
        let username = surface.textFields.element(boundBy: 0)
        guard username.placeholderValue == "Username", let account = username.value as? String,
              account.caseInsensitiveCompare(NSUserName()) == .orderedSame else { return nil }
        return authorization
    }

    /// FDA consent has its own opt-in and matcher; it never widens AX provisioning.
    @MainActor
    private func authorizeRemoteLoginIfNeeded(_ settings: XCUIApplication, authority: RemoteLoginInfoAuthority) -> Bool {
        guard remoteLoginAuthorizationSurface(settings) != nil else { return true }
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-remote-login-fda-enable"),
              CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri",
              let authorization = matchedRemoteLoginAuthorization(settings, authority: authority) else {
            reportRemoteLoginFailure(settings, "Refusing unexpected or ambiguous Remote Login authorization; no credential read or input performed")
            return false
        }
        do {
            try authorization.owner.bringToFront(bundleIdentifier: authorization.bundleIdentifier, timeout: 4)
        } catch {
            reportRemoteLoginFailure(settings, "Could not bring the trusted Remote Login authorization owner forward")
            return false
        }
        guard let current = matchedRemoteLoginAuthorization(settings, authority: authority),
              current.bundleIdentifier == authorization.bundleIdentifier else {
            reportRemoteLoginFailure(settings, "Remote Login authorization changed before credential access")
            return false
        }
        let field = current.surface.secureTextFields.element(boundBy: 0)
        let modify = current.surface.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).element(boundBy: 0)
        let cancel = current.surface.buttons.matching(NSPredicate(format: "label == %@", "Cancel")).element(boundBy: 0)
        guard field.isEnabled, field.isHittable, modify.isEnabled, modify.isHittable,
              cancel.isEnabled, cancel.isHittable else {
            reportRemoteLoginFailure(settings, "Remote Login authorization controls are unavailable; no credential read or input performed")
            return false
        }
        guard remoteLoginUnderlyingInfoFacts(settings)?.authority == authority else {
            reportRemoteLoginFailure(settings, "Remote Login Info authority changed before credential access; no credential read or input performed")
            return false
        }
        guard let password = readVMConsentValue() else {
            reportRemoteLoginFailure(settings, "VM authorization credential is unavailable or invalid")
            return false
        }
        vmConsentValue = password
        let pasteboard = NSPasteboard.general
        defer { pasteboard.clearContents() }
        field.click()
        field.typeKey("a", modifierFlags: .command)
        guard let input = matchedRemoteLoginAuthorization(settings, authority: authority),
              input.bundleIdentifier == current.bundleIdentifier else {
            reportRemoteLoginFailure(settings, "Remote Login Info authority or authorization changed before credential input")
            return false
        }
        pasteboard.clearContents()
        let pasted = pasteboard.setString(password, forType: .string)
        if pasted {
            guard let paste = matchedRemoteLoginAuthorization(settings, authority: authority),
                  paste.bundleIdentifier == input.bundleIdentifier else {
                reportRemoteLoginFailure(settings, "Remote Login Info authority or authorization changed before password paste")
                return false
            }
            paste.surface.secureTextFields.element(boundBy: 0).typeKey("v", modifierFlags: .command)
        }
        pasteboard.clearContents()
        guard pasted, let filled = matchedRemoteLoginAuthorization(settings, authority: authority),
              filled.bundleIdentifier == current.bundleIdentifier else {
            reportRemoteLoginFailure(settings, "Could not fill the exact Remote Login authorization form; no authorization click performed")
            return false
        }
        do {
            try filled.owner.bringToFront(bundleIdentifier: filled.bundleIdentifier, timeout: 4)
        } catch {
            reportRemoteLoginFailure(settings, "Could not bring the trusted authorization owner forward to submit")
            return false
        }
        guard let ready = matchedRemoteLoginAuthorization(settings, authority: authority),
              ready.bundleIdentifier == filled.bundleIdentifier else {
            reportRemoteLoginFailure(settings, "Remote Login authorization changed before Modify Settings")
            return false
        }
        let submit = ready.surface.buttons.matching(NSPredicate(format: "label == %@", "Modify Settings")).element(boundBy: 0)
        guard submit.isEnabled, submit.isHittable else {
            reportRemoteLoginFailure(settings, "Exact Remote Login Modify Settings action is unavailable")
            return false
        }
        guard remoteLoginUnderlyingInfoFacts(settings)?.authority == authority else {
            reportRemoteLoginFailure(settings, "Remote Login Info authority changed before Modify Settings; no authorization click performed")
            return false
        }
        // Exactly one submission; failure or a replacement prompt is never retried.
        submit.click()
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.remoteLoginAuthorizationSurface(settings) == nil
        }, object: nil)
        guard XCTWaiter().wait(for: [dismissed], timeout: 10) == .completed else {
            reportRemoteLoginFailure(settings, "Remote Login authorization did not dismiss within ten seconds; no retry performed")
            return false
        }
        return true
    }

    @MainActor
    private func reportRemoteLoginFailure(_ settings: XCUIApplication, _ reason: String, file: StaticString = #filePath, line: UInt = #line) {
        if vmConsentValue != nil { NSPasteboard.general.clearContents() }
        // XCTest can abort without Swift unwinding: evidence must precede XCTFail.
        let fda = NSPredicate(format: "label == %@ OR value == %@",
                              "Allow full disk access for remote users", "Allow full disk access for remote users")
        let on = NSPredicate(format: "label == %@ OR value == %@", "Remote Login: On", "Remote Login: On")
        let facts: [String: Any] = ["settingsRunning": settings.state != .notRunning,
                                   "windowCountUpTo16": min(16, settings.windows.count),
                                   "sheetCountUpTo16": min(16, settings.sheets.count),
                                   "fdaTextCountUpTo16": min(16, settings.staticTexts.matching(fda).count),
                                   "onHeaderTextCountUpTo16": min(16, settings.staticTexts.matching(on).count),
                                   "exactUnderlyingInfoMatched": remoteLoginUnderlyingInfoFacts(settings) != nil]
        if let data = try? JSONSerialization.data(withJSONObject: facts, options: [.prettyPrinted, .sortedKeys]) {
            let evidence = XCTAttachment(string: String(decoding: data, as: UTF8.self))
            evidence.name = "VM Remote Login underlying Info match facts"
            evidence.lifetime = .keepAlways
            add(evidence)
        }
        for authorization in remoteLoginAuthorizationSurfaces(settings).prefix(16) {
            attachRemoteLoginAuthorizationFacts(authorization.surface)
        }
        XCTFail(safeDiagnostic(reason), file: file, line: line)
    }

    @MainActor
    private func attachRemoteLoginAuthorizationFacts(_ surface: XCUIElement) {
        guard surface.exists else { return }
        let infoRow = NSPredicate(format: "label == %@ OR value == %@",
                                  "Allow full disk access for remote users", "Allow full disk access for remote users")
        let embedded = surface.staticTexts.matching(infoRow).count > 0
        let staticTextCount = embedded ? 0 : surface.staticTexts.count
        let buttonCount = embedded ? 0 : surface.buttons.count
        let textFieldCount = surface.textFields.count
        let secureFieldCount = surface.secureTextFields.count
        guard staticTextCount <= 16, buttonCount <= 16,
              textFieldCount <= 16, secureFieldCount <= 16 else { return }
        let staticTexts = embedded ? [] : surface.staticTexts.allElementsBoundByAccessibilityElement
        let buttonElements = embedded ? [] : surface.buttons.allElementsBoundByAccessibilityElement
        let textFields = surface.textFields.allElementsBoundByAccessibilityElement
        let secureFields = surface.secureTextFields.allElementsBoundByAccessibilityElement
        guard surface.exists, staticTexts.count == staticTextCount, buttonElements.count == buttonCount,
              textFields.count == textFieldCount, secureFields.count == secureFieldCount else { return }
        var texts: [[String: String]] = []
        for text in staticTexts {
            guard text.exists else { return }
            texts.append(["label": String(safeDiagnostic(text.label).prefix(512)),
                          "value": String(safeDiagnostic(text.value as? String ?? "").prefix(512))])
        }
        var buttons: [[String: String]] = []
        for button in buttonElements {
            guard button.exists else { return }
            buttons.append(["identifier": String(safeDiagnostic(button.identifier).prefix(256)),
                            "title": String(safeDiagnostic(button.title).prefix(256)),
                            "label": String(safeDiagnostic(button.label).prefix(256))])
        }
        var facts: [String: Any] = ["staticTexts": texts, "buttons": buttons,
                                   "textFieldCount": textFieldCount, "secureFieldCount": secureFieldCount]
        if textFields.count == 1 {
            let username = textFields[0]
            guard username.exists else { return }
            facts["usernameMatchesCurrentAccount"] = (username.value as? String)?.caseInsensitiveCompare(NSUserName()) == .orderedSame
            facts["usernamePlaceholderIsUsername"] = username.placeholderValue == "Username"
        }
        if secureFields.count == 1 {
            let password = secureFields[0]
            guard password.exists else { return }
            facts["passwordLabelIsPassword"] = password.label == "Password"
        }
        guard let data = try? JSONSerialization.data(withJSONObject: facts, options: [.prettyPrinted, .sortedKeys]) else { return }
        let evidence = XCTAttachment(string: safeDiagnostic(String(decoding: data, as: UTF8.self)))
        evidence.name = "VM Remote Login observed authorization prompt facts"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    /// The single recorded Sharing > Remote Login Info route; no settings are changed here.
    @MainActor
    private func openRemoteLoginInfo(_ settings: XCUIApplication, onFailure: ((String) -> Void)? = nil) -> (windows: [XCUIElement], appeared: Bool)? {
        let fail: (String) -> Void = { reason in
            if let onFailure { onFailure(reason) } else { XCTFail(reason) }
        }
        let url = URL(string: "x-apple.systempreferences:com.apple.Sharing-Settings.extension?RemoteLogin")!
        guard NSWorkspace.shared.open(url), settings.wait(for: .runningForeground, timeout: 5) else {
            fail("Could not open System Settings Remote Login pane")
            return nil
        }
        do {
            try settings.bringToFront(bundleIdentifier: "com.apple.systempreferences", timeout: 4)
        } catch {
            fail("Could not bring System Settings forward for Remote Login inspection")
            return nil
        }
        let windows = settings.windows.allElementsBoundByAccessibilityElement
        guard (1...2).contains(windows.count) else {
            fail("Expected one or two System Settings windows; inspection evidence withheld")
            return nil
        }

        let infoIdentifier = "Remote LoginToggle.infoButton"
        let infoButtons = settings.buttons.matching(identifier: infoIdentifier)
        guard infoButtons.firstMatch.waitForExistence(timeout: 5) else {
            fail("Remote Login Info button did not appear; inspection evidence withheld")
            return nil
        }
        // Only the observed detail pane may scroll; never use the sidebar or toggle.
        for attempt in 0...3 {
            guard !settings.secureTextFields.firstMatch.exists,
                  settings.sheets.count == 0, infoButtons.count == 1 else {
                fail("Remote Login navigation is ambiguous or has secure fields; inspection evidence withheld")
                return nil
            }
            let info = settings.buttons[infoIdentifier]
            guard info.label == "Show Detail", info.isEnabled else {
                fail("Remote Login Info button did not match; inspection evidence withheld")
                return nil
            }
            if info.isHittable { break }
            guard attempt < 3 else {
                fail("Remote Login Info button remained offscreen after three scrolls; inspection evidence withheld")
                return nil
            }
            let owners = windows.filter { $0.buttons.matching(identifier: infoIdentifier).count == 1 }
            guard owners.count == 1 else {
                fail("Remote Login Info window is ambiguous; inspection evidence withheld")
                return nil
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
                fail("Remote Login detail scroll view is ambiguous or button is not below it; inspection evidence withheld")
                return nil
            }
            panes[0].scroll(byDeltaX: 0, deltaY: -180)
        }
        guard !settings.secureTextFields.firstMatch.exists,
              settings.sheets.count == 0, infoButtons.count == 1 else {
            fail("Remote Login Info navigation changed; inspection evidence withheld")
            return nil
        }
        let info = settings.buttons[infoIdentifier]
        guard info.label == "Show Detail", info.isEnabled, info.isHittable else {
            fail("Exact Remote Login Info button is unavailable; inspection evidence withheld")
            return nil
        }
        info.click()
        let infoAppearance = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            settings.sheets.count > 0 || settings.windows.count > windows.count
        }, object: nil)
        let appeared = XCTWaiter().wait(for: [infoAppearance], timeout: 5) == .completed
        let inspectionWindows = settings.windows.allElementsBoundByAccessibilityElement
        guard (1...2).contains(inspectionWindows.count),
              settings.windows.count == inspectionWindows.count,
              inspectionWindows.allSatisfy({ $0.exists }) else {
            fail("Expected one or two stable System Settings windows after Info; inspection evidence withheld")
            return nil
        }

        return (inspectionWindows, appeared)
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
    func testRefreshRunnerAccessibilityRegistration() throws {
        continueAfterFailure = false
        guard try runnerRefreshBoundary(), let trusted = recordRunnerIdentityAndTrust() else { return }
        if trusted { return }

        let settings = XCUIApplication(bundleIdentifier: "com.apple.systempreferences")
        defer {
            if settings.state != .notRunning {
                attachSetupEvidence(settings, stage: "final")
                settings.terminate()
            }
            if vmConsentValue != nil { NSPasteboard.general.clearContents() }
            vmConsentValue = nil
        }
        let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
        guard NSWorkspace.shared.open(url), settings.wait(for: .runningForeground, timeout: 10) else {
            reportSetupFailure(settings, "Could not open Settings Accessibility privacy pane for runner refresh")
            return
        }
        _ = settings.staticTexts.matching(identifier: "MeguriUITests-Runner_Title").firstMatch.waitForExistence(timeout: 10)
        guard requireRunnerRefreshPane(settings), let current = refreshRunnerRow(settings) else { return }
        attachSetupEvidence(settings, stage: "before refresh")
        // The row itself can be disabled; select only its unique native title, without coordinates.
        guard click(current.title, in: settings, description: "exact runner title") else { return }
        let selected = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            let rows = current.outline.outlineRows.matching(NSPredicate(format: "selected == true"))
            return rows.count == 1 && current.row.isSelected
                && rows.element(boundBy: 0).staticTexts.matching(identifier: "MeguriUITests-Runner_Title").count == 1
        }, object: nil)
        guard XCTWaiter().wait(for: [selected], timeout: 5) == .completed,
              requireRunnerRefreshPane(settings), let selection = refreshRunnerRow(settings) else {
            reportSetupFailure(settings, "Exact runner row was not uniquely selected; no removal performed")
            return
        }
        let selectedRows = selection.outline.outlineRows.matching(NSPredicate(format: "selected == true"))
        let remove = settings.buttons.matching(NSPredicate(format: "label == %@", "Remove"))
        guard selection.row.isSelected, selectedRows.count == 1,
              selectedRows.element(boundBy: 0).staticTexts.matching(identifier: "MeguriUITests-Runner_Title").count == 1,
              remove.count == 1, remove.element(boundBy: 0).isEnabled, remove.element(boundBy: 0).isHittable else {
            reportSetupFailure(settings, "Runner selection or enabled Remove action is ambiguous; no removal performed")
            return
        }
        guard click(remove.element(boundBy: 0), in: settings, description: "selected runner Remove") else { return }
        let removalStarted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.refreshRunnerRows(settings).isEmpty || self.runnerRefreshHasSurface(settings)
        }, object: nil)
        guard XCTWaiter().wait(for: [removalStarted], timeout: 10) == .completed,
              requireRunnerRefreshPane(settings) else {
            reportSetupFailure(settings, "Runner removal did not complete normally; no further input performed")
            return
        }
        let removed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.refreshRunnerRows(settings).isEmpty || self.runnerRefreshHasSurface(settings)
        }, object: nil)
        guard XCTWaiter().wait(for: [removed], timeout: 10) == .completed,
              !runnerRefreshHasSurface(settings), refreshRunnerRows(settings).isEmpty,
              settings.staticTexts.matching(identifier: "MeguriUITests-Runner_Title").count == 0 else {
            reportSetupFailure(settings, "Runner row remains or an unexpected surface appeared after removal; no Add performed")
            return
        }

        guard addCurrentRunner(settings) else { return }
        let readded = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            if self.hasAuthorizationPrompt(settings) { return true }
            if self.runnerRefreshHasSurface(settings) {
                guard let picker = self.pickerContainer(settings) else { return true }
                return !self.isAppPicker(picker)
            }
            return !self.refreshRunnerRows(settings).isEmpty
        }, object: nil)
        guard XCTWaiter().wait(for: [readded], timeout: 10) == .completed,
              requireRunnerRefreshPane(settings), let added = refreshRunnerRow(settings) else {
            reportSetupFailure(settings, "Expected exactly one readded runner row without unexpected surfaces")
            return
        }
        if remoteLoginBoolean(added.control) == false {
            guard click(added.control, in: settings, description: "readded runner switch") else { return }
            let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                self.remoteLoginBoolean(added.control) == true || self.runnerRefreshHasSurface(settings)
            }, object: nil)
            guard XCTWaiter().wait(for: [changed], timeout: 10) == .completed,
                  requireRunnerRefreshPane(settings) else {
                reportSetupFailure(settings, "Readded runner switch did not enable normally")
                return
            }
        }
        let enabled = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.remoteLoginBoolean(added.control) == true || self.runnerRefreshHasSurface(settings)
        }, object: nil)
        guard XCTWaiter().wait(for: [enabled], timeout: 10) == .completed,
              !runnerRefreshHasSurface(settings), let configured = refreshRunnerRow(settings),
              remoteLoginBoolean(configured.control) == true else {
            reportSetupFailure(settings, "Readded runner registration is not uniquely enabled")
            return
        }
        let evidence = XCTAttachment(string: "Runner registration configured; awaiting fresh-process prompt-free trust verification with the same frozen runner identity.")
        evidence.name = "VM runner registration refresh completion"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    /// Run alone in a new test runner/job; two methods in one process do not prove relaunch.
    @MainActor
    func testCheckRunnerAccessibilityTrust() throws {
        continueAfterFailure = false
        guard try runnerRefreshBoundary(), let trusted = recordRunnerIdentityAndTrust() else { return }
        XCTAssertTrue(trusted, "Fresh runner AX trust is false; compare the frozen runner identity with refresh evidence")
    }

    @MainActor
    private func runnerRefreshBoundary() throws -> Bool {
        guard FileManager.default.fileExists(atPath: "/tmp/mg-vm-ax-refresh-runner"),
              FileManager.default.fileExists(atPath: "/tmp/mg-vm-ax-enable") else {
            throw XCTSkip("VM runner refresh requires both Accessibility opt-ins")
        }
        guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
            XCTFail("Runner refresh/check requires the dedicated VM's meguri account")
            return false
        }
        return true
    }

    @MainActor
    private func recordRunnerIdentityAndTrust() -> Bool? {
        guard let runnerURL = currentRunnerBundleURL(),
              runnerURL.lastPathComponent == "MeguriUITests-Runner.app" else {
            XCTFail("Current bundle is not the exact expected runner app")
            return nil
        }

        let runnerExecutableDirectory = runnerURL.appendingPathComponent("Contents/MacOS").resolvingSymlinksInPath()
        guard let executable = Bundle.main.executableURL?.resolvingSymlinksInPath(),
              executable.lastPathComponent == "MeguriUITests-Runner",
              executable.deletingLastPathComponent() == runnerExecutableDirectory else {
            XCTFail("Current executable does not have the expected name and resolved runner directory")
            return nil
        }

        guard let runnerBundle = Bundle(url: runnerURL),
              let bundleExecutable = runnerBundle.executableURL?.resolvingSymlinksInPath(),
              bundleExecutable == executable else {
            XCTFail("The exact runner bundle does not independently identify the current executable")
            return nil
        }
        let bytes: Data
        do {
            bytes = try Data(contentsOf: executable)
        } catch {
            XCTFail("Could not read the current runner executable for SHA256 identity")
            return nil
        }
        guard !bytes.isEmpty else {
            XCTFail("Current runner executable is empty; no identity emitted")
            return nil
        }
        let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        let trusted = AXIsProcessTrusted()
        let evidence = XCTAttachment(string: "bundleIdentifier=com.antoinefink.meguri.dev.uitests.xctrunner\nexecutableSHA256=\(digest)\naxTrusted=\(trusted)")
        evidence.name = "VM runner code identity and prompt-free trust"
        evidence.lifetime = .keepAlways
        add(evidence)
        return trusted
    }

    @MainActor
    private func runnerRefreshHasSurface(_ settings: XCUIApplication) -> Bool {
        settings.descendants(matching: .sheet).count > 0 || settings.descendants(matching: .dialog).count > 0
            || settings.alerts.count > 0 || settings.secureTextFields.count > 0
    }

    @MainActor
    private func requireRunnerRefreshPane(_ settings: XCUIApplication) -> Bool {
        // Only the existing exact Settings authorization matcher may consume credentials or approve.
        guard settings.alerts.count == 0, authorizeSettingsIfNeeded(settings), !runnerRefreshHasSurface(settings) else {
            reportSetupFailure(settings, "Unexpected Settings surface during runner refresh; no generic confirmation performed")
            return false
        }
        return true
    }

    @MainActor
    private func refreshRunnerRows(_ settings: XCUIApplication) -> [XCUIElement] {
        let names = ["MeguriUITests-Runner", "MeguriUITests Runner"]
        return runnerRows(settings, predicate: NSPredicate(format: "label IN %@ OR value IN %@", names, names))
    }

    @MainActor
    private func refreshRunnerRow(_ settings: XCUIApplication) -> (row: XCUIElement, title: XCUIElement, outline: XCUIElement, control: XCUIElement)? {
        let titleID = "MeguriUITests-Runner_Title"
        let titles = settings.staticTexts.matching(identifier: titleID)
        let outlines = settings.outlines.containing(.staticText, identifier: titleID)
        guard refreshRunnerRows(settings).count == 1, titles.count == 1, outlines.count == 1 else {
            reportSetupFailure(settings, "Expected one exact runner row/title and containing outline")
            return nil
        }
        let outline = outlines.element(boundBy: 0)
        let rows = outline.outlineRows.containing(.staticText, identifier: titleID)
        guard rows.count == 1 else {
            reportSetupFailure(settings, "Runner title is not contained in exactly one row of its outline")
            return nil
        }
        let row = rows.element(boundBy: 0)
        let names = ["MeguriUITests-Runner", "MeguriUITests Runner"]
        let exactName = NSPredicate(format: "label IN %@ OR value IN %@", names, names)
        let controls = row.switches.matching(identifier: "MeguriUITests-Runner_Toggle")
        guard row.staticTexts.matching(exactName).matching(identifier: titleID).count == 1,
              row.switches.count == 1, row.checkBoxes.count == 0, controls.count == 1,
              remoteLoginBoolean(controls.element(boundBy: 0)) != nil else {
            reportSetupFailure(settings, "Exact runner title or its single boolean switch is missing or unknown")
            return nil
        }
        return (row, titles.element(boundBy: 0), outline, controls.element(boundBy: 0))
    }

    @MainActor
    private func runnerRows(_ settings: XCUIApplication, predicate: NSPredicate) -> [XCUIElement] {
        settings.outlineRows.containing(predicate).allElementsBoundByIndex
            + settings.tableRows.containing(predicate).allElementsBoundByIndex
    }

    private func currentRunnerBundleURL() -> URL? {
        let runnerURL = Bundle.main.bundleURL
        guard runnerURL.isFileURL, runnerURL.pathExtension == "app",
              Bundle.main.bundleIdentifier == "com.antoinefink.meguri.dev.uitests.xctrunner",
              Bundle(url: runnerURL)?.bundleIdentifier == "com.antoinefink.meguri.dev.uitests.xctrunner" else {
            return nil
        }
        return runnerURL
    }

    @MainActor
    private func addCurrentRunner(_ settings: XCUIApplication) -> Bool {
        guard let runnerURL = currentRunnerBundleURL() else {
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
