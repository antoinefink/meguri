#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Instrumentation
import Synchronization
import Testing
#if DEBUG
import class MeguriCoreBindings.CloudWorkflow
import func MeguriCoreBindings.testCloudPreparationWitness
import func MeguriCoreBindings.testCloudPrepareRuntimeObserved
#endif

// One prefix preserves the first cause across backend replacement and cleanup.
// Only fixed event labels, booleans and local ordinals may be appended.
nonisolated final class ServiceAppCausalTrace: Sendable {
    private struct State { var enabled = false; var failed = false; var count = 0; var entries: [String] = [] }
    private let state = Mutex(State())
    func start() { state.withLock { $0.enabled = true } }
    func append(_ event: String) {
        state.withLock {
            guard $0.enabled else { return }
            $0.count += 1
            if $0.entries.count < 512 { $0.entries.append("\($0.count):\(event)") }
        }
    }
    @discardableResult func expectation(_ value: Bool, line: UInt = #line) -> Bool {
        append("expectation.\(line)=\(value)")
        if !value { state.withLock { if $0.enabled { $0.failed = true } } }
        return value
    }
    func presence<T>(_ value: T?, line: UInt = #line) -> T? {
        expectation(value != nil, line: line); return value
    }
    func emit(onlyIfFailed: Bool = false) {
        let snapshot = state.withLock { $0 }
        guard snapshot.enabled, !onlyIfFailed || snapshot.failed else { return }
        print("Service owner causal trace=[\(snapshot.entries.joined(separator: " | "))] omitted=\(snapshot.count - snapshot.entries.count)")
    }
}

nonisolated enum ServiceAppExchangePhase: String {
    case aBaseline = "A-baseline", bBaseline = "B-baseline"
    case bRemoteSave = "B-remote-save", aMergedSave = "A-merged-save"
}
nonisolated func serviceAppUptimeMilliseconds() -> UInt64 { DispatchTime.now().uptimeNanoseconds / 1_000_000 }

// Test-local StopEngine identities only. Retaining each original command also
// prevents raw object-identity reuse from conflating later observations.
nonisolated final class ServiceStopEngineTrace: Sendable {
    enum Phase: String, Sendable { case beforeCancel, cancelling, cancelReturned, postCancelDrain, closing }
    enum Boundary: Sendable {
        case inputReturned, begin, end, core(BridgeCall), resultEnqueue, acknowledgementJoined
        var label: String {
            switch self {
            case .inputReturned: "inputReturned"
            case .begin: "begin"
            case .end: "end"
            case .core(let call): "Core.\(call)"
            case .resultEnqueue: "resultEnqueue"
            case .acknowledgementJoined: "acknowledgementJoined"
            }
        }
    }
    private struct ObservedReturn: Sendable {
        let command: CloudPolicyCommand
        let event: CloudPolicyEvent
        let phase: Phase
    }
    private struct State {
        var phase = Phase.beforeCancel
        var returns: [ObservedReturn] = []
        var lines = 0
    }
    private let state = Mutex(State())
    func setPhase(_ phase: Phase) {
        let omitted = state.withLock { value in
            value.phase = phase
            return max(0, value.lines - 256)
        }
        print("Service app heldActive.phase=\(phase.rawValue) omitted=\(omitted)")
    }
    func input(_ event: CloudPolicyEvent, output: CloudPolicyOutput) {
        let commands: [CloudPolicyCommand]
        switch output {
        case .commands(let issued), .coreActionFailed(_, let issued): commands = issued
        default: return
        }
        for command in commands {
            guard case .stopEngine = command.action else { continue }
            state.withLock { value in
                let observation = ObservedReturn(command: command, event: event, phase: value.phase)
                value.returns.append(observation)
            }
            record(command, at: .inputReturned)
        }
    }
    func record(_ command: CloudPolicyCommand, at boundary: Boundary, error: Bool? = nil) {
        guard case .stopEngine(let reason) = command.action else { return }
        let line = state.withLock { state -> String? in
            // Bound emitted diagnostics, not retention of actual handles.
            state.lines += 1
            guard state.lines <= 256 else { return nil }
            let index = state.returns.firstIndex { $0.command.raw === command.raw }
            let observation = index.map { state.returns[$0] }
            let handle = index.map { String($0 + 1) } ?? "unobservedReturn"
            let event = observation.map { Self.label($0.event) } ?? "unobservedReturn"
            let returnPhase = observation?.phase.rawValue ?? "unobservedReturn"
            let failure = error.map { " error=\($0)" } ?? ""
            return "Service app heldActive.stopEngine handle=\(handle) boundary=\(boundary.label) reason=\(reason) phase=\(state.phase.rawValue) observedReturnContext=\(event) observedReturnPhase=\(returnPhase) taskCancelled=\(Task.isCancelled)\(failure)"
        }
        if let line { print(line) }
    }
    private static func label(_ event: CloudPolicyEvent) -> String {
        switch event {
        case .refresh(let cause):
            switch cause {
            case .local: "refresh.local"
            case .cloud: "refresh.cloud"
            case .scan: "refresh.scan"
            case .probe: "refresh.probe"
            }
        case .trackingWriteFailed: "trackingWriteFailed"
        case .settings: "settings"
        case .network: "network"
        case .foreground: "foreground"
        case .background: "background"
        case .backgroundExpiring: "backgroundExpiring"
        case .quitRequested: "quitRequested"
        case .flushTimerFired: "flushTimerFired"
        case .syncNowRequested: "syncNowRequested"
        case .syncNowCancelled: "syncNowCancelled"
        case .volumeSpaceAvailable: "volumeSpaceAvailable"
        case .visibleMedia: "visibleMedia"
        case .operation: "operation"
        case .operationError: "operationError"
        }
    }
}

