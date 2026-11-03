#if DEBUG
import Foundation
import OSLog
import Synchronization

/// Content-free native failure evidence. No error or action payload reaches Logger.
public enum DebugCloudDiagnostics {
    public enum Boundary: String, Sendable {
        case policyExecute, policyExchange, ownWork, connectionPreview
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

    /// The only value passed to the log sink. Construction discards all payloads.
    struct Value: Sendable, Equatable {
        let boundary: Boundary
        let action: String
        let failure: String
        let code: Int32?

        init(_ boundary: Boundary, action: CloudPolicyAction? = nil, error: any Error) {
            self.boundary = boundary
            self.action = boundary == .ownWork ? "ownWork" : action.map(DebugCloudDiagnostics.actionTag) ?? "none"
            failure = DebugCloudDiagnostics.failureTag(error)
            code = (error as? CloudKitSDKOperationFailure).flatMap { Int32(exactly: $0.code) }
        }

        var formatted: String {
            let tags = "boundary=\(boundary.rawValue) action=\(action) failure=\(failure)"
            if let code { return "\(tags) code=\(code)" }
            return tags
        }
    }

    /// A fresh instance lets tests check the same cap without consuming process logs.
    final class Recorder: Sendable {
        private let state = Mutex<(count: Int, latest: String?)>((0, nil))

        var latestValue: String? { state.withLock { $0.latest } }

        func emit(_ value: Value, using sink: (Value) -> Void) {
            let admitted = state.withLock { state in
                state.latest = value.formatted
                guard state.count < 64 else { return false }
                state.count += 1
                return true
            }
            if admitted { sink(value) }
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
