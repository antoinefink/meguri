import CoreBridge
import Foundation

/// The parts one refresh returned (one request, `stats/history-reads` H15): every part of a set
/// shares the request's header, so its days, its context and its scope are one. A box draws
/// all its parts from one set, never from two refreshes, and only for the scope the set was
/// read for (S10.2).
nonisolated public struct StatsPartSet: Sendable, Hashable, Codable {
    /// The scope and the history the request named.
    public let scope: StatsPageScope
    public let history: StatsHistory
    /// The request's header, from its first part on.
    public private(set) var header: StatsPageHeader?
    public private(set) var parts: [StatsPart: StatsPartOutcome]
    /// The request ended `.complete`: every part it named returned or failed.
    public internal(set) var complete: Bool
    /// The model's refresh number: the request's parts carry it back.
    let refresh: UInt64
    /// This set, among every window's and every launch's.
    let id: UUID

    init(scope: StatsPageScope, history: StatsHistory, refresh: UInt64) {
        self.scope = scope
        self.history = history
        self.refresh = refresh
        id = UUID()
        header = nil
        parts = [:]
        complete = false
    }

    /// Takes a returned part. A part of another request (another refresh, scope or history)
    /// is refused: it never joins this set.
    @discardableResult
    mutating func receive(_ result: StatsPartResult) -> Bool {
        guard result.header.refresh == refresh, result.header.history == history else { return false }
        if let header, header != result.header { return false }
        header = result.header
        parts[result.part] = result.outcome
        return true
    }

    /// When the set's values were read (G1's now): newer sets win.
    public var readAt: Date? {
        header.map { Date(timeIntervalSince1970: TimeInterval($0.context.nowMilliseconds) / 1_000) }
    }

    /// Whether this set was read after `other`.
    func isNewer(than other: StatsPartSet) -> Bool {
        let mine = header?.context.nowMilliseconds ?? .min
        let theirs = other.header?.context.nowMilliseconds ?? .min
        return mine != theirs ? mine > theirs : refresh > other.refresh
    }

    /// The part's value, when it returned data.
    func value(_ part: StatsPart) -> StatsPartValue? {
        if case let .data(value) = parts[part] { return value }
        return nil
    }

    /// Whether every one of `wanted` has returned (with data or a failure).
    func has(_ wanted: [StatsPart]) -> Bool {
        wanted.allSatisfy { parts[$0] != nil }
    }
}

/// The parts whose values depend on the history window: with `last12Months` they hold only
/// the last year, so on All's axis they are partial until the `allHistory` refresh returns
/// (S10.3's two refreshes). The present state, Activity (every year whatever the window) and
/// Retention Today are complete with either history.
nonisolated extension StatsPart {
    var dependsOnWindow: Bool {
        switch self {
        case .reviews, .byAnswer, .retentionByDay, .added, .cardStatesOverTime, .started, .knowledgeOverTime,
             .hours, .buttons, .byDeck:
            true
        case .cardCounts, .futureDue, .intervals, .stability, .difficulty, .retrievability, .studiedToday,
             .retention, .activityDays:
            false
        }
    }
}
