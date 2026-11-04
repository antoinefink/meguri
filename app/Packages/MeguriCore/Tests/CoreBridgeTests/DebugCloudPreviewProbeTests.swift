#if DEBUG
import Foundation
import Observation
import Synchronization
#if canImport(CloudKit)
import CloudKit
#endif
import Testing
@testable import CoreBridge

struct DebugCloudPreviewProbeTests {
    @Test func transportAndTaskFailuresHaveFixedTags() {
        let failures: [(CloudOwnTransportFailure, String)] = [
            (.closed, "transportClosed"), (.busy, "transportBusy"),
            (.queueRejected, "transportQueueRejected"), (.missingReply, "transportMissingReply"),
            (.unexpectedReply, "transportUnexpectedReply"), (.invalidCursor, "transportInvalidCursor"),
            (.privateDatabaseRequired, "transportPrivateDatabaseRequired")
        ]
        for (error, expected) in failures { expectTag(error, expected: expected) }
        expectTag(CancellationError(), expected: "taskCancelled")
    }

    @Test func bridgeFailuresIgnoreAssociatedPayloads() {
        let sentinel = UUID().uuidString
        expectTag(BridgeError.cancelled, expected: "coreCancelled")
        expectTag(BridgeError.unavailable(reason: sentinel), expected: "coreUnavailable", sentinel: sentinel)
        expectTag(BridgeError.internal(sentinel), expected: "coreInternal", sentinel: sentinel)
        let others: [BridgeError] = [
            .busy(runningOperation: sentinel), .unsupported(feature: sentinel), .invalidArgument(sentinel),
            .suspendDeadline(phase: sentinel), .mediaCommitPending(sequence: 7, message: sentinel),
            .commitCompleted(sequence: 11, message: sentinel), .sessionActive
        ]
        for error in others { expectTag(error, expected: "coreOther", sentinel: sentinel) }
    }

    @Test func publicNSErrorDomainsPreserveOnlyInt32Codes() {
        let domains = ["CKErrorDomain", NSURLErrorDomain, NSCocoaErrorDomain, NSOSStatusErrorDomain]
        let codes = [Int(Int32.min), -1, 0, 1, Int(Int32.max)]
        for domain in domains {
            for code in codes {
                let sentinel = UUID().uuidString
                let error = syntheticError(domain: domain, code: code, sentinel: sentinel)
                expectTag(error, expected: "\(domain):\(code)", sentinel: sentinel)
            }
        }
    }

    #if canImport(CloudKit)
    @Test func cloudKitValuePreservesOnlyPublicCode() {
        let sentinel = UUID().uuidString
        let error = CKError(.networkUnavailable, userInfo: [
            NSLocalizedDescriptionKey: sentinel,
            NSLocalizedFailureReasonErrorKey: sentinel,
            "payload": sentinel
        ])
        expectTag(error, expected: "CKErrorDomain:3", sentinel: sentinel)
    }
    #endif

    @Test func foundationValuesPreserveOnlyPublicCodes() {
        let sentinel = UUID().uuidString
        let userInfo: [String: Any] = [
            NSLocalizedDescriptionKey: sentinel,
            NSLocalizedFailureReasonErrorKey: sentinel,
            "payload": sentinel
        ]
        expectTag(URLError(.notConnectedToInternet, userInfo: userInfo),
                  expected: "NSURLErrorDomain:-1009", sentinel: sentinel)
        expectTag(CocoaError(.fileReadNoSuchFile, userInfo: userInfo),
                  expected: "NSCocoaErrorDomain:260", sentinel: sentinel)
    }

    @Test func foundationValuesWithOversizedCodesAreOther() {
        let oversized = [Int(Int32.min) - 1, Int(Int32.max) + 1, Int.min, Int.max]
        for code in oversized {
            let sentinel = UUID().uuidString
            let userInfo: [String: Any] = [NSLocalizedDescriptionKey: sentinel, "payload": sentinel]
            expectTag(URLError(URLError.Code(rawValue: code), userInfo: userInfo),
                      expected: "other", sentinel: sentinel)
            expectTag(CocoaError(CocoaError.Code(rawValue: code), userInfo: userInfo),
                      expected: "other", sentinel: sentinel)
        }
    }

