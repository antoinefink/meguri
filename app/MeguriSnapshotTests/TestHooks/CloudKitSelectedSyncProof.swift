#if DEBUG && MEGURI_TEST_HOOKS
import CoreBridge
import Foundation

nonisolated enum CloudKitSelectedSyncProof {
    static func requireSource(_ current: OriginalCloudObservation, edited: OriginalCloudEditFacts) throws {
        guard current.generation == edited.generation, current.seq >= edited.seq,
              current.observedSeq == edited.seq, current.qChanged,
              let revision = edited.localRevision, revision > 0,
              current.entity.label == "n1", current.entity.revision == revision,
              current.entity.digest == edited.noteDigest, current.entity.digest.count == 32 else { throw AgentFailure.execution }
    }
    static func requireRestoredSource(_ current: OriginalCloudObservation, edited: HistoricalOriginalCloudEdit) throws {
        guard current.generation == edited.generation, current.qChanged,
              current.entity.label == "n1", current.entity.revision == edited.localRevision,
              current.entity.digest == edited.noteDigest, current.entity.digest.count == 32,
              edited.originalSeq > 0, edited.originalGeneration > 0 else { throw AgentFailure.execution }
    }
    static func sourceAcknowledged(_ current: OriginalCloudObservation, edited: OriginalCloudEditFacts) throws {
        try requireSource(current, edited: edited)
        guard current.entity.acknowledged, current.entity.ackedRevision == edited.localRevision,
              current.entity.ackedEnvelopeDigest.count == 32 else { throw AgentFailure.execution }
    }
    static func peerFacts(_ current: OriginalCloudObservation) throws -> [AgentFact] {
        guard current.qChanged, current.entity.label == "n1", current.entity.digest.count == 32,
              let applied = current.appliedSeq, applied > 0,
              applied == current.observedSeq, applied <= current.seq else { throw AgentFailure.implementationMissing }
        return [.init(name: .localDurable, count: applied,
                      digest: CloudKitSmokeGeneratedInput.hex(current.entity.digest), operation: "n1")]
    }
    static func controlFacts(_ control: SelectedCloudControlObservation, generation: UInt64) throws -> [AgentFact] {
        guard control.generation == generation, control.status == .proven,
              control.bindingMatchesEpoch == .proven, control.requiredScanComplete,
              let active = control.activeSHA256, active.count == 32,
              let epoch = control.epochSHA256, epoch.count == 32 else { throw AgentFailure.execution }
        return [.init(name: .applied, digest: CloudKitSmokeGeneratedInput.hex(active), operation: "canonical-active"),
                .init(name: .applied, digest: CloudKitSmokeGeneratedInput.hex(epoch), operation: "canonical-complete-epoch"),
                .init(name: .applied, count: generation, operation: "binding-matches-epoch"),
                .init(name: .applied, count: generation, operation: "required-scan-native-sdk-admitted")]
    }
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
        return successfulExchangeFacts(exchange.exchange, selected: selected, held: held, newer: newer,
                                       incoming: incoming, media: media)
    }

    static func terminalExchangeFacts(_ exchange: CloudPolicyExchangeRecord) throws -> [AgentFact] {
        guard exchange.exchange > 0, exchange.source == .command else { throw AgentFailure.execution }
        let outcome: String
        var accounting: [AgentFact] = []
        switch exchange.outcome {
        case .succeeded(let selected, let held, let newer, let incoming, let media):
            outcome = "succeeded"
            accounting = successfulExchangeFacts(exchange.exchange, selected: selected, held: held, newer: newer,
                                                  incoming: incoming, media: media).dropFirst().map { fact in
                var entry = fact
                entry.operation = fact.operation.map { "terminal-" + $0 }
                return entry
            }
        case .incomplete(let outstanding, let blocked):
            outcome = "incomplete"
            accounting = [.init(name: .syncResult, count: outstanding, operation: "terminal-outstanding"),
                          .init(name: .syncResult, count: blocked, operation: "terminal-blocked")]
        case .waiting(let error, let retry):
            outcome = "waiting"
            accounting = [terminalError(error)]
            if let retry {
                accounting.append(.init(name: .syncResult, count: UInt64(bitPattern: retry), operation: "terminal-retry-ms"))
            }
        case .failed(let error):
            outcome = "failed"
            accounting = [terminalError(error)]
        case .cancelled:
            outcome = "cancelled"
        }
        return [.init(name: .syncResult, count: exchange.exchange, operation: "terminal-exchange-" + outcome),
                .init(name: .syncResult, count: UInt64(bitPattern: exchange.endedMilliseconds),
                      operation: "terminal-ended-ms")] + accounting
    }

    private static func terminalError(_ error: CloudPolicyOperationError) -> AgentFact {
        let label = switch error {
        case .noAccount: "no-account"
        case .configuration: "configuration"
        case .offline: "offline"
        case .throttled: "throttled"
        case .quotaFull: "quota-full"
        case .localStorageFull: "local-storage-full"
        case .trackingError: "tracking"
        case .conflictRecovery: "conflict-recovery"
        case .unsupportedSchema: "unsupported-schema"
        case .other: "other"
        }
        return .init(name: .syncResult, operation: "terminal-error-" + label)
    }

    private static func successfulExchangeFacts(_ exchange: UInt64, selected: UInt64, held: UInt64, newer: UInt64,
                                                incoming: UInt64, media: CloudPolicyMediaFacts) -> [AgentFact] {
        var facts = [AgentFact(name: .syncResult, count: exchange, operation: "original-exchange"),
                     .init(name: .syncResult, count: selected, operation: "selected"),
                     .init(name: .syncResult, count: held, operation: "held-back"),
                     .init(name: .syncResult, count: newer, operation: "newer-pending"),
                     .init(name: .syncResult, count: incoming, operation: "incoming-waiting")]
        // Collection completion does not establish media completion. Nil means
        // unobserved, never zero. Keep every observed counter independently.
        for (label, count) in [("media-upload-pending", media.uploadFiles), ("media-download-pending", media.downloadFiles),
                               ("media-missing", media.missingNames), ("media-unknown", media.unknownBlobs),
                               ("media-uploads-running", media.uploadsRunning), ("media-downloads-running", media.downloadsRunning),
                               ("media-upload-bytes", media.uploadBytes), ("media-download-bytes", media.downloadBytes),
                               ("media-remaining-download-ms", media.remainingDownloadMs)] {
            if let count { facts.append(.init(name: .syncResult, count: count, operation: label)) }
        }
        for (label, observed) in [("media-root-acked", media.rootAcked), ("media-zone-missing", media.zoneMissing),
                                  ("media-no-space", media.noSpace)] {
            if let observed { facts.append(.init(name: .syncResult, count: observed ? 1 : 0, operation: label)) }
        }
        return facts
    }
}

// Wrap only the existing native factory. The retained normal adapter's actual
// makeEngine call supplies the SDK identity after Core registration and scan.
nonisolated final class CloudKitSelectedSDKFactory: CloudKitEngineFactory {
    private let original: any CloudKitEngineFactory
    private let beforeMake: @MainActor @Sendable (CloudEngineConfiguration) async throws -> Void
    private let failed: @Sendable () -> Void
    init(original: any CloudKitEngineFactory,
         beforeMake: @escaping @MainActor @Sendable (CloudEngineConfiguration) async throws -> Void,
         failed: @escaping @Sendable () -> Void) {
        self.original = original; self.beforeMake = beforeMake; self.failed = failed
    }
    func accountChanges() async -> AsyncStream<Void>? { await original.accountChanges() }
    func resolveAccount() async throws -> CloudResolvedAccount? { try await original.resolveAccount() }
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        do {
            try await beforeMake(configuration)
            return try await original.makeEngine(configuration: configuration, delegate: delegate)
        } catch { failed(); throw error }
    }
}
#endif
