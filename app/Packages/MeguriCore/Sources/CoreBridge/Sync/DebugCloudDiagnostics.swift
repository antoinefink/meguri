#if DEBUG
import Foundation
import OSLog
import Synchronization

/// Content-free native failure evidence. No error or action payload reaches Logger.
public enum DebugCloudDiagnostics {
    public enum Boundary: String, Sendable {
        case policyExecute, policyExchange, ownWork, connectionPreview
        case commandResult, policyStatus, coreActionFailed, applyDeferred
        case accountCompletionRefused, accountResultCurrent, accountResultSuperseded
        case accountReaderClosed, accountReaderFrozen, accountReaderTaskCancelled, accountReaderInvalidated
        case accountProjectionLifecycle, accountProjectionStatus, accountProjectionPrepare, accountProjectionRefresh
        case platformBootstrap, backendAttachment, retainedRecoveryFailure, connectionFailure
        case ownWorkTrigger, connectionOperationCancel, suspendFreeze, beginAccountRead
        case policyInput, backendReplacement, closeDrain
    }

    /// Deferred source evidence; classification retains neither the error nor its payload.
    public struct ProjectedFailure: Sendable {
        let value: Value

        public init(boundary: Boundary, error: any Error) {
            value = Value(boundary, error: error)
        }
    }

    /// One command-scoped observation, inherited by its existing child tasks.
    /// Enqueue callbacks explicitly capture it because SDK admission is detached.
    @TaskLocal public static var sendFailureCapture: SendFailureCapture?
    // Retain collection/media context when a deeper send catch wins the latch.
    @TaskLocal static var mediaSendPhase: SendFailureSite?

    enum SendFailureSite: String, Sendable {
        case executeAdmission, executeCore, prepareExecution, requestExecution, policyRefresh
        case originalSDK, sendAdmission, pendingUpdateApplication, beginSend, admissionValidation
        case sendValidation, sdkAuthorization, sdkPreparation, enqueue, invocationReport
        case enqueueSDKAuthorization, enqueueExecutionValidation, drain, postDrainValidation
        case mediaPreparation, mediaOriginalSDK, mediaQuiescing, mediaPreNetwork, mediaWorker
        case mediaBegin, mediaValidation, mediaNextWork, mediaDrainWait, mediaReadReply
        case mediaSendWorkflow, mediaCollectionSend, mediaSend, mediaContinueAfterSend
        case mediaReportBegin, mediaReportEnd, mediaRequiredRefresh, mediaCancellationCheck
        case completion, deferredUpdates, finish
        case exchangeReadiness, exchangeGate, exchangePreparation, exchangeReservation
        case exchangeCapture, exchangeRun, exchangeCompletion, exchangeEndBeforeCapture, heldConfiguration
        case exchangeFetch, exchangeFetched, exchangeSend, exchangeSendDue, nativeExchangeCompletion
    }

    public final class SendFailureCapture: Sendable {
        private struct Value: Sendable {
            let site: SendFailureSite
            let category: String
            let mediaSendPhase: SendFailureSite?
            var formatted: String {
                let phase = mediaSendPhase.map { " mediaSendPhase=\($0.rawValue)" } ?? ""
                return "firstSite=\(site.rawValue) firstCategory=\(category)\(phase)"
            }
        }
        private let first = Mutex<Value?>(nil)
        public init() {}
        public var firstValue: String? { first.withLock { $0?.formatted } }
        // Classify immediately; retain neither the error nor any associated payload.
        func record(_ site: SendFailureSite, error: any Error,
                    mediaSendPhase: SendFailureSite? = DebugCloudDiagnostics.mediaSendPhase) {
            let value = Value(site: site, category: DebugCloudDiagnostics.failureTag(error), mediaSendPhase: mediaSendPhase)
            first.withLock { if $0 == nil { $0 = value } }
        }
    }

    /// First local snapshot at the original fetch-read identity-refresh catch.
    /// The command's task-local scope supplies pairing; only Boolean facts survive.
    @TaskLocal public static var fetchReadFailureCapture: FetchReadFailureCapture?

