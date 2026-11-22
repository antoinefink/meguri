import CardRendering
import CoreGraphics
import Foundation

/// Review tap zones (U180; R401–R406, R412, R415, R416): what one finished
/// touch on the card does. Pure: the native gesture, the page's observation
/// of the same gesture and the review's state go in, one action comes out.
public nonisolated enum ReviewTapAction: Equatable, Sendable {
    case reveal
    case again
    case good
    /// R412: a tap outside the focused answer field only dismisses the keyboard.
    case dismissKeyboard
    case none(ReviewTapRefusal)
}

/// Why a tap did nothing. For tests and diagnostics only.
public nonisolated enum ReviewTapRefusal: Equatable, Sendable {
    case notATap, otherCard, inactive, exempt, cancelledByCard, selection,
         doubleTapPrevention, answerJustShown, assistiveTechnology, notNearTouch
}

/// What native touch tracking saw (R404, R415).
public nonisolated struct ReviewTapGesture: Equatable, Sendable {
    public var downUptime: TimeInterval
    public var upUptime: TimeInterval
    /// The most fingers down at once.
    public var fingers: Int
    /// The farthest the touch went from where it started, in points.
    public var movement: CGFloat
    /// The system cancelled the touch, or the card was scrolling when it began.
    public var interrupted: Bool
    /// Where it ended, in the card view's points.
    public var location: CGPoint
    /// The presented side when the touch went down.
    public var downUpdate: CardUpdate?

    public init(downUptime: TimeInterval, upUptime: TimeInterval, fingers: Int, movement: CGFloat,
                interrupted: Bool, location: CGPoint, downUpdate: CardUpdate?) {
        self.downUptime = downUptime; self.upUptime = upUptime; self.fingers = fingers; self.movement = movement
        self.interrupted = interrupted; self.location = location; self.downUpdate = downUpdate
    }

    /// R404: one finger, under 10 pt of movement and under 500 ms.
    public var isTap: Bool {
        fingers == 1 && !interrupted && movement < ReviewTapRules.maximumMovement
            && upUptime >= downUptime && upUptime - downUptime < ReviewTapRules.maximumDuration
    }
}

/// What the card page saw of the same gesture, after its own handlers (R405–R407).
public nonisolated struct ReviewTapObservation: Equatable, Sendable {
    public var update: CardUpdate
    public var exempt: Bool
    public var defaultPrevented: Bool
    public var selection: Bool
    public var editable: Bool
    public var composing: Bool

    public init(update: CardUpdate, exempt: Bool, defaultPrevented: Bool, selection: Bool, editable: Bool, composing: Bool) {
        self.update = update; self.exempt = exempt; self.defaultPrevented = defaultPrevented
        self.selection = selection; self.editable = editable; self.composing = composing
    }
}

/// The review at the moment of the decision.
public nonisolated struct ReviewTapState: Equatable, Sendable {
    public enum Side: Equatable, Sendable { case question, answer }
    /// The side that accepts input now; nil while nothing does (a transition,
    /// a pause, an open form or dialog, N4, N11.1, R395).
    public var side: Side?
    /// The update on screen now.
    public var update: CardUpdate?
    /// The card view's visible horizontal extent, in its points (R402).
    public var visibleX: ClosedRange<CGFloat>
    /// Double Tap Prevention (R416), in seconds; 0 turns it off.
    public var doubleTapPrevention: TimeInterval
    /// When the last answer was accepted (any input), and when the answer
    /// side now shown appeared, on the touches' clock.
    public var lastAnswerUptime: TimeInterval?
    public var answerShownUptime: TimeInterval?
    /// VoiceOver or Switch Control: the answer buttons are the way to answer (R184, R1981).
    public var assistiveTechnology: Bool

    public init(side: Side?, update: CardUpdate?, visibleX: ClosedRange<CGFloat>, doubleTapPrevention: TimeInterval,
                lastAnswerUptime: TimeInterval?, answerShownUptime: TimeInterval?, assistiveTechnology: Bool) {
        self.side = side; self.update = update; self.visibleX = visibleX; self.doubleTapPrevention = doubleTapPrevention
        self.lastAnswerUptime = lastAnswerUptime; self.answerShownUptime = answerShownUptime
        self.assistiveTechnology = assistiveTechnology
    }
}

