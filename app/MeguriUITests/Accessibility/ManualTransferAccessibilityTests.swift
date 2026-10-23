import Foundation
import XCTest

/// Initial all-types audits use the existing exact suppression receipt scopes.
/// No manual-transfer contrast or hit-target exception is added here.
@MainActor final class ManualTransferAccessibilityTests: XCTestCase {
    func testExplicitTransferSetupStatesAtEveryTextSizeAndAppearance() throws {
        continueAfterFailure = false
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("Requires rendering routes") }
        let suppressions = try AuditSuppressions.bundled()
        let states = try ["empty", "typical", "error", "loading", "overflow",
                          "media-consent", "media-progress", "media-partial", "media-complete", "media-unknown"].map { state in
            try AccessibilityAudit.runAllVariants(state: state,
                launch: MeguriLaunch(locale: "en_US", animations: false, route: "settings/anki-transfer/\(state)"),
                waitFor: "settings.anki.content", suppressions: suppressions,
                ready: { app in
                    if state == "media-progress" {
                        XCTAssertTrue(app.staticTexts["settings.anki.progress"].waitForExistence(timeout: 5))
                    }
                },
                supplemental: { app, variant in
                    guard state == "overflow" || state.hasPrefix("media-") else {
                        return AccessibilityAudit.StateResult(state: state, findings: [], touchTargets: [])
                    }
                    return try self.auditMedia(state, app: app, variant: variant, build: build, suppressions: suppressions)
                })
        }
        add(AccessibilityAudit.screenReport("settings.sync", states: states))
        for state in states { AccessibilityAudit.assertClean(state.findings); TouchTargets.assertMet(state.touchTargets) }
    }

    private func auditMedia(_ state: String, app: XCUIApplication, variant: String, build: BuildUnderTest,
                            suppressions: [AuditSuppression]) throws -> AccessibilityAudit.StateResult {
        let consent = state == "overflow" || state == "media-consent"
        let complete = state == "media-complete"
        let unknown = "Current exclusions are unknown until a fresh media check completes. Recorded transfer counts retain saved effects from earlier attempts."
        var messages = ["Fixture only: simulated media effects. No server work or acceptance."]
        var controls: [(String, String)] = []
        if consent {
            messages += [
                "Import and export both reconcile media in both directions. Server files are downloaded and pending local files are uploaded, even when the collection does not reference them. Counts become available after the media check.",
                "A server file can overwrite a pending local edit, moving the previous local file to trash. A server deletion can move an unchanged local file to trash. Files marked for local deletion may return through download; local deletions may also be sent to the server.",
                "Repairing names and resolving case conflicts may rename files or remove server names. References in collection fields are not rewritten. Files that are empty or larger than 100 MiB are left out.",
                "Allowing media applies these effects across every supported file and batch, including changes that occur during the run. Stopping keeps saved changes and does not restore either side. Reviews remain available during media transfer.", unknown,
            ]
            controls = [("startMedia", "Allow Both-Side Media Changes and Start"), ("cancelPreparedMedia", "Keep Media for Later")]
        } else if state == "media-progress" {
            messages += ["Reconciling media on this device and server. Reviews remain available.", "Receiving media files…",
                         "48000", "28800", "3200", "480", "234567890 / 123456789", "6400", "128", unknown]
            controls = [("cancel", "Cancel Operation")]
        } else if complete {
            messages += ["Collection and supported media complete. Excluded files were not transferred.",
                         "f-0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789.wav",
                         "f-9876543210987654321098765432109876543210987654321098765432109876543210987654321098765432109876543210.png",
                         "Excluded: over 100 MiB", "Excluded: empty file",
                         "9600 files, 23 deletions, 987654321 file bytes recorded.",
                         "12480 files, 17 deletions, 123456789 file bytes recorded."]
        } else {
            messages += ["Collection complete. The whole selected operation is incomplete.",
                         "Cancelled. The operation is partial; saved effects remain.",
                         "3200 files, 23 deletions, 234567890 file bytes recorded.", unknown]
            controls = [("resumeMedia.render-selected-media", "Review Media Decision / Resume…")]
            if state == "media-unknown" { messages += ["Additional server effects may have completed without a saved acknowledgement."] }
        }
        let bundle = try build.bundleIdentifier
        try app.bringToFront(bundleIdentifier: bundle)
        let surface = try ManualTransferFixtureUI.scrollSurface(app)
        let targets = messages.map { ManualTransferFixtureUI.text($0, in: app) }
            + controls.map { app.buttons["settings.anki." + $0.0].firstMatch }
        try ManualTransferFixtureUI.reveal(targets[0], in: app, bundle: bundle)
        var checked: Set<Int> = []
        var textEnds: [Int: CGFloat] = [:]
        var findings: [AuditFinding] = []
        var touchTargets: [TouchTargetViolation] = []
        // Overlapping viewport audits cover the long disclosure paragraphs as
        // well as complete action frames. Never exempt clipped app content.
        for gesture in 0...48 {
            let viewport = try ManualTransferFixtureUI.viewport(surface, app: app)
            for (index, target) in targets.enumerated() where target.exists && !target.frame.isEmpty {
                let frame = target.frame
                if index >= messages.count {
                    if viewport.contains(frame) && target.isHittable { checked.insert(index) }
                } else {
                    let portion = frame.intersection(viewport)
                    guard !portion.isNull, !portion.isEmpty else { continue }
                    let start = max(0, portion.minY - frame.minY), end = min(frame.height, portion.maxY - frame.minY)
                    if start <= (textEnds[index] ?? 0) + 1 { textEnds[index] = max(textEnds[index] ?? 0, end) }
                    if (textEnds[index] ?? 0) >= frame.height - 1 { checked.insert(index) }
                }
            }
            findings += try AccessibilityAudit.run(app, variant: "\(state) scroll \(gesture), \(variant)", suppressions: suppressions)
            for violation in try TouchTargets.violations(in: app) where !touchTargets.contains(violation) { touchTargets.append(violation) }
            if checked.count == targets.count { break }
            guard gesture < 48 else { break }
            try ManualTransferFixtureUI.scroll(surface, down: true, app: app, bundle: bundle)
        }
        XCTAssertEqual(checked.count, targets.count, "Selected-media content must be covered within 48 scoped scroll gestures: \(state)")
        for (id, label) in controls {
            let button = app.buttons["settings.anki." + id].firstMatch
            try ManualTransferFixtureUI.reveal(button, in: app, bundle: bundle)
            guard button.exists else { throw AuditError(description: "Revealed synthetic control disappeared") }
            XCTAssertEqual(button.label, label)
            XCTAssertTrue(button.isEnabled)
        }
        let prepare = app.buttons["settings.anki.prepareExport"].firstMatch
        try ManualTransferFixtureUI.reveal(prepare, in: app, bundle: bundle, mode: .visibleDisabled)
        guard prepare.exists else { throw AuditError(description: "Revealed synthetic Prepare disappeared") }
        XCTAssertFalse(prepare.isEnabled, "Rendering must not gain collection activation ownership")
        XCTAssertFalse(ManualTransferFixtureUI.text("The server acknowledged the collection.", in: app).exists)
        if complete {
            XCTAssertFalse(ManualTransferFixtureUI.text(unknown, in: app).exists)
            XCTAssertFalse(app.buttons["settings.anki.resumeMedia.render-selected-media"].exists)
        } else {
            XCTAssertFalse(ManualTransferFixtureUI.text("Excluded: over 100 MiB", in: app).exists)
            XCTAssertFalse(ManualTransferFixtureUI.text("Excluded: empty file", in: app).exists)
        }
        return AccessibilityAudit.StateResult(state: state, findings: findings, touchTargets: touchTargets)
    }
}