    public final class FetchReadFailureCapture: Sendable {
        private struct Value: Sendable {
            let targetActive, transportError, resultFailed, evidenceFailure, evidenceTerminal: Bool
            let taskCancelled, stopping, policyStop, retirementFailure, accountRetired, cancellationError: Bool
            var formatted: String {
                "fetchReadRefresh{targetActive=\(targetActive ? 1 : 0) transportError=\(transportError ? 1 : 0) resultFailed=\(resultFailed ? 1 : 0) evidenceFailure=\(evidenceFailure ? 1 : 0) evidenceTerminal=\(evidenceTerminal ? 1 : 0) taskCancelled=\(taskCancelled ? 1 : 0) stopping=\(stopping ? 1 : 0) policyStop=\(policyStop ? 1 : 0) retirementFailure=\(retirementFailure ? 1 : 0) accountRetired=\(accountRetired ? 1 : 0) cancellationError=\(cancellationError ? 1 : 0)}"
            }
        }
        private let first = Mutex<Value?>(nil)
        public init() {}
        public var firstValue: String? { first.withLock { $0?.formatted } }
        func record(targetActive: Bool, transportError: Bool, resultFailed: Bool,
                    evidenceFailure: Bool, evidenceTerminal: Bool, taskCancelled: Bool,
                    stopping: Bool, policyStop: Bool, retirementFailure: Bool,
                    accountRetired: Bool, cancellationError: Bool) {
            let value = Value(targetActive: targetActive, transportError: transportError, resultFailed: resultFailed,
                evidenceFailure: evidenceFailure, evidenceTerminal: evidenceTerminal, taskCancelled: taskCancelled,
                stopping: stopping, policyStop: policyStop, retirementFailure: retirementFailure,
                accountRetired: accountRetired, cancellationError: cancellationError)
            first.withLock { if $0 == nil { $0 = value } }
        }
    }

    private static let logger = Logger(subsystem: "app.meguri", category: "CloudDiagnostics")
    private static let recorder = Recorder()
    private static let policyStatusTrace = PolicyStatusTrace()
    static let fetchRecorder = FetchRecorder()

    /// All original callbacks in this PROCESS, across owners/instances. These
    /// totals prove neither current-owner delivery nor convergence or cause.
    public static var processFetchValue: String { "\(fetchRecorder.formatted) \(fetchRecorder.sentFormatted)" }

    enum FetchZone: Equatable, Sendable { case collection, root, other }
    enum FetchCounter: Int, CaseIterable, Sendable {
        case will, zoneC, modC, modR, coreC, refused, zoneFail
        var label: String {
            switch self {
            case .will: "will"
            case .zoneC: "zoneC"
            case .modC: "modC"
            case .modR: "modR"
            case .coreC: "coreC"
            case .refused: "refused"
            case .zoneFail: "zoneFail"
            }
        }
    }

    enum SentCounter: Int, CaseIterable, Sendable {
        case sentC, sentM, sendFailC, sendFailM
        var label: String {
            switch self {
            case .sentC: "sentC"
            case .sentM: "sentM"
            case .sendFailC: "sendFailC"
            case .sendFailM: "sendFailM"
            }
        }
    }

