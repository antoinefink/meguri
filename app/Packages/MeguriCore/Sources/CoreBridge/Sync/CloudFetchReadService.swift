import Dispatch
import Foundation
import Instrumentation
internal import MeguriCoreBindings

enum CloudFetchReadNativeFailure: Sendable {
    case malformedRecord
    var raw: MeguriCoreBindings.CloudFetchReadNativeFailure {
        switch self { case .malformedRecord: .malformedRecord }
    }
}

struct CoreCloudFetchRead: Sendable {
    private let raw: MeguriCoreBindings.CloudFetchRead
    init(_ raw: MeguriCoreBindings.CloudFetchRead) { self.raw = raw }
    func preparationOutcome(error: (any Error)? = nil) async throws -> CloudPolicyEarlyEnd? {
        let raw = self.raw
        let failure = error as? CloudKitSDKOperationFailure
        let stale: Bool
        if let bridge = error as? BridgeError {
            switch bridge {
            case .cloudConnection(.authorityChanged), .cloudConnection(.fileIdentityUnavailable): stale = true
            default: stale = false
            }
        } else { stale = false }
        return try await MaintenanceLane.shared.call(.cloudFetchReadEpochPreparationOutcome) {
            try raw.epochPreparationOutcome(code: failure?.code, retryAfterMs: failure?.retryAfterMilliseconds,
                cancelled: stale || cloudOperationWasCancelled(error), failed: error != nil).map(CloudPolicyEarlyEnd.init)
        }
    }
    func target() async throws -> CloudControlTarget {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudFetchReadTarget) { CloudControlTarget(try raw.target()) }
    }
    func cancel() {
        let raw = self.raw
        // Atomic-only cancellation uses the independent control lane, so a
        // provider/tracking call on maintenance cannot delay closing admission.
        let queue = ControlLane.shared.queue
        dispatchPrecondition(condition: .notOnQueue(queue))
        try? queue.sync { try Signposts.bridge(.cloudFetchReadCancel, lane: Lane.control.rawValue) { try raw.cancel() } }
    }
    func finalAuthorization() -> @Sendable () throws -> Void {
        let raw = self.raw
        return {
            try cloudFinalCoreCall(.cloudFetchReadValidate) {
                #if DEBUG
                CloudFetchReadWorkObservation.beforeValidation?()
                #endif
                try raw.validate()
            }
            try cloudFinalCoreCall(.cloudFetchReadValidateEnqueue) { try raw.validateEnqueue() }
        }
    }
    func complete(_ result: CloudControlRead, discard: Bool = false,
                  evidence: CloudZoneDeletionRead? = nil,
                  nativeFailure: CloudFetchReadNativeFailure? = nil) async throws -> CoreCloudFetchRead? {
        let raw = self.raw
        return try await withTaskCancellationHandler {
            try await MaintenanceLane.shared.call(.cloudFetchReadComplete) {
                #if DEBUG
                CloudFetchReadWorkObservation.beforeCompletion?()
                #endif
                return try raw.complete(result: result.raw, discard: discard, failure: evidence?.failure?.raw,
                                        terminal: evidence?.terminal?.raw, nativeFailure: nativeFailure?.raw).map(CoreCloudFetchRead.init)
            }
        } onCancel: { cancel() }
    }
}
struct CloudFetchReadPlan: Sendable {
    let qualified: Bool
    let reads: [CoreCloudFetchRead]
}

/// A result before capture, retaining the original error for diagnostics.
struct CloudEpochPreparationError: Error {
    let outcome: CloudPolicyEarlyEnd
    let original: (any Error)?
}

/// The direct facade retains the original P12.1 admission just as policy retains RunExchange.
struct CoreCloudSyncNowRequest: Sendable {
    let raw: MeguriCoreBindings.CloudSyncNowRequest
    func epochRead() async throws -> CoreCloudFetchRead? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudSyncNowRequestEpochRead) {
            try raw.epochRead().map(CoreCloudFetchRead.init)
        }
    }
    func beginExchange() async throws -> UInt64 {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudSyncNowRequestBeginExchange) { try raw.beginExchange() }
    }
}

#if DEBUG
/// Scheduling observation only; the real Core work and outcomes are unchanged.
enum CloudFetchReadWorkObservation {
    @TaskLocal static var beforeValidation: (@Sendable () -> Void)?
    @TaskLocal static var beforeCompletion: (@Sendable () -> Void)?
}
#endif
