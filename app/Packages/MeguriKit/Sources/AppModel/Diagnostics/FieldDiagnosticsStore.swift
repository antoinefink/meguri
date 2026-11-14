import CoreBridge
import Foundation
import Instrumentation
import MetricKit
import os

/// On-device field diagnostics (R1867): what MetricKit reports about hangs,
/// hitches, launch, memory and disk writes, plus the daily statistics of
/// the registered intervals (R1861, emitted to MetricKit by `Signposts`).
/// Everything stays in a directory of the app's private container; nothing
/// here talks to the network. `debugInfo()` produces a safe export for a
/// user-initiated Copy Debug Info action.
///
/// The store is an actor on its own utility-QoS serial queue: every read
/// and write runs there, never on the main actor (KTD9), and callers await
/// it. MetricKit files are only ever added, never rewritten, so a report costs one
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

    public nonisolated static let timelineEventLimit = 512
    public nonisolated static let timelineAgeSeconds: TimeInterval = 7 * 24 * 60 * 60
    public nonisolated static let byteLimit = 1_048_576
    private nonisolated let clock: AppClock
    private nonisolated let inbox = FieldDiagnosticInbox()
    private var timelineLoaded = false
    private var timelineNeedsRewrite = false
    private var timelineState = FieldDiagnosticTimeline.empty
    private var processDroppedCount: UInt64 = 0
    private var directoryAuthority: FieldDiagnosticDirectory?
    // Successful admissions only, including every intermediate signpost folder.
    // A retained authority is never replaced after pathname substitution.
    private var archiveAuthorities: [String: FieldDiagnosticDirectory] = [:]
    private let cloudDiagnostics: @Sendable () async -> CloudDiagnosticsExport

    public init(directory: URL, clock: AppClock = .system,
                cloudDiagnostics: @escaping @Sendable () async -> CloudDiagnosticsExport = { .unavailable }) {
        self.directory = directory
        self.clock = clock
        self.cloudDiagnostics = cloudDiagnostics
    }

    /// One process-wide store. The export accessor neither creates a workflow
    /// owner nor needs an open collection; no collection is explicit absence.
    public nonisolated static let standard: FieldDiagnosticsStore = {
        let workflow = CoreCloudWorkflowService()
        return FieldDiagnosticsStore(directory: URL.applicationSupportDirectory.appending(path: "Diagnostics", directoryHint: .isDirectory),
                                     cloudDiagnostics: { await workflow.diagnosticsExport() })
    }()

    // MARK: Writing

    /// Stores a daily metric report: its hang, hitch, launch, memory and
    /// disk-write values, and its registered signposts by name.
    public func record(_ report: MetricReport) throws {
        let received = clock.now()
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
        let received = clock.now()
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

    /// One bounded read-only cloud projection, plus the store's pinned archive
    /// authorities. Optional section failures leave header/timeline copyable.
    public func debugInfo() async throws -> Data {
        let cloud = await cloudDiagnostics()
        let snapshot = timeline()
        var archives: [String: Result<FieldDiagnosticDirectory, DiagnosticReadFailure>] = [:]
        for folder in [Folder.metrics, Folder.diagnostics] + IntervalNames.all.map({ Folder.signposts($0.name) }) {
            do { archives[folder] = .success(try archiveDirectory(folder)) }
            catch DiagnosticReadFailure.notFound { archives[folder] = .failure(.notFound) }
            catch { archives[folder] = .failure(.unavailable) }
        }
        return try SafeFieldDiagnosticsExport.make(archives: archives, now: clock.now(), timeline: snapshot, cloud: cloud)
    }

    /// Nonblocking with respect to storage: no actor hop, file work or await.
    /// A burst has at most 64 queued values and one consumer task. A full
    /// inbox drops the new observation and increments a safe loss counter.
    public nonisolated func enqueue(_ observation: FieldDiagnosticObservation) {
        let event = diagnosticMilliseconds(clock.now()).map {
            FieldDiagnosticEvent(observedAtUnixMilliseconds: $0, observation: observation)
        }
        if inbox.enqueue(event) {
            Task { await self.drainTimeline() }
        }
    }

    /// Also flushes observations already admitted to the inbox. Never throws;
    /// an unavailable disk leaves the bounded in-memory evidence accessible.
    public func timeline() -> FieldDiagnosticTimeline {
        ingest(inbox.take(finishingIfEmpty: false))
        return timelineState
    }

    private func drainTimeline() async {
        while true {
            let batch = inbox.take(finishingIfEmpty: true)
            ingest(batch)
            if batch.finished { return }
            await Task.yield()
        }
    }

    private func ingest(_ batch: FieldDiagnosticInbox.Batch) {
        loadTimeline()
        do { _ = try storageDirectory() }
        catch { timelineState.storage = .unavailable }
        let prior = timelineState
        let needsRepair = timelineNeedsRewrite
        let newDrops = batch.dropped >= processDroppedCount ? batch.dropped - processDroppedCount : 0
        timelineState.droppedCount = saturatedAdd(timelineState.droppedCount, newDrops)
        processDroppedCount = batch.dropped
        timelineState.events.append(contentsOf: batch.events)
        // The persisted array and each inbox batch are already in admission
        // order. Wall-clock rollback changes metadata, never observation order.
        if let now = diagnosticMilliseconds(clock.now()) {
            let cutoff = now - Int64(Self.timelineAgeSeconds * 1_000)
            let expired = timelineState.events.count(where: { $0.observedAtUnixMilliseconds < cutoff })
            timelineState.events.removeAll { $0.observedAtUnixMilliseconds < cutoff }
            timelineState.expiredCount = saturatedAdd(timelineState.expiredCount, UInt64(expired))
        }
        let excess = max(0, timelineState.events.count - Self.timelineEventLimit)
        if excess > 0 {
            timelineState.events.removeFirst(excess)
            timelineState.truncatedCount = saturatedAdd(timelineState.truncatedCount, UInt64(excess))
        }
        guard needsRepair || !batch.events.isEmpty || newDrops > 0 || prior.events != timelineState.events else { return }
        // Clear only after a successful write. A later ordinary snapshot may
        // persist this same evidence after initial storage becomes available.
        timelineNeedsRewrite = true
        persistTimeline()
    }

    private func loadTimeline() {
        guard !timelineLoaded else { return }
        timelineLoaded = true
        do {
            let bytes = try storageDirectory().read("timeline.json", limit: Self.byteLimit)
            var value = try JSONDecoder().decode(FieldDiagnosticTimeline.self, from: bytes)
            guard value.schemaVersion == 1 else {
                timelineNeedsRewrite = true
                timelineState.storage = .unsupportedSchema; timelineState.priorHistoryUnavailable = true
                timelineState.priorHistoryUnavailableReason = .unsupportedSchema; return
            }
            let valid = value.events.filter { (-62_135_596_800_000...253_402_300_799_999).contains($0.observedAtUnixMilliseconds) }
            timelineNeedsRewrite = valid.count != value.events.count
            value.invalidCount = saturatedAdd(value.invalidCount, UInt64(value.events.count - valid.count))
            value.events = valid
            timelineState = value
        } catch DiagnosticReadFailure.notFound {
            return
        } catch DiagnosticReadFailure.oversized {
            timelineNeedsRewrite = true
            timelineState.storage = .oversized; timelineState.priorHistoryUnavailable = true
            timelineState.priorHistoryUnavailableReason = .oversized
        } catch is DecodingError {
            timelineNeedsRewrite = true
            timelineState.storage = .corrupt; timelineState.priorHistoryUnavailable = true
            timelineState.priorHistoryUnavailableReason = .corrupt
        } catch {
            timelineNeedsRewrite = true
            timelineState.storage = .unavailable; timelineState.priorHistoryUnavailable = true
            timelineState.priorHistoryUnavailableReason = .unavailable
        }
    }

    private func persistTimeline() {
        do {
            var saved = timelineState
            saved.storage = .available
            var bytes = try Self.encoder.encode(saved)
            while bytes.count > Self.byteLimit, !saved.events.isEmpty {
                saved.events.removeFirst()
                saved.truncatedCount = saturatedAdd(saved.truncatedCount, 1)
                bytes = try Self.encoder.encode(saved)
            }
            guard bytes.count <= Self.byteLimit else { timelineState.storage = .unavailable; return }
            try storageDirectory().write(bytes, name: "timeline.json", replacing: true)
            timelineState = saved
            timelineNeedsRewrite = false
        } catch {
            // No logging through this store, error description, or retries.
            timelineState.storage = .unavailable
        }
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

    private func storageDirectory() throws -> FieldDiagnosticDirectory {
        if let directoryAuthority {
            try directoryAuthority.validate()
            return directoryAuthority
        }
        let authority = try FieldDiagnosticDirectory.root(directory)
        directoryAuthority = authority
        return authority
    }

    private func archiveDirectory(_ path: String, create: Bool = false) throws -> FieldDiagnosticDirectory {
        var authority = try storageDirectory()
        var prefix = ""
        for component in path.split(separator: "/", omittingEmptySubsequences: false) {
            prefix = prefix.isEmpty ? String(component) : "\(prefix)/\(component)"
            if let retained = archiveAuthorities[prefix] {
                try retained.validate()
                authority = retained
            } else {
                authority = try authority.child(String(component), create: create)
                archiveAuthorities[prefix] = authority
            }
        }
        return authority
    }

    /// Writes a new file; an existing file is never replaced. The bytes go
    /// to a staging file (`.tmp`, which readers ignore) first; the hard link
    /// to the final name appears at once, complete, and fails when that name
    /// exists. Paths stay relative to retained directory descriptors.
    private func write<T: Encodable>(_ value: T, to file: URL) throws {
        let relative = file.deletingLastPathComponent().pathComponents.dropFirst(directory.pathComponents.count).joined(separator: "/")
        let folder = try archiveDirectory(relative, create: true)
        try folder.write(Self.encoder.encode(value), name: file.lastPathComponent, replacing: false)
    }

    private func readAll<T: Decodable>(_ type: T.Type, in folder: String) throws -> [T] {
        let stored: FieldDiagnosticDirectory
        do {
            stored = try archiveDirectory(folder)
        } catch DiagnosticReadFailure.notFound {
            return []
        }
        return try stored.names().filter { $0.hasSuffix(".json") }.sorted().compactMap { name -> T? in
            do {
                return try JSONDecoder().decode(type, from: stored.read(name))
            } catch is DecodingError {
                // A partly written report: kept on disk, left out of the export.
                return nil
            }
        }
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
                    let code = FieldDiagnosticObservation.Failure.storageFailure(for: error)
                    Self.log.error("fieldDiagnosticsStoreFailure operation=metricWrite code=\(code.rawValue, privacy: .public)")
                }
            }
        }
        diagnosticLoop = {
            for await report in diagnosticReports {
                do {
                    try await store.record(report)
                } catch {
                    let code = FieldDiagnosticObservation.Failure.storageFailure(for: error)
                    Self.log.error("fieldDiagnosticsStoreFailure operation=diagnosticWrite code=\(code.rawValue, privacy: .public)")
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