    /// Only fixed scalar counts, Boolean observations and an overflow flag survive.
    /// will/zoneC count native starts; modC/modR count original modifications
    /// before conversion. coreC counts records after successful core intake.
    /// refused counts native fetched callbacks reaching the existing catch;
    /// zoneFail counts original failed zone results (all zones). No new sink.
    /// inC/outC OR-retain whether an original SDK options callback requested/
    /// returned a scope containing its registered collection zone. They may
    /// come from different callbacks/owners; neither pairs causes nor proves
    /// current-owner authority. No zone identity or options are retained.
    /// cut OR-retains whether any original callback requested but did not offer
    /// its registered collection zone; it identifies no completed exchange or owner.
    /// db/dbC OR-retain original database callbacks/collection modifications;
    /// un/unC OR-retain nonempty/collection unfetched-server-change snapshots.
    /// These aggregate across instances and times, never causal pairs,
    /// completion, current-owner evidence or convergence.
    /// processSent counts only original saved records and failed saves/deletes
    /// in the registered collection or current-user media zone. It is a separate
    /// bounded block so the existing processAll fields and size limit survive.
    /// These process-wide totals are not selected-leg or signed acceptance evidence.
    final class FetchRecorder: Sendable {
        private struct State: Sendable {
            var counts: [UInt64]
            var sentCounts: [UInt64]
            var sentOverflow = false
            var requestedCollection = false
            var offeredCollection = false
            var scopeCut = false
            var database = false
            var databaseCollection = false
            var unfetched = false
            var unfetchedCollection = false
            var overflow = false
        }
        private let state: Mutex<State>
        // Scalar seed allows overflow testing without changing the process recorder.
        init(initialCount: UInt64 = 0) {
            state = Mutex(State(counts: Array(repeating: initialCount, count: FetchCounter.allCases.count),
                sentCounts: Array(repeating: initialCount, count: SentCounter.allCases.count)))
        }
        func record(_ counter: FetchCounter, count: UInt64 = 1) {
            state.withLock { state in
                let sum = state.counts[counter.rawValue].addingReportingOverflow(count)
                state.counts[counter.rawValue] = sum.overflow ? .max : sum.partialValue
                state.overflow = state.overflow || sum.overflow
            }
        }
        func recordSent(_ counter: SentCounter, count: UInt64 = 1) {
            state.withLock { state in
                let sum = state.sentCounts[counter.rawValue].addingReportingOverflow(count)
                state.sentCounts[counter.rawValue] = sum.overflow ? .max : sum.partialValue
                state.sentOverflow = state.sentOverflow || sum.overflow
            }
        }
        var sentFormatted: String {
            state.withLock { state in
                let counts = SentCounter.allCases.map { "\($0.label)=\(state.sentCounts[$0.rawValue])" }
                    .joined(separator: " ")
                return "processSent{\(counts) overflow=\(state.sentOverflow ? 1 : 0)}"
            }
        }
        func recordFetchOptions(requestedCollection: Bool, offeredCollection: Bool) {
            state.withLock { state in
                state.requestedCollection = state.requestedCollection || requestedCollection
                state.offeredCollection = state.offeredCollection || offeredCollection
                state.scopeCut = state.scopeCut || (requestedCollection && !offeredCollection)
            }
        }
        func recordServerChangeObservations(database: Bool = false, databaseCollection: Bool = false,
                                            unfetched: Bool = false, unfetchedCollection: Bool = false) {
            state.withLock { state in
                state.database = state.database || database
                state.databaseCollection = state.databaseCollection || databaseCollection
                state.unfetched = state.unfetched || unfetched
                state.unfetchedCollection = state.unfetchedCollection || unfetchedCollection
            }
        }
        var formatted: String {
            state.withLock { state in
                let counts = FetchCounter.allCases.map { "\($0.label)=\(state.counts[$0.rawValue])" }
                    .joined(separator: " ")
                let serverChanges = "db=\(state.database ? 1 : 0) dbC=\(state.databaseCollection ? 1 : 0) un=\(state.unfetched ? 1 : 0) unC=\(state.unfetchedCollection ? 1 : 0)"
                return "processAll{\(counts) inC=\(state.requestedCollection ? 1 : 0) outC=\(state.offeredCollection ? 1 : 0) cut=\(state.scopeCut ? 1 : 0) \(serverChanges) overflow=\(state.overflow ? 1 : 0)}"
            }
        }
    }

    public static var latestValue: String? { recorder.latestValue }
    public static func policyStatusValue(generation: UInt64, hostRevision: UInt64) -> String? {
        policyStatusTrace.value(generation: generation, hostRevision: hostRevision)
    }

    public static func recordPolicyStatus(_ status: CloudPolicyStatus, generation: UInt64, hostRevision: UInt64) {
        policyStatusTrace.retain(status, generation: generation, hostRevision: hostRevision)
    }

    /// Core policy projections only: selected is not original SDK accounting or convergence evidence.
    /// Retain safe strings, never the status DTO or its associated payloads. No separate observation is needed.
    final class PolicyStatusTrace: Sendable {
        private struct State: Sendable {
            let generation: UInt64
            let hostRevision: UInt64
            let initial: String
            var firstCompleted: String?
            var latest: String
        }
        private let state = Mutex<State?>(nil)

        func value(generation: UInt64, hostRevision: UInt64) -> String? {
            state.withLock { state in
                guard let state, state.generation == generation, state.hostRevision == hostRevision else { return nil }
                return "policy{latest={\(state.latest)} initial={\(state.initial)} firstCompleted=\(state.firstCompleted ?? "nil")}"
            }
        }

