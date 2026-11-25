#if os(macOS)
@testable import AppModel
import Foundation
import Synchronization
import Testing

@MainActor @Suite struct SentenceMiningHTTPTests {
    private func request(_ text: String) throws -> MiningHTTPRequest {
        try #require(MiningHTTP.head(Data(text.utf8))?.0)
    }

    @Test func preflightHasNoContentLengthAndLocalAbsoluteIPv6IsAccepted() throws {
        let head = try request("OPTIONS http://[::1]:44432/receive HTTP/1.1\r\n\r\n")
        #expect(head.absoluteAuthority == "[::1]:44432")
        #expect(head.path == "/receive")
        let response = try #require(MiningHTTP.preliminary(head, port: 44432, sockets: 0))
        #expect(response.status == 204)
        let wire = String(decoding: response.bytes, as: UTF8.self)
        #expect(!wire.contains("Content-Length:"))
        #expect(wire.hasSuffix("\r\n\r\n"))
        // H6.1 deliberately accepts hand-built requests without Host.
        let noHost = try request("GET /anki-hello HTTP/1.1\r\n\r\n")
        #expect(MiningHTTP.admission(noHost, port: 44432) == nil)
    }

    @Test func connectionTokensRespectVersionAndClose() throws {
        for (version, connection, expected) in [
            ("1.1", "", true), ("1.1", "Connection: close\r\n", false),
            ("1.0", "", false), ("1.0", "Connection: Keep-Alive\r\n", true),
            ("1.1", "Connection: keep-alive, Close\r\n", false)
        ] {
            let head = try request("GET /anki-hello HTTP/\(version)\r\n\(connection)\r\n")
            #expect(head.keepsAlive == expected)
        }
    }

