import AVFoundation
import Foundation

/// Native-format dispatch only. U69 can inject another backend for codecs;
/// unsupported formats are never reported as successfully decoded.
public final class AVQueuePlayback: CardAudioBackend {
    public nonisolated enum Failure: Error { case unsupported }
    public init() {}
    public func prepare(_ file: MediaFile) async throws -> any CardAudioClip {
        let ext = (file.name as NSString).pathExtension.lowercased()
        guard ["mp3", "m4a", "aac", "wav", "flac", "3gp"].contains(ext) else { throw Failure.unsupported }
        let url = try await file.playbackURL()
        let player = try await Self.makePlayer(at: url)
        return NativeClip(player: player, file: file)
    }

    // Create the audio object off the main actor, then transfer it once;
    // no file open/initial decode on the UI executor, no unchecked isolation.
    @concurrent private nonisolated static func makePlayer(at url: URL) async throws -> sending AVAudioPlayer {
        let player = try AVAudioPlayer(contentsOf: url)
        #if os(macOS)
        // Mac has no playback/ducking AVAudioSession. Prepare hardware and
        // bounded decoder buffers here so explicit play need not start it cold.
        guard player.prepareToPlay() else { throw Failure.unsupported }
        #endif
        // On iOS preparation must not activate the playback/ducking session.
        return player
    }

    private final class NativeClip: NSObject, CardAudioClip, AVAudioPlayerDelegate {
        let player: AVAudioPlayer
        // Retain the file descriptor while this clip exists.
        let file: MediaFile
        var onEnd: ((Bool) -> Void)?
        var time: TimeInterval { player.currentTime }
        var duration: TimeInterval? { player.duration }
        var seekable: Bool { player.duration.isFinite }
        init(player: AVAudioPlayer, file: MediaFile) { self.player = player; self.file = file; super.init(); player.delegate = self }
        func play() throws {
            // Bound the scheduling lead; starting hardware can still block this call.
            guard player.play(atTime: player.deviceCurrentTime + 0.005) else { throw Failure.unsupported }
        }
        func pause() { player.pause() }
        func stop() { player.stop(); player.delegate = nil; onEnd = nil; let file = self.file; Task { await file.close() } }
        func seek(to time: TimeInterval) async throws { player.currentTime = time }
        nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
            Task { @MainActor [weak self] in self?.onEnd?(flag) }
        }
        nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: (any Error)?) {
            Task { @MainActor [weak self] in self?.onEnd?(false) }
        }
    }
}
