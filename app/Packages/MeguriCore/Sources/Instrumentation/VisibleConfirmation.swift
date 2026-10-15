import Foundation

/// "Visible" for the review loop (R1862), read per KTD11's conflict
/// call-out. `review.answerToNextVisible` runs from the answer input to the
/// display frame that presents the swap commit, and ends there. The page
/// then confirms with a double requestAnimationFrame after the swap, which
/// must name the card the swap showed: it proves the content painted and is
/// the right card, but does not end the interval. A sample whose
/// confirmation names another card, or never comes, is a miss.
///
/// This file is the contract; the card runtime (U66) posts the messages and
/// CardRendering (U67) and the review pipeline (U176) drive a
/// `SwapRecorder`:
///
/// 1. On answer input, `beginSwap(to:inputTime:)` begins the interval and
///    returns the swap's token; the token goes to the page with the swap.
/// 2. The frame probe calls `presented(_:frameTime:)` from the display link
///    callback of the frame that presents the swap commit: the ending API.
/// 3. The front page, two animation frames after the swap, posts
///    `{token, card, phase: "visible"}` to the `messageName` handler from
///    Meguri's isolated content world; the handler passes the body to
///    `receive(_:at:)`. The occluded page's pre-swap double rAF posts the
///    same shape with phase "settled" (U176's settle signal).
public enum VisibleConfirmation {
    /// The script message handler name the card runtime posts to.
    public static let messageName = "meguriVisible"

    /// A message from the card runtime.
    public struct Message: Sendable, Equatable {
        public enum Phase: String, Sendable {
            /// Pre-swap: the occluded page has painted the next card.
            case settled
            /// Post-swap: the front page shows the card after the swap.
            case visible
        }

        /// The swap's `SwapToken.sequence`.
        public let token: UInt64
        /// The card id the page is showing.
        public let cardID: Int64
        public let phase: Phase

        public init(token: UInt64, cardID: Int64, phase: Phase) {
            self.token = token
            self.cardID = cardID
            self.phase = phase
        }

        /// Reads a script message body: a dictionary with the integers
        /// `token` (≥ 0) and `card`, and `phase` "settled" or "visible".
        /// Anything else is not a confirmation (nil).
        public init?(body: Any) {
            guard let dictionary = body as? [String: Any],
                let token = Self.integer(dictionary["token"]), token >= 0,
                let card = Self.integer(dictionary["card"]),
                let phase = (dictionary["phase"] as? String).flatMap(Phase.init(rawValue:))
            else { return nil }
            self.init(token: UInt64(token), cardID: card, phase: phase)
        }

        /// The body the card runtime posts.
        public var body: [String: Any] {
            ["token": token, "card": cardID, "phase": phase.rawValue]
        }

        /// A JavaScript number that is an exact integer (card ids are below
        /// 2^53, so they survive the page's doubles).
        private static func integer(_ value: Any?) -> Int64? {
            switch value {
            case let value as Int: Int64(value)
            case let value as Int64: value
            case let value as UInt64: Int64(exactly: value)
            case let value as Double: Int64(exactly: value)
            case let value as NSNumber: Int64(exactly: value.doubleValue)
            default: nil
            }
        }
    }
}

/// One swap of the review page pair.
public struct SwapToken: Sendable, Hashable {
    /// Increases by one per swap of a recorder.
    public let sequence: UInt64
    /// The card the swap shows.
    public let cardID: Int64
}

/// One measured swap.
public struct VisibleSample: Sendable, Equatable {
    public enum Outcome: Sendable, Equatable {
        /// The post-swap confirmation named the swap's card.
        case confirmed
        /// It named another card: a miss.
        case wrongCard(reported: Int64)
        /// No confirmation came before the next swap: a miss.
        case unconfirmed
    }

    public let token: SwapToken
    /// When the input was delivered (the event's timestamp), in seconds on
    /// the clock the frame times use.
    public let inputTime: TimeInterval
    /// When the frame that presents the swap commit was shown: where the
    /// interval ends.
    public let presentedTime: TimeInterval
    /// When the post-swap confirmation arrived, if it did.
    public let confirmedTime: TimeInterval?
    public let outcome: Outcome

