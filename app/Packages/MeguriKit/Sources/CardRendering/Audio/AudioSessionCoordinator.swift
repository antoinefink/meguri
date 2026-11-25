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
    private let routeIdentity: () -> String?
    private var continuityRoute: String?
    private var continuityOwners: Set<UUID> = []
    private var listeners: [UUID: (Event) -> Void] = [:]
    private var revision: UInt64 = 0
    private var hardwareUpdate: Task<Void, Never>?
    private var configurationWork: Task<Void, Never>?
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
    init(hardware: SessionHardware, continuityEligible: @escaping () -> Bool,
         routeIdentity: @escaping () -> String? = { AudioSessionCoordinator.selectedBluetoothRoute() }) {
        self.hardware = hardware
        self.continuityEligible = continuityEligible
        self.routeIdentity = routeIdentity
        #if !os(macOS)
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: AVAudioSession.didBecomeInactiveNotification, object: nil, queue: .main) { [weak self] notification in
            guard let event = Self.deactivationEvent(notification) else { return }
            MainActor.assumeIsolated { self?.emit(event) }
        })
        observers.append(center.addObserver(forName: AVAudioSession.resumptionRecommendationNotification, object: nil, queue: nil) { [weak self] notification in
            guard let context = notification.userInfo?[AVAudioSession.resumptionContextKey] as? AVAudioSession.ResumptionContext else { return }
            Task { @MainActor [weak self] in
                if let event = Self.resumptionEvent(recommendation: context.recommendation, isActive: UIApplication.shared.applicationState == .active) {
                    self?.emit(event)
                }
            }
        })
        observers.append(center.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] notification in
            let lost = (notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? NSNumber)?.uintValue == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue
            let previous = (notification.userInfo?[AVAudioSessionRouteChangePreviousRouteKey] as? AVAudioSessionRouteDescription).flatMap(Self.bluetoothRoute)
            MainActor.assumeIsolated {
                self?.routeChanged(previous: previous, disconnected: lost)
            }
        })
        observers.append(center.addObserver(forName: .AVAudioEngineConfigurationChange, object: nil, queue: .main) { [weak self] notification in
            guard let engine = notification.object as? AVAudioEngine else { return }
            let engineID = ObjectIdentifier(engine)
            MainActor.assumeIsolated {
                self?.configurationChanged(engine: engineID)
            }
        })
        observers.append(center.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.emit(.stop) }
        })
        observers.append(center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: nil) { [weak self] _ in
            Task { @MainActor [weak self] in self?.emit(.available) }
        })
        #endif
    }
    nonisolated static func selectedBluetoothRoute() -> String? {
        #if os(iOS)
        bluetoothRoute(AVAudioSession.sharedInstance().currentRoute)
        #else
        nil
        #endif
    }
    #if os(iOS)
    nonisolated private static func bluetoothRoute(_ route: AVAudioSessionRouteDescription) -> String? {
        let ports = route.outputs.filter { [.bluetoothA2DP, .bluetoothHFP, .bluetoothLE].contains($0.portType) }
        guard !ports.isEmpty else { return nil }
        return ports.map { $0.portType.rawValue + ":" + $0.uid }.sorted().joined(separator: "|")
    }
    #endif
    // Category changes on the selected device are ordinary. A different UID,
    // or loss of the selected previous route, revokes every pending audio intent.
    func routeChanged(previous: String?, disconnected: Bool) {
        let selected = routeIdentity()
        if disconnected && (continuityRoute == nil || previous == nil || previous == continuityRoute)
            || (!continuityOwners.isEmpty && (!continuityEligible() || selected != continuityRoute)) {
            emit(.pause)
        }
    }
    func configurationChanged(engine: ObjectIdentifier) {
        guard configurationWork == nil, !continuityOwners.isEmpty, continuityEligible(), routeIdentity() == continuityRoute else { return }
        let hardware = hardware, revision = revision, route = continuityRoute
        configurationWork = Task { [weak self] in
            await hardware.configurationChanged(engine: engine, route: route, revision: revision)
            self?.configurationWork = nil
        }
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
            configurationWork?.cancel()
            owners.removeAll(); continuityOwners.removeAll(); continuityRoute = nil
            updateHardware()
        }
        for listener in Array(listeners.values) { listener(event) }
        for continuation in subscribers.values { continuation.yield(event) }
    }
    public func maintainContinuity(_ owner: UUID) {
        guard continuityEligible(), continuityOwners.insert(owner).inserted else { return }
        continuityRoute = routeIdentity()
        updateHardware()
    }
    public func releaseContinuity(_ owner: UUID) {
        guard continuityOwners.remove(owner) != nil else { return }
        if continuityOwners.isEmpty { continuityRoute = nil; configurationWork?.cancel() }
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
        await configurationWork?.value
        await hardware.settled()
    }
    public func activate(_ owner: UUID) async throws {
        guard !owners.contains(owner) else { return }
        let route = routeIdentity()
        owners.insert(owner)
        revision += 1
        do { try await hardware.set(audible: true, continuity: !continuityOwners.isEmpty, revision: revision) }
        catch {
            release(owner)
            throw error
        }
        guard owners.contains(owner) else { throw CancellationError() }
        // Hardware activation may outlast disconnection and its notification.
        // Never let that late return submit the first word on a replacement route.
        if let route, routeIdentity() != route {
            emit(.pause)
            throw CancellationError()
        }
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
    private var recoveryWork: Task<Void, Never>?
    init(perform: (@Sendable (Operation) throws -> Void)? = nil,
         outputIdentity: ObjectIdentifier? = nil, isRendering: (@Sendable () -> Bool)? = nil) {
        performOverride = perform
        output = SilentOutput(perform: perform, identity: outputIdentity, isRendering: isRendering)
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
        recoveryWork?.cancel()
        let output = self.output
        // Engine startup must never be a prerequisite of a normal word's
        // activation. Its own serial worker also owns delayed-start cleanup.
        outputWork = Task {
            do { try await output.set(enabled: enabled, revision: revision) }
            catch { self.outputFailed(revision: revision) }
        }
    }
    func configurationChanged(engine: ObjectIdentifier, route: String?, revision: UInt64) async {
        guard !Task.isCancelled, revision >= self.revision, silenceRequested, !silenceFailed, recoveryWork == nil else { return }
        let output = output, outputRevision = outputRevision
        let recovery = Task {
            do { try await output.recover(engine: engine, route: route, revision: outputRevision) }
            catch { self.outputFailed(revision: outputRevision) }
        }
        recoveryWork = recovery
        await withTaskCancellationHandler { await recovery.value } onCancel: { recovery.cancel() }
        recoveryWork = nil
    }
    private func outputFailed(revision: UInt64) {
        // A newer off/on request owns its own failure result.
        guard silenceRequested, revision == outputRevision else { return }
        silenceFailed = true
        silenceRequested = false
        if !audible { try? deactivate() }
    }
    private var outputRevision: UInt64 = 0
    func settled() async { await outputWork?.value; await recoveryWork?.value }
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
    private var enabled = false
    private let identityOverride: ObjectIdentifier?
    private let isRenderingOverride: (@Sendable () -> Bool)?
    #if os(iOS)
    private var engine: AVAudioEngine?
    private var player: AVAudioPlayerNode?
    #endif
    init(perform: (@Sendable (SessionHardware.Operation) throws -> Void)?, identity: ObjectIdentifier?,
         isRendering: (@Sendable () -> Bool)?) {
        performOverride = perform; identityOverride = identity; isRenderingOverride = isRendering
    }
    func recover(engine identity: ObjectIdentifier, route: String?, revision: UInt64) throws {
        guard enabled, revision == self.revision else { return }
        if performOverride != nil {
            guard identity == identityOverride, isRenderingOverride?() == false else { return }
        } else {
            #if os(iOS)
            guard route != nil, AudioSessionCoordinator.selectedBluetoothRoute() == route,
                  let engine, let player, ObjectIdentifier(engine) == identity,
                  !engine.isRunning || !player.isPlaying else { return }
            #else
            return
            #endif
        }
        try Task.checkCancellation()
        stop()
        try set(enabled: true, revision: revision)
    }
    func set(enabled: Bool, revision: UInt64) throws {
        guard revision >= self.revision else { return }
        self.revision = revision; self.enabled = enabled
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
            try engine.connectNode(player, to: engine.mainMixerNode, format: format)
            player.scheduleBuffer(buffer, at: nil, options: .loops)
            try Task.checkCancellation()
            try engine.start()
            try Task.checkCancellation()
            try player.playAudio()
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
