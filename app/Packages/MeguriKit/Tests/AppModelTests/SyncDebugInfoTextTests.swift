@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// Copy Debug Info's text (first-run report): readable, ordered, content-free.
@Suite struct SyncDebugInfoTextTests {
    private func export(events: Int) throws -> Data {
        let event: [String: Any] = ["observedAtUnixMilliseconds": 1_894_701_600_000,
            "observation": ["component": "startup", "operation": "open", "stage": "admission",
                            "outcome": "refused", "failure": "unavailable"]]
        let cloud: [String: Any] = ["observedAtUnixMilliseconds": 1_894_701_600_001,
            "observation": ["component": "cloud", "operation": "status", "stage": "statusObserved", "outcome": "waiting"]]
        let root: [String: Any] = [
            "header": ["schemaVersion": 1, "generatedAtUTC": "2026-11-20T17:30:00.000Z", "appVersion": "1.0",
                       "appBuild": "7", "osFamily": "macOS", "osVersion": ["major": 27, "minor": 2, "patch": 0]],
            "timeline": ["schemaVersion": 1, "truncatedCount": 38, "droppedCount": 0,
                         "events": Array(repeating: event, count: 2) + Array(repeating: cloud, count: max(0, events - 2))],
            "cloudDiagnosticsAvailable": true, "statusEvidenceUnavailable": true]
        return try JSONSerialization.data(withJSONObject: root)
    }
    private let blocked = SyncDebugFacts(service: "meguri", providerPhase: "blocked", blockedSource: .admissionClosed,
        meguriConnection: "setupRequired", meguriSetupStage: "media", statusTitleKey: "sync.status.needsAttention.title",
        statusDetailKey: "sync.needsYou.blocked")

    @Test func summaryComesFirstThenThePrettyExportThenTheTimeline() throws {
        let text = try SyncDebugInfoText.compose(export: export(events: 5), sync: blocked)
        let lines = text.components(separatedBy: "\n")
        #expect(lines.first == "== Meguri Debug Info ==")
        #expect(text.contains("Service: meguri  Provider phase: blocked"))
        #expect(text.contains("Blocked by: admissionClosed"))
        #expect(text.contains("Meguri: connection setupRequired, setup media, routine none"))
        #expect(text.contains("Timeline: 5 events, 38 truncated, 0 dropped"))
        #expect(text.contains("By component and outcome: cloud waiting ×3, startup refused ×2"))
        #expect(text.contains("Failures: startup/open/admission/refused/unavailable ×2"))
        let exportStart = try #require(text.range(of: "== Export ==\n"))
        let timelineStart = try #require(text.range(of: "\n== Timeline ==\n"))
        #expect(exportStart.upperBound < timelineStart.lowerBound)
        // The export section is one pretty-printed object with sorted keys, the timeline taken out.
        let body = String(text[exportStart.upperBound..<timelineStart.lowerBound])
        let object = try #require(try JSONSerialization.jsonObject(with: Data(body.utf8)) as? [String: Any])
        #expect(object["timeline"] == nil)
        #expect((object["sync"] as? [String: Any])?["blockedSource"] as? String == "admissionClosed")
        #expect(body.contains("\n  \"cloudDiagnosticsAvailable\""))
        let keys = body.components(separatedBy: "\n").filter { $0.hasPrefix("  \"") }.map { $0.dropFirst(3).prefix { $0 != "\"" } }
        #expect(keys == keys.sorted())
        let timeline = try JSONSerialization.jsonObject(with: Data(text[timelineStart.upperBound...].utf8)) as? [String: Any]
        #expect((timeline?["events"] as? [Any])?.count == 5)
    }

    @Test func missingSyncFactsAreSaidNotGuessed() throws {
        let text = try SyncDebugInfoText.compose(export: export(events: 2), sync: nil)
        #expect(text.contains("Sync: unavailable"))
        #expect(text.contains("\"syncUnavailable\" : true"))
    }

    @Test func anOversizedTextDropsTheOldestEventsAndCountsThem() throws {
        let full = try SyncDebugInfoText.compose(export: export(events: 400), sync: blocked)
        let limit = full.utf8.count / 2
        let bounded = try SyncDebugInfoText.compose(export: export(events: 400), sync: blocked, limit: limit)
        #expect(bounded.utf8.count <= limit)
        let start = try #require(bounded.range(of: "\n== Timeline ==\n"))
        let timeline = try #require(try JSONSerialization.jsonObject(with: Data(bounded[start.upperBound...].utf8)) as? [String: Any])
        let kept = try #require(timeline["events"] as? [Any]).count
        #expect(kept < 400 && kept > 0)
        #expect((timeline["truncatedCount"] as? NSNumber)?.intValue == 38 + 400 - kept)
    }

    @Test func failureReasonsAreKindsOnly() {
        #expect(ProviderFailureReason(BridgeError.busy(runningOperation: "private words")) == .busy)
        #expect(ProviderFailureReason(BridgeError.unavailable(reason: "private words")) == .unavailable)
        #expect(ProviderFailureReason(CancellationError()) == .cancelled)
        let facts = try? JSONEncoder().encode(SyncDebugFacts(service: "meguri", providerPhase: "blocked",
            blockedSource: .settingsWrite, blockedReason: ProviderFailureReason(BridgeError.busy(runningOperation: "private words")),
            statusTitleKey: "sync.status.needsAttention.title"))
        #expect(facts.map { String(decoding: $0, as: UTF8.self).contains("private") } == false)
    }
}
