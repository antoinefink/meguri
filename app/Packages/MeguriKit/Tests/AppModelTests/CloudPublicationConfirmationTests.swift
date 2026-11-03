import Foundation
import Synchronization
import Testing
@testable import AppModel
import CoreBridge

extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func publicationPresentationReleasesAtDurableAcceptanceWhileOriginalTransportStaysOwned(backgroundThrows: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let presentation = CloudPublicationConfirmation()
        var returned = false, confirmations = 0
        harness.sync.observePublicationConfirmation = { confirmations += 1 }
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            harness.owner.navigation.present(.cloudSync(.replaceCloud))
            #expect(harness.owner.navigation.sheet == .cloudSync(.replaceCloud))
            original = Task { await presentation.confirm(using: harness.sync); returned = true }
            try await harness.transport.zones.waitForAdmission()
            #expect(presentation.accepted && !presentation.confirming && presentation.submitted)
            #expect(presentation.backgroundErrorKey == nil)
            #expect(!returned && harness.sync.connectionOperationActive && harness.sync.settings.enabled)
            #expect(try await harness.connection().recoveryStatus() != nil)
            await presentation.confirm(using: harness.sync)
            await harness.sync.confirmPublication() // consumed preview cannot start another Core call
            #expect(confirmations == 1 && harness.transport.zones.enqueues == 1)
            #expect(presentation.close())
            harness.owner.navigation.dismissSheet()
            #expect(harness.owner.navigation.sheet == nil && !harness.transport.zones.cancelled)
            harness.transport.zones.finish(.publication(.zonesCreated))
            try await harness.transport.save.waitForAdmission()
            #expect(!returned && harness.sync.connectionOperationActive)
            #expect(harness.transport.save.enqueues == 1 && !harness.transport.save.cancelled)
            // A different presentation cannot inherit the accepted attempt.
            let nextPresentation = CloudPublicationConfirmation()
            #expect(!nextPresentation.submitted && !nextPresentation.confirming)
            if backgroundThrows { harness.transport.save.fail() }
            else { harness.transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil))) }
            await original?.value
            #expect(returned && !harness.sync.connectionOperationActive)
            #expect(presentation.accepted && harness.sync.settings.enabled)
            #expect(try await harness.connection().recoveryStatus() != nil)
            if backgroundThrows { #expect(harness.sync.operationFailed) }
            #expect(presentation.backgroundErrorKey == nil) // Close invalidated this presentation's attempt.
            #expect(confirmations == 1 && !nextPresentation.submitted)
            #expect(await harness.owner.close())
        } catch { harness.release(); await original?.value; _ = await harness.owner.close(); throw error }
    }

    @Test(arguments: [false, true])
    func publicationRefusalIsAttemptSpecificAndHistoricalFailureDoesNotBecomeCurrentFailure(backgroundThrows: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let refused = CloudPublicationConfirmation()
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            guard case .ready(let handle, _) = harness.sync.preview else { throw CloudOwnTransportFailure.unexpectedReply }
            try await harness.connection().cancelPreview(handle)
            await refused.confirm(using: harness.sync)
            #expect(!refused.confirming && !refused.submitted && refused.refused)
            #expect(refused.failure == .consumed && harness.sync.operationFailed)
            #expect(!harness.sync.settings.enabled && !harness.sync.connectionOperationActive)
            #expect(try await harness.connection().recoveryStatus() == nil)
            harness.sync.beginConnectionPreview()
            await harness.sync.waitForConnectionPreview()
            #expect(harness.sync.operationFailed && harness.sync.connectionFailure == nil)
            refused.clearRefusal()
            #expect(!refused.refused && refused.failure == nil)
            #expect(refused.backgroundErrorKey == nil)
            original = Task { await refused.confirm(using: harness.sync) }
            try await harness.transport.zones.waitForAdmission()
            #expect(refused.accepted && !refused.refused && refused.failure == nil)
            #expect(harness.sync.operationFailed) // history is preserved
            #expect(refused.backgroundErrorKey == nil) // History is not a failure of this accepted attempt.
            harness.owner.navigation.present(.cloudSync(.replaceCloud))
            harness.transport.zones.finish(.publication(.zonesCreated))
            try await harness.transport.save.waitForAdmission()
            #expect(refused.backgroundErrorKey == nil && harness.sync.connectionOperationActive)
            if backgroundThrows { harness.transport.save.fail() }
            else { harness.transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil))) }
            harness.release()
            await original?.value
            #expect(refused.backgroundErrorKey == (backgroundThrows ? "sync.recovery.error.other" : nil))
            #expect(refused.accepted && !refused.refused && refused.failure == nil)
            #expect(harness.sync.operationFailed && harness.sync.settings.enabled && !harness.sync.connectionOperationActive)
            #expect(try await harness.connection().recoveryStatus() != nil)
            #expect(!harness.transport.save.cancelled && harness.transport.save.enqueues == 1)
            let nextPresentation = CloudPublicationConfirmation()
            #expect(nextPresentation.backgroundErrorKey == nil && !nextPresentation.submitted)
            #expect(refused.close())
            harness.owner.navigation.dismissSheet()
            #expect(harness.owner.navigation.sheet == nil)
            #expect(await harness.owner.close())
        } catch { harness.release(); await original?.value; _ = await harness.owner.close(); throw error }
    }

    @Test(arguments: [false, true])
    func publicationBeforeCoreReturnIsSingleFlightAndStalePresentationCannotReceiveResult(changeGeneration: Bool) async throws {
        let harness = try PublicationPresentationHarness()
        defer { harness.removeStorage() }
        let presentation = CloudPublicationConfirmation()
        let entered = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let release = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        var confirmations = 0
        harness.sync.observePublicationConfirmation = {
            confirmations += 1; entered.continuation.yield(true)
            for await _ in release.stream { break }
        }
        var original: Task<Void, Never>?
        do {
            try await harness.prepare()
            original = Task { await presentation.confirm(using: harness.sync) }
            try #require(try await publicationSignal(entered.stream))
            #expect(presentation.confirming && presentation.submitted && !presentation.close())
            #expect(!harness.sync.settings.enabled && confirmations == 1)
            await presentation.confirm(using: harness.sync)
            await harness.sync.confirmPublication()
            #expect(confirmations == 1)
            if changeGeneration {
                harness.sync.collectionReplaced(harness.sync.generation + 1)
                await harness.sync.waitForInputs()
            }
            if !changeGeneration { presentation.invalidate() }
            release.continuation.yield(()); release.continuation.finish()
            harness.release()
            await original?.value
            #expect(!presentation.accepted && !presentation.refused)
            if changeGeneration {
                #expect(presentation.confirming) // no stale-generation result was delivered
                presentation.invalidate() // mirrors the view's generation observer
            }
            #expect(!presentation.confirming)
            #expect(confirmations == 1)
            if changeGeneration { await harness.sync.waitForReplacement() }
            #expect(await harness.owner.close())
        } catch {
            presentation.invalidate(); release.continuation.yield(()); release.continuation.finish()
            harness.release(); await original?.value; _ = await harness.owner.close(); throw error
        }
    }
}

