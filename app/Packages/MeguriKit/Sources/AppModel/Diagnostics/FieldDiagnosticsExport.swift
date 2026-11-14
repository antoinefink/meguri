import CoreBridge
import Darwin
import Foundation
import Instrumentation
import Synchronization

nonisolated enum DiagnosticReadFailure: Error { case oversized, unavailable, notFound }

/// Reject links and special files before any read; never allocate from an
/// untrusted file size. A racing replacement cannot redirect an open fd.
nonisolated func boundedDiagnosticRead(_ file: URL, limit: Int) throws -> Data {
    let descriptor = file.path(percentEncoded: false).withCString { open($0, O_RDONLY | O_NOFOLLOW | O_NONBLOCK) }
    guard descriptor >= 0 else {
        throw errno == ENOENT ? DiagnosticReadFailure.notFound : DiagnosticReadFailure.unavailable
    }
    let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
    defer { try? handle.close() }
    var facts = stat()
    guard fstat(descriptor, &facts) == 0, (facts.st_mode & S_IFMT) == S_IFREG else { throw DiagnosticReadFailure.unavailable }
    guard facts.st_size <= limit else { throw DiagnosticReadFailure.oversized }
    let bytes = try handle.read(upToCount: limit + 1) ?? Data()
    guard bytes.count <= limit else { throw DiagnosticReadFailure.oversized }
    return bytes
}

