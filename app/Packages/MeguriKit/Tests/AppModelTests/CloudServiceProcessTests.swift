#if os(macOS)
import CryptoKit
import Foundation
import Testing

@Suite nonisolated struct CloudServiceProcessTests {
    @Test func explicitPageSizeUsesOriginalServicePaginationAndPreservesDefault() async throws {
        for binary in [false, true] {
            let transport: CloudServiceTransport = binary ? .binaryAssetsV1 : .legacy
            for explicit in [false, true] {
                let root = try makeRoot()
                defer { try? FileManager.default.removeItem(at: root) }
                let size = explicit ? 32 : 200
                let count = explicit ? 41 : 201
                let client = try await CloudServiceProcess.open(stateDirectory: root, transport: transport,
                                                               pageSize: explicit ? 32 : nil)
                var expected: [ServiceRecord] = []
                let first: ServiceZonePage
                let updated: ServiceRecord
                do {
                    try await createZone(client)
                    for index in 0..<count {
                        expected.append(try await save(client, name: String(format: "r%04d", index),
                                                       bytes: [UInt8(index), 0, 255]))
                    }
                    first = try await zonePage(client)
                    #expect(first.more)
                    #expect(first.changes.map(\.Saved) == Array(expected.prefix(size)))
                    let last = try required(expected.last)
                    updated = try await save(client, name: last.name, bytes: [99, 128, 0],
                                             ifUnchanged: last.system_fields)
                    #expect(updated.system_fields != last.system_fields)
                    let read = try await readRecord(client, name: last.name)
                    #expect(read == updated)
                    expectClean(await client.close())
                } catch { _ = await client.cancel(); throw error }

                // No override on reopen: retain the original snapshot and stored size.
                let reopened = try await CloudServiceProcess.open(stateDirectory: root, transport: transport)
                do {
                    let second = try await zonePage(reopened, token: first.token)
                    #expect(!second.more)
                    #expect(second.changes.map(\.Saved) == Array(expected.dropFirst(size)))
                    let replay = try await zonePage(reopened, token: first.token)
                    #expect(replay.more == second.more)
                    #expect(replay.changes.map(\.Saved) == second.changes.map(\.Saved))
                    let sameToken = try normalized(replay.token) == normalized(second.token)
                    #expect(sameToken)
                    let delta = try await zonePage(reopened, token: second.token)
                    #expect(!delta.more)
                    #expect(delta.changes.map(\.Saved) == [updated])
                    let fresh = try await zonePage(reopened)
                    #expect(fresh.more && fresh.changes.count == size)
                    expectClean(await reopened.close())
                } catch { _ = await reopened.cancel(); throw error }
            }
        }
    }

    @Test func invalidPageSizeIsRefusedBeforeProcessLaunch() async throws {
        let root = try makeRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let transports: [CloudServiceTransport] = [.legacy, .binaryAssetsV1]
        for transport in transports {
            for size in [Int.min, -1, 0, 201, Int.max] {
                do {
                    let client = try await CloudServiceProcess.open(stateDirectory: root, transport: transport, pageSize: size)
                    _ = await client.cancel()
                    Issue.record("accepted invalid initial page size")
                } catch CloudServiceProcessFailure.configuration {} catch { throw error }
                let contents = try FileManager.default.contentsOfDirectory(atPath: root.path)
                #expect(contents.isEmpty)
            }
        }
    }

    @Test(arguments: [false, true])
    func binaryAssetsPreparedCancellationDrainsBodiesAndConcurrentFramesStayAligned(typed: Bool) async throws {
        try await withService(transport: .binaryAssetsV1) { client in
            try await createZone(client)
            let payload = Data((0..<960_000).map { UInt8($0 % 256) })
            let saveRequest = assetMessage(payload)
            let upload = try prepare(client, message: saveRequest, typed: typed)
            #expect(upload.enqueue())
            #expect(!upload.enqueue())
            let savedReply = try serviceOriginalReply(request: saveRequest, response: await reply(upload, typed: typed))
            let raw = try serviceRequired(try savedReply.ok()["Modified"]?.items?.first?["result"]?["Ok"]?["Saved"])
            let saved = try ServiceRecordBytes(raw)
            #expect(try saved.asset("pa") == payload)
            if !typed {
                #expect(try savedReply.originalResponseJSON.count > CloudServiceBinary.headerLimit) // Logical JSON, not pipe bytes.
            }
            _ = try await save(client, name: "next", bytes: [1, 128, 0])

            let (written, signal) = AsyncStream<Void>.makeStream()
            let release = DispatchSemaphore(value: 0)
            defer { release.signal(); signal.finish() }
            let readMessage = try JSONDecoder().decode(ServiceJSON.self, from: readFrame("large"))
            let prepared = try prepare(client, message: readMessage, typed: typed, afterWrite: {
                signal.yield(())
                release.wait()
            })
            #expect(prepared.enqueue())
            #expect(!prepared.enqueue())
            var iterator = written.makeAsyncIterator()
            _ = await iterator.next()
            prepared.cancel() // Already accepted: the entire original body still belongs to the queue.
            let nextMessage = try JSONDecoder().decode(ServiceJSON.self, from: readFrame("next"))
            let following = try prepare(client, message: nextMessage, typed: typed)
            #expect(following.enqueue())
            release.signal()
            do {
                _ = try await reply(prepared, typed: typed)
                Issue.record("cancelled prepared exchange returned success")
            } catch is CancellationError {} catch { throw error }
            let drained = try await reply(prepared, typed: typed, preservingCancelledResult: true)
            let read = try serviceOriginalReply(request: readMessage, response: drained)
            #expect(try ServiceRecordBytes(serviceRequired(try read.ok()["Record"])).asset("pa") == payload)
            let next = try serviceOriginalReply(request: nextMessage, response: await reply(following, typed: typed))
            #expect(try next.ok()["Record"]?["name"]?.text == "next")
            try await withThrowingTaskGroup(of: Bool.self) { group in
                for name in ["large", "next", "large", "next"] {
                    group.addTask {
                        let message = try JSONDecoder().decode(ServiceJSON.self, from: readFrame(name))
                        let response = try await request(client, message: message, typed: typed)
                        let reply = try serviceOriginalReply(request: message, response: response)
                        let row = try ServiceRecordBytes(serviceRequired(try reply.ok()["Record"]))
                        let asset = try row.asset("pa")
                        return row.name == name && (name != "large" || asset == payload)
                    }
                }
                for try await matched in group { #expect(matched) }
            }
            let refused = try prepare(client, message: readMessage, typed: typed)
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

    @Test(arguments: [false, true])
    func binaryAssetsPreparationRejectsBoundsBeforeAcceptanceOrIO(typed: Bool) async throws {
        try await withService(transport: .binaryAssetsV1) { client in
            let decodedInput = assetMessage(Data([0, 10, 255]))
            let assetValue = try serviceRequired(decodedInput["operation"]?["Modify"]?["changes"]?.items?.first?["Save"]?["record"]?["fields"]?["pa"])
            let repeatedFields = Dictionary(uniqueKeysWithValues: (0..<17).map { ("asset\($0)", assetValue) })
            let requestWithSeventeenBodies = assetMessage(Data(), fields: repeatedFields)
            do {
                _ = try prepare(client, message: requestWithSeventeenBodies, typed: typed)
                Issue.record("accepted too many attachment bodies")
            } catch CloudServiceProcessFailure.fixtureAttachmentLimit {} catch { throw error }
            if typed {
                let payload = Data(count: CloudServiceBinary.bodyLimit + 1)
                do {
                    _ = try prepare(client, message: assetMessage(payload), typed: typed)
                    Issue.record("accepted oversized attachment body")
                } catch CloudServiceProcessFailure.fixtureAttachmentLimit {} catch { throw error }
            }
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

    @Test func typedAndDataAPIsPreserveOneMiBAssetsAndConditionalSavedReceipts() async throws {
        try await withService(transport: .binaryAssetsV1) { client in
            try await createZone(client)
            let payload = Data((0..<1_048_576).map { UInt8(truncatingIfNeeded: $0) })
            let message = assetMessage(payload)
            let typedPacket = try CloudServiceBinary.prepare(message: message, transport: .binaryAssetsV1)
            let dataPacket = try CloudServiceBinary.prepare(message.encoded(), transport: .binaryAssetsV1)
            #expect(typedPacket.bodies == [payload] && typedPacket.bodies == dataPacket.bodies)
            let typedHeader = try JSONDecoder().decode(ServiceJSON.self, from: typedPacket.header)
            let dataHeader = try JSONDecoder().decode(ServiceJSON.self, from: dataPacket.header)
            #expect(try normalized(typedHeader) == normalized(dataHeader))
            let headerRecord = try serviceRequired(typedHeader["message"]?["operation"]?["Modify"]?["changes"]?.items?.first?["Save"]?["record"])
            #expect(try headerRecord["fields"]?["e"]?["EncryptedBytes"]?.bytes() == Data([0, 255, 10]))
            #expect(try headerRecord["fields"]?["pa"]?["Asset"]?["sha256"]?.bytes() == Data(SHA256.hash(data: payload)))

            let saved = try serviceOriginalReply(request: message, response: await client.request(message: message))
            let savedRow = try ServiceRecordBytes(serviceRequired(try saved.ok()["Modified"]?.items?.first?["result"]?["Ok"]?["Saved"]))
            #expect(try savedRow.asset("pa") == payload)
            let readRequest = try readFrame("large")
            let oldRead = try serviceOriginalReply(request: readRequest, response: await client.request(readRequest))
            let oldRow = try ServiceRecordBytes(serviceRequired(try oldRead.ok()["Record"]))
            #expect(try oldRow.asset("pa") == payload)
            #expect(try normalized(savedRow.raw) == normalized(oldRow.raw))
            let conditional = assetMessage(payload, ifUnchanged: savedRow.systemFields)
            let updated = try serviceOriginalReply(request: conditional, response: await client.request(message: conditional))
            let updatedRow = try ServiceRecordBytes(serviceRequired(try updated.ok()["Modified"]?.items?.first?["result"]?["Ok"]?["Saved"]))
            #expect(try updatedRow.asset("pa") == payload)
            #expect(updatedRow.systemFields != savedRow.systemFields)
            let receiptRequest = try JSONDecoder().decode(ServiceJSON.self, from: updated.originalRequestJSON)
            let condition = try serviceRequired(receiptRequest["operation"]?["Modify"]?["changes"]?.items?.first?["Save"]?["if_unchanged"])
            #expect(try condition.bytes() == savedRow.systemFields)
            let receiptResponse = try JSONDecoder().decode(ServiceJSON.self, from: updated.originalResponseJSON)
            #expect(receiptResponse["request_id"]?.number == updated.requestID)
            #expect(saved.requestID < oldRead.requestID && oldRead.requestID < updated.requestID)
            let ledger = client.transportExchanges()
            #expect(ledger.count == 4)
            let ids = try ledger.map { try serviceRequired(CloudServiceBinary.replyHeader($0.response.header).message["request_id"]?.number) }
            #expect(ids == ids.sorted() && Set(ids).count == ids.count)
            #expect(ledger[1].request.bodies == [payload] && ledger[1].response.bodies == [payload])
            #expect(ledger[2].response.bodies == [payload] && ledger[3].response.bodies == [payload])
        }
        // Default legacy transport also accepts typed calls without changing JSON semantics.
        try await withService { client in
            try await createZone(client)
            let message = assetMessage(Data([0, 10, 255]))
            let saved = try serviceOriginalReply(request: message, response: await client.request(message: message))
            let row = try ServiceRecordBytes(serviceRequired(try saved.ok()["Modified"]?.items?.first?["result"]?["Ok"]?["Saved"]))
            #expect(try row.asset("pa") == Data([0, 10, 255]))
            #expect(client.transportExchanges().allSatisfy { $0.request.bodies.isEmpty && $0.response.bodies.isEmpty })
        }
    }

    @Test func binaryAssetsRejectMalformedReferencesAndKeepBuffersAsOrdinaryJSONOutsideAssets() throws {
        let buffer = ServiceJSON.bytes(Data([0, 10, 128, 255]))
        #expect(try buffer.bytes() == Data([0, 10, 128, 255]))
        #expect(try buffer.encoded() == Data("[0,10,128,255]".utf8))
        let index: ServiceJSON = .object(["attachment": .integer(0)])
        func response(_ assets: [ServiceJSON]) -> ServiceJSON {
            .object(["result": .object(["Ok": .object(["Record": .object(["fields": .object(
                Dictionary(uniqueKeysWithValues: assets.enumerated().map { ("a\($0.offset)", .object(["Asset": .object(["bytes": $0.element])])) })
            )])])])])
        }
        let malformed: [(ServiceJSON, [Int])] = [
            (response([index, index]), [1]), // duplicate
            (response([index]), []), // missing/out-of-range
            (response([]), [1]), // unused
            (response([.object(["attachment": .string("0")])]), [1]),
            (response([.object(["attachment": .integer(0), "extra": .null])]), [1]),
            (.object(["result": .object(["attachment": .integer(0)])]), [1]), // forbidden path
        ]
        for (message, lengths) in malformed {
            let header = try ServiceJSON.object(["version": .integer(1),
                "lengths": .array(lengths.map { .integer(Int64($0)) }), "message": message]).encoded()
            do {
                _ = try CloudServiceBinary.replyHeader(header)
                Issue.record("accepted malformed attachment reference")
            } catch CloudServiceProcessFailure.malformedReply {} catch { throw error }
        }
        let validHeader = try ServiceJSON.object(["version": .integer(1), "lengths": .array([.integer(4)]), "message": response([index])]).encoded()
        let header = try CloudServiceBinary.replyHeader(validHeader)
        let logical = try header.logical([Data([0, 10, 128, 255])])
        guard case .byteBuffer(let bytes)? = logical["result"]?["Ok"]?["Record"]?["fields"]?["a0"]?["Asset"]?["bytes"] else {
            Issue.record("binary reply expanded its asset into integer nodes"); return
        }
        #expect(bytes == Data([0, 10, 128, 255]))
        do {
            _ = try header.logical([Data([0])])
            Issue.record("accepted mismatched body length")
        } catch CloudServiceProcessFailure.malformedReply {} catch { throw error }
        for message in [ServiceJSON.object(["attachment": .integer(0)]),
                        assetMessage(Data(), fields: ["pa": .object(["Asset": .object(["bytes": index])])])] {
            do {
                _ = try CloudServiceBinary.prepare(message: message, transport: .binaryAssetsV1)
                Issue.record("accepted caller-supplied attachment reference")
            } catch CloudServiceProcessFailure.input {} catch CloudServiceProcessFailure.malformedReply {} catch { throw error }
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
private nonisolated struct ServiceSavedChange: Decodable, Sendable { let Saved: ServiceRecord }
private nonisolated struct ServiceZonePage: Decodable, Sendable {
    let changes: [ServiceSavedChange]
    let token: ServiceJSON
    let more: Bool
}
private nonisolated struct ServiceZoneReply: Decodable, Sendable { let ZoneChanges: ServiceZonePage }
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
private nonisolated func zonePage(_ client: CloudServiceProcess, token: ServiceJSON? = nil) async throws -> ServiceZonePage {
    let tokenValue: Any = try token.map { try JSONSerialization.jsonObject(with: $0.encoded()) } ?? NSNull()
    let response = try decode(ServiceResponse<ServiceZoneReply>.self,
                              await client.request(frame(["FetchZone": ["zone": "z", "token": tokenValue]])))
    return try required(response.result.Ok?.ZoneChanges)
}
private nonisolated func assetMessage(_ payload: Data, fields: [String: ServiceJSON]? = nil,
    ifUnchanged: Data? = nil) -> ServiceJSON {
    .object([
        "scope": .object(["container": .string("fixture-container"), "environment": .string("development"), "account": .string("fixture-account")]),
        "caller": .object(["device": .string("fixture-device"), "instance": .string("fixture-instance"), "own_operation": .string("fixture-operation")]),
        "operation": .object(["Modify": .object(["zone": .string("z"), "atomic": .bool(true), "changes": .array([
            .object(["Save": .object([
                "record": .object(["name": .string("large"), "record_type": .string("MGEntity"), "fields": .object(fields ?? [
                    "e": .object(["EncryptedBytes": .bytes(Data([0, 255, 10]))]),
                    "pa": .object(["Asset": .object(["bytes": .bytes(payload), "sha256": .bytes(Data(SHA256.hash(data: payload)))])]),
                ])]), "if_unchanged": ifUnchanged.map(ServiceJSON.bytes) ?? .null,
            ])]),
        ])])]),
    ])
}
private nonisolated func prepare(_ client: CloudServiceProcess, message: ServiceJSON, typed: Bool,
    afterWrite: (@Sendable () -> Void)? = nil) throws -> CloudServicePreparedFrame {
    if typed { return try client.prepare(message: message, afterWrite: afterWrite) }
    return try client.prepare(message.encoded(), afterWrite: afterWrite)
}
private nonisolated func reply(_ frame: CloudServicePreparedFrame, typed: Bool,
    preservingCancelledResult: Bool = false) async throws -> ServiceJSON {
    if typed { return try await frame.logicalReply(preservingCancelledResult: preservingCancelledResult) }
    return try JSONDecoder().decode(ServiceJSON.self, from: await frame.reply(preservingCancelledResult: preservingCancelledResult))
}
private nonisolated func request(_ client: CloudServiceProcess, message: ServiceJSON, typed: Bool) async throws -> ServiceJSON {
    if typed { return try await client.request(message: message) }
    return try JSONDecoder().decode(ServiceJSON.self, from: await client.request(message.encoded()))
}
private nonisolated func normalized(_ value: ServiceJSON) throws -> Data {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    return try encoder.encode(value)
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
