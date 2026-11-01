#if os(macOS)
@testable import CoreBridge
import CryptoKit
import Foundation

// Small, synthetic Service wire only. Tokens and original Response JSON remain
// opaque transport values; none of these bytes are CKRecord archives.
nonisolated enum ServiceJSON: Codable, Sendable {
    case object([String: Self]), array([Self]), string(String), integer(Int64), bool(Bool), null
    init(from decoder: any Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(Int64.self) { self = .integer(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([Self].self) { self = .array(v) }
        else { self = .object(try c.decode([String: Self].self)) }
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .integer(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
    subscript(_ key: String) -> Self? { if case .object(let v) = self { v[key] } else { nil } }
    var items: [Self]? { if case .array(let v) = self { v } else { nil } }
    var text: String? { if case .string(let v) = self { v } else { nil } }
    var flag: Bool? { if case .bool(let v) = self { v } else { nil } }
    var number: Int64? { if case .integer(let v) = self { v } else { nil } }
    static func bytes(_ v: Data) -> Self { .array(v.map { .integer(Int64($0)) }) }
    func bytes() throws -> Data {
        guard let items else { throw ServiceWorkflowFailure.wire }
        return try Data(items.map { value in
            guard let n = value.number, let b = UInt8(exactly: n) else { throw ServiceWorkflowFailure.wire }
            return b
        })
    }
    func encoded() throws -> Data { try JSONEncoder().encode(self) }
}
nonisolated enum ServiceWorkflowFailure: Error { case wire, unsupported, notQueued, refused, server(String), missingReceipt }
nonisolated func serviceRequired<T>(_ v: T?) throws -> T {
    guard let v else { throw ServiceWorkflowFailure.wire }; return v
}
nonisolated struct ServiceOriginalReply: Sendable {
    let originalRequestJSON: Data
    let originalResponseJSON: Data
    let requestID: Int64
    let result: ServiceJSON
    func ok() throws -> ServiceJSON {
        if let error = result["Err"] { throw ServiceWorkflowFailure.server(error["code"]?.text ?? "malformed-error") }
        return try serviceRequired(result["Ok"])
    }
}
nonisolated struct ServiceSavedReceipt: Sendable {
    let original: ServiceOriginalReply
    let zone: String
    let record: CloudRecord
}
extension ServiceSavedReceipt {
    nonisolated func blobBytes() throws -> Data? {
        guard zone == "meguri-media", record.recordType == "MGBlob" else { return nil }
        let mutations = try serviceRequired(try original.ok()["Modified"]?.items)
        let saved = try serviceRequired(mutations.first { $0["name"]?.text == record.name }?["result"]?["Ok"]?["Saved"])
        return try ServiceRecordBytes(saved).asset("a")
    }
}
nonisolated struct ServiceRecordBytes: Sendable {
    let raw: ServiceJSON
    let name: String
    let recordType: String
    let envelope: Data
    let systemFields: Data
    init(_ raw: ServiceJSON) throws {
        self.raw = raw
        name = try serviceRequired(raw["name"]?.text)
        recordType = try serviceRequired(raw["record_type"]?.text)
        envelope = try serviceRequired(raw["fields"]?["e"]?["EncryptedBytes"]).bytes()
        systemFields = try serviceRequired(raw["system_fields"]).bytes()
    }
    var control: CloudControlRecord { .init(encrypted: envelope, systemFields: systemFields) }
    func record(zone: String, lease: CloudMediaStagedAsset? = nil) -> CloudRecord {
        .init(name: name, envelope: envelope, systemFields: systemFields, recordType: recordType, zone: zone, assetURL: lease?.url)
    }
    func asset(_ field: String) throws -> Data? {
        guard let value = raw["fields"]?[field]?["Asset"] else { return nil }
        let bytes = try serviceRequired(value["bytes"]).bytes()
        let digest = try serviceRequired(value["sha256"]).bytes()
        guard Data(SHA256.hash(data: bytes)) == digest else { throw ServiceWorkflowFailure.wire }
        return bytes
    }
}

nonisolated func serviceRequestFrame(_ operation: ServiceJSON, instance: String? = nil, ownOperation: String? = nil) throws -> Data {
    try ServiceJSON.object([
        "scope": .object(["container": .string("synthetic-initial-app"), "environment": .string("development"), "account": .string("synthetic-initial-account")]),
        "caller": .object(["device": .string("synthetic-app-workflow"), "instance": instance.map(ServiceJSON.string) ?? .null, "own_operation": ownOperation.map(ServiceJSON.string) ?? .null]),
        "operation": operation,
    ]).encoded()
}
nonisolated func serviceOriginalReply(request: Data, response: Data) throws -> ServiceOriginalReply {
    let root = try JSONDecoder().decode(ServiceJSON.self, from: response)
    return try .init(originalRequestJSON: request, originalResponseJSON: response,
        requestID: serviceRequired(root["request_id"]?.number), result: serviceRequired(root["result"]))
}

actor ServiceWorkflowWire {
    let process: CloudServiceProcess
    let staging: CloudMediaStaging
    private let sourceDirectory: URL
    private(set) var receipts: [ServiceSavedReceipt] = []
    init(process: CloudServiceProcess, directory: URL) throws {
        self.process = process
        sourceDirectory = directory.appending(path: "callback-sources")
        try FileManager.default.createDirectory(at: sourceDirectory, withIntermediateDirectories: true)
        staging = try CloudMediaStaging(directory: directory.appending(path: "leases"))
    }
    func call(_ operation: ServiceJSON, instance: String? = nil) async throws -> ServiceOriginalReply {
        let frame = try serviceRequestFrame(operation, instance: instance)
        let data = try await process.request(frame)
        return try serviceOriginalReply(request: frame, response: data)
    }

    func createZone(_ zone: String) async throws {
        let reply = try await call(.object(["CreateZone": .object(["zone": .string(zone)])]))
        guard try reply.ok().text == "Done" else { throw ServiceWorkflowFailure.wire }
    }
    func seed(_ fixture: InitialCloudFixture) async throws {
        for zone in Set(fixture.records.map(\.zone)).sorted() { try await createZone(zone) }
        for record in fixture.records {
            // Fresh Service tags only: never fixture systemFields or ACK seeds.
            let input: ServiceJSON = .object(["name": .string(record.name), "record_type": .string(record.recordType),
                "fields": .object(["e": .object(["EncryptedBytes": .bytes(Data(record.encrypted))])])])
            let reply = try await modify(zone: record.zone, changes: [.object(["Save": .object(["record": input, "if_unchanged": .null])])])
            let mutations = try serviceRequired(try reply.ok()["Modified"]?.items)
            guard mutations.count == 1, mutations[0]["result"]?["Ok"]?["Saved"] != nil else { throw ServiceWorkflowFailure.missingReceipt }
        }
    }
    func read(zone: String, name: String, all: Bool) async throws -> ServiceOriginalReply {
        try await call(.object(["Read": .object(["zone": .string(zone), "name": .string(name),
            "fields": all ? .string("All") : .object(["Named": .array([.string("e")])])])]))
    }
    func fetch(zone: String, cursor: Data?, instance: String? = nil) async throws -> ServiceOriginalReply {
        let token = try cursor.map { try JSONDecoder().decode(ServiceJSON.self, from: $0) } ?? .null
        return try await call(.object(["FetchZone": .object(["zone": .string(zone), "token": token])]), instance: instance)
    }
    func database(cursor: Data?, instance: String) async throws -> ServiceOriginalReply {
        let token = try cursor.map { try JSONDecoder().decode(ServiceJSON.self, from: $0) } ?? .null
        return try await call(.object(["FetchDatabase": .object(["token": token])]), instance: instance)
    }
    func modify(zone: String, changes: [ServiceJSON], instance: String? = nil) async throws -> ServiceOriginalReply {
        try await call(.object(["Modify": .object(["zone": .string(zone), "atomic": .bool(false), "changes": .array(changes)])]), instance: instance)
    }
    nonisolated func saveInput(_ record: CloudRecord) throws -> ServiceJSON {
        var fields: [String: ServiceJSON] = ["e": .object(["EncryptedBytes": .bytes(record.envelope)])]
        if let url = record.assetURL {
            let bytes = try Data(contentsOf: url)
            let field = record.recordType == "MGBlob" ? "a" : "pa"
            fields[field] = .object(["Asset": .object(["bytes": .bytes(bytes), "sha256": .bytes(Data(SHA256.hash(data: bytes)))])])
        }
        return .object(["Save": .object(["record": .object(["name": .string(record.name), "record_type": .string(record.recordType), "fields": .object(fields)]),
            "if_unchanged": record.systemFields.map(ServiceJSON.bytes) ?? .null])])
    }
    func retainAsset(_ bytes: Data?, payload: Bool) throws -> CloudMediaStagedAsset? {
        guard let bytes else { return nil }
        let source = sourceDirectory.appending(path: UUID().uuidString)
        try bytes.write(to: source, options: .atomic)
        defer { try? FileManager.default.removeItem(at: source) }
        if payload { return try staging.copyPayload(source, cancelled: { false }, retainedAssetLimit: 250) }
        return try staging.copy(source, cancelled: { false })
    }
    func recordReceipts(_ original: ServiceOriginalReply, zone: String) throws {
        for mutation in try serviceRequired(try original.ok()["Modified"]?.items) {
            if let raw = mutation["result"]?["Ok"]?["Saved"] {
                let record = try ServiceRecordBytes(raw)
                receipts.append(.init(original: original, zone: zone, record: record.record(zone: zone)))
            }
        }
    }
    func own(_ command: CloudOwnTransportCommand, replies: [ServiceOriginalReply]) throws -> CloudOwnTransportReply {
        // Every reply belongs to a frame accepted by the submit guard. This
        // method only projects those original outcomes; it issues no request.
        let reply = try serviceRequired(replies.first)
        switch command {
        case .zoneInventory:
            guard replies.count == 1 else { throw ServiceWorkflowFailure.wire }
            let values = try serviceRequired(try reply.ok()["AllZones"]?.items)
            let zones = try values.map { try serviceRequired($0.text) }
            guard zones == zones.sorted(), Set(zones).count == zones.count else { throw ServiceWorkflowFailure.wire }
            return .zoneInventory(zones: zones)
        case .zoneDeletionActiveRead:
            if let error = reply.result["Err"] {
                let failure = try deletionFailure(error)
                let read: CloudControlRead = failure.code == 11 ? .missing : (failure.code == 26 ? .zoneMissing : .failed)
                return .zoneDeletionActiveRead(.init(read: read, failure: failure))
            }
            return .zoneDeletionActiveRead(.init(read: try controlRead(reply)))
        case .deleteZones(let batch):
            guard replies.count == batch.zones.count else { throw ServiceWorkflowFailure.wire }
            let results = try zip(batch.zones, replies).map { zone, reply -> CloudZoneDeletionResult in
                if let error = reply.result["Err"] { return .init(zone: zone, failure: try deletionFailure(error)) }
                guard try reply.ok().text == "Done" else { throw ServiceWorkflowFailure.wire }
                return .init(zone: zone, failure: nil)
            }
            return .deletedZones(.init(results: results))
        case .read: return .read(try controlRead(reply))
        case .publication(.read): return .publication(.read(try controlRead(reply)))
        case .publication(.createZones(let zones)):
            guard replies.count == zones.count else { throw ServiceWorkflowFailure.wire }
            for reply in replies {
                guard try reply.ok().text == "Done" else { throw ServiceWorkflowFailure.wire }
            }
            return .publication(.zonesCreated)
        case .publication(.save(let target, _, _)):
            let result = try serviceRequired(try reply.ok()["Modified"]?.items?.first?["result"])
            if let saved = result["Ok"]?["Saved"] {
                try recordReceipts(reply, zone: target.zone)
                return .publication(.saved(try ServiceRecordBytes(saved).control))
            }
            if result["Err"]?["code"]?.text == "ServerRecordChanged", let server = result["Err"]?["server_record"] {
                return .publication(.conflict(try ServiceRecordBytes(server).control))
            }
            throw ServiceWorkflowFailure.server(result["Err"]?["code"]?.text ?? "malformed-save")
        case .adoptionPage:
            let page = try serviceRequired(try reply.ok()["ZoneChanges"])
            var records: [CloudStagingRecord] = [], deleted: [String] = []
            for change in try serviceRequired(page["changes"]?.items) {
                if let saved = change["Saved"] {
                    let row = try ServiceRecordBytes(saved)
                    let lease = try retainAsset(row.asset("pa"), payload: true)
                    records.append(.init(name: row.name, recordType: row.recordType, record: row.control, payloadLease: lease))
                } else if let name = change["Deleted"]?["name"]?.text { deleted.append(name) }
                else { throw ServiceWorkflowFailure.wire }
            }
            let more = try serviceRequired(page["more"]?.flag)
            return .page(records: records, deleted: deleted, next: more ? try serviceRequired(page["token"]).encoded() : nil)
        case .assetRead(let zone, _):
            let row = try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"]))
            let lease = try retainAsset(row.asset("pa"), payload: true)
            return .assetRead(try .init(record: row.record(zone: zone, lease: lease), asset: lease))
        }
    }
    private func deletionFailure(_ error: ServiceJSON) throws -> CloudZoneDeletionFailure {
        let code: Int64
        switch try serviceRequired(error["code"]?.text) {
        case "UnknownItem": code = 11
        case "ZoneNotFound": code = 26
        case "UserDeletedZone": code = 28
        case "ServerRecordChanged": code = 14
        case "InvalidArguments": code = 12
        case "LimitExceeded": code = 27
        case "QuotaExceeded": code = 25
        case "BatchRequestFailed": code = 22
        case "ChangeTokenExpired": code = 21
        default: throw ServiceWorkflowFailure.wire
        }
        return .init(code: code, encryptedDataReset: error["encrypted_data_reset"]?.flag == true)
    }
    private func controlRead(_ reply: ServiceOriginalReply) throws -> CloudControlRead {
        if let error = reply.result["Err"] {
            switch error["code"]?.text {
            case "UnknownItem": return .missing
            case "ZoneNotFound": return .zoneMissing
            default: throw ServiceWorkflowFailure.server(error["code"]?.text ?? "malformed-read")
            }
        }
        return .found(try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"])).control)
    }
    func media(_ request: CloudMediaRequest, replies: [ServiceOriginalReply]) throws -> CloudMediaTransportReply {
        if case .upload = request { throw CloudMediaFailure.requiresSyncEngine }
        if case .reads(let names) = request {
            guard replies.count == names.count else { throw ServiceWorkflowFailure.wire }
            return .init(result: .blobReads(try zip(names, replies).map { name, reply in
                try CloudMediaBlobReadResult(name: name, result: mediaResult(.read(name: name), reply: reply))
            }))
        }
        guard replies.count == 1 else { throw ServiceWorkflowFailure.wire }
        return .init(result: try mediaResult(request, reply: replies[0]))
    }
    private func mediaResult(_ request: CloudMediaRequest, reply: ServiceOriginalReply) throws -> CloudMediaReadResult {
        let name: String, download: Bool
        switch request {
        case .root: name = "root"; download = false
        case .read(let n): name = n; download = false
        case .download(let n): name = n; download = true
        default: throw ServiceWorkflowFailure.unsupported
        }
        if let error = reply.result["Err"] {
            switch error["code"]?.text {
            case "UnknownItem": return .unknownItem
            case "ZoneNotFound": return .zoneMissing
            default: throw ServiceWorkflowFailure.server(error["code"]?.text ?? "malformed-media")
            }
        }
        let row = try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"]))
        let lease: CloudMediaStagedAsset?
        if download {
            let bytes = try serviceRequired(row.asset("a"))
            lease = try retainAsset(bytes, payload: false)
        } else { lease = nil }
        guard row.name == name else { throw ServiceWorkflowFailure.wire }
        return .record(name: row.name, recordType: row.recordType, envelope: row.envelope, asset: lease)
    }
}
#endif