    @Test func incrementalChunksCountContentAndLeaveTheNextRequestBuffered() throws {
        let head = try request("POST /receive HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
        var decoder = try MiningHTTPBody(request: head, limit: 5)
        // Framing is larger than the decoded-content limit, which is valid.
        for byte in Data("2;label=first\r\nhe\r\n3\r\nllo\r\n0\r\nX-Receipt: yes\r\n\r".utf8) {
            #expect(try decoder.consume(Data([byte])) == 1)
            #expect(!decoder.complete)
        }
        let next = Data("\nGET /anki-hello HTTP/1.1\r\n\r\n".utf8)
        #expect(try decoder.consume(next) == 1)
        #expect(decoder.complete)
        #expect(decoder.body == Data("hello".utf8))
        #expect(decoder.retainedBytes == 5)
    }

    @Test func framingLineAtLimitAllowsSplitCRLF() throws {
        let head = try request("POST /receive HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
        var decoder = try MiningHTTPBody(request: head, limit: 1)
        let size = "1;label=" + String(repeating: "a", count: 8192 - 8)
        #expect(size.utf8.count == 8192)
        #expect(try decoder.consume(Data((size + "\r").utf8)) == 8193)
        #expect(decoder.retainedBytes == 8193)
        _ = try decoder.consume(Data("\nx\r\n0\r\n".utf8))
        let trailer = "X-Receipt: " + String(repeating: "b", count: 8192 - 11)
        #expect(trailer.utf8.count == 8192)
        _ = try decoder.consume(Data((trailer + "\r").utf8))
        _ = try decoder.consume(Data("\n\r".utf8))
        #expect(!decoder.complete)
        _ = try decoder.consume(Data([10]))
        #expect(decoder.complete && decoder.body == Data("x".utf8))
        #expect(decoder.retainedBytes == 1)
    }

    @Test func ordinaryCloseReasonIsNotEchoed() {
        var socket = MiningWebSocket()
        let payload: [UInt8] = [3, 232] + Array("Done".utf8)
        let mask: [UInt8] = [1, 2, 3, 4]
        let frame = Data([0x88, 0x80 | UInt8(payload.count)] + mask)
            + Data(payload.enumerated().map { $0.element ^ mask[$0.offset % 4] })
        if case .some(.close(let reply)) = socket.receive(frame).first {
            #expect(reply == MiningWebSocket.close(1000))
        } else { Issue.record("Expected an ordinary close reply") }
    }

    @Test(.timeLimit(.minutes(1))) func bufferedRequestsReplySequentiallyAndCloseIsTerminal() async throws {
        let states = Mutex<[MiningListener.State]>([])
        let requests = Mutex<[String]>([])
        let listener = MiningListener(port: 0, launch: { request, _ in
            requests.withLock { $0.append(request.target) }
            return Task { .json(200, ["target": request.target, "body": String(decoding: request.body, as: UTF8.self)]) }
        }, changed: { state in states.withLock { $0.append(state) } })
        listener.start()
        do {
            let port = try await listeningPort(states)
            let first = "POST /receive?first HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n0\r\n\r\n"
            let second = "GET /anki-hello?second HTTP/1.1\r\nConnection: close\r\n\r\n"
            let wire = try await MiningHTTPTestClient.exchange(port: port, bytes: Data((first + second).utf8))
            let replies = String(decoding: wire, as: UTF8.self).components(separatedBy: "HTTP/1.1 200 OK\r\n")
            #expect(replies.count == 3)
            if replies.count == 3 {
                #expect(!replies[1].contains("Connection: close"))
                #expect(replies[1].contains("first") && replies[1].contains("hi"))
                #expect(replies[2].contains("second") && replies[2].contains("Connection: close"))
            }
            #expect(requests.withLock { $0 } == ["/receive?first", "/anki-hello?second"])
            requests.withLock { $0.removeAll() }
            // An ordinary close request finishes this conversation, even when
            // another complete request was already buffered by the transport.
            let terminal = "GET /anki-hello?last HTTP/1.1\r\nConnection: close\r\n\r\n"
            _ = try await MiningHTTPTestClient.exchange(port: port, bytes: Data((terminal + second).utf8))
            #expect(requests.withLock { $0 } == ["/anki-hello?last"])
            await listener.stop()
        } catch {
            await listener.stop()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func bodylessPreliminaryRepliesKeepAliveAndRetainPipelinedBytes() async throws {
        let states = Mutex<[MiningListener.State]>([])
        let calls = Mutex(0)
        let listener = MiningListener(port: 0, handler: { _, _ in
            calls.withLock { $0 += 1 }
            return .json(200, ["next": true])
        }, changed: { state in states.withLock { $0.append(state) } })
        listener.start()
        do {
            let port = try await listeningPort(states)
            let next = "GET /anki-hello HTTP/1.1\r\nConnection: close\r\n\r\n"
            for (line, status) in [("GET /missing", 404), ("OPTIONS /receive", 204), ("PUT /receive", 405)] {
                let first = line + " HTTP/1.1\r\nContent-Length: 0\r\n\r\n"
                let wire = try await MiningHTTPTestClient.exchange(port: port, bytes: Data((first + next).utf8))
                let replies = String(decoding: wire, as: UTF8.self).components(separatedBy: "HTTP/1.1 ")
                try #require(replies.count == 3)
                #expect(replies[1].hasPrefix(String(status)))
                #expect(!replies[1].contains("Connection: close"))
                #expect(replies[2].hasPrefix("200 OK") && replies[2].contains("Connection: close"))
            }
            let ordinary = try await MiningHTTPTestClient.exchange(port: port,
                bytes: Data("GET /missing HTTP/1.1\r\n\r\n".utf8), followup: Data(next.utf8))
            let ordinaryReplies = String(decoding: ordinary, as: UTF8.self).components(separatedBy: "HTTP/1.1 ")
            try #require(ordinaryReplies.count == 3)
            #expect(ordinaryReplies[1].hasPrefix("404") && !ordinaryReplies[1].contains("Connection: close"))
            #expect(ordinaryReplies[2].hasPrefix("200 OK"))
            #expect(calls.withLock { $0 } == 4)
            for framing in ["Content-Length: 1", "Transfer-Encoding: chunked"] {
                let first = "GET /missing HTTP/1.1\r\n" + framing + "\r\n\r\n"
                let wire = try await MiningHTTPTestClient.exchange(port: port, bytes: Data((first + next).utf8))
                let reply = String(decoding: wire, as: UTF8.self)
                #expect(reply.hasPrefix("HTTP/1.1 404") && reply.contains("Connection: close"))
                #expect(!reply.contains("HTTP/1.1 200"))
            }
            #expect(calls.withLock { $0 } == 4)
            await listener.stop()
        } catch {
            await listener.stop()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func versionBypassesFullWriteQueueAfterAdmission() async throws {
        let receiver = MiningReceiver(pendingLimit: 0)
        await receiver.configure(enabled: true, destination: nil)
        func root(_ body: String) -> MiningHTTPRequest {
            .init(method: "POST", target: "/", headers: [], body: Data(body.utf8))
        }
        let queued = await receiver.handle(root(#"{"action":"deckNames","version":6}"#))
        #expect(queued.status == 503)
        let version = await receiver.handle(root(#"{"action":"version","version":6}"#))
        let envelope = try #require(try JSONSerialization.jsonObject(with: version.body) as? [String: Any])
        #expect(version.status == 200 && envelope["result"] as? Int == 6 && envelope["error"] is NSNull)
        let legacy = await receiver.handle(root(#"{"action":"version"}"#))
        #expect(legacy.status == 200 && String(decoding: legacy.body, as: UTF8.self) == "6")
        let invalid = await receiver.handle(root(#"{"action":"version","params":[]}"#))
        #expect(invalid.status == 503)
        let oldToken = receiver.admission.current()
        await receiver.configure(enabled: true, destination: nil)
        let stale = await receiver.handle(root(#"{"action":"version"}"#), admissionToken: oldToken)
        #expect(stale.status == 503)
        await receiver.configure(enabled: false, destination: nil)
        let off = await receiver.handle(root(#"{"action":"version"}"#))
        #expect(off.status == 503)
    }

    @Test func retiredEndpointCannotDispatchIntoTheSharedReceiver() async throws {
        let receiver = MiningReceiver()
        await receiver.configure(enabled: true, destination: nil)
        let first = MiningAdmission(), sibling = MiningAdmission()
        let firstToken = first.update(enabled: true)
        let siblingToken = sibling.update(enabled: true)
        _ = first.update(enabled: false)
        let request = MiningHTTPRequest(method: "POST", target: "/", headers: [],
            body: Data(#"{"action":"version","version":6}"#.utf8))
        let stale = await receiver.launch(request, isCurrent: { first.matches(firstToken) }).value
        let active = await receiver.launch(request, isCurrent: { sibling.matches(siblingToken) }).value
        #expect(stale.status == 503 && active.status == 200)
        await receiver.configure(enabled: false, destination: nil)
    }

    @Test(.timeLimit(.minutes(1))) func shutdownStaysSuspendedUntilRefusedQuitResumesIt() async throws {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic-shutdown", ports: (0, 0))
        model.setEnabled(true, for: .migaku)
        model.firstOpenAttemptEnded()
        await model.settle()
        await model.shutdown()
        model.collectionAvailable()
        model.selectDeck(1)
        model.setEnabled(false, for: .migaku)
        model.setEnabled(true, for: .migaku)
        await model.settle()
        #expect(model.migaku.status.hasPrefix("Off"))
        model.resumeAfterRefusedQuit()
        await model.settle()
        #expect(!model.migaku.status.hasPrefix("Off"))
        await model.shutdown()
    }

    private func listeningPort(_ states: borrowing Mutex<[MiningListener.State]>) async throws -> UInt16 {
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            if let port = states.withLock({ $0.compactMap { state -> UInt16? in
                if case .listening(let port) = state { return port }
                return nil
            }.last }) { return port }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw MiningFailure("Synthetic HTTP listener did not start.")
    }
}

/// Each exchange has one client and a terminal close request. The watchdog
/// cancels its own connection, so a failing test cannot leave a receive hung.
private nonisolated enum MiningHTTPTestClient {
    static func exchange(port: UInt16, bytes: Data, followup: Data? = nil) async throws -> Data {
        let client = MiningWireTestClient(port: port)
        defer { client.close() }
        try await client.start()
        try await client.send(bytes)
        var followup = followup
        var received = Data()
        while true {
            let part = try await client.receive()
            received.append(part.0)
            if let next = followup, let boundary = received.range(of: Data("\r\n\r\n".utf8)) {
                let head = String(decoding: received[..<boundary.lowerBound], as: UTF8.self)
                let length = head.components(separatedBy: "\r\n").compactMap { line -> Int? in
                    guard line.hasPrefix("Content-Length: ") else { return nil }
                    return Int(line.dropFirst("Content-Length: ".count))
                }.first ?? 0
                if received.count >= boundary.upperBound + length {
                    followup = nil
                    try await client.send(next)
                }
            }
            if part.1 { return received }
        }
    }

}
#endif
