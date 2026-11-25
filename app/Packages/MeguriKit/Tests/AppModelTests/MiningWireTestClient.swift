#if os(macOS)
@testable import AppModel
import Foundation
import Network
import Testing

/// Small raw HTTP/RFC6455 client shared with the HTTP wire tests. Every network
/// operation has a deadline that cancels this connection, including reads of EOF.
nonisolated final class MiningWireTestClient: @unchecked Sendable {
    private let connection: NWConnection
    private let queue = DispatchQueue(label: "mining.wire.test")
    private var inputEnded = false
    private var buffered = Data() // Used only by the test's sequential caller.

    init(port: UInt16) {
        connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
    }

    func close() { connection.cancel() }

    private func bounded<T: Sendable>(_ operation: () async throws -> T) async throws -> T {
        let watchdog = DispatchWorkItem { [connection] in connection.cancel() }
        queue.asyncAfter(deadline: .now() + .seconds(5), execute: watchdog)
        defer { watchdog.cancel() }
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await operation()
        } onCancel: { [connection] in connection.cancel() }
    }

    func start() async throws {
        try await bounded {
            let ready = AsyncStream<Bool>.makeStream()
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: ready.continuation.yield(true); ready.continuation.finish()
                case .failed, .cancelled: ready.continuation.yield(false); ready.continuation.finish()
                default: break
                }
            }
            connection.start(queue: queue)
            for await started in ready.stream {
                guard started else { throw MiningFailure("Synthetic wire connection did not start.") }
                return
            }
            throw MiningFailure("Synthetic wire connection ended before ready.")
        }
    }

    func send(_ bytes: Data) async throws {
        try await bounded {
            try await withCheckedThrowingContinuation { (reply: CheckedContinuation<Void, any Error>) in
                connection.send(content: bytes, completion: .contentProcessed { error in
                    if let error { reply.resume(throwing: error) } else { reply.resume() }
                })
            }
        }
    }

    func receive() async throws -> (Data, Bool) {
        if inputEnded { return (Data(), true) }
        let part: (Data, Bool) = try await bounded {
            try await withCheckedThrowingContinuation { reply in
                connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, complete, error in
                    if let error { reply.resume(throwing: error) }
                    else { reply.resume(returning: (data ?? Data(), complete)) }
                }
            }
        }
        inputEnded = part.1
        return part
    }

    func upgrade(port: UInt16) async throws {
        let request = "GET /anki-connect HTTP/1.1\r\nHost: 127.0.0.1:\(port)\r\nOrigin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        try await send(Data(request.utf8))
        while buffered.range(of: Data("\r\n\r\n".utf8)) == nil {
            try await readMore()
            guard buffered.count <= 16_384 else { throw MiningFailure("Oversized synthetic upgrade reply.") }
        }
        let boundary = try #require(buffered.range(of: Data("\r\n\r\n".utf8)))
        let head = String(decoding: buffered[..<boundary.lowerBound], as: UTF8.self)
        try #require(head.hasPrefix("HTTP/1.1 101 "))
        #expect(head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="))
        buffered = Data(buffered[boundary.upperBound...])
    }

    func frame(_ opcode: UInt8, payload: Data) async throws {
        precondition(payload.count <= 125)
        let mask: [UInt8] = [1, 2, 3, 4]
        try await send(Data([0x80 | opcode, 0x80 | UInt8(payload.count)] + mask)
            + Data(payload.enumerated().map { $0.element ^ mask[$0.offset % 4] }))
    }

    func expectFrame(_ opcode: UInt8, payload: Data) async throws {
        while buffered.count < 2 { try await readMore() }
        try #require(buffered[0] == (0x80 | opcode) && buffered[1] <= 125)
        let count = Int(buffered[1])
        while buffered.count < 2 + count { try await readMore() }
        #expect(Data(buffered[2..<(2 + count)]) == payload)
        buffered = Data(buffered.dropFirst(2 + count))
    }

    func expectEnd() async throws {
        try #require(buffered.isEmpty)
        let (data, complete) = try await receive()
        #expect(data.isEmpty && complete)
    }

    private func readMore() async throws {
        let (data, _) = try await receive()
        guard !data.isEmpty else { throw MiningFailure("Synthetic wire connection closed before its reply.") }
        guard buffered.count + data.count <= 65_536 else { throw MiningFailure("Oversized synthetic wire reply.") }
        buffered.append(data)
    }
}
#endif
