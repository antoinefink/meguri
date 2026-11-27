import CoreBridge
import Foundation
import Instrumentation
import MetricKit
import Synchronization
import Testing

import AppModel

/// Field diagnostics (R1867, U12): MetricKit's reports land in app-private
/// storage, registered signposts under their names, and nothing is sent.
/// The files are read and written off the main actor (KTD9).
@Suite(.serialized) struct FieldDiagnosticsStoreTests {
    /// A daily report as MetricKit's Swift API encodes it: hang, hitch,
    /// launch, memory, disk writes, two signposts (one registered), and a
    /// value the store does not keep (Wi-Fi upload).
    static let metricReportJSON = """
        {"timeRange": {"begin": 813628800, "end": 813715200},
         "stateEntries": [],
         "intervalEntries": [{
           "states": [],
           "duration": {"value": 86400, "unit": "s"},
           "values": [
             {"hangTimeMetric": {"histogram": {"buckets": [
               {"lowerBound": {"value": 0, "unit": "ms"}, "upperBound": {"value": 250, "unit": "ms"}, "count": 2}]}}},
             {"hitchTimeMetric": {"ratio": {"value": 2.5, "unit": "ms per s"},
               "totalHitchTime": {"value": 30, "unit": "ms"}, "totalAnimationTime": {"value": 12, "unit": "s"}}},
             {"timeToFirstDrawMetric": {"histogram": {"buckets": [
               {"lowerBound": {"value": 300, "unit": "ms"}, "upperBound": {"value": 400, "unit": "ms"}, "count": 5}]}}},
             {"peakMemoryMetric": {"value": {"value": 120, "unit": "MB"}}},
             {"logicalDiskWritesMetric": {"value": {"value": 12, "unit": "MB"}}},
             {"totalWiFiUploadMetric": {"value": {"value": 3, "unit": "MB"}}},
             {"signpostIntervalMetric": {"signpostName": "review.answerToNextVisible", "signpostCategory": "Review",
               "totalCount": 410, "signpostDuration": {"buckets": [
                 {"lowerBound": {"value": 0, "unit": "ms"}, "upperBound": {"value": 10, "unit": "ms"}, "count": 400},
                 {"lowerBound": {"value": 10, "unit": "ms"}, "upperBound": {"value": 20, "unit": "ms"}, "count": 10}]}}},
             {"signpostIntervalMetric": {"signpostName": "someone.elses", "signpostCategory": "Other",
               "totalCount": 1, "signpostDuration": {"buckets": []}}}
           ]}]}
        """

    static let hangDiagnosticJSON = """
        {"timeRange": {"begin": 813628800, "end": 813715200},
         "environment": {"regionFormat": "US", "osVersion": {"platform": "iOS", "number": "27.0", "buildNumber": "24A434"},
           "deviceType": "iPhone18,1", "platformArchitecture": "arm64e", "lowPowerModeEnabled": false,
           "isTestFlightApp": false, "applicationVersion": "1.0", "applicationBuildVersion": "1", "pid": 42,
           "bundleIdentifier": "com.antoinefink.meguri", "signpostData": [], "states": []},
         "hangDiagnostic": {"hangDuration": {"value": 1.5, "unit": "s"},
           "callStackTree": {"callStackThreads": [], "callStackPerThread": true, "binaryInfo": []}}}
        """

    static func metricReport() throws -> MetricReport {
        try JSONDecoder().decode(MetricReport.self, from: Data(metricReportJSON.utf8))
    }

    static func hangDiagnostic() throws -> DiagnosticReport {
        try JSONDecoder().decode(DiagnosticReport.self, from: Data(hangDiagnosticJSON.utf8))
    }

    func temporaryStore() throws -> FieldDiagnosticsStore {
        let directory = FileManager.default.temporaryDirectory
            .appending(path: "FieldDiagnostics-\(UUID().uuidString)", directoryHint: .isDirectory)
        return FieldDiagnosticsStore(directory: directory)
    }

    /// Delivers the reports through a subscriber, as MetricKit would, and
    /// waits until the store holds them.
    func deliver(_ metrics: [MetricReport], _ diagnostics: [DiagnosticReport], to store: FieldDiagnosticsStore) async throws {
        let (metricStream, metricFeed) = AsyncStream.makeStream(of: MetricReport.self)
        let (diagnosticStream, diagnosticFeed) = AsyncStream.makeStream(of: DiagnosticReport.self)
        let subscriber = FieldDiagnosticsSubscriber(store: store, metricReports: metricStream, diagnosticReports: diagnosticStream)
        subscriber.start()
        metrics.forEach { metricFeed.yield($0) }
        diagnostics.forEach { diagnosticFeed.yield($0) }
        metricFeed.finish()
        diagnosticFeed.finish()
        await subscriber.finished()
    }

