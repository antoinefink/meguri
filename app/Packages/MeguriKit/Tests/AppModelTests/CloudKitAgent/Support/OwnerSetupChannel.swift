#if os(macOS)
import Darwin
import Foundation

// A single live socket in this app's OS-derived support root. Candidates live
// only in memory/kernel buffers. Relative connect avoids macOS sun_path limits
// without accepting another root or placing a socket outside the sandbox.
@MainActor final class OwnerSetupChannel {
    enum Checkpoint: String {
        case initial, root, directories, socket, events, ready, responseWrite, commandRead, commandParse
        case attributeEvent, renameEvent, deleteEvent, revokeEvent, otherEvent
        case stageAttribute, supportAttribute, ancestorAttribute
    }
    private(set) var checkpoint = Checkpoint.initial
    private struct Pin: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, mode: mode_t
        init(_ value: stat) { device = value.st_dev; inode = value.st_ino; owner = value.st_uid; mode = value.st_mode }
    }
    private struct Directory { let parent: Int32?, name: String, fd: Int32, pin: Pin }
    private var directories: [Directory] = []
    private var socketFD: Int32 = -1
    private var socketPin: Pin?
    private var closed = false
    private var invalidated = false
    private let rootObservation: () throws -> URL
    private let path: String
    private let events: Int32
    private var buffered = Data()
    private static let limit = 128 * 1024

    convenience init(path: String) throws {
        try self.init(path: path, rootObservation: { try AgentRunDirectory.osApplicationSupportRoot() })
    }

    // Generated tests supply one canonical fixture root; this is no OS or account proof.
    static func generatedUnitOnly(path: String, trustedRoot: @escaping () throws -> URL) throws -> OwnerSetupChannel {
        try OwnerSetupChannel(path: path, rootObservation: trustedRoot)
    }

    private init(path: String, rootObservation: @escaping () throws -> URL) throws {
        self.rootObservation = rootObservation
        self.path = path
        events = kqueue()
        guard events >= 0 else { throw AgentFailure.storage }
        do {
            let root = try rootObservation()
            guard path.hasPrefix(root.path + "/.meguri-account-"),
                  String(path.dropFirst(root.path.count + 1)).range(of: #"^\.meguri-account-[0-9a-f]{32}$"#,
                       options: .regularExpression) != nil else { throw AgentFailure.storage }
            var parent: Int32?
            let components = ["/"] + path.split(separator: "/").map(String.init)
            for (index, name) in components.enumerated() {
                let fd = parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK) }
                    ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                guard fd >= 0 else { throw AgentFailure.storage }
                var info = stat()
                guard fstat(fd, &info) == 0 else { _ = Darwin.close(fd); throw AgentFailure.storage }
                directories.append(Directory(parent: parent, name: name, fd: fd, pin: Pin(info)))
                // OS-managed ancestors may receive harmless timestamp/xattr updates.
                // Their retained identities and permissions are checked at every boundary.
                // Keep transient-attribute detection on the private stage itself.
                let mask = NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE | (index == components.count - 1 ? NOTE_ATTRIB : 0)
                var event = kevent(ident: UInt(fd), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                    fflags: UInt32(mask), data: 0, udata: nil)
                guard kevent(events, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
                parent = fd
            }
            let directory = directories.last!
            guard directory.pin.owner == getuid(), directory.pin.mode & 0o7777 == 0o700 else { throw AgentFailure.storage }
            var info = stat()
            guard fstatat(directory.fd, "channel", &info, AT_SYMLINK_NOFOLLOW) == 0,
                  info.st_mode & S_IFMT == S_IFSOCK, info.st_mode & 0o7777 == 0o600,
                  info.st_uid == getuid(), info.st_nlink == 1 else { throw AgentFailure.storage }
            socketPin = Pin(info)
            socketFD = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
            guard socketFD >= 0, fcntl(socketFD, F_SETFD, FD_CLOEXEC) == 0 else { throw AgentFailure.storage }
            var yes: Int32 = 1
            guard setsockopt(socketFD, SOL_SOCKET, SO_NOSIGPIPE, &yes, socklen_t(MemoryLayout.size(ofValue: yes))) == 0 else {
                throw AgentFailure.storage
            }
            var timeout = timeval(tv_sec: 600, tv_usec: 0)
            guard setsockopt(socketFD, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout))) == 0,
                  setsockopt(socketFD, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout))) == 0 else {
                throw AgentFailure.storage
            }
            let previous = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC)
            guard previous >= 0 else { throw AgentFailure.storage }
            defer { _ = fchdir(previous); _ = Darwin.close(previous) }
            guard fchdir(directory.fd) == 0 else { throw AgentFailure.storage }
            var address = sockaddr_un()
            address.sun_family = sa_family_t(AF_UNIX)
            address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
            withUnsafeMutableBytes(of: &address.sun_path) { bytes in
                _ = bytes.initializeMemory(as: UInt8.self, repeating: 0)
                for (index, byte) in "channel".utf8.enumerated() { bytes[index] = byte }
            }
            let result = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.connect(socketFD, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
                }
            }
            guard result == 0 else { throw AgentFailure.storage }
            try check()
        } catch {
            close()
            throw AgentFailure.storage
        }
    }
    func check() throws {
        do {
            try checkOriginal()
        } catch {
            invalidated = true
            throw error
        }
    }

    private func checkOriginal() throws {
        checkpoint = .root
        guard !closed, !invalidated, try rootObservation().path + "/" + URL(fileURLWithPath: path).lastPathComponent == path else {
            throw AgentFailure.storage
        }
        checkpoint = .directories
        for directory in directories {
            var held = stat(), named = stat()
            guard fstat(directory.fd, &held) == 0, Pin(held) == directory.pin else { throw AgentFailure.storage }
            let result = directory.parent.map { fstatat($0, directory.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
            guard result == 0, Pin(named) == directory.pin else { throw AgentFailure.storage }
        }
        var named = stat(), event = kevent(), zero = timespec(tv_sec: 0, tv_nsec: 0)
        checkpoint = .socket
        guard let directory = directories.last, fstatat(directory.fd, "channel", &named, AT_SYMLINK_NOFOLLOW) == 0,
              Pin(named) == socketPin, named.st_nlink == 1 else { throw AgentFailure.storage }
        checkpoint = .events
        let count = kevent(events, nil, 0, &event, 1, &zero)
        guard count == 0 else {
            if count == 1 {
                switch event.fflags {
                case UInt32(NOTE_ATTRIB):
                    if event.ident == directories.last.map({ UInt($0.fd) }) {
                        checkpoint = .stageAttribute
                    } else if event.ident == directories.dropLast().last.map({ UInt($0.fd) }) {
                        checkpoint = .supportAttribute
                    } else {
                        checkpoint = .ancestorAttribute
                    }
                case UInt32(NOTE_RENAME): checkpoint = .renameEvent
                case UInt32(NOTE_DELETE): checkpoint = .deleteEvent
                case UInt32(NOTE_REVOKE): checkpoint = .revokeEvent
                default: checkpoint = .otherEvent
                }
            }
            throw AgentFailure.storage
        }
        checkpoint = .ready
    }
    func send(_ value: [String: Any]) throws {
        try check()
        let bytes = try JSONSerialization.data(withJSONObject: value.merging(["bytes": 0]) { _, new in new }, options: [.sortedKeys]) + Data([10])
        guard bytes.count <= Self.limit else { throw AgentFailure.storage }
        checkpoint = .responseWrite
        try bytes.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let count = Darwin.send(socketFD, raw.baseAddress!.advanced(by: offset), raw.count - offset, 0)
                guard count > 0 else { throw AgentFailure.storage }
                offset += count
            }
        }
        try check()
    }
    func action() throws -> String {
        try check()
        checkpoint = .commandRead
        while !buffered.contains(10) {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let count = chunk.withUnsafeMutableBytes { Darwin.recv(socketFD, $0.baseAddress, $0.count, 0) }
            guard count > 0, count <= Self.limit - buffered.count else { throw AgentFailure.storage }
            buffered.append(contentsOf: chunk.prefix(count))
        }
        let end = buffered.firstIndex(of: 10)!
        let line = Data(buffered[..<end])
        buffered.removeSubrange(...end)
        checkpoint = .commandParse
        let fields = try AgentJSON.parseEvidence(line).record("action bytes")
        guard try fields.integer("bytes") == 0 else { throw AgentFailure.shape }
        try check()
        return try fields.text("action")
    }
    func close() {
        guard !closed else { return }
        closed = true
        if socketFD >= 0 { _ = Darwin.close(socketFD) }
        for directory in directories.reversed() { _ = Darwin.close(directory.fd) }
        directories.removeAll()
        _ = Darwin.close(events)
    }
}
#endif
