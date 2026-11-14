import Darwin
import Foundation

nonisolated enum DiagnosticReadFailure: Error { case oversized, unavailable, notFound }

/// Authority for this store's directory tree only. Every app-controlled
/// component is opened without following links; children stay relative to
/// retained descriptors. The store retains admitted children between calls;
/// validation rejects pathname replacement before subsequent reads or writes.
nonisolated final class FieldDiagnosticDirectory {
    private let descriptor: Int32
    private let parent: FieldDiagnosticDirectory?
    private let name: String?

    private init(descriptor: Int32, parent: FieldDiagnosticDirectory? = nil, name: String? = nil) {
        self.descriptor = descriptor; self.parent = parent; self.name = name
    }

    deinit { close(descriptor) }

    static func root(_ url: URL) throws -> FieldDiagnosticDirectory {
        guard url.isFileURL else { throw DiagnosticReadFailure.unavailable }
        var components = url.path(percentEncoded: false).split(separator: "/").map(String.init)
        guard !components.isEmpty, !components.contains("..") else { throw DiagnosticReadFailure.unavailable }
        // These aliases belong to the OS, outside the app-controlled tree.
        // Never resolve arbitrary caller-supplied links to establish authority.
        if let first = components.first, ["var", "tmp", "etc"].contains(first) {
            components.insert("private", at: 0)
        }
        let descriptor = open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard descriptor >= 0 else { throw DiagnosticReadFailure.unavailable }
        var directory = FieldDiagnosticDirectory(descriptor: descriptor)
        for component in components {
            directory = try directory.child(component, create: true)
        }
        return directory
    }

    func validate() throws {
        guard let parent, let name else { return }
        try parent.validate()
        var current = stat()
        var retained = stat()
        guard fstat(descriptor, &retained) == 0,
              fstatat(parent.descriptor, name, &current, AT_SYMLINK_NOFOLLOW) == 0,
              (current.st_mode & S_IFMT) == S_IFDIR,
              current.st_dev == retained.st_dev, current.st_ino == retained.st_ino else {
            throw DiagnosticReadFailure.unavailable
        }
    }

    private func validName(_ name: String) throws {
        guard !name.isEmpty, name != ".", name != "..", !name.contains("/"), !name.utf8.contains(0) else {
            throw DiagnosticReadFailure.unavailable
        }
    }

    func child(_ name: String, create: Bool = false) throws -> FieldDiagnosticDirectory {
        try validName(name)
        try validate()
        if create, mkdirat(descriptor, name, 0o700) != 0, errno != EEXIST { throw DiagnosticReadFailure.unavailable }
        let child = openat(descriptor, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard child >= 0 else {
            throw errno == ENOENT ? DiagnosticReadFailure.notFound : DiagnosticReadFailure.unavailable
        }
        let directory = FieldDiagnosticDirectory(descriptor: child, parent: self, name: name)
        try directory.validate()
        return directory
    }

    func folder(_ path: String, create: Bool = false) throws -> FieldDiagnosticDirectory {
        var directory = self
        for component in path.split(separator: "/", omittingEmptySubsequences: false) {
            directory = try directory.child(String(component), create: create)
        }
        return directory
    }

    func read(_ name: String, limit: Int? = nil) throws -> Data {
        try validName(name)
        try validate()
        let file = openat(descriptor, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard file >= 0 else {
            throw errno == ENOENT ? DiagnosticReadFailure.notFound : DiagnosticReadFailure.unavailable
        }
        let handle = FileHandle(fileDescriptor: file, closeOnDealloc: true)
        defer { try? handle.close() }
        var facts = stat()
        guard fstat(file, &facts) == 0, (facts.st_mode & S_IFMT) == S_IFREG else { throw DiagnosticReadFailure.unavailable }
        let bytes: Data
        if let limit {
            guard facts.st_size >= 0, facts.st_size <= limit else { throw DiagnosticReadFailure.oversized }
            bytes = try handle.read(upToCount: limit + 1) ?? Data()
            guard bytes.count <= limit else { throw DiagnosticReadFailure.oversized }
        } else {
            bytes = try handle.readToEnd() ?? Data()
        }
        try validate()
        return bytes
    }

    /// Each scan opens its own stream; callers may cap the number of names.
    func names(limit: Int? = nil) throws -> [String] {
        try validate()
        let scan = openat(descriptor, ".", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard scan >= 0 else { throw DiagnosticReadFailure.unavailable }
        guard let stream = fdopendir(scan) else { close(scan); throw DiagnosticReadFailure.unavailable }
        defer { closedir(stream) }
        var names: [String] = []
        while limit.map({ names.count < $0 }) ?? true {
            errno = 0
            guard let entry = readdir(stream) else {
                guard errno == 0 else { throw DiagnosticReadFailure.unavailable }
                break
            }
            let capacity = MemoryLayout.size(ofValue: entry.pointee.d_name)
            let name = withUnsafePointer(to: &entry.pointee.d_name) {
                $0.withMemoryRebound(to: CChar.self, capacity: capacity) { String(cString: $0) }
            }
            if name != ".", name != ".." { names.append(name) }
        }
        try validate()
        return names
    }

    /// Stage in the retained directory, then publish relative to that same
    /// descriptor. A destination link is never opened or followed.
    func write(_ bytes: Data, name: String, replacing: Bool) throws {
        try validName(name)
        try validate()
        let staging = ".\(UUID().uuidString).tmp"
        let file = openat(descriptor, staging, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard file >= 0 else { throw DiagnosticReadFailure.unavailable }
        let handle = FileHandle(fileDescriptor: file, closeOnDealloc: true)
        defer { try? handle.close(); unlinkat(descriptor, staging, 0) }
        try handle.write(contentsOf: bytes)
        try validate()
        if replacing {
            var facts = stat()
            let found = fstatat(descriptor, name, &facts, AT_SYMLINK_NOFOLLOW)
            guard (found == 0 && (facts.st_mode & S_IFMT) == S_IFREG) || (found != 0 && errno == ENOENT) else {
                throw DiagnosticReadFailure.unavailable
            }
            guard renameat(descriptor, staging, descriptor, name) == 0 else { throw DiagnosticReadFailure.unavailable }
        } else {
            guard linkat(descriptor, staging, descriptor, name, 0) == 0 else { throw DiagnosticReadFailure.unavailable }
        }
        try validate()
    }
}
