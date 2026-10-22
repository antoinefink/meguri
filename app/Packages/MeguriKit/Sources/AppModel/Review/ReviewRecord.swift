import Foundation

/// App presentation history only. The core's pending-answer journal owns replay.
public nonisolated struct ReviewRecord: Codable, Sendable, Equatable {
    public struct Answer: Codable, Sendable, Equatable, Identifiable {
        public var id: UUID
        public var cardID: Int64
        public var rating: String
        public var kind: String
        public var answeredAt: Date
        public var elapsedMilliseconds: UInt64
        public var durable: Bool
        public init(cardID: Int64, rating: String, kind: String, answeredAt: Date, elapsedMilliseconds: UInt64) {
            id = UUID(); self.cardID = cardID; self.rating = rating; self.kind = kind
            self.answeredAt = answeredAt; self.elapsedMilliseconds = elapsedMilliseconds; durable = false
        }
    }
    public var id = UUID()
    public var deckID: Int64
    public var startedAt: Date
    public var endedAt: Date?
    public var finished = false
    public var activeMilliseconds: UInt64 = 0
    public var answers: [Answer] = []
    public var undone: [Answer] = []
    public var missed: Set<Int64> { Set(answers.filter { $0.rating == "again" }.map(\.cardID)) }
    // Attach points for U154/U177 and the later action owners.
    public var buried: Set<Int64> = []
    public var suspended: Set<Int64> = []
    public var forgotten: Set<Int64> = []
    public var rescheduled: Set<Int64> = []
    public var deleted: Set<Int64> = []

    public init(deckID: Int64, startedAt: Date) { self.deckID = deckID; self.startedAt = startedAt }

    public mutating func reject(cardID: Int64) {
        if let index = answers.firstIndex(where: { $0.cardID == cardID && !$0.durable }) {
            answers.removeSubrange(index...)
        }
        undone.removeAll()
    }
}

/// Actor isolation keeps encoding and atomic file replacement off the UI lane.
/// Versions also reject delayed snapshots submitted after a newer recovery.
public actor ReviewRecordStore {
    private let file: URL
    private var version: UInt64 = 0
    private var activeID: UUID?
    public init(file: URL) { self.file = file }
    public func load() throws -> ReviewRecord? {
        guard FileManager.default.fileExists(atPath: file.path) else { return nil }
        let record = try JSONDecoder().decode(ReviewRecord.self, from: Data(contentsOf: file))
        activeID = record.id
        return record
    }
    public func begin(_ record: ReviewRecord) throws {
        activeID = record.id; version = 0
        try save(record, version: 0)
    }
    public func save(_ record: ReviewRecord, version: UInt64) throws {
        guard record.id == activeID, version >= self.version else { return }
        let data = try JSONEncoder().encode(record)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: file, options: .atomic)
        self.version = version
    }
    public func remove() throws {
        if FileManager.default.fileExists(atPath: file.path) { try FileManager.default.removeItem(at: file) }
    }
}

/// Monotonic active time; wall-clock changes cannot add study time.
public struct ReviewActiveTime {
    private var began: TimeInterval?
    private var accumulated: TimeInterval = 0
    public init() {}
    public mutating func start(at instant: TimeInterval) { accumulated = 0; began = instant }
    public mutating func pause(at instant: TimeInterval) {
        if let began { accumulated += max(0, instant - began) }
        began = nil
    }
    public mutating func resume(at instant: TimeInterval) { if began == nil { began = instant } }
    public func milliseconds(at instant: TimeInterval, capSeconds: UInt32) -> UInt64 {
        let seconds = accumulated + (began.map { max(0, instant - $0) } ?? 0)
        return UInt64(min(Double(capSeconds), max(0, seconds)) * 1_000)
    }
}
