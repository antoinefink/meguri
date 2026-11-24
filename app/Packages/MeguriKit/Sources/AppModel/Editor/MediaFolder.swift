import AVFoundation
import Foundation
import UniformTypeIdentifiers

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

    /// The file a field names (the page host passes plain names only), when it
    /// exists inside the folder once links are resolved.
    public func file(_ name: String) -> URL? {
        guard let directory else { return nil }
        let root = directory.resolvingSymlinksInPath().standardizedFileURL
        let url = directory.appending(path: name, directoryHint: .notDirectory)
            .resolvingSymlinksInPath().standardizedFileURL
        guard url.deletingLastPathComponent().path == root.path,
              FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url
    }

    /// A clicked thumbnail's file, when it is an image: only images go to the
    /// system's default app.
    public func image(_ name: String) -> URL? {
        guard let url = file(name), let type = UTType(filenameExtension: url.pathExtension),
              type.conforms(to: .image) else { return nil }
        return url
    }
}
