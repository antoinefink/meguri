import CoreBridge

nonisolated extension ReviewCounts {
    /// Everything left to study today: new, learning and to-review cards together (N13.2).
    public var waiting: Int { Int(new) + Int(learning) + Int(review) }
}

/// What a deck page, a sidebar row and the Study command say about a deck's waiting cards (N14.3, N14.6).
nonisolated public struct DeckWaiting: Sendable, Equatable {
    public let counts: ReviewCounts
    public let filtered: Bool
    public init(counts: ReviewCounts, filtered: Bool) { self.counts = counts; self.filtered = filtered }

    /// New, learning and to-review cards together.
    public var total: Int { counts.waiting }
    /// A regular deck with nothing waiting says so and offers Custom Study in Study's place,
    /// or Add Note when it has no card (N14.3). A filtered one offers Rebuild instead (F6).
    public var regularNothingWaiting: Bool { total == 0 && !filtered }
    /// A sidebar row reads quieter with nothing waiting, except on the selection highlight.
    public func quietInSidebar(selected: Bool) -> Bool { total == 0 && !selected }
}
