#if os(macOS)
@testable import AppModel
import Foundation
import Network
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

    @Test(.timeLimit(.minutes(1))) func shutdownStaysSuspendedUntilRefusedQuitResumesIt() async throws {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic-shutdown", port: 0)
        model.setEnabled(true)
        model.firstOpenAttemptEnded()
        await model.settle()
        await model.shutdown()
        model.collectionAvailable()
        model.selectDeck(1)
        model.setEnabled(false)
        model.setEnabled(true)
        await model.settle()
        #expect(model.status.hasPrefix("Off"))
        model.resumeAfterRefusedQuit()
        await model.settle()
        #expect(!model.status.hasPrefix("Off"))
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
    static func exchange(port: UInt16, bytes: Data) async throws -> Data {
        let connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
        let queue = DispatchQueue(label: "mining.http.test")
        let ready = AsyncStream<Bool>.makeStream()
        connection.stateUpdateHandler = { state in
            switch state {
            case .ready: ready.continuation.yield(true); ready.continuation.finish()
            case .failed, .cancelled: ready.continuation.yield(false); ready.continuation.finish()
            default: break
            }
        }
        let watchdog = DispatchWorkItem { connection.cancel() }
        queue.asyncAfter(deadline: .now() + .seconds(5), execute: watchdog)
        defer { watchdog.cancel(); connection.cancel() }
        connection.start(queue: queue)
        for await started in ready.stream {
            guard started else { throw MiningFailure("Synthetic HTTP connection did not start.") }
            break
        }
        try await withCheckedThrowingContinuation { (reply: CheckedContinuation<Void, any Error>) in
            connection.send(content: bytes, completion: .contentProcessed { error in
                if let error { reply.resume(throwing: error) } else { reply.resume() }
            })
        }
        var received = Data()
        while true {
            let part: (Data, Bool) = try await withCheckedThrowingContinuation { reply in
                connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, complete, error in
                    if let error { reply.resume(throwing: error) }
                    else { reply.resume(returning: (data ?? Data(), complete)) }
                }
            }
            received.append(part.0)
            if part.1 { return received }
        }
    }
}
#endif
