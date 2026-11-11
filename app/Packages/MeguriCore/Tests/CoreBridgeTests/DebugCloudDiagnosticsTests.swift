#if DEBUG
import Dispatch
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

struct DebugCloudDiagnosticsTests {
    private typealias Value = DebugCloudDiagnostics.Value

    @Test func processSentCountsRequireTheirOwnObservationsAndStaySeparateFromFetchFacts() {
        let recorder = DebugCloudDiagnostics.FetchRecorder()
        let empty = "processSent{sentC=0 sentM=0 sendFailC=0 sendFailM=0 overflow=0}"
        #expect(recorder.sentFormatted == empty)
        recorder.recordFetchOptions(requestedCollection: true, offeredCollection: true)
        recorder.recordServerChangeObservations(database: true, databaseCollection: true,
            unfetched: true, unfetchedCollection: true)
        for counter in DebugCloudDiagnostics.FetchCounter.allCases { recorder.record(counter) }
        #expect(recorder.sentFormatted == empty)
        let fetch = recorder.formatted
        for counter in DebugCloudDiagnostics.SentCounter.allCases { recorder.recordSent(counter) }
        #expect(recorder.sentFormatted == "processSent{sentC=1 sentM=1 sendFailC=1 sendFailM=1 overflow=0}")
        #expect(recorder.formatted == fetch)
        #expect(DebugCloudDiagnostics.FetchRecorder().sentFormatted == empty)
    }

    @Test func processSentCountsSaturateEveryBoundaryAndKeepTheirOwnBoundAndOverflow() {
        for counter in DebugCloudDiagnostics.SentCounter.allCases {
            let recorder = DebugCloudDiagnostics.FetchRecorder(initialCount: .max - 1)
            let fetch = recorder.formatted
            recorder.recordSent(counter)
            #expect(recorder.sentFormatted.contains("\(counter.label)=\(UInt64.max) "))
            #expect(recorder.sentFormatted.hasSuffix("overflow=0}"))
            recorder.recordSent(counter)
            recorder.recordSent(counter, count: .max)
            recorder.recordSent(counter, count: 0)
            #expect(recorder.sentFormatted.contains("\(counter.label)=\(UInt64.max) "))
            #expect(recorder.sentFormatted.hasSuffix("overflow=1}"))
            #expect(recorder.formatted == fetch)
        }
        let bounded = DebugCloudDiagnostics.FetchRecorder(initialCount: .max)
        for counter in DebugCloudDiagnostics.SentCounter.allCases { bounded.recordSent(counter) }
        let original = bounded.sentFormatted
        DispatchQueue.concurrentPerform(iterations: 256) { _ in
            for counter in DebugCloudDiagnostics.SentCounter.allCases { bounded.recordSent(counter) }
        }
        #expect(bounded.sentFormatted == original && original.utf8.count < 160)
        #expect(bounded.formatted.utf8.count < 256)
    }

    @Test func processFetchCountsStartAtZeroAndKeepBoundariesIndependent() {
        let recorder = DebugCloudDiagnostics.FetchRecorder()
        #expect(recorder.formatted == "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}")
        recorder.record(.will)
        recorder.record(.zoneC)
        recorder.record(.modC, count: 3)
        recorder.record(.modR, count: 2)
        recorder.record(.coreC, count: 2)
        recorder.record(.refused)
        recorder.record(.zoneFail)
        recorder.record(.coreC, count: 0)
        #expect(recorder.formatted == "processAll{will=1 zoneC=1 modC=3 modR=2 coreC=2 refused=1 zoneFail=1 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}")
        #expect(DebugCloudDiagnostics.FetchRecorder().formatted == "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}")
    }

