import Foundation
import Instrumentation
import MetricKit
import os

/// On-device field diagnostics (R1867): what MetricKit reports about hangs,
/// hitches, launch, memory and disk writes, plus the daily statistics of
/// the registered intervals (R1861, emitted to MetricKit by `Signposts`).
/// Everything stays in a directory of the app's private container; nothing
/// here talks to the network. Settings shows it through Copy Debug Info
/// (U160, `debugInfo()`).
///
/// The store is an actor on its own utility-QoS serial queue: every read
/// and write runs there, never on the main actor (KTD9), and callers await
/// it. Files are only ever added, never rewritten, so a report costs one
/// write however much history is stored. A report is written under a
/// staging name and then linked in under its final one, so a reader never
/// sees it partly written; a report that does not decode (left partly
/// written by an older build) is skipped, not removed.
///
/// Layout, one JSON file per report:
/// - `metrics/<received>-<uuid>.json`: a `FieldMetrics`, the report's
///   entries with only the kept values;
/// - `diagnostics/<received>-<uuid>.json`: a `FieldDiagnostic` (hang, disk
///   write exception, launch, and on iOS memory exception);
/// - `signposts/<interval name>/<received>-<uuid>.json`: the
///   `FieldSignpost`s a report had for that registered interval. Signposts
///   under other names are dropped.
public actor FieldDiagnosticsStore {
    public nonisolated let directory: URL
    private nonisolated let queue = DispatchSerialQueue(label: "com.antoinefink.meguri.fieldDiagnostics", qos: .utility)

    public nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }

    public init(directory: URL) {
        self.directory = directory
    }

    /// `<Application Support>/Diagnostics/` in the app's container.
    public nonisolated static var standard: FieldDiagnosticsStore {
        FieldDiagnosticsStore(directory: URL.applicationSupportDirectory.appending(path: "Diagnostics", directoryHint: .isDirectory))
    }

    // MARK: Writing

    /// Stores a daily metric report: its hang, hitch, launch, memory and
    /// disk-write values, and its registered signposts by name.
    public func record(_ report: MetricReport) throws {
        let received = Date()
        var signposts: [String: [FieldSignpost]] = [:]
        var entries: [FieldMetrics.Entry] = []
        let reported = report.intervalEntries.map { (states: $0.states, duration: Optional($0.duration), values: $0.values) }
            + report.stateEntries.map { (states: [$0.state], duration: nil, values: $0.values) }
        for entry in reported {
            var kept: [MetricResult] = []
            for value in entry.values {
                if case .signpostInterval(let metric) = value {
                    if let interval = IntervalNames.named(metric.signpostName),
                        interval.category.rawValue == metric.signpostCategory
                    {
                        signposts[interval.name, default: []].append(FieldSignpost(timeRange: report.timeRange, metric: metric))
                    }
                } else if Self.keptGroups.contains(value.metricGroup) {
                    kept.append(value)
                }
            }
            entries.append(FieldMetrics.Entry(states: entry.states, duration: entry.duration, values: kept))
        }
        let metrics = FieldMetrics(
            received: received,
            timeRange: report.timeRange,
            applicationVersion: report.environment?.latestApplicationVersion,
            entries: entries
        )
        try write(metrics, to: newFile(in: Folder.metrics, received: received))
        for (name, entries) in signposts {
            try write(entries, to: newFile(in: Folder.signposts(name), received: received))
        }
    }

    /// Stores a diagnostic report of a kind the store keeps; drops others.
    public func record(_ report: DiagnosticReport) throws {
        guard Self.keeps(report.result) else { return }
        let received = Date()
        let diagnostic = FieldDiagnostic(
            received: received, timeRange: report.timeRange,
            applicationVersion: report.environment.applicationVersion, result: report.result)
        try write(diagnostic, to: newFile(in: Folder.diagnostics, received: received))
    }

    // MARK: Reading

    /// Every stored metric report, oldest first.
    public func metrics() throws -> [FieldMetrics] {
        try readAll(FieldMetrics.self, in: Folder.metrics)
    }

    /// Every stored diagnostic, oldest first.
    public func diagnostics() throws -> [FieldDiagnostic] {
        try readAll(FieldDiagnostic.self, in: Folder.diagnostics)
    }

    /// The stored daily statistics of the registered interval `name`,
    /// oldest report first.
    public func signposts(named name: String) throws -> [FieldSignpost] {
        guard IntervalNames.named(name) != nil else { return [] }
        return try readAll([FieldSignpost].self, in: Folder.signposts(name)).flatMap { $0 }
    }

    /// Everything stored, as one JSON object (`metrics`, `diagnostics`,
    /// `signposts` by name) for Copy Debug Info.
    public func debugInfo() throws -> Data {
        var signposts: [String: [FieldSignpost]] = [:]
        for interval in IntervalNames.all {
            let stored = try self.signposts(named: interval.name)
            if !stored.isEmpty { signposts[interval.name] = stored }
        }
        return try Self.encoder.encode(DebugInfo(metrics: try metrics(), diagnostics: try diagnostics(), signposts: signposts))
    }

    // MARK: What is kept

    /// Hang, hitch, launch, memory and disk writes (R1867). MetricKit
    /// reports no memory metrics on the Mac.
    nonisolated static let keptGroups: Set<MetricGroup> = {
        var groups: Set<MetricGroup> = [.applicationResponsiveness, .animation, .appLaunch, .diskIO]
        #if os(iOS)
        groups.insert(.memory)
        #endif
        return groups
    }()

    nonisolated static func keeps(_ result: DiagnosticResult) -> Bool {
        switch result {
        case .hang, .diskWriteException, .appLaunch: return true
        default:
            #if os(iOS)
            if case .memoryException = result { return true }
            #endif
            return false
        }
    }

    // MARK: Files

    private enum Folder {
        static let metrics = "metrics"
        static let diagnostics = "diagnostics"
        static func signposts(_ name: String) -> String { "signposts/\(name)" }
    }

    private nonisolated static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }()

    private func newFile(in folder: String, received: Date) -> URL {
        let stamp = String(format: "%013.0f", received.timeIntervalSince1970 * 1000)
        return directory.appending(path: "\(folder)/\(stamp)-\(UUID().uuidString).json", directoryHint: .notDirectory)
    }

    /// Writes a new file; an existing file is never replaced. The bytes go
    /// to a staging file (`.tmp`, which readers ignore) first; the hard link
    /// to the final name appears at once, complete, and fails when that name
    /// exists.
    private func write<T: Encodable>(_ value: T, to file: URL) throws {
        let folder = file.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let staging = folder.appending(path: "\(UUID().uuidString).tmp", directoryHint: .notDirectory)
        defer { try? FileManager.default.removeItem(at: staging) }
        try Self.encoder.encode(value).write(to: staging, options: [.withoutOverwriting])
        try FileManager.default.linkItem(at: staging, to: file)
    }

    private func read<T: Decodable>(_ type: T.Type, from file: URL) throws -> T {
        try JSONDecoder().decode(type, from: Data(contentsOf: file))
    }

    private func readAll<T: Decodable>(_ type: T.Type, in folder: String) throws -> [T] {
        let dir = directory.appending(path: folder, directoryHint: .isDirectory)
        guard FileManager.default.fileExists(atPath: dir.path(percentEncoded: false)) else { return [] }
        return try FileManager.default.contentsOfDirectory(atPath: dir.path(percentEncoded: false))
            .filter { $0.hasSuffix(".json") }
            .sorted()
            .compactMap { name -> T? in
                do {
                    return try read(type, from: dir.appending(path: name, directoryHint: .notDirectory))
                } catch is DecodingError {
                    // A partly written report: kept on disk, left out of the export.
                    return nil
                }
            }
    }

    private struct DebugInfo: Encodable {
        let metrics: [FieldMetrics]
        let diagnostics: [FieldDiagnostic]
        let signposts: [String: [FieldSignpost]]
    }
}