        func retain(_ status: CloudPolicyStatus, generation: UInt64, hostRevision: UInt64) {
            let snapshot = Self.snapshot(status)
            let completed = status.lastCompleted.map { Self.exchange($0) }
            state.withLock { state in
                if state?.generation != generation || state?.hostRevision != hostRevision {
                    state = State(generation: generation, hostRevision: hostRevision,
                        initial: snapshot, firstCompleted: completed, latest: snapshot)
                } else {
                    if state?.firstCompleted == nil { state?.firstCompleted = completed }
                    state?.latest = snapshot
                }
            }
        }

        static func snapshot(_ status: CloudPolicyStatus) -> String {
            "pending=\(status.pending) heldBack=\(status.heldBack) inFlight=\(status.inFlight) incomingWaiting=\(status.incomingWaiting) bindingActive=\(flag(status.controls.bindingActive)) bindingUsable=\(flag(status.controls.bindingUsable)) accountMatchesBinding=\(flag(status.accountMatchesBinding)) lastCompleted=\(exchange(status.lastCompleted)) lastAttempt=\(exchange(status.lastAttempt))"
        }

        static func flag(_ value: Bool?) -> String {
            switch value { case .some(true): "true"; case .some(false): "false"; case nil: "unknown" }
        }

        static func exchange(_ record: CloudPolicyExchangeRecord?) -> String {
            guard let record else { return "nil" }
            let outcome: String
            switch record.outcome {
            case .succeeded(let selected, let heldBack, let newerPending, let incomingWaiting, _):
                outcome = "succeeded selected=\(selected) heldBack=\(heldBack) newerPending=\(newerPending) incomingWaiting=\(incomingWaiting)"
            case .incomplete(let outstanding, let blocked):
                outcome = "incomplete outstanding=\(outstanding) blocked=\(blocked)"
            case .waiting: outcome = "waiting"
            case .failed: outcome = "failed"
            case .cancelled: outcome = "cancelled"
            }
            return "{endedMilliseconds=\(record.endedMilliseconds) outcome=\(outcome)}"
        }
    }

    public static func record(_ boundary: Boundary, action: CloudPolicyAction? = nil, error: any Error) {
        let value = Value(boundary, action: action, error: error)
        recorder.emit(value) { value in
            logger.error("cloudFailure \(value.formatted, privacy: .public)")
        }
    }

    public static func record(_ failure: ProjectedFailure) {
        recorder.emit(failure.value) { value in
            logger.error("cloudFailure \(value.formatted, privacy: .public)")
        }
    }

    static func recordAdoptionFailure(_ diagnostic: CloudAdoptionFailureDiagnostic) {
        let value = Value.adoption(diagnostic)
        recorder.emit(value) { value in
            logger.error("cloudFailure \(value.formatted, privacy: .public)")
        }
    }

    public static func recordStatus(_ detail: CloudPolicyErrorDetail?) {
        let value = detail.map {
            Value.status($0.error, zone: $0.operation.zone, kind: $0.operation.kind, code: $0.code)
        }
        recorder.emitStatus(value) { value in
            logger.error("cloudFailure \(value.formatted, privacy: .public)")
        }
    }

    // Tests use a fresh recorder; native callbacks capture it at preparation.
    @TaskLocal static var mediaRecorder: Recorder?
    @TaskLocal static var mediaAssetStaging = false
    @TaskLocal static var mediaRequestKind: MediaRequestKind?
    static var currentMediaRecorder: Recorder { mediaRecorder ?? recorder }

