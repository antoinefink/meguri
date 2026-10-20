import AVFoundation
import Foundation
#if !os(macOS)
import UIKit
#endif

/// Balanced across card surfaces. HTML playback is reported by each pair's
/// isolated adapter and participates in the same owner set.
public final class AudioSessionCoordinator: CardAudioSession {
    public static let shared = AudioSessionCoordinator()
    public nonisolated enum Event: Sendable, Equatable { case stop, pause, available }
    private var owners: Set<UUID> = []
    private var subscribers: [UUID: AsyncStream<Event>.Continuation] = [:]
    private var observers: [NSObjectProtocol] = []
    private let hardware = SessionHardware()
    private var revision: UInt64 = 0
    public init() {
        #if !os(macOS)
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: AVAudioSession.didBecomeInactiveNotification, object: nil, queue: nil) { [weak self] notification in
            guard let event = Self.deactivationEvent(notification) else { return }
            Task { @MainActor [weak self] in self?.emit(event) }
        })
        observers.append(center.addObserver(forName: AVAudioSession.resumptionRecommendationNotification, object: nil, queue: nil) { [weak self] notification in
            guard let context = notification.userInfo?[AVAudioSession.resumptionContextKey] as? AVAudioSession.ResumptionContext else { return }
            Task { @MainActor [weak self] in
                if let event = Self.resumptionEvent(recommendation: context.recommendation, isActive: UIApplication.shared.applicationState == .active) {
                    self?.emit(event)
                }
            }
        })
        observers.append(center.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: nil) { [weak self] notification in
            let lost = (notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? NSNumber)?.uintValue == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue
            if lost { Task { @MainActor [weak self] in self?.emit(.pause) } }
        })
        observers.append(center.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: nil) { [weak self] _ in
            Task { @MainActor [weak self] in self?.emit(.stop) }
        })
        observers.append(center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: nil) { [weak self] _ in
            Task { @MainActor [weak self] in self?.emit(.available) }
        })
        #endif
    }
    isolated deinit { for observer in observers { NotificationCenter.default.removeObserver(observer) } }

    #if !os(macOS)
    nonisolated static func deactivationEvent(_ notification: Notification) -> Event? {
        guard let context = notification.userInfo?[AVAudioSession.deactivationContextKey] as? AVAudioSession.DeactivationContext else { return nil }
        return deactivationEvent(source: context.source, hasInterruption: context.interruptionContext != nil,
                                 reason: context.interruptionContext?.reason)
    }

    // Typed facts keep policy testable without constructing system-only contexts.
    nonisolated static func deactivationEvent(source: AVAudioSession.DeactivationSource, hasInterruption: Bool,
                                              reason: AVAudioSession.InterruptionReason?) -> Event? {
        switch source {
        case .app: return nil // Releasing our last lease must not clear waiting playback.
        case .system:
            // System inactivity can occur without a subsequent recommendation.
            // Pause preserves position and explicit review eligibility.
            guard hasInterruption else { return .pause }
            return reason == .routeDisconnected ? .pause : .stop
        @unknown default: return .stop
        }
    }

    nonisolated static func resumptionEvent(recommendation: AVAudioSession.ResumptionRecommendation, isActive: Bool) -> Event? {
        guard isActive else { return nil }
        switch recommendation {
        // Availability restores presentation/manual actions, never playback.
        case .shouldResume, .shouldNotResume: return .available
        @unknown default: return nil
        }
    }
    #endif

    public func events() -> AsyncStream<Event> {
        let id = UUID()
        return AsyncStream(bufferingPolicy: .bufferingNewest(16)) { continuation in
            subscribers[id] = continuation
            continuation.onTermination = { [weak self] _ in Task { @MainActor in self?.subscribers[id] = nil } }
        }
    }
    func emit(_ event: Event) { for continuation in subscribers.values { continuation.yield(event) } }
    public func activate(_ owner: UUID) async throws {
        guard !owners.contains(owner) else { return }
        owners.insert(owner)
        revision += 1
        do { try await hardware.set(active: true, revision: revision) }
        catch {
            release(owner)
            throw error
        }
        guard owners.contains(owner) else { throw CancellationError() }
    }
    public func release(_ owner: UUID) {
        guard owners.remove(owner) != nil, owners.isEmpty else { return }
        revision += 1
        let hardware = self.hardware, revision = self.revision
        Task { try? await hardware.set(active: false, revision: revision) }
    }
}

/// Serialized native session calls can block on hardware; they never block
/// the UI actor. Revisions prevent a delayed release from disabling a newer
/// surface's lease, regardless of task scheduling order.
private actor SessionHardware {
    private var revision: UInt64 = 0
    private var active = false
    func set(active: Bool, revision: UInt64) throws {
        guard revision >= self.revision else { return }
        self.revision = revision
        guard self.active != active else { return }
        #if !os(macOS)
        if active {
            try AVAudioSession.sharedInstance().setCategory(.playback, options: [.duckOthers])
            try AVAudioSession.sharedInstance().setActive(true)
        } else {
            try AVAudioSession.sharedInstance().setActive(false, options: [.notifyOthersOnDeactivation])
        }
        #endif
        self.active = active
    }
}