/// A stored metric report.
nonisolated public struct FieldMetrics: Codable, Sendable {
    /// One of the report's entries: an interval entry (states and duration)
    /// or a state entry (one state, no duration).
    public struct Entry: Codable, Sendable {
        public let states: [MetricManager.ReportedState]
        public let duration: Measurement<UnitDuration>?
        public let values: [MetricResult]
    }

    public let received: Date
    public let timeRange: DateInterval
    public let applicationVersion: String?
    public let entries: [Entry]
}

/// A stored diagnostic.
nonisolated public struct FieldDiagnostic: Codable, Sendable {
    public let received: Date
    public let timeRange: DateInterval
    public let applicationVersion: String
    public let result: DiagnosticResult
}

/// One report's statistics of a registered interval.
nonisolated public struct FieldSignpost: Codable, Sendable {
    public let timeRange: DateInterval
    public let metric: SignpostIntervalMetric
}

/// Hands MetricKit's reports to a `FieldDiagnosticsStore` for as long as it
/// runs. The app starts one at launch (`FieldDiagnosticsSubscriber.start`).
public final class FieldDiagnosticsSubscriber {
    private let metricLoop: () async -> Void
    private let diagnosticLoop: () async -> Void
    private let keepAlive: AnyObject?
    private var tasks: [Task<Void, Never>] = []

