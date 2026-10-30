import Foundation
import Synchronization
import Testing
@testable import AppModel
import CoreBridge

extension ReviewCollectionStartupTests {
    @Test func firstConnectionRetainsOriginalNoCollectionHostAndPreviewCanCancelThenRetry() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        let first = PreviewReadGate(), second = PreviewReadGate()
        let transport = PreviewReadTransport([first, second])
        var attachments = 0
        let sync = SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-first-preview", environment: .development,
                status: .available, identity: Data("synthetic-first-account".utf8))
        }, makeBackend: { _ in
            attachments += 1
            let connection = try await CoreCloudWorkflowService().connection()
            return PreviewActivityBackend(connection: connection, own: CloudOwnExecutor(core: connection, transport: transport))
        }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        await owner.start()
        do {
            #expect(owner.state == .noFile && owner.lifecycle == .noCollection)
            #expect(sync.connectionService == nil)
            try await owner.prepareCloudConnectionPreview()
            await first.waitForAdmission()
            #expect(attachments == 1 && sync.connectionService != nil)
            #expect(sync.connectionPreviewActive && !sync.settings.enabled)
            var cancel: Task<Void, Never>?
            await withCheckedContinuation { started in
                cancel = Task { @MainActor in
                    started.resume()
                    await sync.cancelConnectionPreview()
                }
            }
            #expect(sync.connectionPreviewActive && sync.connectionPreviewCancelling)
            first.finish()
            await cancel?.value
            #expect(sync.preview == nil && !sync.connectionPreviewActive)
            try await owner.prepareCloudConnectionPreview()
            await second.waitForAdmission()
            second.finish()
            await sync.waitForConnectionPreview()
            let work = try #require(sync.preview)
            guard case .ready(_, let summary) = work else { throw CloudOwnTransportFailure.unexpectedReply }
            #expect(!summary.localCollectionPresent && !summary.localEmpty)
            #expect(summary.localCounts.isEmpty && summary.decision == .noCollection)
            #expect(attachments == 1 && !sync.settings.enabled && sync.recovery == nil)
            #expect(!FileManager.default.fileExists(atPath: root.appending(path: "collection.anki2").path))
            #expect(owner.state == .noFile && owner.lifecycle == .noCollection && owner.review == nil)
            await sync.cancelConnectionPreview()
            #expect(await owner.close())
        } catch { first.finish(); second.finish(); _ = await owner.close(); throw error }
    }

    @Test func retainedPreviewStaysActiveThroughCancelDrainAndCanBeRetried() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: source, to: root.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root)
        let first = PreviewReadGate(), second = PreviewReadGate()
        let transport = PreviewReadTransport([first, second])
        var backend: PreviewActivityBackend?
        let sync = SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-ui-preview", environment: .development,
                status: .available, identity: Data("synthetic-ui-account".utf8))
        }, makeBackend: { _ in
            let connection = try await CoreCloudWorkflowService().connection()
            let value = PreviewActivityBackend(connection: connection,
                own: CloudOwnExecutor(core: connection, transport: transport))
            backend = value
            return value
        }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync)
        await owner.start()
        do {
            try #require(owner.state == .ready && backend != nil)
            for gate in [first, second] {
                try await owner.prepareCloudConnectionPreview()
                // prepare has returned, but the original network read is retained.
                #expect(sync.connectionPreviewActive)
                #expect(!sync.connectionOperationActive && !sync.settings.enabled)
                await gate.waitForAdmission()
                #expect(sync.preview == nil && sync.connectionPreviewActive)
                var cancel: Task<Void, Never>?
                await withCheckedContinuation { started in
                    cancel = Task { @MainActor in
                        started.resume()
                        await sync.cancelConnectionPreview()
                    }
                }
                #expect(sync.connectionPreviewActive && sync.connectionPreviewCancelling)
                #expect(!sync.connectionOperationActive && !sync.settings.enabled)
                // Cancellation is not completion: deliver the ORIGINAL callback.
                gate.finish()
                await cancel?.value
                #expect(!sync.connectionPreviewActive && !sync.connectionPreviewCancelling)
                #expect(sync.preview == nil && sync.recovery == nil)
                #expect(!sync.settings.enabled)
            }
            #expect(transport.remaining == 0)
            #expect(await owner.close())
        } catch { first.finish(); second.finish(); _ = await owner.close(); throw error }
    }
}

private nonisolated final class PreviewReadGate: CloudPreparedOwnOperation, Sendable {
    private let admitted: AsyncStream<Void>
    private let admission: AsyncStream<Void>.Continuation
    private let replies: AsyncStream<CloudOwnTransportReply>
    private let replySink: AsyncStream<CloudOwnTransportReply>.Continuation
    init() {
        (admitted, admission) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
        (replies, replySink) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func enqueue() -> Bool { admission.yield(()); return true }
    func cancel() {} // The test deliberately holds terminal delivery after cancellation.
    func waitForAdmission() async { for await _ in admitted { return } }
    func finish() { replySink.yield(.read(.zoneMissing)); replySink.finish() }
    func reply() async throws -> CloudOwnTransportReply {
        for await value in replies { return value }
        throw CloudOwnTransportFailure.missingReply
    }
}
private nonisolated final class PreviewReadTransport: CloudOwnTransport, Sendable {
    private let gates: Mutex<[PreviewReadGate]>
    init(_ gates: [PreviewReadGate]) { self.gates = Mutex(gates) }
    var remaining: Int { gates.withLock { $0.count } }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .read = command else { throw CloudOwnTransportFailure.unexpectedReply }
        return try gates.withLock {
            guard !$0.isEmpty else { throw CloudOwnTransportFailure.missingReply }
            return $0.removeFirst()
        }
    }
}
@MainActor private final class PreviewActivityBackend: SyncCoordinatorBackend {
    let connection: CoreCloudConnectionService?
    let own: CloudOwnExecutor?
    init(connection: CoreCloudConnectionService, own: CloudOwnExecutor) { self.connection = connection; self.own = own }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput { .commands([]) }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult { .init(outputs: [], error: nil, deferred: false) }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { 1 }
    func readAccount(_ read: UInt64) async -> SyncCommandResult { .init(outputs: [], error: nil, deferred: false) }
    func close() async throws { await own?.close() }
}
