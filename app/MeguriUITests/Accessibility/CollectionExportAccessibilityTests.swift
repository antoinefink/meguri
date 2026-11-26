import Foundation
import XCTest
#if os(iOS)
import UIKit
#endif

@MainActor final class CollectionExportAccessibilityTests: XCTestCase {
    func testExportStatesPassAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["error", "typical", "loading"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "settings/import-export/\(state)"),
                waitFor: "settings.importExport.content", suppressions: suppressions,
                supplemental: { app, _ in
                    // N17.3: the sheet is progress, or a message with its actions. It has no steps.
                    let close = app.buttons["settings.importExport.close"].firstMatch
                    XCTAssertTrue(close.exists)
                    XCTAssertEqual(close.label, "Cancel")
                    XCTAssertEqual(close.isEnabled, state != "loading", "Cancel waits while the system save dialog owns the file")
                    for removed in ["explanation", "prepare", "discard", "ready", "readyDetail", "saved"] {
                        XCTAssertFalse(app.descendants(matching: .any)["settings.importExport.\(removed)"].firstMatch.exists, removed)
                    }
                    let progress = app.descendants(matching: .any)["settings.importExport.progress"].firstMatch
                    let message = app.descendants(matching: .any)["settings.importExport.message"].firstMatch
                    if state == "error" {
                        XCTAssertTrue(message.exists && !progress.exists)
                    } else {
                        XCTAssertTrue(progress.exists && !message.exists)
                        XCTAssertEqual(progress.label, state == "loading" ? "Saving…" : "Preparing Export…")
                    }
                    #if os(macOS)
                    for identifier in ["shell.root.sidebarDecksEmpty", "shell.root.sidebar.home"] {
                        XCTAssertFalse(app.descendants(matching: .any)[identifier].firstMatch.exists,
                            "The underlying shell must leave the AX tree while the export sheet is active: \(identifier)")
                    }
                    #endif
                    return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: try TouchTargets.violations(in: app))
                })
        }
        add(AccessibilityAudit.screenReport("settings.importExport", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }
    func testDeckOptionsUseNamedNativeControlsAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires public view fixtures") }
        #if os(macOS)
        guard try build.bundleIdentifier.hasSuffix(".dev") else { throw XCTSkip("VM development build required") }
        #endif
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["options", "options-changed"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "collection/deck-export/\(state)"),
                waitFor: "home.deckExport.content", suppressions: suppressions,
                supplemental: { app, _ in
                    let labels = ["scheduling": "Include scheduling information",
                                  "presets": "Include deck presets", "media": "Include media"]
                    for (option, label) in labels {
                        let control = app.descendants(matching: .any)["home.deckExport.\(option)"].firstMatch
                        XCTAssertTrue(control.exists && control.isEnabled)
                        XCTAssertEqual(control.label, label)
                    }
                    XCTAssertTrue(app.buttons["home.deckExport.export"].firstMatch.isEnabled)
                    XCTAssertTrue(app.buttons["home.deckExport.close"].firstMatch.isEnabled)
                    return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: try TouchTargets.violations(in: app))
                })
        }
        add(AccessibilityAudit.screenReport("settings.importExport", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }

}