    private static let log = Logger(subsystem: IntervalNames.subsystem, category: "FieldDiagnostics")

    /// Subscribes `store` to MetricKit.
    public convenience init(store: FieldDiagnosticsStore) {
        let manager = MetricManager()
        self.init(store: store, metricReports: manager.metricReports, diagnosticReports: manager.diagnosticReports, keepAlive: manager)
    }

    /// Subscribes `store` to the given report sequences (MetricKit's, or a
    /// test's).
    public convenience init<Metrics: AsyncSequence, Diagnostics: AsyncSequence>(
        store: FieldDiagnosticsStore, metricReports: Metrics, diagnosticReports: Diagnostics
    ) where Metrics.Element == MetricReport, Metrics.Failure == Never, Diagnostics.Element == DiagnosticReport, Diagnostics.Failure == Never {
        self.init(store: store, metricReports: metricReports, diagnosticReports: diagnosticReports, keepAlive: nil)
    }

    private init<Metrics: AsyncSequence, Diagnostics: AsyncSequence>(
        store: FieldDiagnosticsStore, metricReports: Metrics, diagnosticReports: Diagnostics, keepAlive: AnyObject?
    ) where Metrics.Element == MetricReport, Metrics.Failure == Never, Diagnostics.Element == DiagnosticReport, Diagnostics.Failure == Never {
        self.keepAlive = keepAlive
        metricLoop = {
            for await report in metricReports {
                do {
                    try await store.record(report)
                } catch {
                    Self.log.error("storing a metric report failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        }
        diagnosticLoop = {
            for await report in diagnosticReports {
                do {
                    try await store.record(report)
                } catch {
                    Self.log.error("storing a diagnostic report failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        }
    }

    /// Starts receiving; a second call does nothing.
    public func start() {
        guard tasks.isEmpty else { return }
        tasks = [Task { await metricLoop() }, Task { await diagnosticLoop() }]
    }

    /// Stops receiving.
    public func stop() {
        tasks.forEach { $0.cancel() }
    }

    /// Returns when both sequences have ended (a test's do; MetricKit's
    /// never end).
    public func finished() async {
        for task in tasks {
            await task.value
        }
    }

    /// The app's subscriber: the standard store, subscribed to MetricKit.
    public static func start() -> FieldDiagnosticsSubscriber {
        let subscriber = FieldDiagnosticsSubscriber(store: .standard)
        subscriber.start()
        return subscriber
    }
}
