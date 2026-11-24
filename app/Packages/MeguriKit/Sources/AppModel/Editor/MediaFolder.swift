import AVFoundation
import Foundation

/// The collection's media folder as the field pages use it: their thumbnails
/// load from it by name, a sound chip plays one of its files, and a thumbnail
/// opens its file. Browse's inspector and Edit Note share it.
@MainActor
public final class MediaFolder {
    public let directory: URL?
    private var player: AVAudioPlayer?

    public init(directory: URL?) { self.directory = directory }

    /// A sound chip was clicked: its file plays (once; a new click starts it again).
    public func play(_ name: String) {
        guard let url = file(name) else { return }
        player?.stop()
        player = try? AVAudioPlayer(contentsOf: url)
        player?.play()
    }

    /// The file a field names (the page host passes plain names only), when it exists.
    public func file(_ name: String) -> URL? {
        guard let directory else { return nil }
        let url = directory.appending(path: name, directoryHint: .notDirectory)
        return FileManager.default.fileExists(atPath: url.path) ? url : nil
    }
}
