import CoreBridge

/// The published sync state that study preparation reads, as plain values, so
/// that the mapping below is a pure function (`sched/app-coordination` F5.1).
struct StudyPreparationSyncInput: Equatable, Sendable {
    /// The service waits on the person: a stopped provider, an account, a
    /// sign-in, a pending setup or decision.
    var needsUser = false
    var enabled = true
    var automatic = true
    /// The update hold.
    var paused = false
    var selection: SyncProviderSelection?

    // Meguri Sync
    var meguriPhase: MeguriProviderConnectionPhase = .unconfigured
    /// The connection is loaded and its round service is ready.
    var meguriReady = false
    /// No connection load or other Meguri action runs and none is held back:
    /// only then does `meguriPhase` describe the stored connection.
    var meguriSettled = true
    var meguriRoutine: MeguriRoutineState?
    var meguriLastExchange: Int64?

    // iCloud Sync
    var cloudAttached = false
    var cloudState: CloudPolicyStatusState?
    var cloudLastEnded: Int64?
}

extension StudyPreparationSyncFacts {
    /// `.none` only for a state that is known: the service Off, Sync
    /// Automatically off, the update hold, a service that waits on the person,
    /// or no service set up. A service that may still connect (a connection
    /// load in flight, a sign-in under way, not ready yet) is expected to
    /// exchange: the job waits for it or for a study entry point (F5.1).
    static func mapping(_ input: StudyPreparationSyncInput) -> Self {
        if input.needsUser { return .none }
        guard input.enabled, input.automatic, !input.paused else { return .none }
        let waiting = Self(exchangeExpected: true, activity: .waiting)
        switch input.selection {
        case nil, .localOnly:
            return .none
        case .meguri:
            guard input.meguriReady else {
                // Before the first load the phase still reads `unconfigured`,
                // and a reload clears readiness first: neither is "not set up".
                guard input.meguriSettled else { return waiting }
                switch input.meguriPhase {
                case .unconfigured, .signedOut, .cancelled, .blocked: return .none
                case .authenticating, .setupRequired: return waiting
                }
            }
            switch input.meguriRoutine {
            case .exchanging:
                return .init(exchangeExpected: true, activity: .exchanging)
            case nil, .unreported, .network, .waiting:
                return waiting
            case .idle, .pending, .mediaBlocked, .authentication, .acceptanceUnknown, .fullSyncRequired,
                 .server, .cancelled, .failed:
                return .init(exchangeExpected: true, activity: .settled(endedMilliseconds: input.meguriLastExchange))
            }
        case .iCloud:
            guard input.cloudAttached else { return .none }
            switch input.cloudState {
            case .localOnly, .paused, .needsAttention:
                return .none
            case .iCloudUnavailable(let reason):
                return reason == .temporary ? waiting : .none
            case .syncingCollection:
                return .init(exchangeExpected: true, activity: .exchanging)
            case nil, .offline, .awaitingEvidence:
                return waiting
            case .syncingMedia, .changesPending, .upToDate:
                return .init(exchangeExpected: true, activity: .settled(endedMilliseconds: input.cloudLastEnded))
            }
        }
    }
}

extension SyncCoordinator {
    /// What study preparation may read from sync (`sched/app-coordination`
    /// F5.1). Derived from published state only; it asks the service for nothing.
    var studyPreparationInput: StudyPreparationSyncInput {
        .init(needsUser: needsUser, enabled: settings.enabled, automatic: settings.automatic,
            paused: settings.paused, selection: providerState?.selection,
            meguriPhase: meguriPhase, meguriReady: meguriAutomaticSettingVisible,
            meguriSettled: meguriLocalActionAvailable,
            meguriRoutine: meguriRoutine, meguriLastExchange: meguriLastExchange,
            cloudAttached: setupState == .attached, cloudState: status?.state,
            cloudLastEnded: status?.lastAttempt?.endedMilliseconds)
    }
    var studyPreparationFacts: StudyPreparationSyncFacts { .mapping(studyPreparationInput) }
}
