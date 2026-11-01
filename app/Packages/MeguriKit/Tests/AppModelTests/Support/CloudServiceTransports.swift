#if os(macOS)
@testable import CoreBridge
import Foundation
import Synchronization

// Preparation constructs every exact frame and its terminal reply owner. The
// submit callback synchronously adds those ORIGINAL frames to the real stdio
// queue. Cancel only fences admission/marks replies; queued frames still drain.
private nonisolated final class ServicePreparedFrames: Sendable {
    private struct State { var queued = false; var cancelled = false }
    private let state = Mutex(State())
    private let frames: [(Data, CloudServicePreparedFrame)]
    init(process: CloudServiceProcess, operations: [ServiceJSON]) throws {
        frames = try operations.map { operation in
            let frame = try serviceRequestFrame(operation)
            return (frame, try process.prepare(frame))
        }
    }
    func enqueue() -> Bool {
        state.withLock { value in
            guard !value.queued, !value.cancelled else { return false }
            value.queued = true
            return frames.allSatisfy { $0.1.enqueue() }
        }
    }
    func cancel() {
        state.withLock { value in
            value.cancelled = true
            for (_, original) in frames { original.cancel() }
        }
    }
    func replies() async throws -> [ServiceOriginalReply] {
        guard state.withLock({ $0.queued }) else { throw ServiceWorkflowFailure.notQueued }
        return try await withTaskCancellationHandler {
            var replies: [ServiceOriginalReply] = []
            var firstFailure: (any Error)?
            for (frame, original) in frames {
                do { replies.append(try serviceOriginalReply(request: frame, response: await original.reply())) }
                catch { if firstFailure == nil { firstFailure = error } }
            }
            // Join ALL accepted frames even after cancellation or a first error.
            if let firstFailure { throw firstFailure }
            try Task.checkCancellation()
            return replies
        } onCancel: { self.cancel() }
    }
}
private nonisolated func serviceReadOperation(zone: String, name: String, fields: [String]?) -> ServiceJSON {
    .object(["Read": .object(["zone": .string(zone), "name": .string(name),
        "fields": fields.map { .object(["Named": .array($0.map(ServiceJSON.string))]) } ?? .string("All")])])
}
nonisolated struct ServiceOwnTransport: CloudOwnTransport {
    let wire: ServiceWorkflowWire
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        let operations: [ServiceJSON]
        switch command {
        case .read(let target), .publication(.read(let target)):
            operations = [serviceReadOperation(zone: target.zone, name: target.name, fields: ["e"])]
        case .assetRead(let zone, let name): operations = [serviceReadOperation(zone: zone, name: name, fields: nil)]
        case .adoptionPage(let zone, let cursor):
            let token = try cursor.map { try JSONDecoder().decode(ServiceJSON.self, from: $0) } ?? .null
            operations = [.object(["FetchZone": .object(["zone": .string(zone), "token": token])])]
        case .publication(.createZones(let zones)):
            guard !zones.isEmpty else { throw ServiceWorkflowFailure.unsupported }
            operations = zones.map { .object(["CreateZone": .object(["zone": .string($0)])]) }
        case .publication(.save(let target, let encrypted, let fields)):
            let record = CloudRecord(name: target.name, envelope: encrypted, systemFields: fields, recordType: target.recordType, zone: target.zone)
            operations = [.object(["Modify": .object(["zone": .string(target.zone), "atomic": .bool(false), "changes": .array([try wire.saveInput(record)])])])]
        }
        return try ServicePreparedOwn(wire: wire, command: command, operations: operations)
    }
}
private nonisolated final class ServicePreparedOwn: CloudPreparedOwnOperation, Sendable {
    let wire: ServiceWorkflowWire
    let command: CloudOwnTransportCommand
    private let originals: ServicePreparedFrames
    init(wire: ServiceWorkflowWire, command: CloudOwnTransportCommand, operations: [ServiceJSON]) throws {
        self.wire = wire; self.command = command
        originals = try ServicePreparedFrames(process: wire.process, operations: operations)
    }
    func enqueue() -> Bool { originals.enqueue() }
    func cancel() { originals.cancel() }
    func reply() async throws -> CloudOwnTransportReply { try await wire.own(command, replies: originals.replies()) }
}
nonisolated struct ServiceMediaTransport: CloudMediaTransport {
    let wire: ServiceWorkflowWire
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        let names: [String], fields: [String]
        switch request {
        case .root: names = ["root"]; fields = ["e"]
        case .read(let name): names = [name]; fields = ["e"]
        case .reads(let batch): names = batch; fields = ["e"]
        case .download(let name): names = [name]; fields = ["e", "a"]
        case .upload: throw CloudMediaFailure.requiresSyncEngine
        }
        guard !names.isEmpty else { throw ServiceWorkflowFailure.unsupported }
        let operations = names.map { serviceReadOperation(zone: "meguri-media", name: $0, fields: fields) }
        return try ServicePreparedMedia(wire: wire, request: request, operations: operations)
    }
}
private nonisolated final class ServicePreparedMedia: CloudPreparedMediaRead, Sendable {
    let wire: ServiceWorkflowWire
    let request: CloudMediaRequest
    private let originals: ServicePreparedFrames
    init(wire: ServiceWorkflowWire, request: CloudMediaRequest, operations: [ServiceJSON]) throws {
        self.wire = wire; self.request = request
        originals = try ServicePreparedFrames(process: wire.process, operations: operations)
    }
    func enqueue() -> Bool { originals.enqueue() }
    func cancel() { originals.cancel() }
    func reply() async throws -> CloudMediaTransportReply { try await wire.media(request, replies: originals.replies()) }
}
#endif