    @Test func unknownDomainsAndOversizedCodesAreOther() {
        let domains = ["CKErrorDomain", NSURLErrorDomain, NSCocoaErrorDomain, NSOSStatusErrorDomain]
        let oversized = [Int(Int32.min) - 1, Int(Int32.max) + 1, Int.min, Int.max]
        for domain in domains {
            for code in oversized {
                let sentinel = UUID().uuidString
                let error = syntheticError(domain: domain, code: code, sentinel: sentinel)
                expectTag(error, expected: "other", sentinel: sentinel)
            }
            let sentinel = UUID().uuidString
            for code in [Int(Int32.min), 0, Int(Int32.max)] {
                let error = syntheticError(domain: domain + sentinel, code: code, sentinel: sentinel)
                expectTag(error, expected: "other", sentinel: sentinel)
            }
        }
    }

    @Test func unrecognizedSwiftErrorIsOther() {
        struct SyntheticError: Error { let payload: String }
        struct SyntheticCustomNSError: Error, CustomNSError {
            let payload: String
            static var errorDomain: String { "CKErrorDomain" }
            var errorCode: Int { 19 }
            var errorUserInfo: [String: Any] { [NSLocalizedDescriptionKey: payload] }
        }
        struct SyntheticURLCustomNSError: Error, CustomNSError {
            let payload: String
            static var errorDomain: String { "NSURLErrorDomain" }
            var errorCode: Int { -1009 }
            var errorUserInfo: [String: Any] { [NSLocalizedDescriptionKey: payload] }
        }
        struct SyntheticCocoaCustomNSError: Error, CustomNSError {
            let payload: String
            static var errorDomain: String { "NSCocoaErrorDomain" }
            var errorCode: Int { 260 }
            var errorUserInfo: [String: Any] { [NSLocalizedDescriptionKey: payload] }
        }
        let sentinel = UUID().uuidString
        expectTag(SyntheticError(payload: sentinel), expected: "other", sentinel: sentinel)
        expectTag(SyntheticCustomNSError(payload: sentinel), expected: "other", sentinel: sentinel)
        expectTag(SyntheticURLCustomNSError(payload: sentinel), expected: "other", sentinel: sentinel)
        expectTag(SyntheticCocoaCustomNSError(payload: sentinel), expected: "other", sentinel: sentinel)
    }

    @Test func olderCompletionCannotReplaceNewRunningOrTerminalAttempt() {
        let probe = DebugCloudPreviewProbe()
        let untouched = DebugCloudPreviewProbe()
        #expect(probe.latestTag == "unobserved")
        let older = probe.begin()
        let newer = probe.begin()
        #expect(older != newer)
        probe.finish(older)
        #expect(probe.latestTag == "running")
        probe.fail(older, error: CloudOwnTransportFailure.closed)
        #expect(probe.latestTag == "running")
        probe.finish(newer)
        probe.finish(older)
        probe.fail(older, error: CancellationError())
        #expect(probe.latestTag == "finished")
        let newest = probe.begin()
        probe.fail(newest, error: CloudOwnTransportFailure.busy)
        probe.finish(newer)
        probe.fail(newer, error: CancellationError())
        #expect(probe.latestTag == "transportBusy")
        #expect(untouched.latestTag == "unobserved")
    }

    @Test func typedWorkCompletionsHaveExactTags() {
        let completions: [(CloudConnectionProgress, String)] = [
            (.progressed, "progressed"), (.sendEntities, "sendEntities"), (.idle, "idle")
        ]
        for (progress, expected) in completions {
            let work = DebugCloudPreviewProbe()
            let attempt = work.begin()
            work.finish(attempt, progress: progress)
            #expect(work.latestTag == expected)
        }
    }

    @Test func previewAndWorkInstancesAreIndependent() {
        let preview = DebugCloudPreviewProbe()
        let work = DebugCloudPreviewProbe()
        let previewAttempt = preview.begin()
        #expect(work.latestTag == "unobserved")
        #expect(work.lastEnteredWorkStageTag == nil)
        let workAttempt = work.begin()
        preview.finish(previewAttempt)
        #expect(preview.latestTag == "finished")
        #expect(work.latestTag == "running")
        work.enter(.performLocal, attempt: workAttempt)
        #expect(preview.lastEnteredWorkStageTag == nil)
        #expect(work.lastEnteredWorkStageTag == "performLocal")
        work.finish(workAttempt, progress: .sendEntities)
        #expect(preview.latestTag == "finished")
        #expect(work.latestTag == "sendEntities")
        let nextPreview = preview.begin()
        preview.fail(nextPreview, error: CloudOwnTransportFailure.closed)
        #expect(preview.latestTag == "transportClosed")
        #expect(work.latestTag == "sendEntities")
        #expect(preview.lastEnteredWorkStageTag == nil)
        #expect(work.lastEnteredWorkStageTag == "performLocal")
        _ = work.begin()
        #expect(work.lastEnteredWorkStageTag == nil)
        #expect(preview.latestTag == "transportClosed")
    }

