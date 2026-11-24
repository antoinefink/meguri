#if os(macOS)
import Foundation

/// RFC 6455 framing only. The mining connection has no server greeting or
/// data commands (migaku-protocol P10.3); it stays open and answers pings.
nonisolated struct MiningWebSocket {
    private var buffer = Data()
    private var message = Data()
    private var messageOpcode: UInt8?
    nonisolated enum Event { case send(Data), close(Data) }
    static func frame(_ opcode: UInt8, _ payload: Data) -> Data {
        precondition(payload.count <= 125)
        return Data([0x80 | opcode, UInt8(payload.count)]) + payload
    }
    static func close(_ code: UInt16) -> Data { frame(8, Data([UInt8(code >> 8), UInt8(code & 255)])) }
    mutating func receive(_ bytes: Data) -> [Event] {
        buffer.append(bytes)
        var events: [Event] = []
        func failure(_ code: UInt16) -> [Event] { [.close(Self.close(code))] }
        while buffer.count >= 2 {
            let b = [UInt8](buffer.prefix(14)), fin = b[0] & 0x80 != 0, op = b[0] & 15
            guard b[0] & 0x70 == 0, b[1] & 0x80 != 0, [0, 1, 2, 8, 9, 10].contains(op) else { return failure(1002) }
            var length = UInt64(b[1] & 127), start = 2
            if length == 126 {
                guard buffer.count >= 4 else { return events }
                length = UInt64(b[2]) << 8 | UInt64(b[3]); start = 4
                if length < 126 { return failure(1002) }
            } else if length == 127 {
                guard buffer.count >= 10 else { return events }
                length = b[2..<10].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }; start = 10
                if length < 65536 || length >> 63 != 0 { return failure(1002) }
            }
            if op >= 8 && (!fin || length > 125) { return failure(1002) }
            guard length <= MiningHTTP.bodyLimit else { return failure(1009) }
            if op < 8, length > MiningHTTP.bodyLimit - message.count { return failure(1009) }
            guard buffer.count >= start + 4 + Int(length) else { return events }
            let mask = [UInt8](buffer[start..<start+4]); start += 4
            let payload = Data(buffer[start..<start+Int(length)].enumerated().map { $0.element ^ mask[$0.offset % 4] })
            buffer = Data(buffer.dropFirst(start + Int(length)))
            if op == 8 {
                if payload.count == 1 { return failure(1002) }
                if payload.isEmpty { return events + [.close(Self.frame(8, Data()))] }
                let code = UInt16(payload[0]) << 8 | UInt16(payload[1])
                guard (1000...1003).contains(code) || (1007...1014).contains(code) || (3000...4999).contains(code),
                      String(data: payload.dropFirst(2), encoding: .utf8) != nil else { return failure(1002) }
                return events + [.close(Self.close(code))]
            }
            if op == 9 { events.append(.send(Self.frame(10, payload))); continue }
            if op == 10 { continue }
            if op == 0 {
                guard messageOpcode != nil else { return failure(1002) }
            } else {
                guard messageOpcode == nil else { return failure(1002) }
                messageOpcode = op
            }
            message.append(payload)
            if fin {
                if messageOpcode == 1 && String(data: message, encoding: .utf8) == nil { return failure(1007) }
                message.removeAll(keepingCapacity: false); messageOpcode = nil
            }
        }
        return events
    }
}
#endif
