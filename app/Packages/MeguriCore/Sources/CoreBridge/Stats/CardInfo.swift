internal import MeguriCoreBindings
import Instrumentation

// Card Info (`stats/card-info` C1 to C11, U92's FFI): one query-lane read of
// one card's snapshot. Every value text is the core's (C12); the view adds
// only its String Catalog labels, keyed by `CardInfoStat.key`.

/// C11's kinds: the view shows its unavailable state for each (C12).
public enum CardInfoUnavailable: Sendable, Hashable {
    case notFound, unreadableRecord, missingNote, missingNotetype, missingTemplate, missingDeck, missingHomeDeck
    case filteredHomeDeck, missingPreset, invalidCutoff, fsrsFailed

    init(_ kind: MeguriCoreBindings.CardInfoUnavailable) {
        self = switch kind {
        case .notFound: .notFound
        case .unreadableRecord: .unreadableRecord
        case .missingNote: .missingNote
        case .missingNotetype: .missingNotetype
        case .missingTemplate: .missingTemplate
        case .missingDeck: .missingDeck
        case .missingHomeDeck: .missingHomeDeck
        case .filteredHomeDeck: .filteredHomeDeck
        case .missingPreset: .missingPreset
        case .invalidCutoff: .invalidCutoff
        case .fsrsFailed: .fsrsFailed
        }
    }
}

/// A C4 row: `cardInfo.row.<key>` names its label; `value` is the core's text.
public struct CardInfoStat: Sendable, Hashable {
    public let key: String
    public let value: String
    public init(key: String, value: String) {
        self.key = key
        self.value = value
    }
}

/// C2's kinds of a review-log row.
public enum CardInfoReviewKind: Sendable, Hashable {
    case learn, review, relearn, filtered, manual, rescheduled

    init(_ kind: MeguriCoreBindings.CardInfoReviewKind) {
        self = switch kind {
        case .learn: .learn
        case .review: .review
        case .relearn: .relearn
        case .filtered: .filtered
        case .manual: .manual
        case .rescheduled: .rescheduled
        }
    }
}

/// A history entry (C6.1) with its column texts (C6.2); the snapshot lists
/// them newest first.
public struct CardInfoReview: Sendable, Hashable, Identifiable {
    public let id: Int64
    public let kind: CardInfoReviewKind
    /// The row's `ease`; 0 for an unrated row.
    public let rating: UInt8
    public let date: String
    public let dateTime: String
    public let kindText: String
    public let ratingText: String
    public let interval: String
    public let ease: String
    public let time: String
    public let stability: String
    public init(id: Int64, kind: CardInfoReviewKind, rating: UInt8, date: String, dateTime: String, kindText: String,
                ratingText: String, interval: String, ease: String, time: String, stability: String) {
        self.id = id
        self.kind = kind
        self.rating = rating
        self.date = date
        self.dateTime = dateTime
        self.kindText = kindText
        self.ratingText = ratingText
        self.interval = interval
        self.ease = ease
        self.time = time
        self.stability = stability
    }
}

/// C7.3's ranges.
public enum CardInfoCurveRangeKind: Sendable, Hashable, CaseIterable {
    case firstWeek, firstMonth, firstYear, allTime

    init(_ kind: MeguriCoreBindings.CardInfoCurveRangeKind) {
        self = switch kind {
        case .firstWeek: .firstWeek
        case .firstMonth: .firstMonth
        case .firstYear: .firstYear
        case .allTime: .allTime
        }
    }
}

/// One point the view draws (C7.5), with its tooltip texts (C7.6).
public struct CardInfoCurvePoint: Sendable, Hashable {
    /// Epoch seconds.
    public let instant: Double
    /// Percent.
    public let retrievability: Double
    /// Drawn dashed (after now).
    public let future: Bool
    public let elapsedText: String
    public let retrievabilityText: String
    public let stabilityText: String
    public init(instant: Double, retrievability: Double, future: Bool, elapsedText: String, retrievabilityText: String,
                stabilityText: String) {
        self.instant = instant
        self.retrievability = retrievability
        self.future = future
        self.elapsedText = elapsedText
        self.retrievabilityText = retrievabilityText
        self.stabilityText = stabilityText
    }
}

public struct CardInfoCurveRange: Sendable, Hashable {
    public let kind: CardInfoCurveRangeKind
    public let spanDays: Double
    public let points: [CardInfoCurvePoint]
    public let yMin: Double
    /// The desired-retention line is above `yMin` and drawn.
    public let showsRetention: Bool
    public init(kind: CardInfoCurveRangeKind, spanDays: Double, points: [CardInfoCurvePoint], yMin: Double, showsRetention: Bool) {
        self.kind = kind
        self.spanDays = spanDays
        self.points = points
        self.yMin = yMin
        self.showsRetention = showsRetention
    }
}

