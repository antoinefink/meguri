import Foundation
import Synchronization
import Testing
@testable import CoreBridge

/// Synthetic inputs only; no private account or container configuration. Uses
/// the existing collection bootstrap and process-wide EventTests serialization.
struct CloudConnectionFixture: Sendable {
    let directory: URL
    let collection: CoreCollectionService
    let workflow: CoreCloudWorkflowService
    let connection: CoreCloudConnectionService
    static func open(status: CloudAccountStatus = .available, identity: Data? = Data("synthetic-account-A".utf8)) async throws -> Self {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let data = directory.appending(path: "collection")
        let backups = directory.appending(path: "backups")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        let location = CollectionLocation(collectionDirectory: data, backupsDirectory: backups, appGroupSnapshot: directory.appending(path: "widget"))
        let collection = CoreCollectionService()
        do {
            try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-u114", environment: .development, status: status, identity: identity)
            try await MaintenanceLane.shared.bootstrapCollection(location)
            try await collection.open(location, randomSeed: 114)
            let workflow = CoreCloudWorkflowService()
            return Self(directory: directory, collection: collection, workflow: workflow, connection: try await workflow.connection())
        } catch {
            try? await collection.close()
            try? FileManager.default.removeItem(at: directory)
            throw error
        }
    }
    func ready() async throws -> CloudConnectionPreview {
        guard case .readActive(let preview, let target) = try await connection.beginConnectionPreview() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(target == .init(zone: "meguri-root", name: "active", recordType: "MGActive"))
        guard case .ready(let confirmation, let summary) = try await connection.connectionActiveRead(preview, read: .zoneMissing) else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(summary.manifestCounts == nil && summary.publishedAtMilliseconds == nil)
        #expect(summary.decision == .confirmUpload)
        return confirmation
    }
    func own() async throws -> (CloudPublicationRequest, CloudOwnOperation, CloudRecoveryOperation) {
        for _ in 0..<8 {
            switch try await connection.nextPublicationWork() {
            case .local(let h, _, _): try await connection.performLocal(h)
            case .own(let r, let c, let o, _): return (r, c, o)
            default: throw CloudOwnTransportFailure.unexpectedReply
            }
        }
        throw CloudOwnTransportFailure.unexpectedReply
    }
    func close() async {
        try? await collection.close()
        try? FileManager.default.removeItem(at: directory)
    }
}
func withCloudConnection(
    status: CloudAccountStatus = .available, identity: Data? = Data("synthetic-account-A".utf8),
    _ body: (CloudConnectionFixture) async throws -> Void
) async throws {
    let fixture = try await CloudConnectionFixture.open(status: status, identity: identity)
    do { try await body(fixture) } catch { await fixture.close(); throw error }
    await fixture.close()
}
final class ConnectionQueue: CloudPublicationQueue, Sendable {
    let count = Mutex(0)
    func enqueue() -> Bool { count.withLock { $0 += 1 }; return true }
}

/// Deterministic transport seam. Queue admission and replies are independent;
/// tests can hold the original reply through cancellation and core close.
final class HeldOwnOperation: CloudPreparedOwnOperation, Sendable {
    let count = Mutex(0)
    let cancelled = Mutex(false)
    private let stream: AsyncStream<CloudOwnTransportReply>
    private let continuation: AsyncStream<CloudOwnTransportReply>.Continuation
    private let admissions: AsyncStream<Void>
    private let admitted: AsyncStream<Void>.Continuation
    private let cancellations: AsyncStream<Void>
    private let didCancel: AsyncStream<Void>.Continuation
    init() {
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
        (admissions, admitted) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
        (cancellations, didCancel) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func enqueue() -> Bool {
        if cancelled.withLock({ $0 }) { return false }
        count.withLock { $0 += 1 }; admitted.yield(()); return true
    }
    func cancel() { cancelled.withLock { $0 = true }; didCancel.yield(()) }
    func finishAdmissionWait() { admitted.finish() }
    func waitUntilQueued() async throws {
        var iterator = admissions.makeAsyncIterator()
        guard await iterator.next() != nil else { throw CloudOwnTransportFailure.missingReply }
    }
    func waitUntilCancelled() async throws {
        var iterator = cancellations.makeAsyncIterator()
        guard await iterator.next() != nil else { throw CloudOwnTransportFailure.missingReply }
    }
    func finish(_ reply: CloudOwnTransportReply) { continuation.yield(reply); continuation.finish() }
    func reply() async throws -> CloudOwnTransportReply {
        var iterator = stream.makeAsyncIterator()
        guard let reply = await iterator.next() else { throw CloudOwnTransportFailure.missingReply }
        return reply
    }
}
struct HeldOwnTransport: CloudOwnTransport {
    let operation: HeldOwnOperation
    func prepare(_ command: CloudOwnTransportCommand) -> any CloudPreparedOwnOperation { operation }
}
