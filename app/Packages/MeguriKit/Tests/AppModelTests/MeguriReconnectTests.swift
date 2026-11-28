import Foundation
import MeguriCoreBindings
import Testing
@testable import AppModel
@testable import CoreBridge

/// MP5/MP6.10: phase-reducer edge cases; the live command path is covered by
/// ReviewCollectionStartupTests, transport and the durable binding by Rust.
@MainActor @Suite struct MeguriReconnectTests {
    private func phase(_ outcome: MeguriRoundPhaseOutcome, tracking: BridgeError? = nil) -> MeguriRoundPhaseResult {
        .init(outcome: outcome, finalization: nil, trackingFailure: tracking)
    }
    private func collection(_ kind: String? = nil, unknown: Bool = false,
                            refusal: MeguriCoreBindings.ProviderRoundRefusal? = nil,
                            recovery: MeguriCoreBindings.ProviderRoundRecovery? = nil,
                            state: MeguriCoreBindings.ProviderCollectionRoundState? = nil) -> MeguriRoundPhaseOutcome {
        .collection(.init(.init(state: state ?? (kind == nil ? .noChanges : .failed),
            failure: kind.map { .init(kind: $0, method: nil, status: nil, clockOffsetSeconds: nil, databaseCheckIsLocal: nil) },
            refusal: refusal, acceptanceUnknown: unknown, skipped: 0, recovery: recovery)))
    }
    private func media(_ kind: String? = nil,
                       refusal: MeguriCoreBindings.ProviderRoundRefusal? = nil,
                       recovery: MeguriCoreBindings.ProviderRoundRecovery? = nil) -> MeguriRoundPhaseResult {
        phase(.media(.init(.init(complete: kind == nil && refusal == nil && recovery == nil,
            failure: kind.map { .init(kind: $0, method: nil, status: nil, clockOffsetSeconds: nil, databaseCheckIsLocal: nil) },
            refusal: refusal, recovery: recovery,
            uploadedFiles: 0, uploadedDeletions: 0, downloadedFiles: 0, downloadedDeletions: 0,
            bytesUploaded: 0, bytesDownloaded: 0, checked: 0, resynced: false,
            excluded: 0, pending: 0, awaiting: 0, affected: [], accountingFailure: nil))))
    }
    private func route(_ state: MeguriRoutineState, stopped: Bool = false, settling: Bool = false,
                       connection: MeguriProviderConnectionPhase = .setupRequired,
                       structure: Bool = false) -> MeguriSyncNowRoute {
        MeguriSyncStatusPresentation(stopped: stopped, connection: connection, stage: nil,
            routine: state, settling: settling, structureUploadPending: structure).route
    }

