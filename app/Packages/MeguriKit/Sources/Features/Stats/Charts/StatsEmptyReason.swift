import Foundation

/// Why a box has no data: its one line in the plot area (S8.5, S16).
public enum StatsEmptyReason: Hashable, Sendable {
    /// The span a chart of the past looks back over, for its empty line.
    public enum Span: Hashable, Sendable {
        case lastMonth, last3Months, last6Months, lastYear
    }

    /// No review ever in the page scope.
    case noReviewsYet
    /// No review in the span, optionally in a filtered deck.
    case noReviews(in: Span, deck: String?)
    /// Reviews Due with nothing due.
    case noCardsDue
    /// The Memory boxes with no card with a memory state.
    case noMemoryState
    /// Knowledge Over Time with no card with a memory estimate.
    case noMemoryEstimate

    public var text: String {
        switch self {
        case .noReviewsYet:
            return String(localized: "stats.empty.noReviewsYet", bundle: .module)
        case let .noReviews(span, deck):
            let spanText = span.text
            if let deck {
                return String(localized: "stats.empty.noReviewsInDeck \(deck) \(spanText)", bundle: .module)
            }
            return String(localized: "stats.empty.noReviewsIn \(spanText)", bundle: .module)
        case .noCardsDue:
            return String(localized: "stats.empty.noCardsDue", bundle: .module)
        case .noMemoryState:
            return String(localized: "stats.empty.noMemoryState", bundle: .module)
        case .noMemoryEstimate:
            return String(localized: "stats.empty.noMemoryEstimate", bundle: .module)
        }
    }
}

extension StatsEmptyReason.Span {
    /// "the last month", "the last 3 months", … (S8.5).
    public var text: String {
        switch self {
        case .lastMonth: String(localized: "stats.span.lastMonth", bundle: .module)
        case .last3Months: String(localized: "stats.span.last3Months", bundle: .module)
        case .last6Months: String(localized: "stats.span.last6Months", bundle: .module)
        case .lastYear: String(localized: "stats.span.lastYear", bundle: .module)
        }
    }
}
