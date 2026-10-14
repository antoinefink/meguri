import Foundation
import XCTest

/// One suppressed system-audit issue (R1977): exactly one element
/// identifier, one audit type, and the reason it is not a defect. The only
/// place suppressions live is `Accessibility/suppressions.json`.
struct AuditSuppression: Decodable, Equatable {
    let identifier: String
    let auditType: String
    let reason: String
}

struct SuppressionError: Error, CustomStringConvertible, Equatable {
    let description: String
}

/// Loads `suppressions.json`. A suppression without an element identifier,
/// with an unknown audit type, or without a reason fails the load, and with
/// it every audit that uses the file.
enum AuditSuppressions {
    /// Audit type names, as `suppressions.json` and findings write them.
    static let typeNames: [(name: String, type: XCUIAccessibilityAuditType)] = {
        var names: [(String, XCUIAccessibilityAuditType)] = [
            ("contrast", .contrast), ("elementDetection", .elementDetection), ("hitRegion", .hitRegion),
            ("sufficientElementDescription", .sufficientElementDescription),
        ]
        #if os(macOS)
        names += [("action", .action), ("parentChild", .parentChild)]
        #else
        names += [("dynamicType", .dynamicType), ("textClipped", .textClipped), ("trait", .trait)]
        #endif
        return names
    }()

    /// Every name a suppression may use, on either platform.
    static let allTypeNames: Set<String> = [
        "contrast", "elementDetection", "hitRegion", "sufficientElementDescription",
        "dynamicType", "textClipped", "trait", "action", "parentChild",
    ]

    static func name(of type: XCUIAccessibilityAuditType) -> String {
        typeNames.first { $0.type == type }?.name ?? "unknown(\(type.rawValue))"
    }

    private struct File: Decodable {
        let version: Int
        let suppressions: [Entry]
    }

    private struct Entry: Decodable {
        let identifier: String?
        let auditType: String?
        let reason: String?
    }

    /// The suppressions file in this test bundle.
    static func bundled() throws -> [AuditSuppression] {
        let url = try XCTUnwrap(Bundle(for: Marker.self).url(forResource: "suppressions", withExtension: "json"), "suppressions.json is not in the test bundle")
        return try parse(Data(contentsOf: url))
    }

    static func parse(_ data: Data) throws -> [AuditSuppression] {
        let file: File
        do {
            file = try JSONDecoder().decode(File.self, from: data)
        } catch {
            throw SuppressionError(description: "suppressions.json is not {\"version\": 1, \"suppressions\": [...]}: \(error)")
        }
        guard file.version == 1 else { throw SuppressionError(description: "suppressions.json version \(file.version); expected 1") }
        var result: [AuditSuppression] = []
        for (index, entry) in file.suppressions.enumerated() {
            let label = "suppression \(index + 1) (\(entry.identifier ?? "no identifier"), \(entry.auditType ?? "no audit type"))"
            guard let identifier = entry.identifier?.trimmingCharacters(in: .whitespaces), !identifier.isEmpty, !identifier.contains("*") else {
                throw SuppressionError(description: "\(label): needs the identifier of exactly one element")
            }
            guard let type = entry.auditType, allTypeNames.contains(type) else {
                throw SuppressionError(description: "\(label): audit type must be one of \(allTypeNames.sorted().joined(separator: ", "))")
            }
            guard let reason = entry.reason?.trimmingCharacters(in: .whitespacesAndNewlines), !reason.isEmpty else {
                throw SuppressionError(description: "\(label): has no reason; every suppression needs a written reason (R1977)")
            }
            let suppression = AuditSuppression(identifier: identifier, auditType: type, reason: reason)
            guard !result.contains(where: { $0.identifier == identifier && $0.auditType == type }) else {
                throw SuppressionError(description: "\(label): listed twice")
            }
            result.append(suppression)
        }
        return result
    }

    private final class Marker {}
}

/// One issue the system audit reported.
struct AuditFinding: Equatable, CustomStringConvertible {
    let variant: String
    let identifier: String
    let label: String
    /// The element's type and frame, for findings on elements without an
    /// identifier (often the system's own chrome).
    let element: String
    let auditType: String
    let detail: String
    let suppressed: Bool

    var description: String {
        let name = identifier.isEmpty ? "an element without identifier (\"\(label)\", \(element))" : identifier
        return "\(variant): \(auditType) on \(name): \(detail)"
    }
}

/// The system accessibility audit (R1977) on the app's current screen.
enum AccessibilityAudit {
    /// A text size and appearance the audit runs in.
    struct Variant: CustomStringConvertible {
        let largestText: Bool
        let dark: Bool