@MainActor private final class PublicationPresentationHarness {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    let transport = PublicationPresentationTransport()
    private var backend: PublicationPresentationBackend?
    lazy var sync = SyncCoordinator(bootstrap: {
        try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-publication-presentation", environment: .development,
            status: .available, identity: Data("synthetic-publication-account".utf8))
    }, makeBackend: { [self] _ in
        let connection = try await CoreCloudWorkflowService().connection()
        let backend = PublicationPresentationBackend(connection: connection, own: CloudOwnExecutor(core: connection, transport: transport))
        self.backend = backend
        return backend
    }, defaults: nil, clock: .system, directory: nil, observeSignals: false)
    lazy var owner: ReviewCollection = {
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        return ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
    }()
    init() throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: source, to: root.appending(path: "collection.anki2"))
    }
    func prepare() async throws {
        await owner.start()
        try #require(owner.state == .ready)
        try await owner.prepareCloudConnectionPreview()
        await sync.waitForConnectionPreview()
        guard case .ready(_, let summary) = sync.preview else { throw CloudOwnTransportFailure.unexpectedReply }
        try #require(summary.decision == .confirmUpload)
    }
    func connection() throws -> CoreCloudConnectionService { try #require(backend?.connection) }
    func release() {
        transport.zones.finish(.publication(.failed(retryAfterMilliseconds: nil)))
        transport.save.finish(.publication(.failed(retryAfterMilliseconds: nil)))
    }
    func removeStorage() { try? FileManager.default.removeItem(at: root) }
}