// Observes the original production backend's command completion. It does not
// drive policy, acknowledge records, or implement a second coordinator.
@MainActor final class ServiceAppBackend: SyncCoordinatorBackend {
    var causalTrace: ServiceAppCausalTrace?
    let production: NativeCollectionCloudBackend
    let workflow: CoreCloudWorkflowService
    var connection: CoreCloudConnectionService? { production.connection }
    var own: CloudOwnExecutor? { production.own }
    // A negative may supply an actual retired Core facade. No state or proof
    // is substituted; attachment must observe its real refresh refusal.
    var attachmentProvider: CoreProviderService?
    var providerService: CoreProviderService? { attachmentProvider ?? production.providerService }
    private(set) var accountReadCount = 0
    func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                           acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) async {
        await production.setProofAdmission(admission, acknowledgeOutputs: acknowledgeOutputs)
    }
    // Observation only: callbacks must record/yield without native calls or reentry.
    var onCommands: (@MainActor ([CloudPolicyCommand]) -> Void)? = nil
    // Return context only: production may merge buffered commands into output.
    // Retain actual handles without claiming this input issued them; no reentry.
    var afterInput: (@MainActor (CloudPolicyEvent, CloudPolicyOutput) -> Void)?
    var beforeExecution: (@MainActor (CloudPolicyCommand) async -> Void)?
    // Passive inspection after natural command/own ACK drain, immediately
    // before the original admitted account completion changes Core authority.
    var beforeAdmittedAccountCompletion: (@MainActor (UInt64) async -> Void)?
    #if DEBUG
    var observeCoreCall: (@Sendable (CloudPolicyCommand, BridgeCall) -> Void)?
    var afterMediaOriginalLookup: (@Sendable (CloudMediaRegistrationBoundary.Observation) async -> Void)?
    var afterSDKRegistration: (@Sendable (String) async -> Void)?
    var afterFailedSDKRetirement: (@Sendable (String) async -> Void)?
    #endif
    // A focused shutdown regression can hold delivery of an actual completed
    // command result; it never substitutes execution or issues a capability.
    var afterExecution: (@MainActor (CloudPolicyCommand, SyncCommandResult) async -> Void)?
    // Negatives register exact issued commands and inspect their actual errors
    // once. Every unregistered failure still trips the strict backend check.
    var expectedCommandFailures: [ObjectIdentifier: @MainActor (SyncCommandResult) -> Void] = [:]
    #if DEBUG
    var originalDiscardFaults: [ObjectIdentifier: @Sendable () throws -> Void] = [:]
    var observeRuntimePreparationRefusals = false

    private nonisolated static func observeRuntimePreparation(
        _ raw: MeguriCoreBindings.CloudWorkflow, _ sink: DebugCloudDiagnostics.RuntimePreparationSink
    ) throws {
        let witness = try MeguriCoreBindings.testCloudPreparationWitness()
        do {
            try MeguriCoreBindings.testCloudPrepareRuntimeObserved(workflow: raw, witness: witness)
        } catch {
            // Read only this invocation's owned witness; preserve the original
            // error even if reading the diagnostic itself is unavailable.
            if let observation = try? witness.observation() {
                switch observation {
                case .missingBinding: sink.record(.missingBinding)
                case .providerAdmissionDenied(let handoff):
                    let value: DebugCloudDiagnostics.RuntimePreparationHandoff
                    switch handoff {
                    case .present: value = .present
                    case .absent: value = .absent
                    case .unknown: value = .unknown
                    }
                    sink.record(.providerAdmissionDenied(handoff: value))
                case .publishingBoundUsabilityRefused: sink.record(.publishingBoundUsabilityRefused)
                case .reconcileUsabilityRefused: sink.record(.reconcileUsabilityRefused)
                }
            }
            throw error
        }
    }
    #endif
    // Runs at the coordinator's existing shutdown boundary, after admission
    // closes and original command tasks receive owned cancellation.
    var beforeQuiesce: (@MainActor () async -> Void)?
    private(set) var quiesced = false
    private let quiescences = AsyncStream<Bool>.makeStream()
    private let starts: AsyncStream<Bool>
    private let started: AsyncStream<Bool>.Continuation
    private let exchanges: AsyncStream<Bool>
    private let exchanged: AsyncStream<Bool>.Continuation
    private let assetRetries: AsyncStream<Bool>
    private let assetRetried: AsyncStream<Bool>.Continuation
    private(set) var assetRetryStarted = 0
    private(set) var assetRetryCompleted = 0
    private(set) var closed = false
    private(set) var closeEntered = 0
    private(set) var closeReturned = 0
    private(set) var closeFailure: String?
    private struct DiagnosticEntry {
        let sequence: UInt64
        let message: String
    }
    private var diagnosticEntries: [DiagnosticEntry] = []
    private var diagnosticSequence: UInt64 = 0
    private var diagnosticBaseline: UInt64 = 0
    private var commandSequence: UInt64 = 0
    // The first observed failure survives ring trimming and later cleanup errors.
    private(set) var firstFailureDiagnostic: String?
    var diagnostics: [String] { diagnosticEntries.map(\.message) }
    var exchangeDiagnostics: [String] {
        diagnosticEntries.filter { $0.sequence > diagnosticBaseline }.map(\.message)
    }
    // Sequence survives buffer trimming; a retained-array index does not.
    func markDiagnosticBaseline() { diagnosticBaseline = diagnosticSequence }
    func trace(_ message: String, emit: Bool = false) {
        diagnosticSequence += 1
        let entry = "at_ms=\(serviceAppUptimeMilliseconds()) \(message)"
        if diagnosticEntries.count == 32 { diagnosticEntries.removeFirst() }
        diagnosticEntries.append(DiagnosticEntry(sequence: diagnosticSequence, message: entry))
        if emit { print("Service app \(entry)") }
    }
    private let wire: ServiceWorkflowWire
    private let mediaCompletions: AsyncStream<ServiceMediaCompletion>
    private let mediaCompleted: AsyncStream<ServiceMediaCompletion>.Continuation
    static func make(wire: ServiceWorkflowWire, factory: ServiceSDKFactory,
                     persisted: @escaping @Sendable () async -> Void,
                     observeOwn: (@Sendable (CloudOwnObservation) -> Void)? = nil,
                     ownTransport: (any CloudOwnTransport)? = nil) async throws -> ServiceAppBackend {
        let workflow = CoreCloudWorkflowService()
        let production = try await NativeCollectionCloudBackend.make(workflow: workflow, factory: factory,
            ownTransport: ownTransport ?? ServiceOwnTransport(wire: wire), mediaTransport: ServiceMediaTransport(wire: wire), persisted: persisted,
            observeOwn: observeOwn)
        return ServiceAppBackend(production: production, workflow: workflow, wire: wire)
    }
    private init(production: NativeCollectionCloudBackend, workflow: CoreCloudWorkflowService, wire: ServiceWorkflowWire) {
        self.production = production; self.workflow = workflow; self.wire = wire
        (mediaCompletions, mediaCompleted) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (starts, started) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (exchanges, exchanged) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (assetRetries, assetRetried) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }
    private enum DiagnosticStage: String {
        case input, inputThrow = "input.throw", executeOutputs = "execute.outputs", executeError = "execute.error"
        case beginAccountReadThrow = "beginAccountRead.throw", readAccountError = "readAccount.error"
        case readAccountOutputs = "readAccount.outputs"
        case closeThrow = "close.throw"
    }
    private enum DiagnosticKind: String { case error, coreActionFailed, applyDeferred }
    @discardableResult
    private func capture(_ error: any Error, stage: DiagnosticStage, kind: DiagnosticKind = .error,
                         action: CloudPolicyAction? = nil, sendFailure: String? = nil, latestMedia: String? = nil,
                         fetchReadFailure: String? = nil, expectedRefusal: Bool = false) -> String {
        let category: String
        switch error {
        case is CancellationError: category = "cancellation"
        case let error as BridgeError:
            // Fixed public taxonomy from DebugCloudDiagnostics.failureTag;
            // associated values must never enter harness diagnostics.
            switch error {
            case .cloudPolicy(let failure):
                switch failure {
                case .closed: category = "cloudPolicy.closed"
                case .wrongOwner: category = "cloudPolicy.wrongOwner"
                case .generationChanged: category = "cloudPolicy.generationChanged"
                case .staleCommand: category = "cloudPolicy.staleCommand"
                case .notCoreAction: category = "cloudPolicy.notCoreAction"
                case .notNativeAction: category = "cloudPolicy.notNativeAction"
                case .coreCapabilityUnavailable: category = "cloudPolicy.coreCapabilityUnavailable"
                case .unqualifiedCompletion: category = "cloudPolicy.unqualifiedCompletion"
                }
            case .cloudConnection(let failure):
                switch failure {
                case .closed: category = "connection.closed"
                case .wrongOwner: category = "connection.wrongOwner"
                case .consumed: category = "connection.consumed"
                case .authorityChanged: category = "connection.authorityChanged"
                case .accountUnavailable: category = "connection.accountUnavailable"
                case .accountMismatch: category = "connection.accountMismatch"
                case .busy: category = "connection.busy"
                case .keyUnavailable: category = "connection.keyUnavailable"
                case .epochIncomplete: category = "connection.epochIncomplete"
                case .unreadableControl: category = "connection.unreadableControl"
                case .notOffered: category = "connection.notOffered"
                case .incomingStagingUnavailable: category = "connection.incomingStagingUnavailable"
                case .mediaApplyUnavailable: category = "connection.mediaApplyUnavailable"
                case .resetRecoveryUnavailable: category = "connection.resetRecoveryUnavailable"
                case .tracking: category = "connection.tracking"
                case .incomplete: category = "connection.incomplete"
                case .fileIdentityUnavailable: category = "connection.fileIdentityUnavailable"
                }
            case .maintenance: category = "bridge.maintenance"
            case .exportSourceChanged: category = "bridge.exportSourceChanged"
            case .exportSourceInvalid: category = "bridge.exportSourceInvalid"
            case .exportFormatLimit: category = "bridge.exportFormatLimit"
            case .deckOptionsInvalid: category = "bridge.deckOptionsInvalid"
            case .stale: category = "bridge.stale"
            case .staleGeneration: category = "bridge.staleGeneration"
            case .undoUnavailable: category = "bridge.undoUnavailable"
            case .notetypeRefused: category = "bridge.notetypeRefused"
            case .deckRefused: category = "bridge.deckRefused"
            case .noteNotFound: category = "bridge.noteNotFound"
            case .notFound: category = "bridge.notFound"
            case .insufficientSpace: category = "bridge.insufficientSpace"
            case .open: category = "bridge.open"
            case .suspendDeadline: category = "bridge.suspendDeadline"
            case .schedOpRefused: category = "bridge.schedOpRefused"
            case .sessionActive: category = "bridge.sessionActive"
            case .invalidTypeAnswerReplacement: category = "bridge.invalidTypeAnswerReplacement"
            case .cancelled: category = "bridge.cancelled"
            case .busy: category = "bridge.busy"
            case .unavailable: category = "bridge.unavailable"
            case .unsupported: category = "bridge.unsupported"
            case .invalidArgument: category = "bridge.invalidArgument"
            case .internal: category = "bridge.internal"
            case .mediaCommitPending: category = "bridge.mediaCommitPending"
            case .commitCompleted: category = "bridge.commitCompleted"
            case .cloudSDKCallbackRejected: category = "bridge.cloudSDKCallbackRejected"
            }
        default: category = "other"
        }
        let actionTag = action.map { " action=\(actionLabel($0))" } ?? ""
        let taskState = Task.isCancelled ? "cancelled" : "active"
        let closePhase = closed ? "returned" : closeFailure != nil ? "failed" : closeEntered > 0 ? "draining" : "notStarted"
        let ownership = SyncCommandTaskContext.current?.shutdownCancelled == true ? "ownedShutdown" : "none"
        #if DEBUG
        let firstSendFailure = sendFailure ?? DebugCloudDiagnostics.sendFailureCapture?.firstValue
        let errorType: String
        if case .startEngine? = action {
            let helperTag: String
            if let helper = error as? CloudServiceProcessFailure {
                switch helper {
                case .configuration: helperTag = "configuration"
                case .launch: helperTag = "launch"
                case .closed: helperTag = "closed"
                case .input: helperTag = "input"
                case .output: helperTag = "output"
                case .malformedReply: helperTag = "malformedReply"
                case .fixtureInputLimit: helperTag = "fixtureInputLimit"
                case .fixtureReplyLimit: helperTag = "fixtureReplyLimit"
                case .fixtureDiagnosticLimit: helperTag = "fixtureDiagnosticLimit"
                case .fixtureAttachmentLimit: helperTag = "fixtureAttachmentLimit"
                }
            } else { helperTag = "" }
            let helperFailure = helperTag.isEmpty ? "" : " helperFailure=\(helperTag)"
            errorType = " errorType=\(String(describing: type(of: error)))\(helperFailure)"
        } else { errorType = "" }
        #else
        let firstSendFailure = sendFailure
        let errorType = ""
        #endif
        let sendSite = firstSendFailure.map { " \($0)" } ?? ""
        let mediaSite = latestMedia.map { " latestMedia=[\($0)]" } ?? ""
        let fetchReadSite = fetchReadFailure.map { " \($0)" } ?? ""
        let diagnostic = "\(stage.rawValue).\(kind.rawValue): \(category)\(actionTag)\(errorType) task=\(taskState) close=\(closePhase) cancellation=\(ownership)\(sendSite)\(mediaSite)\(fetchReadSite)"
        if expectedRefusal {
            trace("qualifiedOriginalRefusal \(diagnostic)")
        } else if firstFailureDiagnostic == nil {
            firstFailureDiagnostic = diagnostic
            trace("firstFailure \(diagnostic)", emit: true)
        } else {
            trace(diagnostic)
        }
        return diagnostic
    }
    private func actionLabel(_ action: CloudPolicyAction) -> String {
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
    private func capture(_ output: CloudPolicyOutput, stage: DiagnosticStage, sendFailure: String? = nil,
                         latestMedia: String? = nil, fetchReadFailure: String? = nil) {
        switch output {
        case .coreActionFailed(let error, let commands):
            capture(error, stage: stage, kind: .coreActionFailed, sendFailure: sendFailure, latestMedia: latestMedia, fetchReadFailure: fetchReadFailure)
            onCommands?(commands)
        case .commands(let commands): onCommands?(commands)
        case .applyDeferred(let error): capture(error, stage: stage, kind: .applyDeferred, sendFailure: sendFailure, latestMedia: latestMedia, fetchReadFailure: fetchReadFailure)
        case .syncNowUnavailable: break
        }
    }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        do {
            let output = try await production.input(event)
            afterInput?(event, output)
            capture(output, stage: .input)
            return output
        } catch {
            capture(error, stage: .inputThrow)
            throw error
        }
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        await execute(command, corePrepared: {})
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult {
        commandSequence += 1
        let sequence = commandSequence
        let start = serviceAppUptimeMilliseconds()
        let label = actionLabel(command.action)
        trace("command.begin ordinal=\(sequence) action=\(label) elapsed_ms=0")
        if case .configureAutomatic(let requested) = command.action {
            trace("command.configureAutomatic ordinal=\(sequence) requested=\(requested)")
        }
        if case .retryAssetEntries = command.action { assetRetryStarted += 1 }
        await beforeExecution?(command)
        #if DEBUG
        let sendCapture = DebugCloudDiagnostics.SendFailureCapture()
        let fetchReadCapture = DebugCloudDiagnostics.FetchReadFailureCapture()
        let mediaRecorder = DebugCloudDiagnostics.Recorder()
        let observe = observeCoreCall
        let discardFault = originalDiscardFaults.removeValue(forKey: ObjectIdentifier(command.raw))
        let preparationInterceptor: CloudRuntimePreparationBoundary.Interceptor? = observeRuntimePreparationRefusals
            ? { @Sendable raw, sink in try Self.observeRuntimePreparation(raw, sink) } : nil
        let result = await MaintenanceCallObservation.$capture.withValue({ observe?(command, $0) }) {
            await CloudFetchProofCompletionFault.$afterOriginalDiscard.withValue(discardFault) {
            await DebugCloudDiagnostics.$sendFailureCapture.withValue(sendCapture) {
                await CloudRuntimePreparationBoundary.$interceptor.withValue(preparationInterceptor) {
                await DebugCloudDiagnostics.$fetchReadFailureCapture.withValue(fetchReadCapture) {
                    await DebugCloudDiagnostics.$mediaRecorder.withValue(mediaRecorder) {
                        await CloudMediaRegistrationBoundary.$afterOriginalLookup.withValue(afterMediaOriginalLookup) {
                            await CloudSDKConstructionBoundary.$registered.withValue(afterSDKRegistration) {
                            await CloudSDKConstructionBoundary.$failedRetirement.withValue(afterFailedSDKRetirement) {
                            await production.execute(command, corePrepared: corePrepared)
                            }
                            }
                        }
                    }
                }
                }
            }
            }
        }
        // Last retained media failure, not necessarily the first. Absence does
        // not establish that no media work occurred during this command.
        let latestMedia = mediaRecorder.latestValue
        let sendFailure = sendCapture.firstValue
        let fetchReadFailure = fetchReadCapture.firstValue
        #else
        let result = await production.execute(command, corePrepared: corePrepared)
        let latestMedia: String? = nil
        let sendFailure: String? = nil
        let fetchReadFailure: String? = nil
        #endif
        await afterExecution?(command, result)
        let fetchReadSite = fetchReadFailure.map { " \($0)" } ?? ""
        trace("command.end ordinal=\(sequence) action=\(label) elapsed_ms=\(serviceAppUptimeMilliseconds() - start) error=\(result.error != nil) deferred=\(result.deferred)\(fetchReadSite)")
        for output in result.outputs { capture(output, stage: .executeOutputs, sendFailure: sendFailure, latestMedia: latestMedia, fetchReadFailure: fetchReadFailure) }
        switch command.action {
        case .startEngine: started.yield(result.error == nil && !result.deferred)
        case .runExchange: exchanged.yield(result.error == nil && !result.deferred)
        case .retryAssetEntries:
            assetRetryCompleted += 1
            assetRetried.yield(result.error == nil && !result.deferred)
        default: break
        }
        if case .resumeMedia = command.action, !result.deferred {
            do {
                if let error = result.error { throw error }
                let inventory = try await CoreMediaService().trackedNames()
                let proof = try await serviceMediaProof(generation: inventory.generation)
                mediaCompleted.yield(.init(proof: .success(proof), receipts: await wire.receipts))
            } catch { mediaCompleted.yield(.init(proof: .failure(error), receipts: await wire.receipts)) }
        }
        if let error = result.error {
            let context = SyncCommandTaskContext.current
            let ownedCancellation = context.map {
                $0.ownsNativeCancellation(result, command: command, backend: self,
                    generation: $0.generation, revision: $0.revision)
            } == true
            let diagnostic = capture(error, stage: .executeError, action: command.action, sendFailure: sendFailure, latestMedia: latestMedia, fetchReadFailure: fetchReadFailure, expectedRefusal: ownedCancellation || result.originalGuardRefusal != nil || result.dependentFetchRefusal != nil || result.fetchProofOutcome?.accountStopCancellation == true)
            if !ownedCancellation, result.originalGuardRefusal == nil, result.dependentFetchRefusal == nil, result.fetchProofOutcome?.accountStopCancellation != true, !(error is CancellationError && SyncCommandTaskContext.current?.shutdownCancelled == true && result.fetchProofOutcome?.preservesFailure != true) {
                if let inspect = expectedCommandFailures.removeValue(forKey: ObjectIdentifier(command.raw)) { inspect(result) }
                else {
                    causalTrace?.expectation(false)
                    Issue.record("Service app command failed; \(diagnostic)")
                }
            }
        }
        return result
    }
    func waitForStart() async throws { try #require(try await serviceAppSignal(starts, stage: "SDK start", trace: causalTrace)) }
    func waitForExchange() async throws { try #require(try await serviceAppSignal(exchanges, stage: "Sync Now exchange", trace: causalTrace)) }
    func waitForAssetRetry() async throws {
        try #require(try await serviceAppSignal(assetRetries, stage: "automatic asset retry command completion"))
    }
    func waitForInstalledMedia(name: String, bytes: Data, receiptsAfter: Int? = nil) async throws {
        let stream = mediaCompletions
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask {
                for await completion in stream {
                    let proof = try completion.proof.get()
                    if let receiptsAfter {
                        let blobs = try completion.receipts.dropFirst(receiptsAfter).compactMap { try $0.blobBytes() }
                        if !blobs.contains(bytes) { continue }
                    }
                    if proof.bytes[name] == bytes { return }
                }
                throw CloudOwnTransportFailure.missingReply
            }
            group.addTask { try await Task.sleep(for: .seconds(10)); throw ServiceAppTimeout(stage: "original media command completion") }
            defer { group.cancelAll() }
            _ = try await group.next()
        }
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        do {
            let read = try await production.beginAccountRead(transition: transition)
            accountReadCount += 1
            return read
        }
        catch {
            capture(error, stage: .beginAccountReadThrow)
            throw error
        }
    }
    func readAccount(_ read: UInt64) async -> SyncCommandResult {
        let complete = await prepareAccountRead(read)
        return await complete(true)
    }
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        let complete = await production.prepareAccountRead(read)
        return { [self] admitted in
            if admitted { await beforeAdmittedAccountCompletion?(read) }
            let result = await complete(admitted)
            if let error = result.error { capture(error, stage: .readAccountError) }
            for output in result.outputs { capture(output, stage: .readAccountOutputs) }
            return result
        }
    }
    func takeAccountReadRequest() async -> Bool { await production.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await production.freezeForSuspend() }
    func resumeAfterCoreResume() async { await production.resumeAfterCoreResume() }
    func quiesce() async throws {
        await beforeQuiesce?()
        try await production.quiesce()
        quiesced = true
        quiescences.continuation.yield(true)
    }
    func waitForQuiesce() async throws {
        try #require(try await serviceAppSignal(quiescences.stream, stage: "backend quiesce"))
    }
    func close() async throws {
        let start = serviceAppUptimeMilliseconds()
        trace("backend.close.begin elapsed_ms=0", emit: true)
        closeEntered += 1
        do { try await production.close() }
        catch {
            closeFailure = capture(error, stage: .closeThrow)
            trace("backend.close.end elapsed_ms=\(serviceAppUptimeMilliseconds() - start) success=false", emit: true)
            throw error
        }
        trace("backend.close.end elapsed_ms=\(serviceAppUptimeMilliseconds() - start) success=true", emit: true)
        closeReturned += 1
        closed = true
        started.finish(); exchanged.finish(); mediaCompleted.finish(); assetRetried.finish()
    }
}
private nonisolated struct ServiceMediaCompletion: Sendable {
    let proof: Result<ServiceMediaProof, any Error>
    let receipts: [ServiceSavedReceipt]
}
private nonisolated struct ServiceAppTimeout: Error { let stage: String }
nonisolated func serviceAppSignal(_ stream: AsyncStream<Bool>, stage: String, trace: ServiceAppCausalTrace? = nil,
                                 failure: AsyncStream<any Error>? = nil, line: UInt = #line) async throws -> Bool {
    var completed = false
    defer { if !completed { trace?.expectation(false, line: line) } }
    return try await withThrowingTaskGroup(of: Bool.self) { group in
        group.addTask {
            for await success in stream { return success }
            throw CloudOwnTransportFailure.missingReply
        }
        group.addTask { try await Task.sleep(for: .seconds(10)); throw ServiceAppTimeout(stage: stage) }
        if let failure {
            group.addTask {
                for await error in failure { throw error }
                try Task.checkCancellation()
                throw CloudOwnTransportFailure.missingReply
            }
        }
        defer { group.cancelAll() }
        let result = try await group.next()
        let value = try serviceRequired(result)
        completed = true
        return value
    }
}

