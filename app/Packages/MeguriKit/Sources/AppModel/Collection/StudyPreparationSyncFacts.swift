import CoreBridge

extension SyncCoordinator {
    /// What study preparation may read from sync (`sched/app-coordination`
    /// F5.1): whether an exchange is to be expected by itself, and whether a
    /// collection exchange runs or has ended. Derived from published state
    /// only; it asks the service for nothing.
    var studyPreparationFacts: StudyPreparationSyncFacts {
        // A stopped provider, an account, a sign-in, a pending setup or decision.
        if needsUser { return .none }
        // Off, Sync Automatically off and the update hold.
        guard settings.enabled, settings.automatic, !settings.paused else { return .none }
        switch providerState?.selection {
        case nil, .localOnly:
            return .none
        case .meguri:
            // Not connected yet (signing in, setting up): nothing syncs by itself.
            guard meguriAutomaticSettingVisible else { return .none }
            switch meguriRoutine {
            case .exchanging:
                return .init(exchangeExpected: true, activity: .exchanging)
            case nil, .unreported, .network, .waiting:
                return .init(exchangeExpected: true, activity: .waiting)
            case .idle, .pending, .mediaBlocked, .authentication, .acceptanceUnknown, .fullSyncRequired,
                 .server, .cancelled, .failed:
                return .init(exchangeExpected: true, activity: .settled(endedMilliseconds: meguriLastExchange))
            }
        case .iCloud:
            guard setupState == .attached else { return .none }
            switch status?.state {
            case .localOnly, .paused, .needsAttention:
                return .none
            case .iCloudUnavailable(let reason):
                return reason == .temporary ? .init(exchangeExpected: true, activity: .waiting) : .none
            case .syncingCollection:
                return .init(exchangeExpected: true, activity: .exchanging)
            case nil, .offline, .awaitingEvidence:
                return .init(exchangeExpected: true, activity: .waiting)
            case .syncingMedia, .changesPending, .upToDate:
                return .init(exchangeExpected: true,
                    activity: .settled(endedMilliseconds: status?.lastAttempt?.endedMilliseconds))
            }
        }
    }
}