    @Test func networkFailureKindsAndOtherTransientStartsKeepSyncNowEligible() {
        for kind in ["network", "offline", "timeout"] {
            let network = SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind)), media: nil))
            #expect(network == .network && route(network) == .round)
        }
        for refusal: MeguriRoundRefusal in [.busy, .interrupted] {
            let waiting = SyncCoordinator.meguriRoundState(.init(collection: phase(.refused(refusal)), media: nil))
            #expect(waiting == .waiting && route(waiting) == .round)
        }
        let settings = SyncDeviceSettings(enabled: true, paused: false, automatic: false, media: true)
        #expect(!SyncCoordinator.meguriSchedulingAllowed(automatic: true, committed: settings, desired: nil))
        #expect(SyncCoordinator.meguriSchedulingAllowed(automatic: false, committed: settings, desired: nil))
        #expect(!SyncCoordinator.meguriSchedulingAllowed(automatic: false, committed: settings, desired: settings))
    }

    @Test func transientStartRefusalDoesNotBroadenAutomaticReplays() {
        for refusal: MeguriRoundRefusal in [.busy, .interrupted, .storage] {
            let availability = MeguriWakeAvailability(generation: 4)
            availability.observe(kind: .available, sequence: 10, generation: 4)
            #expect(availability.consume(collectionOutcome: .refused(refusal), since: 0, allowed: true)
                == (refusal == .busy))
            availability.observe(kind: .available, sequence: 11, generation: 4)
            #expect(!availability.consume(collectionOutcome: .refused(refusal), since: 10, allowed: true))
        }
        for allowed in [false, true] {
            let availability = MeguriWakeAvailability(generation: 4)
            availability.observe(kind: .available, sequence: 10, generation: 5)
            #expect(!availability.consume(collectionOutcome: .refused(.busy), since: 0, allowed: allowed))
        }
    }

    @Test func unsampledAuthorityAndFinalizationOutcomesStayFenced() {
        for refusal: MeguriRoundRefusal in [.notSelected, .paused, .notReady, .bindingChanged,
                                          .connectionChanged, .accountChanged, .publicationChanged] {
            let state = SyncCoordinator.meguriRoundState(.init(collection: phase(.refused(refusal)), media: nil))
            #expect(state == .failed && route(state) == .disabled(nil))
        }
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(.pending), media: nil)) == .failed)
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(.refused(.storage), tracking: .cancelled), media: nil)) == .failed)
        for refusal: MeguriCoreBindings.ProviderRoundRefusal in [.busy, .interrupted, .storage, .bindingChanged,
                                                                 .connectionChanged, .accountChanged, .publicationChanged] {
            // A terminal refusal is not an unstarted transient, even with a network error.
            let value = MeguriRoundReport(collection: phase(collection("network", refusal: refusal)), media: nil)
            #expect(SyncCoordinator.meguriRoundState(value) == .failed)
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()),
                media: media("network", refusal: refusal))) == .failed)
        }
        for refusal: MeguriCoreBindings.ProviderRoundRefusal in [.bindingChanged, .connectionChanged,
                                                                 .accountChanged, .publicationChanged] {
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection("interrupted", refusal: refusal)),
                media: nil), cancelled: true) == .failed)
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()),
                media: media("interrupted", refusal: refusal)), cancelled: true) == .failed)
        }
        for kind in ["timeout", "network", "interrupted"] {
            let unknown = SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, unknown: true)), media: nil), cancelled: true)
            #expect(unknown == .acceptanceUnknown && route(unknown) != .round)
            let recovery = SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, recovery: .storage)), media: nil), cancelled: true)
            #expect(recovery == .failed && route(recovery) != .round)
        }
        let full = SyncCoordinator.meguriRoundState(.init(collection: phase(collection(state: .fullSyncRequired)), media: nil))
        #expect(full == .fullSyncRequired && route(full) != .round)
        let auth = SyncCoordinator.meguriRoundState(.init(collection: phase(collection("auth_failed")), media: nil))
        #expect(auth == .authentication && route(auth) != .round)
        for state: MeguriRoutineState in [.waiting, .network, .cancelled] {
            #expect(route(state, stopped: true) != .round)
            #expect(route(state, settling: true) != .round)
            #expect(route(state, connection: .blocked) != .round)
            #expect(route(state, structure: true) != .round)
        }
        let failed = SyncKinds.meguri(.init(routine: .failed))
        #expect(failed.titleKey == "sync.meguri.status.failed.title" && failed.detailKey == nil)
        #expect(SyncKinds.meguriRow(.init()).textKey == "sync.meguri.status.setupUnavailable")
    }

    @Test func cancellationAtTheCollectionMediaBoundaryKeepsWorkPendingWithoutClaimingSuccess() {
        let partial = MeguriRoundReport(collection: phase(collection()), media: nil)
        #expect(!partial.complete)
        #expect(SyncCoordinator.meguriRoundState(partial, cancelled: true) == .cancelled)
        #expect(SyncCoordinator.meguriRoundState(partial) == .failed, "missing media alone proves no cancellation")
        #expect(SyncCoordinator.meguriThrownRoundState(BridgeError.cancelled) == .cancelled)
        #expect(SyncCoordinator.meguriThrownRoundState(CancellationError()) == .cancelled)
        #expect(SyncCoordinator.meguriThrownRoundState(BridgeError.internal("synthetic")) == .failed)
        // Actual phase results outrank the caller's cancellation bit.
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection("timeout")), media: nil), cancelled: true) == .network)
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()), media: media("timeout"))) == .network)
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()), media: media("interrupted"))) == .cancelled)
        for kind in ["interrupted", "cancelled"] {
            for refusal: MeguriCoreBindings.ProviderRoundRefusal in [.notSelected, .paused, .notReady,
                                                                     .busy, .interrupted, .storage] {
                #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, refusal: refusal)),
                    media: nil)) == .cancelled)
            }
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()),
                media: media(kind, refusal: .interrupted))) == .failed)
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, unknown: true, refusal: .interrupted)),
                media: nil)) == .acceptanceUnknown)
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, refusal: .interrupted, recovery: .storage)),
                media: nil)) == .failed)
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection(kind, refusal: .interrupted), tracking: .cancelled),
                media: nil)) == .failed)
        }
        #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()), media: media("timeout", recovery: .storage))) == .failed)
        for refusal: MeguriRoundRefusal in [.busy, .interrupted, .storage] {
            #expect(SyncCoordinator.meguriRoundState(.init(collection: phase(collection()), media: phase(.refused(refusal)))) == .waiting)
        }
    }

    @Test func explicitFinalizationOutcomesOverrideOnlyTheirOwnPhase() {
        let held = MeguriRoundReport(collection: phase(collection("interrupted", recovery: .storage)), media: nil)
        #expect(SyncCoordinator.meguriRoundState(held, cancelled: true) == .failed)
        #expect(SyncCoordinator.meguriRoundState(held, collectionFinalization: collection("interrupted")) == .cancelled)
        // A collection-only report (also used by structure operations) is not a whole exchange.
        #expect(SyncCoordinator.meguriRoundState(held, collectionFinalization: collection()) == .failed)
        let mediaHeld = MeguriRoundReport(collection: phase(collection()), media: media("interrupted", recovery: .storage))
        #expect(SyncCoordinator.meguriRoundState(mediaHeld, cancelled: true) == .failed)
        #expect(SyncCoordinator.meguriRoundState(mediaHeld, mediaFinalization: media("interrupted").outcome) == .cancelled)
        #expect(SyncCoordinator.meguriRoundState(mediaHeld, mediaFinalization: media().outcome) == .idle)
        let tracking = MeguriRoundReport(collection: phase(collection(), tracking: .cancelled), media: media())
        #expect(SyncCoordinator.meguriRoundState(tracking, collectionFinalization: collection(), cancelled: true) == .failed)
        let mediaTracking = MeguriRoundReport(collection: phase(collection()),
            media: phase(media().outcome, tracking: .cancelled))
        #expect(SyncCoordinator.meguriRoundState(mediaTracking, cancelled: true) == .failed)
    }
}
