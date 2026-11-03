#if DEBUG
import Foundation
import OSLog
import Synchronization

/// Content-free native failure evidence. No error or action payload reaches Logger.
public enum DebugCloudDiagnostics {
    public enum Boundary: String, Sendable {
        case policyExecute, policyExchange, ownWork, connectionPreview
        case commandResult, policyStatus, coreActionFailed, applyDeferred
    }

    private static let logger = Logger(subsystem: "app.meguri", category: "CloudDiagnostics")
    private static let recorder = Recorder()

    public static var latestValue: String? { recorder.latestValue }

    public static func record(_ boundary: Boundary, action: CloudPolicyAction? = nil, error: any Error) {
        let value = Value(boundary, action: action, error: error)
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

    /// The only value passed to the log sink. Construction discards all payloads.
    struct Value: Sendable, Equatable {
        let boundary: Boundary
        let action: String
        let failure: String
        let code: Int32?
        let zone: String?
        let kind: String?

        init(_ boundary: Boundary, action: CloudPolicyAction? = nil, error: any Error) {
            self.boundary = boundary
            switch boundary {
            case .ownWork: self.action = "ownWork"
            case .commandResult, .policyStatus, .coreActionFailed, .applyDeferred: self.action = "none"
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
            boundary = .policyStatus
            action = "none"
            self.failure = failure
            self.code = code
            self.zone = zone
            self.kind = kind
        }

        var formatted: String {
            var tags = "boundary=\(boundary.rawValue) action=\(action) failure=\(failure)"
            if let zone, let kind { tags += " zone=\(zone) kind=\(kind)" }
            if let code { return "\(tags) code=\(code)" }
            return tags
        }
    }

    /// A fresh instance lets tests check the same cap without consuming process logs.
    final class Recorder: Sendable {
        private let state = Mutex<(count: Int, latest: String?, lastStatus: Value?)>((0, nil, nil))

        var latestValue: String? { state.withLock { $0.latest } }

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
            case .unavailable: return "bridge.unavailable"
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