    @Test func aDeliveredPayloadIsStoredInAppPrivateStorage() async throws {
        let store = try temporaryStore()
        try await deliver([Self.metricReport()], [Self.hangDiagnostic()], to: store)

        let stored = try await store.metrics()
        #expect(stored.count == 1)
        let kinds = stored[0].entries.flatMap(\.values).map(\.metricGroup)
        var expected: Set<MetricGroup> = [.applicationResponsiveness, .animation, .appLaunch, .diskIO]
        #if os(iOS)
        expected.insert(.memory)  // MetricKit reports no memory on the Mac
        #endif
        #expect(Set(kinds) == expected)
        #expect(stored[0].timeRange == DateInterval(
            start: Date(timeIntervalSinceReferenceDate: 813_628_800), end: Date(timeIntervalSinceReferenceDate: 813_715_200)))

        let diagnostics = try await store.diagnostics()
        #expect(diagnostics.count == 1)
        guard case .hang(let hang) = diagnostics[0].result else {
            Issue.record("expected the hang diagnostic, got \(diagnostics[0].result)")
            return
        }
        #expect(hang.hangDuration == Measurement(value: 1.5, unit: UnitDuration.seconds))

        // On disk, inside the store's directory, and readable by a new store.
        let files = try FileManager.default.subpathsOfDirectory(atPath: store.directory.path(percentEncoded: false))
        #expect(files.contains { $0.hasPrefix("metrics/") && $0.hasSuffix(".json") })
        #expect(files.contains { $0.hasPrefix("diagnostics/") && $0.hasSuffix(".json") })
        let reopened = FieldDiagnosticsStore(directory: store.directory)
        #expect(try await reopened.metrics().count == 1)
        #expect(try await reopened.diagnostics().count == 1)
    }

    @Test func aRegisteredSignpostIsStoredUnderItsName() async throws {
        let store = try temporaryStore()
        try await deliver([Self.metricReport(), Self.metricReport()], [], to: store)

        let answers = try await store.signposts(named: IntervalNames.answerToNextVisible.name)
        #expect(answers.count == 2, "one entry per delivered report")
        #expect(answers.allSatisfy { $0.metric.totalCount == 410 && $0.metric.signpostCategory == "Review" })
        #expect(answers[0].metric.signpostDuration.buckets.map(\.count) == [400, 10])
        #expect(try await store.signposts(named: "someone.elses").isEmpty, "only registered names are kept")
        #expect(try await store.signposts(named: IntervalNames.editorOpenToEditable.name).isEmpty)
        // Signposts are kept apart from the report's other values.
        #expect(try await store.metrics().allSatisfy { $0.entries.flatMap(\.values).allSatisfy { $0.metricGroup != .signpost } })
    }

    @Test func aNewReportAddsToTheSignpostHistoryWithoutRewritingIt() async throws {
        let store = try temporaryStore()
        try await deliver([Self.metricReport()], [], to: store)
        let before = try Self.files(in: store.directory)
        #expect(before.keys.contains { $0.hasPrefix("signposts/") }, "\(before.keys)")

        try await deliver([Self.metricReport()], [], to: store)
        let after = try Self.files(in: store.directory)
        #expect(after.keys.allSatisfy { $0.hasSuffix(".json") }, "completed writes leave no staging files")
        for (path, bytes) in before {
            #expect(after[path] == bytes, "\(path) was rewritten")
        }
        let added = after.keys.filter { before[$0] == nil }
        #expect(added.filter { $0.hasPrefix("signposts/") }.count == 1, "\(added)")
        #expect(try await store.signposts(named: IntervalNames.answerToNextVisible.name).count == 2)
    }

    @Test(arguments: ["metrics", "diagnostics", "signposts/review.answerToNextVisible"], [".json", ".tmp"])
    func partialFilesDoNotBlockExport(folder: String, suffix: String) async throws {
        let store = try temporaryStore()
        defer { try? FileManager.default.removeItem(at: store.directory) }
        try await store.record(Self.metricReport())
        try await store.record(Self.hangDiagnostic())
        let partial = store.directory.appending(path: "\(folder)/interrupted\(suffix)")
        let bytes = Data("{\"timeRange\":".utf8)
        try bytes.write(to: partial)
        let before = try Self.files(in: store.directory)

        let reopened = FieldDiagnosticsStore(directory: store.directory)
        try await reopened.record(Self.metricReport())
        try await reopened.record(Self.hangDiagnostic())
        let exported = try await reopened.debugInfo()
        let info = try #require(JSONSerialization.jsonObject(with: exported) as? [String: Any])
        #expect((info["metrics"] as? [Any])?.count == 2)
        #expect((info["diagnostics"] as? [Any])?.count == 2)
        let signposts = try #require(info["signposts"] as? [String: [Any]])
        #expect(signposts[IntervalNames.answerToNextVisible.name]?.count == 2)
        let after = try Self.files(in: store.directory)
        for (path, bytes) in before {
            #expect(after[path] == bytes, "\(path) was rewritten or removed")
        }
    }

    /// The main thread stays blocked while the store records and reads: a
    /// store that needed the main actor could not finish (KTD9: no file work
    /// on the UI thread).
    @Test @MainActor func theStoreWorksOffTheMainActor() throws {
        let store = try temporaryStore()
        let report = try Self.metricReport()
        let diagnostic = try Self.hangDiagnostic()
        let done = DispatchSemaphore(value: 0)
        let counts = Mutex<[Int]>([])
        Task.detached {
            try? await store.record(report)
            try? await store.record(diagnostic)
            let metrics = (try? await store.metrics().count) ?? -1
            let diagnostics = (try? await store.diagnostics().count) ?? -1
            counts.withLock { $0 = [metrics, diagnostics] }
            done.signal()
        }
        #expect(done.wait(timeout: .now() + 20) == .success, "the store did not finish while the main thread was busy")
        #expect(counts.withLock { $0 } == [1, 1])
    }

    /// Every file under `directory`, by relative path, with its bytes.
    static func files(in directory: URL) throws -> [String: Data] {
        let root = directory.path(percentEncoded: false)
        var files: [String: Data] = [:]
        for path in try FileManager.default.subpathsOfDirectory(atPath: root) {
            let url = directory.appending(path: path, directoryHint: .notDirectory)
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path(percentEncoded: false), isDirectory: &isDirectory),
                !isDirectory.boolValue
            else { continue }
            files[path] = try Data(contentsOf: url)
        }
        return files
    }

    @Test func storingMakesNoNetworkRequest() async throws {
        RequestRecorder.install()
        defer { RequestRecorder.uninstall() }
        // The recorder sees requests made through the URL loading system.
        let probe = URL(string: "https://field-diagnostics.invalid/probe")!
        _ = try? await URLSession.shared.data(from: probe)
        #expect(RequestRecorder.requests.withLock { $0 } == [probe])
        RequestRecorder.requests.withLock { $0 = [] }

        let store = try temporaryStore()
        try await deliver([Self.metricReport()], [Self.hangDiagnostic()], to: store)
        _ = try await store.debugInfo()
        #expect(try await store.metrics().count == 1)
        #expect(RequestRecorder.requests.withLock { $0 }.isEmpty, "field diagnostics never leave the device")
    }



    /// Serialization unit coverage only. Real Runtime/adapter A7 input is
    /// exercised by serviceAppNoStateProofReturnsThroughCoordinatorAndCopyDebugInfo.



    @Test func debugInfoProjectsStoredReportsWithoutRawSDKDetails() async throws {
        let store = try temporaryStore()
        let empty = try await store.debugInfo()
        #expect(try JSONSerialization.jsonObject(with: empty) as? [String: Any] != nil)
        let emptyInfo = try #require(JSONSerialization.jsonObject(with: empty) as? [String: Any])
        #expect(emptyInfo["zoneDeletions"] == nil)
        try await deliver([Self.metricReport()], [Self.hangDiagnostic()], to: store)
        let stored = try await store.debugInfo()
        let info = try #require(JSONSerialization.jsonObject(with: stored) as? [String: Any])
        #expect((info["metrics"] as? [Any])?.count == 1)
        #expect((info["diagnostics"] as? [Any])?.count == 1)
        let signposts = try #require(info["signposts"] as? [String: Any])
        #expect(Array(signposts.keys) == [IntervalNames.answerToNextVisible.name])
        let diagnostic = try #require((info["diagnostics"] as? [[String: Any]])?.first)
        #expect(diagnostic["kind"] as? String == "hang")
        #expect(diagnostic["detailsUnavailable"] as? Bool == true)
        #expect(Set(diagnostic.keys) == ["receivedAtUnixMilliseconds", "reportTime", "kind", "detailsUnavailable"])
        let metric = try #require((info["metrics"] as? [[String: Any]])?.first)
        #expect(metric["detailsUnavailable"] as? Bool == true)
        #expect(metric["entries"] == nil && metric["applicationVersion"] == nil)
        let header = try #require(info["header"] as? [String: Any])
        #expect(header["schemaVersion"] as? Int == 1)
        #expect((header["generatedAtUTC"] as? String)?.hasSuffix("Z") == true)
    }
}

/// Records every request the URL loading system sees and fails it, so no
/// test request reaches the network.
nonisolated final class RequestRecorder: URLProtocol, @unchecked Sendable {
    static let requests = Mutex<[URL]>([])

    static func install() {
        requests.withLock { $0 = [] }
        URLProtocol.registerClass(RequestRecorder.self)
    }

    static func uninstall() {
        URLProtocol.unregisterClass(RequestRecorder.self)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        if let url = request.url {
            Self.requests.withLock { $0.append(url) }
        }
        client?.urlProtocol(self, didFailWithError: URLError(.notConnectedToInternet))
    }

    override func stopLoading() {}
}
