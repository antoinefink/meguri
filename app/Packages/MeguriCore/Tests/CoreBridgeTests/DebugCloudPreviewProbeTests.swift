#if DEBUG
import Foundation
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
