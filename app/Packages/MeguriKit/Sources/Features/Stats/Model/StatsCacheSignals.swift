import CoreBridge
import Dispatch
import Foundation

/// The two calls the bridge leaves to the app (`stats/graphs-common` G9.2, G9.5, H14.4):
/// `homeIdle()` once Home has been idle for 2 s after each open, so the column caches build in
/// the background off the launch path, and `releaseCaches()` on a memory warning. One per app;
/// the shell reports Home's idleness and the opens.
@MainActor
public final class StatsCacheSignals {
    private let service: any StatsService
    private let timers: any StatsTimers
    private var pressure: (any DispatchSourceMemoryPressure)?
    private var waiting: StatsTimerToken?
    /// An open is current and its signal has not been sent.
    private var due = false
    private var idle = false
    private(set) var releasing = false
    /// Runs after each memory warning (the kept results trim to the shown scope).
    public var onMemoryWarning: [@MainActor () -> Void] = []

    static let idleDelay = Duration.seconds(2)

    /// - Parameter watchesMemory: listen to the system's memory pressure (off in tests).
    public init(service: any StatsService = CoreStatsService(), timers: any StatsTimers = StatsSystemTimers(),
                watchesMemory: Bool = true) {
        self.service = service
        self.timers = timers
        if watchesMemory {
            let source = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
            source.setEventHandler { [weak self] in
                MainActor.assumeIsolated { self?.memoryWarning() }
            }
            source.resume()
            pressure = source
        }
    }

    isolated deinit {
        pressure?.cancel()
    }

    /// A collection finished opening: Home's idle time counts again, once for this open.
    public func collectionOpened() {
        due = true
        waiting?.cancel()
        waiting = nil
        if idle { wait() }
    }

    /// No collection is open: nothing is signalled until the next open.
    public func collectionClosed() {
        due = false
        waiting?.cancel()
        waiting = nil
    }

    /// Whether Home is on screen with nothing over it (no review, no sheet) and the app active.
    public func homeIdle(_ isIdle: Bool) {
        guard isIdle != idle else { return }
        idle = isIdle
        if isIdle {
            if due { wait() }
        } else {
            waiting?.cancel()
            waiting = nil
        }
    }

    private func wait() {
        waiting?.cancel()
        waiting = timers.schedule(after: Self.idleDelay) { [weak self] in
            guard let self, self.due, self.idle else { return }
            self.due = false
            self.waiting = nil
            let service = self.service
            Task.detached { try? await service.homeIdle() }
        }
    }

    /// The system's memory warning: the core releases its caches (persisted first) off the
    /// main actor; a warning while a release runs adds nothing.
    public func memoryWarning() {
        for handler in onMemoryWarning { handler() }
        guard !releasing else { return }
        releasing = true
        let service = service
        Task { [weak self] in
            try? await service.releaseCaches()
            self?.releasing = false
        }
    }
}
