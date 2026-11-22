import SwiftUI

/// A chart series' colour (`stats/statistics-page` S8.1, contract N25.4).
///
/// Every value is a system colour, so the light, dark and Increase Contrast
/// variants come from the system and none is a custom value. These are chart
/// series colours (`ChartColors` in the spec), not the deck count colours of the
/// rubric's `color-meaning` rule, which keep their own meaning where they apply
/// (S8.1's recorded exception).
public enum StatsSeriesColor: String, CaseIterable, Hashable, Sendable {
    // The card state family.
    case new, learning, relearning, young, mature, suspended, buried
    /// Filtered reviews: only in Reviews Over Time › By Type.
    case filtered
    // The answer family.
    case again, hard, good, easy
    /// A chart of a single measure (Stability, Difficulty, Time of Day's reviews, By Deck).
    case measure
    /// The Forgotten chart: orange, decided 2026-11-22 ("red is a bit aggressive").
    case forgotten
    /// Retention, Retrievability and Knowledge's Known line: green.
    case remembered
    /// Knowledge's Studied line: grey, drawn dashed.
    case reference
    /// A reference rule such as the desired retention: the label colour (S7.2), drawn dashed.
    case rule

    /// The colour family a series belongs to. A chart draws one family, never
    /// both; the neutral colours (`reference`, `rule`) go with any.
    public enum Family: Hashable, Sendable {
        case cardStates, answers, singleMeasure, neutral
    }

    public var family: Family {
        switch self {
        case .new, .learning, .relearning, .young, .mature, .suspended, .buried, .filtered: .cardStates
        case .again, .hard, .good, .easy: .answers
        case .measure, .forgotten, .remembered: .singleMeasure
        case .reference, .rule: .neutral
        }
    }

    /// The system colour. Mature, Good and the remembered measure share the
    /// heatmap's system green (N24.6); Young is mint, so the two stay apart in
    /// a stack.
    public var color: Color {
        switch self {
        case .new, .easy: .blue
        case .learning, .hard, .forgotten: .orange
        case .relearning, .again: .red
        case .young: .mint
        case .mature, .good, .remembered: .green
        case .suspended: .yellow
        case .buried, .reference: .gray
        case .filtered: .purple
        case .measure: .indigo
        case .rule: .primary
        }
    }

    /// Whether a set of series colours keeps to one family (S8.1: "A chart shows
    /// one family, never both").
    public static func keepsOneFamily(_ colors: some Sequence<StatsSeriesColor>) -> Bool {
        Set(colors.map(\.family)).subtracting([.neutral]).count <= 1
    }
}
