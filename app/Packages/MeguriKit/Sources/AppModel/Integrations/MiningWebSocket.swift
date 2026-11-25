#if os(macOS)
import Foundation

/// RFC 6455 framing only. Data messages have no mining commands, so consume
/// them incrementally. Only control payloads and an incomplete header are kept.
nonisolated struct MiningWebSocket {
    nonisolated enum Event {
        case send(Data)
        case close(Data)
    }

    private nonisolated struct Frame {
        var remaining: Int
        var offset = 0
        let mask: [UInt8]
        let opcode: UInt8
        let final: Bool
    }

    private var header: [UInt8] = []
    private var incomingFrame: Frame?
    private var control = Data()
    private var messageOpcode: UInt8?
    private var messageLength = 0
    private var utf8Remaining = 0
    private var utf8Lower: UInt8 = 0x80
    private var utf8Upper: UInt8 = 0xbf

    var retainedBytes: Int {
        header.count + control.count + (incomingFrame?.mask.count ?? 0)
    }

    static func frame(_ opcode: UInt8, _ payload: Data) -> Data {
        precondition(payload.count <= 125)
        return Data([0x80 | opcode, UInt8(payload.count)]) + payload
    }

    static func close(_ code: UInt16) -> Data {
        frame(8, Data([UInt8(code >> 8), UInt8(code & 255)]))
    }

    private mutating func acceptTextByte(_ byte: UInt8) -> Bool {
        if utf8Remaining > 0 {
            guard byte >= utf8Lower, byte <= utf8Upper else { return false }
            utf8Remaining -= 1
            utf8Lower = 0x80
            utf8Upper = 0xbf
            return true
        }
        switch byte {
        case 0...0x7f: return true
        case 0xc2...0xdf: utf8Remaining = 1
        case 0xe0:
            utf8Remaining = 2
            utf8Lower = 0xa0
        case 0xe1...0xec, 0xee...0xef: utf8Remaining = 2
        case 0xed:
            utf8Remaining = 2
            utf8Upper = 0x9f
        case 0xf0:
            utf8Remaining = 3
            utf8Lower = 0x90
        case 0xf1...0xf3: utf8Remaining = 3
        case 0xf4:
            utf8Remaining = 3
            utf8Upper = 0x8f
        default: return false
        }
        return true
    }

    mutating func receive(_ bytes: Data) -> [Event] {
        var events: [Event] = []
        func failure(_ code: UInt16) -> [Event] { events + [.close(Self.close(code))] }
        var cursor = bytes.startIndex
        while cursor < bytes.endIndex || incomingFrame?.remaining == 0 {
            if incomingFrame == nil {
                // Read just the header, including its client mask (at most 14 bytes).
                while cursor < bytes.endIndex {
                    header.append(bytes[cursor])
                    cursor += 1
                    guard header.count >= 2 else { continue }
                    let lengthBytes = header[1] & 127
                    let needed = 6 + (lengthBytes == 126 ? 2 : lengthBytes == 127 ? 8 : 0)
                    if header.count == needed { break }
                }
                guard header.count >= 2 else { return events }
                let final = header[0] & 0x80 != 0
                let opcode = header[0] & 15
                guard header[0] & 0x70 == 0, header[1] & 0x80 != 0,
                      [0, 1, 2, 8, 9, 10].contains(opcode) else { return failure(1002) }
                var length = UInt64(header[1] & 127)
                let extended = length == 126 ? 2 : length == 127 ? 8 : 0
                guard header.count == 6 + extended else { return events }
                if extended > 0 {
                    length = header[2..<(2 + extended)].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
                    guard length >= (extended == 2 ? 126 : 65_536), length >> 63 == 0 else { return failure(1002) }
                }
                if opcode >= 8 {
                    guard final, length <= 125 else { return failure(1002) }
                } else {
                    if opcode == 0 {
                        guard messageOpcode != nil else { return failure(1002) }
                    } else {
                        guard messageOpcode == nil else { return failure(1002) }
                        messageOpcode = opcode
                    }
                    guard length <= MiningHTTP.bodyLimit - messageLength else { return failure(1009) }
                    messageLength += Int(length)
                }
                incomingFrame = Frame(remaining: Int(length), mask: Array(header.suffix(4)), opcode: opcode, final: final)
                header.removeAll(keepingCapacity: false)
            }
            guard var current = incomingFrame else { return events }
            let count = min(current.remaining, bytes.endIndex - cursor)
            if current.opcode >= 8 || messageOpcode == 1 {
                for index in 0..<count {
                    let byte = bytes[cursor + index] ^ current.mask[(current.offset + index) % 4]
                    if current.opcode >= 8 { control.append(byte) }
                    else if !acceptTextByte(byte) { return failure(1007) }
                }
            }
            cursor += count
            current.remaining -= count
            current.offset += count
            incomingFrame = current
            guard current.remaining == 0 else { return events }
            incomingFrame = nil
            switch current.opcode {
            case 8:
                guard control.count != 1 else { return failure(1002) }
                if control.isEmpty { return events + [.close(Self.frame(8, Data()))] }
                let code = UInt16(control[0]) << 8 | UInt16(control[1])
                guard (1000...1003).contains(code) || (1007...1014).contains(code) || (3000...4999).contains(code),
                      String(data: control.dropFirst(2), encoding: .utf8) != nil else { return failure(1002) }
                return events + [.close(Self.close(code))]
            case 9: events.append(.send(Self.frame(10, control)))
            case 10: break
            default:
                if current.final {
                    guard messageOpcode != 1 || utf8Remaining == 0 else { return failure(1007) }
                    messageOpcode = nil
                    messageLength = 0
                }
            }
            control.removeAll(keepingCapacity: false)
        }
        return events
    }
}
#endif