    enum MediaRequestKind: String, Sendable {
        case next, root, read, reads, download, upload, delete
        init(_ request: CloudMediaRequest?) {
            switch request {
            case nil: self = .next
            case .some(.root): self = .root
            case .some(.read): self = .read
            case .some(.reads): self = .reads
            case .some(.download): self = .download
            case .some(.upload): self = .upload
            case .some(.delete): self = .delete
            }
        }
    }
    enum MediaStage: String, Sendable {
        case recordValidation, assetStaging, recordCallback, terminalCallback
        case prepare, coreNext, coreAdmission, transportReply, coreReply
    }
    enum MediaCategory: String, Sendable {
        case invalidRecord, missingAsset, duplicateCallback, missingCallback, invalidReply
        case cloudKit, nonCloudKit, cancelled, queueRejected, metadataScope, coreRejected
        case invalidAsset, assetTooLarge, noSpace, stagingLimit, other
        case sourceUnavailable, sourceShortRead, sourceShape, sourceSize
        static func local(_ error: any Error) -> Self {
            if error is CancellationError { return .cancelled }
            if let error = error as? CloudMediaSourceFailure {
                switch error {
                case .unavailable: return .sourceUnavailable
                case .shortRead: return .sourceShortRead
                case .invalidShape: return .sourceShape
                case .invalidSize: return .sourceSize
                }
            }
            if error is BridgeError { return .coreRejected }
            guard let error = error as? CloudMediaFailure else { return .other }
            switch error {
            case .queueRejected: return .queueRejected
            case .metadataScope: return .metadataScope
            case .invalidAsset: return .invalidAsset
            case .assetTooLarge: return .assetTooLarge
            case .noSpace: return .noSpace
            case .stagingLimit: return .stagingLimit
            case .missingReply: return .missingCallback
            default: return .other
            }
        }
    }
    struct MediaValue: Sendable, Equatable {
        let request: MediaRequestKind
        let stage: MediaStage
        let category: MediaCategory
        var formatted: String {
            "mediaRequest=\(request.rawValue) mediaStage=\(stage.rawValue) mediaCategory=\(category.rawValue)"
        }
    }
    static func recordMedia(_ request: CloudMediaRequest? = nil, stage: MediaStage, category: MediaCategory) {
        currentMediaRecorder.retainMedia(.init(request: mediaRequestKind ?? .init(request), stage: stage, category: category))
    }

    /// The only value passed to the log sink. Construction discards all payloads.
    struct Value: Sendable, Equatable {
        let boundary: Boundary
        let action: String
        let failure: String
        let code: Int32?
        let zone: String?
        let kind: String?
        private let adoption: CloudAdoptionFailureDiagnostic?

        init(_ boundary: Boundary, action: CloudPolicyAction? = nil, error: any Error) {
            self.boundary = boundary
            adoption = nil
            switch boundary {
            case .ownWork: self.action = "ownWork"
            case .commandResult, .policyStatus, .coreActionFailed, .applyDeferred,
                 .accountCompletionRefused, .accountResultCurrent, .accountResultSuperseded,
                 .accountReaderClosed, .accountReaderFrozen, .accountReaderTaskCancelled,
                 .accountReaderInvalidated, .accountProjectionLifecycle, .accountProjectionStatus,
                 .accountProjectionPrepare, .accountProjectionRefresh, .platformBootstrap, .backendAttachment,
                 .retainedRecoveryFailure, .connectionFailure, .ownWorkTrigger,
                 .connectionOperationCancel, .suspendFreeze, .beginAccountRead,
                 .policyInput, .backendReplacement, .closeDrain: self.action = "none"
            default: self.action = action.map(DebugCloudDiagnostics.actionTag) ?? "none"
            }
            failure = DebugCloudDiagnostics.failureTag(error)
            code = (error as? CloudKitSDKOperationFailure).flatMap { Int32(exactly: $0.code) }
            zone = nil
            kind = nil
        }

        /// Pure typed projection shared by status recording and focused tests.
        static func status(_ error: CloudPolicyOperationError, zone: CloudPolicyZone,
                           kind: CloudPolicyOperationKind, code: Int64) -> Self {
            Self(failure: DebugCloudDiagnostics.statusTag(error), code: Int32(exactly: code),
                 zone: DebugCloudDiagnostics.zoneTag(zone), kind: DebugCloudDiagnostics.kindTag(kind))
        }

        private init(failure: String, code: Int32?, zone: String, kind: String) {
            adoption = nil
            boundary = .policyStatus
            action = "none"
            self.failure = failure
            self.code = code
            self.zone = zone
            self.kind = kind
        }

        static func adoption(_ diagnostic: CloudAdoptionFailureDiagnostic) -> Self {
            Self(adoption: diagnostic)
        }

        private init(adoption: CloudAdoptionFailureDiagnostic) {
            boundary = .connectionFailure
            action = "adoptionResult"
            failure = "adoption"
            code = nil; zone = nil; kind = nil
            self.adoption = adoption
        }