    @Test func processFetchCountsSaturateEveryBoundaryAndKeepOverflowSticky() {
        for counter in DebugCloudDiagnostics.FetchCounter.allCases {
            let recorder = DebugCloudDiagnostics.FetchRecorder(initialCount: .max - 1)
            recorder.record(counter)
            #expect(recorder.formatted.contains("\(counter.label)=\(UInt64.max) "))
            #expect(recorder.formatted.hasSuffix("overflow=0}"))
            recorder.record(counter)
            #expect(recorder.formatted.contains("\(counter.label)=\(UInt64.max) "))
            #expect(recorder.formatted.hasSuffix("overflow=1}"))
            recorder.record(counter, count: .max)
            recorder.record(counter, count: 0)
            #expect(recorder.formatted.contains("\(counter.label)=\(UInt64.max) "))
            #expect(recorder.formatted.hasSuffix("overflow=1}"))
        }
        let bounded = DebugCloudDiagnostics.FetchRecorder(initialCount: .max)
        for counter in DebugCloudDiagnostics.FetchCounter.allCases { bounded.record(counter) }
        bounded.recordFetchOptions(requestedCollection: true, offeredCollection: true)
        bounded.recordFetchOptions(requestedCollection: true, offeredCollection: false)
        bounded.recordServerChangeObservations(database: true, databaseCollection: true, unfetched: true, unfetchedCollection: true)
        let original = bounded.formatted
        for _ in 0..<1_000 {
            for counter in DebugCloudDiagnostics.FetchCounter.allCases { bounded.record(counter) }
        }
        #expect(bounded.formatted == original && original.utf8.count < 256)
    }

