#if DEBUG
import Dispatch
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

struct DebugCloudDiagnosticsTests {
    private typealias Value = DebugCloudDiagnostics.Value

    @Test func policyFailuresHaveExactTags() {
        let cases: [(CloudPolicyFailure, String)] = [
            (.closed, "policy.closed"), (.wrongOwner, "policy.wrongOwner"),
            (.generationChanged, "policy.generationChanged"), (.staleCommand, "policy.staleCommand"),
            (.notCoreAction, "policy.notCoreAction"), (.notNativeAction, "policy.notNativeAction"),
            (.coreCapabilityUnavailable, "policy.coreCapabilityUnavailable"),
            (.unqualifiedCompletion, "policy.unqualifiedCompletion")
        ]
        for (failure, expected) in cases {
            expectFailure(BridgeError.cloudPolicy(failure), expected)
        }
    }

    @Test func connectionFailuresHaveExactTags() {
        let cases: [(CloudConnectionFailure, String)] = [
            (.closed, "connection.closed"), (.wrongOwner, "connection.wrongOwner"),
            (.consumed, "connection.consumed"), (.authorityChanged, "connection.authorityChanged"),
            (.accountUnavailable, "connection.accountUnavailable"), (.accountMismatch, "connection.accountMismatch"),
            (.busy, "connection.busy"), (.keyUnavailable, "connection.keyUnavailable"),
            (.epochIncomplete, "connection.epochIncomplete"), (.unreadableControl, "connection.unreadableControl"),
            (.notOffered, "connection.notOffered"), (.incomingStagingUnavailable, "connection.incomingStagingUnavailable"),
            (.mediaApplyUnavailable, "connection.mediaApplyUnavailable"), (.resetRecoveryUnavailable, "connection.resetRecoveryUnavailable"),
            (.tracking, "connection.tracking"), (.incomplete, "connection.incomplete"),
            (.fileIdentityUnavailable, "connection.fileIdentityUnavailable")
        ]
        for (failure, expected) in cases {
            expectFailure(BridgeError.cloudConnection(failure), expected)
        }
    }

    @Test func ownTransportAndCancellationHaveExactTags() {
        let cases: [(CloudOwnTransportFailure, String)] = [
            (.closed, "own.closed"), (.busy, "own.busy"), (.queueRejected, "own.queueRejected"),
            (.missingReply, "own.missingReply"), (.unexpectedReply, "own.unexpectedReply"),
            (.invalidCursor, "own.invalidCursor"), (.privateDatabaseRequired, "own.privateDatabaseRequired")
        ]
        for (failure, expected) in cases { expectFailure(failure, expected) }
        expectFailure(CancellationError(), "taskCancelled")
        expectFailure(BridgeError.cancelled, "bridge.cancelled")
    }

    @Test func bridgePayloadsNeverReachTheFormattedValue() {
        let sentinel = UUID().uuidString
        let cases: [(BridgeError, String)] = [
            (.maintenance(.io(sentinel)), "bridge.maintenance"),
            (.exportSourceChanged, "bridge.exportSourceChanged"),
            (.exportSourceInvalid, "bridge.exportSourceInvalid"),
            (.exportFormatLimit, "bridge.exportFormatLimit"),
            (.deckOptionsInvalid([]), "bridge.deckOptionsInvalid"),
            (.stale(table: .notes, key: sentinel), "bridge.stale"),
            (.staleGeneration(planned: 1234567, current: 7654321), "bridge.staleGeneration"),
            (.undoUnavailable(reason: .changedExternally), "bridge.undoUnavailable"),
            (.notetypeRefused(.template(kind: .unknownField, template: 1234567, side: .front,
                name: sentinel, earlier: 7654321)), "bridge.notetypeRefused"),
            (.deckRefused(.filteredParent), "bridge.deckRefused"),
            (.noteNotFound(id: 1234567), "bridge.noteNotFound"),
            (.notFound(table: .cards, key: sentinel), "bridge.notFound"),
            (.insufficientSpace(required: 1234567, available: 7654321), "bridge.insufficientSpace"),
            (.open(.corrupt(detail: sentinel)), "bridge.open"),
            (.suspendDeadline(phase: sentinel), "bridge.suspendDeadline"),
            (.schedOpRefused(.homeDeckFiltered(deck: 1234567)), "bridge.schedOpRefused"),
            (.sessionActive, "bridge.sessionActive"),
            (.invalidTypeAnswerReplacement, "bridge.invalidTypeAnswerReplacement"),
            (.busy(runningOperation: sentinel), "bridge.busy"),
            (.unavailable(reason: sentinel), "bridge.unavailable"),
            (.unsupported(feature: sentinel), "bridge.unsupported"),
            (.invalidArgument(sentinel), "bridge.invalidArgument"),
            (.internal(sentinel), "bridge.internal"),
            (.mediaCommitPending(sequence: 1234567, message: sentinel), "bridge.mediaCommitPending"),
            (.commitCompleted(sequence: 7654321, message: sentinel), "bridge.commitCompleted")
        ]
        for (failure, expected) in cases {
            expectFailure(failure, expected)
            let value = Value(.policyExecute, error: failure)
            #expect(!value.formatted.contains(sentinel))
            #expect(!value.formatted.contains("1234567"))
            #expect(!value.formatted.contains("7654321"))
        }
    }

