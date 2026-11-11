import Foundation

// Original completion inputs only. This does not prove SDK acknowledgements,
// state-save ordering, durable tracking or a recovered command's success.
nonisolated struct AgentOriginalExchangeAccounting {
    enum Outcome: String, CaseIterable, Hashable {
        case acked, clean, dropped, absent, blocked, outstanding
    }
    enum IncomingReason: String {
        case ready, generation, reference, structure, format, asset
        case applyFailed = "apply-failed"
        var waiting: Bool { self == .generation || self == .reference || self == .structure }
    }
    struct Incoming {
        let entity: String
        let reason: IncomingReason
    }
    let outcomes: [Outcome: Set<String>]
    let heldPending: Set<String>
    let heldAcknowledged: Set<String>
    let pendingUnheld: Set<String>
    let incoming: [Incoming]
    let media: [String: UInt64]

    private static let mediaCounts: Set<String> = [
        "media-upload-pending", "media-download-pending", "media-missing", "media-unknown",
        "media-uploads-running", "media-downloads-running", "media-upload-bytes", "media-download-bytes",
        "media-remaining-download-ms"
    ]
    private static let mediaFlags: Set<String> = ["media-root-acked", "media-zone-missing", "media-no-space"]

    init(records: [AgentRecord], capture: AgentRecord, terminal: AgentRecord, selected: Set<String>, exchange: UInt64) throws {
        let groups = records.filter { record in
            record.kind == .observation && record.command_seq == capture.command_seq &&
                record.facts.contains { $0.operation == "exchange-work" || $0.operation?.hasPrefix("work-") == true }
        }
        guard groups.count == 1, let group = groups.first,
              group.agent_instance == capture.agent_instance, terminal.agent_instance == capture.agent_instance,
              group.header_sha256 == capture.header_sha256, group.command_sha256 == capture.command_sha256,
              capture.record_seq < group.record_seq, group.record_seq < terminal.record_seq,
              group.facts.first == .init(name: .syncResult, count: exchange, operation: "exchange-work")
        else { throw AgentFailure.recovery }

        var outcomes = Dictionary(uniqueKeysWithValues: Outcome.allCases.map { ($0, Set<String>()) })
        var members = ["held-pending": Set<String>(), "held-acked": Set<String>(), "pending-unheld": Set<String>()]
        var incoming: [Incoming] = [], media: [String: UInt64] = [:], labels = Set<String>()
        for fact in group.facts.dropFirst() {
            guard let label = fact.operation, labels.insert(label).inserted else { throw AgentFailure.recovery }
            if let kind = Outcome.allCases.first(where: { label.hasPrefix("work-selected-" + $0.rawValue + "-") }) {
                let entity = try Self.entity(String(label.dropFirst(("work-selected-" + kind.rawValue + "-").count)))
                guard fact == .init(name: .syncResult, operation: label) else { throw AgentFailure.recovery }
                outcomes[kind, default: []].insert(entity)
            } else if let kind = members.keys.first(where: { label.hasPrefix("work-" + $0 + "-") }) {
                let entity = try Self.entity(String(label.dropFirst(("work-" + kind + "-").count)))
                guard fact == .init(name: .syncResult, operation: label) else { throw AgentFailure.recovery }
                members[kind, default: []].insert(entity)
            } else if label.hasPrefix("work-incoming") {
                let parts = label.split(separator: "-", maxSplits: 3).map(String.init)
                guard parts.count == 4, parts[1] == "incoming\(incoming.count + 1)",
                      let reason = IncomingReason(rawValue: parts[3]),
                      fact == .init(name: .syncResult, operation: label) else { throw AgentFailure.recovery }
                incoming.append(.init(entity: try Self.entity(parts[2]), reason: reason))
            } else if label.hasPrefix("work-") {
                let key = String(label.dropFirst(5))
                guard Self.mediaCounts.contains(key) || Self.mediaFlags.contains(key), let count = fact.count,
                      !Self.mediaFlags.contains(key) || count <= 1,
                      fact == .init(name: .syncResult, count: count, operation: label) else { throw AgentFailure.recovery }
                media[key] = count
            } else { throw AgentFailure.recovery }
        }
        var terminalCounts: [String: UInt64] = [:]
        let required: Set<String> = ["terminal-exchange-succeeded", "terminal-ended-ms", "terminal-selected",
                                     "terminal-held-back", "terminal-newer-pending", "terminal-incoming-waiting"]
        let allowed = required.union(Self.mediaCounts.union(Self.mediaFlags).map { "terminal-" + $0 })
        for fact in terminal.facts {
            guard let label = fact.operation, allowed.contains(label), let count = fact.count,
                  terminalCounts.updateValue(count, forKey: label) == nil,
                  fact == .init(name: .syncResult, count: count, operation: label) else { throw AgentFailure.recovery }
        }
        let all = outcomes.values.reduce(into: Set<String>()) { $0.formUnion($1) }
        let heldPending = members["held-pending", default: []], heldAcknowledged = members["held-acked", default: []]
        let pendingUnheld = members["pending-unheld", default: []]
        guard required.isSubset(of: Set(terminalCounts.keys)),
              terminalCounts["terminal-exchange-succeeded"] == exchange,
              all == selected, all.count == outcomes.values.reduce(0, { $0 + $1.count }),
              outcomes[.blocked, default: []].isEmpty, outcomes[.outstanding, default: []].isEmpty,
              heldPending.isDisjoint(with: pendingUnheld), heldAcknowledged.isSubset(of: outcomes[.acked, default: []]),
              terminalCounts["terminal-selected"] == UInt64(selected.count),
              terminalCounts["terminal-held-back"] == UInt64(heldPending.union(heldAcknowledged).count),
              terminalCounts["terminal-newer-pending"] == UInt64(pendingUnheld.count),
              incoming.allSatisfy({ $0.reason.waiting }),
              terminalCounts["terminal-incoming-waiting"] == UInt64(incoming.count),
              terminalCounts.filter({ $0.key.hasPrefix("terminal-media-") }) ==
                Dictionary(uniqueKeysWithValues: media.map { ("terminal-" + $0.key, $0.value) })
        else { throw AgentFailure.recovery }
        self.outcomes = outcomes; self.heldPending = heldPending; self.heldAcknowledged = heldAcknowledged
        self.pendingUnheld = pendingUnheld; self.incoming = incoming; self.media = media
    }

    private static func entity(_ label: String) throws -> String {
        guard label.hasPrefix("entity"), let number = UInt64(label.dropFirst(6)), number > 0,
              label == "entity\(number)" else { throw AgentFailure.recovery }
        return label
    }
}
