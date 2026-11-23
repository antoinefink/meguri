import Foundation

/// Reads `stats/graphs-common` G6's encoding of one part's value, as the
/// core writes it (`meguri_stats::graphs::encoding`):
///
/// - an integer is a zigzag LEB128 varint, and so are a map's number of
///   runs, its start keys and distances, its run lengths and a list's
///   length; a text is its UTF-8 byte length as an unsigned varint, then
///   the bytes;
/// - a sparse map is its number of runs, then per run its start key (the
///   first as is, each later one as its distance from the previous run's
///   last key), its length, then one value array per member with that
///   many items;
/// - a binary32 or binary64 value is its IEEE-754 bytes, least significant
///   first;
/// - an optional value (the page's own parts) is a list of none or one item.
///
/// A record is its members one after another: `finish()` fails on any byte
/// left, so a value is never taken from bytes it does not account for.
struct StatsBytes {
    struct Failure: Error, Hashable {
        let reason: String
    }

    private let bytes: [UInt8]
    private var at = 0

    init(_ data: Data) {
        bytes = [UInt8](data)
    }

    init(_ bytes: [UInt8]) {
        self.bytes = bytes
    }

    mutating func unsigned() throws(Failure) -> UInt64 {
        var value: UInt64 = 0
        var shift: UInt64 = 0
        while shift < 64 {
            guard at < bytes.count else { throw Failure(reason: "truncated") }
            let byte = bytes[at]
            at += 1
            // The tenth byte holds the 64th bit only.
            if shift == 63 && byte & 0x7f > 1 { throw Failure(reason: "a varint over 64 bits") }
            value |= UInt64(byte & 0x7f) << shift
            if byte & 0x80 == 0 { return value }
            shift += 7
        }
        throw Failure(reason: "a varint over 64 bits")
    }

    mutating func integer() throws(Failure) -> Int {
        let value = try unsigned()
        return Int(Int64(bitPattern: (value >> 1) ^ (0 &- (value & 1))))
    }

    /// An integer that counts something: never negative.
    mutating func count() throws(Failure) -> Int {
        let value = try integer()
        guard value >= 0 else { throw Failure(reason: "a negative count") }
        return value
    }

    mutating func flag() throws(Failure) -> Bool {
        switch try integer() {
        case 0: return false
        case 1: return true
        default: throw Failure(reason: "a flag that is neither 0 nor 1")
        }
    }

    private mutating func fixed(_ size: Int) throws(Failure) -> UInt64 {
        guard bytes.count - at >= size else { throw Failure(reason: "truncated") }
        var value: UInt64 = 0
        for offset in 0..<size {
            value |= UInt64(bytes[at + offset]) << (8 * UInt64(offset))
        }
        at += size
        return value
    }

    mutating func float32() throws(Failure) -> Float {
        Float(bitPattern: UInt32(try fixed(4)))
    }

    mutating func float64() throws(Failure) -> Double {
        Double(bitPattern: try fixed(8))
    }

    mutating func text() throws(Failure) -> String {
        let length = try unsigned()
        guard length <= UInt64(bytes.count - at) else { throw Failure(reason: "truncated") }
        let end = at + Int(length)
        guard let text = String(validating: bytes[at..<end], as: UTF8.self) else {
            throw Failure(reason: "a text that is not UTF-8")
        }
        at = end
        return text
    }

    mutating func list<T>(_ item: (inout StatsBytes) throws(Failure) -> T) throws(Failure) -> [T] {
        let length = try count()
        var items: [T] = []
        items.reserveCapacity(min(length, bytes.count - at))
        for _ in 0..<length {
            items.append(try item(&self))
        }
        return items
    }

    /// A list of exactly `length` items.
    mutating func list<T>(of length: Int, _ item: (inout StatsBytes) throws(Failure) -> T) throws(Failure) -> [T] {
        let items = try list(item)
        guard items.count == length else { throw Failure(reason: "a list of \(items.count) items, not \(length)") }
        return items
    }

    mutating func optional<T>(_ item: (inout StatsBytes) throws(Failure) -> T) throws(Failure) -> T? {
        let items = try list(item)
        guard items.count <= 1 else { throw Failure(reason: "an optional value of \(items.count) items") }
        return items.first
    }

    /// A sparse map whose `members` integers per key make one value.
    mutating func map<Value>(members: Int, _ value: ([Int]) -> Value) throws(Failure) -> StatsSparseMap<Value> {
        let runs = try count()
        var keys: [Int] = []
        var values: [Value] = []
        var previous: Int?
        for _ in 0..<runs {
            let coded = try integer()
            let start = previous.map { $0 &+ coded } ?? coded
            let length = try count()
            guard length > 0 else { throw Failure(reason: "an empty run") }
            // A run whose last key overflows would leave the keys out of order.
            guard !start.addingReportingOverflow(length - 1).overflow else { throw Failure(reason: "a run past the key range") }
            if let previous, start <= previous { throw Failure(reason: "keys out of order") }
            var columns: [[Int]] = []
            columns.reserveCapacity(members)
            for _ in 0..<members {
                var column: [Int] = []
                column.reserveCapacity(min(length, bytes.count - at))
                for _ in 0..<length {
                    column.append(try integer())
                }
                columns.append(column)
            }
            for offset in 0..<length {
                keys.append(start &+ offset)
                values.append(value(columns.map { $0[offset] }))
            }
            previous = keys.last
        }
        return StatsSparseMap(keys: keys, values: values)
    }

    /// A map of one integer per key.
    mutating func counts() throws(Failure) -> StatsSparseMap<Int> {
        try map(members: 1) { $0[0] }
    }

    /// Fails unless every byte was read.
    func finish() throws(Failure) {
        guard at == bytes.count else { throw Failure(reason: "\(bytes.count - at) bytes left") }
    }
}

/// A sparse map of G6: keys ascending, one value per key; only the keys
/// that have data.
public struct StatsSparseMap<Value: Sendable & Hashable>: Sendable, Hashable {
    public let keys: [Int]
    public let values: [Value]

    public init(keys: [Int], values: [Value]) {
        precondition(keys.count == values.count, "one value per key")
        self.keys = keys
        self.values = values
    }

    public static var empty: StatsSparseMap { StatsSparseMap(keys: [], values: []) }

    public var isEmpty: Bool { keys.isEmpty }
    public var count: Int { keys.count }

    /// The value at `key`, or nil when the key has no data.
    public subscript(key: Int) -> Value? {
        var low = 0
        var high = keys.count
        while low < high {
            let middle = (low + high) / 2
            if keys[middle] < key { low = middle + 1 } else { high = middle }
        }
        return low < keys.count && keys[low] == key ? values[low] : nil
    }

    /// Every entry, in key order.
    public var entries: [(key: Int, value: Value)] {
        zip(keys, values).map { (key: $0, value: $1) }
    }
}