        var description: String { "\(dark ? "dark" : "light"), \(largestText ? "largest accessibility size" : "default size")" }

        /// Default and largest accessibility size, light and dark. The Mac
        /// has no Dynamic Type setting: default size only.
        static var all: [Variant] {
            #if os(macOS)
            [Variant(largestText: false, dark: false), Variant(largestText: false, dark: true)]
            #else
            [false, true].flatMap { largest in [false, true].map { Variant(largestText: largest, dark: $0) } }
            #endif
        }

        /// Launch arguments for the text size (the appearance is set on the
        /// device, see `apply`).
        var launchArguments: [String] {
            #if os(macOS)
            dark ? ["-AppleInterfaceStyle", "Dark"] : []
            #else
            largestText ? ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"] : []
            #endif
        }

        @MainActor
        func apply() {
            #if !os(macOS)
            XCUIDevice.shared.appearance = dark ? .dark : .light
            #endif
        }
    }

    /// Runs the audit; every issue comes back as a finding, suppressed when
    /// a suppression names its element and its type exactly.
    @MainActor
    static func run(
        _ app: XCUIApplication, variant: String, types: XCUIAccessibilityAuditType = .all, suppressions: [AuditSuppression]
    ) throws -> [AuditFinding] {
        var findings: [AuditFinding] = []
        try app.performAccessibilityAudit(for: types) { issue in
            let snapshot = try? issue.element?.snapshot()
            let identifier = snapshot?.identifier ?? ""
            let type = AuditSuppressions.name(of: issue.auditType)
            let suppressed = !identifier.isEmpty && suppressions.contains { $0.identifier == identifier && $0.auditType == type }
            let element = snapshot.map { "type \($0.elementType.rawValue) at \(Int($0.frame.minX)),\(Int($0.frame.minY)) \(Int($0.frame.width))×\(Int($0.frame.height))" } ?? "no element"
            findings.append(AuditFinding(
                variant: variant, identifier: identifier, label: snapshot?.label ?? "", element: element, auditType: type,
                detail: issue.compactDescription, suppressed: suppressed
            ))
            // Handled here: unsuppressed findings fail through `assertClean`,
            // with the identifier and type in the message.
            return true
        }
        return findings
    }

    /// Launches `launch` once per variant and audits the screen it opens.
    /// The device appearance is restored afterwards.
    @MainActor
    static func runAllVariants(
        launch: MeguriLaunch, waitFor identifier: String, suppressions: [AuditSuppression]
    ) throws -> [AuditFinding] {
        #if !os(macOS)
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }
        #endif
        var findings: [AuditFinding] = []
        for variant in Variant.all {
            variant.apply()
            let app = XCUIApplication()
            var arguments = launch
            arguments.extra += variant.launchArguments
            app.launchArguments = arguments.arguments
            app.launch()
            defer { app.terminate() }
            let anchor = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
            guard anchor.waitForExistence(timeout: 15) else {
                throw SuppressionError(description: "\(identifier) did not appear (\(variant))")
            }
            findings += try run(app, variant: variant.description, suppressions: suppressions)
        }
        return findings
    }

    /// The screen's result as a JSON attachment `a11y-screen.<screen id>.json`;
    /// `tools/mg a11y` lists these as per-screen evidence (the critic's
    /// `--a11y`, the status ledger).
    static func screenReport(_ screen: String, findings: [AuditFinding], touchTargets: [TouchTargetViolation]) -> XCTAttachment {
        let unsuppressed = findings.filter { !$0.suppressed }
        let report: [String: Any] = [
            "screen": screen,
            "status": unsuppressed.isEmpty && touchTargets.isEmpty ? "pass" : "fail",
            "variants": Variant.all.map(\.description),
            "findings": findings.map {
                ["variant": $0.variant, "identifier": $0.identifier, "label": $0.label, "element": $0.element, "auditType": $0.auditType, "detail": $0.detail, "suppressed": $0.suppressed]
            },
            "touchTargets": touchTargets.map(\.description),
        ]
        let data = (try? JSONSerialization.data(withJSONObject: report, options: [.sortedKeys, .prettyPrinted])) ?? Data()
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "a11y-screen.\(screen).json"
        attachment.lifetime = .keepAlways
        return attachment
    }

    /// Fails once per unsuppressed finding.
    static func assertClean(_ findings: [AuditFinding], file: StaticString = #filePath, line: UInt = #line) {
        for finding in findings where !finding.suppressed {
            XCTFail("accessibility audit: \(finding)", file: file, line: line)
        }
    }
}
