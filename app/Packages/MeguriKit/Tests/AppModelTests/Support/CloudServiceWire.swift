#if os(macOS)
@testable import CoreBridge
import CryptoKit
import Foundation

// Small, synthetic Service wire only. Tokens and original Response JSON remain
// opaque transport values; none of these bytes are CKRecord archives.
nonisolated enum ServiceJSON: Codable, Sendable {
    case object([String: Self]), array([Self]), string(String), integer(Int64), bool(Bool), null
    case byteBuffer(Data)
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
        case .byteBuffer(let v): try c.encode(Array(v)) // Explicit JSON stays an integer array, never base64.
        }
    }
    subscript(_ key: String) -> Self? { if case .object(let v) = self { v[key] } else { nil } }
    var items: [Self]? { if case .array(let v) = self { v } else { nil } }
    var text: String? { if case .string(let v) = self { v } else { nil } }
    var flag: Bool? { if case .bool(let v) = self { v } else { nil } }
    var number: Int64? { if case .integer(let v) = self { v } else { nil } }
    static func bytes(_ v: Data) -> Self { .byteBuffer(v) }
    func bytes() throws -> Data {
        if case .byteBuffer(let data) = self { return data }
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
// Original logical Service request/response JSON. In binaryAssetsV1 these are
// normalized after attachment reconstruction, not the bytes on the pipe. Raw
// headers/bodies belong to CloudServiceProcess.transportExchanges().
nonisolated struct ServiceOriginalReply: Sendable {
    let originalRequest: ServiceJSON
    let originalResponse: ServiceJSON
    var originalRequestJSON: Data { get throws { try originalRequest.encoded() } }
    var originalResponseJSON: Data { get throws { try originalResponse.encoded() } }
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
// Actual per-record conflict fields, retained alongside their original request
// and response. Observation only; this never substitutes an SDK result.
nonisolated struct ServiceSaveConflictReceipt: Sendable {
    let original: ServiceOriginalReply
    let zone: String
    let name: String
    let code: String
    let server: ServiceRecordBytes
}
// Original accepted Read receipts, never fabricated server records or ACKs.
nonisolated struct ServiceAssetReadObservation: Sendable {
    let original: ServiceOriginalReply
    let zone: String
    let record: CloudRecord
    let failedDelivery: Bool
    let retainedLease: Bool
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

nonisolated func serviceRequestMessage(_ operation: ServiceJSON, instance: String? = nil, ownOperation: String? = nil) -> ServiceJSON {
    .object([
        "scope": .object(["container": .string("synthetic-initial-app"), "environment": .string("development"), "account": .string("synthetic-initial-account")]),
        "caller": .object(["device": .string("synthetic-app-workflow"), "instance": instance.map(ServiceJSON.string) ?? .null, "own_operation": ownOperation.map(ServiceJSON.string) ?? .null]),
        "operation": operation,
    ])
}
nonisolated func serviceRequestFrame(_ operation: ServiceJSON, instance: String? = nil, ownOperation: String? = nil) throws -> Data {
    try serviceRequestMessage(operation, instance: instance, ownOperation: ownOperation).encoded()
}
nonisolated func serviceOriginalReply(request: ServiceJSON, response: ServiceJSON) throws -> ServiceOriginalReply {
    try .init(originalRequest: request, originalResponse: response,
        requestID: serviceRequired(response["request_id"]?.number), result: serviceRequired(response["result"]))
}
nonisolated func serviceOriginalReply(request: Data, response: Data) throws -> ServiceOriginalReply {
    try serviceOriginalReply(request: JSONDecoder().decode(ServiceJSON.self, from: request),
        response: JSONDecoder().decode(ServiceJSON.self, from: response))
}

// Failure diagnostics inspect only completed raw packet headers. Never rebuild
// logical asset JSON or render record fields, identifiers, tokens or envelopes.
nonisolated func serviceTransportSummary(_ process: CloudServiceProcess, after completedBefore: Int = 0) -> String {
    let exchanges = process.transportExchanges().dropFirst(completedBefore)
    let allowedKinds = ["CreateZone", "DeleteZone", "FetchDatabase", "FetchZone", "Modify", "Read"]
    var kinds: [String: Int] = [:]
    var saved = 0
    var unreadableHeaders = 0
    var requestSizes = ServiceTransportSizes(), responseSizes = ServiceTransportSizes()
    for exchange in exchanges {
        let request = requestSizes.observe(exchange.request.header)
        let response = responseSizes.observe(exchange.response.header)
        if request == nil { unreadableHeaders += 1 }
        if response == nil { unreadableHeaders += 1 }
        let operation = request?["operation"] as? [String: Any]
        let kind = allowedKinds.first { operation?[$0] != nil } ?? "other"
        kinds[kind, default: 0] += 1
        let result = response?["result"] as? [String: Any]
        let ok = result?["Ok"] as? [String: Any]
        for mutation in ok?["Modified"] as? [[String: Any]] ?? [] {
            let result = mutation["result"] as? [String: Any]
            let ok = result?["Ok"] as? [String: Any]
            if ok?["Saved"] != nil { saved += 1 }
        }
    }
    let counts = kinds.keys.sorted().map { "\($0)=\(kinds[$0]!)" }.joined(separator: ",")
    return "Service transport completed_before=\(completedBefore) completed_count=\(exchanges.count) kinds=[\(counts)] saved_count=\(saved) unreadable_headers=\(unreadableHeaders) request=[\(requestSizes.summary)] response=[\(responseSizes.summary)]"
}
private nonisolated struct ServiceTransportSizes {
    var headerTotal = 0, headerMax = 0, bodyCount = 0, bodyTotal = 0, bodyMax = 0
    mutating func observe(_ header: Data) -> [String: Any]? {
        headerTotal += header.count
        headerMax = max(headerMax, header.count)
        guard let root = (try? JSONSerialization.jsonObject(with: header)) as? [String: Any] else { return nil }
        if let lengths = root["lengths"] as? [Int] {
            bodyCount += lengths.count
            bodyTotal += lengths.reduce(0, +)
            bodyMax = max(bodyMax, lengths.max() ?? 0)
        }
        return root["message"] as? [String: Any] ?? root
    }
    var summary: String {
        "header_bytes_total=\(headerTotal) header_bytes_max=\(headerMax) body_count=\(bodyCount) body_bytes_total=\(bodyTotal) body_bytes_max=\(bodyMax)"
    }
}

actor ServiceWorkflowWire {
    let process: CloudServiceProcess
    let staging: CloudMediaStaging
    private let sourceDirectory: URL
    private(set) var receipts: [ServiceSavedReceipt] = []
    private(set) var saveConflicts: [ServiceSaveConflictReceipt] = []
    private struct AssetDeliveryFault {
        let zone: String
        let name: String
        let envelope: Data
        let systemFields: Data?
        let payload: Data
        var missingFetchDelivery = true
        var failedReadsRemaining = 5
    }
    private var assetDeliveryFault: AssetDeliveryFault?
    private(set) var withheldPayloadDeliveries = 0
    private(set) var assetReadObservations: [ServiceAssetReadObservation] = []

    // Opt-in and confined to one real Service version/payload. Every other
    // caller retains the existing transport behavior. Arm only after B closes.
    func withholdAssetAndFailFiveReads(record: CloudRecord, payload: Data) throws {
        guard assetDeliveryFault == nil, let zone = record.zone, !payload.isEmpty else {
            throw ServiceWorkflowFailure.wire
        }
        assetDeliveryFault = .init(zone: zone, name: record.name, envelope: record.envelope,
            systemFields: record.systemFields, payload: payload)
    }
    func restoreAssetDelivery() throws {
        guard let fault = assetDeliveryFault, !fault.missingFetchDelivery,
              fault.failedReadsRemaining == 0 else { throw ServiceWorkflowFailure.wire }
        assetDeliveryFault = nil
    }
    init(process: CloudServiceProcess, directory: URL) throws {
        self.process = process
        sourceDirectory = directory.appending(path: "callback-sources")
        try FileManager.default.createDirectory(at: sourceDirectory, withIntermediateDirectories: true)
        staging = try CloudMediaStaging(directory: directory.appending(path: "leases"))
    }
    func call(_ operation: ServiceJSON, instance: String? = nil) async throws -> ServiceOriginalReply {
        let message = serviceRequestMessage(operation, instance: instance)
        let response = try await process.request(message: message)
        return try serviceOriginalReply(request: message, response: response)
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
        if payload, var fault = assetDeliveryFault, fault.missingFetchDelivery, bytes == fault.payload {
            fault.missingFetchDelivery = false
            assetDeliveryFault = fault
            withheldPayloadDeliveries += 1
            return nil
        }
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
            } else if let error = mutation["result"]?["Err"], error["code"]?.text == "ServerRecordChanged" {
                saveConflicts.append(try .init(original: original, zone: zone,
                    name: serviceRequired(mutation["name"]?.text), code: serviceRequired(error["code"]?.text),
                    server: ServiceRecordBytes(serviceRequired(error["server_record"]))))
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
        case .read, .adoptionActive: return .read(try controlRead(reply))
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
        case .adoptionPayload(let zone, let name, let expected):
            guard replies.count == 1,
                  reply.originalRequest["operation"]?["Read"]?["zone"]?.text == zone,
                  reply.originalRequest["operation"]?["Read"]?["name"]?.text == name else {
                throw ServiceWorkflowFailure.wire
            }
            let row = try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"]))
            guard row.name == name, row.recordType == "MGEntity",
                  let encrypted = expected.encrypted, row.envelope == encrypted,
                  row.systemFields == expected.systemFields else { throw ServiceWorkflowFailure.wire }
            let lease = try serviceRequired(try retainAsset(row.asset("pa"), payload: true))
            return .adoptionPayload(.init(name: row.name, recordType: row.recordType,
                record: row.control, payloadLease: lease))
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
            // The original accepted Read has already drained through Service.
            // Only its delivery fails; selection/accounting stay in Rust.
            if var fault = assetDeliveryFault, zone == fault.zone, row.name == fault.name {
                assetReadObservations.append(.init(original: reply, zone: zone,
                    record: row.record(zone: zone), failedDelivery: true, retainedLease: false))
                guard row.envelope == fault.envelope, row.systemFields == fault.systemFields,
                      try row.asset("pa") == fault.payload, fault.failedReadsRemaining > 0 else {
                    throw ServiceWorkflowFailure.wire
                }
                fault.failedReadsRemaining -= 1
                assetDeliveryFault = fault
                return .assetRead(.failed)
            }
            let lease = try retainAsset(row.asset("pa"), payload: true)
            assetReadObservations.append(.init(original: reply, zone: zone,
                record: row.record(zone: zone), failedDelivery: false, retainedLease: lease != nil))
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
