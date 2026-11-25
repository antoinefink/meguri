import AVFoundation
import Foundation

/// Native-format dispatch only. U69 can inject another backend for codecs;
/// unsupported formats are never reported as successfully decoded.
public final class AVQueuePlayback: CardAudioBackend {
    public nonisolated enum Failure: Error { case unsupported }
    // Diagnostic seam for the existing generated-silent hardware helper.
    var onNativePlayInterval: ((TimeInterval, TimeInterval) -> Void)?
    // Temporary, nil-by-default test diagnostics; no app/UI observation or logging.
    var onNativeCompletionInterval: (@MainActor (TimeInterval, TimeInterval) -> Void)?
    nonisolated enum NativePreparationPhase: Sendable {
        case beginNativePlayback, playerConstructor, prepareToPlay, metadata, requireNativePlaybackOpen
    }
    // Fixed, bounded elapsed milliseconds only; safe on the concurrent constructor.
    var onNativePreparationInterval: (@Sendable (NativePreparationPhase, UInt64) -> Void)?
    // Internal deterministic close/cancel barrier after native initialization.
    // Nil in production; never receives the player or its private URL.
    var beforeNativeTransfer: (@Sendable () -> Void)?
    // Operation gate before file admission; nil during normal playback.
    var beforeNativePlay: (@Sendable () async -> Void)?
    public init() {}
    public func prepare(_ file: MediaFile) async throws -> any CardAudioClip {
        let onPreparation = onNativePreparationInterval
        var ownsNativeInput = false
        do {
            try Task.checkCancellation()
            let ext = (file.name as NSString).pathExtension.lowercased()
            guard NativePlaybackCopy.extensions.contains(ext) else { throw Failure.unsupported }
            let url: URL
            do {
                let startedAt = onPreparation.map { _ in ContinuousClock.now }
                defer { Self.recordPreparation(.beginNativePlayback, since: startedAt, observer: onPreparation) }
                url = try await file.beginNativePlayback()
            }
            ownsNativeInput = true
            let owner = PlayerOwner(player: try await Self.makePlayer(at: url, beforeTransfer: beforeNativeTransfer,
                                                                    onPreparation: onPreparation), file: file, beforePlay: beforeNativePlay)
            do {
                let metadata: PlayerOwner.Metadata
                do {
                    let startedAt = onPreparation.map { _ in ContinuousClock.now }
                    defer { Self.recordPreparation(.metadata, since: startedAt, observer: onPreparation) }
                    metadata = await owner.metadata()
                }
                do {
                    let startedAt = onPreparation.map { _ in ContinuousClock.now }
                    defer { Self.recordPreparation(.requireNativePlaybackOpen, since: startedAt, observer: onPreparation) }
                    try await file.requireNativePlaybackOpen()
                }
                try Task.checkCancellation()
                return NativeClip(owner: owner, duration: metadata.duration, seekable: metadata.seekable,
                                  onPlay: onNativePlayInterval, onCompletion: onNativeCompletionInterval)
            } catch {
                await owner.stop()
                ownsNativeInput = false
                throw error
            }
        } catch {
            if ownsNativeInput { await file.endNativePlayback() }
            else { await file.close() }
            throw error
        }
    }

    private nonisolated static func recordPreparation(_ phase: NativePreparationPhase,
        since start: ContinuousClock.Instant?, observer: (@Sendable (NativePreparationPhase, UInt64) -> Void)?) {
        guard let start, let observer else { return }
        let elapsed = start.duration(to: .now)
        guard elapsed > .zero else { observer(phase, 0); return }
        let components = elapsed.components
        guard components.seconds < 600 else { observer(phase, 600_000); return }
        let milliseconds = components.seconds * 1_000 + components.attoseconds / 1_000_000_000_000_000
        observer(phase, UInt64(max(0, min(600_000, milliseconds))))
    }

    // Transfer the sole player reference into its actor after bounded preparation.
    @concurrent private nonisolated static func makePlayer(at url: URL,
        beforeTransfer: (@Sendable () -> Void)?,
        onPreparation: (@Sendable (NativePreparationPhase, UInt64) -> Void)?) async throws -> sending AVAudioPlayer {
        try Task.checkCancellation()
        let player: AVAudioPlayer
        do {
            let startedAt = onPreparation.map { _ in ContinuousClock.now }
            defer { recordPreparation(.playerConstructor, since: startedAt, observer: onPreparation) }
            player = try AVAudioPlayer(contentsOf: url)
        } catch {
            // AVFoundation may attach the private URL to userInfo. Preserve only
            // its numeric decoder result across the public backend boundary.
            let native = error as NSError
            let domains = [NSOSStatusErrorDomain, AVFoundationErrorDomain, NSPOSIXErrorDomain, NSCocoaErrorDomain]
            let domain = domains.contains(native.domain) ? native.domain : "MeguriNativeDecoder"
            throw NSError(domain: domain, code: native.code)
        }
        var prepared = false
        defer { if !prepared { player.stop(); player.delegate = nil } }
        try Task.checkCancellation()
        #if os(macOS)
        do {
            let startedAt = onPreparation.map { _ in ContinuousClock.now }
            defer { recordPreparation(.prepareToPlay, since: startedAt, observer: onPreparation) }
            guard player.prepareToPlay() else { throw Failure.unsupported }
        }
        #endif
        // On iOS preparation must not activate the playback/ducking session.
        beforeTransfer?()
        try Task.checkCancellation()
        prepared = true
        return player
    }

