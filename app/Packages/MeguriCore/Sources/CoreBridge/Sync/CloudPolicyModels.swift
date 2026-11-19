import Foundation
internal import MeguriCoreBindings

/// Policy DTOs contain presentation and coordinator inputs only. Opaque
/// command handles retain Rust authority and cannot be constructed by callers.

public enum CloudPolicyZone: Sendable, Hashable {
    case root, collection, media
    init(_ v: MeguriCoreBindings.CloudPolicyZone) {
        self = switch v {
        case .root: .root
        case .collection: .collection
        case .media: .media
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyZone {
        switch self {
        case .root: .root
        case .collection: .collection
        case .media: .media
        }
    }
}

public enum CloudPolicyOperationKind: Sendable, Hashable {
    case send, fetch, read
    init(_ v: MeguriCoreBindings.CloudPolicyOperationKind) {
        self = switch v {
        case .send: .send
        case .fetch: .fetch
        case .read: .read
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyOperationKind {
        switch self {
        case .send: .send
        case .fetch: .fetch
        case .read: .read
        }
    }
}

public enum CloudPolicyOperationError: Sendable, Hashable {
    case noAccount, configuration, offline, throttled, quotaFull, localStorageFull, trackingError, conflictRecovery, unsupportedSchema, other
    init(_ v: MeguriCoreBindings.CloudPolicyOperationError) {
        self = switch v {
        case .noAccount: .noAccount
        case .configuration: .configuration
        case .offline: .offline
        case .throttled: .throttled
        case .quotaFull: .quotaFull
        case .localStorageFull: .localStorageFull
        case .trackingError: .trackingError
        case .conflictRecovery: .conflictRecovery
        case .unsupportedSchema: .unsupportedSchema
        case .other: .other
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyOperationError {
        switch self {
        case .noAccount: .noAccount
        case .configuration: .configuration
        case .offline: .offline
        case .throttled: .throttled
        case .quotaFull: .quotaFull
        case .localStorageFull: .localStorageFull
        case .trackingError: .trackingError
        case .conflictRecovery: .conflictRecovery
        case .unsupportedSchema: .unsupportedSchema
        case .other: .other
        }
    }
}

public enum CloudPolicySource: Sendable, Hashable {
    case command, key, ankiConnect
    init(_ v: MeguriCoreBindings.CloudPolicySource) {
        self = switch v {
        case .command: .command
        case .key: .key
        case .ankiConnect: .ankiConnect
        }
    }
    var raw: MeguriCoreBindings.CloudPolicySource {
        switch self {
        case .command: .command
        case .key: .key
        case .ankiConnect: .ankiConnect
        }
    }
}

public enum CloudPolicyChangeCause: Sendable, Hashable {
    case local, cloud, scan, probe
    init(_ v: MeguriCoreBindings.CloudPolicyChangeCause) {
        self = switch v {
        case .local: .local
        case .cloud: .cloud
        case .scan: .scan
        case .probe: .probe
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyChangeCause {
        switch self {
        case .local: .local
        case .cloud: .cloud
        case .scan: .scan
        case .probe: .probe
        }
    }
}

public enum CloudPolicyUnavailable: Sendable, Hashable {
    case collectionUnavailable, reviewActive, notConnected, paused, iCloudUnavailable, decisionRequired, recoveryRequired
    init(_ v: MeguriCoreBindings.CloudPolicyUnavailable) {
        self = switch v {
        case .collectionUnavailable: .collectionUnavailable
        case .reviewActive: .reviewActive
        case .notConnected: .notConnected
        case .paused: .paused
        case .iCloudUnavailable: .iCloudUnavailable
        case .decisionRequired: .decisionRequired
        case .recoveryRequired: .recoveryRequired
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyUnavailable {
        switch self {
        case .collectionUnavailable: .collectionUnavailable
        case .reviewActive: .reviewActive
        case .notConnected: .notConnected
        case .paused: .paused
        case .iCloudUnavailable: .iCloudUnavailable
        case .decisionRequired: .decisionRequired
        case .recoveryRequired: .recoveryRequired
        }
    }
}

public enum CloudPolicyUnavailableReason: Sendable, Hashable {
    case noAccount, restricted, temporary, setupNeeded
    init(_ v: MeguriCoreBindings.CloudPolicyUnavailableReason) {
        self = switch v {
        case .noAccount: .noAccount
        case .restricted: .restricted
        case .temporary: .temporary
        case .setupNeeded: .setupNeeded
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyUnavailableReason {
        switch self {
        case .noAccount: .noAccount
        case .restricted: .restricted
        case .temporary: .temporary
        case .setupNeeded: .setupNeeded
        }
    }
}

public enum CloudPolicyPausedReason: Sendable, Hashable {
    case decisionRequired, userPaused, automaticOff
    init(_ v: MeguriCoreBindings.CloudPolicyPausedReason) {
        self = switch v {
        case .decisionRequired: .decisionRequired
        case .userPaused: .userPaused
        case .automaticOff: .automaticOff
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyPausedReason {
        switch self {
        case .decisionRequired: .decisionRequired
        case .userPaused: .userPaused
        case .automaticOff: .automaticOff
        }
    }
}

public enum CloudPolicyAccountStatus: Sendable, Hashable {
    case available, noAccount, restricted, temporary, configuration, undetermined
    init(_ v: MeguriCoreBindings.CloudPolicyAccountStatus) {
        self = switch v {
        case .available: .available
        case .noAccount: .noAccount
        case .restricted: .restricted
        case .temporary: .temporary
        case .configuration: .configuration
        case .undetermined: .undetermined
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyAccountStatus {
        switch self {
        case .available: .available
        case .noAccount: .noAccount
        case .restricted: .restricted
        case .temporary: .temporary
        case .configuration: .configuration
        case .undetermined: .undetermined
        }
    }
}

public enum CloudPolicyStopReason: Sendable, Hashable {
    case gateClosed, trackingFailed, bindingChanged
    init(_ v: MeguriCoreBindings.CloudPolicyStopReason) {
        self = switch v {
        case .gateClosed: .gateClosed
        case .trackingFailed: .trackingFailed
        case .bindingChanged: .bindingChanged
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyStopReason {
        switch self {
        case .gateClosed: .gateClosed
        case .trackingFailed: .trackingFailed
        case .bindingChanged: .bindingChanged
        }
    }
}

public enum CloudPolicyUpdate: Sendable, Hashable {
    case full, changes
    init(_ v: MeguriCoreBindings.CloudPolicyUpdate) {
        self = switch v {
        case .full: .full
        case .changes: .changes
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyUpdate {
        switch self {
        case .full: .full
        case .changes: .changes
        }
    }
}

public enum CloudPolicyAttention: Sendable, Hashable {
    case isolated, rootUnreadable, chooseCollection, conflicts, conflictsUnreadable, refused, referenceWaiting, oversize, unreadable, unsyncable, claimStuck, generationIncomplete, assetUnavailable, applyFailed, rekeyFailed, crtMismatch, collectionCorrupt, mediaZoneMissing, mediaChanged, mediaMissing, recoveryOpWaiting, exchangeIncomplete, quotaFull, localStorageFull, trackingError, unsupportedSchema, cloudError
    init(_ v: MeguriCoreBindings.CloudPolicyAttention) {
        self = switch v {
        case .isolated: .isolated
        case .rootUnreadable: .rootUnreadable
        case .chooseCollection: .chooseCollection
        case .conflicts: .conflicts
        case .conflictsUnreadable: .conflictsUnreadable
        case .refused: .refused
        case .referenceWaiting: .referenceWaiting
        case .oversize: .oversize
        case .unreadable: .unreadable
        case .unsyncable: .unsyncable
        case .claimStuck: .claimStuck
        case .generationIncomplete: .generationIncomplete
        case .assetUnavailable: .assetUnavailable
        case .applyFailed: .applyFailed
        case .rekeyFailed: .rekeyFailed
        case .crtMismatch: .crtMismatch
        case .collectionCorrupt: .collectionCorrupt
        case .mediaZoneMissing: .mediaZoneMissing
        case .mediaChanged: .mediaChanged
        case .mediaMissing: .mediaMissing
        case .recoveryOpWaiting: .recoveryOpWaiting
        case .exchangeIncomplete: .exchangeIncomplete
        case .quotaFull: .quotaFull
        case .localStorageFull: .localStorageFull
        case .trackingError: .trackingError
        case .unsupportedSchema: .unsupportedSchema
        case .cloudError: .cloudError
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyAttention {
        switch self {
        case .isolated: .isolated
        case .rootUnreadable: .rootUnreadable
        case .chooseCollection: .chooseCollection
        case .conflicts: .conflicts
        case .conflictsUnreadable: .conflictsUnreadable
        case .refused: .refused
        case .referenceWaiting: .referenceWaiting
        case .oversize: .oversize
        case .unreadable: .unreadable
        case .unsyncable: .unsyncable
        case .claimStuck: .claimStuck
        case .generationIncomplete: .generationIncomplete
        case .assetUnavailable: .assetUnavailable
        case .applyFailed: .applyFailed
        case .rekeyFailed: .rekeyFailed
        case .crtMismatch: .crtMismatch
        case .collectionCorrupt: .collectionCorrupt
        case .mediaZoneMissing: .mediaZoneMissing
        case .mediaChanged: .mediaChanged
        case .mediaMissing: .mediaMissing
        case .recoveryOpWaiting: .recoveryOpWaiting
        case .exchangeIncomplete: .exchangeIncomplete
        case .quotaFull: .quotaFull
        case .localStorageFull: .localStorageFull
        case .trackingError: .trackingError
        case .unsupportedSchema: .unsupportedSchema
        case .cloudError: .cloudError
        }
    }
}

public enum CloudPolicyFailure: Sendable, Hashable {
    case closed, wrongOwner, generationChanged, staleCommand, notCoreAction, notNativeAction, coreCapabilityUnavailable, unqualifiedCompletion
    init(_ v: MeguriCoreBindings.CloudPolicyFailure) {
        self = switch v {
        case .closed: .closed
        case .wrongOwner: .wrongOwner
        case .generationChanged: .generationChanged
        case .staleCommand: .staleCommand
        case .notCoreAction: .notCoreAction
        case .notNativeAction: .notNativeAction
        case .coreCapabilityUnavailable: .coreCapabilityUnavailable
        case .unqualifiedCompletion: .unqualifiedCompletion
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyFailure {
        switch self {
        case .closed: .closed
        case .wrongOwner: .wrongOwner
        case .generationChanged: .generationChanged
        case .staleCommand: .staleCommand
        case .notCoreAction: .notCoreAction
        case .notNativeAction: .notNativeAction
        case .coreCapabilityUnavailable: .coreCapabilityUnavailable
        case .unqualifiedCompletion: .unqualifiedCompletion
        }
    }
}

public struct CloudPolicyDeviceSettings: Sendable, Equatable {
    public let enabled: Bool
    public let paused: Bool
    public let automatic: Bool
    public let media: Bool
    public init(enabled: Bool, paused: Bool, automatic: Bool, media: Bool) {
        self.enabled = enabled; self.paused = paused; self.automatic = automatic; self.media = media
    }
    var raw: MeguriCoreBindings.CloudPolicyDeviceSettings { .init(enabled: enabled, paused: paused, automatic: automatic, media: media) }
}
public struct CloudPolicyOperation: Sendable, Hashable {
    public let zone: CloudPolicyZone
    public let kind: CloudPolicyOperationKind
    public init(zone: CloudPolicyZone, kind: CloudPolicyOperationKind) { self.zone = zone; self.kind = kind }
    init(_ v: MeguriCoreBindings.CloudPolicyOperation) { self.init(zone: .init(v.zone), kind: .init(v.kind)) }
    var raw: MeguriCoreBindings.CloudPolicyOperation { .init(zone: zone.raw, kind: kind.raw) }
}
public enum CloudPolicyActivity: Sendable {
    case started
    case ended(succeeded: Bool, saved: UInt64)
    var raw: MeguriCoreBindings.CloudPolicyActivity {
        switch self { case .started: .started; case .ended(let succeeded, let saved): .ended(succeeded: succeeded, saved: saved) }
    }
}
/// Engine/session/generation/binding facts are never supplied by native DTOs.
public enum CloudPolicyEvent: Sendable {
    case refresh(CloudPolicyChangeCause)
    case trackingWriteFailed(noSpace: Bool)
    case settings(CloudPolicyDeviceSettings)
    case network(reachable: Bool)
    case foreground, background(deadlineMilliseconds: Int64), backgroundExpiring, quitRequested, flushTimerFired
    case syncNowRequested(CloudPolicySource), syncNowCancelled(exchange: UInt64)
    case volumeSpaceAvailable, visibleMedia(names: [String])
    case operation(instance: String, operation: CloudPolicyOperation, phase: CloudPolicyActivity)
    case operationError(instance: String, operation: CloudPolicyOperation, error: CloudPolicyOperationError, code: Int64, retryAfterMilliseconds: Int64?)
    var raw: MeguriCoreBindings.CloudPolicyEvent {
        switch self {
        case .refresh(let cause): .refresh(cause: cause.raw)
        case .trackingWriteFailed(let noSpace): .trackingWriteFailed(noSpace: noSpace)
        case .settings(let settings): .settings(settings: settings.raw)
        case .network(let reachable): .network(reachable: reachable)
        case .foreground: .foreground
        case .background(let deadline): .background(deadlineMs: deadline)
        case .backgroundExpiring: .backgroundExpiring
        case .quitRequested: .quitRequested
        case .flushTimerFired: .flushTimerFired
        case .syncNowRequested(let source): .syncNowRequested(source: source.raw)
        case .syncNowCancelled(let exchange): .syncNowCancelled(exchange: exchange)
        case .volumeSpaceAvailable: .volumeSpaceAvailable
        case .visibleMedia(let names): .visibleMedia(names: names)
        case .operation(let instance, let operation, let phase): .operation(instance: instance, operation: operation.raw, phase: phase.raw)
        case .operationError(let instance, let operation, let error, let code, let retry):
            .operationError(instance: instance, operation: operation.raw, class: error.raw, code: code, retryAfterMs: retry)
        }
    }
}
public struct CloudPolicyMediaFacts: Sendable, Equatable {
    public let uploadFiles: UInt64?
    public let uploadBytes: UInt64?
    public let downloadFiles: UInt64?
    public let downloadBytes: UInt64?
    public let unknownBlobs: UInt64?
    public let missingNames: UInt64?
    public let uploadsRunning: UInt64?
    public let downloadsRunning: UInt64?
    public let remainingDownloadMs: UInt64?
    public let rootAcked: Bool?
    public let zoneMissing: Bool?
    public let noSpace: Bool?
    init(_ v: MeguriCoreBindings.CloudPolicyMediaFacts) {
        uploadFiles = v.uploadFiles
        uploadBytes = v.uploadBytes
        downloadFiles = v.downloadFiles
        downloadBytes = v.downloadBytes
        unknownBlobs = v.unknownBlobs
        missingNames = v.missingNames
        uploadsRunning = v.uploadsRunning
        downloadsRunning = v.downloadsRunning
        remainingDownloadMs = v.remainingDownloadMs
        rootAcked = v.rootAcked
        zoneMissing = v.zoneMissing
        noSpace = v.noSpace
    }
}

public enum CloudPolicyExchangeOutcome: Sendable {
    case succeeded(selected: UInt64, heldBack: UInt64, newerPending: UInt64, incomingWaiting: UInt64, media: CloudPolicyMediaFacts)
    case incomplete(outstanding: UInt64, blocked: UInt64)
    case waiting(CloudPolicyOperationError?, retryAfterMilliseconds: Int64?)
    case failed(CloudPolicyOperationError)
    case cancelled
    init(_ v: MeguriCoreBindings.CloudPolicyExchangeOutcome) {
        self = switch v {
        case .succeeded(let selected, let held, let newer, let incoming, let media): .succeeded(selected: selected, heldBack: held, newerPending: newer, incomingWaiting: incoming, media: .init(media))
        case .incomplete(let outstanding, let blocked): .incomplete(outstanding: outstanding, blocked: blocked)
        case .waiting(let error, let retry): .waiting(error.map(CloudPolicyOperationError.init), retryAfterMilliseconds: retry)
        case .failed(let error): .failed(.init(error))
        case .cancelled: .cancelled
        }
    }
}
public enum CloudPolicyEarlyEnd: Sendable {
    case waiting(CloudPolicyOperationError?, retryAfterMilliseconds: Int64?)
    case failed(CloudPolicyOperationError)
    case cancelled
    init(_ raw: MeguriCoreBindings.CloudPolicyEarlyEnd) {
        switch raw {
        case .waiting(let kind, let retry): self = .waiting(kind.map(CloudPolicyOperationError.init), retryAfterMilliseconds: retry)
        case .failed(let kind): self = .failed(.init(kind))
        case .cancelled: self = .cancelled
        }
    }
    var raw: MeguriCoreBindings.CloudPolicyEarlyEnd {
        switch self {
        case .waiting(let error, let retry): .waiting(class: error?.raw, retryAfterMs: retry)
        case .failed(let error): .failed(class: error.raw)
        case .cancelled: .cancelled
        }
    }
}
public struct CloudPolicyExchangeRecord: Sendable {
    public let exchange: UInt64
    public let endedMilliseconds: Int64
    public let source: CloudPolicySource
    public let outcome: CloudPolicyExchangeOutcome
    init(_ v: MeguriCoreBindings.CloudPolicyExchangeRecord) {
        exchange = v.exchange; endedMilliseconds = v.endedMs; source = .init(v.source); outcome = .init(v.outcome)
    }
}
public enum CloudPolicyStatusState: Sendable, Equatable {
    case localOnly, iCloudUnavailable(CloudPolicyUnavailableReason), paused(CloudPolicyPausedReason)
    case needsAttention, offline, syncingCollection, syncingMedia, changesPending
    case upToDate(asOfMilliseconds: Int64), awaitingEvidence
    init(_ v: MeguriCoreBindings.CloudPolicyStatusState) {
        self = switch v {
        case .localOnly: .localOnly
        case .iCloudUnavailable(let reason): .iCloudUnavailable(.init(reason))
        case .paused(let reason): .paused(.init(reason))
        case .needsAttention: .needsAttention
        case .offline: .offline
        case .syncingCollection: .syncingCollection
        case .syncingMedia: .syncingMedia
        case .changesPending: .changesPending
        case .upToDate(let at): .upToDate(asOfMilliseconds: at)
        case .awaitingEvidence: .awaitingEvidence
        }
    }
}
public enum CloudPolicySyncNowAvailability: Sendable, Equatable {
    case available, unavailable(CloudPolicyUnavailable)
    init(_ v: MeguriCoreBindings.CloudPolicySyncNowAvailability) {
        self = switch v { case .available: .available; case .unavailable(let reason): .unavailable(.init(reason)) }
    }
}
public struct CloudPolicyErrorDetail: Sendable {
    public let error: CloudPolicyOperationError
    public let code: Int64
    public let retryAfterMilliseconds: Int64?
    public let operation: CloudPolicyOperation
    init(_ v: MeguriCoreBindings.CloudPolicyErrorDetail) {
        error = .init(v.`class`); code = v.code; retryAfterMilliseconds = v.retryAfterMs; operation = .init(v.operation)
    }
}
public enum CloudPolicyEnableControl: Sendable, Equatable {
    case resumePause, decisionRequired, connect, pause, awaitingResumeOffer
    init(_ value: MeguriCoreBindings.CloudPolicyEnableControl) {
        switch value {
        case .resumePause: self = .resumePause
        case .decisionRequired: self = .decisionRequired
        case .connect: self = .connect
        case .pause: self = .pause
        case .awaitingResumeOffer: self = .awaitingResumeOffer
        }
    }
}
/// Read-only Core facts. Nil means the provider has supplied no answer.
public struct CloudPolicyControls: Sendable {
    public let enable: CloudPolicyEnableControl?
    public let bindingUsable: Bool?
    public let bindingActive: Bool?
    public let bindingStopped: Bool?
    public let decisionRequired: Bool?
    public let userPaused: Bool?
    public let resumeBindingOffered: Bool?
    public let automaticEligible: Bool?
    public static let unknown = Self()
    public init(enable: CloudPolicyEnableControl? = nil, bindingUsable: Bool? = nil,
                bindingActive: Bool? = nil, bindingStopped: Bool? = nil, decisionRequired: Bool? = nil,
                userPaused: Bool? = nil, resumeBindingOffered: Bool? = nil, automaticEligible: Bool? = nil) {
        self.enable = enable; self.bindingUsable = bindingUsable; self.bindingActive = bindingActive
        self.bindingStopped = bindingStopped; self.decisionRequired = decisionRequired; self.userPaused = userPaused
        self.resumeBindingOffered = resumeBindingOffered; self.automaticEligible = automaticEligible
    }
    init(_ value: MeguriCoreBindings.CloudPolicyControls) {
        self.init(enable: .init(value.enable), bindingUsable: value.bindingUsable, bindingActive: value.bindingActive,
            bindingStopped: value.bindingStopped, decisionRequired: value.decisionRequired, userPaused: value.userPaused,
            resumeBindingOffered: value.resumeBindingOffered, automaticEligible: value.automaticEligible)
    }
}
public struct CloudPolicyStatus: Sendable {
    public let state: CloudPolicyStatusState
    /// P25.4: only in `needsAttention`, the state the device would show with no attention item.
    public let stateWithoutAttention: CloudPolicyStatusState?
    public let pending: UInt64
    public let heldBack: UInt64
    public let inFlight: UInt64
    public let incomingWaiting: UInt64
    public let attention: [CloudPolicyAttention: UInt64]
    public let media: CloudPolicyMediaFacts
    public let mediaPaused: Bool
    public let error: CloudPolicyErrorDetail?
    public let accountStatus: CloudPolicyAccountStatus
    public let accountMatchesBinding: Bool
    /// Durable qualified evidence only. An unknown stamp remains nil.
    public let asOfMilliseconds: Int64?
    public let lastCompleted: CloudPolicyExchangeRecord?
    public let lastAttempt: CloudPolicyExchangeRecord?
    public let syncNow: CloudPolicySyncNowAvailability
    public private(set) var controls: CloudPolicyControls = .unknown
    init(_ v: MeguriCoreBindings.CloudPolicyStatus) {
        controls = .init(v.controls)
        state = .init(v.state); stateWithoutAttention = v.stateWithoutAttention.map(CloudPolicyStatusState.init)
        pending = v.pending; heldBack = v.heldBack; inFlight = v.inFlight; incomingWaiting = v.incomingWaiting
        attention = Dictionary(uniqueKeysWithValues: v.attention.map { (CloudPolicyAttention($0.kind), $0.count) })
        media = .init(v.media); mediaPaused = v.mediaPaused; error = v.error.map(CloudPolicyErrorDetail.init)
        accountStatus = .init(v.accountStatus); accountMatchesBinding = v.accountMatchesBinding
        asOfMilliseconds = v.asOfMs; lastCompleted = v.lastCompleted.map(CloudPolicyExchangeRecord.init)
        lastAttempt = v.lastAttempt.map(CloudPolicyExchangeRecord.init); syncNow = .init(v.syncNow)
    }
}
public enum CloudPolicyAction: Sendable {
    case startEngine(automatic: Bool), stopEngine(CloudPolicyStopReason), configureAutomatic(on: Bool)
    case updatePending(CloudPolicyUpdate), requestFetch(zones: [CloudPolicyZone]), requestSend(zones: [CloudPolicyZone])
    case readBlobs(includeMissing: Bool), retryAssetEntries, runCleanup, deleteZones
    case armFlushTimer(atMilliseconds: Int64), cancelFlushTimer
    case runExchange(exchange: UInt64, source: CloudPolicySource), cancelExchange(exchange: UInt64)
    case backgroundFlush(untilMilliseconds: Int64), cancelBackgroundFlush, quitFlush(untilMilliseconds: Int64), quitNow
    case pauseMedia, resumeMedia, prioritizeMedia(names: [String]), applyReady(retryFailed: Bool)
    case recordExchange(CloudPolicyExchangeRecord), recordQuietPoint(atMilliseconds: Int64), probeTracking
    case statusChanged(CloudPolicyStatus)
    init(_ v: MeguriCoreBindings.CloudPolicyAction) {
        self = switch v {
        case .startEngine(let automatic): .startEngine(automatic: automatic)
        case .stopEngine(let reason): .stopEngine(.init(reason))
        case .configureAutomatic(let on): .configureAutomatic(on: on)
        case .updatePending(let update): .updatePending(.init(update))
        case .requestFetch(let zones): .requestFetch(zones: zones.map(CloudPolicyZone.init))
        case .requestSend(let zones): .requestSend(zones: zones.map(CloudPolicyZone.init))
        case .readBlobs(let missing): .readBlobs(includeMissing: missing)
        case .retryAssetEntries: .retryAssetEntries
        case .runCleanup: .runCleanup
        case .deleteZones: .deleteZones
        case .armFlushTimer(let at): .armFlushTimer(atMilliseconds: at)
        case .cancelFlushTimer: .cancelFlushTimer
        case .runExchange(let exchange, let source): .runExchange(exchange: exchange, source: .init(source))
        case .cancelExchange(let exchange): .cancelExchange(exchange: exchange)
        case .backgroundFlush(let until): .backgroundFlush(untilMilliseconds: until)
        case .cancelBackgroundFlush: .cancelBackgroundFlush
        case .quitFlush(let until): .quitFlush(untilMilliseconds: until)
        case .quitNow: .quitNow
        case .pauseMedia: .pauseMedia
        case .resumeMedia: .resumeMedia
        case .prioritizeMedia(let names): .prioritizeMedia(names: names)
        case .applyReady(let retry): .applyReady(retryFailed: retry)
        case .recordExchange(let record): .recordExchange(.init(record))
        case .recordQuietPoint(let at): .recordQuietPoint(atMilliseconds: at)
        case .probeTracking: .probeTracking
        case .statusChanged(let status): .statusChanged(.init(status))
        }
    }
}
// Only a successful original account projection can attach this receipt to
// its own stop/cancel commands. It is provenance, never execution authority.
struct CloudPolicyAccountRetirement: Sendable {
    let status: CloudPolicyStatus
    fileprivate init(_ status: CloudPolicyStatus) { self.status = status }
}
public struct CloudPolicyCommand: Sendable {
    let raw: MeguriCoreBindings.CloudPolicyCommand
    public let action: CloudPolicyAction
    let accountRetirement: CloudPolicyAccountRetirement?
    init(_ v: MeguriCoreBindings.CloudPolicyIssuedCommand) { self.init(v, accountRetirement: nil) }
    fileprivate init(_ v: MeguriCoreBindings.CloudPolicyIssuedCommand, accountRetirement: CloudPolicyAccountRetirement?) {
        raw = v.handle; action = .init(v.action); self.accountRetirement = accountRetirement
    }
}
/// Follow-up commands on failure are required work, including StopEngine.
/// Consumers handle the output before selecting or displaying an error.
public enum CloudPolicyOutput: Sendable {
    case commands([CloudPolicyCommand])
    case coreActionFailed(error: BridgeError, commands: [CloudPolicyCommand])
    case applyDeferred(error: BridgeError)
    case syncNowUnavailable(CloudPolicyUnavailable)
    init(_ v: MeguriCoreBindings.CloudPolicyOutput) {
        self = switch v {
        case .commands(let commands): .commands(commands.map(CloudPolicyCommand.init))
        case .coreActionFailed(let error, let commands): .coreActionFailed(error: BridgeError(error), commands: commands.map(CloudPolicyCommand.init))
        case .applyDeferred(let error): .applyDeferred(error: BridgeError(error))
        case .syncNowUnavailable(let reason): .syncNowUnavailable(.init(reason))
        }
    }
    // Inspect only the status returned WITH this successfully projected input.
    // A later status lookup or an arbitrary StopEngine cannot supply this fact.
    static func accountProjection(_ value: MeguriCoreBindings.CloudPolicyOutput) -> Self {
        guard case .commands(let commands) = value,
              let status = commands.compactMap({ issued -> CloudPolicyStatus? in
                  if case .statusChanged(let status) = issued.action { return .init(status) }
                  return nil
              }).last,
              status.accountStatus == .noAccount || (status.accountStatus == .available && !status.accountMatchesBinding),
              commands.contains(where: { if case .stopEngine(.gateClosed) = $0.action { true } else { false } })
        else { return .init(value) }
        return .commands(commands.map { issued in
            let receipt: CloudPolicyAccountRetirement?
            if case .stopEngine(.gateClosed) = issued.action { receipt = .init(status) }
            else if case .cancelExchange = issued.action { receipt = .init(status) }
            else { receipt = nil }
            return CloudPolicyCommand(issued, accountRetirement: receipt)
        })
    }
}
public struct CloudPolicyCapture: Sendable {
    public let runtimeExchange: UInt64?
    public let output: CloudPolicyOutput
    init(_ v: MeguriCoreBindings.CloudPolicyCapture) { runtimeExchange = v.runtimeExchange; output = .init(v.output) }
}

public enum CloudPolicyAdmission: Sendable {
    /// Queue admission alone, never completion or an exchange result.
    case admitted(accepted: Bool)
    /// The setting was retained with no engine to configure; no work was queued.
    case noOperation
    case policyOutput(CloudPolicyOutput)
    init(_ v: MeguriCoreBindings.CloudPolicyAdmission) {
        self = switch v {
        case .admitted(let accepted): .admitted(accepted: accepted)
        case .noOperation: .noOperation
        case .policyOutput(let output): .policyOutput(.init(output))
        }
    }
}
