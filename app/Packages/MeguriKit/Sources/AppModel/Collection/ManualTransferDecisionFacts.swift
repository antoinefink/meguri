import CoreBridge
import Foundation

public struct ManualTransferDecisionFact: Identifiable {
    public let id: String
    public let label: String
    public let value: String
}

public extension ManualTransferRecord {
    /// The same facts appear before confirmation and in offline history. Dates
    /// come only from recorded instants; preparation is not a backup timestamp.
    var decisionFacts: [ManualTransferDecisionFact] {
        var facts: [ManualTransferDecisionFact] = []
        func add(_ id: String, _ label: String, _ value: String) {
            facts.append(.init(id: id, label: label, value: value))
        }
        func date(_ recordedInstant: Int64) -> String {
            let formatter = DateFormatter()
            formatter.dateStyle = .medium
            formatter.timeStyle = .short
            return formatter.string(from: Date(timeIntervalSince1970: Double(recordedInstant) / 1_000))
        }
        add("endpoint", "Configured endpoint", configuredEndpoint)
        if targetEndpoint != configuredEndpoint { add("target", "Approved destination", targetEndpoint) }
        add("direction", "Replacement", direction == .import ? "Server collection replaces this device’s collection." : "Prepared collection replaces the server collection and becomes active on this device.")
        add("prepared", "Prepared", date(preparedMilliseconds))
        if let local {
            add("local", "Local collection", "\(local.cards) cards, \(local.notes) notes, \(local.reviews) saved reviews.")
            add("pending", "Pending local work", "\(local.pendingReviews) reviews; \(local.pendingRows.reduce(0, +)) other changed records.")
        } else {
            add("local", "Local collection", "No readable collection. An existing file needs a raw safety copy before replacement.")
        }
        if let servedCounts {
            add("serverCounts", "Retained server content", "\(servedCounts.cards) cards, \(servedCounts.notes) notes, \(servedCounts.reviews) saved reviews. Deck count was not recorded.")
        } else {
            add("serverCounts", "Retained server content", "Card, note, review and deck counts are unknown.")
        }
        if let remote {
            add("remoteBaseline", "Server observation", direction == .export ? "Observed after retaining the server copy." : "Observed during import preparation.")
            add("remoteModified", "Server last modified", date(remote.modifiedMilliseconds))
            add("remoteSchema", "Server structure last modified", date(remote.schemaModifiedMilliseconds))
            add("remoteRevision", "Server change marker", "\(remote.revision). Used to detect changes; it does not guarantee a saved version or reserve the server collection.")
            add("remoteMediaRevision", "Server media change marker", mediaSelected ? "\(remote.mediaRevision). Media requires a separate decision." : "\(remote.mediaRevision). Media is excluded from this transfer.")
        } else {
            add("remoteBaseline", "Server observation", "Modification time, structure time and change markers were not recorded.")
        }
        if let empty = remoteHasNoCards {
            add("remoteEmpty", "Server card availability", empty ? "The server reports no cards; other content may still exist." : "The server reports cards are present.")
        }
        if copies.isEmpty { add("copies", "Retained copies", "No retained copy was recorded.") }
        for (index, copy) in copies.enumerated() {
            let key = "copy.\(index)"
            let title = copy.server ? "Server copy" : "Local safety copy"
            add(key + ".name", title, copy.name)
            add(key + ".kind", "\(title) format", copy.verifiedPackage ? "Verified collection package" : "Unverified raw file; inspect before use")
            add(key + ".size", "\(title) size", "\(copy.byteLength) bytes")
            let digest = copy.sha256.map { String(format: "%02x", $0) }.joined()
            add(key + ".identity", "\(title) file fingerprint", digest.isEmpty ? "Not recorded" : digest)
            add(key + ".time", "\(title) created", copy.instantMilliseconds.map(date) ?? "Creation time was not recorded.")
        }
        if acceptance == .exactOk {
            add("acceptance", "Server acceptance", "The server acknowledged the collection. Local activation and media completion require separate evidence.")
        }
        add("scope", "Transfer scope", mediaSelected ? "Collection and media selected; each has a separate completion outcome." : "Collection only. Media is excluded and unchanged on both sides.")
        if let media {
            let totals = media.counters
            let status: String
            if phase == .superseded {
                status = "Partial operation superseded by a verified local collection replacement. Media cannot resume for the replaced collection. Saved effects and any unknown effects remain recorded."
            } else {
                switch media.stage {
                case .complete: status = "Supported media complete."
                case .awaitingConsent: status = "Waiting for your decision. The operation is partial."
                case .declined: status = "Selected media remains for later. The operation is partial; saved effects remain."
                case .running: status = "Unfinished media work. Inspect before resuming; saved effects remain."
                case .cancelled: status = "Cancelled. The operation is partial; saved effects remain."
                case .interrupted: status = "Interrupted by the system. The operation is partial; saved effects remain."
                case .failed: status = "Failed. The operation is partial; saved effects remain."
                }
            }
            add("media.stage", "Media", status)
            add("media.downloaded", "Media received", "\(totals.downloadedFiles) files, \(totals.downloadedDeletions) deletions, \(totals.bytesDownloaded) file bytes recorded.")
            add("media.uploaded", "Media sent", "\(totals.uploadedFiles) files, \(totals.uploadedDeletions) deletions, \(totals.bytesUploaded) file bytes recorded.")
            if media.remoteOutcomeUnknown { add("media.unknown", "Other media effects", "Additional server effects may have completed without a saved acknowledgement.") }
            if media.exclusionsVerified {
                for (index, name) in totals.oversized.enumerated() { add("media.large.\(index)", "Excluded: over 100 MiB", name) }
                for (index, name) in totals.empty.enumerated() { add("media.empty.\(index)", "Excluded: empty file", name) }
            } else {
                add("media.counts", "Media work", "Current exclusions are unknown until a fresh media check completes. Recorded transfer counts retain saved effects from earlier attempts.")
            }
        }
        if let nextEligibleMilliseconds { add("next", "Next eligible", date(nextEligibleMilliseconds)) }
        return facts
    }
}
