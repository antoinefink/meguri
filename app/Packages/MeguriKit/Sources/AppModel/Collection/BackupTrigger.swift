import CoreBridge
import Foundation

/// Core owns due-ness. This owner only keeps its single wall-clock deadline.
@MainActor
public final class BackupTrigger {
    public private(set) var deadlineMilliseconds: Int64?
    private var generation: UInt64?
    private var epoch: UInt64 = 0
    private var revision: UInt64 = 0
    private var timerEnabled = true
    private var cancelTimer: (() -> Void)?
    private var task: Task<Void, Never>?
    private var taskID: UUID?
    private var operation: MaintenanceOperation?
    private let service: any MaintenanceService
    private let clock: AppClock
    private let usesInjectedSchedule: Bool
    private let schedule: (Date, @escaping @MainActor () -> Void) -> (() -> Void)
    public var didRun: (@MainActor () async -> Void)?

    public init(service: any MaintenanceService, clock: AppClock = .system,
                schedule: ((Date, @escaping @MainActor () -> Void) -> (() -> Void))? = nil) {
        self.service = service
        self.clock = clock; usesInjectedSchedule = schedule != nil
        self.schedule = schedule ?? { date, fire in
            let timer = Timer(fire: date, interval: 0, repeats: false) { _ in
                Task { @MainActor in fire() }
            }
            RunLoop.main.add(timer, forMode: .common)
            return { timer.invalidate() }
        }
    }

    public func own(_ generation: UInt64?) {
        guard self.generation != generation else { return }
        self.generation = generation; epoch &+= 1
        event(deadline: nil)
        task?.cancel()
        if let operation { Task { await operation.cancel() } }
    }

    public func event(deadline: Int64?) {
        revision &+= 1
        cancelTimer?(); cancelTimer = nil
        deadlineMilliseconds = generation == nil ? nil : deadline
        guard timerEnabled, let deadline = deadlineMilliseconds else { return }
        if clock.isFixed && !usesInjectedSchedule { return }
        let expected = epoch, expectedRevision = revision
        cancelTimer = schedule(Date(timeIntervalSince1970: Double(deadline) / 1_000)) { [weak self] in
            guard let self, self.epoch == expected, self.revision == expectedRevision else { return }
            self.cancelTimer = nil; self.deadlineMilliseconds = nil
            Task { await self.run(.deadline) }
        }
    }

    /// Background and quit callers join this before suspend/close. A returned
    /// deadline cannot overwrite a newer event or a successor collection's timer.
    public func run(_ trigger: MaintenanceTrigger) async {
        guard generation != nil else { return }
        let expected = epoch, previous = task, id = UUID()
        let next = Task { [weak self] in
            await previous?.value
            guard let self, self.epoch == expected, self.generation != nil, !Task.isCancelled else { return }
            guard trigger != .deadline || self.timerEnabled else { return }
            let began = self.revision, operation = MaintenanceOperation()
            self.operation = operation
            defer { self.operation = nil }
            do {
                let deadline = try await self.service.runDueMaintenance(trigger, operation: operation, progress: { _ in })
                guard self.epoch == expected, !Task.isCancelled else { return }
                if self.revision == began { self.event(deadline: deadline) }
                await self.didRun?()
            } catch {
                // Core records periodic failures and owns the next deadline.
                // An unavailable call must not leave an expired timer spinning.
                if self.epoch == expected, self.revision == began { self.event(deadline: nil) }
            }
        }
        task = next; taskID = id
        await next.value
        if taskID == id { task = nil; taskID = nil }
    }

    public func pauseTimer() { event(deadline: nil) }
    public func setTimerEnabled(_ enabled: Bool) {
        timerEnabled = enabled
        event(deadline: deadlineMilliseconds)
    }
    public func drain() async { await task?.value }
}
