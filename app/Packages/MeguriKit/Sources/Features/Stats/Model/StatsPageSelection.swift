import CoreBridge
import Foundation

/// Reviews Over Time's first switch (S7.4).
nonisolated public enum StatsReviewsMode: String, CaseIterable, Hashable, Sendable {
    case byType, byAnswer
}

/// Reviews Over Time's second switch (S7.4).
nonisolated public enum StatsReviewsMeasure: String, CaseIterable, Hashable, Sendable {
    case count, time
}

/// Retention's switch (S7.2).
nonisolated public enum StatsRetentionKind: String, CaseIterable, Hashable, Sendable {
    case all, young, mature
}

/// The page's two controls and the boxes' switches (`stats/statistics-page` S3.3): kept per
/// window (scene state) and per device, never synced and never written to the collection.
nonisolated public struct StatsPageSelection: Hashable, Sendable, Codable, RawRepresentable {
    /// nil: All Decks.
    public var deck: Int64?
    public var period: StatsPagePeriod
    public var reviewsMode: StatsReviewsMode
    public var reviewsMeasure: StatsReviewsMeasure
    public var retentionKind: StatsRetentionKind

    /// The first time on a device: All Decks and All (decided 2026-11-22), each switch on its
    /// first option.
    public static let initial = StatsPageSelection(deck: nil, period: .all, reviewsMode: .byType,
                                                   reviewsMeasure: .count, retentionKind: .all)

    public init(deck: Int64?, period: StatsPagePeriod, reviewsMode: StatsReviewsMode = .byType,
                reviewsMeasure: StatsReviewsMeasure = .count, retentionKind: StatsRetentionKind = .all) {
        self.deck = deck
        self.period = period
        self.reviewsMode = reviewsMode
        self.reviewsMeasure = reviewsMeasure
        self.retentionKind = retentionKind
    }

    public var scope: StatsPageScope { deck.map(StatsPageScope.deck) ?? .allDecks }

    // A stored value that is not an option reads as the first option (S3.3); a period that is
    // not one reads as All, the first period.
    private enum CodingKeys: String, CodingKey {
        case deck, period, reviewsMode, reviewsMeasure, retentionKind
    }

    static let periodNames: [StatsPagePeriod: String] = [
        .oneMonth: "1M", .threeMonths: "3M", .sixMonths: "6M", .oneYear: "1Y", .all: "All",
    ]

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        func text(_ key: CodingKeys) -> String? { (try? container.decodeIfPresent(String.self, forKey: key)) ?? nil }
        deck = (try? container.decodeIfPresent(Int64.self, forKey: .deck)) ?? nil
        period = text(.period).flatMap { name in Self.periodNames.first { $0.value == name }?.key } ?? .all
        reviewsMode = text(.reviewsMode).flatMap(StatsReviewsMode.init(rawValue:)) ?? .byType
        reviewsMeasure = text(.reviewsMeasure).flatMap(StatsReviewsMeasure.init(rawValue:)) ?? .count
        retentionKind = text(.retentionKind).flatMap(StatsRetentionKind.init(rawValue:)) ?? .all
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encodeIfPresent(deck, forKey: .deck)
        try container.encode(Self.periodNames[period], forKey: .period)
        try container.encode(reviewsMode.rawValue, forKey: .reviewsMode)
        try container.encode(reviewsMeasure.rawValue, forKey: .reviewsMeasure)
        try container.encode(retentionKind.rawValue, forKey: .retentionKind)
    }

    /// For `@SceneStorage`: the selection as JSON text.
    public init?(rawValue: String) {
        guard let data = rawValue.data(using: .utf8),
              let value = try? JSONDecoder().decode(StatsPageSelection.self, from: data) else { return nil }
        self = value
    }

    public var rawValue: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return (try? encoder.encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? ""
    }

    // Field by field: RawRepresentable's own `==` would compare the JSON text.
    public static func == (lhs: StatsPageSelection, rhs: StatsPageSelection) -> Bool {
        lhs.deck == rhs.deck && lhs.period == rhs.period && lhs.reviewsMode == rhs.reviewsMode
            && lhs.reviewsMeasure == rhs.reviewsMeasure && lhs.retentionKind == rhs.retentionKind
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(deck)
        hasher.combine(period)
        hasher.combine(reviewsMode)
        hasher.combine(reviewsMeasure)
        hasher.combine(retentionKind)
    }
}

/// Keeps the last selection used on this device, which the next Statistics page opened starts
/// with (S3.3). With nil defaults it keeps nothing (test-hook launches start fresh).
public struct StatsSelectionStore: Sendable {
    public static let key = "meguri.stats.selection"
    private nonisolated(unsafe) let defaults: UserDefaults?

    public init(defaults: UserDefaults?) {
        self.defaults = defaults
    }

    /// The saved selection, or the first-time one when none was saved.
    public func load() -> StatsPageSelection {
        guard let text = defaults?.string(forKey: Self.key), let value = StatsPageSelection(rawValue: text) else {
            return .initial
        }
        return value
    }

    public func save(_ selection: StatsPageSelection) {
        defaults?.set(selection.rawValue, forKey: Self.key)
    }

    /// Whether the narrow page's Activity grid shows every year (Show All Years), kept per device;
    /// the first time it shows the recent lines.
    public static let activityAllYearsKey = "meguri.stats.activityAllYears"

    public func loadActivityAllYears() -> Bool {
        defaults?.bool(forKey: Self.activityAllYearsKey) ?? false
    }

    public func saveActivityAllYears(_ value: Bool) {
        defaults?.set(value, forKey: Self.activityAllYearsKey)
    }
}
