import CoreGraphics
import Foundation
import XCTest
#if os(macOS)
import AppKit
typealias PlatformScreenshotImage = NSImage
#else
import UIKit
typealias PlatformScreenshotImage = UIImage
#endif

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

/// The audit could not run as asked (a screen that never appeared, an
/// appearance that did not apply).
struct AuditError: Error, CustomStringConvertible, Equatable {
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
    /// Why the finding is the window's and not the app's (the Mac only;
    /// `AccessibilityAudit.systemElement`): listed in the report, not failed.
    var excluded: String? = nil

    /// Whether the finding fails the audit.
    var counts: Bool { !suppressed && excluded == nil }

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

        /// Launch arguments: on the Mac the app's own appearance
        /// (`-MeguriAppearance`, NSApp.appearance), so a light run never
        /// inherits a dark workstation's (with `-AppleInterfaceStyle Light`
        /// the app showed its dark background on a Mac in dark mode); on iOS the text size (the appearance is set
        /// on the device, see `apply`).
        var launchArguments: [String] {
            #if os(macOS)
            ["-MeguriAppearance", dark ? "dark" : "light"]
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

    /// Why an element the audit reports belongs to the window rather than
    /// the app, if it does. On the Mac the audit also covers the window's
    /// own elements: its close, minimize and full-screen buttons (`_XCUI:`
    /// identifiers) and title bar and toolbar elements that carry neither an
    /// identifier nor a label (the first Mac runs found
    /// sufficientElementDescription and contrast issues only on those). The
    /// app's elements carry an identifier or a label (R1964) and are audited;
    /// the others are listed with their type and frame, not failed, until
    /// U253's sweep decides how the Mac audit treats them. iOS audits every
    /// element.
    static func systemElement(identifier: String, label: String) -> String? {
        #if os(macOS)
        if identifier.hasPrefix("_XCUI:") { return "the window's own control" }
        if identifier.isEmpty && label.isEmpty { return "a window element with neither identifier nor label, not the app's (U253's sweep)" }
        #endif
        return nil
    }

    /// Runs the audit; every issue comes back as a finding, suppressed when
    /// a suppression names its element and its type exactly, excluded when
    /// it is the window's (`systemElement`).
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
            let label = snapshot?.label ?? ""
            let finding = AuditFinding(
                variant: variant, identifier: identifier, label: label, element: element, auditType: type,
                detail: "\(issue.compactDescription) (\(issue.detailedDescription))", suppressed: suppressed,
                excluded: systemElement(identifier: identifier, label: label)
            )
            findings.append(finding)
            if finding.counts {
                // What the audit saw: the window, kept with the result.
                let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                shot.name = "a11y-finding \(variant) \(type) \(element).png"
                shot.lifetime = .keepAlways
                XCTContext.runActivity(named: "accessibility finding: \(finding)") { $0.add(shot) }
            }
            // Handled here: unsuppressed findings fail through `assertClean`,
            // with the identifier and type in the message.
            return true
        }
        return findings
    }

    /// One state of a screen, audited in every variant.
    struct StateResult {
        let state: String
        let findings: [AuditFinding]
        let touchTargets: [TouchTargetViolation]

        var passed: Bool { !findings.contains(where: \.counts) && touchTargets.isEmpty }
    }

    /// Launches `launch` once per variant, checks that the variant's
    /// appearance is the one the app shows, and audits the screen it opens
    /// and its touch targets. Optional readiness runs once after the anchor
    /// appears, before any screenshot or audit. The device appearance is
    /// restored afterwards. Supplemental states use the same running app after
    /// each main audit, and contribute to the same result.
    @MainActor
    static func runAllVariants(
        state: String, launch: MeguriLaunch, waitFor identifier: String, suppressions: [AuditSuppression],
        ready: (@MainActor (XCUIApplication) throws -> Void)? = nil,
        supplemental: (@MainActor (XCUIApplication, String) throws -> StateResult)? = nil
    ) throws -> StateResult {
        #if !os(macOS)
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }
        #endif
        var findings: [AuditFinding] = []
        var touchTargets: [TouchTargetViolation] = []
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
                throw AuditError(description: "\(identifier) did not appear (\(state), \(variant))")
            }
            try ready?(app)
            let shown = try Appearance.shown(by: app)
            guard shown.dark == variant.dark else {
                throw AuditError(description: "\(state), \(variant): the app shows a \(shown.dark ? "dark" : "light") screen (median luminance \(String(format: "%.2f", shown.luminance))); the variant's appearance did not apply")
            }
            let mainLabel = supplemental == nil ? state : "\(state) main"
            findings += try run(app, variant: "\(mainLabel), \(variant)", suppressions: suppressions)
            for violation in try TouchTargets.violations(in: app) where !touchTargets.contains(violation) {
                touchTargets.append(violation)
            }
            if let extra = try supplemental?(app, variant.description) {
                findings += extra.findings
                for violation in extra.touchTargets where !touchTargets.contains(violation) {
                    touchTargets.append(violation)
                }
            }
        }
        return StateResult(state: state, findings: findings, touchTargets: touchTargets)
    }

    /// The appearance an app's window shows: dark when most of it is dark.
    enum Appearance {
        @MainActor
        static func shown(by app: XCUIApplication) throws -> (dark: Bool, luminance: Double) {
            let luminance = try medianLuminance(app.windows.firstMatch.screenshot().image)
            return (luminance < 0.5, luminance)
        }

        /// The median relative luminance (0 black, 1 white) of the image,
        /// sampled on a 64×64 grid.
        static func medianLuminance(_ image: PlatformScreenshotImage) throws -> Double {
            #if os(macOS)
            guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { throw AuditError(description: "the screenshot has no bitmap") }
            #else
            guard let cgImage = image.cgImage else { throw AuditError(description: "the screenshot has no bitmap") }
            #endif
            let side = 64
            var bytes = [UInt8](repeating: 0, count: side * side * 4)
            guard let context = CGContext(
                data: &bytes, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { throw AuditError(description: "could not sample the screenshot") }
            context.draw(cgImage, in: CGRect(x: 0, y: 0, width: side, height: side))
            let values = stride(from: 0, to: bytes.count, by: 4).map { index in
                (0.2126 * Double(bytes[index]) + 0.7152 * Double(bytes[index + 1]) + 0.0722 * Double(bytes[index + 2])) / 255
            }.sorted()
            return values[values.count / 2]
        }
    }

    /// The screen's result as a JSON attachment `a11y-screen.<screen id>.json`,
    /// with a result per state; `tools/mg a11y` lists these as per-screen
    /// evidence (the critic's `--a11y`, the status ledger). The screen passes
    /// only when every state does.
    static func screenReport(_ screen: String, states: [StateResult]) -> XCTAttachment {
        let attachment = XCTAttachment(data: screenReportData(screen, states: states), uniformTypeIdentifier: "public.json")
        attachment.name = "a11y-screen.\(screen).json"
        attachment.lifetime = .keepAlways
        return attachment
    }

    /// The report's JSON.
    static func screenReportData(_ screen: String, states: [StateResult]) -> Data {
        func findingsJSON(_ result: StateResult) -> [[String: Any]] {
            result.findings.map {
                var finding: [String: Any] = [
                    "state": result.state, "variant": $0.variant, "identifier": $0.identifier, "label": $0.label, "element": $0.element,
                    "auditType": $0.auditType, "detail": $0.detail, "suppressed": $0.suppressed,
                ]
                if let excluded = $0.excluded { finding["excluded"] = excluded }
                return finding
            }
        }
        let report: [String: Any] = [
            "screen": screen,
            "status": states.allSatisfy(\.passed) && !states.isEmpty ? "pass" : "fail",
            "variants": Variant.all.map(\.description),
            "states": Dictionary(uniqueKeysWithValues: states.map { result in
                (result.state, [
                    "status": result.passed ? "pass" : "fail",
                    "findings": findingsJSON(result),
                    "touchTargets": result.touchTargets.map(\.description),
                ] as [String: Any])
            }),
            "findings": states.flatMap(findingsJSON),
            "touchTargets": states.flatMap { result in result.touchTargets.map { "\(result.state): \($0)" } },
        ]
        return (try? JSONSerialization.data(withJSONObject: report, options: [.sortedKeys, .prettyPrinted])) ?? Data()
    }

    /// Fails once per finding that counts (neither suppressed nor the
    /// window's).
    static func assertClean(_ findings: [AuditFinding], file: StaticString = #filePath, line: UInt = #line) {
        for finding in findings where finding.counts {
            XCTFail("accessibility audit: \(finding)", file: file, line: line)
        }
    }
}
