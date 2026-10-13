import SwiftUI

/// One named motion (R1992, KTD20): how a change animates, and what it does
/// instead when Reduce Motion is on. Every token carries both, so no animation
/// in the app lacks a Reduce Motion alternative. Animations and moving
/// transitions written outside this folder fail the `raw-motion` lint rule.
nonisolated public struct MotionToken: Hashable, Sendable {
    public enum Curve: Hashable, Sendable {
        case easeOut
        case easeInOut
        /// A spring that settles within `duration`.
        case spring(bounce: Double)
    }

    /// The Reduce Motion alternative: never movement.
    public enum Reduced: Hashable, Sendable {
        /// A short opacity change.
        case crossfade(duration: Double)
        /// No animation: the change jumps.
        case instant
    }

    public let name: String
    /// Seconds.
    public let duration: Double
    public let curve: Curve
    public let reduced: Reduced

    public var animation: Animation {
        switch curve {
        case .easeOut: .easeOut(duration: duration)
        case .easeInOut: .easeInOut(duration: duration)
        case let .spring(bounce): .spring(duration: duration, bounce: bounce)
        }
    }

    /// The Reduce Motion animation; `nil` when the change jumps.
    public var reducedAnimation: Animation? {
        switch reduced {
        case let .crossfade(duration): .easeInOut(duration: duration)
        case .instant: nil
        }
    }

    public func animation(reduceMotion: Bool) -> Animation? {
        reduceMotion ? reducedAnimation : animation
    }

    /// `moving` normally; under Reduce Motion a crossfade, or nothing when
    /// the token jumps.
    public func transition(_ moving: AnyTransition, reduceMotion: Bool) -> AnyTransition {
        guard reduceMotion else { return moving.animation(animation) }
        switch reduced {
        case .crossfade: return AnyTransition.opacity.animation(reducedAnimation)
        case .instant: return .identity
        }
    }
}

/// The motion tokens.
nonisolated public enum Motion {
    /// Small state changes: counts, progress, toggled indicators.
    public static let quick = MotionToken(name: "quick", duration: 0.15, curve: .easeOut, reduced: .instant)
    /// Most transitions: content swaps, toasts, panels.
    public static let standard = MotionToken(name: "standard", duration: 0.25, curve: .easeInOut, reduced: .crossfade(duration: 0.15))
    /// Large, deliberate moves: sheets of content, reveals.
    public static let emphasized = MotionToken(name: "emphasized", duration: 0.35, curve: .spring(bounce: 0.15), reduced: .crossfade(duration: 0.2))

    /// Every token. A new token joins this list.
    public static let all: [MotionToken] = [quick, standard, emphasized]
}

extension View {
    /// Animates changes of `value` with `token`, or with its Reduce Motion
    /// alternative.
    public func motion(_ token: MotionToken, value: some Equatable) -> some View {
        modifier(MotionAnimation(token: token, value: value))
    }

    /// Inserts and removes this view with `moving` (e.g. `.move(edge: .bottom)`)
    /// animated by `token`; under Reduce Motion it crossfades or jumps instead.
    public func motionTransition(_ moving: AnyTransition, _ token: MotionToken) -> some View {
        modifier(MotionTransition(token: token, moving: moving))
    }
}

private struct MotionAnimation<Value: Equatable>: ViewModifier {
    let token: MotionToken
    let value: Value
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.animation(token.animation(reduceMotion: reduceMotion), value: value)
    }
}

private struct MotionTransition: ViewModifier {
    let token: MotionToken
    let moving: AnyTransition
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.transition(token.transition(moving, reduceMotion: reduceMotion))
    }
}
