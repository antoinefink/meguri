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
    @Test func previewReadProjectionIsBoundedAndContentFree() {
        let sentinel = UUID().uuidString
        let record = CloudControlRecord(encrypted: Data(sentinel.utf8), systemFields: Data(sentinel.utf8))
        #expect(DebugCloudPreviewProbe.ReadPhase(recordType: "MGActive") == .active)
        #expect(DebugCloudPreviewProbe.ReadPhase(recordType: "MGEpoch") == .epoch)
        let rejected = [sentinel, "MGActive" + sentinel, "MGEpoch" + sentinel, "mgactive"]
        for value in rejected { #expect(DebugCloudPreviewProbe.ReadPhase(recordType: value) == nil) }
        let outcomes: [(CloudControlRead, String)] = [
            (.found(record), "found"), (.missing, "missing"), (.zoneMissing, "zoneMissing"), (.failed, "failed")
        ]
        let probe = DebugCloudPreviewProbe()
        let attempt = probe.begin()
        for (read, outcome) in outcomes {
            probe.recordRead(.epoch, attempt: attempt, read: read)
            let unknown = probe.previewReadTag
            #expect(unknown == "previewRead:epoch;previewReadOutcome:\(outcome);previewReadEvidence:none;previewReadItem:unknown;previewReadTerminal:unknown;previewReadThrown:none")
            // All-fields success has evidence but no failure; it must not be
            // presented as an SDK failure, even when the record carries bytes.
            probe.recordRead(.active, attempt: attempt, read: read, evidence: .init(read: read))
            let observed = probe.previewReadTag
            #expect(observed == "previewRead:active;previewReadOutcome:\(outcome);previewReadEvidence:present;previewReadItem:none;previewReadTerminal:none;previewReadThrown:none")
            let boundedAndContentFree = observed.count < 320 && unknown.count < 320
                && !observed.contains(sentinel) && !unknown.contains(sentinel)
            #expect(boundedAndContentFree)
        }
    }

    @Test func previewReadRetainsOnlyOriginalItemAndTerminalCodesThroughFinalFailure() {
        let probe = DebugCloudPreviewProbe()
        let attempt = probe.begin()
        for code in [Int64.min, 11, Int64.max] {
            probe.recordRead(.epoch, attempt: attempt, read: .failed, evidence: .init(read: .failed,
                failure: .init(code: code, retryAfterMilliseconds: 91357, encryptedDataReset: true),
                terminal: .init(code: 2, retryAfterMilliseconds: 84629)))
            let tag = probe.previewReadTag
            #expect(tag == "previewRead:epoch;previewReadOutcome:failed;previewReadEvidence:present;previewReadItem:CKErrorDomain:\(code);previewReadTerminal:CKErrorDomain:2;previewReadThrown:none")
            #expect(tag.count < 320)
            #expect(!tag.contains("91357") && !tag.contains("84629"))
            probe.fail(attempt, error: BridgeError.cloudConnection(.epochIncomplete))
            #expect(probe.previewReadTag == tag)
            probe.finish(attempt)
            #expect(probe.previewReadTag == tag)
        }
        probe.recordRead(.epoch, attempt: attempt, read: .failed,
            evidence: .init(read: .failed, failure: .init(code: nil)))
        #expect(probe.previewReadTag == "previewRead:epoch;previewReadOutcome:failed;previewReadEvidence:present;previewReadItem:unknown:none;previewReadTerminal:none;previewReadThrown:none")
    }

    @Test func previewReadResetAndStaleIsolationKeepOnlyLastActualRead() {
        let probe = DebugCloudPreviewProbe()
        #expect(probe.previewReadTag == "previewRead:none")
        let older = probe.begin()
        probe.recordRead(.active, attempt: older, read: .missing)
        let current = probe.begin()
        #expect(probe.previewReadTag == "previewRead:none")
        probe.recordRead(.epoch, attempt: older, read: .failed)
        #expect(probe.previewReadTag == "previewRead:none")
        probe.recordRead(.active, attempt: current, read: .missing,
            evidence: .init(read: .missing, failure: .init(code: 11)))
        probe.recordRead(.epoch, attempt: current)
        #expect(probe.previewReadTag == "previewRead:epoch;previewReadOutcome:unknown;previewReadEvidence:none;previewReadItem:unknown;previewReadTerminal:unknown;previewReadThrown:none")
        let sentinel = UUID().uuidString
        probe.recordRead(.epoch, attempt: current, error: BridgeError.internal(sentinel))
        let expected = "previewRead:epoch;previewReadOutcome:error;previewReadEvidence:none;previewReadItem:unknown;previewReadTerminal:unknown;previewReadThrown:coreInternal"
        probe.fail(current, error: BridgeError.cloudConnection(.epochIncomplete))
        probe.recordRead(.active, attempt: older, read: .zoneMissing,
            evidence: .init(read: .zoneMissing, terminal: .init(code: 26)))
        probe.recordRead(.active, attempt: older, error: CancellationError())
        probe.finish(older)
        #expect(probe.previewReadTag == expected)
        let payloadAbsent = !probe.previewReadTag.contains(sentinel)
        #expect(payloadAbsent)
    }

    @Test func previewReadNotificationsAllowSynchronousRereadsAndReset() {
        let probe = DebugCloudPreviewProbe()
        let attempt = probe.begin()
        let calls = Mutex(0)
        let mutations: [() -> Void] = [
            { probe.recordRead(.epoch, attempt: attempt, read: .failed) },
            { _ = probe.begin() }
        ]
        for mutate in mutations {
            withObservationTracking {
                _ = probe.previewReadTag
            } onChange: {
                _ = probe.previewReadTag
                _ = probe.latestTag
                calls.withLock { $0 += 1 }
            }
            mutate()
        }
        #expect(calls.withLock { $0 } == 2)
        #expect(probe.previewReadTag == "previewRead:none")
    }

    @Test func previewReadReentrantBeginPreservesNewerAttempt() throws {
        let probe = DebugCloudPreviewProbe()
        let original = probe.begin()
        probe.enter(.performLocal, attempt: original)
        probe.recordRead(.active, attempt: original, read: .missing)
        let originalRead = probe.previewReadTag
        let newerAttempt = Mutex<UUID?>(nil)
        let didReenter = Mutex(false)
        let newerRead = "previewRead:epoch;previewReadOutcome:zoneMissing;previewReadEvidence:present;previewReadItem:CKErrorDomain:26;previewReadTerminal:CKErrorDomain:2;previewReadThrown:none"
        withObservationTracking {
            _ = probe.previewReadTag
        } onChange: {
            // Native willSet can notify this observer again before it unregisters.
            guard didReenter.withLock({ entered in
                if entered { return false }
                entered = true
                return true
            }) else { return }
            #expect(probe.latestTag == "performLocal")
            #expect(probe.lastEnteredWorkStageTag == "performLocal")
            #expect(probe.previewReadTag == originalRead)
            // The outer begin has reserved ordering, but has not reset values.
            probe.enter(.afterActivation, attempt: original)
            probe.finish(original)
            #expect(probe.latestTag == "performLocal")
            #expect(probe.lastEnteredWorkStageTag == "performLocal")
            let newer = probe.begin()
            #expect(probe.latestTag == "running")
            #expect(probe.lastEnteredWorkStageTag == nil)
            #expect(probe.previewReadTag == "previewRead:none")
            probe.enter(.adoptionControlRead, attempt: newer)
            probe.recordRead(.epoch, attempt: newer, read: .zoneMissing,
                evidence: .init(read: .zoneMissing, failure: .init(code: 26), terminal: .init(code: 2)))
            newerAttempt.withLock { $0 = newer }
        }
        let stale = probe.begin()
        #expect(didReenter.withLock { $0 })
        let newer = try #require(newerAttempt.withLock { $0 })
        #expect(stale != newer)
        #expect(probe.latestTag == "adoptionControlRead")
        #expect(probe.lastEnteredWorkStageTag == "adoptionControlRead")
        #expect(probe.previewReadTag == newerRead)
        probe.recordRead(.active, attempt: stale, read: .failed)
        probe.enter(.afterActivation, attempt: stale)
        probe.fail(stale, error: CloudOwnTransportFailure.busy)
        probe.finish(stale)
        #expect(probe.latestTag == "adoptionControlRead")
        #expect(probe.lastEnteredWorkStageTag == "adoptionControlRead")
        #expect(probe.previewReadTag == newerRead)
        probe.recordRead(.active, attempt: newer, read: .missing)
        probe.enter(.adoptionControlResult, attempt: newer)
        probe.finish(newer, progress: .idle)
        #expect(probe.latestTag == "idle")
        #expect(probe.lastEnteredWorkStageTag == "adoptionControlResult")
        #expect(probe.previewReadTag == "previewRead:active;previewReadOutcome:missing;previewReadEvidence:none;previewReadItem:unknown;previewReadTerminal:unknown;previewReadThrown:none")
    }

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

    @Test func adoptionPagesAccumulateAcrossWorkAttempts() {
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        owner.reset(in: work)
        for records in [3, 0, 7] {
            let attempt = work.begin()
            let session = owner.capture()
            work.pageEnqueued(session: session)
            work.enter(.adoptionPageReply, attempt: attempt)
            work.pageReplied(session: session, recordCount: records)
            work.enter(.adoptionPageResult, attempt: attempt)
            work.finish(attempt, progress: .progressed)
        }
        #expect(work.adoptionProgressTag == "pagesEnqueued:3;pageRepliesReceived:3;recordsReturned:10")
        #expect(work.latestTag == "progressed")
        #expect(work.lastEnteredWorkStageTag == "adoptionPageResult")
    }

    @Test func newAdoptionResetsCountersWithoutResettingAttemptTags() {
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        owner.reset(in: work)
        let session = owner.capture()
        let attempt = work.begin()
        work.pageEnqueued(session: session)
        work.pageReplied(session: session, recordCount: 59_000)
        work.enter(.adoptionPageResult, attempt: attempt)
        work.finish(attempt, progress: .progressed)
        owner.reset(in: work)
        #expect(work.adoptionProgressTag == "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0")
        #expect(work.latestTag == "progressed")
        #expect(work.lastEnteredWorkStageTag == "adoptionPageResult")
        work.pageEnqueued(session: owner.capture())
        work.pageReplied(session: owner.capture(), recordCount: 2)
        #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:2")
    }

    @Test func busyAndRejectedWorkAttemptsDoNotResetAdoption() {
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        owner.reset(in: work)
        let session = owner.capture()
        work.pageEnqueued(session: session)
        work.pageReplied(session: session, recordCount: 4)
        for failure in [CloudOwnTransportFailure.busy, .queueRejected] {
            let attempt = work.begin()
            work.fail(attempt, error: failure)
            #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:4")
        }
        work.pageEnqueued(session: session)
        work.pageReplied(session: session, recordCount: 5)
        #expect(work.adoptionProgressTag == "pagesEnqueued:2;pageRepliesReceived:2;recordsReturned:9")
    }

    @Test func lateRepliesAndOldConnectionSessionsCannotUpdateNewAdoption() {
        let work = DebugCloudPreviewProbe()
        let original = DebugCloudPreviewProbe.AdoptionSession()
        original.reset(in: work)
        let beforeNewAdoption = original.capture()
        original.reset(in: work)
        work.pageEnqueued(session: beforeNewAdoption)
        work.pageReplied(session: beforeNewAdoption, recordCount: 99)
        #expect(work.adoptionProgressTag == "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0")

        let attached = DebugCloudPreviewProbe.AdoptionSession()
        attached.reset(in: work)
        work.pageEnqueued(session: attached.capture())
        work.pageReplied(session: attached.capture(), recordCount: 6)
        work.pageEnqueued(session: original.capture())
        work.pageReplied(session: original.capture(), recordCount: 100)
        #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:6")
    }

    @Test func pageReplyCountsRecordsExactlyAndDoesNotImplyCoreIntake() {
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        work.pageEnqueued(session: owner.capture())
        work.pageReplied(session: owner.capture(), recordCount: 12)
        #expect(work.adoptionProgressTag == "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0")
        owner.reset(in: work)
        let session = owner.capture()
        work.pageEnqueued(session: session)
        #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:0;recordsReturned:0")
        // Only the count crosses the probe, including any epoch/control records.
        work.pageReplied(session: session, recordCount: 3)
        let attempt = work.begin()
        work.fail(attempt, error: BridgeError.internal("synthetic"))
        #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:3")
        work.pageEnqueued(session: session)
        work.pageReplied(session: session, recordCount: 0)
        #expect(work.adoptionProgressTag == "pagesEnqueued:2;pageRepliesReceived:2;recordsReturned:3")
    }

    @Test func adoptionCountersSaturateWithoutOverflow() {
        var counters = DebugCloudPreviewProbe.AdoptionCounters(
            pagesEnqueued: .max - 1, pageRepliesReceived: .max - 1, recordsReturned: .max - 2)
        counters.enqueue()
        counters.reply(recordCount: 1)
        #expect(counters.pagesEnqueued == .max)
        #expect(counters.pageRepliesReceived == .max)
        #expect(counters.recordsReturned == UInt64.max - 1)
        counters.enqueue()
        counters.reply(recordCount: Int.max)
        counters.enqueue()
        counters.reply(recordCount: 1)
        #expect(counters.pagesEnqueued == .max)
        #expect(counters.pageRepliesReceived == .max)
        #expect(counters.recordsReturned == .max)
    }

    @Test func adoptionProgressAndResetNotifyObserverThatSynchronouslyRereads() {
        final class CallbackState: Sendable {
            let value = Mutex<(calls: Int, tag: String?)>((0, nil))
        }
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        owner.reset(in: work)
        let session = owner.capture()
        let mutations: [() -> Void] = [
            { work.pageEnqueued(session: session) },
            { work.pageReplied(session: session, recordCount: 8) },
            { owner.reset(in: work) }
        ]
        let expected = [
            "pagesEnqueued:1;pageRepliesReceived:0;recordsReturned:0",
            "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:8",
            "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0"
        ]
        for (mutate, finalTag) in zip(mutations, expected) {
            let callback = CallbackState()
            let before = withObservationTracking {
                work.adoptionProgressTag
            } onChange: {
                let tag = work.adoptionProgressTag
                callback.value.withLock { $0.calls += 1; $0.tag = tag }
            }
            mutate()
            let observed = callback.value.withLock { $0 }
            #expect(observed.calls == 1)
            #expect(observed.tag == before || observed.tag == finalTag)
            #expect(work.adoptionProgressTag == finalTag)
        }
    }

    @Test func adoptionFailureRetainsExactTypedDetailAcrossFollowups() {
        let categories: [(CloudAdoptionFailureDiagnostic.Category?, CloudAdoptionFailureDiagnostic.Category?, String)] = [
            (.validation, nil, "adoptionPrimary:validation;adoptionCleanup:none"),
            (nil, .fileIdentityUnavailable, "adoptionPrimary:none;adoptionCleanup:fileIdentityUnavailable"),
            (.validation, .fileIdentityUnavailable, "adoptionPrimary:validation;adoptionCleanup:fileIdentityUnavailable")
        ]
        for (primary, cleanup, suffix) in categories {
            let work = DebugCloudPreviewProbe()
            let owner = DebugCloudPreviewProbe.AdoptionSession()
            owner.reset(in: work)
            let session = owner.capture()
            let original = CloudAdoptionFailureDiagnostic(
                task: .fetchPayload, lastEnteredStage: .receivedValidation,
                primaryCategory: primary, cleanupCategory: cleanup)
            work.recordAdoptionFailure(original, session: session)
            let expected = "adoptionTask:fetchPayload;adoptionLastEnteredStage:receivedValidation;\(suffix)"
            #expect(work.adoptionFailureTag == expected)
            let followup = work.begin()
            work.enter(.adoptionPageFailureResult, attempt: followup)
            work.fail(followup, error: BridgeError.cloudConnection(.tracking))
            work.recordAdoptionFailure(.init(task: .fetch, lastEnteredStage: .transport,
                primaryCategory: .transport, cleanupCategory: .tracking), session: session)
            #expect(work.adoptionFailureTag == expected)
            #expect(expected.count < 256)
            #expect(work.adoptionProgressTag == "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0")
        }
    }

    @Test func adoptionFailureRejectsStaleAndOldConnectionSessions() {
        let work = DebugCloudPreviewProbe()
        let original = DebugCloudPreviewProbe.AdoptionSession()
        let failure = CloudAdoptionFailureDiagnostic(task: .fetch, lastEnteredStage: .rowStage,
            primaryCategory: .tracking, cleanupCategory: nil)
        work.recordAdoptionFailure(failure, session: original.capture())
        #expect(work.adoptionFailureTag == nil)
        original.reset(in: work)
        let stale = original.capture()
        original.reset(in: work)
        work.recordAdoptionFailure(failure, session: stale)
        #expect(work.adoptionFailureTag == nil)
        let attached = DebugCloudPreviewProbe.AdoptionSession()
        attached.reset(in: work)
        work.recordAdoptionFailure(failure, session: original.capture())
        #expect(work.adoptionFailureTag == nil)
        work.recordAdoptionFailure(failure, session: attached.capture())
        let expected = "adoptionTask:fetch;adoptionLastEnteredStage:rowStage;adoptionPrimary:tracking;adoptionCleanup:none"
        #expect(work.adoptionFailureTag == expected)
        work.recordAdoptionFailure(.init(task: .activate, lastEnteredStage: .authority,
            primaryCategory: .authorityChanged, cleanupCategory: .cancelled), session: stale)
        #expect(work.adoptionFailureTag == expected)
    }

    @Test func adoptionSessionResetClearsFailureAndCountersTogether() {
        let work = DebugCloudPreviewProbe()
        let original = DebugCloudPreviewProbe.AdoptionSession()
        original.reset(in: work)
        let failure = CloudAdoptionFailureDiagnostic(task: .fetch, lastEnteredStage: .pageAdvance,
            primaryCategory: .engine, cleanupCategory: .tracking)
        for owner in [original, DebugCloudPreviewProbe.AdoptionSession()] {
            let session = original.capture()
            work.pageEnqueued(session: session)
            work.pageReplied(session: session, recordCount: 9)
            work.recordAdoptionFailure(failure, session: session)
            #expect(work.adoptionFailureTag != nil)
            #expect(work.adoptionProgressTag == "pagesEnqueued:1;pageRepliesReceived:1;recordsReturned:9")
            owner.reset(in: work)
            #expect(work.adoptionFailureTag == nil)
            #expect(work.adoptionProgressTag == "pagesEnqueued:0;pageRepliesReceived:0;recordsReturned:0")
        }
    }

    @Test func adoptionFailureAndResetNotifyObserverThatSynchronouslyRereads() {
        final class CallbackState: Sendable {
            let value = Mutex<(calls: Int, tag: String?)>((0, nil))
        }
        let work = DebugCloudPreviewProbe()
        let owner = DebugCloudPreviewProbe.AdoptionSession()
        owner.reset(in: work)
        let session = owner.capture()
        let failure = CloudAdoptionFailureDiagnostic(task: .fetch, lastEnteredStage: .pageSpace,
            primaryCategory: .incomplete, cleanupCategory: nil)
        let mutations: [() -> Void] = [
            { work.recordAdoptionFailure(failure, session: session) },
            { owner.reset(in: work) }
        ]
        let expected: [String?] = [
            "adoptionTask:fetch;adoptionLastEnteredStage:pageSpace;adoptionPrimary:incomplete;adoptionCleanup:none", nil
        ]
        for (mutate, finalTag) in zip(mutations, expected) {
            let callback = CallbackState()
            let before = withObservationTracking {
                work.adoptionFailureTag
            } onChange: {
                let tag = work.adoptionFailureTag
                _ = work.adoptionProgressTag
                callback.value.withLock { $0.calls += 1; $0.tag = tag }
            }
            mutate()
            let observed = callback.value.withLock { $0 }
            #expect(observed.calls == 1)
            #expect(observed.tag == before || observed.tag == finalTag)
            #expect(work.adoptionFailureTag == finalTag)
        }
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

/// Holds the original reply independently of its evidence, as native callbacks
/// do. No SDK calls; evidence is available only after that same reply drains.
private final class DebugPreparedPreviewRead: CloudOwnTransport, CloudPreparedOwnOperation, Sendable {
    let held = HeldOwnOperation()
    let evidenceReads = Mutex(0)
    private let replied = Mutex(false)
    private let error: (any Error)?
    init(error: (any Error)? = nil) { self.error = error }
    func prepare(_ command: CloudOwnTransportCommand) -> any CloudPreparedOwnOperation { self }
    func enqueue() -> Bool { held.enqueue() }
    func cancel() { held.cancel() }
    func reply() async throws -> CloudOwnTransportReply {
        let reply = try await held.reply()
        replied.withLock { $0 = true }
        if let error { throw error }
        return reply
    }
    func controlReadEvidence() -> CloudZoneDeletionRead? {
        evidenceReads.withLock { $0 += 1 }
        guard replied.withLock({ $0 }) else { return nil }
        return .init(read: .zoneMissing, failure: .init(code: 26), terminal: .init(code: 2))
    }
}

extension EventTests {
    @Test func debugPreviewOriginalReplyEvidenceAndThrownAwait() async throws {
        for throwing in [false, true] {
            try await withCloudConnection { f in
                try await f.beginDiscovery()
                defer { f.endDiscovery() }
                let sentinel = UUID().uuidString
                let operation = DebugPreparedPreviewRead(error: throwing ? BridgeError.internal(sentinel) : nil)
                let executor = CloudOwnExecutor(core: f.connection, transport: operation)
                let pending = Task { try await executor.preview() }
                try await operation.held.waitUntilQueued()
                operation.held.finish(.read(.zoneMissing))
                if throwing {
                    do { _ = try await pending.value; Issue.record("Expected error") }
                    catch {
                        let originalError = error as? BridgeError == .internal(sentinel)
                        #expect(originalError)
                    }
                    #expect(DebugCloudPreviewProbe.shared.previewReadTag == "previewRead:active;previewReadOutcome:error;previewReadEvidence:present;previewReadItem:CKErrorDomain:26;previewReadTerminal:CKErrorDomain:2;previewReadThrown:coreInternal")
                    #expect(operation.evidenceReads.withLock { $0 } == 1)
                } else {
                    guard case .ready(let handle, let summary) = try await pending.value else {
                        throw CloudOwnTransportFailure.unexpectedReply
                    }
                    #expect(summary.decision == .confirmUpload)
                    #expect(DebugCloudPreviewProbe.shared.previewReadTag == "previewRead:active;previewReadOutcome:zoneMissing;previewReadEvidence:present;previewReadItem:CKErrorDomain:26;previewReadTerminal:CKErrorDomain:2;previewReadThrown:none")
                    #expect(operation.evidenceReads.withLock { $0 } == 1)
                    try await f.connection.cancelPreview(handle)
                }
                let payloadAbsent = !DebugCloudPreviewProbe.shared.previewReadTag.contains(sentinel)
                #expect(payloadAbsent)
            }
        }
    }

    @Test func debugPreviewOldResultTaskCannotUpdateNewBusyAttempt() async throws {
        try await withCloudConnection { f in
            try await f.beginDiscovery()
            defer { f.endDiscovery() }
            let operation = DebugPreparedPreviewRead()
            let executor = CloudOwnExecutor(core: f.connection, transport: operation)
            let pending = Task { try await executor.preview() }
            try await operation.held.waitUntilQueued()
            await #expect(throws: CloudOwnTransportFailure.busy) { _ = try await executor.preview() }
            #expect(DebugCloudPreviewProbe.shared.previewReadTag == "previewRead:none")
            operation.held.finish(.read(.zoneMissing))
            guard case .ready(let handle, _) = try await pending.value else {
                throw CloudOwnTransportFailure.unexpectedReply
            }
            #expect(operation.evidenceReads.withLock { $0 } == 1)
            #expect(DebugCloudPreviewProbe.shared.latestTag == "transportBusy")
            #expect(DebugCloudPreviewProbe.shared.previewReadTag == "previewRead:none")
            try await f.connection.cancelPreview(handle)
        }
    }
}
#endif
