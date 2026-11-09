#if DEBUG && MEGURI_TEST_HOOKS
import CoreBridge

nonisolated enum CloudKitSelectedSyncProof {
    static func requireReceiverPreview(_ summary: CloudConnectionSummary) throws {
        guard summary.localCollectionPresent, !summary.initialAdoptionRequired, !summary.localEmpty,
              summary.decision == .chooseCollection || summary.decision == .confirmAdoption,
              summary.manifestCounts != nil, summary.publishedAtMilliseconds != nil,
              summary.resumeBindingId == nil,
              summary.localCounts[.note] == 20, summary.localCounts[.card] == 20,
              summary.localCounts[.revlog] == 60, summary.localCounts[.deck] == 2,
              summary.localCounts[.notetype] == 1, summary.localCounts[.mediaName] == 5 else { throw AgentFailure.execution }
    }
    static func exchangeFacts(_ exchange: CloudPolicyExchangeRecord) throws -> [AgentFact] {
        guard case .succeeded(let selected, let held, let newer, let incoming, let media) = exchange.outcome else {
            throw AgentFailure.execution
        }
        var facts = [AgentFact(name: .syncResult, count: exchange.exchange, operation: "original-exchange"),
                     .init(name: .syncResult, count: selected, operation: "selected"),
                     .init(name: .syncResult, count: held, operation: "held-back"),
                     .init(name: .syncResult, count: newer, operation: "newer-pending"),
                     .init(name: .syncResult, count: incoming, operation: "incoming-waiting")]
        // Collection completion does not establish media completion. Nil means
        // unobserved, never zero. Keep every observed counter independently.
        for (label, count) in [("media-upload-pending", media.uploadFiles), ("media-download-pending", media.downloadFiles),
                               ("media-missing", media.missingNames), ("media-unknown", media.unknownBlobs),
                               ("media-uploads-running", media.uploadsRunning), ("media-downloads-running", media.downloadsRunning)] {
            if let count { facts.append(.init(name: .syncResult, count: count, operation: label)) }
        }
        return facts
    }
}
#endif