    public var isMiss: Bool { outcome != .confirmed }
    /// The interval: input to the presented frame.
    public var inputToPresented: TimeInterval { presentedTime - inputTime }
}

/// How a recorder begins and ends its interval; `signposts` in the app,
/// a recording one in tests.
public struct IntervalEmitter: Sendable {
    public var begin: @Sendable (IntervalName) -> IntervalToken
    public var end: @Sendable (IntervalToken) -> Void

    public init(begin: @escaping @Sendable (IntervalName) -> IntervalToken, end: @escaping @Sendable (IntervalToken) -> Void) {
        self.begin = begin
        self.end = end
    }

    public static let signposts = IntervalEmitter(begin: Signposts.begin, end: Signposts.end)
}

/// Pairs each swap's presented frame with its post-swap confirmation and
/// reports one `VisibleSample` per swap.
@MainActor
public final class SwapRecorder {
    private struct Pending {
        let token: SwapToken
        let inputTime: TimeInterval
        var interval: IntervalToken?
        var presentedTime: TimeInterval?
        var confirmation: (cardID: Int64, time: TimeInterval)?
    }

    private let interval: IntervalName
    private let emitter: IntervalEmitter
    private let onSample: @MainActor (VisibleSample) -> Void
    private var next: UInt64 = 1
    private var pending: [UInt64: Pending] = [:]

    public init(
        interval: IntervalName = IntervalNames.answerToNextVisible,
        emitter: IntervalEmitter = .signposts,
        onSample: @escaping @MainActor (VisibleSample) -> Void
    ) {
        self.interval = interval
        self.emitter = emitter
        self.onSample = onSample
    }

    /// The input for a swap to `cardID` was delivered at `inputTime`: begins
    /// the interval and returns the token the page must echo. A presented
    /// swap still waiting for its confirmation becomes an unconfirmed miss.
    public func beginSwap(to cardID: Int64, inputTime: TimeInterval) -> SwapToken {
        for (sequence, swap) in pending where swap.presentedTime != nil {
            pending[sequence] = nil
            report(swap, outcome: .unconfirmed)
        }
        let token = SwapToken(sequence: next, cardID: cardID)
        next += 1
        pending[token.sequence] = Pending(token: token, inputTime: inputTime, interval: emitter.begin(interval))
        return token
    }

    /// The ending API: the display frame that presents `token`'s swap
    /// commit was shown at `frameTime`. Ends the interval.
    public func presented(_ token: SwapToken, frameTime: TimeInterval) {
        guard var swap = pending[token.sequence], swap.presentedTime == nil else { return }
        if let begun = swap.interval {
            emitter.end(begun)
            swap.interval = nil
        }
        swap.presentedTime = frameTime
        pending[token.sequence] = swap
        completeIfReady(token.sequence)
    }

    /// A message from the card runtime, received at `time`. A post-swap
    /// confirmation completes its swap's sample; a pre-swap one is only
    /// parsed (U176 consumes it). Returns the message, or nil when `body`
    /// is not one.
    @discardableResult
    public func receive(_ body: Any, at time: TimeInterval) -> VisibleConfirmation.Message? {
        guard let message = VisibleConfirmation.Message(body: body) else { return nil }
        if message.phase == .visible, var swap = pending[message.token], swap.confirmation == nil {
            swap.confirmation = (message.cardID, time)
            pending[message.token] = swap
            completeIfReady(message.token)
        }
        return message
    }

    private func completeIfReady(_ sequence: UInt64) {
        guard let swap = pending[sequence], swap.presentedTime != nil, let confirmation = swap.confirmation else { return }
        pending[sequence] = nil
        report(swap, outcome: confirmation.cardID == swap.token.cardID ? .confirmed : .wrongCard(reported: confirmation.cardID))
    }

    private func report(_ swap: Pending, outcome: VisibleSample.Outcome) {
        guard let presentedTime = swap.presentedTime else { return }
        onSample(
            VisibleSample(
                token: swap.token,
                inputTime: swap.inputTime,
                presentedTime: presentedTime,
                confirmedTime: swap.confirmation?.time,
                outcome: outcome
            ))
    }
}