public nonisolated enum ReviewTapRules {
    public static let maximumMovement: CGFloat = 10
    public static let maximumDuration: TimeInterval = 0.5
    /// R416's default. Settings › Review's row is not built yet, so this is the value.
    public static let defaultDoubleTapPrevention: TimeInterval = 0.25

    public static func decide(_ gesture: ReviewTapGesture, _ observation: ReviewTapObservation,
                              _ state: ReviewTapState) -> ReviewTapAction {
        guard gesture.isTap else { return .none(.notATap) }
        // R415: only the side that was on screen when the finger went down.
        guard let update = state.update, gesture.downUpdate == update, observation.update == update else {
            return .none(.otherCard)
        }
        guard let side = state.side, (side == .answer) == (update.side == .answer) else { return .none(.inactive) }
        // R405, R406: the card's own controls and handlers win.
        if observation.exempt { return .none(.exempt) }
        if observation.defaultPrevented { return .none(.cancelledByCard) }
        if observation.selection { return .none(.selection) }
        if observation.editable || observation.composing { return .dismissKeyboard }
        let window = max(0, state.doubleTapPrevention)
        switch side {
        case .question:
            if window > 0, let answered = state.lastAnswerUptime, gesture.downUptime - answered < window {
                return .none(.doubleTapPrevention)
            }
            return .reveal
        case .answer:
            if state.assistiveTechnology { return .none(.assistiveTechnology) }
            // The same window right after the back of the card appears, so
            // the second half of a quick double tap is ignored (decisions.md).
            if window > 0, let shown = state.answerShownUptime, gesture.downUptime - shown < window {
                return .none(.answerJustShown)
            }
            let middle = (state.visibleX.lowerBound + state.visibleX.upperBound) / 2
            return gesture.location.x < middle ? .again : .good
        }
    }
}

/// Joins a native gesture with the page's observation of the same gesture.
/// Either can arrive first; each waits at most `maximumWait` for the other,
/// and they pair only when the page saw the tap where the finger lifted.
public nonisolated struct ReviewTapPairing: Sendable {
    public static let maximumWait: TimeInterval = 1
    /// In points: the page's point and the touch's agree to well within a fingertip.
    public static let maximumDistance: CGFloat = 24
    private var gesture: (value: ReviewTapGesture, arrival: TimeInterval)?
    private var observation: (value: ReviewTapObservation, point: CGPoint, arrival: TimeInterval)?

    public init() {}

    public mutating func add(_ gesture: ReviewTapGesture, at arrival: TimeInterval) -> (ReviewTapGesture, ReviewTapObservation)? {
        self.gesture = (gesture, arrival)
        return pair(now: arrival)
    }

    /// `point`: where the page saw the gesture end, in the card view's points.
    public mutating func add(_ observation: ReviewTapObservation, point: CGPoint, at arrival: TimeInterval)
        -> (ReviewTapGesture, ReviewTapObservation)? {
        self.observation = (observation, point, arrival)
        return pair(now: arrival)
    }

    public mutating func clear() { gesture = nil; observation = nil }

    private mutating func pair(now: TimeInterval) -> (ReviewTapGesture, ReviewTapObservation)? {
        if let gesture, now - gesture.arrival > Self.maximumWait { self.gesture = nil }
        if let observation, now - observation.arrival > Self.maximumWait { self.observation = nil }
        guard let gesture, let observation else { return nil }
        self.gesture = nil; self.observation = nil
        let dx = gesture.value.location.x - observation.point.x, dy = gesture.value.location.y - observation.point.y
        guard (dx * dx + dy * dy).squareRoot() <= Self.maximumDistance else { return nil }
        return (gesture.value, observation.value)
    }
}
