import CoreBridge
import Foundation

/// Where a blocked provider phase came from: Core's snapshot, or the call whose
/// failure this device's coordinator reports as blocked. Names only.
public enum ProviderBlockedSource: String, Codable, Sendable, Equatable {
    /// Core could not read or publish this device's provider metadata (MP1).
    case metadataUnreadable
    /// Core read the metadata but keeps network admission closed.
    case admissionClosed
    case migration, settingsWrite, publicationRetry, authorityRefresh, adoptionRestore
}

/// An error's kind, never its text.
public enum ProviderFailureReason: String, Codable, Sendable, Equatable {
    case busy, unavailable, cancelled, unsupported, invalidArgument, internalFailure, other
    public init(_ error: any Error) {
        switch error as? BridgeError {
        case .busy: self = .busy
        case .unavailable: self = .unavailable
        case .cancelled: self = .cancelled
        case .unsupported: self = .unsupported
        case .invalidArgument: self = .invalidArgument
        case .internal: self = .internalFailure
        default: self = error is CancellationError ? .cancelled : .other
        }
    }
}

/// Content-free sync facts for Copy Debug Info: enum and key names only. No
/// server address, account, file name or collection content.
public struct SyncDebugFacts: Codable, Sendable, Equatable {
    public var service: String
    public var providerPhase: String
    public var blockedSource: ProviderBlockedSource?
    public var blockedReason: ProviderFailureReason?
    public var meguriConnection: String?
    public var meguriSetupStage: String?
    public var meguriRoutine: String?
    public var statusTitleKey: String
    public var statusDetailKey: String?
    public init(service: String, providerPhase: String, blockedSource: ProviderBlockedSource? = nil,
                blockedReason: ProviderFailureReason? = nil, meguriConnection: String? = nil,
                meguriSetupStage: String? = nil, meguriRoutine: String? = nil,
                statusTitleKey: String, statusDetailKey: String? = nil) {
        self.service = service; self.providerPhase = providerPhase
        self.blockedSource = blockedSource; self.blockedReason = blockedReason
        self.meguriConnection = meguriConnection; self.meguriSetupStage = meguriSetupStage
        self.meguriRoutine = meguriRoutine; self.statusTitleKey = statusTitleKey; self.statusDetailKey = statusDetailKey
    }
}

/// The copied text: a short summary, then the safe export pretty-printed with
/// sorted keys, its timeline last. It adds only `SyncDebugFacts` to the export.
public enum SyncDebugInfoText {
    public static func compose(export: Data, sync: SyncDebugFacts?, limit: Int = FieldDiagnosticsStore.byteLimit) throws -> String {
        guard var root = try JSONSerialization.jsonObject(with: export) as? [String: Any] else {
            throw DiagnosticReadFailure.unavailable
        }
        if let sync {
            root["sync"] = try JSONSerialization.jsonObject(with: JSONEncoder().encode(sync))
        } else { root["syncUnavailable"] = true }
        var timeline = root.removeValue(forKey: "timeline") as? [String: Any] ?? [:]
        while true {
            let text = try render(root: root, timeline: timeline, sync: sync)
            guard text.utf8.count > limit else { return text }
            // Drop the oldest events first, as the export itself does.
            guard var events = timeline["events"] as? [Any], !events.isEmpty else { throw DiagnosticReadFailure.oversized }
            let drop = min(events.count, max(1, events.count / 8))
            events.removeFirst(drop)
            timeline["events"] = events
            timeline["truncatedCount"] = ((timeline["truncatedCount"] as? NSNumber)?.uint64Value ?? 0) &+ UInt64(drop)
        }
    }

    private static func render(root: [String: Any], timeline: [String: Any], sync: SyncDebugFacts?) throws -> String {
        let options: JSONSerialization.WritingOptions = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        let body = String(decoding: try JSONSerialization.data(withJSONObject: root, options: options), as: UTF8.self)
        let events = String(decoding: try JSONSerialization.data(withJSONObject: timeline, options: options), as: UTF8.self)
        return summary(root: root, timeline: timeline, sync: sync).joined(separator: "\n")
            + "\n\n== Export ==\n" + body + "\n\n== Timeline ==\n" + events + "\n"
    }

    static func summary(root: [String: Any], timeline: [String: Any], sync: SyncDebugFacts?) -> [String] {
        var lines = ["== Meguri Debug Info =="]
        if let header = root["header"] as? [String: Any] {
            let version = header["appVersion"] as? String ?? "unknown"
            let build = header["appBuild"] as? String ?? "unknown"
            var os = header["osFamily"] as? String ?? "unknown"
            if let v = header["osVersion"] as? [String: Any], let major = v["major"], let minor = v["minor"], let patch = v["patch"] {
                os += " \(major).\(minor).\(patch)"
            }
            lines.append("Generated: \(header["generatedAtUTC"] as? String ?? "unknown")  App: \(version) (\(build))  OS: \(os)")
        }
        if let sync {
            lines.append("Service: \(sync.service)  Provider phase: \(sync.providerPhase)")
            if let source = sync.blockedSource {
                lines.append("Blocked by: \(source.rawValue)\(sync.blockedReason.map { " (\($0.rawValue))" } ?? "")")
            }
            if sync.service == "meguri" {
                lines.append("Meguri: connection \(sync.meguriConnection ?? "none"), setup \(sync.meguriSetupStage ?? "none"), routine \(sync.meguriRoutine ?? "none")")
            }
            lines.append("Status: \(sync.statusTitleKey)\(sync.statusDetailKey.map { " / \($0)" } ?? "")")
        } else { lines.append("Sync: unavailable") }
        let events = (timeline["events"] as? [[String: Any]] ?? []).compactMap { $0["observation"] as? [String: Any] }
        lines.append("Timeline: \(events.count) events, \(timeline["truncatedCount"] ?? 0) truncated, \(timeline["droppedCount"] ?? 0) dropped")
        var outcomes: [String: Int] = [:], failures: [String: Int] = [:]
        for event in events {
            let component = event["component"] as? String ?? "?", outcome = event["outcome"] as? String ?? "?"
            outcomes["\(component) \(outcome)", default: 0] += 1
            if let failure = event["failure"] as? String {
                let key = [component, event["operation"] as? String ?? "?", event["stage"] as? String ?? "?", outcome, failure].joined(separator: "/")
                failures[key, default: 0] += 1
            }
        }
        if !outcomes.isEmpty {
            lines.append("By component and outcome: " + outcomes.sorted { $0.key < $1.key }.map { "\($0.key) ×\($0.value)" }.joined(separator: ", "))
        }
        if !failures.isEmpty {
            lines.append("Failures: " + failures.sorted { $0.key < $1.key }.map { "\($0.key) ×\($0.value)" }.joined(separator: ", "))
        }
        return lines
    }
}
