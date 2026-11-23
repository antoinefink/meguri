import Foundation

/// The model's clock and delays (S10.4's minute, S11's half second), a seam so tests run them
/// by hand.
@MainActor
public protocol StatsTimers: AnyObject {
    func now() -> Date
    /// Runs `action` on the main actor after `delay`, unless the token is cancelled first.
    func schedule(after delay: Duration, _ action: @escaping @MainActor () -> Void) -> StatsTimerToken
}

/// A scheduled action; `cancel()` stops it.
@MainActor
public final class StatsTimerToken {
    private var stop: (() -> Void)?

    public init(cancel: @escaping () -> Void) {
        stop = cancel
    }

    public func cancel() {
        stop?()
        stop = nil
    }
}

/// The system clock and `Task.sleep`.
@MainActor
public final class StatsSystemTimers: StatsTimers {
    public init() {}

    public func now() -> Date { Date() }

    public func schedule(after delay: Duration, _ action: @escaping @MainActor () -> Void) -> StatsTimerToken {
        let task = Task { @MainActor in
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            action()
        }
        return StatsTimerToken { task.cancel() }
    }
}
