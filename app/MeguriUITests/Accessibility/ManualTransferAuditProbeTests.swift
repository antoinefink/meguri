import Foundation
import XCTest

/// Isolated diagnostic measurements of the inert empty route, not acceptance evidence.
@MainActor final class ManualTransferAuditProbeTests: XCTestCase {
    func testIsolatedContrastAndDynamicType() throws {
        #if os(iOS)
        continueAfterFailure = true
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires development rendering hooks") }
        let bundle = try build.bundleIdentifier
        guard bundle == "com.antoinefink.meguri.dev", bundle == build.expectedBundleIdentifier else {
            throw XCTSkip("Requires the development app bundle")
        }
        let suppressions = try AuditSuppressions.bundled()
        let appearance = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = appearance }

        let categories: [(name: String, type: XCUIAccessibilityAuditType)] = [
            ("contrast", .contrast), ("dynamicType", .dynamicType),
        ]
        for largestText in [false, true] {
            let variant = AccessibilityAudit.Variant(largestText: largestText, dark: false)
            for category in categories {
                let probe = "manual-transfer-diagnostic-probe-\(largestText ? "AX5" : "default")-light-\(category.name)"
                variant.apply()
                let app = XCUIApplication(bundleIdentifier: bundle)
                app.launchArguments = MeguriLaunch(locale: "en_US", animations: false,
                    route: "settings/anki-transfer/empty", extra: variant.launchArguments).arguments
                app.launch()
                defer { app.terminate() }

                let root = app.descendants(matching: .any).matching(identifier: "settings.anki.content").firstMatch
                let window = app.windows.firstMatch
                guard root.waitForExistence(timeout: 15), window.exists,
                      app.state == .runningForeground,
                      !app.sheets.firstMatch.exists, !app.alerts.firstMatch.exists else {
                    XCTFail("\(probe): inert route or app window was not ready")
                    continue
                }
                let frame = window.frame
                let visible = root.frame.intersection(frame)
                guard !frame.isNull, !frame.isInfinite, !frame.isEmpty,
                      [frame.minX, frame.minY, frame.maxX, frame.maxY, frame.width, frame.height].allSatisfy(\.isFinite),
                      !visible.isNull, !visible.isInfinite, !visible.isEmpty,
                      [visible.minX, visible.minY, visible.maxX, visible.maxY].allSatisfy(\.isFinite) else {
                    XCTFail("\(probe): app window or content bounds were invalid")
                    continue
                }
                do {
                    guard try AccessibilityAudit.Appearance.shown(by: app).dark == variant.dark else {
                        XCTFail("\(probe): light appearance did not apply")
                        continue
                    }
                } catch {
                    XCTFail("\(probe): Appearance unavailable")
                    continue
                }

                let shot = XCTAttachment(screenshot: window.screenshot())
                shot.name = "\(probe)-pre-audit.png"
                shot.lifetime = .keepAlways
                add(shot)

                let lines = ["settings.anki.saveEndpoint", "settings.anki.login"].map { identifier in
                    let element = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
                    let exists = element.exists
                    let bounds = exists ? element.frame : .zero
                    let finite = exists && [bounds.origin.x, bounds.origin.y, bounds.width, bounds.height].allSatisfy(\.isFinite)
                    let enabled = exists ? " enabled=\(element.isEnabled)" : ""
                    return "id=\(identifier) exists=\(exists)\(enabled) finite=\(finite) " +
                        "x=\(bounds.origin.x.isFinite ? bounds.origin.x : 0) " +
                        "y=\(bounds.origin.y.isFinite ? bounds.origin.y : 0) " +
                        "width=\(bounds.width.isFinite ? bounds.width : 0) " +
                        "height=\(bounds.height.isFinite ? bounds.height : 0)"
                }
                let metadata = XCTAttachment(string: lines.joined(separator: "\n"))
                metadata.name = "\(probe)-pre-audit-controls.txt"
                metadata.lifetime = .keepAlways
                add(metadata)

                do {
                    let findings = try AccessibilityAudit.run(app, variant: probe,
                        types: category.type, suppressions: suppressions)
                    AccessibilityAudit.assertClean(findings)
                } catch {
                    XCTFail("\(probe): Audit unavailable")
                }
            }
        }
        #else
        throw XCTSkip("iOS-only diagnostic audit probe")
        #endif
    }
}
