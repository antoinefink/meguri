import Darwin
import Foundation
#if DEBUG
import Synchronization
#endif

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

    func openAttachmentPDF(_ name: String) throws -> MediaFile {
        guard CardAttachmentPDF.accepts(name), index[Data(name.utf8)]?.contains(name) == true else {
            throw Failure.missing
        }
        return try openName(name, exact: true)
    }

    /// AV metadata is already decoded. Percent characters remain literal.
    public func openAudioName(_ name: String) throws -> MediaFile {
        guard !name.contains("\0"), !name.contains("\\"), let last = name.split(separator: "/", omittingEmptySubsequences: false).last else {
            throw Failure.missing
        }
        return try openName(String(last))
    }

    private func openName(_ name: String, exact: Bool = false) throws -> MediaFile {
        guard descriptor >= 0 else { throw Failure.closed }
        var current = stat()
        guard lstat(root, &current) == 0, current.st_mode & S_IFMT == S_IFDIR,
              current.st_dev == device, current.st_ino == inode else { throw Failure.invalidRoot }
        guard MediaOrigin.filename(name) else { throw Failure.missing }
        let nfc = name.precomposedStringWithCanonicalMapping
        let nfd = nfc.decomposedStringWithCanonicalMapping
        let candidates = exact ? [name] : [nfc, nfd] + (index[Data(nfc.lowercased().utf8)] ?? [])
        for candidate in candidates {
            // O_NONBLOCK prevents a substituted FIFO from hanging open.
            let fd = openat(descriptor, candidate, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard fd >= 0 else { continue }
            var info = stat()
            guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_size >= 0 else {
                Darwin.close(fd)
                continue
            }
            return MediaFile(descriptor: fd, name: candidate, opening: info)
        }
        throw Failure.missing
    }
}

/// An open regular file descriptor; its original path stays internal.
public actor MediaFile {
    public nonisolated let name: String
    public nonisolated let length: UInt64
    private var descriptor: Int32
    private let opening: MediaFileIdentity
    private var playbackCopy: NativePlaybackCopy?
    private var nativeInUse = false
    private var closeRequested = false

    init(descriptor: Int32, name: String, opening: stat) {
        self.descriptor = descriptor; self.name = name; self.length = UInt64(opening.st_size)
        self.opening = MediaFileIdentity(opening)
    }
    #if DEBUG
    /// Tests: files whose deinit is watched, and those whose deinit has run.
    nonisolated static let deinits = Mutex<(watched: Set<ObjectIdentifier>, ran: Set<ObjectIdentifier>)>(([], []))
    nonisolated static func watchDeinit(_ file: MediaFile) {
        let id = ObjectIdentifier(file)
        deinits.withLock { _ = $0.watched.insert(id) }
    }
    nonisolated static func deinitRan(_ id: ObjectIdentifier) -> Bool { deinits.withLock { $0.ran.remove(id) != nil } }
    #endif
    /// Nonisolated on purpose: it runs where the last reference goes, closes
    /// the raw descriptor and drops the private copy, and puts nothing on this
    /// actor. An `isolated deinit` runs as a job on the actor's queue whenever
    /// the actor is not free at that release, and the runtime then destroys
    /// the actor from inside that job; a device crashed there (2026-11-22:
    /// `swift_defaultActor_destroy` aborting under `__isolated_deallocating_deinit`).
    /// Owners close explicitly (`close`, `endNativePlayback`); this is the fallback.
    deinit {
        if descriptor >= 0 { Darwin.close(descriptor) }
        #if DEBUG
        let id = ObjectIdentifier(self)
        Self.deinits.withLock { if $0.watched.remove(id) != nil { $0.ran.insert(id) } }
        #endif
    }

    public func read(offset: UInt64, count: Int) throws -> Data {
        guard descriptor >= 0, !closeRequested, offset <= UInt64(Int64.max), count > 0 && count <= 65_536 else { throw MediaPathResolver.Failure.read }
        var data = Data(count: count)
        var received: Int
        repeat {
            received = data.withUnsafeMutableBytes { pread(descriptor, $0.baseAddress, count, off_t(offset)) }
        } while received < 0 && errno == EINTR && !Task.isCancelled
        guard received > 0 else { throw MediaPathResolver.Failure.read }
        data.count = received
        return data
    }

    public func close() {
        closeRequested = true
        // A close racing decoder initialization cannot unlink its input. The
        // native owner must stop/revoke callbacks before ending this ownership.
        guard !nativeInUse else { return }
        if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
        playbackCopy = nil
    }

    /// Actor-owned descriptor, bounded chunks, no path reopening or disk copy.
    func attachmentPDFBytes() throws -> Data {
        func validate() throws {
            var info = stat()
            guard descriptor >= 0, !closeRequested, fstat(descriptor, &info) == 0,
                  opening == MediaFileIdentity(info) else { throw MediaPathResolver.Failure.read }
        }
        try validate()
        let bytes = try CardAttachmentPDF.read(length: length) { try read(offset: $0, count: $1) }
        try validate()
        return bytes
    }

    func beginNativePlayback() throws -> URL {
        guard !nativeInUse else { throw NativePlaybackCopy.Failure.copy }
        let url = try playbackURL()
        nativeInUse = true
        return url
    }

    func requireNativePlaybackOpen() throws {
        guard !closeRequested, nativeInUse, descriptor >= 0 else { throw CancellationError() }
    }

    func endNativePlayback() {
        nativeInUse = false
        close()
    }

    /// Actor-isolated synchronous work stays off MainActor and cannot race close.
    /// The only reader is the retained descriptor; no collection path is reopened.
    func playbackURL(in area: URL? = nil, hooks: NativePlaybackCopy.Hooks = .init()) throws -> URL {
        try Task.checkCancellation()
        guard descriptor >= 0, !closeRequested else { throw MediaPathResolver.Failure.closed }
        if let playbackCopy { return playbackCopy.url }
        let copy = try NativePlaybackCopy.create(extension: (name as NSString).pathExtension.lowercased(),
            length: length, area: area, hooks: hooks,
            validate: {
                var info = stat()
                guard fstat(self.descriptor, &info) == 0, self.opening == MediaFileIdentity(info) else {
                    throw NativePlaybackCopy.Failure.copy
                }
            }, read: { try self.read(offset: $0, count: $1) })
        try Task.checkCancellation()
        playbackCopy = copy
        return copy.url
    }
}

/// Includes opening timestamps, not merely the descriptor's current length.
nonisolated struct MediaFileIdentity: Equatable {
    let device: dev_t
    let inode: ino_t
    let mode: mode_t
    let owner: uid_t
    let links: nlink_t
    let length: off_t
    let modifiedSeconds: Int
    let modifiedNanos: Int
    let changedSeconds: Int
    let changedNanos: Int

    init(_ info: stat) {
        device = info.st_dev; inode = info.st_ino; mode = info.st_mode
        owner = info.st_uid; links = info.st_nlink; length = info.st_size
        modifiedSeconds = info.st_mtimespec.tv_sec; modifiedNanos = info.st_mtimespec.tv_nsec
        changedSeconds = info.st_ctimespec.tv_sec; changedNanos = info.st_ctimespec.tv_nsec
    }
}
