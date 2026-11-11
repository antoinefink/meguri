import Foundation
import CryptoKit

// Validate the original bytes before decoding. In particular, Foundation's
// object decoder alone would discard duplicate keys. Whitespace and key order
// are deliberately unrestricted; hashes always use the delivered bytes.
nonisolated enum AgentJSON {
    indirect enum Value {
        case object([String: Value]), array([Value]), scalar
        case string(String), number(String), boolean(Bool), null
        nonisolated func object(required: Set<String>, optional: Set<String> = []) throws -> [String: Value] {
            guard case .object(let fields) = self,
                  required.isSubset(of: Set(fields.keys)),
                  Set(fields.keys).isSubset(of: required.union(optional)) else { throw AgentFailure.shape }
            return fields
        }
    }
    static func hash(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
    static func digest(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    static func label(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 80 && value.utf8.allSatisfy {
            (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || [45, 95].contains($0)
        }
    }
    static func parse(_ data: Data) throws -> Value {
        try parse(data, limit: 1_048_576, evidenceScalars: false)
    }
    // Evidence alone has a larger bound and retains original numeric tokens.
    static func parseEvidence(_ data: Data) throws -> Value {
        try parse(data, limit: 16 * 1024 * 1024, evidenceScalars: true)
    }
    private static func parse(_ data: Data, limit: Int, evidenceScalars: Bool) throws -> Value {
        guard data.count <= limit, String(data: data, encoding: .utf8) != nil else { throw AgentFailure.json }
        var parser = Parser(bytes: Array(data), evidenceScalars: evidenceScalars)
        let value = try parser.value(depth: 0)
        parser.space()
        guard parser.index == parser.bytes.count else { throw AgentFailure.json }
        return value
    }
    static func decode<T: Decodable>(_ type: T.Type, _ bytes: Data) throws -> T {
        do { return try JSONDecoder().decode(type, from: bytes) }
        catch { throw AgentFailure.shape }
    }
    static func lines(_ data: Data) -> (complete: [Data], suffix: Data) {
        var lines: [Data] = [], start = data.startIndex
        for index in data.indices where data[index] == 10 {
            lines.append(Data(data[start..<index])); start = data.index(after: index)
        }
        return (lines, Data(data[start...]))
    }
    private struct Parser {
        let bytes: [UInt8]
        let evidenceScalars: Bool
        var index = 0
        mutating func space() { while index < bytes.count && [9, 10, 13, 32].contains(bytes[index]) { index += 1 } }
        mutating func take(_ byte: UInt8) -> Bool {
            if index < bytes.count && bytes[index] == byte { index += 1; return true }; return false
        }
        mutating func string() throws -> String {
            let start = index
            guard take(34) else { throw AgentFailure.json }
            var escaped = false
            while index < bytes.count {
                let byte = bytes[index]; index += 1
                if byte == 34 && !escaped {
                    do { return try JSONDecoder().decode(String.self, from: Data(bytes[start..<index])) }
                    catch { throw AgentFailure.json }
                }
                if byte < 32 { throw AgentFailure.json }
                if escaped { escaped = false } else if byte == 92 { escaped = true }
            }
            throw AgentFailure.json
        }
        mutating func value(depth: Int) throws -> Value {
            guard depth < 32 else { throw AgentFailure.json }
            space()
            guard index < bytes.count else { throw AgentFailure.json }
            if bytes[index] == 34 { let text = try string(); return evidenceScalars ? .string(text) : .scalar }
            if take(123) {
                var fields: [String: Value] = [:]
                space(); if take(125) { return .object(fields) }
                repeat {
                    space(); let key = try string()
                    guard fields[key] == nil else { throw AgentFailure.json }
                    space(); guard take(58) else { throw AgentFailure.json }
                    fields[key] = try value(depth: depth + 1)
                    space(); if take(125) { return .object(fields) }
                    guard take(44) else { throw AgentFailure.json }
                } while true
            }
            if take(91) {
                var values: [Value] = []
                space(); if take(93) { return .array(values) }
                repeat {
                    values.append(try value(depth: depth + 1))
                    space(); if take(93) { return .array(values) }
                    guard take(44) else { throw AgentFailure.json }
                } while true
            }
            for literal in ["true", "false", "null"] {
                let token = Array(literal.utf8)
                if bytes[index...].starts(with: token) {
                    index += token.count
                    if !evidenceScalars { return .scalar }
                    return literal == "null" ? .null : .boolean(literal == "true")
                }
            }
            let start = index
            _ = take(45)
            if !take(48) {
                guard index < bytes.count, (49...57).contains(bytes[index]) else { throw AgentFailure.json }
                digits()
            }
            if take(46) {
                let before = index; digits(); guard index > before else { throw AgentFailure.json }
            }
            if take(101) || take(69) {
                if !take(43) { _ = take(45) }
                let before = index; digits(); guard index > before else { throw AgentFailure.json }
            }
            guard let number = Double(String(decoding: bytes[start..<index], as: UTF8.self)), number.isFinite else { throw AgentFailure.json }
            return evidenceScalars ? .number(String(decoding: bytes[start..<index], as: UTF8.self)) : .scalar
        }
        mutating func digits() { while index < bytes.count && (48...57).contains(bytes[index]) { index += 1 } }
    }
}

// No native error descriptions, userInfo, paths or identities cross this seam.
nonisolated enum AgentFailure: String, Error, Codable {
    case json, shape, version, binding, prefix, sequence, journal, busy
    case wrongContainer = "wrong-container"
    case unsupported, execution, recovery, storage
    case implementationMissing = "implementation_missing"
    case engineOrderNotShown = "engine_order_not_shown"
    case awaitingUser = "awaiting_user"

    enum Disposition: String, Codable { case failed, awaitingUser = "awaiting_user" }
    var disposition: Disposition { self == .awaitingUser ? .awaitingUser : .failed }
}
