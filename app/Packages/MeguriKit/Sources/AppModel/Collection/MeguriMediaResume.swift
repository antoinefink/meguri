import CoreBridge
import Foundation

/// When setup's media step, stopped before it finished, continues by itself
/// (`sync/meguri-provider` MP6.5, changed 2026-11-21 (resume fix)). It holds
/// no authority: the core decides what may run, on the same operation, and
/// the coordinator runs it.
struct MeguriMediaResume: Equatable, Sendable {
    enum Trigger: Equatable, Sendable {
        /// The provider attached at launch, the app came back to the foreground,
        /// the network came back, the blocked phase cleared, or a wait ended.
        case launch, foreground, network, recovered, retry
        /// Try Again: always admitted, and the automatic attempts start again.
        case user
        var automatic: Bool { self != .user }
    }
    enum Outcome: Equatable, Sendable {
        /// Setup finished: the binding is ready.
        case ready
        /// The step ended before it finished and can run again: a quit, a lost
        /// connection, a server error, a busy core.
        case unfinished
        /// The person must act before it runs again (free space on this device),
        /// or a refusal a later attempt is not known to clear: Try Again.
        case stopped
        /// Listed files the person must deal with (Affected Files) block completion (MP5).
        case blocked
        /// The account, the connection or the sign-in changed since setup began:
        /// setup shows its own row, whose action is Sign In (T19.5).
        case failed
        /// Nothing ran: the provider needs the person, the service is paused,
        /// or the work is no longer this connection's.
        case notRun
    }
    /// Automatic attempts in a row that end unfinished before Try Again is needed.
    static let automaticAttempts = 3
    /// The wait before the next automatic attempt, after the first and the second.
    static let delays: [Duration] = [.seconds(30), .seconds(120)]

    private(set) var failures = 0
    private(set) var blocked = false

    /// Automatic attempts are used up, or something only the person can clear blocks the step.
    var stopped: Bool { blocked || failures >= Self.automaticAttempts }
    func admits(_ trigger: Trigger) -> Bool { !trigger.automatic || !stopped }
    mutating func started(_ trigger: Trigger) {
        if !trigger.automatic { failures = 0; blocked = false }
    }
    /// Records how an attempt ended. Returns the wait before the next automatic
    /// attempt, when one follows.
    mutating func finished(_ outcome: Outcome) -> Duration? {
        switch outcome {
        case .ready:
            failures = 0; blocked = false
            return nil
        case .stopped, .blocked:
            blocked = true
            return nil
        // A new sign-in, which setup's row asks for, lets the step continue by itself again.
        case .failed, .notRun:
            return nil
        case .unfinished:
            failures += 1
            guard failures < Self.automaticAttempts else { return nil }
            return Self.delays[min(failures, Self.delays.count) - 1]
        }
    }

    /// A retained operation whose collection step is done and whose media step
    /// has not finished setup: the one step a resume may continue. Its
    /// acceptance and activation guards stay the core's (MP4).
    static func continues(_ receipt: MeguriBootstrapReceipt) -> Bool {
        switch receipt.phase {
        case .collectionComplete, .mediaIncomplete, .complete: true
        default: false
        }
    }
    /// What a media step's refusal means for the next attempt.
    static func outcome(refused failure: MeguriBootstrapFailure, accounting: MeguriBootstrapMediaAccounting?) -> Outcome {
        if accounting != nil { return .blocked }
        switch failure {
        case .busy, .cancelled, .unavailable, .mediaIncomplete: return .unfinished
        // Identity: setup's sign-in row (T19.5).
        case .accountChanged, .connectionChanged, .username, .notSelected: return .failed
        case .transfer(let transfer):
            if transfer.issue == .server, transfer.serverKind == "auth_failed" { return .failed }
            if transfer.boundary == .identityChanged { return .failed }
            switch transfer.issue {
            case .busy, .cancelled, .interrupted, .sessionActive, .unavailable, .server, .cooldown, .mediaIncomplete:
                return .unfinished
            default: return .stopped
            }
        // Disk full, storage, and any refusal an automatic attempt is not known
        // to clear: stopped with Try Again, never a setup failure with no action.
        default: return .stopped
        }
    }
}
