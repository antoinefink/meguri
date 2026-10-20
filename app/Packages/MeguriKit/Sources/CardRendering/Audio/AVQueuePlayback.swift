import AVFoundation
import Foundation

/// Native-format dispatch only. U69 can inject another backend for codecs;
/// unsupported formats are never reported as successfully decoded.
public final class AVQueuePlayback: CardAudioBackend {
    public nonisolated enum Failure: Error { case unsupported }
    // Diagnostic seam for the existing generated-silent hardware helper.
    var onNativePlayInterval: ((TimeInterval, TimeInterval) -> Void)?
    public init() {}
    public func prepare(_ file: MediaFile) async throws -> any CardAudioClip {
        let ext = (file.name as NSString).pathExtension.lowercased()
        guard ["mp3", "m4a", "aac", "wav", "flac", "3gp"].contains(ext) else { throw Failure.unsupported }
        let url = try await file.playbackURL()
        let owner = PlayerOwner(player: try await Self.makePlayer(at: url), file: file)
        let metadata = await owner.metadata()
        let clip = NativeClip(owner: owner, duration: metadata.duration, seekable: metadata.seekable, onPlay: onNativePlayInterval)
        await owner.setDelegate(clip)
        return clip
    }

    // Transfer the sole player reference into its actor after bounded preparation.
    @concurrent private nonisolated static func makePlayer(at url: URL) async throws -> sending AVAudioPlayer {
        let player = try AVAudioPlayer(contentsOf: url)
        #if os(macOS)
        guard player.prepareToPlay() else { throw Failure.unsupported }
        #endif
        // On iOS preparation must not activate the playback/ducking session.
        return player
    }

    private actor PlayerOwner {
        nonisolated struct Metadata: Sendable {
            let duration: TimeInterval
            let seekable: Bool
        }
        private let player: AVAudioPlayer
        private let file: MediaFile
        private var stopped = false
        init(player: sending AVAudioPlayer, file: MediaFile) { self.player = player; self.file = file }
        func metadata() -> Metadata { Metadata(duration: player.duration, seekable: player.duration.isFinite) }
        func setDelegate(_ delegate: NativeClip) { player.delegate = delegate }
        func play() throws -> (TimeInterval, TimeInterval) {
            let startedAt = ProcessInfo.processInfo.systemUptime
            guard !stopped, player.play(atTime: player.deviceCurrentTime + 0.005) else { throw Failure.unsupported }
            return (startedAt, ProcessInfo.processInfo.systemUptime)
        }
        func pause() { if !stopped { player.pause() } }
        func stop() async {
            guard !stopped else { return }
            stopped = true
            player.stop()
            player.delegate = nil
            // Neither the descriptor nor the session may be released during stop.
            await file.close()
        }
        func seek(to time: TimeInterval) throws {
            guard !stopped else { throw CancellationError() }
            player.currentTime = time
        }
        func sample() -> CardAudioSample {
            let position = player.currentTime
            return CardAudioSample(position: position, observedAt: ProcessInfo.processInfo.systemUptime)
        }
    }

    private final class NativeClip: NSObject, CardAudioClip, AVAudioPlayerDelegate {
        private let owner: PlayerOwner
        let duration: TimeInterval?
        let seekable: Bool
        var onEnd: ((Bool) -> Void)?
        private let onPlay: ((TimeInterval, TimeInterval) -> Void)?
        init(owner: PlayerOwner, duration: TimeInterval, seekable: Bool, onPlay: ((TimeInterval, TimeInterval) -> Void)?) {
            self.owner = owner; self.duration = duration; self.seekable = seekable; self.onPlay = onPlay
            super.init()
        }
        isolated deinit {
            let owner = self.owner
            Task { await owner.stop() }
        }
        func play() async throws {
            let (startedAt, finishedAt) = try await owner.play()
            onPlay?(startedAt, finishedAt)
        }
        func pause() async { await owner.pause() }
        func stop() async { onEnd = nil; await owner.stop() }
        func seek(to time: TimeInterval) async throws { try await owner.seek(to: time) }
        func sample() async -> CardAudioSample { await owner.sample() }
        nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
            Task { @MainActor [weak self] in self?.onEnd?(flag) }
        }
        nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: (any Error)?) {
            Task { @MainActor [weak self] in self?.onEnd?(false) }
        }
    }
}