        var formatted: String {
            if let adoption {
                return "boundary=connectionFailure action=adoptionResult task=\(adoption.task.rawValue) lastEnteredStage=\(adoption.lastEnteredStage.rawValue) primary=\(adoption.primaryCategory?.rawValue ?? "none") cleanup=\(adoption.cleanupCategory?.rawValue ?? "none")"
            }
            var tags = "boundary=\(boundary.rawValue) action=\(action) failure=\(failure)"
            if let zone, let kind { tags += " zone=\(zone) kind=\(kind)" }
            if let code { return "\(tags) code=\(code)" }
            return tags
        }
    }

    /// A fresh instance lets tests check the same cap without consuming process logs.
    final class Recorder: Sendable {
        private let state = Mutex<(count: Int, latest: String?, lastStatus: Value?, media: MediaValue?)>((0, nil, nil, nil))

        var latestValue: String? {
            state.withLock { state in
                let values = [state.latest, state.media?.formatted].compactMap { $0 }
                return values.isEmpty ? nil : values.joined(separator: " ")
            }
        }

        // One retained tuple, no history and no additional log sink/cap.
        func retainMedia(_ value: MediaValue) { state.withLock { $0.media = value } }

        func emit(_ value: Value, using sink: (Value) -> Void) {
            emit(value, status: false, using: sink)
        }

        func emitStatus(_ value: Value?, using sink: (Value) -> Void) {
            emit(value, status: true, using: sink)
        }

        private func emit(_ value: Value?, status: Bool, using sink: (Value) -> Void) {
            let admitted = state.withLock { state in
                guard let value else {
                    if status { state.lastStatus = nil }
                    return false
                }
                state.latest = value.formatted
                if status {
                    let duplicate = state.lastStatus == value
                    state.lastStatus = value
                    if duplicate { return false }
                }
                guard state.count < 64 else { return false }
                state.count += 1
                return true
            }
            if admitted, let value { sink(value) }
        }
    }

    private static func statusTag(_ error: CloudPolicyOperationError) -> String {
        switch error {
        case .noAccount: "noAccount"
        case .configuration: "configuration"
        case .offline: "offline"
        case .throttled: "throttled"
        case .quotaFull: "quotaFull"
        case .localStorageFull: "localStorageFull"
        case .trackingError: "trackingError"
        case .conflictRecovery: "conflictRecovery"
        case .unsupportedSchema: "unsupportedSchema"
        case .other: "other"
        }
    }

    private static func zoneTag(_ zone: CloudPolicyZone) -> String {
        switch zone {
        case .root: "root"
        case .collection: "collection"
        case .media: "media"
        }
    }

    private static func kindTag(_ kind: CloudPolicyOperationKind) -> String {
        switch kind {
        case .send: "send"
        case .fetch: "fetch"
        case .read: "read"
        }
    }

    private static func actionTag(_ action: CloudPolicyAction) -> String {
        switch action {
        case .startEngine: "startEngine"
        case .stopEngine: "stopEngine"
        case .configureAutomatic: "configureAutomatic"
        case .updatePending: "updatePending"
        case .requestFetch: "requestFetch"
        case .requestSend: "requestSend"
        case .readBlobs: "readBlobs"
        case .retryAssetEntries: "retryAssetEntries"
        case .runCleanup: "runCleanup"
        case .deleteZones: "deleteZones"
        case .armFlushTimer: "armFlushTimer"
        case .cancelFlushTimer: "cancelFlushTimer"
        case .runExchange: "runExchange"
        case .cancelExchange: "cancelExchange"
        case .backgroundFlush: "backgroundFlush"
        case .cancelBackgroundFlush: "cancelBackgroundFlush"
        case .quitFlush: "quitFlush"
        case .quitNow: "quitNow"
        case .pauseMedia: "pauseMedia"
        case .resumeMedia: "resumeMedia"
        case .prioritizeMedia: "prioritizeMedia"
        case .applyReady: "applyReady"
        case .recordExchange: "recordExchange"
        case .recordQuietPoint: "recordQuietPoint"
        case .probeTracking: "probeTracking"
        case .statusChanged: "statusChanged"
        }
    }

