#if os(macOS)
import Foundation

nonisolated enum CloudServiceTransport: Sendable { case legacy, binaryAssetsV1 }

// These are actual pipe components (header excludes LF). Logical JSON returned
// to existing receipt readers is reconstructed and must never be called pipe bytes.
nonisolated struct CloudServicePacket: Sendable {
    let header: Data
    let bodies: [Data]
}
nonisolated struct CloudServiceExchange: Sendable {
    let request: CloudServicePacket
    let response: CloudServicePacket
}
nonisolated enum CloudServiceBinary {
    static let headerLimit = 1_048_576
    // Swift and CLI share these limits for fixture attachments only.
    static let bodyLimit = 4 * 1_048_576
    static let countLimit = 16
    static let aggregateLimit = 8 * 1_048_576

    static func bounds(_ lengths: [Int]) throws {
        guard lengths.count <= countLimit else { throw CloudServiceProcessFailure.fixtureAttachmentLimit }
        var total = 0
        for length in lengths {
            guard length >= 0, length <= bodyLimit, length <= aggregateLimit - total else {
                throw CloudServiceProcessFailure.fixtureAttachmentLimit
            }
            total += length
        }
    }
    private static func assetLeaf(_ path: [String]) -> Bool {
        let records = [
            ["operation", "Modify", "changes", "*", "Save", "record"],
            ["result", "Ok", "Record"],
            ["result", "Ok", "Modified", "*", "result", "Ok", "Saved"],
            ["result", "Ok", "Modified", "*", "result", "Err", "server_record"],
            ["result", "Ok", "ZoneChanges", "changes", "*", "Saved"],
            ["result", "Err", "server_record"],
        ]
        return records.contains { record in
            let pattern = record + ["fields", "*", "Asset", "bytes"]
            return path.count == pattern.count && zip(pattern, path).allSatisfy { $0 == "*" || $0 == $1 }
        }
    }
    private static func walk(_ value: ServiceJSON, path: [String] = [],
                             leaf: (ServiceJSON) throws -> ServiceJSON) throws -> ServiceJSON {
        if assetLeaf(path) { return try leaf(value) }
        switch value {
        case .object(let object):
            guard object["attachment"] == nil || assetLeaf(path + ["*", "Asset", "bytes"]) else {
                throw CloudServiceProcessFailure.malformedReply
            }
            var changed: [String: ServiceJSON] = [:]
            for key in object.keys.sorted() {
                changed[key] = try walk(object[key]!, path: path + [key], leaf: leaf)
            }
            return .object(changed)
        case .array(let array):
            return .array(try array.enumerated().map { try walk($0.element, path: path + [String($0.offset)], leaf: leaf) })
        default: return value
        }
    }
    static func prepare(_ frame: Data, transport: CloudServiceTransport) throws -> CloudServicePacket {
        guard !frame.contains(10) else { throw CloudServiceProcessFailure.input }
        if case .legacy = transport {
            guard frame.count <= headerLimit else { throw CloudServiceProcessFailure.fixtureInputLimit }
            return .init(header: frame, bodies: [])
        }
        let message: ServiceJSON
        do { message = try JSONDecoder().decode(ServiceJSON.self, from: frame) }
        catch { throw CloudServiceProcessFailure.input }
        return try prepare(message: message, transport: transport)
    }
    static func prepare(message: ServiceJSON, transport: CloudServiceTransport) throws -> CloudServicePacket {
        if case .legacy = transport { return try prepare(message.encoded(), transport: transport) }
        var lengths: [Int] = []
        _ = try walk(message) { value in
            switch value {
            case .byteBuffer(let data): lengths.append(data.count)
            case .array(let array):
                lengths.append(array.count)
            default: throw CloudServiceProcessFailure.input
            }
            try bounds(lengths)
            // Validate every byte before acceptance or body allocation.
            if case .array(let array) = value,
               !array.allSatisfy({ $0.number.flatMap { UInt8(exactly: $0) } != nil }) {
                throw CloudServiceProcessFailure.input
            }
            return value
        }
        var bodies: [Data] = []
        let indexed = try walk(message) { value in
            let index = bodies.count
            bodies.append(try value.bytes())
            return .object(["attachment": .integer(Int64(index))])
        }
        let header = try ServiceJSON.object(["version": .integer(1),
            "lengths": .array(lengths.map { .integer(Int64($0)) }), "message": indexed]).encoded()
        guard header.count <= headerLimit else { throw CloudServiceProcessFailure.fixtureInputLimit }
        return .init(header: header, bodies: bodies)
    }
    struct ReplyHeader {
        let message: ServiceJSON
        let lengths: [Int]
        func logical(_ bodies: [Data]?) throws -> ServiceJSON {
            if let bodies {
                guard bodies.count == lengths.count,
                      zip(bodies, lengths).allSatisfy({ $0.count == $1 }) else {
                    throw CloudServiceProcessFailure.malformedReply
                }
            }
            var used = Set<Int>()
            let result = try walk(message) { value in
                guard case .object(let object) = value, object.count == 1,
                      let n = object["attachment"]?.number, let index = Int(exactly: n),
                      lengths.indices.contains(index), used.insert(index).inserted else {
                    throw CloudServiceProcessFailure.malformedReply
                }
                if let bodies { return .bytes(bodies[index]) }
                return .array([])
            }
            guard used.count == lengths.count else { throw CloudServiceProcessFailure.malformedReply }
            return result
        }
    }
    static func replyHeader(_ header: Data) throws -> ReplyHeader {
        let root: ServiceJSON
        do { root = try JSONDecoder().decode(ServiceJSON.self, from: header) }
        catch { throw CloudServiceProcessFailure.malformedReply }
        guard case .object(let object) = root, Set(object.keys) == Set(["version", "lengths", "message"]),
              root["version"]?.number == 1, let values = root["lengths"]?.items,
              let message = root["message"] else { throw CloudServiceProcessFailure.malformedReply }
        let lengths = try values.map { value -> Int in
            guard let n = value.number, let length = Int(exactly: n), length >= 0 else {
                throw CloudServiceProcessFailure.malformedReply
            }
            return length
        }
        try bounds(lengths)
        let result = ReplyHeader(message: message, lengths: lengths)
        _ = try result.logical(nil) // Reject duplicates, missing/unused indices before body IO.
        return result
    }
}
#endif
