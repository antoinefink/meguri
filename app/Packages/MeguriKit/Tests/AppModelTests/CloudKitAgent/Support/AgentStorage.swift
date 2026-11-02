import Foundation
import Darwin

@MainActor protocol AgentStorage: AnyObject {
    func read(_ file: AgentFile) throws -> Data
    func replace(_ file: AgentFile, with bytes: Data) throws
    func appendJournal(_ bytes: Data) throws
    func preserveAndRemoveSuffix(_ suffix: Data, complete: Data) throws
}

nonisolated enum AgentFile: String { case header = "accepted-run.json", commands = "accepted-commands.jsonl", journal = "steps.jsonl"
    case run = "run.json", delivery = "commands.jsonl", retained = "retained-steps.jsonl", status = "agent-status.json" }

// Fixed names, relative to one admitted directory throughout this lifetime.
@MainActor final class AgentFileStorage: AgentStorage {
    struct Hooks {
        var write: ((Int32, Data, Int) -> Int)?
        var flush: ((Int32) -> Int32)?
    }
    private nonisolated final class Descriptor {
        let value: Int32
        init(_ value: Int32) throws {
            guard value >= 0 else { throw AgentFailure.storage }
            self.value = value
        }
        deinit { _ = Darwin.close(value) }
    }
    private static let limit = 64 * 1024 * 1024
    private let directory: Descriptor
    private let hooks: Hooks

    init(directory: URL, hooks: Hooks = .init()) throws {
        let components = directory.pathComponents
        guard directory.path.hasPrefix("/"), directory.path.utf8.count <= 4096,
              components.count <= 128, !components.contains(".."), !components.contains(".") else { throw AgentFailure.storage }
        var admitted = try Descriptor(open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC))
        for component in components.dropFirst() {
            admitted = try Descriptor(openat(admitted.value, component, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC))
        }
        var info = stat()
        // APFS directory link counts do not establish directory identity.
        guard fstat(admitted.value, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
              info.st_uid == getuid(), info.st_mode & 0o022 == 0 else { throw AgentFailure.storage }
        self.directory = admitted
        self.hooks = hooks
    }
    private func regular(_ fd: Int32) throws -> stat {
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_nlink == 1, info.st_uid == getuid(), info.st_mode & 0o022 == 0,
              info.st_size >= 0, info.st_size <= Self.limit else { throw AgentFailure.storage }
        return info
    }
    private func sameEntry(_ name: String, _ info: stat) throws {
        var named = stat()
        guard fstatat(directory.value, name, &named, AT_SYMLINK_NOFOLLOW) == 0,
              named.st_mode & S_IFMT == S_IFREG, named.st_dev == info.st_dev,
              named.st_ino == info.st_ino else { throw AgentFailure.storage }
    }
    private func checkDestination(_ name: String) throws {
        var info = stat()
        if fstatat(directory.value, name, &info, AT_SYMLINK_NOFOLLOW) != 0 {
            guard errno == ENOENT else { throw AgentFailure.storage }
            return
        }
        guard info.st_mode & S_IFMT == S_IFREG, info.st_nlink == 1,
              info.st_uid == getuid(), info.st_mode & 0o022 == 0,
              info.st_size >= 0, info.st_size <= Self.limit else { throw AgentFailure.storage }
    }
    func read(_ file: AgentFile) throws -> Data {
        let fd = openat(directory.value, file.rawValue, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        guard fd >= 0 else { if errno == ENOENT { return Data() }; throw AgentFailure.storage }
        let descriptor = try Descriptor(fd)
        defer { withExtendedLifetime(descriptor) {} }
        let before = try regular(descriptor.value)
        try sameEntry(file.rawValue, before)
        var bytes = Data(), buffer = [UInt8](repeating: 0, count: 65_536)
        while true {
            let count = buffer.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
            guard count >= 0, bytes.count + count <= Self.limit else { throw AgentFailure.storage }
            if count == 0 { break }
            bytes.append(contentsOf: buffer.prefix(count))
        }
        let after = try regular(fd)
        try sameEntry(file.rawValue, after)
        guard before.st_size == after.st_size, bytes.count == after.st_size,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else { throw AgentFailure.storage }
        return bytes
    }
    private func flush(_ fd: Int32) throws {
        guard (hooks.flush?(fd) ?? fsync(fd)) == 0 else { throw AgentFailure.storage }
    }
    private func write(_ bytes: Data, name: String, append: Bool, exclusive: Bool = false) throws {
        guard bytes.count <= Self.limit else { throw AgentFailure.storage }
        let flags = O_WRONLY | O_CREAT | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK | (append ? O_APPEND : 0) | (exclusive ? O_EXCL : 0)
        let descriptor = try Descriptor(openat(directory.value, name, flags, mode_t(0o600)))
        defer { withExtendedLifetime(descriptor) {} }
        let fd = descriptor.value
        let before = try regular(fd)
        try sameEntry(name, before)
        guard let previousSize = Int(exactly: before.st_size) else { throw AgentFailure.storage }
        let (expectedSize, overflow) = (append ? previousSize : 0).addingReportingOverflow(bytes.count)
        guard !overflow, expectedSize <= Self.limit else { throw AgentFailure.storage }
        if !append { guard ftruncate(fd, 0) == 0 else { throw AgentFailure.storage } }
        var offset = 0
        while offset < bytes.count {
            let count = hooks.write?(fd, bytes, offset) ?? bytes.withUnsafeBytes {
                Darwin.write(fd, $0.baseAddress!.advanced(by: offset), $0.count - offset)
            }
            guard count > 0, count <= bytes.count - offset else { throw AgentFailure.storage }
            offset += count
        }
        let after = try regular(fd)
        try sameEntry(name, after)
        guard let actualSize = Int(exactly: after.st_size), actualSize == expectedSize else { throw AgentFailure.storage }
        try flush(fd)
        try flush(directory.value)
    }
    func replace(_ file: AgentFile, with bytes: Data) throws {
        try checkDestination(file.rawValue)
        let name = "pending-" + UUID().uuidString
        defer { _ = unlinkat(directory.value, name, 0) }
        try write(bytes, name: name, append: false, exclusive: true)
        try checkDestination(file.rawValue)
        guard renameat(directory.value, name, directory.value, file.rawValue) == 0 else { throw AgentFailure.storage }
        try flush(directory.value)
    }
    func appendJournal(_ bytes: Data) throws { try write(bytes, name: AgentFile.journal.rawValue, append: true) }
    func preserveAndRemoveSuffix(_ suffix: Data, complete: Data) throws {
        // Keep the interrupted bytes locally before removing the suffix.
        let current = try read(.journal)
        guard current == complete + suffix else { throw AgentFailure.storage }
        try write(suffix, name: "incomplete-" + UUID().uuidString + ".bin", append: false, exclusive: true)
        let descriptor = try Descriptor(openat(directory.value, AgentFile.journal.rawValue, O_WRONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
        defer { withExtendedLifetime(descriptor) {} }
        let info = try regular(descriptor.value)
        try sameEntry(AgentFile.journal.rawValue, info)
        guard info.st_size == current.count, ftruncate(descriptor.value, off_t(complete.count)) == 0 else { throw AgentFailure.storage }
        try flush(descriptor.value)
        try flush(directory.value)
    }
}
