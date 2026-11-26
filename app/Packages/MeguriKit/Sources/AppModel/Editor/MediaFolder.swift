import AVFoundation
import Darwin
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
        directory.flatMap { Self.file(name, in: $0) }
    }

    /// `name` in `directory` when, once links are resolved, it is an existing
    /// file directly inside it. Every media read (thumbnail, sound, open) goes
    /// through this check.
    public nonisolated static func file(_ name: String, in directory: URL) -> URL? {
        guard !name.isEmpty, name != ".", name != "..", !name.contains("/"), !name.contains("\\"), !name.contains("\0") else { return nil }
        let root = directory.resolvingSymlinksInPath().standardizedFileURL
        let url = directory.appending(path: name, directoryHint: .notDirectory)
            .resolvingSymlinksInPath().standardizedFileURL
        guard url.deletingLastPathComponent().path == root.path,
              FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url
    }

    /// IO reads hold the directory descriptor and refuse links and non-files,
    /// so a name cannot escape the collection between checking and reading.
    public nonisolated static func read(_ name: String, in directory: URL) throws -> Data {
        guard !name.isEmpty, name != ".", name != "..", !name.contains("/"), !name.contains("\\"), !name.contains("\0") else {
            throw CocoaError(.fileReadNoPermission)
        }
        let root = directory.resolvingSymlinksInPath().standardizedFileURL
        let parent = open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard parent >= 0 else { throw CocoaError(.fileNoSuchFile) }
        defer { close(parent) }
        let descriptor = openat(parent, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard descriptor >= 0 else { throw CocoaError(.fileNoSuchFile) }
        var info = stat()
        guard fstat(descriptor, &info) == 0, info.st_mode & mode_t(S_IFMT) == mode_t(S_IFREG) else {
            close(descriptor); throw CocoaError(.fileReadNoPermission)
        }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        return try handle.readToEnd() ?? Data()
    }

    /// A clicked thumbnail's file, when it is an image: only images go to the
    /// system's default app.
    public func image(_ name: String) -> URL? {
        guard let url = file(name), let type = UTType(filenameExtension: url.pathExtension),
              type.conforms(to: .image) else { return nil }
        return url
    }
}