/// All export types are owned scalar projections. No SDK object is Encodable
/// through this boundary, including MetricResult and DiagnosticResult.
nonisolated enum SafeFieldDiagnosticsExport {
    static let reportLimit = 32
    static let reportByteLimit = 65_536
    static let scanLimit = 512

    struct Section: Encodable {
        var omittedCountLowerBound = 0
        var unreadableCount = 0
        var corruptCount = 0
        var oversizedCount = 0
        var scanTruncated = false
        var unavailable = false
        var availabilityUnknown = false
        var detailsUnavailable = false
    }
    struct Budget { var bytes = FieldDiagnosticsStore.byteLimit; var scanned = 0 }

    struct Header: Encodable {
        enum OSFamily: String, Encodable { case macOS, iOS, other }
        struct Version: Encodable { let major: Int; let minor: Int; let patch: Int }
        let schemaVersion = 1
        let generatedAtUTC: String?
        let appVersion: String?
        let appBuild: String?
        let appVersionUnavailable: Bool
        let appBuildUnavailable: Bool
        let osFamily: OSFamily
        let osVersion: Version
        init(now: Date) {
            let formatter = ISO8601DateFormatter()
            formatter.timeZone = TimeZone(secondsFromGMT: 0)
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            generatedAtUTC = diagnosticMilliseconds(now) == nil ? nil : formatter.string(from: now)
            appVersion = Self.safeVersion(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
            appBuild = Self.safeVersion(Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String)
            appVersionUnavailable = appVersion == nil; appBuildUnavailable = appBuild == nil
            #if os(macOS)
            osFamily = .macOS
            #elseif os(iOS)
            osFamily = .iOS
            #else
            osFamily = .other
            #endif
            let version = ProcessInfo.processInfo.operatingSystemVersion
            osVersion = Version(major: version.majorVersion, minor: version.minorVersion, patch: version.patchVersion)
        }
        static func safeVersion(_ value: String?) -> String? {
            guard let value, !value.isEmpty, value.utf8.prefix(65).count <= 64,
                  value.utf8.allSatisfy({ (48...57).contains($0) || $0 == 46 }),
                  value.utf8.contains(where: { (48...57).contains($0) }) else { return nil }
            return value
        }
    }

    struct ReportTime: Encodable {
        let startUnixMilliseconds: Int64?
        let endUnixMilliseconds: Int64?
        init(_ value: DateInterval) {
            startUnixMilliseconds = diagnosticMilliseconds(value.start)
            endUnixMilliseconds = diagnosticMilliseconds(value.end)
        }
    }
    struct Metric: Encodable {
        let receivedAtUnixMilliseconds: Int64?
        let reportTime: ReportTime
        let entryCount: Int
        // Histograms, states, units and SDK environment are intentionally not
        // projected in schema 1. Their absence is explicit, never zero values.
        let detailsUnavailable = true
        init(_ value: FieldMetrics) {
            receivedAtUnixMilliseconds = diagnosticMilliseconds(value.received)
            reportTime = ReportTime(value.timeRange); entryCount = value.entries.count
        }
    }
    struct Diagnostic: Encodable {
        enum Kind: String, Encodable { case hang, diskWriteException, appLaunch, memoryException, other }
        let receivedAtUnixMilliseconds: Int64?
        let reportTime: ReportTime
        let kind: Kind
        let detailsUnavailable = true
        init(_ value: FieldDiagnostic) {
            receivedAtUnixMilliseconds = diagnosticMilliseconds(value.received)
            reportTime = ReportTime(value.timeRange)
            switch value.result {
            case .hang: kind = .hang
            case .diskWriteException: kind = .diskWriteException
            case .appLaunch: kind = .appLaunch
            default:
                #if os(iOS)
                if case .memoryException = value.result { kind = .memoryException } else { kind = .other }
                #else
                kind = .other
                #endif
            }
        }
    }
    struct Signpost: Encodable {
        let reportTime: ReportTime
        let totalCount: UInt64?
        let detailsUnavailable = true
        init(_ value: FieldSignpost) {
            reportTime = ReportTime(value.timeRange)
            totalCount = UInt64(exactly: value.metric.totalCount)
        }
    }
    struct Failure: Encodable {
        let code: Int64?
        let retryAfterMilliseconds: Int64?
        let encryptedDataReset: Bool
        init(_ value: CloudZoneDeletionFailure) {
            code = value.code; retryAfterMilliseconds = value.retryAfterMilliseconds
            encryptedDataReset = value.encryptedDataReset
        }
    }
    struct RootRead: Encodable {
        enum Outcome: String, Encodable { case found, unknownItem, zoneNotFound, failed, other }
        let active: Bool
        let lostCollection: Bool
        let outcome: Outcome
        let a7Failure: Bool
        let accepted: Bool
        let failure: Failure?
        let terminal: Failure?
        init(_ value: CloudRootReadDiagnostic) {
            active = value.active; lostCollection = value.lostCollection
            outcome = Outcome(rawValue: value.outcome) ?? .other
            a7Failure = value.a7Failure; accepted = value.accepted
            failure = value.failure.map(Failure.init); terminal = value.terminal.map(Failure.init)
        }
    }
    struct Deletion: Encodable {
        enum Kind: String, Encodable { case read, deleted, localFailure }
        struct Result: Encodable { let failure: Failure? }
        let kind: Kind
        var found: Bool?
        var allowed: UInt64?
        var failure: Failure?
        var terminal: Failure?
        var results: [Result]?
        var resultsOmittedCount: Int?
        var category: CloudZoneDeletionLocalFailure?
        init(_ value: CloudZoneDeletionDiagnostic) {
            switch value {
            case .read(let found, let allowed, let failure, let terminal):
                kind = .read; self.found = found; self.allowed = allowed
                self.failure = failure.map(Failure.init); self.terminal = terminal.map(Failure.init)
            case .deleted(let results, let terminal):
                kind = .deleted
                self.results = results.prefix(64).map { Result(failure: $0.failure.map(Failure.init)) }
                resultsOmittedCount = max(0, results.count - 64)
                self.terminal = terminal.map(Failure.init)
            case .localFailure(let category, let failure):
                kind = .localFailure; self.category = category; self.failure = Failure(failure)
            }
        }
    }
    struct Export: Encodable {
        let header: Header
        var timeline: FieldDiagnosticTimeline
        var metrics: [Metric]
        var diagnostics: [Diagnostic]
        var signposts: [String: [Signpost]]
        var zoneDeletions: [Deletion]
        var rootReads: [RootRead]
        var sections: [String: Section]
        let coreSnapshotIsAtomic = false
        // Core callbacks do not carry availability or collection-size facts.
        // An empty array must never imply collection health or convergence.
        let collectionDetailsUnavailable = true
    }

    static func make(directory: URL, now: Date, timeline: FieldDiagnosticTimeline,
                     deletions: [CloudZoneDeletionDiagnostic], reads: [CloudRootReadDiagnostic]) throws -> Data {
        var budget = Budget()
        var metricStatus = Section(detailsUnavailable: true)
        let metrics = reports(FieldMetrics.self, folder: directory.appending(path: "metrics"), budget: &budget, status: &metricStatus).map(Metric.init)
        var diagnosticStatus = Section(detailsUnavailable: true)
        let diagnostics = reports(FieldDiagnostic.self, folder: directory.appending(path: "diagnostics"), budget: &budget, status: &diagnosticStatus).map(Diagnostic.init)
        var signpostStatus = Section(detailsUnavailable: true)
        var signposts: [String: [Signpost]] = [:]
        let signpostRoot = directory.appending(path: "signposts")
        let signpostRootFacts = try? signpostRoot.resourceValues(forKeys: [.isSymbolicLinkKey])
        let signpostRootIsLink = signpostRootFacts?.isSymbolicLink == true
        if signpostRootIsLink { signpostStatus.unavailable = true }
        for interval in IntervalNames.all where !signpostRootIsLink {
            let stored = reports([FieldSignpost].self, folder: directory.appending(path: "signposts/\(interval.name)"), budget: &budget, status: &signpostStatus)
            var values: [Signpost] = []
            for report in stored {
                // Only registered intervals are exported.
                for value in report.prefix(reportLimit) {
                    guard value.metric.signpostName == interval.name,
                          value.metric.signpostCategory == interval.category.rawValue else {
                        signpostStatus.unreadableCount += 1; continue
                    }
                    if values.count < reportLimit { values.append(Signpost(value)) }
                    else { signpostStatus.omittedCountLowerBound += 1 }
                }
                signpostStatus.omittedCountLowerBound += max(0, report.count - reportLimit)
            }
            if !values.isEmpty { signposts[interval.name] = values }
        }
        var result = Export(header: Header(now: now), timeline: timeline, metrics: metrics, diagnostics: diagnostics,
                            signposts: signposts, zoneDeletions: deletions.suffix(128).map(Deletion.init),
                            rootReads: reads.suffix(64).map(RootRead.init), sections: [
                                "metrics": metricStatus, "diagnostics": diagnosticStatus, "signposts": signpostStatus,
                                "zoneDeletions": Section(omittedCountLowerBound: max(0, deletions.count - 128), availabilityUnknown: true),
                                "rootReads": Section(omittedCountLowerBound: max(0, reads.count - 64), availabilityUnknown: true)
                            ])
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        var bytes = try encoder.encode(result)
        // Final encoded-byte bound includes the header, metadata and nested
        // deletion results, rather than estimating from event counts.
        while bytes.count > FieldDiagnosticsStore.byteLimit {
            if !result.metrics.isEmpty {
                result.metrics.removeFirst(); result.sections["metrics"]?.omittedCountLowerBound += 1
            } else if !result.diagnostics.isEmpty {
                result.diagnostics.removeFirst(); result.sections["diagnostics"]?.omittedCountLowerBound += 1
            } else if let key = result.signposts.keys.sorted().first {
                result.sections["signposts"]?.omittedCountLowerBound += result.signposts.removeValue(forKey: key)?.count ?? 0
            } else if !result.zoneDeletions.isEmpty {
                result.zoneDeletions.removeFirst(); result.sections["zoneDeletions"]?.omittedCountLowerBound += 1
            } else if !result.rootReads.isEmpty {
                result.rootReads.removeFirst(); result.sections["rootReads"]?.omittedCountLowerBound += 1
            } else if !result.timeline.events.isEmpty {
                result.timeline.events.removeFirst(); result.timeline.truncatedCount = saturatedAdd(result.timeline.truncatedCount, 1)
            } else { throw DiagnosticReadFailure.oversized }
            bytes = try encoder.encode(result)
        }
        return bytes
    }

    /// A bounded directory sample, newest filenames within that sample. When
    /// enumeration stops early, no claim is made about unvisited reports.
    static func reports<T: Decodable>(_ type: T.Type, folder: URL, budget: inout Budget, status: inout Section) -> [T] {
        var folderFacts = stat()
        let accessible = folder.path(percentEncoded: false).withCString { lstat($0, &folderFacts) }
        guard accessible == 0 else {
            if errno != ENOENT { status.unavailable = true }
            return []
        }
        guard (folderFacts.st_mode & S_IFMT) == S_IFDIR else { status.unavailable = true; return [] }
        guard budget.scanned < scanLimit else { status.scanTruncated = true; return [] }
        let enumerationFailed = Synchronization.Mutex(false)
        guard let enumerator = FileManager.default.enumerator(at: folder, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey],
                                                             options: [.skipsSubdirectoryDescendants, .skipsHiddenFiles], errorHandler: { _, _ in
            enumerationFailed.withLock { $0 = true }; return false
        }) else { status.unavailable = true; return [] }
        var candidates: [URL] = []
        while budget.scanned < scanLimit, let file = enumerator.nextObject() as? URL {
            budget.scanned += 1
            guard file.pathExtension == "json" else { continue }
            let facts = try? file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard facts?.isRegularFile == true, facts?.isSymbolicLink != true else { status.unreadableCount += 1; continue }
            candidates.append(file)
        }
        if budget.scanned == scanLimit { status.scanTruncated = true }
        status.unavailable = status.unavailable || enumerationFailed.withLock { $0 }
        candidates.sort { $0.lastPathComponent > $1.lastPathComponent }
        status.omittedCountLowerBound += max(0, candidates.count - reportLimit)
        var values: [T] = []
        for file in candidates.prefix(reportLimit).reversed() {
            guard budget.bytes > 0 else { status.omittedCountLowerBound += 1; status.scanTruncated = true; continue }
            do {
                let limit = min(reportByteLimit, budget.bytes)
                // Charge each attempt for its maximum read size; a complete
                // read returns unused capacity.
                budget.bytes -= limit
                let bytes = try boundedDiagnosticRead(file, limit: limit)
                budget.bytes += limit - bytes.count
                values.append(try JSONDecoder().decode(type, from: bytes))
            } catch DiagnosticReadFailure.oversized {
                status.oversizedCount += 1
            } catch is DecodingError {
                status.corruptCount += 1
            } catch {
                status.unreadableCount += 1
            }
        }
        return values
    }
}