    @Test func olderWorkCompletionsCannotReplaceNewRunningOrTerminalAttempt() {
        let work = DebugCloudPreviewProbe()
        let older = work.begin()
        let newer = work.begin()
        let progresses: [CloudConnectionProgress] = [.progressed, .sendEntities, .idle]
        #expect(older != newer)
        for progress in progresses {
            work.finish(older, progress: progress)
            work.fail(older, error: CloudOwnTransportFailure.closed)
            #expect(work.latestTag == "running")
        }
        work.finish(newer, progress: .idle)
        for progress in progresses {
            work.finish(older, progress: progress)
            work.fail(older, error: CancellationError())
            #expect(work.latestTag == "idle")
        }
        let newest = work.begin()
        work.fail(newest, error: CloudOwnTransportFailure.busy)
        work.finish(newer, progress: .progressed)
        work.fail(newer, error: CancellationError())
        #expect(work.latestTag == "transportBusy")
    }

    @Test func typedWorkStagesHaveExactTags() {
        let stages: [(DebugCloudPreviewProbe.WorkStage, String)] = [
            (.nextPublicationWork, "nextPublicationWork"), (.performLocal, "performLocal"),
            (.adoptionControlRead, "adoptionControlRead"), (.adoptionControlResult, "adoptionControlResult"),
            (.adoptionPageReply, "adoptionPageReply"), (.adoptionPageResult, "adoptionPageResult"),
            (.adoptionPageFailureResult, "adoptionPageFailureResult"),
            (.adoptionValidation, "adoptionValidation"), (.beforeActivation, "beforeActivation"),
            (.adoptionActivation, "adoptionActivation"), (.afterActivation, "afterActivation")
        ]
        #expect(stages.map(\.0) == DebugCloudPreviewProbe.WorkStage.allCases)
        let work = DebugCloudPreviewProbe()
        let older = work.begin()
        let current = work.begin()
        #expect(work.latestTag == "running")
        for (stage, expected) in stages {
            work.enter(stage, attempt: current)
            #expect(work.latestTag == expected)
            #expect(work.lastEnteredWorkStageTag == expected)
            work.enter(stage, attempt: older)
            #expect(work.latestTag == expected)
            #expect(work.lastEnteredWorkStageTag == expected)
            work.fail(current, error: CloudOwnTransportFailure.busy)
            #expect(work.latestTag == "transportBusy")
            #expect(work.lastEnteredWorkStageTag == expected)
        }
        work.finish(current)
        #expect(work.latestTag == "finished")
        #expect(work.lastEnteredWorkStageTag == "afterActivation")
        work.finish(current, progress: .progressed)
        #expect(work.latestTag == "progressed")
        #expect(work.lastEnteredWorkStageTag == "afterActivation")
    }

    @Test func olderWorkStagesCannotReplaceNewRunningOrTerminalAttempt() {
        let work = DebugCloudPreviewProbe()
        let older = work.begin()
        work.enter(.performLocal, attempt: older)
        _ = work.begin()
        #expect(work.lastEnteredWorkStageTag == nil)
        for stage in DebugCloudPreviewProbe.WorkStage.allCases {
            work.enter(stage, attempt: older)
            work.fail(older, error: CloudOwnTransportFailure.closed)
            work.finish(older)
            work.finish(older, progress: .progressed)
            #expect(work.latestTag == "running")
            #expect(work.lastEnteredWorkStageTag == nil)
        }
        let completions: [(CloudConnectionProgress, String)] = [
            (.progressed, "progressed"), (.sendEntities, "sendEntities"), (.idle, "idle")
        ]
        for (progress, expected) in completions {
            let current = work.begin()
            work.enter(.adoptionValidation, attempt: current)
            work.finish(current, progress: progress)
            for stage in DebugCloudPreviewProbe.WorkStage.allCases {
                work.enter(stage, attempt: older)
                work.fail(older, error: CancellationError())
                work.finish(older, progress: .sendEntities)
                #expect(work.latestTag == expected)
                #expect(work.lastEnteredWorkStageTag == "adoptionValidation")
            }
        }
        let current = work.begin()
        #expect(work.lastEnteredWorkStageTag == nil)
        work.enter(.adoptionActivation, attempt: current)
        work.fail(current, error: CloudOwnTransportFailure.busy)
        for stage in DebugCloudPreviewProbe.WorkStage.allCases {
            work.enter(stage, attempt: older)
            work.fail(older, error: CancellationError())
            work.finish(older)
            #expect(work.latestTag == "transportBusy")
            #expect(work.lastEnteredWorkStageTag == "adoptionActivation")
        }
    }

