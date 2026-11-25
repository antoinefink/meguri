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
    private let hardware: SessionHardware
    private let continuityEligible: () -> Bool
    private var continuityOwners: Set<UUID> = []
    private var listeners: [UUID: (Event) -> Void] = [:]
    private var revision: UInt64 = 0
    private var hardwareUpdate: Task<Void, Never>?
    public convenience init() {
        self.init(hardware: SessionHardware(), continuityEligible: {
            #if os(iOS)
            UIApplication.shared.applicationState == .active && AVAudioSession.sharedInstance().currentRoute.outputs.contains {
                [.bluetoothA2DP, .bluetoothHFP, .bluetoothLE].contains($0.portType)
            }
            #else
            false
            #endif
        })
    }
    init(hardware: SessionHardware, continuityEligible: @escaping () -> Bool) {
        self.hardware = hardware
        self.continuityEligible = continuityEligible
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
            Task { @MainActor [weak self] in
                guard let self else { return }
                if lost || (!self.continuityOwners.isEmpty && !self.continuityEligible()) { self.emit(.pause) }
            }
        })
        observers.append(center.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: nil) { [weak self] _ in
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
    // Native admission is revoked in this turn, before any pending load or
    // activation continuation can run. Page cleanup can remain asynchronous.
    func observe(_ listener: @escaping (Event) -> Void) -> UUID {
        let id = UUID(); listeners[id] = listener; return id
    }
    func removeObserver(_ id: UUID) { listeners[id] = nil }
    func emit(_ event: Event) {
        if event != .available {
            owners.removeAll(); continuityOwners.removeAll()
            updateHardware()
        }
        for listener in Array(listeners.values) { listener(event) }
        for continuation in subscribers.values { continuation.yield(event) }
    }
    public func maintainContinuity(_ owner: UUID) {
        guard continuityEligible(), continuityOwners.insert(owner).inserted else { return }
        updateHardware()
    }
    public func releaseContinuity(_ owner: UUID) {
        guard continuityOwners.remove(owner) != nil else { return }
        updateHardware()
    }
    private func updateHardware() {
        revision += 1
        let hardware = self.hardware, revision = self.revision
        let audible = !owners.isEmpty, continuity = !continuityOwners.isEmpty
        hardwareUpdate = Task { try? await hardware.set(audible: audible, continuity: continuity, revision: revision) }
    }
    func hardwareSettled() async {
        await hardwareUpdate?.value
        await hardware.settled()
    }
    public func activate(_ owner: UUID) async throws {
        guard !owners.contains(owner) else { return }
        owners.insert(owner)
        revision += 1
        do { try await hardware.set(audible: true, continuity: !continuityOwners.isEmpty, revision: revision) }
        catch {
            release(owner)
            throw error
        }
        guard owners.contains(owner) else { throw CancellationError() }
    }
    public func release(_ owner: UUID) {
        guard owners.remove(owner) != nil else { return }
        updateHardware()
    }
}

