import Darwin
import Foundation
import OSLog

/// Sealed decoder input owned by one MediaFile.
/// Its actor performs creation and removal without suspension.
nonisolated final class NativePlaybackCopy {
    nonisolated enum Failure: Error { case copy }
    static let extensions: Set<String> = ["mp3", "m4a", "aac", "wav", "flac", "3gp"]
    static let namespace = "meguri-native-audio-v1"
    private static let maintenance = Maintenance()
    private static let lockName = "lease"
    private static let markerName = "complete"

    // Per-operation, internal fault/barrier seams; production uses only defaults.
    nonisolated enum Stage: Sendable, Hashable { case beforeLease, copying, published, disposing }
    nonisolated enum AreaObservation: Sendable, Hashable { case opening, validating }
    nonisolated struct Hooks: Sendable {
        var barrier: (@Sendable (Stage, URL) -> Void)?
        var didRead: (@Sendable (Int) -> Void)?
        var write: (@Sendable (Int32, Data, Int) -> Int)?
        var unlink: (@Sendable (Int32, String) -> Int32)?
        var didObserveArea: (@Sendable (AreaObservation, URL) -> Void)?
    }

    let url: URL
    private let areaFD: Int32
    private let directoryFD: Int32
    private var leaseFD: Int32 = -1
    private let directoryName: String
    private let directoryIdentity: MediaFileIdentity
    private var files: [String: MediaFileIdentity] = [:]
    private let hooks: Hooks

    private init(areaFD: Int32, directoryFD: Int32, name: String, identity: MediaFileIdentity,
                 url: URL, hooks: Hooks) {
        self.areaFD = areaFD; self.directoryFD = directoryFD; directoryName = name
        directoryIdentity = identity; self.url = url; self.hooks = hooks
    }

    static func create(extension suffix: String, length: UInt64, area: URL?, hooks: Hooks,
                       validate: () throws -> Void, read: (UInt64, Int) throws -> Data) throws -> NativePlaybackCopy {
        guard extensions.contains(suffix), length <= UInt64(Int64.max) else { throw Failure.copy }
        try Task.checkCancellation()
        try validate()
        let root = area ?? FileManager.default.temporaryDirectory.appending(path: namespace, directoryHint: .isDirectory)
        let parent = try openArea(root, hooks: hooks)
        // Only production's first use scans the private namespace, never media.
        if area == nil, maintenance.claimRecovery() { recover(areaFD: parent) }
        let name = "clip-" + UUID().uuidString.lowercased()
        guard mkdirat(parent, name, 0o700) == 0 else { Darwin.close(parent); throw Failure.copy }
        let directory = openat(parent, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard directory >= 0, let directoryIdentity = identity(directory), privateDirectory(directoryIdentity),
              sameEntry(parent, name, directoryIdentity) else {
            if directory >= 0 { Darwin.close(directory) }
            // An uncertain entry is deliberately left alone.
            Darwin.close(parent)
            throw Failure.copy
        }
        let payload = "payload." + suffix
        let copy = NativePlaybackCopy(areaFD: parent, directoryFD: directory, name: name, identity: directoryIdentity,
            url: root.appending(path: name, directoryHint: .isDirectory).appending(path: payload), hooks: hooks)
        hooks.barrier?(.beforeLease, copy.url)
        try Task.checkCancellation()
        let lease = try copy.createFile(lockName)
        copy.leaseFD = lease
        guard flock(lease, LOCK_EX | LOCK_NB) == 0 else { throw Failure.copy }
        try Task.checkCancellation()
        let output = try copy.createFile(payload)
        var writable = output
        defer { if writable >= 0 { Darwin.close(writable) } }
        var offset: UInt64 = 0
        while offset < length {
            try Task.checkCancellation()
            let count = Int(min(UInt64(65_536), length - offset))
            let bytes: Data
            do { bytes = try read(offset, count) }
            catch is CancellationError { throw CancellationError() }
            catch { try Task.checkCancellation(); throw Failure.copy }
            hooks.didRead?(count)
            try Task.checkCancellation()
            guard !bytes.isEmpty, bytes.count <= count else { throw Failure.copy }
            try writeAll(bytes, to: output, hooks: hooks)
            let (next, overflow) = offset.addingReportingOverflow(UInt64(bytes.count))
            guard !overflow, next <= length else { throw Failure.copy }
            offset = next
            hooks.barrier?(.copying, copy.url)
            try Task.checkCancellation()
        }
        try validate()
        guard let written = identity(output), written.length == off_t(length), regular(written, mode: 0o600),
              copy.ownedEntry(payload, written), fchmod(output, 0o400) == 0 else { throw Failure.copy }
        guard let sealed = identity(output) else { throw Failure.copy }
        copy.files[payload] = sealed
        let closed = Darwin.close(output)
        writable = -1
        guard closed == 0 else { throw Failure.copy }
        try Task.checkCancellation()
        try validate()
        guard copy.validDirectory(), sameEntry(directory, payload, sealed),
              regular(sealed, mode: 0o400), let leaseIdentity = copy.files[lockName],
              sameEntry(directory, lockName, leaseIdentity),
              names(directory, limit: 3) == Set([lockName, payload]) else { throw Failure.copy }
        let marker = try copy.createFile(markerName)
        var markerWritable = marker
        defer { if markerWritable >= 0 { Darwin.close(markerWritable) } }
        let publicationBytes = markerBytes(directory: directoryIdentity, lease: leaseIdentity, payload: sealed, name: payload)
        try writeAll(publicationBytes, to: marker, hooks: .init())
        guard fchmod(marker, 0o400) == 0, let completed = self.identity(marker), regular(completed, mode: 0o400),
              completed.length == off_t(publicationBytes.count) else { throw Failure.copy }
        copy.files[markerName] = completed
        let markerClosed = Darwin.close(marker)
        markerWritable = -1
        guard markerClosed == 0, copy.validDirectory(), sameEntry(directory, markerName, completed),
              sameEntry(directory, payload, sealed), sameEntry(directory, lockName, leaseIdentity) else {
            throw Failure.copy
        }
        hooks.barrier?(.published, copy.url)
        try Task.checkCancellation()
        try validate()
        guard copy.validDirectory(), let final = publication(directory), final.lease == leaseIdentity,
              final.payload == sealed, final.marker == completed else { throw Failure.copy }
        return copy
    }

    private func createFile(_ name: String) throws -> Int32 {
        guard validDirectory() else { throw Failure.copy }
        let fd = openat(directoryFD, name, O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw Failure.copy }
        guard let info = Self.identity(fd), Self.regular(info, mode: 0o600), Self.sameEntry(directoryFD, name, info) else {
            Darwin.close(fd); throw Failure.copy
        }
        files[name] = info
        return fd
    }

    private func validDirectory() -> Bool {
        var root = stat()
        let area = url.deletingLastPathComponent().deletingLastPathComponent()
        guard let current = Self.identity(directoryFD), let parent = Self.identity(areaFD) else { return false }
        hooks.didObserveArea?(.validating, area)
        guard lstat(Self.directoryPath(area), &root) == 0,
              Self.samePrivateArea(parent, MediaFileIdentity(root)) else { return false }
        return Self.privateDirectory(current) && Self.sameNode(current, directoryIdentity)
            && Self.sameEntry(areaFD, directoryName, current)
    }

    private func ownedEntry(_ name: String, _ current: MediaFileIdentity) -> Bool {
        guard let original = files[name] else { return false }
        return Self.sameNode(original, current) && Self.sameEntry(directoryFD, name, current)
    }

    deinit {
        hooks.barrier?(.disposing, url)
        // Partial creation is ours too; removal never follows a substituted name.
        // Keep the lease open and locked until every removal attempt finishes.
        var removed = validDirectory()
        if removed {
            for name in [url.lastPathComponent, Self.markerName, Self.lockName] {
                guard let original = files[name] else { continue }
                guard let current = Self.entry(directoryFD, name), Self.sameNode(original, current),
                      current.owner == getuid(), current.mode & S_IFMT == S_IFREG, current.links == 1 else {
                    removed = false; break
                }
                let result = hooks.unlink?(directoryFD, name) ?? unlinkat(directoryFD, name, 0)
                if result != 0 { removed = false; break }
            }
            if removed {
                removed = validDirectory() && unlinkat(areaFD, directoryName, AT_REMOVEDIR) == 0
            }
        }
        if !removed { Self.maintenance.reportRemovalFailure() }
        if leaseFD >= 0 { Darwin.close(leaseFD) }
        Darwin.close(directoryFD)
        Darwin.close(areaFD)
    }

    /// Testable independently of WebKit. Callers supply only this private area.
    static func recover(in area: URL) {
        guard let fd = try? openArea(area) else { return }
        defer { Darwin.close(fd) }
        recover(areaFD: fd)
    }

    private nonisolated struct Publication: Equatable {
        let directory: MediaFileIdentity
        let lease: MediaFileIdentity
        let payload: MediaFileIdentity
        let marker: MediaFileIdentity
        let payloadName: String
    }

    private static func publication(_ directory: Int32) -> Publication? {
        guard let dir = identity(directory), privateDirectory(dir), let children = names(directory, limit: 4),
              children.count == 3, children.contains(lockName), children.contains(markerName),
              let payloadName = children.first(where: { $0.hasPrefix("payload.") }),
              extensions.contains(String(payloadName.dropFirst("payload.".count))),
              let lease = entry(directory, lockName), regular(lease, mode: 0o600), lease.length == 0,
              let payload = entry(directory, payloadName), regular(payload, mode: 0o400), payload.length >= 0,
              let marker = entry(directory, markerName), regular(marker, mode: 0o400),
              marker.length > 0, marker.length <= 512 else { return nil }
        let fd = openat(directory, markerName, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        guard fd >= 0 else { return nil }
        defer { Darwin.close(fd) }
        guard identity(fd) == marker else { return nil }
        var bytes = Data(count: Int(marker.length))
        let count = bytes.withUnsafeMutableBytes { pread(fd, $0.baseAddress, $0.count, 0) }
        guard count == bytes.count, identity(fd) == marker,
              bytes == markerBytes(directory: dir, lease: lease, payload: payload, name: payloadName) else { return nil }
        return Publication(directory: dir, lease: lease, payload: payload, marker: marker, payloadName: payloadName)
    }

    private static func recover(areaFD: Int32) {
        // At most 128 names per process lifecycle; no recursive or shared-temp walk.
        guard let stream = directoryStream(areaFD) else { return }
        defer { closedir(stream) }
        var inspected = 0
        while inspected < 128, let item = readdir(stream) {
            let name = entryName(item)
            if name == "." || name == ".." { continue }
            inspected += 1
            guard generatedDirectory(name), let initial = entry(areaFD, name), privateDirectory(initial) else { continue }
            let dir = openat(areaFD, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard dir >= 0 else { continue }
            defer { Darwin.close(dir) }
            guard identity(dir) == initial, let before = publication(dir), before.directory == initial else { continue }
            let lease = openat(dir, lockName, O_RDWR | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard lease >= 0 else { continue }
            defer { Darwin.close(lease) }
            guard identity(lease) == before.lease, flock(lease, LOCK_EX | LOCK_NB) == 0 else { continue }
            guard let after = publication(dir), before == after, sameEntry(areaFD, name, after.directory),
                  identity(lease) == after.lease else { continue }
            // Revalidate the complete set once more under exclusive authority.
            guard publication(dir) == after, sameEntry(areaFD, name, after.directory) else { continue }
            if unlinkat(dir, after.payloadName, 0) != 0 || unlinkat(dir, markerName, 0) != 0
                || unlinkat(dir, lockName, 0) != 0 || unlinkat(areaFD, name, AT_REMOVEDIR) != 0 {
                maintenance.reportRemovalFailure()
            }
        }
    }

    private static func openArea(_ area: URL, hooks: Hooks = .init()) throws -> Int32 {
        guard area.isFileURL, area.lastPathComponent == namespace else { throw Failure.copy }
        // A trailing slash can make a final-component symlink resolve as a
        // directory despite O_NOFOLLOW. Always check the actual final name.
        let path = directoryPath(area)
        let result = mkdir(path, 0o700)
        guard result == 0 || errno == EEXIST else { throw Failure.copy }
        let fd = Darwin.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw Failure.copy }
        var pathInfo = stat()
        guard let info = identity(fd) else { Darwin.close(fd); throw Failure.copy }
        hooks.didObserveArea?(.opening, area)
        guard lstat(path, &pathInfo) == 0, samePrivateArea(info, MediaFileIdentity(pathInfo)) else {
            Darwin.close(fd); throw Failure.copy
        }
        return fd
    }

    private static func directoryPath(_ url: URL) -> String {
        var path = url.path
        while path.count > 1 && path.hasSuffix("/") { path.removeLast() }
        return path
    }

    private static func writeAll(_ bytes: Data, to fd: Int32, hooks: Hooks) throws {
        var offset = 0
        while offset < bytes.count {
            try Task.checkCancellation()
            let remaining = bytes.count - offset
            let count = hooks.write?(fd, bytes, offset) ?? bytes.withUnsafeBytes {
                Darwin.write(fd, $0.baseAddress!.advanced(by: offset), remaining)
            }
            if count < 0, errno == EINTR { continue }
            guard count > 0, count <= remaining else { throw Failure.copy }
            offset += count
            try Task.checkCancellation()
        }
    }

    private static func markerBytes(directory: MediaFileIdentity, lease: MediaFileIdentity,
                                    payload: MediaFileIdentity, name: String) -> Data {
        Data((namespace + "\n\(directory.device):\(directory.inode)\n\(lease.device):\(lease.inode)\n"
              + "\(lease.changedSeconds):\(lease.changedNanos)\n"
              + "\(payload.device):\(payload.inode):\(payload.length)\n"
              + "\(payload.modifiedSeconds):\(payload.modifiedNanos):\(payload.changedSeconds):\(payload.changedNanos)\n\(name)\n").utf8)
    }

    private static func identity(_ fd: Int32) -> MediaFileIdentity? {
        var info = stat()
        return fstat(fd, &info) == 0 ? MediaFileIdentity(info) : nil
    }
    private static func entry(_ fd: Int32, _ name: String) -> MediaFileIdentity? {
        var info = stat()
        return fstatat(fd, name, &info, AT_SYMLINK_NOFOLLOW) == 0 ? MediaFileIdentity(info) : nil
    }
    private static func sameEntry(_ fd: Int32, _ name: String, _ info: MediaFileIdentity) -> Bool {
        entry(fd, name) == info
    }
    private static func sameNode(_ a: MediaFileIdentity, _ b: MediaFileIdentity) -> Bool {
        a.device == b.device && a.inode == b.inode
    }
    private static func samePrivateArea(_ descriptor: MediaFileIdentity, _ path: MediaFileIdentity) -> Bool {
        privateDirectory(descriptor) && privateDirectory(path) && sameNode(descriptor, path)
    }
    private static func privateDirectory(_ info: MediaFileIdentity) -> Bool {
        info.owner == getuid() && info.mode & S_IFMT == S_IFDIR && info.mode & 0o7777 == 0o700
    }
    private static func regular(_ info: MediaFileIdentity, mode: mode_t) -> Bool {
        info.owner == getuid() && info.mode & S_IFMT == S_IFREG && info.mode & 0o7777 == mode && info.links == 1
    }
    private static func generatedDirectory(_ name: String) -> Bool {
        guard name.hasPrefix("clip-"), let uuid = UUID(uuidString: String(name.dropFirst(5))) else { return false }
        return name == "clip-" + uuid.uuidString.lowercased()
    }
    private static func directoryStream(_ fd: Int32) -> UnsafeMutablePointer<DIR>? {
        // A new open-file description, so scans don't share a directory offset.
        let scan = openat(fd, ".", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard scan >= 0 else { return nil }
        guard let stream = fdopendir(scan) else { Darwin.close(scan); return nil }
        return stream
    }
    private static func entryName(_ item: UnsafeMutablePointer<dirent>) -> String {
        withUnsafePointer(to: &item.pointee.d_name) {
            $0.withMemoryRebound(to: CChar.self, capacity: Int(item.pointee.d_namlen) + 1) { String(cString: $0) }
        }
    }
    private static func names(_ fd: Int32, limit: Int) -> Set<String>? {
        guard let stream = directoryStream(fd) else { return nil }
        defer { closedir(stream) }
        var result: Set<String> = []
        while let item = readdir(stream) {
            let name = entryName(item)
            if name == "." || name == ".." { continue }
            result.insert(name)
            if result.count > limit { return nil }
        }
        return result
    }

    private nonisolated final class Maintenance: @unchecked Sendable {
        private let lock = NSLock()
        private var recovered = false
        private var reported = false
        func claimRecovery() -> Bool {
            lock.lock(); defer { lock.unlock() }
            guard !recovered else { return false }
            recovered = true
            return true
        }
        func reportRemovalFailure() {
            lock.lock(); defer { lock.unlock() }
            guard !reported else { return }
            reported = true
            Logger(subsystem: "Meguri", category: "NativeAudio").error("Private audio cleanup failed")
        }
    }
}
