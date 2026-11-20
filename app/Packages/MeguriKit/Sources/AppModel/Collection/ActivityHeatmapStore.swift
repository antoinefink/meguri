import CoreBridge
import Observation

/// Which reviews an activity heatmap shows (`stats/activity-heatmap` AH4).
public enum ActivityScope: Hashable, Sendable {
    case collection
    case deck(Int64)
}

/// The activity heatmap's in-memory cache (contract N14.11): one query per scope,
/// kept per (scope, scheduler day) and marked stale by the collection's own change
/// events, never by polling. A page shows a scope's last value at once, stale or
/// not, while a refresh runs off the main actor; a width change never asks again.
@Observable
@MainActor
public final class ActivityHeatmapStore {
    /// What a cached value was read for: the scheduler day (counted by day events)
    /// and the change revisions it saw.
    public struct Stamp: Hashable, Sendable {
        let day: UInt64
        let reviews: UInt64
        let membership: UInt64
    }
    /// What a page keys its load task by: a new key means a refresh is due.
    public struct Request: Hashable, Sendable {
        public let scope: ActivityScope
        let stamp: Stamp
    }

    private struct Entry {
        var days: ActivityDays
        var stamp: Stamp
    }

    /// Scopes kept: the collection and the last few deck pages.
    static let capacity = 6

    private let service: any ActivityService
    private var entries: [ActivityScope: Entry] = [:]
    /// Least recently used first.
    @ObservationIgnored private var order: [ActivityScope] = []
    /// Moved by every day change (rollover, zone, `rollover` key): a new day is a new key.
    private var day: UInt64 = 0
    /// Moved by anything that adds or removes review rows: every scope is stale.
    private var reviews: UInt64 = 0
    /// Moved by card and deck changes: deck scopes are stale, the collection's is not.
    private var membership: UInt64 = 0
    @ObservationIgnored private var generation: UInt64?

    public init(service: any ActivityService = CoreActivityService()) {
        self.service = service
    }

    /// The scope's last value, current or stale; nil before its first result.
    public func days(_ scope: ActivityScope) -> ActivityDays? {
        entries[scope]?.days
    }

    /// The key a page's load task runs under (`.task(id:)`).
    public func request(_ scope: ActivityScope) -> Request {
        Request(scope: scope, stamp: stamp(for: scope))
    }

    /// Reads the scope unless its value is current. A cancelled call (the page went
    /// away) or a result for a superseded key or generation is dropped.
    public func load(_ request: Request) async {
        let scope = request.scope
        guard request.stamp == stamp(for: scope) else { return }
        if let entry = entries[scope], entry.stamp == request.stamp {
            touch(scope)
            return
        }
        let expected = generation
        let deckID: Int64? = if case .deck(let id) = scope { id } else { nil }
        guard let days = try? await service.heatmap(deckID: deckID) else { return }
        guard !Task.isCancelled, request.stamp == stamp(for: scope),
              expected == generation, generation.map({ $0 == days.generation }) ?? true else { return }
        entries[scope] = Entry(days: days, stamp: request.stamp)
        touch(scope)
    }

    // MARK: Invalidation, from the collection's events

    /// A committed op's changes (`ChangeFanout`): answers, undo, sync and imports.
    public func changesCommitted(_ changes: ChangeSet) {
        switch Self.effect(of: changes) {
        case .none: break
        case .membership: membership &+= 1
        case .reviews: reviews &+= 1
        case .day: day &+= 1
        }
    }

    /// A review session ended: its answers already moved `reviews`; this also covers
    /// a session whose changes this owner did not see.
    public func sessionEnded() { reviews &+= 1 }

    /// Rollover, a time zone change, or a new `next_day_at`.
    public func dayChanged() { day &+= 1 }

    /// Another collection, or the same file reopened: nothing kept applies.
    public func generationChanged(to next: UInt64?) {
        guard next != generation || next == nil else { return }
        generation = next
        entries.removeAll(); order.removeAll()
        reviews &+= 1
    }

    enum Effect: Equatable { case none, membership, reviews, day }

    /// What a change does to cached counts. A new or removed review row changes every
    /// scope; a card or deck change can move rows between deck scopes; a change of the
    /// `rollover` key moves the day boundary.
    static func effect(of changes: ChangeSet) -> Effect {
        if changes.wholeCollection || !changes.revlogAdded.isEmpty || !changes.revlogRemoved.isEmpty {
            return .reviews
        }
        if changes.configKeys.contains(where: { $0.name == "rollover" }) { return .day }
        if !changes.decks.isEmpty || changes.cards.contains(where: { !$0.memoryStateOnly }) {
            return .membership
        }
        return .none
    }

    private func stamp(for scope: ActivityScope) -> Stamp {
        // The collection's counts ignore which deck a card is in.
        Stamp(day: day, reviews: reviews, membership: scope == .collection ? 0 : membership)
    }

    private func touch(_ scope: ActivityScope) {
        order.removeAll { $0 == scope }
        order.append(scope)
        while order.count > Self.capacity {
            entries[order.removeFirst()] = nil
        }
    }
}