@MainActor final class ServiceAppHarness {
    let root: URL
    let suite = "synthetic-service-app-\(UUID().uuidString)"
    let defaults: UserDefaults
    let wire: ServiceWorkflowWire
    let factory: ServiceSDKFactory
    let causalTrace = ServiceAppCausalTrace()
    private let clock: AppClock
    private let cachedIdentityAvailable: Bool
    private(set) var backends: [ServiceAppBackend] = []
    var observeOwn: (@Sendable (CloudOwnObservation) -> Void)?
    var ownTransport: (any CloudOwnTransport)?
    #if DEBUG
    var observeRuntimePreparationRefusals = false {
        didSet { for backend in backends { backend.observeRuntimePreparationRefusals = observeRuntimePreparationRefusals } }
    }
    #endif
    // Install before preparation and carry observation across real backend
    // handoffs. This callback never drives policy or owns issued capabilities.
    var onCommands: (@MainActor ([CloudPolicyCommand]) -> Void)? {
        didSet { for backend in backends { backend.onCommands = onCommands } }
    }
    // Test-owned admission before a new native backend can resume work.
    var beforeBackend: (@MainActor () async throws -> Void)?
    var afterBackend: (@MainActor (ServiceAppBackend) -> Void)?
    var sync: SyncCoordinator { owner.sync }

