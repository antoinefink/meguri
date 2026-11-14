import CoreBridge
import Foundation
import Instrumentation

/// All export types are owned scalar projections. No SDK object is Encodable
/// through this boundary, including MetricResult and DiagnosticResult.
nonisolated enum SafeFieldDiagnosticsExport {
    static let reportLimit = 32
    static let reportByteLimit = 65_536
    static let scanLimit = 512
    static let archiveCountLimit = 512
    static let archiveDepthLimit = 32
    static let archiveStructureLimit = 4_096

    /// Check structural cost before JSONDecoder constructs any containers.
    /// Quoted SDK text is skipped, including escaped quotes and brackets.
    static func validateArchive(_ bytes: Data) throws {
        var depth = 0
        var syntaxUnits = 0
        var quoted = false
        var escaped = false
        for byte in bytes {
            if quoted {
                if escaped { escaped = false }
                else if byte == 92 { escaped = true }
                else if byte == 34 { quoted = false }
                continue
            }
            if byte == 34 { quoted = true; continue }
            if byte == 123 || byte == 91 { depth += 1; syntaxUnits += 1 }
            else if byte == 125 || byte == 93 { depth -= 1 }
            else if byte == 44 || byte == 58 { syntaxUnits += 1 }
            guard depth <= archiveDepthLimit, syntaxUnits <= archiveStructureLimit else { throw DiagnosticReadFailure.oversized }
        }
    }

    struct ArchiveKey: CodingKey {
        let stringValue: String
        let intValue: Int? = nil
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }

    /// Read the owned array count without decoding any entry, state,
    /// measurement or histogram. The input cost was checked before decoding.
    struct EntryCount: Decodable {
        let value: Int
        init(from decoder: any Decoder) throws {
            let entries = try decoder.unkeyedContainer()
            guard let count = entries.count, count <= archiveCountLimit else { throw DiagnosticReadFailure.oversized }
            value = count
        }
    }

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

    struct ReportTime: Encodable, Decodable {
        let startUnixMilliseconds: Int64?
        let endUnixMilliseconds: Int64?
        private enum CodingKeys: String, CodingKey { case start, duration }
        init(from decoder: any Decoder) throws {
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            let start = try fields.decode(Date.self, forKey: .start)
            let duration = try fields.decode(TimeInterval.self, forKey: .duration)
            guard duration.isFinite, duration >= 0 else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Invalid report interval"))
            }
            startUnixMilliseconds = diagnosticMilliseconds(start)
            endUnixMilliseconds = diagnosticMilliseconds(start.addingTimeInterval(duration))
        }
        private enum ExportKeys: String, CodingKey { case startUnixMilliseconds, endUnixMilliseconds }
        func encode(to encoder: any Encoder) throws {
            var fields = encoder.container(keyedBy: ExportKeys.self)
            try fields.encodeIfPresent(startUnixMilliseconds, forKey: .startUnixMilliseconds)
            try fields.encodeIfPresent(endUnixMilliseconds, forKey: .endUnixMilliseconds)
        }
    }

    struct Metric: Encodable, Decodable {
        let receivedAtUnixMilliseconds: Int64?
        let reportTime: ReportTime
        let entryCount: Int
        // Histograms, states, units and SDK environment are intentionally not
        // projected in schema 1. Their absence is explicit, never zero values.
        let detailsUnavailable = true
        private enum CodingKeys: String, CodingKey { case received, timeRange, entries }
        init(from decoder: any Decoder) throws {
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            receivedAtUnixMilliseconds = diagnosticMilliseconds(try fields.decode(Date.self, forKey: .received))
            reportTime = try fields.decode(ReportTime.self, forKey: .timeRange)
            entryCount = try fields.decode(EntryCount.self, forKey: .entries).value
        }

        private enum ExportKeys: String, CodingKey { case receivedAtUnixMilliseconds, reportTime, entryCount, detailsUnavailable }
        func encode(to encoder: any Encoder) throws {
            var fields = encoder.container(keyedBy: ExportKeys.self)
            try fields.encodeIfPresent(receivedAtUnixMilliseconds, forKey: .receivedAtUnixMilliseconds)
            try fields.encode(reportTime, forKey: .reportTime)
            try fields.encode(entryCount, forKey: .entryCount)
            try fields.encode(detailsUnavailable, forKey: .detailsUnavailable)
        }
    }
    struct Diagnostic: Encodable, Decodable {
        enum Kind: String, Encodable { case hang, diskWriteException, appLaunch, memoryException, other }
        let receivedAtUnixMilliseconds: Int64?
        let reportTime: ReportTime
        let kind: Kind
        let detailsUnavailable = true
        private enum CodingKeys: String, CodingKey { case received, timeRange, result }
        init(from decoder: any Decoder) throws {
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            receivedAtUnixMilliseconds = diagnosticMilliseconds(try fields.decode(Date.self, forKey: .received))
            reportTime = try fields.decode(ReportTime.self, forKey: .timeRange)
            let result = try fields.nestedContainer(keyedBy: ArchiveKey.self, forKey: .result)
            guard result.allKeys.count == 1, let key = result.allKeys.first else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Invalid diagnostic discriminator"))
            }
            // Only the discriminator is read. Its SDK payload is never decoded.
            switch key.stringValue {
            case "hangDiagnostic", "hang": kind = .hang
            case "diskWriteExceptionDiagnostic", "diskWriteException": kind = .diskWriteException
            case "appLaunchDiagnostic", "appLaunch": kind = .appLaunch
            case "memoryExceptionDiagnostic", "memoryException":
                #if os(iOS)
                kind = .memoryException
                #else
                kind = .other
                #endif
            default: kind = .other
            }
        }
        private enum ExportKeys: String, CodingKey { case receivedAtUnixMilliseconds, reportTime, kind, detailsUnavailable }
        func encode(to encoder: any Encoder) throws {
            var fields = encoder.container(keyedBy: ExportKeys.self)
            try fields.encodeIfPresent(receivedAtUnixMilliseconds, forKey: .receivedAtUnixMilliseconds)
            try fields.encode(reportTime, forKey: .reportTime)
            try fields.encode(kind, forKey: .kind)
            try fields.encode(detailsUnavailable, forKey: .detailsUnavailable)
        }
    }
    struct Signpost: Encodable, Decodable {
        let reportTime: ReportTime
        let totalCount: UInt64?
        let detailsUnavailable = true
        // Used only to check membership in the registered interval allowlist.
        let name: String
        let category: String
        private enum CodingKeys: String, CodingKey { case timeRange, metric }
        private enum MetricKeys: String, CodingKey { case totalCount, signpostName, signpostCategory }
        init(from decoder: any Decoder) throws {
            let fields = try decoder.container(keyedBy: CodingKeys.self)
            reportTime = try fields.decode(ReportTime.self, forKey: .timeRange)
            let metric = try fields.nestedContainer(keyedBy: MetricKeys.self, forKey: .metric)
            totalCount = try metric.decode(UInt64.self, forKey: .totalCount)
            name = try metric.decode(String.self, forKey: .signpostName)
            category = try metric.decode(String.self, forKey: .signpostCategory)
        }
        private enum ExportKeys: String, CodingKey { case reportTime, totalCount, detailsUnavailable }
        func encode(to encoder: any Encoder) throws {
            var fields = encoder.container(keyedBy: ExportKeys.self)
            try fields.encode(reportTime, forKey: .reportTime)
            try fields.encodeIfPresent(totalCount, forKey: .totalCount)
            try fields.encode(detailsUnavailable, forKey: .detailsUnavailable)
        }
    }
    struct SignpostArchive: Decodable {
        let values: [Signpost]
        let omitted: Int
        init(from decoder: any Decoder) throws {
            var entries = try decoder.unkeyedContainer()
            guard let count = entries.count, count <= archiveCountLimit else { throw DiagnosticReadFailure.oversized }
            var summaries: [Signpost] = []
            for _ in 0..<min(count, reportLimit) { summaries.append(try entries.decode(Signpost.self)) }
            values = summaries
            omitted = max(0, count - reportLimit)
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

    static func make(directory: FieldDiagnosticDirectory?, now: Date, timeline: FieldDiagnosticTimeline,
                     deletions: [CloudZoneDeletionDiagnostic], reads: [CloudRootReadDiagnostic]) throws -> Data {
        var budget = Budget()
        var metricStatus = Section(detailsUnavailable: true)
        let metrics = reports(Metric.self, root: directory, folder: "metrics", budget: &budget, status: &metricStatus)
        var diagnosticStatus = Section(detailsUnavailable: true)
        let diagnostics = reports(Diagnostic.self, root: directory, folder: "diagnostics", budget: &budget, status: &diagnosticStatus)
        var signpostStatus = Section(detailsUnavailable: true)
        var signposts: [String: [Signpost]] = [:]
        for interval in IntervalNames.all {
            let stored = reports(SignpostArchive.self, root: directory, folder: "signposts/\(interval.name)", budget: &budget, status: &signpostStatus)
            var values: [Signpost] = []
            for report in stored {
                for value in report.values {
                    guard value.name == interval.name, value.category == interval.category.rawValue else {
                        signpostStatus.unreadableCount += 1; continue
                    }
                    if values.count < reportLimit { values.append(value) }
                    else { signpostStatus.omittedCountLowerBound += 1 }
                }
                signpostStatus.omittedCountLowerBound += report.omitted
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
    static func reports<T: Decodable>(_ type: T.Type, root: FieldDiagnosticDirectory?, folder: String,
                                      budget: inout Budget, status: inout Section) -> [T] {
        guard let root else { status.unavailable = true; return [] }
        guard budget.scanned < scanLimit else { status.scanTruncated = true; return [] }
        let stored: FieldDiagnosticDirectory
        let names: [String]
        do {
            stored = try root.folder(folder)
            names = try stored.names(limit: scanLimit - budget.scanned)
        } catch DiagnosticReadFailure.notFound {
            return []
        } catch {
            status.unavailable = true; return []
        }
        budget.scanned += names.count
        if budget.scanned == scanLimit { status.scanTruncated = true }
        let candidates = names.filter { !$0.hasPrefix(".") && $0.hasSuffix(".json") }.sorted(by: >)
        status.omittedCountLowerBound += max(0, candidates.count - reportLimit)
        var values: [T] = []
        for name in candidates.prefix(reportLimit).reversed() {
            guard budget.bytes > 0 else { status.omittedCountLowerBound += 1; status.scanTruncated = true; continue }
            do {
                let limit = min(reportByteLimit, budget.bytes)
                // Charge each attempt for its maximum read size; a complete
                // read returns unused capacity.
                budget.bytes -= limit
                let bytes = try stored.read(name, limit: limit)
                budget.bytes += limit - bytes.count
                try validateArchive(bytes)
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
