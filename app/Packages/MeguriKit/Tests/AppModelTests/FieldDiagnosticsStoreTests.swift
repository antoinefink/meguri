import Foundation
import Instrumentation
import MetricKit
import Synchronization
import Testing

import AppModel

/// Field diagnostics (R1867, U12): MetricKit's reports land in app-private
/// storage, registered signposts under their names, and nothing is sent.
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

        let stored = try store.metrics()
        #expect(stored.count == 1)
        let kinds = stored[0].entries.flatMap(\.values).map(\.metricGroup)
        var expected: Set<MetricGroup> = [.applicationResponsiveness, .animation, .appLaunch, .diskIO]
        #if os(iOS)
        expected.insert(.memory)  // MetricKit reports no memory on the Mac
        #endif
        #expect(Set(kinds) == expected)
        #expect(stored[0].timeRange == DateInterval(
            start: Date(timeIntervalSinceReferenceDate: 813_628_800), end: Date(timeIntervalSinceReferenceDate: 813_715_200)))

        let diagnostics = try store.diagnostics()
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
        #expect(try reopened.metrics().count == 1)
        #expect(try reopened.diagnostics().count == 1)
    }

    @Test func aRegisteredSignpostIsStoredUnderItsName() async throws {
        let store = try temporaryStore()
        try await deliver([Self.metricReport(), Self.metricReport()], [], to: store)

        let answers = try store.signposts(named: IntervalNames.answerToNextVisible.name)
        #expect(answers.count == 2, "one entry per delivered report")
        #expect(answers.allSatisfy { $0.metric.totalCount == 410 && $0.metric.signpostCategory == "Review" })
        #expect(answers[0].metric.signpostDuration.buckets.map(\.count) == [400, 10])
        #expect(try store.signposts(named: "someone.elses").isEmpty, "only registered names are kept")
        #expect(try store.signposts(named: IntervalNames.editorOpenToEditable.name).isEmpty)
        // Signposts are kept apart from the report's other values.
        #expect(try store.metrics().allSatisfy { $0.entries.flatMap(\.values).allSatisfy { $0.metricGroup != .signpost } })
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
        _ = try store.debugInfo()
        #expect(try store.metrics().count == 1)
        #expect(RequestRecorder.requests.withLock { $0 }.isEmpty, "field diagnostics never leave the device")
    }

    @Test func debugInfoHoldsEverythingStored() async throws {
        let store = try temporaryStore()
        #expect(try JSONSerialization.jsonObject(with: store.debugInfo()) as? [String: Any] != nil)
        try await deliver([Self.metricReport()], [Self.hangDiagnostic()], to: store)
        let info = try #require(JSONSerialization.jsonObject(with: store.debugInfo()) as? [String: Any])
        #expect((info["metrics"] as? [Any])?.count == 1)
        #expect((info["diagnostics"] as? [Any])?.count == 1)
        let signposts = try #require(info["signposts"] as? [String: Any])
        #expect(Array(signposts.keys) == [IntervalNames.answerToNextVisible.name])
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