@MainActor private final class PublicationPresentationBackend: SyncCoordinatorBackend {
    let connection: CoreCloudConnectionService?
    let own: CloudOwnExecutor?
    init(connection: CoreCloudConnectionService, own: CloudOwnExecutor) { self.connection = connection; self.own = own }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput { .commands([]) }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult { .init(outputs: [], error: nil, deferred: false) }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { 1 }
    func readAccount(_ read: UInt64) async -> SyncCommandResult { .init(outputs: [], error: nil, deferred: false) }
    func quiesce() async throws { await own?.close() }
    func close() async throws { await own?.close() }
}

private nonisolated final class PublicationPresentationTransport: CloudOwnTransport, Sendable {
    let zones = PublicationPresentationGate(), save = PublicationPresentationGate()
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        switch command {
        case .read: PublicationImmediateReply(value: .read(.zoneMissing))
        case .publication(.createZones): zones
        case .publication(.save): save
        default: throw CloudOwnTransportFailure.unexpectedReply
        }
    }
}
private nonisolated final class PublicationImmediateReply: CloudPreparedOwnOperation, Sendable {
    let value: CloudOwnTransportReply
    init(value: CloudOwnTransportReply) { self.value = value }
    func enqueue() -> Bool { true }
    func cancel() {}
    func reply() async throws -> CloudOwnTransportReply { value }
}
private nonisolated struct PublicationTestFailure: Error {}
private nonisolated final class PublicationPresentationGate: CloudPreparedOwnOperation, Sendable {
    private let state = Mutex((enqueues: 0, cancelled: false))
    private let admission = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
    private let terminal = AsyncStream<Result<CloudOwnTransportReply, PublicationTestFailure>>.makeStream(bufferingPolicy: .bufferingNewest(1))
    var enqueues: Int { state.withLock { $0.enqueues } }
    var cancelled: Bool { state.withLock { $0.cancelled } }
    func enqueue() -> Bool { state.withLock { $0.enqueues += 1 }; admission.continuation.yield(true); return true }
    func cancel() { state.withLock { $0.cancelled = true } }
    func finish(_ value: CloudOwnTransportReply) { terminal.continuation.yield(.success(value)); terminal.continuation.finish() }
    func fail() { terminal.continuation.yield(.failure(PublicationTestFailure())); terminal.continuation.finish() }
    func waitForAdmission() async throws { try #require(try await publicationSignal(admission.stream)) }
    func reply() async throws -> CloudOwnTransportReply {
        for await result in terminal.stream { return try result.get() }
        throw CloudOwnTransportFailure.missingReply
    }
}
private nonisolated func publicationSignal(_ stream: AsyncStream<Bool>) async throws -> Bool {
    try await withThrowingTaskGroup(of: Bool.self) { group in
        group.addTask { for await value in stream { return value }; throw CloudOwnTransportFailure.missingReply }
        group.addTask { try await Task.sleep(for: .seconds(10)); throw PublicationTestFailure() }
        defer { group.cancelAll() }
        return try #require(try await group.next())
    }
}