    @Test func processFetchScopeFlagsStartFalseAndORRetainIndependentCallbacksUntilFreshRecorder() {
        let empty = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}"
        for (requested, offered) in [(false, false), (false, true), (true, true)] {
            let recorder = DebugCloudDiagnostics.FetchRecorder()
            recorder.recordFetchOptions(requestedCollection: requested, offeredCollection: offered)
            #expect(recorder.formatted == "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=\(requested ? 1 : 0) outC=\(offered ? 1 : 0) cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}")
        }
        for inputFirst in [false, true] {
            let recorder = DebugCloudDiagnostics.FetchRecorder()
            #expect(recorder.formatted == empty)
            recorder.recordFetchOptions(requestedCollection: false, offeredCollection: false)
            #expect(recorder.formatted == empty)
            recorder.recordFetchOptions(requestedCollection: inputFirst, offeredCollection: !inputFirst)
            let first = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=\(inputFirst ? 1 : 0) outC=\(inputFirst ? 0 : 1) cut=\(inputFirst ? 1 : 0) db=0 dbC=0 un=0 unC=0 overflow=0}"
            #expect(recorder.formatted == first)
            recorder.recordFetchOptions(requestedCollection: false, offeredCollection: false)
            #expect(recorder.formatted == first)
            // Separate callbacks may establish the two flags; they are not paired evidence.
            recorder.recordFetchOptions(requestedCollection: !inputFirst, offeredCollection: inputFirst)
            let both = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=1 outC=1 cut=1 db=0 dbC=0 un=0 unC=0 overflow=0}"
            #expect(recorder.formatted == both)
            recorder.recordFetchOptions(requestedCollection: false, offeredCollection: false)
            recorder.record(.will, count: 0)
            #expect(recorder.formatted == both)
            for (requested, offered) in [(false, true), (true, true)] {
                recorder.recordFetchOptions(requestedCollection: requested, offeredCollection: offered)
                #expect(recorder.formatted == both)
            }
            #expect(DebugCloudDiagnostics.FetchRecorder().formatted == empty)
        }
    }

    @Test func processServerChangeFlagsStartFalseAndORRetainUntilFreshRecorder() {
        let empty = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}"
        let recorder = DebugCloudDiagnostics.FetchRecorder()
        #expect(recorder.formatted == empty)
        recorder.recordServerChangeObservations()
        #expect(recorder.formatted == empty)
        // A database callback need not discover the collection zone.
        recorder.recordServerChangeObservations(database: true)
        let databaseOnly = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=1 dbC=0 un=0 unC=0 overflow=0}"
        #expect(recorder.formatted == databaseOnly)
        recorder.recordServerChangeObservations()
        #expect(recorder.formatted == databaseOnly)
        // A later noncollection snapshot establishes only any-zone knowledge.
        recorder.recordServerChangeObservations(unfetched: true)
        let noncollection = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=1 dbC=0 un=1 unC=0 overflow=0}"
        #expect(recorder.formatted == noncollection)
        recorder.recordServerChangeObservations()
        #expect(recorder.formatted == noncollection)
        recorder.recordServerChangeObservations(unfetched: true, unfetchedCollection: true)
        let unfetchedCollection = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=1 dbC=0 un=1 unC=1 overflow=0}"
        #expect(recorder.formatted == unfetchedCollection)
        recorder.recordServerChangeObservations()
        #expect(recorder.formatted == unfetchedCollection)
        recorder.recordServerChangeObservations(database: true, databaseCollection: true)
        let all = "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=1 dbC=1 un=1 unC=1 overflow=0}"
        #expect(recorder.formatted == all)
        recorder.recordServerChangeObservations()
        recorder.recordFetchOptions(requestedCollection: false, offeredCollection: false)
        recorder.record(.will, count: 0)
        #expect(recorder.formatted == all)
        #expect(DebugCloudDiagnostics.FetchRecorder().formatted == empty)

        // Server-change knowledge can also precede database discovery.
        let reverse = DebugCloudDiagnostics.FetchRecorder()
        reverse.recordServerChangeObservations(unfetched: true)
        #expect(reverse.formatted == "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=0 dbC=0 un=1 unC=0 overflow=0}")
        reverse.recordServerChangeObservations(database: true, databaseCollection: true)
        #expect(reverse.formatted == "processAll{will=0 zoneC=0 modC=0 modR=0 coreC=0 refused=0 zoneFail=0 inC=0 outC=0 cut=0 db=1 dbC=1 un=1 unC=0 overflow=0}")
        reverse.recordServerChangeObservations()
        reverse.recordServerChangeObservations(unfetched: true, unfetchedCollection: true)
        reverse.recordServerChangeObservations()
        #expect(reverse.formatted == all)
        #expect(recorder.formatted == all)
        #expect(DebugCloudDiagnostics.FetchRecorder().formatted == empty)
    }

    @Test func processFetchCountsSynchronizeConcurrentOriginalBoundaries() {
        let recorder = DebugCloudDiagnostics.FetchRecorder()
        DispatchQueue.concurrentPerform(iterations: 256) { _ in
            for counter in DebugCloudDiagnostics.FetchCounter.allCases { recorder.record(counter) }
        }
        #expect(recorder.formatted == "processAll{will=256 zoneC=256 modC=256 modR=256 coreC=256 refused=256 zoneFail=256 inC=0 outC=0 cut=0 db=0 dbC=0 un=0 unC=0 overflow=0}")
    }

    @Test func pageEpochStagesMapAndFormatOnlyFiniteTags() {
        let cases: [(CloudAdoptionFailureDiagnostic.Stage, String)] = [
            (.init(.pageEpochShape), "pageEpochShape"),
            (.init(.pageEpochData), "pageEpochData"),
            (.init(.pageEpochSystemFields), "pageEpochSystemFields")
        ]
        for (stage, tag) in cases {
            let snapshot = CloudAdoptionFailureDiagnostic(
                task: .fetch, lastEnteredStage: stage,
                primaryCategory: .validation, cleanupCategory: .fileIdentityUnavailable)
            #expect(snapshot.lastEnteredStage.rawValue == tag)
            let value = Value.adoption(snapshot)
            #expect(value.formatted == "boundary=connectionFailure action=adoptionResult task=fetch lastEnteredStage=\(tag) primary=validation cleanup=fileIdentityUnavailable")
            #expect(value.code == nil && value.zone == nil && value.kind == nil)
        }
    }

    @Test func adoptionFailureTagsKeepPrimaryAndCleanupSeparate() {
        let cases: [(CloudAdoptionFailureDiagnostic.Category?, CloudAdoptionFailureDiagnostic.Category?, String)] = [
            (.validation, nil, "primary=validation cleanup=none"),
            (nil, .fileIdentityUnavailable, "primary=none cleanup=fileIdentityUnavailable"),
            (.validation, .fileIdentityUnavailable, "primary=validation cleanup=fileIdentityUnavailable")
        ]
        for (primary, cleanup, suffix) in cases {
            let snapshot = CloudAdoptionFailureDiagnostic(
                task: .fetchPayload, lastEnteredStage: .receivedValidation,
                primaryCategory: primary, cleanupCategory: cleanup)
            let value = Value.adoption(snapshot)
            #expect(value.formatted == "boundary=connectionFailure action=adoptionResult task=fetchPayload lastEnteredStage=receivedValidation \(suffix)")
            #expect(value.code == nil && value.zone == nil && value.kind == nil)
            let recorder = DebugCloudDiagnostics.Recorder()
            var logged: [Value] = []
            recorder.emit(value) { logged.append($0) }
            #expect(logged == [value])
        }
    }

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

    @Test func returnedFailuresHaveFiniteBoundariesAndNoCommandAction() {
        for boundary: DebugCloudDiagnostics.Boundary in [.commandResult, .coreActionFailed, .applyDeferred] {
            let value = Value(boundary, action: .prioritizeMedia(names: [UUID().uuidString]),
                              error: BridgeError.cancelled)
            #expect(value.formatted == "boundary=\(boundary.rawValue) action=none failure=bridge.cancelled")
            #expect(value.zone == nil && value.kind == nil && value.code == nil)
        }
    }

    @Test func statusProjectsEveryClassAndLogicalZoneAndKind() {
        let failures: [(CloudPolicyOperationError, String)] = [
            (.noAccount, "noAccount"), (.configuration, "configuration"), (.offline, "offline"),
            (.throttled, "throttled"), (.quotaFull, "quotaFull"), (.localStorageFull, "localStorageFull"),
            (.trackingError, "trackingError"), (.conflictRecovery, "conflictRecovery"),
            (.unsupportedSchema, "unsupportedSchema"), (.other, "other")
        ]
        let zones: [(CloudPolicyZone, String)] = [(.root, "root"), (.collection, "collection"), (.media, "media")]
        let kinds: [(CloudPolicyOperationKind, String)] = [(.send, "send"), (.fetch, "fetch"), (.read, "read")]
        for (failure, tag) in failures {
            for (zone, zoneTag) in zones {
                for (kind, kindTag) in kinds {
                    let value = Value.status(failure, zone: zone, kind: kind, code: 20)
                    #expect(value.boundary == .policyStatus && value.action == "none")
                    #expect(value.failure == tag && value.zone == zoneTag && value.kind == kindTag)
                    #expect(value.formatted ==
                        "boundary=policyStatus action=none failure=\(tag) zone=\(zoneTag) kind=\(kindTag) code=20")
                }
            }
        }
    }

    @Test func statusUsesOnlyExactlyBoundedCode() {
        for code in [Int64(Int32.min), -1, 0, Int64(Int32.max)] {
            let value = Value.status(.other, zone: .root, kind: .read, code: code)
            #expect(value.code == Int32(exactly: code))
            #expect(value.formatted == "boundary=policyStatus action=none failure=other zone=root kind=read code=\(code)")
        }
        for code in [Int64.min, Int64(Int32.min) - 1, Int64(Int32.max) + 1, Int64.max] {
            let value = Value.status(.other, zone: .root, kind: .read, code: code)
            #expect(value.code == nil)
            #expect(value.formatted == "boundary=policyStatus action=none failure=other zone=root kind=read")
        }
    }

    @Test func statusNilDedupResetAndInterleavedCommandsUseOneSafeLatestValue() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let status = Value.status(.offline, zone: .media, kind: .fetch, code: 4)
        let command = Value(.commandResult, error: HostileError())
        var received: [Value] = []
        recorder.emitStatus(nil) { received.append($0) }
        #expect(recorder.latestValue == nil && received.isEmpty)
        recorder.emitStatus(status) { value in
            #expect(recorder.latestValue == value.formatted)
            received.append(value)
            // Status callbacks also permit synchronous reads and reentry.
            recorder.emitStatus(status) { received.append($0) }
        }
        recorder.emitStatus(status) { received.append($0) }
        recorder.emit(command) { received.append($0) }
        #expect(recorder.latestValue == command.formatted)
        recorder.emitStatus(status) { received.append($0) }
        #expect(received == [status, command])
        #expect(recorder.latestValue == status.formatted)
        recorder.emitStatus(nil) { received.append($0) }
        #expect(recorder.latestValue == status.formatted)
        recorder.emitStatus(status) { received.append($0) }
        #expect(received == [status, command, status])
    }

    @Test func statusDedupUsesTheWholeSafeTuple() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let values: [Value] = [
            .status(.offline, zone: .root, kind: .read, code: 1),
            .status(.noAccount, zone: .root, kind: .read, code: 1),
            .status(.noAccount, zone: .collection, kind: .read, code: 1),
            .status(.noAccount, zone: .collection, kind: .fetch, code: 1),
            .status(.noAccount, zone: .collection, kind: .fetch, code: 2),
            .status(.noAccount, zone: .collection, kind: .fetch, code: .max)
        ]
        var received: [Value] = []
        for value in values {
            recorder.emitStatus(value) { received.append($0) }
            recorder.emitStatus(value) { received.append($0) }
        }
        // Distinct out-of-range codes project to the same safe tuple.
        recorder.emitStatus(.status(.noAccount, zone: .collection, kind: .fetch, code: .min)) { received.append($0) }
        recorder.emitStatus(values[0]) { received.append($0) }
        #expect(received == values + [values[0]])
    }

    @Test func statusAndCommandsShareThe64SinkCapAndKeepUpdatingLatest() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let command = Value(.applyDeferred, error: BridgeError.cancelled)
        var calls = 0
        for code in 0..<32 {
            let status = Value.status(.offline, zone: .media, kind: .send, code: Int64(code))
            recorder.emitStatus(status) { _ in calls += 1 }
            recorder.emitStatus(status) { _ in calls += 1 }
            recorder.emit(command) { _ in calls += 1 }
        }
        #expect(calls == 64)
        let latest = Value.status(.quotaFull, zone: .collection, kind: .send, code: 25)
        recorder.emitStatus(latest) { _ in calls += 1 }
        #expect(recorder.latestValue == latest.formatted)
        recorder.emitStatus(nil) { _ in calls += 1 }
        recorder.emitStatus(latest) { _ in calls += 1 }
        recorder.emit(command) { _ in calls += 1 }
        #expect(calls == 64 && recorder.latestValue == command.formatted)
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

    @Test func mediaTupleSurvivesPolicyZeroAndTheExistingSinkCap() {
        let recorder = DebugCloudDiagnostics.Recorder()
        let tuple = DebugCloudDiagnostics.MediaValue(request: .download, stage: .assetStaging, category: .sourceUnavailable)
        let old = Value(.policyExecute, error: HostileError())
        var calls = 0
        recorder.emit(old) { _ in calls += 1 }
        recorder.retainMedia(tuple)
        #expect(recorder.latestValue == "\(old.formatted) \(tuple.formatted)")
        let status = Value.status(.other, zone: .media, kind: .read, code: 0)
        recorder.emitStatus(status) { _ in calls += 1 }
        #expect(status.code == 0)
        #expect(recorder.latestValue == "boundary=policyStatus action=none failure=other zone=media kind=read code=0 \(tuple.formatted)")
        for _ in 0..<100 { recorder.emit(old) { _ in calls += 1 } }
        #expect(calls == 64)
        recorder.emitStatus(status) { _ in calls += 1 }
        recorder.emitStatus(nil) { _ in calls += 1 }
        #expect(calls == 64)
        #expect(recorder.latestValue == "\(status.formatted) \(tuple.formatted)")
        let replacement = DebugCloudDiagnostics.MediaValue(request: .reads, stage: .recordCallback, category: .duplicateCallback)
        recorder.retainMedia(replacement)
        #expect(recorder.latestValue == "\(status.formatted) \(replacement.formatted)")
        #expect(DebugCloudDiagnostics.Recorder().latestValue == nil)
        #expect(DebugCloudDiagnostics.MediaCategory.local(HostileError()) == .other)
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
