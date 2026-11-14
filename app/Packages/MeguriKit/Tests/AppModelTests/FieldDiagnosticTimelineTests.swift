import CoreBridge
import Foundation
import Instrumentation
import MetricKit
import Synchronization
import Testing

@testable import AppModel

@Suite(.serialized) struct FieldDiagnosticTimelineTests {
    static let instant = Date(timeIntervalSince1970: 1_800_000_000)
    static let observation = FieldDiagnosticObservation(component: .cloud, operation: .exchange, stage: .exchangeCompleted,
                                                        outcome: .incomplete, failure: .other, count: 3, durationMilliseconds: 42)

    func location() -> URL {
        FileManager.default.temporaryDirectory.appending(path: "DiagnosticTimeline-\(UUID().uuidString)", directoryHint: .isDirectory)
    }
    func json(_ bytes: Data) throws -> [String: Any] {
        try #require(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
    }

    @Test func observationsPersistReopenAndKeepExchangeCompletionDistinctFromConvergence() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        store.enqueue(Self.observation)
        store.enqueue(.init(component: .cloud, operation: .status, stage: .statusObserved, outcome: .waiting))
        let written = await store.timeline()
        #expect(written.events.count == 2)
        #expect(written.events[0].observation.stage == .exchangeCompleted)
        #expect(written.events[0].observation.outcome == .incomplete)
        #expect(written.events.allSatisfy { $0.observation.stage != .convergenceObserved })
        #expect(written.events.allSatisfy { $0.observedAtUnixMilliseconds == 1_800_000_000_000 })
        let reopened = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let restored = await reopened.timeline()
        #expect(restored.events == written.events)
        #expect(restored.storage == .available && !restored.priorHistoryUnavailable)
    }

    @Test func newest512AreRetainedAndAgeUsesInjectedTimeIncludingTheBoundary() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let time = Mutex(Self.instant)
        let store = FieldDiagnosticsStore(directory: directory, clock: AppClock { time.withLock { $0 } })
        // Flush small bursts so this tests retention rather than queue loss.
        for batch in 0..<18 {
            for index in 0..<32 {
                store.enqueue(.init(component: .review, operation: .save, stage: .persistence,
                                    outcome: .failed, count: UInt64(batch * 32 + index)))
            }
            _ = await store.timeline()
        }
        let capped = await store.timeline()
        #expect(capped.events.count == 512 && capped.truncatedCount == 64 && capped.droppedCount == 0)
        #expect(capped.events.first?.observation.count == 64)
        #expect(capped.events.last?.observation.count == 575)
        time.withLock { $0 = Self.instant.addingTimeInterval(7 * 24 * 60 * 60) }
        #expect(await store.timeline().events.count == 512)
        time.withLock { $0 = Self.instant.addingTimeInterval(7 * 24 * 60 * 60 + 1) }
        let afterRetentionWindow = await store.timeline()
        #expect(afterRetentionWindow.events.isEmpty && afterRetentionWindow.expiredCount == 512)
        let reopened = FieldDiagnosticsStore(directory: directory, clock: .fixed(time.withLock { $0 }))
        #expect(await reopened.timeline().expiredCount == 512)
    }

    @Test func boundedInboxDropsWithoutSchedulingMoreConsumersAndReportsLoss() {
        let inbox = FieldDiagnosticInbox()
        let event = FieldDiagnosticEvent(observedAtUnixMilliseconds: 1_800_000_000_000, observation: Self.observation)
        #expect(inbox.enqueue(event))
        for _ in 1..<64 { #expect(!inbox.enqueue(event)) }
        for _ in 0..<1000 { #expect(!inbox.enqueue(event)) }
        let batch = inbox.take(finishingIfEmpty: true)
        #expect(batch.events.count == 64 && batch.dropped == 1000 && !batch.finished)
        // While a consumer is active, taking a batch cannot start a second.
        #expect(!inbox.enqueue(event))
        #expect(inbox.take(finishingIfEmpty: true).events.count == 1)
        #expect(inbox.take(finishingIfEmpty: true).finished)
        #expect(inbox.enqueue(event))
    }

    @Test func invalidClockDropsAreObservableAndPersistedWithoutInventingTimestamps() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Date(timeIntervalSince1970: .infinity)))
        store.enqueue(Self.observation)
        let timeline = await store.timeline()
        #expect(timeline.events.isEmpty && timeline.droppedCount == 1)
        let info = try json(await store.debugInfo())
        let header = try #require(info["header"] as? [String: Any])
        #expect(header["generatedAtUTC"] == nil)
        let reopened = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        #expect(await reopened.timeline().droppedCount == 1)
    }

    @Test(arguments: ["corrupt", "oversized", "unsupported"])
    func damagedHistoryIsExplicitAndRepairsToABoundedSnapshot(kind: String) async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let file = directory.appending(path: "timeline.json")
        let bytes: Data
        switch kind {
        case "corrupt": bytes = Data("{\"events\":".utf8)
        case "oversized": bytes = Data(repeating: 32, count: 1_048_577)
        default:
            var value = try json(JSONEncoder().encode(FieldDiagnosticTimeline.empty))
            value["schemaVersion"] = 999
            bytes = try JSONSerialization.data(withJSONObject: value)
        }
        try bytes.write(to: file)
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let snapshot = await store.timeline()
        #expect(snapshot.events.isEmpty && snapshot.priorHistoryUnavailable)
        let reason: FieldDiagnosticTimeline.Storage = kind == "corrupt" ? .corrupt : (kind == "oversized" ? .oversized : .unsupportedSchema)
        #expect(snapshot.priorHistoryUnavailableReason == reason)
        #expect(try Data(contentsOf: file).count <= 1_048_576)
        store.enqueue(Self.observation)
        #expect(await store.timeline().events.count == 1)
        let reopened = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let restored = await reopened.timeline()
        #expect(restored.events.count == 1 && restored.priorHistoryUnavailableReason == reason)
    }

    @Test func diskFailureLeavesObservationsAvailableAndExportDoesNotThrowOrDescribeErrors() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        try Data("not a directory".utf8).write(to: directory)
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        store.enqueue(Self.observation)
        let snapshot = await store.timeline()
        #expect(snapshot.events.count == 1 && snapshot.storage == .unavailable)
        let output = try json(await store.debugInfo())
        #expect((output["timeline"] as? [String: Any])?["storage"] as? String == "unavailable")
        #expect(output["header"] != nil)
        // Subscriber catches metric and diagnostic write failures too; it
        // finishes its streams rather than propagating a storage error.
        try await FieldDiagnosticsStoreTests().deliver([FieldDiagnosticsStoreTests.metricReport()],
                                                       [FieldDiagnosticsStoreTests.hangDiagnostic()], to: store)
        HostileDiagnosticError.accesses.withLock { $0 = 0 }
        #expect(FieldDiagnosticObservation.Failure.storageFailure(for: HostileDiagnosticError()) == .storageUnavailable)
        #expect(HostileDiagnosticError.accesses.withLock { $0 } == 0)
    }

    @Test func safeHeaderAndTimelineExportNeedNoCollection() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let info = try json(await store.debugInfo())
        let header = try #require(info["header"] as? [String: Any])
        #expect(header["schemaVersion"] as? Int == 1)
        #expect(header["generatedAtUTC"] as? String == "2027-01-15T08:00:00.000Z")
        #expect(header["osFamily"] != nil && header["osVersion"] != nil)
        #expect(header["appVersion"] != nil || header["appVersionUnavailable"] as? Bool == true)
        #expect(header["appBuild"] != nil || header["appBuildUnavailable"] as? Bool == true)
        #expect(Set(header.keys).isSubset(of: ["schemaVersion", "generatedAtUTC", "appVersion", "appBuild", "appVersionUnavailable", "appBuildUnavailable", "osFamily", "osVersion"]))
        #expect((info["metrics"] as? [Any])?.isEmpty == true)
        #expect((info["diagnostics"] as? [Any])?.isEmpty == true)
        #expect((info["rootReads"] as? [Any])?.isEmpty == true)
        #expect(info["collectionDetailsUnavailable"] as? Bool == true)
        #expect(SafeFieldDiagnosticsExport.Header.safeVersion("1.2.3") == "1.2.3")
        #expect(SafeFieldDiagnosticsExport.Header.safeVersion("/private/token-secret") == nil)
        #expect(SafeFieldDiagnosticsExport.Header.safeVersion(String(repeating: "1", count: 100_000)) == nil)
    }

    @Test func hostileMetricDiagnosticAndRootStringsStayInternal() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let secret = "credential-content-private-binary-path-canary"
        let hostileJSON = FieldDiagnosticsStoreTests.hangDiagnosticJSON
            .replacingOccurrences(of: "\"applicationVersion\": \"1.0\"", with: "\"applicationVersion\": \"\(secret)\"")
            .replacingOccurrences(of: "\"binaryInfo\": []", with: "\"binaryInfo\": [], \"binaryPath\": \"\(secret)\"")
        let diagnostic = try JSONDecoder().decode(DiagnosticReport.self, from: Data(hostileJSON.utf8))
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant), rootReadDiagnostics: {
            [.init(active: false, lostCollection: false, outcome: secret, a7Failure: true, accepted: false)]
        })
        try await store.record(diagnostic)
        try await store.record(FieldDiagnosticsStoreTests.metricReport())
        #expect(try await store.diagnostics().first?.applicationVersion == secret)
        // Put arbitrary metadata in a stored metric; SDK decoding must never
        // make that metadata exportable merely because it is Encodable.
        let metricDirectory = directory.appending(path: "metrics")
        let metricFile = try #require(FileManager.default.contentsOfDirectory(at: metricDirectory, includingPropertiesForKeys: nil).first)
        var metric = try json(Data(contentsOf: metricFile))
        metric["applicationVersion"] = secret
        metric["arbitraryPayload"] = ["token": secret, "path": secret]
        try JSONSerialization.data(withJSONObject: metric).write(to: metricFile)
        let bytes = try await store.debugInfo()
        let info = try json(bytes)
        #expect(!String(decoding: bytes, as: UTF8.self).contains(secret))
        let projected = try #require((info["diagnostics"] as? [[String: Any]])?.first)
        #expect(Set(projected.keys) == ["receivedAtUnixMilliseconds", "reportTime", "kind", "detailsUnavailable"])
        #expect((info["rootReads"] as? [[String: Any]])?.first?["outcome"] as? String == "other")
        #expect((info["rootReads"] as? [[String: Any]])?.first?["a7Failure"] as? Bool == true)
        #expect(info["coreSnapshotIsAtomic"] as? Bool == false)
    }

    @Test func oversizedCorruptAndExcessReportsAreCountedWithoutDeletingMetricStorage() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        try await store.record(FieldDiagnosticsStoreTests.metricReport())
        let folder = directory.appending(path: "metrics")
        try Data(repeating: 32, count: 65_537).write(to: folder.appending(path: "z-oversized.json"))
        try Data("{unfinished".utf8).write(to: folder.appending(path: "z-corrupt.json"))
        let bytes = try await store.debugInfo()
        let info = try json(bytes)
        let sections = try #require(info["sections"] as? [String: [String: Any]])
        #expect(sections["metrics"]?["oversizedCount"] as? Int == 1)
        #expect(sections["metrics"]?["corruptCount"] as? Int == 1)
        #expect((info["metrics"] as? [Any])?.count == 1)
        #expect(try Data(contentsOf: folder.appending(path: "z-oversized.json")).count == 65_537)
        for index in 0..<600 {
            try Data("{}".utf8).write(to: folder.appending(path: "sample-\(index).json"))
        }
        let capped = try await store.debugInfo()
        let cappedInfo = try json(capped)
        let cappedSections = try #require(cappedInfo["sections"] as? [String: [String: Any]])
        #expect(capped.count <= 1_048_576)
        #expect(cappedSections["metrics"]?["scanTruncated"] as? Bool == true)
        #expect((cappedSections["metrics"]?["omittedCountLowerBound"] as? Int ?? 0) > 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: folder.path(percentEncoded: false)).count == 603)
    }

    @Test func exportByteCapIncludesNestedDeletionFactsAndKeepsOriginalCloudCaps() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let failures = (0..<100).map { _ in CloudZoneDeletionResult(zone: "private-zone-canary", failure: .init(code: .max, retryAfterMilliseconds: .max, encryptedDataReset: true)) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant), zoneDeletionDiagnostics: {
            Array(repeating: .deleted(results: failures, terminal: .init(code: .max)), count: 200)
        }, rootReadDiagnostics: {
            Array(repeating: .init(active: false, lostCollection: false, outcome: "found", a7Failure: true, accepted: true), count: 100)
        })
        store.enqueue(Self.observation)
        let bytes = try await store.debugInfo()
        #expect(bytes.count <= 1_048_576)
        let info = try json(bytes)
        let deletions = try #require(info["zoneDeletions"] as? [[String: Any]])
        #expect(deletions.count <= 128)
        #expect((info["rootReads"] as? [Any])?.count == 64)
        #expect(deletions.allSatisfy { $0["resultsOmittedCount"] as? Int == 36 })
        let sections = try #require(info["sections"] as? [String: [String: Any]])
        #expect((sections["zoneDeletions"]?["omittedCountLowerBound"] as? Int ?? 0) >= 72)
        #expect(sections["rootReads"]?["omittedCountLowerBound"] as? Int == 36)
        #expect(!String(decoding: bytes, as: UTF8.self).contains("private-zone-canary"))
        #expect((info["timeline"] as? [String: Any])?["events"] as? [Any] != nil)
    }

    @Test func maximumScalarsAndExcessPersistedEventsStayWithinEncodedByteLimits() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let observation = FieldDiagnosticObservation(component: .collectionExport, operation: .publish, stage: .publication,
                                                     outcome: .unknown, failure: .storageUnavailable,
                                                     count: .max, durationMilliseconds: .max)
        var persisted = FieldDiagnosticTimeline.empty
        persisted.events = Array(repeating: FieldDiagnosticEvent(observedAtUnixMilliseconds: 1_800_000_000_000,
                                                               observation: observation), count: 700)
        persisted.droppedCount = .max
        let file = directory.appending(path: "timeline.json")
        try JSONEncoder().encode(persisted).write(to: file)
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let timeline = await store.timeline()
        #expect(timeline.events.count == 512 && timeline.truncatedCount == 188)
        #expect(timeline.events.allSatisfy { $0.observation.count == .max && $0.observation.durationMilliseconds == .max })
        #expect(timeline.droppedCount == .max)
        #expect(try Data(contentsOf: file).count <= 1_048_576)
        #expect(try await store.debugInfo().count <= 1_048_576)
        let reopened = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        #expect(await reopened.timeline().events.count == 512)
    }

    @Test func timelineNeverFollowsASymlinkToAnotherFile() async throws {
        let directory = location()
        let outside = location()
        defer { try? FileManager.default.removeItem(at: directory); try? FileManager.default.removeItem(at: outside) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let secret = Data("private-content-canary".utf8)
        try secret.write(to: outside)
        try FileManager.default.createSymbolicLink(at: directory.appending(path: "timeline.json"), withDestinationURL: outside)
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let timeline = await store.timeline()
        #expect(timeline.priorHistoryUnavailableReason == .unavailable)
        #expect(try Data(contentsOf: outside) == secret)
        #expect(!String(decoding: try await store.debugInfo(), as: UTF8.self).contains("private-content-canary"))
    }
}

private nonisolated final class HostileDiagnosticError: Error, LocalizedError, CustomStringConvertible, CustomDebugStringConvertible, CustomNSError, @unchecked Sendable {
    static let accesses = Mutex(0)
    static func touched() -> String { accesses.withLock { $0 += 1 }; return "private-error-canary" }
    static var errorDomain: String { touched() }
    var errorCode: Int { _ = Self.touched(); return 42 }
    var errorUserInfo: [String: Any] { [Self.touched(): Self.touched()] }
    var errorDescription: String? { Self.touched() }
    var description: String { Self.touched() }
    var debugDescription: String { Self.touched() }
}
