import CoreBridge
import Darwin
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
        let temporary = FileManager.default.temporaryDirectory.path(percentEncoded: false)
        guard let resolved = temporary.withCString({ realpath($0, nil) }) else { preconditionFailure("Temporary root unavailable") }
        defer { free(resolved) }
        return URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
            .appending(path: "DiagnosticTimeline-\(UUID().uuidString)", directoryHint: .isDirectory)
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

    @Test func clockRollbackKeepsAdmissionOrderAcrossFlushReopenExportAndEviction() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let time = Mutex(Self.instant)
        let clock = AppClock { time.withLock { $0 } }
        let store = FieldDiagnosticsStore(directory: directory, clock: clock)
        store.enqueue(.init(component: .cloud, operation: .start, stage: .requested, outcome: .observed, count: 0))
        _ = await store.timeline()
        time.withLock { $0 = Self.instant.addingTimeInterval(-1) }
        store.enqueue(.init(component: .cloud, operation: .start, stage: .admission, outcome: .failed, count: 1))
        let written = await store.timeline()
        #expect(written.events.map(\.observation.stage) == [.requested, .admission])
        #expect(written.events.map(\.observedAtUnixMilliseconds) == [1_800_000_000_000, 1_799_999_999_000])
        let reopened = FieldDiagnosticsStore(directory: directory, clock: clock)
        #expect(await reopened.timeline().events == written.events)
        let initialExport = try json(await reopened.debugInfo())
        let initialEvents = try #require((initialExport["timeline"] as? [String: Any])?["events"] as? [[String: Any]])
        #expect(initialEvents.compactMap { $0["observedAtUnixMilliseconds"] as? Int64 } == [1_800_000_000_000, 1_799_999_999_000])
        #expect(initialEvents.compactMap { ($0["observation"] as? [String: Any])?["count"] as? Int } == [0, 1])
        // The clock keeps moving backward while admissions cross capacity.
        for index in 2..<576 {
            time.withLock { $0 = Self.instant.addingTimeInterval(-Double(index)) }
            reopened.enqueue(.init(component: .review, operation: .save, stage: .persistence, outcome: .succeeded, count: UInt64(index)))
            if index % 32 == 0 { _ = await reopened.timeline() }
        }
        let capped = await reopened.timeline()
        #expect(capped.events.count == 512 && capped.truncatedCount == 64 && capped.droppedCount == 0)
        #expect(capped.events.compactMap(\.observation.count) == (64..<576).map { UInt64($0) })
        #expect(capped.events.map(\.observedAtUnixMilliseconds) == (64..<576).map { 1_800_000_000_000 - Int64($0) * 1_000 })
        let finalStore = FieldDiagnosticsStore(directory: directory, clock: clock)
        #expect(await finalStore.timeline().events == capped.events)
        let finalExport = try json(await finalStore.debugInfo())
        let finalEvents = try #require((finalExport["timeline"] as? [String: Any])?["events"] as? [[String: Any]])
        #expect(finalEvents.compactMap { ($0["observation"] as? [String: Any])?["count"] as? Int } == Array(64..<576))
        #expect(finalEvents.compactMap { $0["observedAtUnixMilliseconds"] as? Int64 } == capped.events.map(\.observedAtUnixMilliseconds))
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

    @Test func exportDecodesOwnedSummariesWhenStoredSDKDetailsCannotDecode() async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        try await store.record(FieldDiagnosticsStoreTests.metricReport())
        try await store.record(FieldDiagnosticsStoreTests.hangDiagnostic())
        store.enqueue(Self.observation)
        let secret = "private-SDK-detail-canary"
        let diagnosticFolder = directory.appending(path: "diagnostics")
        let diagnosticFile = try #require(FileManager.default.contentsOfDirectory(at: diagnosticFolder, includingPropertiesForKeys: nil).first)
        var diagnostic = try json(Data(contentsOf: diagnosticFile))
        let result = try #require(diagnostic["result"] as? [String: Any])
        diagnostic["result"] = result.mapValues { _ in secret }
        let diagnosticBytes = try JSONSerialization.data(withJSONObject: diagnostic)
        try diagnosticBytes.write(to: diagnosticFile)
        #expect(throws: DecodingError.self) { try JSONDecoder().decode(FieldDiagnostic.self, from: diagnosticBytes) }

        let metricFolder = directory.appending(path: "metrics")
        let metricFile = try #require(FileManager.default.contentsOfDirectory(at: metricFolder, includingPropertiesForKeys: nil).first)
        var metric = try json(Data(contentsOf: metricFile))
        metric["entries"] = [["values": secret, "states": secret, "duration": secret]]
        let metricBytes = try JSONSerialization.data(withJSONObject: metric)
        try metricBytes.write(to: metricFile)
        #expect(throws: DecodingError.self) { try JSONDecoder().decode(FieldMetrics.self, from: metricBytes) }

        let signpostFolder = directory.appending(path: "signposts/\(IntervalNames.answerToNextVisible.name)")
        let signpostFile = try #require(FileManager.default.contentsOfDirectory(at: signpostFolder, includingPropertiesForKeys: nil).first)
        var signposts = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: signpostFile)) as? [[String: Any]])
        var signpostMetric = try #require(signposts[0]["metric"] as? [String: Any])
        signpostMetric["signpostDuration"] = secret
        signposts[0]["metric"] = signpostMetric
        let signpostBytes = try JSONSerialization.data(withJSONObject: signposts)
        try signpostBytes.write(to: signpostFile)
        #expect(throws: DecodingError.self) { try JSONDecoder().decode([FieldSignpost].self, from: signpostBytes) }

        let bytes = try await store.debugInfo()
        let info = try json(bytes)
        #expect(info["header"] != nil)
        #expect(((info["timeline"] as? [String: Any])?["events"] as? [Any])?.count == 1)
        #expect((info["metrics"] as? [[String: Any]])?.first?["entryCount"] as? Int == 1)
        #expect((info["diagnostics"] as? [[String: Any]])?.first?["kind"] as? String == "hang")
        let exportedSignposts = try #require(info["signposts"] as? [String: [[String: Any]]])
        #expect(exportedSignposts[IntervalNames.answerToNextVisible.name]?.first?["totalCount"] as? Int == 410)
        #expect(!String(decoding: bytes, as: UTF8.self).contains(secret))
        #expect(try Data(contentsOf: diagnosticFile) == diagnosticBytes)
        #expect(try Data(contentsOf: metricFile) == metricBytes)
        #expect(try Data(contentsOf: signpostFile) == signpostBytes)
    }

    @Test(arguments: ["depth", "count", "structure"])
    func archiveCostLimitsPreserveHeaderTimelineAndOtherSections(kind: String) async throws {
        let directory = location()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        try await store.record(FieldDiagnosticsStoreTests.metricReport())
        try await store.record(FieldDiagnosticsStoreTests.hangDiagnostic())
        store.enqueue(Self.observation)
        let folder = directory.appending(path: "metrics")
        let file = try #require(FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil).first)
        var metric = try json(Data(contentsOf: file))
        switch kind {
        case "count": metric["entries"] = Array(repeating: [String: String](), count: SafeFieldDiagnosticsExport.archiveCountLimit + 1)
        case "structure": metric["unusedSDKDetails"] = Array(repeating: 0, count: SafeFieldDiagnosticsExport.archiveStructureLimit + 1)
        default:
            var nested: Any = 0
            for _ in 0...SafeFieldDiagnosticsExport.archiveDepthLimit { nested = [nested] }
            metric["unusedSDKDetails"] = nested
        }
        let oversizedArchive = try JSONSerialization.data(withJSONObject: metric)
        #expect(oversizedArchive.count < SafeFieldDiagnosticsExport.reportByteLimit)
        try oversizedArchive.write(to: file)
        let info = try json(await store.debugInfo())
        #expect(info["header"] != nil)
        #expect(((info["timeline"] as? [String: Any])?["events"] as? [Any])?.count == 1)
        #expect((info["metrics"] as? [Any])?.isEmpty == true)
        #expect((info["diagnostics"] as? [Any])?.count == 1)
        let sections = try #require(info["sections"] as? [String: [String: Any]])
        #expect(sections["metrics"]?["oversizedCount"] as? Int == 1)
        #expect(try Data(contentsOf: file) == oversizedArchive)
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

    @Test func linkedDiagnosticsRootNeverImportsOrChangesItsTarget() async throws {
        let directory = location()
        let outside = location()
        defer { try? FileManager.default.removeItem(at: directory); try? FileManager.default.removeItem(at: outside) }
        let original = FieldDiagnosticsStore(directory: outside, clock: .fixed(Self.instant))
        original.enqueue(.init(component: .startup, operation: .open, stage: .requested, outcome: .observed))
        _ = await original.timeline()
        try await original.record(FieldDiagnosticsStoreTests.metricReport())
        try await original.record(FieldDiagnosticsStoreTests.hangDiagnostic())
        let before = try FieldDiagnosticsStoreTests.files(in: outside)
        try FileManager.default.createSymbolicLink(at: directory, withDestinationURL: outside)
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        let initial = await store.timeline()
        #expect(initial.events.isEmpty && initial.storage == .unavailable)
        #expect(initial.priorHistoryUnavailableReason == .unavailable)
        store.enqueue(Self.observation)
        let retained = await store.timeline()
        #expect(retained.events.map(\.observation) == [Self.observation])
        #expect(retained.storage == .unavailable)
        do {
            try await store.record(FieldDiagnosticsStoreTests.metricReport())
            Issue.record("A linked diagnostics root accepted an archive write")
        } catch { }
        let info = try json(await store.debugInfo())
        #expect(info["header"] != nil)
        #expect((info["metrics"] as? [Any])?.isEmpty == true)
        #expect((info["diagnostics"] as? [Any])?.isEmpty == true)
        let sections = try #require(info["sections"] as? [String: [String: Any]])
        for name in ["metrics", "diagnostics", "signposts"] { #expect(sections[name]?["unavailable"] as? Bool == true) }
        #expect(try FieldDiagnosticsStoreTests.files(in: outside) == before)
    }

    @Test(arguments: [false, true])
    func childDirectorySubstitutionCannotRedirectRetainedReadOrReplacement(linked: Bool) throws {
        let directory = location()
        let outside = location()
        defer { try? FileManager.default.removeItem(at: directory); try? FileManager.default.removeItem(at: outside) }
        let root = try FieldDiagnosticDirectory.root(directory)
        let child = try root.child("metrics", create: true)
        try child.write(Data("original".utf8), name: "report.json", replacing: false)
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        let target = outside.appending(path: "report.json")
        let secret = Data("private-child-target-canary".utf8)
        try secret.write(to: target)
        try FileManager.default.moveItem(at: directory.appending(path: "metrics"), to: directory.appending(path: "retained-metrics"))
        let replacement = directory.appending(path: "metrics")
        if linked {
            try FileManager.default.createSymbolicLink(at: replacement, withDestinationURL: outside)
        } else {
            try FileManager.default.createDirectory(at: replacement, withIntermediateDirectories: false)
            try secret.write(to: replacement.appending(path: "report.json"))
        }
        #expect(throws: DiagnosticReadFailure.self) { try child.read("report.json", limit: 65_536) }
        #expect(throws: DiagnosticReadFailure.self) { try child.write(Data("replacement".utf8), name: "report.json", replacing: true) }
        if linked { #expect(throws: DiagnosticReadFailure.self) { try root.child("metrics") } }
        #expect(try Data(contentsOf: replacement.appending(path: "report.json")) == secret)
        #expect(try Data(contentsOf: target) == secret)
        #expect(try Data(contentsOf: directory.appending(path: "retained-metrics/report.json")) == Data("original".utf8))
    }

    @Test func substitutedRootAndLinkedReportFoldersLeaveMemoryEvidenceAndTargetsIntact() async throws {
        let directory = location()
        let outside = location()
        let retained = location()
        defer {
            try? FileManager.default.removeItem(at: directory)
            try? FileManager.default.removeItem(at: outside)
            try? FileManager.default.removeItem(at: retained)
        }
        let store = FieldDiagnosticsStore(directory: directory, clock: .fixed(Self.instant))
        store.enqueue(Self.observation)
        _ = await store.timeline()
        let targetStore = FieldDiagnosticsStore(directory: outside, clock: .fixed(Self.instant))
        try await targetStore.record(FieldDiagnosticsStoreTests.metricReport())
        let before = try FieldDiagnosticsStoreTests.files(in: outside)
        try FileManager.default.moveItem(at: directory, to: retained)
        try FileManager.default.createSymbolicLink(at: directory, withDestinationURL: outside)
        store.enqueue(Self.observation)
        let timeline = await store.timeline()
        #expect(timeline.events.count == 2 && timeline.storage == .unavailable)
        let info = try json(await store.debugInfo())
        #expect((info["metrics"] as? [Any])?.isEmpty == true)
        #expect(try FieldDiagnosticsStoreTests.files(in: outside) == before)

        // A real root with a linked child is independently unavailable.
        let childStore = FieldDiagnosticsStore(directory: retained, clock: .fixed(Self.instant))
        try FileManager.default.createSymbolicLink(at: retained.appending(path: "metrics"), withDestinationURL: outside.appending(path: "metrics"))
        let childInfo = try json(await childStore.debugInfo())
        #expect((childInfo["metrics"] as? [Any])?.isEmpty == true)
        #expect((childInfo["sections"] as? [String: [String: Any]])?["metrics"]?["unavailable"] as? Bool == true)
        #expect(try FieldDiagnosticsStoreTests.files(in: outside) == before)
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
