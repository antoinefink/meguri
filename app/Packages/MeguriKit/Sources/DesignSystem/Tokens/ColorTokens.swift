import SwiftUI

/// Meguri's colours (R1992, KTD20). Each case names a colour set in
/// `Resources/Colors.xcassets` with light, dark and increased-contrast
/// variants of both; features use these names, never raw values (the
/// `raw-color` lint rule, R1994).
///
/// Every token declares its `role`: a background, or a foreground with the
/// backgrounds it is drawn on. The switch is exhaustive, so a new token cannot
/// compile without one, and `TokenCoverageTests` fails when a token takes part
/// in no pairing. `ContrastTests` checks every pairing in light, dark and
/// increased contrast (R1979).
nonisolated public enum ColorToken: String, CaseIterable, Sendable {
    // Surfaces
    /// Window and screen background.
    case backgroundPrimary
    /// Grouped and inset backgrounds (lists, forms, the gallery).
    case backgroundSecondary
    /// Raised content on a grouped background (rows, panels).
    case surface
    /// The default card page in `base.css`: the review's toolbar and answer row share it.
    case cardPage

    // Text
    case textPrimary
    case textSecondary

    // Deck counts (R32): New blue, Learn red, Due green; a zero count dimmed.
    case countNew
    case countLearn
    case countDue
    case countZero

    // Ratings (R355): Again red and Good green, shared with the tap-zone
    // tints; Hard and Easy use neutral text.
    case ratingAgain
    case ratingGood

    // Status
    /// Accent for normal state changes (e.g. sync with changes to send, R104).
    case statusAccent
    /// Attention without failure (e.g. one-way sync required, R104).
    case statusWarning
    /// Failures and error states.
    case statusError
    /// Labels of destructive actions outside system dialogs.
    case destructive

    // Controls
    /// The filled part of progress indicators.
    case progressFill

    /// The colour, resolved by SwiftUI for the current appearance and
    /// contrast.
    public var color: Color { Color(rawValue, bundle: .module) }

    /// What the token is drawn on.
    public var role: ColorRole {
        switch self {
        case .backgroundPrimary, .backgroundSecondary, .surface, .cardPage:
            .background
        // Drawn around the card in a review, too.
        case .textPrimary, .ratingAgain, .statusWarning:
            .foreground(.text, on: Self.contentBackgrounds + [.cardPage])
        case .textSecondary, .countNew, .countLearn, .countDue, .countZero,
             .ratingGood, .statusAccent, .statusError, .destructive:
            .foreground(.text, on: Self.contentBackgrounds)
        case .progressFill:
            .foreground(.nonText, on: Self.contentBackgrounds)
        }
    }

    /// The backgrounds content is drawn on.
    public static let contentBackgrounds: [ColorToken] = [.backgroundPrimary, .backgroundSecondary, .surface]

    /// Every declared foreground-on-background pairing.
    public static var pairings: [ContrastPairing] {
        allCases.flatMap { token -> [ContrastPairing] in
            guard case let .foreground(use, backgrounds) = token.role else { return [] }
            return backgrounds.map { ContrastPairing(foreground: token, background: $0, use: use) }
        }
    }
}

/// What a colour token is drawn on.
nonisolated public enum ColorRole: Hashable, Sendable {
    case background
    /// Drawn on each of the given backgrounds, as text or as non-text UI.
    case foreground(ContrastUse, on: [ColorToken])
}

/// How a foreground is used, which sets its minimum contrast (R1979): text
/// needs 4.5:1; large text and non-text UI (icons, indicators, control
/// boundaries) need 3:1.
nonisolated public enum ContrastUse: Hashable, Sendable {
    case text
    case largeText
    case nonText
}

/// One foreground token drawn on one background token.
nonisolated public struct ContrastPairing: Hashable, Sendable {
    public let foreground: ColorToken
    public let background: ColorToken
    public let use: ContrastUse
}

extension ShapeStyle where Self == Color {
    /// A colour token as a shape style: `.foregroundStyle(.token(.countNew))`.
    public static func token(_ token: ColorToken) -> Color { token.color }
}
