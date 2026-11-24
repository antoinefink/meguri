#if os(macOS)
import Foundation
import XCTest

/// Opt-in guest UI only. MG owns inventory, leases, captures and result export.
final class SimulatorPostureTests: XCTestCase {
    struct Request: Decodable {
        let udid: String
        let name: String
        let posture: String
        let bundleID: String
        let nonce: String
        let mutationUntil: TimeInterval
    }
    struct Refusal: Error { let id: String }
    private let labels = ["Closed", "Partially Open", "Open"]
    private var diagnostics: [String: Any] = [:]

    @MainActor
    func testRequestedPosture() throws {
        guard let raw = ProcessInfo.processInfo.environment["MEGURI_POSTURE_REQUEST"] else {
            throw XCTSkip("tools/mg sim-posture opt-in absent")
        }
        let request = try JSONDecoder().decode(Request.self, from: Data(raw.utf8))
        var report: [String: Any] = ["udid": request.udid, "name": request.name,
            "posture": request.posture, "nonce": request.nonce, "changed": false]
        do {
            guard CardSwapTimingGate.hypervisorPresent() == true, NSUserName() == "meguri" else {
                throw Refusal(id: "guest-session")
            }
            guard UUID(uuidString: request.udid) != nil,
                  request.name.range(of: "^Meguri-Pool-foldable-[0-9]+$", options: .regularExpression) != nil,
                  ["open", "closed"].contains(request.posture),
                  request.bundleID == "com.apple.dt.Devices" else {
                throw Refusal(id: "invalid-target")
            }
            let hub = XCUIApplication(bundleIdentifier: request.bundleID)
            guard hub.state == .runningBackground || hub.state == .runningForeground else {
                throw Refusal(id: "hub-not-running")
            }
            hub.activate()
            var controls = try readyControls(hub, request)
            let desired = request.posture == "open" ? "Open" : "Closed"
            let already = try selected(controls) == desired
            if !already {
                controls = try postureControls(hub, request.name)
                _ = try selected(controls)
                let button = controls[desired]!
                guard button.isEnabled, button.isHittable else { throw Refusal(id: "posture-not-actionable") }
                try mutationAllowed(request)
                button.click() // Single posture mutation; never retry a click.
                report["changed"] = true
            }
            let end = Date().addingTimeInterval(5)
            repeat {
                controls = try postureControls(hub, request.name)
                if try selected(controls) == desired {
                    report.merge(["status": "pass", "reason": "control-verified",
                                  "control_verified": true, "changed": !already], uniquingKeysWith: { _, new in new })
                    break
                }
                Thread.sleep(forTimeInterval: 0.2)
            } while Date() < end
            if report["status"] == nil { throw Refusal(id: "posture-state-mismatch") }
        } catch let refusal as Refusal {
            report["status"] = refusal.id == "posture-state-mismatch" || refusal.id == "invalid-target" ? "fail" : "unavailable"
            report["reason"] = refusal.id
        } catch {
            report["status"] = "fail"
            report["reason"] = "guest-operation"
        }
        report["diagnostics"] = diagnostics
        let attachment = XCTAttachment(data: try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]),
                                       uniformTypeIdentifier: "public.json")
        attachment.name = "mg-posture.json"
        attachment.lifetime = .keepAlways
        add(attachment)
        // MG interprets the receipt, including failures. Avoid XCTest's automatic
        // failure hierarchy, which could include unrelated device labels.
    }

    private func mutationAllowed(_ request: Request) throws {
        let remaining = request.mutationUntil - Date().timeIntervalSince1970
        guard remaining > 5, remaining <= 90 else { throw Refusal(id: "mutation-deadline") }
    }

    static func namesTarget(_ title: String, _ name: String) -> Bool {
        let tokens = title.components(separatedBy: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_")).inverted)
        return tokens.filter { $0.hasPrefix("Meguri-Pool-") } == [name]
    }

    @MainActor
    private func named(_ scope: XCUIElement, _ label: String) -> XCUIElementQuery {
        scope.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR identifier == %@", label, label))
    }

    @MainActor
    private func controls(_ scope: XCUIElement) -> [String: XCUIElement]? {
        var result: [String: XCUIElement] = [:]
        var counts: [String: Int] = [:]
        let types = [XCUIElement.ElementType.button, .radioButton, .checkBox].map(\.rawValue)
        for label in labels {
            let matches = named(scope, label).matching(NSPredicate(format: "elementType IN %@", types))
            counts[label] = matches.count
            if matches.count == 1 { result[label] = matches.element }
        }
        diagnostics["postureControls"] = counts
        return result.count == labels.count ? result : nil
    }

    @MainActor
    private func simulatorRows(_ hub: XCUIElement, _ name: String) -> [XCUIElement] {
        // Never navigate a Devices/physical-device row. The simulator outline
        // or its enclosing Simulators group must supply the context.
        let outlines = hub.outlines.allElementsBoundByIndex.filter { outline in
            outline.label == "Simulators" || named(outline, "Simulators").count == 1
        }
        let groups = hub.groups.allElementsBoundByIndex.filter {
            $0.label == "Simulators" && $0.outlines.count == 1
        }
        let scopes = outlines.isEmpty ? groups.flatMap { $0.outlines.allElementsBoundByIndex } : outlines
        return scopes.flatMap { $0.outlineRows.allElementsBoundByIndex }.filter { named($0, name).count == 1 }
    }

    @MainActor
    private func postureControls(_ hub: XCUIApplication, _ name: String) throws -> [String: XCUIElement] {
        let windows = hub.windows.allElementsBoundByIndex
        let targets = windows.filter { Self.namesTarget($0.label, name) }
        diagnostics["hubWindows"] = windows.count
        diagnostics["namedControls"] = ["Simulators": named(hub, "Simulators").count,
                                        "Inspector": named(hub, "Inspector").count,
                                        "Devices": named(hub, "Devices").count]
        diagnostics["targetWindows"] = targets.count
        let rows = simulatorRows(hub, name)
        diagnostics["targetSimulatorRows"] = rows.count
        guard targets.count <= 1 else { throw Refusal(id: "ambiguous-target-windows") }
        if let window = targets.first {
            // A unique inventory name in a display window is simulator context.
            if let result = controls(window) { return result }
            throw Refusal(id: "compact-posture-controls")
        }
        // Expanded library: selected simulator row plus the same name in a
        // detail group without navigation. Inspector need not expose a UUID.
        let selectedRows = rows.filter(\.isSelected)
        diagnostics["selectedTargetSimulatorRows"] = selectedRows.count
        guard rows.count == 1, selectedRows.count == 1 else { throw Refusal(id: "simulator-row-selection") }
        let libraries = windows.filter { simulatorRows($0, name).filter(\.isSelected).count == 1 }
        diagnostics["targetLibraryWindows"] = libraries.count
        guard libraries.count == 1 else { throw Refusal(id: "simulator-library-window") }
        let panes = libraries[0].groups.allElementsBoundByIndex.filter {
            $0.outlines.count == 0 && $0.tables.count == 0 && $0.outlineRows.count == 0 && $0.tableRows.count == 0 &&
            named($0, name).count == 1 &&
            $0.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Meguri-Pool-' AND label != %@", name)).count == 0
        }
        // Keep the innermost complete detail scope to avoid counting nested
        // wrappers twice. Competing complete detail scopes remain ambiguous.
        let matches = panes.filter { pane in
            !pane.groups.allElementsBoundByIndex.contains { child in
                named(child, name).count == 1 && controls(child) != nil
            }
        }.compactMap { controls($0) }
        diagnostics["targetDetailPanes"] = matches.count
        guard matches.count == 1, let result = matches.first else {
            throw Refusal(id: "expanded-posture-controls")
        }
        return result
    }

    @MainActor
    private func readyControls(_ hub: XCUIApplication, _ request: Request) throws -> [String: XCUIElement] {
        do {
            return try postureControls(hub, request.name)
        } catch {
            // Preserve the actual failed match. Navigation is authorized only
            // when there is no named display window and one unselected simulator.
            guard (diagnostics["targetWindows"] as? Int ?? 0) == 0 else { throw error }
            let rows = simulatorRows(hub, request.name)
            diagnostics["targetSimulatorRows"] = rows.count
            guard rows.count == 1 else { throw Refusal(id: "simulator-row-count") }
            guard !rows[0].isSelected else { throw error }
            guard rows[0].isHittable else { throw Refusal(id: "simulator-row-not-actionable") }
            try mutationAllowed(request)
            rows[0].click()
            return try postureControls(hub, request.name)
        }
    }

    static func selection(_ values: [String: Bool]) throws -> String {
        let chosen = values.filter { $0.value }
        guard Set(values.keys) == Set(["Closed", "Partially Open", "Open"]), chosen.count == 1 else {
            throw Refusal(id: "posture-selection")
        }
        return chosen.first!.key
    }

    @MainActor
    private func selected(_ controls: [String: XCUIElement]) throws -> String {
        let values = controls.mapValues {
            $0.isSelected || ($0.value as? NSNumber)?.boolValue == true || ($0.value as? String) == "1"
        }
        diagnostics["selectedControls"] = values
        return try Self.selection(values)
    }

    func testTargetWindowAndDistinctSelectionPolicy() throws {
        let name = "Meguri-Pool-foldable-1"
        XCTAssertTrue(Self.namesTarget(name + " – Inner Display", name))
        XCTAssertFalse(Self.namesTarget(name + "0", name))
        XCTAssertFalse(Self.namesTarget(name + " Meguri-Pool-foldable-2", name))
        XCTAssertEqual(try Self.selection(["Closed": false, "Partially Open": true, "Open": false]), "Partially Open")
        XCTAssertThrowsError(try Self.selection(["Closed": true, "Partially Open": false, "Open": true]))
        XCTAssertThrowsError(try Self.selection(["Closed": false, "Partially Open": false, "Open": false]))
    }
}
#endif
