import Darwin
import Foundation

// J2.2 only. The caller admits the platform Application Support root; this
// type neither discovers that root nor observes or enrolls a current account.
// Keep this object alive until the caller has finished using the admission.
@MainActor final class AgentExpectedAccounts {
    private nonisolated final class Descriptor {
        let value: Int32
        init(_ value: Int32) throws {
            guard value >= 0 else { throw AgentFailure.storage }
            self.value = value
        }
        deinit { _ = Darwin.close(value) }
    }

    // Directory timestamps, sizes and link counts are deliberately excluded:
    // unrelated APFS directory activity must not invalidate authority.
    private struct DirectoryIdentity: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, group: gid_t, mode: mode_t
        init(_ info: stat) {
            device = info.st_dev; inode = info.st_ino; owner = info.st_uid
            group = info.st_gid; mode = info.st_mode
        }
    }
    private struct Directory {
        let descriptor: Descriptor
        let name: String
        let identity: DirectoryIdentity
    }
    private struct Input: Decodable {
        let protocol_version: Int
        let header_sha256: String
        let job: String, run_id: String, source_commit: String, device: String
        let executable_sha256: String, container: String, environment: String, account_scope: String
        let accounts: [String: [UInt8]]
    }

    // Same byte bound as AgentJSON.parse, enforced before allocation/read.
    private static let byteLimit = 1_048_576
    private static let filename = "expected-accounts.json"
    private let events: Descriptor
    private var directories: [Directory] = []
    private var file: Descriptor?
    private var fileInfo: stat?
    private var originalBytes = Data()
    private var accounts: [String: [UInt8]] = [:]
    private var invalidated = false

    // afterRead is a test-only mutation seam, after the first bounded read and
    // before any parsing or successful admission. It receives no private data.
    init(admittedApplicationSupportRoot root: URL, originalHeaderBytes: Data,
         afterRead: (() throws -> Void)? = nil) throws {
        // Validate run/device before they can become path components.
        let header = try AgentHeader.read(originalHeaderBytes)
        events = try Descriptor(kqueue())
        guard fcntl(events.value, F_SETFD, FD_CLOEXEC) == 0 else { throw AgentFailure.storage }
        guard root.isFileURL, root.host == nil || root.host == "" || root.host == "localhost",
              root.path.hasPrefix("/"), !root.path.utf8.contains(0), root.path.utf8.count <= 4096,
              root.pathComponents.count <= 128,
              !root.pathComponents.contains("."), !root.pathComponents.contains("..") else { throw AgentFailure.storage }

        // Absolute ancestors are retained without following aliases. Their
        // platform admission belongs to the caller; private checks start at
        // the supplied root, not at a checkout or a header-selected path.
        try retainDirectory(name: "/", parent: nil, privateDirectory: false, ownedRoot: root.path == "/")
        for (index, component) in root.pathComponents.dropFirst().enumerated() {
            try retainDirectory(name: component, parent: directories.last!.descriptor.value,
                                privateDirectory: false, ownedRoot: index == root.pathComponents.count - 2)
        }
        for component in ["Meguri", "CloudKitSignedPrivate", header.run_id, header.device] {
            try retainDirectory(name: component, parent: directories.last!.descriptor.value,
                                privateDirectory: true, ownedRoot: false)
        }
        try checkDirectories()
        let parent = directories.last!.descriptor.value
        var named = stat()
        if fstatat(parent, Self.filename, &named, AT_SYMLINK_NOFOLLOW) != 0 {
            let absent = errno == ENOENT
            try checkDirectories()
            guard absent, fstatat(parent, Self.filename, &named, AT_SYMLINK_NOFOLLOW) != 0,
                  errno == ENOENT else { throw AgentFailure.storage }
            throw AgentFailure.awaitingUser
        }
        try Self.checkRegular(named)
        let opened = try Descriptor(openat(parent, Self.filename, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
        file = opened
        var info = stat()
        guard fstat(opened.value, &info) == 0 else { throw AgentFailure.storage }
        try Self.checkRegular(info)
        guard Self.sameFile(named, info) else { throw AgentFailure.storage }
        fileInfo = info
        originalBytes = try readFile(opened.value)
        // Arbitrary errors from a synthetic mutation are also content-free.
        do { try afterRead?() } catch { throw AgentFailure.storage }
        try revalidate()

        let fields = try AgentJSON.parse(originalBytes).object(required: [
            "protocol_version", "header_sha256", "job", "run_id", "source_commit", "device",
            "executable_sha256", "container", "environment", "account_scope", "accounts"
        ])
        guard let labels = fields["accounts"] else { throw AgentFailure.shape }
        _ = try labels.object(required: Set(header.account_labels))
        // JSONDecoder accepts integral floating point tokens as integers.
        // Require integer tokens as well as the decoder's UInt8 range check.
        try Self.checkIntegerTokens(originalBytes)
        let input = try AgentJSON.decode(Input.self, originalBytes)
        guard input.protocol_version == 1 else { throw AgentFailure.version }
        guard input.header_sha256 == AgentJSON.hash(originalHeaderBytes), input.job == header.job,
              input.run_id == header.run_id, input.source_commit == header.source_commit,
              input.device == header.device, input.executable_sha256 == header.executable_sha256 else { throw AgentFailure.binding }
        guard input.container == header.container, input.environment == header.environment,
              input.account_scope == header.account_scope else { throw AgentFailure.wrongContainer }
        guard input.accounts.values.allSatisfy({ !$0.isEmpty }) else { throw AgentFailure.shape }
        accounts = input.accounts
        try revalidate()
    }

    // This compares caller-supplied bytes only; it does not establish that a
    // core/current-account observation or database-start fence has occurred.
    func requireMatch(_ actualIdentity: [UInt8], label: String) throws {
        try revalidate()
        guard ["acct1", "acct2"].contains(label), let expected = accounts[label],
              expected == actualIdentity else { throw AgentFailure.wrongContainer }
        try revalidate()
    }

    // Also called by every comparison. Later runtime owners may call this at
    // their own lifetime boundaries; all descriptors remain retained.
    func revalidate() throws {
        do {
            guard !invalidated, let file, let fileInfo else { throw AgentFailure.storage }
            try checkDirectories()
            try checkFile(file.value, against: fileInfo)
            guard try readFile(file.value) == originalBytes else { throw AgentFailure.storage }
            try checkFile(file.value, against: fileInfo)
            try checkDirectories()
        } catch {
            invalidated = true
            throw AgentFailure.storage
        }
    }

    private func retainDirectory(name: String, parent: Int32?, privateDirectory: Bool, ownedRoot: Bool) throws {
        let fd: Int32
        if let parent {
            fd = openat(parent, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        } else {
            fd = open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        }
        if fd < 0 {
            let absent = errno == ENOENT
            try checkDirectories()
            var named = stat()
            if privateDirectory, absent, let parent,
               fstatat(parent, name, &named, AT_SYMLINK_NOFOLLOW) != 0, errno == ENOENT {
                throw AgentFailure.awaitingUser
            }
            throw AgentFailure.storage
        }
        let descriptor = try Descriptor(fd)
        // NOTE_RENAME catches rename-away-and-back, even when the final name
        // again reaches the retained inode. No directory timestamp binding.
        var change = kevent(ident: UInt(fd), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                            fflags: UInt32(NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE), data: 0, udata: nil)
        guard kevent(events.value, &change, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else { throw AgentFailure.storage }
        if privateDirectory {
            guard info.st_uid == getuid(), info.st_mode & 0o7777 == 0o700 else { throw AgentFailure.storage }
        } else if ownedRoot {
            guard info.st_uid == getuid(), info.st_mode & 0o022 == 0 else { throw AgentFailure.storage }
        }
        directories.append(Directory(descriptor: descriptor, name: name, identity: DirectoryIdentity(info)))
        try checkDirectories()
    }

    private func checkDirectories() throws {
        for (index, directory) in directories.enumerated() {
            var retained = stat()
            guard fstat(directory.descriptor.value, &retained) == 0,
                  DirectoryIdentity(retained) == directory.identity else { throw AgentFailure.storage }
            if index > 0 {
                var named = stat()
                guard fstatat(directories[index - 1].descriptor.value, directory.name, &named, AT_SYMLINK_NOFOLLOW) == 0,
                      DirectoryIdentity(named) == directory.identity else { throw AgentFailure.storage }
            }
        }
        var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
        guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
    }

    private static func checkRegular(_ info: stat) throws {
        guard info.st_mode & S_IFMT == S_IFREG, info.st_mode & 0o7777 == 0o600,
              info.st_uid == getuid(), info.st_nlink == 1,
              info.st_size >= 0, info.st_size <= byteLimit else { throw AgentFailure.storage }
    }

    private static func sameFile(_ lhs: stat, _ rhs: stat) -> Bool {
        lhs.st_dev == rhs.st_dev && lhs.st_ino == rhs.st_ino && lhs.st_uid == rhs.st_uid &&
        lhs.st_gid == rhs.st_gid && lhs.st_mode == rhs.st_mode && lhs.st_nlink == rhs.st_nlink &&
        lhs.st_size == rhs.st_size && lhs.st_mtimespec.tv_sec == rhs.st_mtimespec.tv_sec &&
        lhs.st_mtimespec.tv_nsec == rhs.st_mtimespec.tv_nsec && lhs.st_ctimespec.tv_sec == rhs.st_ctimespec.tv_sec &&
        lhs.st_ctimespec.tv_nsec == rhs.st_ctimespec.tv_nsec
    }

    private func checkFile(_ fd: Int32, against original: stat) throws {
        var retained = stat(), named = stat()
        guard fstat(fd, &retained) == 0,
              fstatat(directories.last!.descriptor.value, Self.filename, &named, AT_SYMLINK_NOFOLLOW) == 0 else { throw AgentFailure.storage }
        try Self.checkRegular(retained)
        try Self.checkRegular(named)
        guard Self.sameFile(original, retained), Self.sameFile(original, named) else { throw AgentFailure.storage }
    }

    private func readFile(_ fd: Int32) throws -> Data {
        var bytes = Data(), buffer = [UInt8](repeating: 0, count: 16_384)
        while true {
            let count = buffer.withUnsafeMutableBytes { pread(fd, $0.baseAddress, $0.count, off_t(bytes.count)) }
            guard count >= 0, count <= Self.byteLimit - bytes.count else { throw AgentFailure.storage }
            if count == 0 { break }
            bytes.append(contentsOf: buffer.prefix(count))
        }
        guard let fileInfo, bytes.count == fileInfo.st_size else { throw AgentFailure.storage }
        return bytes
    }

    // AgentJSON already established valid JSON and unique decoded keys. The
    // protected schema has numbers only in protocol_version and identity arrays.
    private static func checkIntegerTokens(_ data: Data) throws {
        let bytes = Array(data)
        var index = 0, quoted = false, escaped = false
        while index < bytes.count {
            let byte = bytes[index]
            if quoted {
                if escaped { escaped = false }
                else if byte == 92 { escaped = true }
                else if byte == 34 { quoted = false }
                index += 1
            } else if byte == 34 {
                quoted = true; index += 1
            } else if byte == 45 || (48...57).contains(byte) {
                let start = index
                while index < bytes.count && ((48...57).contains(bytes[index]) || [43, 45, 46, 69, 101].contains(bytes[index])) {
                    index += 1
                }
                guard bytes[start..<index].allSatisfy({ (48...57).contains($0) }) else { throw AgentFailure.shape }
            } else { index += 1 }
        }
    }
}