    @Test func actionsDiscardAssociatedData() {
        let sentinel = UUID().uuidString
        let cases: [(CloudPolicyAction, String)] = [
            (.startEngine(automatic: true), "startEngine"),
            (.startEngine(automatic: false), "startEngine"),
            (.stopEngine(.bindingChanged), "stopEngine"),
            (.configureAutomatic(on: false), "configureAutomatic"),
            (.updatePending(.changes), "updatePending"),
            (.requestFetch(zones: [.root, .collection, .media]), "requestFetch"),
            (.requestSend(zones: []), "requestSend"),
            (.readBlobs(includeMissing: true), "readBlobs"),
            (.retryAssetEntries, "retryAssetEntries"), (.runCleanup, "runCleanup"),
            (.deleteZones, "deleteZones"),
            (.armFlushTimer(atMilliseconds: .max), "armFlushTimer"),
            (.cancelFlushTimer, "cancelFlushTimer"),
            (.runExchange(exchange: .max, source: .ankiConnect), "runExchange"),
            (.runExchange(exchange: 1, source: .command), "runExchange"),
            (.cancelExchange(exchange: .max), "cancelExchange"),
            (.backgroundFlush(untilMilliseconds: .min), "backgroundFlush"),
            (.cancelBackgroundFlush, "cancelBackgroundFlush"),
            (.quitFlush(untilMilliseconds: .max), "quitFlush"), (.quitNow, "quitNow"),
            (.pauseMedia, "pauseMedia"), (.resumeMedia, "resumeMedia"),
            (.prioritizeMedia(names: [sentinel]), "prioritizeMedia"),
            (.prioritizeMedia(names: []), "prioritizeMedia"),
            (.applyReady(retryFailed: true), "applyReady"),
            (.recordQuietPoint(atMilliseconds: .min), "recordQuietPoint"),
            (.probeTracking, "probeTracking")
        ]
        for (action, expected) in cases {
            let value = Value(.policyExecute, action: action, error: BridgeError.cancelled)
            #expect(value.action == expected)
            #expect(value.formatted == "boundary=policyExecute action=\(expected) failure=bridge.cancelled")
            #expect(!value.formatted.contains(sentinel))
        }
        let own = Value(.ownWork, action: .prioritizeMedia(names: [sentinel]), error: CancellationError())
        #expect(own.formatted == "boundary=ownWork action=ownWork failure=taskCancelled")
        #expect(Value(.ownWork, error: CancellationError()) == own)
        #expect(Value(.connectionPreview, error: CancellationError()).formatted ==
            "boundary=connectionPreview action=none failure=taskCancelled")
        #expect(Value(.policyExchange, action: .cancelExchange(exchange: .max), error: CancellationError()).formatted ==
            "boundary=policyExchange action=cancelExchange failure=taskCancelled")
    }

    @Test func sdkWrapperUsesOnlyExactlyBoundedCode() {
        let sentinel = UUID().uuidString
        let underlying = NSError(domain: sentinel, code: 987654321, userInfo: [
            NSLocalizedDescriptionKey: sentinel,
            NSUnderlyingErrorKey: NSError(domain: sentinel, code: 1234567, userInfo: [sentinel: sentinel]),
            sentinel: sentinel
        ])
        for code in [Int64(Int32.min), -1, 0, Int64(Int32.max)] {
            let error = CloudKitSDKOperationFailure(underlying: underlying, code: code, retryAfterMilliseconds: .max)
            let value = Value(.policyExecute, error: error)
            #expect(value.failure == "sdkOperation")
            #expect(value.code == Int32(exactly: code))
            #expect(value.formatted == "boundary=policyExecute action=none failure=sdkOperation code=\(code)")
            #expect(!value.formatted.contains(sentinel))
        }
        for code in [Int64.min, Int64(Int32.min) - 1, Int64(Int32.max) + 1, Int64.max] {
            let error = CloudKitSDKOperationFailure(underlying: HostileError(), code: code, retryAfterMilliseconds: 7)
            let value = Value(.policyExecute, error: error)
            #expect(value.code == nil)
            #expect(value.formatted == "boundary=policyExecute action=none failure=sdkOperation")
        }
        expectFailure(underlying, "other")
    }

    @Test func unknownErrorsDoNotInvokeNSErrorOrDescriptionAccessors() {
        struct Unknown: Error { let text: String }
        expectFailure(Unknown(text: UUID().uuidString), "other")
        expectFailure(HostileError(), "other")
        let wrapped = CloudKitSDKOperationFailure(underlying: HostileError(), code: 20, retryAfterMilliseconds: nil)
        let value = Value(.ownWork, error: wrapped)
        #expect(value.formatted == "boundary=ownWork action=ownWork failure=sdkOperation code=20")
    }

    @Test func recorderStartsEmptyAndRetainsOnlyTheExactSafeValue() {
        let recorder = DebugCloudDiagnostics.Recorder()
        #expect(recorder.latestValue == nil)
        let error = CloudKitSDKOperationFailure(underlying: HostileError(), code: 20, retryAfterMilliseconds: .max)
        let value = Value(.ownWork, error: error)
        recorder.emit(value) { received in
            #expect(received == value)
            #expect(recorder.latestValue == received.formatted)
        }
        #expect(recorder.latestValue == "boundary=ownWork action=ownWork failure=sdkOperation code=20")
    }

    @Test func recorderUpdatesLatestAfterTheCapWithoutCallingTheSink() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let value = Value(.ownWork, error: CloudOwnTransportFailure.missingReply)
        var calls = 0
        for _ in 0..<64 { recorder.emit(value) { _ in calls += 1 } }
        #expect(calls == 64)
        #expect(recorder.latestValue == value.formatted)
        for boundary: DebugCloudDiagnostics.Boundary in [.policyExecute, .connectionPreview] {
            let latest = Value(boundary, error: CancellationError())
            recorder.emit(latest) { _ in calls += 1 }
            #expect(recorder.latestValue == "boundary=\(boundary.rawValue) action=none failure=taskCancelled")
            #expect(calls == 64)
        }
    }

    @Test func emitterPassesTheSamePureValueAndStopsAt64EvenWhenReentered() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let value = Value(.ownWork, error: CloudOwnTransportFailure.missingReply)
        var received: [Value] = []
        recorder.emit(value) { outer in
            #expect(recorder.latestValue == outer.formatted)
            received.append(outer)
            // A synchronous reentry proves the callback runs outside the mutex.
            recorder.emit(value) { inner in
                #expect(recorder.latestValue == inner.formatted)
                received.append(inner)
            }
        }
        for _ in 0..<100 { recorder.emit(value) { received.append($0) } }
        #expect(received.count == 64)
        #expect(received.allSatisfy { $0 == value && $0.formatted ==
            "boundary=ownWork action=ownWork failure=own.missingReply" })
    }

    @Test func emitterCapIsSharedAcrossConcurrentFailures() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let received = Mutex<[Value]>([])
        DispatchQueue.concurrentPerform(iterations: 256) { index in
            let boundary: DebugCloudDiagnostics.Boundary = index.isMultiple(of: 2) ? .ownWork : .policyExecute
            recorder.emit(Value(boundary, error: CancellationError())) { value in
                received.withLock { $0.append(value) }
            }
        }
        #expect(received.withLock { $0.count } == 64)
    }

    private func expectFailure(_ error: any Error, _ expected: String) {
        let value = Value(.policyExecute, error: error)
        #expect(value.failure == expected)
        #expect(value.code == nil)
        #expect(value.formatted == "boundary=policyExecute action=none failure=\(expected)")
    }
}

private struct HostileError: Error, CustomNSError, CustomStringConvertible {
    static var errorDomain: String { fatalError("Diagnostic accessed errorDomain") }
    var errorCode: Int { fatalError("Diagnostic accessed errorCode") }
    var errorUserInfo: [String: Any] { fatalError("Diagnostic accessed errorUserInfo") }
    var description: String { fatalError("Diagnostic accessed description") }
}
#endif
