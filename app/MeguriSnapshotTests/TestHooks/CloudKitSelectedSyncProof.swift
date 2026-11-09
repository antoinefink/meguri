#if DEBUG && MEGURI_TEST_HOOKS
import CoreBridge
import Foundation

nonisolated enum CloudKitSelectedSyncProof {
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