    @Test func workStageChangesAndResetNotifyObserverThatSynchronouslyReadsStage() {
        final class CallbackState: Sendable {
            let value = Mutex<(calls: Int, tag: String?)>((0, nil))
        }
        let work = DebugCloudPreviewProbe()
        let attempt = work.begin()
        let entered = CallbackState()
        let initialStage = withObservationTracking {
            work.lastEnteredWorkStageTag
        } onChange: {
            let tag = work.lastEnteredWorkStageTag
            entered.value.withLock { $0.calls += 1; $0.tag = tag }
        }
        #expect(initialStage == nil)
        work.enter(.performLocal, attempt: attempt)
        let entryCallback = entered.value.withLock { $0 }
        #expect(entryCallback.calls == 1)
        #expect(entryCallback.tag == nil || entryCallback.tag == "performLocal")
        #expect(work.lastEnteredWorkStageTag == "performLocal")

        let reset = CallbackState()
        let trackedStage = withObservationTracking {
            work.lastEnteredWorkStageTag
        } onChange: {
            let tag = work.lastEnteredWorkStageTag
            reset.value.withLock { $0.calls += 1; $0.tag = tag }
        }
        #expect(trackedStage == "performLocal")
        _ = work.begin()
        let resetCallback = reset.value.withLock { $0 }
        #expect(resetCallback.calls == 1)
        #expect(resetCallback.tag == "performLocal" || resetCallback.tag == nil)
        #expect(work.lastEnteredWorkStageTag == nil)
        #expect(work.latestTag == "running")
    }

    @Test func typedWorkCompletionNotifiesObserverThatSynchronouslyReadsTag() {
        final class CallbackState: Sendable {
            let value = Mutex<(calls: Int, tag: String?)>((0, nil))
        }
        let work = DebugCloudPreviewProbe()
        let attempt = work.begin()
        let observed = CallbackState()
        let trackedTag = withObservationTracking {
            work.latestTag
        } onChange: {
            let tag = work.latestTag
            observed.value.withLock { $0.calls += 1; $0.tag = tag }
        }
        #expect(trackedTag == "running")
        work.finish(attempt, progress: .progressed)
        let callback = observed.value.withLock { $0 }
        #expect(callback.calls == 1)
        // Native Observation calls onChange before the mutation.
        #expect(callback.tag == "running")
        #expect(work.latestTag == "progressed")
    }

    private func expectTag(_ error: any Error, expected: String, sentinel: String? = nil) {
        let tag = DebugCloudPreviewProbe.classify(error)
        let classificationMatches = tag == expected
        #expect(classificationMatches)
        let probe = DebugCloudPreviewProbe()
        let attempt = probe.begin()
        probe.fail(attempt, error: error)
        let stored = probe.latestTag
        let storedTagMatches = stored == expected
        #expect(storedTagMatches)
        if let sentinel {
            let payloadAbsent = !tag.contains(sentinel) && !stored.contains(sentinel)
            #expect(payloadAbsent)
        }
    }

    private func syntheticError(domain: String, code: Int, sentinel: String) -> NSError {
        NSError(domain: domain, code: code, userInfo: [
            NSLocalizedDescriptionKey: sentinel,
            NSLocalizedFailureReasonErrorKey: sentinel,
            NSUnderlyingErrorKey: NSError(domain: sentinel, code: 19, userInfo: [NSLocalizedDescriptionKey: sentinel]),
            "payload": sentinel
        ])
    }
}
#endif
