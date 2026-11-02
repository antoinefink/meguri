import Darwin
import Foundation

// J2: the OS root is authority; the raw environment spelling is only an
// expectation. No URL normalization, run-file open or mkdir precedes equality.
@MainActor final class AgentRunDirectory {
    private nonisolated final class Descriptor {
        let value: Int32
        init(_ value: Int32) throws { guard value >= 0 else { throw AgentFailure.storage }; self.value = value }
        deinit { _ = Darwin.close(value) }
    }
    private struct Identity: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, group: gid_t, mode: mode_t
        init(_ s: stat) { device = s.st_dev; inode = s.st_ino; owner = s.st_uid; group = s.st_gid; mode = s.st_mode }
    }
    private struct Directory { let fd: Descriptor, parent: Int32?, name: String, identity: Identity }
    // Retain failed admissions too: a later observation cannot freshen a run.
    private static var actual: [String: AgentRunDirectory] = [:]
    let directory: URL
    let applicationSupportRoot: URL
    private(set) var originalHeaderBytes = Data()
    private(set) var storage: AgentFileStorage!
    private let rootObservation: () throws -> URL
    private let events: Descriptor
    private var directories: [Directory] = []
    private var headerFile: (fd: Descriptor, info: stat)?
    private var invalidated = false

    static func osApplicationSupportRoot(create: Bool = false) throws -> URL {
        let root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                               appropriateFor: nil, create: create)
        // Only this trusted OS result is canonicalized.
        guard let path = realpath(root.path, nil) else { throw AgentFailure.storage }
        defer { free(path) }
        return URL(fileURLWithPath: String(cString: path), isDirectory: true)
    }
    // Setup-only observation: retain the trusted OS root chain while the
    // actual loaded-image observation is attached, without opening any run.
    static func withOSApplicationSupportRoot<T>(create: Bool, _ body: (URL, [[UInt64]]) throws -> T) throws -> T {
        let root = try osApplicationSupportRoot(create: create)
        let events = try Descriptor(kqueue())
        var chain: [Directory] = []
        var parent: Int32?
        for name in ["/"] + root.path.split(separator: "/").map(String.init) {
            let fd = try Descriptor(parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC) }
                ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC))
            var info = stat()
            guard fstat(fd.value, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else { throw AgentFailure.storage }
            var event = kevent(ident: UInt(fd.value), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                               fflags: UInt32(NOTE_ATTRIB | NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE), data: 0, udata: nil)
            guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
            chain.append(.init(fd: fd, parent: parent, name: name, identity: Identity(info)))
            parent = fd.value
        }
        guard let last = chain.last, last.identity.owner == getuid(), last.identity.mode & 0o022 == 0 else { throw AgentFailure.storage }
        let pins = chain.dropFirst().map { item in
            [UInt64(item.identity.device), UInt64(item.identity.inode), UInt64(item.identity.mode), UInt64(item.identity.owner), UInt64(item.identity.group)]
        }
        let result = try body(root, pins)
        guard try osApplicationSupportRoot().path.utf8.elementsEqual(root.path.utf8) else { throw AgentFailure.storage }
        for item in chain {
            var held = stat(), named = stat()
            let found = item.parent.map { fstatat($0, item.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
            guard found == 0, fstat(item.fd.value, &held) == 0, Identity(held) == item.identity,
                  Identity(named) == item.identity else { throw AgentFailure.storage }
        }
        var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
        guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
        return result
    }
    static func admitActual(expectedPath: String) throws -> AgentRunDirectory {
        let labels = try pathLabels(expectedPath)
        let key = labels.run + "/" + labels.device
        if let retained = actual[key] {
            guard retained.directory.path.utf8.elementsEqual(expectedPath.utf8) else {
                retained.invalidated = true
                throw AgentFailure.binding
            }
            try retained.revalidate()
            return retained
        }
        let admission = try AgentRunDirectory(expectedPath: expectedPath, rootObservation: { try osApplicationSupportRoot() })
        actual[key] = admission
        try admission.readHeader()
        return admission
    }
    // Synthetic tests supply a POSIX-realpath trusted temp root. This entry
    // point supplies no evidence that the app executed against its OS container.
    static func generatedUnitOnly(expectedPath: String, trustedRoot: @escaping () throws -> URL) throws -> AgentRunDirectory {
        let admission = try AgentRunDirectory(expectedPath: expectedPath, rootObservation: trustedRoot)
        try admission.readHeader()
        return admission
    }
    private static func pathLabels(_ expectedPath: String) throws -> (run: String, device: String) {
        guard expectedPath.utf8.count <= 4096,
              AgentHeader.FixtureDelivery.validPath(expectedPath, absolute: true) else { throw AgentFailure.binding }
        let parts = expectedPath.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        guard parts.count >= 6 else { throw AgentFailure.binding }
        let run = parts[parts.count - 2], device = parts[parts.count - 1]
        guard run.utf8.count <= 128, AgentHeader.Receipt.validRunID(run),
              device.range(of: #"^[A-Za-z][A-Za-z0-9_-]{0,63}$"#, options: .regularExpression) == (device.startIndex..<device.endIndex) else { throw AgentFailure.binding }
        return (run, device)
    }
    private init(expectedPath: String, rootObservation: @escaping () throws -> URL) throws {
        let (run, device) = try Self.pathLabels(expectedPath)
        let root = try rootObservation()
        guard AgentHeader.FixtureDelivery.validPath(root.path, absolute: true),
              expectedPath.utf8.elementsEqual((root.path + "/Meguri/CloudKitSigned/" + run + "/" + device).utf8) else { throw AgentFailure.binding }
        applicationSupportRoot = root
        directory = URL(fileURLWithPath: expectedPath, isDirectory: true)
        self.rootObservation = rootObservation
        events = try Descriptor(kqueue())
        guard fcntl(events.value, F_SETFD, FD_CLOEXEC) == 0 else { throw AgentFailure.storage }
        var parent: Int32?
        for name in ["/"] + expectedPath.split(separator: "/").map(String.init) {
            let fd = try Descriptor(parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC) }
                ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC))
            var info = stat()
            guard fstat(fd.value, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else { throw AgentFailure.storage }
            var event = kevent(ident: UInt(fd.value), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                               fflags: UInt32(NOTE_ATTRIB | NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE), data: 0, udata: nil)
            guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
            directories.append(.init(fd: fd, parent: parent, name: name, identity: Identity(info)))
            parent = fd.value
        }
        // All app-owned descendants must retain private user authority.
        for item in directories.suffix(5) {
            guard item.identity.owner == getuid(), item.identity.mode & 0o022 == 0 else { throw AgentFailure.storage }
        }
        try revalidate()
    }
    private func readHeader() throws {
        do {
            try revalidate()
            let headerFD = try Descriptor(openat(directories.last!.fd.value, "run.json", O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
            var info = stat()
            guard fstat(headerFD.value, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
                  info.st_nlink == 1, info.st_uid == getuid(), info.st_mode & 0o022 == 0 else { throw AgentFailure.storage }
            var event = kevent(ident: UInt(headerFD.value), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                               fflags: UInt32(NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_LINK | NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE), data: 0, udata: nil)
            guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
            headerFile = (headerFD, info)
            let storage = try AgentFileStorage(directory: directory)
            let bytes = try storage.read(.run)
            let header = try AgentHeader.read(bytes)
            guard directory.lastPathComponent == header.device,
                  directory.deletingLastPathComponent().lastPathComponent == header.run_id else { throw AgentFailure.binding }
            self.storage = storage
            originalHeaderBytes = bytes
            try revalidate()
        } catch { invalidated = true; throw error }
    }
    func revalidate() throws {
        do {
            guard !invalidated, try rootObservation().path.utf8.elementsEqual(applicationSupportRoot.path.utf8) else { throw AgentFailure.storage }
            for item in directories {
                var held = stat(), named = stat()
                let result = item.parent.map { fstatat($0, item.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
                guard result == 0, fstat(item.fd.value, &held) == 0,
                      Identity(held) == item.identity, Identity(named) == item.identity else { throw AgentFailure.storage }
            }
            if let headerFile {
                var held = stat(), named = stat()
                guard fstat(headerFile.fd.value, &held) == 0,
                      fstatat(directories.last!.fd.value, "run.json", &named, AT_SYMLINK_NOFOLLOW) == 0,
                      Self.sameFile(held, headerFile.info), Self.sameFile(named, headerFile.info) else { throw AgentFailure.storage }
            }
            var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
            guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
            if let storage { guard try storage.read(.run) == originalHeaderBytes else { throw AgentFailure.binding } }
        } catch { invalidated = true; throw error }
    }
    private static func sameFile(_ a: stat, _ b: stat) -> Bool {
        Identity(a) == Identity(b) && a.st_nlink == b.st_nlink && a.st_size == b.st_size &&
            a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec &&
            a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec
    }
    func retainedApplicationSupportRoot() throws -> URL {
        try revalidate()
        return applicationSupportRoot
    }
}