    private actor PlayerOwner {
        nonisolated struct Metadata: Sendable {
            let duration: TimeInterval
            let seekable: Bool
        }
        private let player: AVAudioPlayer
        private let file: MediaFile
        private let beforePlay: (@Sendable () async -> Void)?
        private var delegate: CompletionDelegate?
        private var stopped = false
        init(player: sending AVAudioPlayer, file: MediaFile, beforePlay: (@Sendable () async -> Void)?) {
            self.player = player; self.file = file; self.beforePlay = beforePlay
        }
        func metadata() -> Metadata { Metadata(duration: player.duration, seekable: player.duration.isFinite) }
        private func setDelegate(_ delegate: CompletionDelegate?) {
            self.delegate = delegate
            player.delegate = delegate
        }
        func play(delegate: CompletionDelegate, permit: CardAudioPlayPermit) async throws -> (TimeInterval, TimeInterval) {
            await beforePlay?()
            try Task.checkCancellation()
            try await file.requireNativePlaybackOpen()
            try Task.checkCancellation()
            guard !stopped else { throw Failure.unsupported }
            setDelegate(delegate)
            let startedAt = ProcessInfo.processInfo.systemUptime
            #if os(iOS)
            // Prepare before sampling the device clock so setup cannot consume the lead time.
            guard player.prepareToPlay() else {
                player.stop()
                setDelegate(nil)
                throw Failure.unsupported
            }
            #endif
            try Task.checkCancellation()
            guard try permit.trySchedule({ player.play(atTime: player.deviceCurrentTime + 0.005) }) else {
                player.stop()
                setDelegate(nil)
                throw Failure.unsupported
            }
            return (startedAt, ProcessInfo.processInfo.systemUptime)
        }
        func pause() {
            guard !stopped else { return }
            setDelegate(nil)
            player.pause()
        }
        func stop() async {
            guard !stopped else { return }
            stopped = true
            player.stop()
            setDelegate(nil)
            // Neither the descriptor nor the session may be released during stop.
            await file.endNativePlayback()
        }
        func seek(to time: TimeInterval, delegate: CompletionDelegate) throws {
            guard !stopped else { throw CancellationError() }
            setDelegate(nil)
            player.currentTime = time
            setDelegate(delegate)
        }
        func sample() -> CardAudioSample {
            let position = player.currentTime
            return CardAudioSample(position: position, observedAt: ProcessInfo.processInfo.systemUptime)
        }
    }

    /// Shared by the native delegate and deterministic, hardware-free checks.
    /// Only MainActor validates epochs or reads the mutable completion.
    @MainActor final class Completion {
        var onEnd: ((Bool) -> Void)? { didSet { invalidate() } }
        private var epoch: UInt64 = 0
        private let onInterval: (@MainActor (TimeInterval, TimeInterval) -> Void)?

        init(onInterval: (@MainActor (TimeInterval, TimeInterval) -> Void)? = nil) {
            self.onInterval = onInterval
        }

        func invalidate() { epoch += 1 }
        func makeDelegate() -> CompletionDelegate {
            invalidate()
            return CompletionDelegate(completion: self, epoch: epoch)
        }
        fileprivate func deliver(_ success: Bool, epoch: UInt64, arrivedAt: TimeInterval) {
            let enteredAt = ProcessInfo.processInfo.systemUptime
            guard self.epoch == epoch else { return }
            invalidate() // Finish/decode-error duplicates complete this play only once.
            let onInterval = self.onInterval
            onEnd?(success)
            onInterval?(arrivedAt, enteredAt)
        }
    }

    nonisolated final class CompletionDelegate: NSObject, AVAudioPlayerDelegate, Sendable {
        private weak let completion: Completion?
        private let epoch: UInt64
        init(completion: Completion, epoch: UInt64) {
            self.completion = completion; self.epoch = epoch
            super.init()
        }
        /// The production delegate hop; the task is exposed internally so tests
        /// can enqueue before an epoch change and await that exact delivery.
        @discardableResult func enqueue(success: Bool) -> Task<Void, Never> {
            let arrivedAt = ProcessInfo.processInfo.systemUptime
            let completion = self.completion, epoch = self.epoch
            return Task { @MainActor in
                guard let completion else { return }
                completion.deliver(success, epoch: epoch, arrivedAt: arrivedAt)
            }
        }
        func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
            enqueue(success: flag)
        }
        func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: (any Error)?) {
            enqueue(success: false)
        }
    }

    private final class NativeClip: NSObject, CardAudioClip {
        private let owner: PlayerOwner
        private let completion: Completion
        let duration: TimeInterval?
        let seekable: Bool
        var onEnd: ((Bool) -> Void)? {
            get { completion.onEnd }
            set { completion.onEnd = newValue }
        }
        private let onPlay: ((TimeInterval, TimeInterval) -> Void)?
        init(owner: PlayerOwner, duration: TimeInterval, seekable: Bool, onPlay: ((TimeInterval, TimeInterval) -> Void)?,
             onCompletion: (@MainActor (TimeInterval, TimeInterval) -> Void)?) {
            self.owner = owner; self.duration = duration; self.seekable = seekable; self.onPlay = onPlay
            self.completion = Completion(onInterval: onCompletion)
            super.init()
        }
        isolated deinit {
            completion.onEnd = nil
            let owner = self.owner
            Task { await owner.stop() }
        }
        func play(permit: CardAudioPlayPermit) async throws {
            do {
                let (startedAt, finishedAt) = try await owner.play(delegate: completion.makeDelegate(), permit: permit)
                onPlay?(startedAt, finishedAt)
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                onEnd = nil
                await owner.stop()
                throw error
            }
        }
        func pause() async { completion.invalidate(); await owner.pause() }
        func stop() async { onEnd = nil; await owner.stop() }
        func seek(to time: TimeInterval) async throws {
            try await owner.seek(to: time, delegate: completion.makeDelegate())
        }
        func sample() async -> CardAudioSample { await owner.sample() }
    }
}
