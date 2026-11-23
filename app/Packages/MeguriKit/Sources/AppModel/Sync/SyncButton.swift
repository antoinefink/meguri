import Foundation

/// N23.6: the sync status button in the toolbar. A tap syncs at once in the
/// ordinary states and opens the status panel only where a sync cannot
/// resolve the state; its feedback is the symbol itself.
public enum SyncButton {
    /// What a tap does.
    public enum Tap: Equatable, Sendable {
        /// Starts a sync now (Sync Now's own request); nothing opens.
        case syncNow
        /// Opens the status panel (N23.1): the state needs the person, sync is
        /// off or not set up, the state has its own action, or Sync Now
        /// cannot run.
        case openPanel
        /// A sync is already running: nothing happens, and no second one starts.
        case nothing
    }

    /// The first row that applies decides (N23.6).
    /// - `slot`: the panel's action place for this status (N23.3).
    /// - `syncNowAvailable`: the panel's Sync Now could run, the status moving aside.
    /// - `moving`: the status moves (the panel's bar shows, N23.1).
    public static func tap(_ status: SyncMainStatus, serviceSelected: Bool, slot: SyncPanel.Slot,
                           syncNowAvailable: Bool, moving: Bool) -> Tap {
        if status.needsYou || !serviceSelected { return .openPanel }
        // The state's own action (Cancel Setup, Try Again) is in the panel only.
        if case .action = slot { return .openPanel }
        if moving { return .nothing }
        if slot == .syncNow, syncNowAvailable { return .syncNow }
        return .openPanel
    }

    /// What the button shows and announces once a sync the person started from it ends.
    public enum Outcome: Equatable, Sendable {
        /// Up to date: the check shows in the accent colour for a moment
        /// (`accentDuration`), then in the default colour.
        case succeeded
        /// Any other state: its own symbol (needs you in orange), no accent.
        case ended
    }

    /// The symbol keeps the accent colour this long after a success.
    public static let accentDuration: Duration = .seconds(1)

    /// Follows one sync the person started from the button, from the tap to
    /// the end of the work it started. Presentation only.
    public struct Feedback: Equatable, Sendable {
        enum Phase: Equatable, Sendable { case idle, requested(Date), running(Date) }
        private(set) var phase: Phase = .idle
        /// The check is in the accent colour (after `Outcome.succeeded`).
        public private(set) var accented = false
        /// A request the status never shows moving for this long is dropped:
        /// the work was refused or ended before it showed.
        static let requestTimeout: TimeInterval = 20
        /// A sync followed for longer than this is let go without feedback: its
        /// end was not seen (the button was off screen when it ended).
        static let runningLimit: TimeInterval = 15 * 60
        /// Test hooks: the accent stays for a capture.
        private var held = false

        public init() {}

        /// The person tapped and a sync was requested.
        public mutating func requested(at now: Date) {
            phase = .requested(now)
            accented = false
        }

        /// Called whenever the status changes; gives the outcome when the
        /// sync the person started has ended.
        public mutating func observe(moving: Bool, upToDate: Bool, needsYou: Bool, now: Date) -> Outcome? {
            switch phase {
            case .idle:
                return nil
            case .requested(let at):
                // Expired first: a later round the person did not start must not
                // take over a request whose own work never showed.
                if now.timeIntervalSince(at) > Self.requestTimeout { phase = .idle; return nil }
                if moving { phase = .running(now); return nil }
                // A request refused at once (a sign-in that expired) ends in needs you.
                if needsYou { phase = .idle; return .ended }
                return nil
            case .running(let since):
                if now.timeIntervalSince(since) > Self.runningLimit { phase = .idle; return nil }
                guard !moving else { return nil }
                phase = .idle
                if upToDate, !needsYou {
                    accented = true
                    return .succeeded
                }
                return .ended
            }
        }

        /// A button appeared: when nothing moves, a sync it was following ended
        /// out of its sight, so it is let go without feedback.
        public mutating func appeared(moving: Bool) {
            if !moving, phase != .idle { phase = .idle }
        }

        /// The moment after a success is over.
        public mutating func endAccent() { if !held { accented = false } }

        /// Test hooks: the moment after a success, held for a capture.
        @_spi(MeguriTestHooks) public mutating func holdAccentForTestHooks() { accented = true; held = true }
    }
}