    private static func failureTag(_ error: any Error) -> String {
        if error is CancellationError { return "taskCancelled" }
        if let error = error as? BridgeError {
            switch error {
            case .cloudPolicy(let failure): return policyTag(failure)
            case .cloudConnection(let failure): return connectionTag(failure)
            case .cloudSDKCallbackRejected: return "bridge.cloudSDKCallbackRejected"
            case .maintenance: return "bridge.maintenance"
            case .exportSourceChanged: return "bridge.exportSourceChanged"
            case .exportSourceInvalid: return "bridge.exportSourceInvalid"
            case .exportFormatLimit: return "bridge.exportFormatLimit"
            case .deckOptionsInvalid: return "bridge.deckOptionsInvalid"
            case .stale: return "bridge.stale"
            case .staleGeneration: return "bridge.staleGeneration"
            case .undoUnavailable: return "bridge.undoUnavailable"
            case .notetypeRefused: return "bridge.notetypeRefused"
            case .deckRefused: return "bridge.deckRefused"
            case .noteNotFound: return "bridge.noteNotFound"
            case .notFound: return "bridge.notFound"
            case .insufficientSpace: return "bridge.insufficientSpace"
            case .open: return "bridge.open"
            case .suspendDeadline: return "bridge.suspendDeadline"
            case .schedOpRefused: return "bridge.schedOpRefused"
            case .sessionActive: return "bridge.sessionActive"
            case .invalidTypeAnswerReplacement: return "bridge.invalidTypeAnswerReplacement"
            case .cancelled: return "bridge.cancelled"
            case .busy: return "bridge.busy"
            case .unavailable(let reason):
                switch reason {
                case "Ordered cloud account admission unavailable": return "bridge.unavailable.orderedAccountAdmission"
                case "Cloud outputs require original acknowledgement": return "bridge.unavailable.originalAcknowledgementRequired"
                case "Cloud account proof unavailable": return "bridge.unavailable.accountProof"
                case "Fresh cloud account reader unavailable": return "bridge.unavailable.freshAccountReader"
                case "Cloud account read expired": return "bridge.unavailable.accountReadExpired"
                case "Cloud registration expired during creation": return "bridge.unavailable.registrationExpiredDuringCreation"
                default: return "bridge.unavailable"
                }
            case .unsupported: return "bridge.unsupported"
            case .invalidArgument: return "bridge.invalidArgument"
            case .internal: return "bridge.internal"
            case .mediaCommitPending: return "bridge.mediaCommitPending"
            case .commitCompleted: return "bridge.commitCompleted"
            }
        }
        if let failure = error as? CloudOwnTransportFailure {
            switch failure {
            case .closed: return "own.closed"
            case .busy: return "own.busy"
            case .queueRejected: return "own.queueRejected"
            case .missingReply: return "own.missingReply"
            case .unexpectedReply: return "own.unexpectedReply"
            case .invalidCursor: return "own.invalidCursor"
            case .privateDatabaseRequired: return "own.privateDatabaseRequired"
            }
        }
        if error is CloudKitSDKOperationFailure { return "sdkOperation" }
        return "other"
    }

    private static func policyTag(_ failure: CloudPolicyFailure) -> String {
        switch failure {
        case .closed: "policy.closed"
        case .wrongOwner: "policy.wrongOwner"
        case .generationChanged: "policy.generationChanged"
        case .staleCommand: "policy.staleCommand"
        case .notCoreAction: "policy.notCoreAction"
        case .notNativeAction: "policy.notNativeAction"
        case .coreCapabilityUnavailable: "policy.coreCapabilityUnavailable"
        case .unqualifiedCompletion: "policy.unqualifiedCompletion"
        }
    }

    private static func connectionTag(_ failure: CloudConnectionFailure) -> String {
        switch failure {
        case .closed: "connection.closed"
        case .wrongOwner: "connection.wrongOwner"
        case .consumed: "connection.consumed"
        case .authorityChanged: "connection.authorityChanged"
        case .accountUnavailable: "connection.accountUnavailable"
        case .accountMismatch: "connection.accountMismatch"
        case .busy: "connection.busy"
        case .keyUnavailable: "connection.keyUnavailable"
        case .epochIncomplete: "connection.epochIncomplete"
        case .unreadableControl: "connection.unreadableControl"
        case .notOffered: "connection.notOffered"
        case .incomingStagingUnavailable: "connection.incomingStagingUnavailable"
        case .mediaApplyUnavailable: "connection.mediaApplyUnavailable"
        case .resetRecoveryUnavailable: "connection.resetRecoveryUnavailable"
        case .tracking: "connection.tracking"
        case .incomplete: "connection.incomplete"
        case .fileIdentityUnavailable: "connection.fileIdentityUnavailable"
        }
    }
}
#endif
