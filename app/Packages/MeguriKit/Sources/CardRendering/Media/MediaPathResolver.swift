import Darwin
import Foundation

public nonisolated enum MediaOrigin {
    public static let scheme = "meguri-media"
    public static let host = "media"
    public static let url = URL(string: "meguri-media://media/")!

    public static func path(_ url: URL) -> String? {
        guard url.scheme == scheme, url.host == host, url.user == nil, url.password == nil, url.port == nil else { return nil }
        return decode(url.path(percentEncoded: true))
    }

    /// Invalid triplets remain literal; valid triplets decode exactly once.
    public static func decode(_ path: String) -> String? {
        let bytes = Array(path.utf8)
        var decoded: [UInt8] = []
        var i = 0
        func hex(_ b: UInt8) -> UInt8? {
            switch b {
            case 48...57: b - 48
            case 65...70: b - 55
            case 97...102: b - 87
            default: nil
            }
        }
        while i < bytes.count {
            if bytes[i] == 37, i + 2 < bytes.count, let hi = hex(bytes[i + 1]), let lo = hex(bytes[i + 2]) {
                let byte = hi * 16 + lo
                guard byte != 47 && byte != 92 && byte != 0 else { return nil }
                decoded.append(byte)
                i += 3
            } else {
                decoded.append(bytes[i])
                i += 1
            }
        }
        guard let result = String(bytes: decoded, encoding: .utf8), !result.contains("\0"), !result.contains("\\"),
              !result.split(separator: "/", omittingEmptySubsequences: false).contains(where: { $0 == "." || $0 == ".." })
        else { return nil }
        return result
    }

    public static func filename(_ name: String) -> Bool {
        !name.isEmpty && name != "." && name != ".." && !name.contains("/") && !name.contains("\\") && !name.contains("\0")
    }
}

/// The collection owner supplies U31's tracked spelling/case-key snapshot.
/// No per-fetch directory listing and no hidden engine/config provider.
public actor MediaPathResolver {
    public nonisolated struct TrackedName: Sendable {
        public let name: String
        public let caseKey: String
        public init(name: String, caseKey: String) { self.name = name; self.caseKey = caseKey }
    }
    public nonisolated enum Failure: Error { case invalidRoot, closed, missing, read }

    private let root: String
    private var descriptor: Int32
    private let device: dev_t
    private let inode: ino_t
    // Swift String keys equate NFC and NFD. U31 case keys deliberately do
    // not normalize, so index their bytes instead.
    private var index: [Data: [String]] = [:]

    private init(root: String, descriptor: Int32, stat: stat, names: [TrackedName]) {
        self.root = root
        self.descriptor = descriptor
        device = stat.st_dev
        inode = stat.st_ino
        index = Self.index(names)
    }

    @concurrent public nonisolated static func open(root: URL, trackedNames: [TrackedName]) async throws -> MediaPathResolver {
        guard root.isFileURL else { throw Failure.invalidRoot }
        let path = root.path
        let fd = Darwin.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw Failure.invalidRoot }
        var info = stat()
        guard fstat(fd, &info) == 0 else { Darwin.close(fd); throw Failure.invalidRoot }
        return MediaPathResolver(root: path, descriptor: fd, stat: info, names: trackedNames)
    }

    deinit { if descriptor >= 0 { Darwin.close(descriptor) } }

    public func update(trackedNames: [TrackedName]) { index = Self.index(trackedNames) }

    public func close() {
        if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
    }

    private nonisolated static func index(_ names: [TrackedName]) -> [Data: [String]] {
        var result: [Data: [String]] = [:]
        for entry in names where MediaOrigin.filename(entry.name) {
            result[Data(entry.caseKey.utf8), default: []].append(entry.name)
        }
        for key in result.keys {
            result[key]?.sort { a, b in
                if Data(a.utf8) == key { return Data(b.utf8) != key }
                if Data(b.utf8) == key { return false }
                return a.utf8.lexicographicallyPrecedes(b.utf8)
            }
        }
        return result
    }

    public func openURLPath(_ path: String) throws -> MediaFile {
        guard path.hasPrefix("/"), MediaOrigin.filename(String(path.dropFirst())) else { throw Failure.missing }
        return try openName(String(path.dropFirst()))
    }

    /// AV metadata is already decoded. Percent characters remain literal.
    public func openAudioName(_ name: String) throws -> MediaFile {
        guard !name.contains("\0"), !name.contains("\\"), let last = name.split(separator: "/", omittingEmptySubsequences: false).last else {
            throw Failure.missing
        }
        return try openName(String(last))
    }

    private func openName(_ name: String) throws -> MediaFile {
        guard descriptor >= 0 else { throw Failure.closed }
        var current = stat()
        guard lstat(root, &current) == 0, current.st_mode & S_IFMT == S_IFDIR,
              current.st_dev == device, current.st_ino == inode else { throw Failure.invalidRoot }
        guard MediaOrigin.filename(name) else { throw Failure.missing }
        let nfc = name.precomposedStringWithCanonicalMapping
        let nfd = nfc.decomposedStringWithCanonicalMapping
        let candidates = [nfc, nfd] + (index[Data(nfc.lowercased().utf8)] ?? [])
        for candidate in candidates {
            // O_NONBLOCK prevents a substituted FIFO from hanging open.
            let fd = openat(descriptor, candidate, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard fd >= 0 else { continue }
            var info = stat()
            guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_size >= 0 else {
                Darwin.close(fd)
                continue
            }
            return MediaFile(descriptor: fd, name: candidate, length: UInt64(info.st_size))
        }
        throw Failure.missing
    }
}

/// An open regular file descriptor; its original path stays internal.
public actor MediaFile {
    public nonisolated let name: String
    public nonisolated let length: UInt64
    private var descriptor: Int32

    init(descriptor: Int32, name: String, length: UInt64) { self.descriptor = descriptor; self.name = name; self.length = length }
    deinit { if descriptor >= 0 { Darwin.close(descriptor) } }

    public func read(offset: UInt64, count: Int) throws -> Data {
        guard descriptor >= 0, offset <= UInt64(Int64.max), count > 0 && count <= 65_536 else { throw MediaPathResolver.Failure.read }
        var data = Data(count: count)
        let received = data.withUnsafeMutableBytes { pread(descriptor, $0.baseAddress, count, off_t(offset)) }
        guard received > 0 else { throw MediaPathResolver.Failure.read }
        data.count = received
        return data
    }

    public func close() { if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 } }

    /// Only an already-open descriptor can reach AVFoundation. Never expose
    /// a path that could be replaced by a symlink between lookup and play.
    func playbackURL() throws -> URL {
        guard descriptor >= 0 else { throw MediaPathResolver.Failure.closed }
        return URL(fileURLWithPath: "/dev/fd/\(descriptor)")
    }
}