/// Serialized native session calls can block on hardware; they never block
/// the UI actor. Revisions prevent a delayed release from disabling a newer
/// surface's lease, regardless of task scheduling order.
actor SessionHardware {
    nonisolated enum Operation: Sendable, Equatable {
        case category(ducking: Bool), activate, deactivate, startSilence, stopSilence
    }
    // Tests replace only physical effects, retaining the real state machine.
    private let performOverride: (@Sendable (Operation) throws -> Void)?
    private var revision: UInt64 = 0
    private var active = false
    private var ducking: Bool?
    private var silenceRequested = false
    private var silenceFailed = false
    private var audible = false
    private let output: SilentOutput
    private var outputWork: Task<Void, Never>?
    init(perform: (@Sendable (Operation) throws -> Void)? = nil) {
        performOverride = perform
        output = SilentOutput(perform: perform)
    }

    func set(audible: Bool, continuity: Bool, revision: UInt64) async throws {
        guard revision >= self.revision else { return }
        self.revision = revision; self.audible = audible
        if !continuity { silenceFailed = false }
        let needsSilence = continuity && !silenceFailed
        if !needsSilence { requestSilence(false, revision: revision) }
        do {
            if audible || needsSilence {
                if ducking != audible {
                    try perform(.category(ducking: audible))
                    ducking = audible
                }
                if !active {
                    // Activation can partially succeed before throwing.
                    active = true
                    do { try perform(.activate) }
                    catch { try? deactivate(); throw error }
                }
                if needsSilence { requestSilence(true, revision: revision) }
            } else {
                await outputWork?.value
                guard revision == self.revision else { return }
                try deactivate()
            }
        } catch {
            if !audible && needsSilence {
                requestSilence(false, revision: revision)
                await outputWork?.value
                if revision == self.revision { try? deactivate() }
            }
            throw error
        }
    }
    private func requestSilence(_ enabled: Bool, revision: UInt64) {
        guard silenceRequested != enabled else { return }
        silenceRequested = enabled; outputRevision = revision
        outputWork?.cancel()
        let output = self.output
        // Engine startup must never be a prerequisite of a normal word's
        // activation. Its own serial worker also owns delayed-start cleanup.
        outputWork = Task {
            do { try await output.set(enabled: enabled, revision: revision) }
            catch { self.outputFailed(revision: revision) }
        }
    }
    private func outputFailed(revision: UInt64) {
        // A newer off/on request owns its own failure result.
        guard silenceRequested, revision == outputRevision else { return }
        silenceFailed = true
        silenceRequested = false
        if !audible { try? deactivate() }
    }
    private var outputRevision: UInt64 = 0
    func settled() async { await outputWork?.value }
    private func deactivate() throws {
        guard active else { return }
        try perform(.deactivate)
        active = false
    }
    private func perform(_ operation: Operation) throws {
        if let performOverride { try performOverride(operation); return }
        #if os(iOS)
        let session = AVAudioSession.sharedInstance()
        switch operation {
        case .category(let ducking):
            try session.setCategory(.playback, options: ducking ? [.duckOthers] : [.mixWithOthers])
        case .activate: try session.setActive(true)
        case .deactivate: try session.setActive(false, options: [.notifyOthersOnDeactivation])
        case .startSilence, .stopSilence: break // Owned by SilentOutput.
        }
        #endif
    }
}

/// Engine calls can block independently of AVAudioSession activation. This
/// worker has no microphone graph and cannot start or advance native words.
private actor SilentOutput {
    private let performOverride: (@Sendable (SessionHardware.Operation) throws -> Void)?
    private var revision: UInt64 = 0
    #if os(iOS)
    private var engine: AVAudioEngine?
    private var player: AVAudioPlayerNode?
    #endif
    init(perform: (@Sendable (SessionHardware.Operation) throws -> Void)?) { performOverride = perform }
    func set(enabled: Bool, revision: UInt64) throws {
        guard revision >= self.revision else { return }
        self.revision = revision
        if !enabled { stop(); return }
        do {
            try Task.checkCancellation()
            if let performOverride {
                try performOverride(.startSilence)
                try Task.checkCancellation()
                return
            }
            #if os(iOS)
            let engine = AVAudioEngine(), player = AVAudioPlayerNode()
            // Schedule real zero PCM indefinitely; inputNode is never accessed.
            guard let format = AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 2),
                  let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 1_024),
                  let channels = buffer.floatChannelData else { throw SilenceFailure.buffer }
            buffer.frameLength = buffer.frameCapacity
            for channel in 0..<Int(format.channelCount) {
                channels[channel].initialize(repeating: 0, count: Int(buffer.frameLength))
            }
            self.engine = engine; self.player = player
            engine.attach(player)
            engine.connect(player, to: engine.mainMixerNode, format: format)
            player.scheduleBuffer(buffer, at: nil, options: .loops)
            try Task.checkCancellation()
            try engine.start()
            try Task.checkCancellation()
            player.play()
            #endif
        } catch { stop(); throw error }
    }
    private func stop() {
        if let performOverride { try? performOverride(.stopSilence); return }
        #if os(iOS)
        player?.stop(); engine?.stop()
        player = nil; engine = nil
        #endif
    }
    private nonisolated enum SilenceFailure: Error { case buffer }
}
