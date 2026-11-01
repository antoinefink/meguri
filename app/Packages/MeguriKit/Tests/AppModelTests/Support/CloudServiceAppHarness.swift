#if os(macOS)
@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

// Observes the original production backend's command completion. It does not
// drive policy, acknowledge records, or implement a second coordinator.
@MainActor final class ServiceAppBackend: SyncCoordinatorBackend {
    let production: NativeCollectionCloudBackend
    let workflow: CoreCloudWorkflowService
    var connection: CoreCloudConnectionService? { production.connection }
    var own: CloudOwnExecutor? { production.own }
    private let starts: AsyncStream<Bool>
    private let started: AsyncStream<Bool>.Continuation
    private let exchanges: AsyncStream<Bool>
    private let exchanged: AsyncStream<Bool>.Continuation
    private(set) var closed = false
    private(set) var diagnostics: [String] = []
    private let wire: ServiceWorkflowWire
    private let mediaCompletions: AsyncStream<ServiceMediaCompletion>
    private let mediaCompleted: AsyncStream<ServiceMediaCompletion>.Continuation
    static func make(wire: ServiceWorkflowWire, factory: ServiceSDKFactory,
                     persisted: @escaping @Sendable () async -> Void) async throws -> ServiceAppBackend {
        let workflow = CoreCloudWorkflowService()
        let production = try await NativeCollectionCloudBackend.make(workflow: workflow, factory: factory,
            ownTransport: ServiceOwnTransport(wire: wire), mediaTransport: ServiceMediaTransport(wire: wire), persisted: persisted)
        return ServiceAppBackend(production: production, workflow: workflow, wire: wire)
    }
    private init(production: NativeCollectionCloudBackend, workflow: CoreCloudWorkflowService, wire: ServiceWorkflowWire) {
        self.production = production; self.workflow = workflow; self.wire = wire
        (mediaCompletions, mediaCompleted) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (starts, started) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
        (exchanges, exchanged) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }
    private enum DiagnosticStage: String {
        case input, inputThrow = "input.throw", executeOutputs = "execute.outputs", executeError = "execute.error"
        case beginAccountReadThrow = "beginAccountRead.throw", readAccountError = "readAccount.error"
        case readAccountOutputs = "readAccount.outputs"
    }
    private enum DiagnosticKind: String { case error, coreActionFailed, applyDeferred }
    @discardableResult
    private func capture(_ error: any Error, stage: DiagnosticStage, kind: DiagnosticKind = .error,
                         action: CloudPolicyAction? = nil) -> String {
        let category: String
        switch error {
        case BridgeError.cloudPolicy(let failure): category = "cloudPolicy.\(String(describing: failure))"
        case is BridgeError: category = "bridge"
        default: category = "other"
        }
        let actionTag = action.map { " action=\(actionLabel($0))" } ?? ""
        let diagnostic = "\(stage.rawValue).\(kind.rawValue): \(category)\(actionTag)"
        if diagnostics.count == 16 { diagnostics.removeFirst() }
        diagnostics.append(diagnostic)
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
    private func capture(_ output: CloudPolicyOutput, stage: DiagnosticStage) {
        switch output {
        case .coreActionFailed(let error, _): capture(error, stage: stage, kind: .coreActionFailed)
        case .applyDeferred(let error): capture(error, stage: stage, kind: .applyDeferred)
        case .commands, .syncNowUnavailable: break
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
        let result = await production.execute(command)
        for output in result.outputs { capture(output, stage: .executeOutputs) }
        switch command.action {
        case .startEngine: started.yield(result.error == nil && !result.deferred)
        case .runExchange: exchanged.yield(result.error == nil && !result.deferred)
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
            let diagnostic = capture(error, stage: .executeError, action: command.action)
            Issue.record("Service app command failed; \(diagnostic)")
        }
        return result
    }
    func waitForStart() async throws { try #require(try await serviceAppSignal(starts, stage: "SDK start")) }
    func waitForExchange() async throws { try #require(try await serviceAppSignal(exchanges, stage: "Sync Now exchange")) }
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
        let result = await production.readAccount(read)
        if let error = result.error { capture(error, stage: .readAccountError) }
        for output in result.outputs { capture(output, stage: .readAccountOutputs) }
        return result
    }
    func takeAccountReadRequest() async -> Bool { await production.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await production.freezeForSuspend() }
    func resumeAfterCoreResume() async { await production.resumeAfterCoreResume() }
    func close() async throws { try await production.close(); closed = true; started.finish(); exchanged.finish(); mediaCompleted.finish() }
}
private nonisolated struct ServiceMediaCompletion: Sendable {
    let proof: Result<ServiceMediaProof, any Error>
    let receipts: [ServiceSavedReceipt]
}
private nonisolated struct ServiceAppTimeout: Error { let stage: String }
private nonisolated func serviceAppSignal(_ stream: AsyncStream<Bool>, stage: String) async throws -> Bool {
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
    private(set) var backends: [ServiceAppBackend] = []
    lazy var sync = makeSync()

    private func makeSync() -> SyncCoordinator {
        SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-initial-app", environment: .development,
                status: .available, identity: Data("synthetic-initial-account".utf8))
        }, makeBackend: { [self] persisted in
            let allBackendsClosed: Bool = backends.allSatisfy { $0.closed }
            try #require(allBackendsClosed)
            let backend = try await ServiceAppBackend.make(wire: wire, factory: factory, persisted: persisted)
            backends.append(backend)
            return backend
        }, defaults: defaults, clock: .system, directory: nil, observeSignals: false)
    }
    lazy var owner: ReviewCollection = {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_735_689_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        return owner
    }()
    init(root: URL, wire: ServiceWorkflowWire, defaults: UserDefaults? = nil) throws {
        self.root = root; self.wire = wire
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
    }
    func reopen() async throws {
        await owner.start()
        await owner.waitForCloudActivation()
        await sync.prepareConnectionAuthority()
        let diagnostics = backends.flatMap { $0.diagnostics }.joined(separator: " | ")
        let failureMessage = "Service app reopen: setupState=\(sync.setupState); backend diagnostics=[\(diagnostics)]"
        try #require(owner.state == .ready && !sync.operationFailed, "\(failureMessage)")
        try await backend.waitForStart()
    }
    func exchange() async throws {
        let original = try backend
        sync.receive(.network(reachable: true)); sync.syncNow()
        try await original.waitForExchange()
        await sync.waitForInputs()
        let exchangeSucceeded = !sync.operationFailed
        let diagnostics = backends.flatMap { $0.diagnostics }.suffix(16).joined(separator: " | ")
        try #require(exchangeSucceeded, "Service app exchange: backend diagnostics=[\(diagnostics)]")
    }
    func cleanup() async { _ = await owner.close() }
}
#endif
