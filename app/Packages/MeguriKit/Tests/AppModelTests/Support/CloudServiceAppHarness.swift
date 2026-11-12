#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Instrumentation
import Testing

nonisolated enum ServiceAppExchangePhase: String {
    case aBaseline = "A-baseline", bBaseline = "B-baseline"
    case bRemoteSave = "B-remote-save", aMergedSave = "A-merged-save"
}
nonisolated func serviceAppUptimeMilliseconds() -> UInt64 { DispatchTime.now().uptimeNanoseconds / 1_000_000 }

// Observes the original production backend's command completion. It does not
// drive policy, acknowledge records, or implement a second coordinator.
@MainActor final class ServiceAppBackend: SyncCoordinatorBackend {
    let production: NativeCollectionCloudBackend
    let workflow: CoreCloudWorkflowService
    var connection: CoreCloudConnectionService? { production.connection }
    var own: CloudOwnExecutor? { production.own }
    func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                           acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) async {
        await production.setProofAdmission(admission, acknowledgeOutputs: acknowledgeOutputs)
    }
    // Observation only: callbacks must record/yield without native calls or reentry.
    var onCommands: (@MainActor ([CloudPolicyCommand]) -> Void)? = nil
    var beforeExecution: (@MainActor (CloudPolicyCommand) async -> Void)?
    #if DEBUG
    var observeCoreCall: (@Sendable (CloudPolicyCommand, BridgeCall) -> Void)?
    #endif
    // A focused shutdown regression can hold delivery of an actual completed
    // command result; it never substitutes execution or issues a capability.
    var afterExecution: (@MainActor (CloudPolicyCommand, SyncCommandResult) async -> Void)?
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
                         action: CloudPolicyAction? = nil, sendFailure: String? = nil, latestMedia: String? = nil, expectedRefusal: Bool = false) -> String {
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
            }
        default: category = "other"
        }
        let actionTag = action.map { " action=\(actionLabel($0))" } ?? ""
        let taskState = Task.isCancelled ? "cancelled" : "active"
        let closePhase = closed ? "returned" : closeFailure != nil ? "failed" : closeEntered > 0 ? "draining" : "notStarted"
        let ownership = SyncCommandTaskContext.current?.shutdownCancelled == true ? "ownedShutdown" : "none"
        #if DEBUG
        let firstSendFailure = sendFailure ?? DebugCloudDiagnostics.sendFailureCapture?.firstValue
        #else
        let firstSendFailure = sendFailure
        #endif
        let sendSite = firstSendFailure.map { " \($0)" } ?? ""
        let mediaSite = latestMedia.map { " latestMedia=[\($0)]" } ?? ""
        let diagnostic = "\(stage.rawValue).\(kind.rawValue): \(category)\(actionTag) task=\(taskState) close=\(closePhase) cancellation=\(ownership)\(sendSite)\(mediaSite)"
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
    private func capture(_ output: CloudPolicyOutput, stage: DiagnosticStage, sendFailure: String? = nil, latestMedia: String? = nil) {
        switch output {
        case .coreActionFailed(let error, let commands):
            capture(error, stage: stage, kind: .coreActionFailed, sendFailure: sendFailure, latestMedia: latestMedia)
            onCommands?(commands)
        case .commands(let commands): onCommands?(commands)
        case .applyDeferred(let error): capture(error, stage: stage, kind: .applyDeferred, sendFailure: sendFailure, latestMedia: latestMedia)
        case .syncNowUnavailable: break
        }
    }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        do {
            let output = try await production.input(event)
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
        let mediaRecorder = DebugCloudDiagnostics.Recorder()
        let observe = observeCoreCall
        let result = await MaintenanceCallObservation.$capture.withValue({ observe?(command, $0) }) {
            await DebugCloudDiagnostics.$sendFailureCapture.withValue(sendCapture) {
                await DebugCloudDiagnostics.$mediaRecorder.withValue(mediaRecorder) {
                    await production.execute(command, corePrepared: corePrepared)
                }
            }
        }
        // Last retained media failure, not necessarily the first. Absence does
        // not establish that no media work occurred during this command.
        let latestMedia = mediaRecorder.latestValue
        let sendFailure = sendCapture.firstValue
        #else
        let result = await production.execute(command, corePrepared: corePrepared)
        let latestMedia: String? = nil
        let sendFailure: String? = nil
        #endif
        await afterExecution?(command, result)
        trace("command.end ordinal=\(sequence) action=\(label) elapsed_ms=\(serviceAppUptimeMilliseconds() - start) error=\(result.error != nil) deferred=\(result.deferred)")
        for output in result.outputs { capture(output, stage: .executeOutputs, sendFailure: sendFailure, latestMedia: latestMedia) }
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
            let diagnostic = capture(error, stage: .executeError, action: command.action, sendFailure: sendFailure, latestMedia: latestMedia, expectedRefusal: result.originalGuardRefusal != nil)
            if result.originalGuardRefusal == nil, !(error is CancellationError && SyncCommandTaskContext.current?.shutdownCancelled == true) {
                Issue.record("Service app command failed; \(diagnostic)")
            }
        }
        return result
    }
    func waitForStart() async throws { try #require(try await serviceAppSignal(starts, stage: "SDK start")) }
    func waitForExchange() async throws { try #require(try await serviceAppSignal(exchanges, stage: "Sync Now exchange")) }
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
        do { return try await production.beginAccountRead(transition: transition) }
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
nonisolated func serviceAppSignal(_ stream: AsyncStream<Bool>, stage: String) async throws -> Bool {
    try await withThrowingTaskGroup(of: Bool.self) { group in
        group.addTask {
            for await success in stream { return success }
            throw CloudOwnTransportFailure.missingReply
        }
        group.addTask { try await Task.sleep(for: .seconds(10)); throw ServiceAppTimeout(stage: stage) }
        defer { group.cancelAll() }
        let result = try await group.next()
        return try serviceRequired(result)
    }
}

@MainActor final class ServiceAppHarness {
    let root: URL
    let suite = "synthetic-service-app-\(UUID().uuidString)"
    let defaults: UserDefaults
    let wire: ServiceWorkflowWire
    let factory: ServiceSDKFactory
    private let clock: AppClock
    private(set) var backends: [ServiceAppBackend] = []
    var observeOwn: (@Sendable (CloudOwnObservation) -> Void)?
    var ownTransport: (any CloudOwnTransport)?
    // Test-owned admission before a new native backend can resume work.
    var beforeBackend: (@MainActor () async throws -> Void)?
    lazy var sync = makeSync()

    private func makeSync() -> SyncCoordinator {
        SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-initial-app", environment: .development,
                status: .available, identity: Data("synthetic-initial-account".utf8))
        }, makeBackend: { [self] persisted in
            let allBackendsClosed: Bool = backends.allSatisfy { $0.closed }
            let closeDiagnostics = backends.enumerated().prefix(16).map { index, backend in
                let phases = backend.production.shutdownTrace
                return "index=\(index) closed=\(backend.closed) close-entered=\(backend.closeEntered) "
                    + "close-returned=\(backend.closeReturned) close-failure=\(backend.closeFailure ?? "none") "
                    + "policy.status=\(phases.policyProbe.rawValue) adapter.stop=\(phases.adapterDrain.rawValue) "
                    + "retained-instances=\(phases.retentionCheck.rawValue)"
            }.joined(separator: " | ")
            try #require(allBackendsClosed,
                "Service app backend close: count=\(backends.count); first 16=[\(closeDiagnostics)]")
            try await beforeBackend?()
            let backend = try await ServiceAppBackend.make(wire: wire, factory: factory, persisted: persisted, observeOwn: observeOwn, ownTransport: ownTransport)
            backends.append(backend)
            return backend
        }, defaults: defaults, clock: .system, directory: nil, observeSignals: false)
    }
    lazy var owner: ReviewCollection = {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = clock
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        return owner
    }()
    init(root: URL, wire: ServiceWorkflowWire, defaults: UserDefaults? = nil,
         clock: AppClock = .fixed(Date(timeIntervalSince1970: 1_735_689_600))) throws {
        self.root = root; self.wire = wire
        self.clock = clock
        factory = ServiceSDKFactory(wire: wire)
        if let defaults { self.defaults = defaults }
        else { self.defaults = try #require(UserDefaults(suiteName: suite)) }
        if defaults == nil {
            self.defaults.set(["enabled": false, "paused": false, "automatic": false, "media": true], forKey: "meguri.cloud.deviceSettings")
        }
    }
    var backend: ServiceAppBackend { get throws { try #require(backends.last) } }
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
        try #require(owner.state == .ready && !sync.operationFailed, "\(failureMessage)")
        try await backend.waitForStart()
        try backend.markDiagnosticBaseline()
    }
    func reopen() async throws {
        await owner.start()
        await owner.waitForCloudActivation()
        await sync.prepareConnectionAuthority()
        let diagnostics = backends.flatMap { $0.diagnostics }.joined(separator: " | ")
        let failureMessage = "Service app reopen: setupState=\(sync.setupState); backend diagnostics=[\(diagnostics)]"
        try #require(owner.state == .ready && !sync.operationFailed, "\(failureMessage)")
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
        record("cleanup.process.cancel.begin")
        let exit = await wire.process.cancel()
        record("cleanup.process.cancel.end joined=\(exit.joined) status=\(exit.status) io_failed=\(exit.ioFailed)")
        #expect(exit.joined, "Failing asset fixture must join its owned Service process")
        record("cleanup.owner.close.begin")
        let closed = await owner.close()
        record("cleanup.owner.close.end closed=\(closed)")
        #expect(closed, "Failing asset fixture final owner.close returned false; host may remain retained")
        reportTrace()
        print(serviceTransportSummary(wire.process))
    }
}
#endif