    #if DEBUG
    func startCausalTrace() { causalTrace.start(); restoreCausalTraceObserver() }
    func restoreCausalTraceObserver() { sync.observeCausalBoundary = { [causalTrace] boundary, _ in Self.recordCausal(boundary, in: causalTrace) } }
    static func recordCausal(_ boundary: SyncCoordinator.CausalBoundary, in trace: ServiceAppCausalTrace) {
        switch boundary {
        case .accountCompletion, .accountAccepted, .resultReduced, .resumeEntered, .resumeReturned, .resumeRefused, .resumeCancelled:
            trace.append(String(describing: boundary))
        default: break // Other cases can contain opaque command/owner identities.
        }
    }
    #endif
    private func makeSync() -> SyncCoordinator {
        SyncCoordinator(bootstrap: { [cachedIdentityAvailable] in
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-initial-app", environment: .development,
                status: cachedIdentityAvailable ? .available : .couldNotDetermine,
                identity: cachedIdentityAvailable ? Data("synthetic-initial-account".utf8) : nil)
        }, makeBackend: { [self] persisted in
            let allBackendsClosed: Bool = backends.allSatisfy { $0.closed }
            let closeDiagnostics = backends.enumerated().prefix(16).map { index, backend in
                let phases = backend.production.shutdownTrace
                return "index=\(index) closed=\(backend.closed) close-entered=\(backend.closeEntered) "
                    + "close-returned=\(backend.closeReturned) close-failure=\(backend.closeFailure ?? "none") "
                    + "policy.status=\(phases.policyProbe.rawValue) adapter.stop=\(phases.adapterDrain.rawValue) "
                    + "retained-instances=\(phases.retentionCheck.rawValue)"
            }.joined(separator: " | ")
            try #require(causalTrace.expectation(allBackendsClosed),
                "Service app backend close: count=\(backends.count); first 16=[\(closeDiagnostics)]")
            try await beforeBackend?()
            let backend = try await ServiceAppBackend.make(wire: wire, factory: factory, persisted: persisted, observeOwn: observeOwn, ownTransport: ownTransport)
            backend.causalTrace = causalTrace
            backend.onCommands = onCommands
            #if DEBUG
            backend.observeRuntimePreparationRefusals = observeRuntimePreparationRefusals
            #endif
            backends.append(backend)
            afterBackend?(backend)
            return backend
        }, defaults: defaults, clock: .system, directory: nil, observeSignals: false,
           providerDirectory: root.appending(path: "synthetic-installation", directoryHint: .isDirectory))
    }
    lazy var owner: ReviewCollection = {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = clock
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: makeSync(),
            providerDirectory: root.appending(path: "synthetic-installation", directoryHint: .isDirectory))
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        return owner
    }()
    init(root: URL, wire: ServiceWorkflowWire, defaults: UserDefaults? = nil,
         cachedIdentityAvailable: Bool = true,
         clock: AppClock = .fixed(Date(timeIntervalSince1970: 1_735_689_600))) throws {
        self.root = root; self.wire = wire
        self.clock = clock
        self.cachedIdentityAvailable = cachedIdentityAvailable
        factory = ServiceSDKFactory(wire: wire, causalTrace: causalTrace)
        if let defaults { self.defaults = defaults }
        else { self.defaults = try #require(UserDefaults(suiteName: suite)) }
        if defaults == nil {
            self.defaults.set(["enabled": false, "paused": false, "automatic": false, "media": true], forKey: "meguri.cloud.deviceSettings")
        }
    }
    var backend: ServiceAppBackend { get throws { try #require(causalTrace.presence(backends.last)) } }
    func adopt() async throws {
        await owner.start()
        try #require(owner.state == .noFile)
        try await owner.prepareCloudConnectionPreview()
        await sync.waitForConnectionPreview()
        await sync.waitForReplacement()
        await owner.waitForCloudActivation()
        await sync.prepareConnectionAuthority()
        let diagnostics = backends.flatMap { $0.diagnostics }.joined(separator: " | ")
        let failureMessage = "Service app adopt: setupState=\(sync.setupState); backend diagnostics=[\(diagnostics)]"
        try #require(causalTrace.expectation(owner.state == .ready && !sync.operationFailed), "\(failureMessage)")
        try await backend.waitForStart()
        try backend.markDiagnosticBaseline()
    }
    func reopen() async throws {
        await owner.start()
        await owner.waitForCloudActivation()
        await sync.prepareConnectionAuthority()
        let diagnostics = backends.flatMap { $0.diagnostics }.joined(separator: " | ")
        let failureMessage = "Service app reopen: setupState=\(sync.setupState); backend diagnostics=[\(diagnostics)]"
        try #require(causalTrace.expectation(owner.state == .ready && !sync.operationFailed), "\(failureMessage)")
        try await backend.waitForStart()
        try backend.markDiagnosticBaseline()
    }
    func exchange(phase: ServiceAppExchangePhase? = nil) async throws {
        let original = try backend
        let label = phase?.rawValue ?? "unspecified"
        let start = serviceAppUptimeMilliseconds()
        let completedBefore = wire.process.transportExchanges().count
        original.trace("exchange.begin phase=\(label) elapsed_ms=0", emit: true)
        var succeeded = false
        defer {
            original.trace("exchange.end phase=\(label) elapsed_ms=\(serviceAppUptimeMilliseconds() - start) success=\(succeeded)", emit: true)
            if !succeeded {
                print("Service app exchange phase=\(label) \(serviceTransportSummary(wire.process, after: completedBefore))")
            }
        }
        do {
            sync.receive(.network(reachable: true)); sync.syncNow()
            try await original.waitForExchange()
            await sync.waitForInputs()
            let diagnostics = original.exchangeDiagnostics.joined(separator: " | ")
            try #require(!sync.operationFailed, "Service app exchange phase=\(label): backend diagnostics=[\(diagnostics)]")
            succeeded = true
        } catch is ServiceAppTimeout {
            let elapsed = serviceAppUptimeMilliseconds() - start
            original.trace("exchange.timeout phase=\(label) elapsed_ms=\(elapsed)", emit: true)
            throw ServiceAppTimeout(stage: "Sync Now exchange phase=\(label) elapsed_ms=\(elapsed)")
        }
    }
    func cleanup() async { _ = await owner.close() }

    // Only the failing asset fixture owns this process and may interrupt it.
    // Join it before the single final owner close so accepted IPC can finish.
    func cleanupAfterServiceFailure() async {
        causalTrace.append("cleanupAfterServiceFailure.enter")
        causalTrace.emit()
        defer { causalTrace.emit() }
        let start = serviceAppUptimeMilliseconds()
        func record(_ message: String) {
            let entry = "\(message) elapsed_ms=\(serviceAppUptimeMilliseconds() - start)"
            if let backend = backends.last { backend.trace(entry, emit: true) }
            else { print("Service app at_ms=\(serviceAppUptimeMilliseconds()) \(entry)") }
        }
        func reportTrace() {
            for (index, backend) in backends.enumerated() {
                print("Service app backend ordinal=\(index) trace=[\(backend.diagnostics.joined(separator: " | "))]")
            }
        }
        reportTrace()
        causalTrace.append("cleanup.process.cancel.begin")
        record("cleanup.process.cancel.begin")
        let exit = await wire.process.cancel()
        causalTrace.append("cleanup.process.cancel.end joined=\(exit.joined)")
        record("cleanup.process.cancel.end joined=\(exit.joined) status=\(exit.status) io_failed=\(exit.ioFailed)")
        #expect(causalTrace.expectation(exit.joined), "Failing asset fixture must join its owned Service process")
        causalTrace.append("cleanup.owner.close.begin")
        record("cleanup.owner.close.begin")
        let closed = await owner.close()
        causalTrace.append("cleanup.owner.close.end closed=\(closed)")
        record("cleanup.owner.close.end closed=\(closed)")
        #expect(causalTrace.expectation(closed), "Failing asset fixture final owner.close returned false; host may remain retained")
        reportTrace()
        print(serviceTransportSummary(wire.process))
    }
}
#endif
