#if os(macOS)
import CryptoKit
import Foundation
import Testing

@Suite nonisolated struct CloudServiceProcessTests {
    @Test func binaryAssetsPreparedCancellationDrainsBodiesAndConcurrentFramesStayAligned() async throws {
        try await withService(transport: .binaryAssetsV1) { client in
            try await createZone(client)
            let payload = Data((0..<960_000).map { UInt8($0 % 256) })
            let saveRequest = try assetFrame(payload)
            let upload = try client.prepare(saveRequest)
            #expect(upload.enqueue())
            #expect(!upload.enqueue())
            let savedData = try await upload.reply()
            let savedReply = try serviceOriginalReply(request: saveRequest, response: savedData)
            let raw = try serviceRequired(try savedReply.ok()["Modified"]?.items?.first?["result"]?["Ok"]?["Saved"])
            let saved = try ServiceRecordBytes(raw)
            #expect(try saved.asset("pa") == payload)
            #expect(savedData.count > CloudServiceBinary.headerLimit) // Logical JSON, not pipe bytes.
            _ = try await save(client, name: "next", bytes: [1, 128, 0])

            let (written, signal) = AsyncStream<Void>.makeStream()
            let release = DispatchSemaphore(value: 0)
            defer { release.signal(); signal.finish() }
            let prepared = try client.prepare(readFrame("large"), afterWrite: {
                signal.yield(())
                release.wait()
            })
            #expect(prepared.enqueue())
            #expect(!prepared.enqueue())
            var iterator = written.makeAsyncIterator()
            _ = await iterator.next()
            prepared.cancel() // Already accepted: the entire original body still belongs to the queue.
            let following = try client.prepare(readFrame("next"))
            #expect(following.enqueue())
            release.signal()
            do {
                _ = try await prepared.reply()
                Issue.record("cancelled prepared exchange returned success")
            } catch is CancellationError {} catch { throw error }
            let drained = try await prepared.reply(preservingCancelledResult: true)
            let read = try serviceOriginalReply(request: readFrame("large"), response: drained)
            #expect(try ServiceRecordBytes(serviceRequired(try read.ok()["Record"])).asset("pa") == payload)
            let next = try decode(ServiceResponse<RecordReply>.self, await following.reply())
            #expect(next.result.Ok?.Record.name == "next")
            try await withThrowingTaskGroup(of: Bool.self) { group in
                for name in ["large", "next", "large", "next"] {
                    group.addTask {
                        let request = try readFrame(name)
                        let reply = try serviceOriginalReply(request: request, response: await client.request(request))
                        let row = try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"]))
                        let asset = try row.asset("pa")
                        return row.name == name && (name != "large" || asset == payload)
                    }
                }
                for try await matched in group { #expect(matched) }
            }
            let refused = try client.prepare(readFrame("large"))
            refused.cancel()
            #expect(!refused.enqueue())
            let ledger = client.transportExchanges()
            #expect(ledger.count == 9) // create, two saves, two prepared reads, four concurrent reads.
            let ids = try ledger.map { exchange -> Int64 in
                #expect(exchange.request.header.count <= CloudServiceBinary.headerLimit)
                #expect(exchange.response.header.count <= CloudServiceBinary.headerLimit)
                let header = try CloudServiceBinary.replyHeader(exchange.response.header)
                return try serviceRequired(header.message["request_id"]?.number)
            }
            #expect(Set(ids).count == ids.count && ids == ids.sorted())
            #expect(ledger[1].request.bodies == [payload])
            #expect(ledger[1].response.bodies == [payload])
            #expect(ledger[3].response.bodies == [payload])
        }
    }

    @Test func binaryAssetsPreparationRejectsBoundsBeforeAcceptanceOrIO() async throws {
        try await withService(transport: .binaryAssetsV1) { client in
            let fixtureInput = try assetFrame(Data([0, 10, 255]))
            let decodedInput = try JSONDecoder().decode(ServiceJSON.self, from: fixtureInput)
            let assetValue = try serviceRequired(decodedInput["operation"]?["Modify"]?["changes"]?.items?.first?["Save"]?["record"]?["fields"]?["pa"])
            let repeatedFields = Dictionary(uniqueKeysWithValues: (0..<17).map { ("asset\($0)", assetValue) })
            let recordInput: [String: Any] = [
                "name": "large", "record_type": "MGEntity",
                "fields": try JSONSerialization.jsonObject(with: ServiceJSON.object(repeatedFields).encoded()),
            ]
            let requestWithSeventeenBodies = try frame(["Modify": [
                "zone": "z", "atomic": true,
                "changes": [["Save": ["record": recordInput, "if_unchanged": NSNull()]]],
            ]])
            do {
                _ = try client.prepare(requestWithSeventeenBodies)
                Issue.record("accepted too many attachment bodies")
            } catch CloudServiceProcessFailure.fixtureAttachmentLimit {} catch { throw error }
            // Validate the exact shared bounds without allocating advertised bodies.
            for lengths in [[CloudServiceBinary.bodyLimit + 1],
                            [CloudServiceBinary.bodyLimit, CloudServiceBinary.bodyLimit, 1]] {
                let header = try ServiceJSON.object(["version": .integer(1),
                    "lengths": .array(lengths.map { .integer(Int64($0)) }), "message": .null]).encoded()
                do {
                    _ = try CloudServiceBinary.replyHeader(header)
                    Issue.record("accepted oversized body bounds")
                } catch CloudServiceProcessFailure.fixtureAttachmentLimit {} catch { throw error }
            }
            let noIO = client.transportExchanges().isEmpty
            #expect(noIO)
            try await createZone(client) // This same process can still handle a valid request.
            #expect(client.transportExchanges().count == 1)
        }
    }

    @Test func realSaveReadAndStaleConditionalConflictPreserveOpaqueBytes() async throws {
        try await withService { client in
            try await createZone(client)
            let original = try await save(client, name: "entity", bytes: [0, 255, 128, 10, 34, 92])
            let originalPayloadMatches = original.fields["e"]?.EncryptedBytes == [0, 255, 128, 10, 34, 92]
            #expect(originalPayloadMatches)
            #expect(!original.system_fields.isEmpty)
            let read = try await readRecord(client, name: "entity")
            #expect(read == original)
            let updated = try await save(client, name: "entity", bytes: [9, 0, 254], ifUnchanged: original.system_fields)
            let updatedPayloadMatches = updated.fields["e"]?.EncryptedBytes == [9, 0, 254]
            let systemFieldsChanged = updated.system_fields != original.system_fields
            #expect(updatedPayloadMatches)
            #expect(systemFieldsChanged)
            let response = try decode(ServiceModified.self, await client.request(saveFrame(
                name: "entity", bytes: [7], ifUnchanged: original.system_fields)))
            let conflict = try required(response.result.Ok?.Modified.first?.result.Err)
            #expect(conflict.code == "ServerRecordChanged")
            #expect(conflict.server_record == updated)
            let afterConflict = try await readRecord(client, name: "entity")
            #expect(afterConflict == updated)
        }
    }

    @Test func separateClientsAndConcurrentRequestsCannotCrossMatchReplies() async throws {
        try await withService { first in
            try await withService { second in
                for client in [first, second] { try await createZone(client) }
                let one = try await save(first, name: "same-name", bytes: [1, 0, 255])
                let two = try await save(second, name: "same-name", bytes: [2, 0, 255])
                var expected: [ServiceRecord] = []
                for index in 0..<8 {
                    expected.append(try await save(first, name: "r\(index)", bytes: [UInt8(index), 128]))
                }
                try await withThrowingTaskGroup(of: Bool.self) { group in
                    for record in expected {
                        group.addTask { try await readRecord(first, name: record.name) == record }
                    }
                    group.addTask { try await readRecord(first, name: "same-name") == one }
                    group.addTask { try await readRecord(second, name: "same-name") == two }
                    for try await matched in group { #expect(matched) }
                }
            }
        }
    }

    @Test func helperStateSurvivesCloseAndExplicitNewProcessAtOwnedRoot() async throws {
        let root = try makeRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let first = try await CloudServiceProcess.open(stateDirectory: root)
        let record: ServiceRecord
        do {
            try await createZone(first)
            record = try await save(first, name: "durable", bytes: [128, 0, 255])
            expectClean(await first.close())
        } catch { _ = await first.cancel(); throw error }
        let reopened = try await CloudServiceProcess.open(stateDirectory: root)
        do {
            let read = try await readRecord(reopened, name: "durable")
            #expect(read == record)
            expectClean(await reopened.close())
        } catch { _ = await reopened.cancel(); throw error }
    }

    @Test func malformedProtocolDoesNotEchoSentinelAndCloseJoinsOriginalHelper() async throws {
        try await withService(expectedStatus: 2) { client in
            let sentinel = "synthetic-body-must-not-appear-in-errors"
            let reply = try await client.request(Data("{\"\(sentinel)\":".utf8))
            let exactReply = reply == Data("{\"protocol_error\":\"malformed-frame\"}".utf8)
            #expect(exactReply)
            let exit = await client.close()
            #expect(exit.status == 2 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
            let exactDiagnostics = exit.diagnostics == Data("cloud service protocol-error: malformed-frame\n".utf8)
            #expect(exactDiagnostics)
            let sentinelData = Data(sentinel.utf8)
            let replyContainsSentinel: Bool = reply.range(of: sentinelData) != nil
            let diagnosticsContainSentinel: Bool = exit.diagnostics.range(of: sentinelData) != nil
            #expect(!replyContainsSentinel)
            #expect(!diagnosticsContainSentinel)
            #expect(await client.close() == exit)
            #expect(await client.cancel() == exit)
        }
    }

    @Test func eofCloseIsIdempotentAndExplicitFixtureCancellationJoins() async throws {
        try await withService { client in
            let exit = await client.close()
            expectClean(exit)
            #expect(await client.close() == exit)
            #expect(await client.cancel() == exit)
        }
        let root = try makeRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let client = try await CloudServiceProcess.open(stateDirectory: root)
        let cancelled = await client.cancel()
        #expect(cancelled.joined && !cancelled.ioFailed)
        #expect(cancelled.status != 0)
        #expect(await client.close() == cancelled)
    }

    @Test func cancelledCallerDrainsOriginalReplyBeforeNextRequest() async throws {
        try await withService { client in
            try await createZone(client)
            let cancelledRecord = try await save(client, name: "cancelled", bytes: [0, 255])
            let nextRecord = try await save(client, name: "next", bytes: [128, 1])
            let (written, signal) = AsyncStream<Void>.makeStream()
            let (queued, submitted) = AsyncStream<Void>.makeStream()
            let release = DispatchSemaphore(value: 0)
            defer { release.signal(); signal.finish(); submitted.finish() }
            let original = Task {
                try await client.request(readFrame("cancelled"), afterWrite: {
                    signal.yield(())
                    release.wait()
                })
            }
            var iterator = written.makeAsyncIterator()
            _ = await iterator.next()
            original.cancel()
            let next = Task {
                try await client.request(readFrame("next"), afterEnqueue: { submitted.yield(()) })
            }
            var queuedIterator = queued.makeAsyncIterator()
            _ = await queuedIterator.next()
            release.signal()
            do {
                _ = try await original.value
                Issue.record("cancelled caller returned success")
            } catch is CancellationError {
                // The queue still consumed the cancelled record's original LF.
            } catch { throw error }
            let nextReply = try decode(ServiceResponse<RecordReply>.self, await next.value)
            let nextRead = try required(nextReply.result.Ok?.Record)
            #expect(nextRead == nextRecord)
            let originalRead = try await readRecord(client, name: "cancelled")
            #expect(originalRead == cancelledRecord)
        }
    }
}

// Only wire encoding/decoding lives here. Service decides every save/conflict.
// Opaque ServiceSystemFields are byte arrays, never CKRecord archives.
private nonisolated struct ServiceRecord: Decodable, Sendable, Equatable, CustomStringConvertible {
    struct EncryptedField: Decodable, Sendable, Equatable { let EncryptedBytes: [UInt8] }
    let name: String
    let record_type: String
    let fields: [String: EncryptedField]
    let system_fields: [UInt8]
    var description: String { "ServiceRecord" }
}
private nonisolated struct ServiceError: Decodable, Sendable, CustomStringConvertible {
    let code: String
    let server_record: ServiceRecord?
    var description: String { "ServiceError" }
}
private nonisolated struct ServiceResult<Value: Decodable & Sendable>: Decodable, Sendable {
    let Ok: Value?
    let Err: ServiceError?
}
private nonisolated struct ServiceResponse<Value: Decodable & Sendable>: Decodable, Sendable {
    let request_id: UInt64
    let result: ServiceResult<Value>
}
private nonisolated struct RecordReply: Decodable, Sendable { let Record: ServiceRecord }
private nonisolated struct SavedReply: Decodable, Sendable { let Saved: ServiceRecord }
private nonisolated struct MutationReply: Decodable, Sendable { let result: ServiceResult<SavedReply> }
private nonisolated struct ModifiedReply: Decodable, Sendable { let Modified: [MutationReply] }
private typealias ServiceModified = ServiceResponse<ModifiedReply>
private nonisolated enum ServiceSmokeFailure: Error { case wire, missing }

private nonisolated func decode<Value: Decodable>(_ type: Value.Type, _ data: Data) throws -> Value {
    do { return try JSONDecoder().decode(type, from: data) }
    catch { throw ServiceSmokeFailure.wire }
}
private nonisolated func required<Value>(_ value: Value?) throws -> Value {
    guard let value else { throw ServiceSmokeFailure.missing }
    return value
}
private nonisolated func frame(_ operation: [String: Any]) throws -> Data {
    do {
        return try JSONSerialization.data(withJSONObject: [
            "scope": ["container": "fixture-container", "environment": "development", "account": "fixture-account"],
            "caller": ["device": "fixture-device", "instance": "fixture-instance", "own_operation": "fixture-operation"],
            "operation": operation,
        ])
    } catch { throw ServiceSmokeFailure.wire }
}
private nonisolated func createZone(_ client: CloudServiceProcess) async throws {
    let response = try decode(ServiceResponse<String>.self,
                             await client.request(frame(["CreateZone": ["zone": "z"]])))
    #expect(response.result.Ok == "Done")
}
private nonisolated func assetFrame(_ payload: Data) throws -> Data {
    try frame(["Modify": ["zone": "z", "atomic": true, "changes": [["Save": [
        "record": ["name": "large", "record_type": "MGEntity", "fields": [
            "e": ["EncryptedBytes": [0, 255, 10]],
            "pa": ["Asset": ["bytes": Array(payload), "sha256": Array(SHA256.hash(data: payload))]],
        ]], "if_unchanged": NSNull(),
    ]]]]])
}
private nonisolated func saveFrame(name: String, bytes: [UInt8], ifUnchanged: [UInt8]? = nil) throws -> Data {
    try frame(["Modify": ["zone": "z", "atomic": true, "changes": [["Save": [
        "record": ["name": name, "record_type": "MGEntity", "fields": ["e": ["EncryptedBytes": bytes]]],
        "if_unchanged": ifUnchanged.map { $0 as Any } ?? NSNull(),
    ]]]]])
}
private nonisolated func save(_ client: CloudServiceProcess, name: String, bytes: [UInt8],
                              ifUnchanged: [UInt8]? = nil) async throws -> ServiceRecord {
    let response = try decode(ServiceModified.self,
                             await client.request(saveFrame(name: name, bytes: bytes, ifUnchanged: ifUnchanged)))
    return try required(response.result.Ok?.Modified.first?.result.Ok?.Saved)
}
private nonisolated func readFrame(_ name: String) throws -> Data {
    try frame(["Read": ["zone": "z", "name": name, "fields": "All"]])
}
private nonisolated func readRecord(_ client: CloudServiceProcess, name: String) async throws -> ServiceRecord {
    let response = try decode(ServiceResponse<RecordReply>.self, await client.request(readFrame(name)))
    return try required(response.result.Ok?.Record)
}
private nonisolated func makeRoot() throws -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("meguri-cloud-service-\(UUID().uuidString)")
    do { try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700]) }
    catch { throw CloudServiceProcessFailure.configuration }
    return root
}
private nonisolated func expectClean(_ exit: CloudServiceProcessExit) {
    #expect(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
    #expect(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
}
private nonisolated func withService(expectedStatus: Int32 = 0, transport: CloudServiceTransport = .legacy,
    _ body: @Sendable (CloudServiceProcess) async throws -> Void) async throws {
    let root = try makeRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let client = try await CloudServiceProcess.open(stateDirectory: root, transport: transport)
    do {
        try await body(client)
        let exit = await client.close()
        #expect(exit.status == expectedStatus && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        #expect(!exit.diagnosticLimitExceeded)
        if expectedStatus == 0 { #expect(exit.diagnostics.isEmpty) }
    } catch { _ = await client.cancel(); throw error }
}
#endif