/// C7: the ranges offered, First Week first; one range means no picker.
public struct CardInfoCurve: Sendable, Hashable {
    public let ranges: [CardInfoCurveRange]
    public let defaultRange: CardInfoCurveRangeKind
    /// 0 to 1.
    public let desiredRetention: Double
    public let desiredRetentionText: String
    public init(ranges: [CardInfoCurveRange], defaultRange: CardInfoCurveRangeKind, desiredRetention: Double,
                desiredRetentionText: String) {
        self.ranges = ranges
        self.defaultRange = defaultRange
        self.desiredRetention = desiredRetention
        self.desiredRetentionText = desiredRetentionText
    }
}

/// What the snapshot read (C10): a change set naming none of these leaves it as it is.
public struct CardInfoDependencies: Sendable, Hashable {
    public let card: Int64
    public let note: Int64
    public let notetype: Int64
    public let deck: Int64
    public let homeDeck: Int64
    public let preset: Int64
    public let revlog: [Int64]
    /// The Due date moves at local midnight (C10.2).
    public let dueFromDayNumber: Bool
    public init(card: Int64, note: Int64, notetype: Int64, deck: Int64, homeDeck: Int64, preset: Int64, revlog: [Int64],
                dueFromDayNumber: Bool) {
        self.card = card
        self.note = note
        self.notetype = notetype
        self.deck = deck
        self.homeDeck = homeDeck
        self.preset = preset
        self.revlog = revlog
        self.dueFromDayNumber = dueFromDayNumber
    }
}

/// C1's snapshot of one card, as the view shows it.
public struct CardInfoSnapshot: Sendable, Hashable {
    public let cardID: Int64
    public let noteID: Int64
    /// C4's rows in the core's order.
    public let rows: [CardInfoStat]
    /// C6, newest first.
    public let history: [CardInfoReview]
    /// Nil: no chart (C7.1).
    public let curve: CardInfoCurve?
    public let dependencies: CardInfoDependencies
    public init(cardID: Int64, noteID: Int64, rows: [CardInfoStat], history: [CardInfoReview], curve: CardInfoCurve?,
                dependencies: CardInfoDependencies) {
        self.cardID = cardID
        self.noteID = noteID
        self.rows = rows
        self.history = history
        self.curve = curve
        self.dependencies = dependencies
    }

    init(_ raw: MeguriCoreBindings.CardInfoSnapshot) {
        let deps = raw.dependencies
        self.init(
            cardID: raw.raw.cardId, noteID: raw.raw.noteId,
            rows: raw.rows.map { CardInfoStat(key: $0.key, value: $0.value) },
            history: raw.history.map {
                CardInfoReview(id: $0.revlogId, kind: CardInfoReviewKind($0.kind), rating: $0.rating, date: $0.dateText,
                               dateTime: $0.dateTimeText, kindText: $0.kindText, ratingText: $0.ratingText,
                               interval: $0.intervalText, ease: $0.easeText, time: $0.timeText, stability: $0.stabilityText)
            },
            curve: raw.curve.map { curve in
                CardInfoCurve(
                    ranges: curve.ranges.map { range in
                        CardInfoCurveRange(
                            kind: CardInfoCurveRangeKind(range.kind), spanDays: range.spanDays,
                            points: range.points.map {
                                CardInfoCurvePoint(instant: $0.instant, retrievability: $0.retrievability, future: $0.future,
                                                   elapsedText: $0.elapsedText, retrievabilityText: $0.retrievabilityText,
                                                   stabilityText: $0.stabilityText)
                            },
                            yMin: range.yMin, showsRetention: range.showsRetention)
                    },
                    defaultRange: CardInfoCurveRangeKind(curve.defaultRange), desiredRetention: Double(curve.desiredRetention),
                    desiredRetentionText: curve.desiredRetentionText)
            },
            dependencies: CardInfoDependencies(card: deps.cardId, note: deps.noteId, notetype: deps.notetypeId, deck: deps.deckId,
                                               homeDeck: deps.homeDeckId, preset: deps.presetId, revlog: deps.revlogIds,
                                               dueFromDayNumber: deps.dueFromDayNumber))
    }
}

/// A card's snapshot, or why there is none (C11).
public enum CardInfoResult: Sendable, Hashable {
    case found(CardInfoSnapshot)
    case unavailable(CardInfoUnavailable)
}

extension Core {
    /// C1: the card's snapshot, one read on the query lane; it never writes (R1252).
    public static func cardInfo(_ cardID: Int64) async throws(BridgeError) -> CardInfoResult {
        try await QueryLane.shared.call(.cardInfo) {
            switch try MeguriCoreBindings.cardInfo(cardId: cardID) {
            case .found(let snapshot): .found(CardInfoSnapshot(snapshot))
            case .unavailable(let kind): .unavailable(CardInfoUnavailable(kind))
            }
        }
    }

    /// C8: Copy Debug Info's JSON; nil when the card has no snapshot.
    public static func cardInfoDebugJSON(_ cardID: Int64) async throws(BridgeError) -> String? {
        try await QueryLane.shared.call(.cardInfoDebugJSON) {
            switch try MeguriCoreBindings.cardInfoDebugJson(cardId: cardID) {
            case .found(let json): json
            case .unavailable: nil
            }
        }
    }
}
