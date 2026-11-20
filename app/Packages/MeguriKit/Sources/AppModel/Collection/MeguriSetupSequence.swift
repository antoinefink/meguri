import CoreBridge
import Foundation

/// The bridge calls that Meguri Sync setup orders (`sync/meguri-provider`
/// MP6.3, MP6.9). The app's bridge provides them; a test provides a fake with
/// no core behind it, so that the order of the calls has tests of its own.
protocol MeguriSetupBridge: AnyObject, Sendable {
    func confirmSetup(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                      progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func prepareSetupDownload(_ decision: MeguriSetupDecision, preview: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                              progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func confirmSetupDownload(_ decision: MeguriSetupDecision, download: MeguriBootstrapPreview, choice: MeguriSetupChoice,
                              progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func settleSetup() async throws -> MeguriBootstrapCommandResult
    func prepareRecovery(_ operation: MeguriBootstrapOperation,
                         progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func confirmSetupActivation(_ recovery: MeguriBootstrapPreview,
                                progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func confirm(_ preview: MeguriBootstrapPreview,
                 progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> MeguriBootstrapCommandResult
    func discard(_ preview: MeguriBootstrapPreview) async throws -> MeguriBootstrapCommandResult
}
extension MeguriProviderBootstrapService: MeguriSetupBridge {}

/// The order of the bridge calls of one setup step. It holds no state: the
/// coordinator keeps what is shown, and the core decides what may run.
enum MeguriSetupSequence {
    /// The failure key that stands for MP6.9's "neither": an existing sentence
    /// of T19.5, whose action opens the keep-collection sheet.
    static let overtakenKey = "sync.needsYou.meguri.chooseCollection"
    /// A routine round that needs a full sync (MP4): the same sentence and
    /// action as `overtakenKey`, but Cancel in its sheet keeps the service.
    static let fullSyncKey = "meguri.setup.fullSyncRequired"
    /// N4: a collection file that is present but closed or damaged. The
    /// closest existing sentence; its action is Copy Debug Info.
    static let localUnusableKey = "sync.attention.collectionCorrupt"

    /// One row's or one answer's transfer. Nil, with no call made, when the
    /// choice is not one the row offers.
    @MainActor static func transfer(_ bridge: any MeguriSetupBridge, decision: MeguriSetupDecision, preview: MeguriBootstrapPreview,
                         choice: MeguriSetupChoice,
                         progress: @escaping @Sendable (CoreProgress) -> Void,
                         proceed: @MainActor () -> Bool = { true }) async throws -> MeguriBootstrapCommandResult? {
        guard let confirmsPreview = SyncCoordinator.meguriSetupConfirmsPreview(decision.facts.row, choice) else { return nil }
        if confirmsPreview {
            return try await bridge.confirmSetup(decision, preview: preview, choice: choice, progress: progress)
        }
        let prepared = try await bridge.prepareSetupDownload(decision, preview: preview, choice: choice, progress: progress)
        guard case .preview(let download) = prepared.outcome else { return prepared }
        // A Cancel that landed after the preparation ended reached no job: the
        // prepared download replaced nothing and is discarded.
        guard proceed() else {
            _ = try? await bridge.discard(download)
            throw CancellationError()
        }
        return try await bridge.confirmSetupDownload(decision, download: download, choice: choice, progress: progress)
    }

    enum Settled {
        /// Local activation remains. `hintUnchanged` only says whether to try
        /// it with no question; the core decides when it is tried.
        case accepted(MeguriBootstrapOperation, hintUnchanged: Bool)
        case notAccepted
        /// Neither: recorded by the core. The keep-collection sheet is next.
        case overtaken
        /// Not readable, or nothing learned: the state and the exclusion stay.
        case unknown
    }
    static func settled(_ outcome: MeguriBootstrapOutcome) -> Settled {
        guard case .settled(let settlement) = outcome else { return .unknown }
        switch settlement {
        case .accepted(let operation, let localUnchanged): return .accepted(operation, hintUnchanged: localUnchanged)
        case .notAccepted: return .notAccepted
        case .neither: return .overtaken
        }
    }

    /// Local activation of an upload the server accepted. `asked` is true only
    /// for the user's own Finish Setup. Without it the recovery preview is
    /// confirmed through the core's test that this device is still what the
    /// upload sent, and a device that changed is refused with nothing replaced.
    /// Returns the preparation, and the confirmation when one was made.
    @MainActor static func finish(_ bridge: any MeguriSetupBridge, operation: MeguriBootstrapOperation, asked: Bool,
                       progress: @escaping @Sendable (CoreProgress) -> Void,
                       current: @MainActor () -> Bool) async throws
        -> (preparation: MeguriBootstrapCommandResult, confirmation: MeguriBootstrapCommandResult?) {
        let preparation = try await bridge.prepareRecovery(operation, progress: progress)
        guard current() else { throw CancellationError() }
        guard case .preview(let recovery) = preparation.outcome else { return (preparation, nil) }
        let confirmation = asked
            ? try await bridge.confirm(recovery, progress: progress)
            : try await bridge.confirmSetupActivation(recovery, progress: progress)
        return (preparation, confirmation)
    }

    /// After a transfer that stopped: the notice for the one reload of both
    /// sides, or nil when nothing is read again.
    static func reloadNotice(after outcome: MeguriBootstrapOutcome?, reloads: Int, choiceAllowed: Bool) -> String? {
        guard reloads > 0, choiceAllowed, let outcome else { return nil }
        return SyncCoordinator.meguriSetupChangeKey(outcome)
    }

    /// Cancel in the sheet: the prepared transfer sent nothing and is discarded.
    /// True when the core discarded it.
    @MainActor static func cancel(_ bridge: any MeguriSetupBridge, preview: MeguriBootstrapPreview) async throws
        -> (result: MeguriBootstrapCommandResult, discarded: Bool) {
        let result = try await bridge.discard(preview)
        if case .refused = result.outcome { return (result, false) }
        return (result, true)
    }

    /// Cancel in a sheet that a status action opened changes nothing but the
    /// sheet (V27.4). After an overtaken upload it leaves the service Off, as
    /// during setup: that upload's own setup never finished (MP6.9, MP6.3).
    static func keepsService(requested: Bool, stage: MeguriSetupStage?, failureKey: String?) -> Bool {
        requested && !(stage == .failed && failureKey == overtakenKey)
    }
}
