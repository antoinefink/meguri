import Darwin
import Foundation

// J2.3 direct-small delivery only. Host artifact paths are metadata. This is
// fixture admission, never product/account admission or an integrity check.
@MainActor final class AgentGeneratedBase {
    private let root: URL, headerBytes: Data, header: AgentHeader
    init(admittedRunDirectory: URL, originalHeaderBytes: Data) throws {
        root = admittedRunDirectory; headerBytes = originalHeaderBytes
        header = try AgentHeader.read(originalHeaderBytes)
    }

    // The engine has flushed initial launch/started before this synchronous
    // boundary. Hooks mutate real files in focused tests; they admit no evidence.
    func requireInstalled(at destination: URL, beforeCopy: (() throws -> Void)? = nil,
                          afterCopy: (() throws -> Void)? = nil) throws {
        guard destination.isFileURL, destination.path == root.path + "/data" else { throw AgentFailure.binding }
        let authority = try GeneratedBaseAuthority(root: root)
        let store = try AgentFileStorage(directory: root)
        guard try store.read(.header) == headerBytes else { throw AgentFailure.binding }
        let commands = AgentJSON.lines(try store.read(.commands))
        guard let launchBytes = commands.complete.first else { throw AgentFailure.journal }
        let launch = try AgentCommand.read(launchBytes, header: AgentJSON.hash(headerBytes), sequence: 1, program: header.program)
        let journal = AgentJSON.lines(try store.read(.journal))
        guard launch.action == .launch, journal.suffix.isEmpty, let first = journal.complete.first else { throw AgentFailure.journal }
        let started = try AgentRecord.read(first)
        guard started.kind == .started, started.action == .launch, started.command_seq == 1,
              started.record_seq == 1, started.header_sha256 == AgentJSON.hash(headerBytes),
              started.command_sha256 == AgentJSON.hash(launchBytes) else { throw AgentFailure.journal }
        let delivery = header.fixture_delivery
        guard delivery.preparation_receipt == nil else { throw AgentFailure.implementationMissing }
        try authority.absent("fixture-evidence/preparation-result.json")
        try authority.absent("fixture-evidence/prepared")
        let frozenBytes = try authority.file("fixture-input.json")
        try Self.matches(frozenBytes, hash: delivery.frozen_input.sha256, bytes: delivery.frozen_input.bytes)
        let frozen = try AgentJSON.parseEvidence(frozenBytes).record("protocol_version run_id device source_commit base parent_files prepared_source files")
        guard try frozen.integer("protocol_version") == 1, try frozen.text("run_id") == header.run_id,
              try frozen.text("device") == header.device, try frozen.text("source_commit") == header.source_commit,
              try frozen.text("base") == delivery.base, try frozen.text("prepared_source") == delivery.source else { throw AgentFailure.binding }
        let parent = try Self.map(frozen.value("parent_files")), files = try Self.map(frozen.value("files"))
        guard parent == files, delivery.manifest.path == "manifest.json" else { throw AgentFailure.binding }
        let media = files.keys.filter { $0.hasPrefix("collection.media/") }
        let required = Set(["collection.anki2", "manifest.json"] + media)
        guard media.count == 5, media.allSatisfy({ $0.split(separator: "/").count == 2 && files[$0]!.bytes == 10_240 }),
              Set(files.keys) == required || Set(files.keys) == required.union(["collection.anki2-wal"]) else { throw AgentFailure.binding }
        let originals = try authority.tree("fixture-evidence/parent", files: files)
        let manifestBytes = originals["manifest.json"]!
        try Self.matches(manifestBytes, hash: delivery.manifest.sha256, bytes: delivery.manifest.bytes)
        let receiptBytes = try authority.file("fixture-evidence/fixtures-result.json")
        let receipt = try AgentReceiptEnvelopeBinding(bytes: receiptBytes, expected: delivery.fixtures_receipt, commit: header.source_commit)
        try receipt.requireArtifact(delivery.source, kind: "fixture")
        try receipt.requireArtifact(delivery.source + "/manifest.json", kind: "manifest")
        try receipt.requireArtifact(receipt.root + "/build/mg/fixtures/" + receipt.runID + "/cloud-small.json", kind: "generation")
        let generationBytes = try authority.file("fixture-evidence/generation.json")
        let generation = try AgentJSON.parseEvidence(generationBytes).dictionary()
        let selected = try receipt.details.list("fixtures").filter {
            let f = try $0.dictionary()
            return try f.text("target") == "cloud-small" && f.text("kind") == "cloud-small"
        }
        guard selected.count == 1 else { throw AgentFailure.binding }
        let entry = try selected[0].dictionary()
        for key in ["request", "manifest", "observed"] {
            guard try Self.equal(entry.value(key), generation.value(key)) else { throw AgentFailure.binding }
        }
        let dir = try generation.text("dir")
        guard dir == delivery.source || (AgentHeader.FixtureDelivery.validPath(dir, absolute: false) && receipt.root + "/" + dir == delivery.source),
              try Self.equal(AgentJSON.parseEvidence(manifestBytes), generation.value("manifest")) else { throw AgentFailure.binding }
        try Self.small(generation)
        // Bind exact original copies as well as the header/frozen input. These
        // digests stay private, outside live data and outside public observations.
        let binding = AgentJSON.hash(Data([headerBytes, frozenBytes, receiptBytes, generationBytes]
            .map(AgentJSON.hash).joined().utf8))
        try authority.revalidate()
        let stateExists = try authority.exists("fixture-install")
        if stateExists {
            _ = try authority.directory("fixture-install")
            let state = try AgentFileStorage(directory: root.appendingPathComponent("fixture-install", isDirectory: true))
            let lines = AgentJSON.lines(try state.read(.journal))
            guard lines.suffix.isEmpty, lines.complete.count == 2 else { throw AgentFailure.recovery }
            let intent = try AgentJSON.parseEvidence(lines.complete[0]).record("binding state")
            let done = try AgentJSON.parseEvidence(lines.complete[1]).record("binding state device inode")
            guard try intent.text("binding") == binding, try intent.text("state") == "intent",
                  try done.text("binding") == binding, try done.text("state") == "complete" else { throw AgentFailure.binding }
            let data = try authority.directory("data")
            guard try done.integer("device") == UInt64(data.st_dev), try done.integer("inode") == UInt64(data.st_ino) else { throw AgentFailure.binding }
            // No original digest comparison against live rows, media or tracking.
            try authority.liveTree()
            try authority.revalidate()
            return
        }
        try authority.absent("data")
        try authority.makeDirectory("fixture-install")
        let state = try AgentFileStorage(directory: root.appendingPathComponent("fixture-install", isDirectory: true))
        let intentBytes = try Self.line(["binding": binding, "state": "intent"])
        try state.appendJournal(intentBytes)
        try beforeCopy?()
        try authority.revalidate()
        try authority.absent("data")
        try authority.makeDirectory("data")
        try authority.makeDirectory("data/collection.media")
        for path in files.keys.sorted() { try authority.writeExclusive("data/" + path, bytes: originals[path]!) }
        try afterCopy?()
        _ = try authority.tree("data", files: files)
        try authority.revalidate()
        let data = try authority.directory("data")
        guard try state.read(.journal) == intentBytes else { throw AgentFailure.recovery }
        let completionBytes = try Self.line(["binding": binding, "state": "complete", "device": UInt64(data.st_dev), "inode": UInt64(data.st_ino)])
        try state.appendJournal(completionBytes)
        guard try state.read(.journal) == intentBytes + completionBytes else { throw AgentFailure.recovery }
        try authority.revalidate()
    }

    private struct Identity: Equatable { let sha256: String, bytes: UInt64 }
    private static func map(_ value: AgentJSON.Value) throws -> [String: Identity] {
        let raw = try value.dictionary()
        guard (7...8).contains(raw.count) else { throw AgentFailure.shape }
        var result: [String: Identity] = [:], total: UInt64 = 0
        for (path, value) in raw {
            guard AgentHeader.FixtureDelivery.validPath(path, absolute: false), path.utf8.count <= 255 else { throw AgentFailure.shape }
            let f = try value.record("sha256 bytes"), hash = try f.text("sha256"), bytes = try f.integer("bytes")
            guard AgentJSON.digest(hash), bytes <= 64 * 1024 * 1024 else { throw AgentFailure.shape }
            total += bytes; result[path] = Identity(sha256: hash, bytes: bytes)
        }
        guard total <= 128 * 1024 * 1024 else { throw AgentFailure.shape }
        return result
    }
    private static func matches(_ data: Data, hash: String, bytes: UInt64) throws {
        guard UInt64(data.count) == bytes, AgentJSON.hash(data) == hash else { throw AgentFailure.binding }
    }
    private static func line(_ value: [String: Any]) throws -> Data {
        var data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]); data.append(10); return data
    }
    private static func equal(_ lhs: AgentJSON.Value, _ rhs: AgentJSON.Value) -> Bool {
        switch (lhs, rhs) {
        case (.object(let a), .object(let b)): return Set(a.keys) == Set(b.keys) && a.allSatisfy { equal($0.value, b[$0.key]!) }
        case (.array(let a), .array(let b)): return a.count == b.count && zip(a, b).allSatisfy { equal($0.0, $0.1) }
        case (.string(let a), .string(let b)), (.number(let a), .number(let b)): return a == b
        case (.boolean(let a), .boolean(let b)): return a == b
        case (.null, .null): return true
        default: return false
        }
    }
    private static func small(_ g: [String: AgentJSON.Value]) throws {
        let r = try g.value("request").record("seed now_ms utc_offset_secs rollover_hour notes cards media media_files media_file_bytes reviews_per_card review_days_before_pinned answer_seconds_after_day_start answer_ordinal_seconds")
        let seed = try r.integer("seed")
        for (key, value) in [
            ("now_ms", 1_735_689_600_000), ("utc_offset_secs", 0), ("rollover_hour", 4), ("notes", 20), ("cards", 20),
            ("media_files", 5), ("media_file_bytes", 10_240), ("reviews_per_card", 3), ("answer_seconds_after_day_start", 28_800)
        ] as [(String, UInt64)] { guard try r.integer(key) == value else { throw AgentFailure.binding } }
        guard try r.text("media") == "files", try r.bool("answer_ordinal_seconds"),
              try r.list("review_days_before_pinned").map({ try $0.integer() }) == [3, 2, 1] else { throw AgentFailure.binding }
        let m = try g.value("manifest").record("generator_version kind name seed cards_requested now_ms utc_offset_secs profile_sha256 worst_case_sha256 media counts dump_sha256")
        guard try m.integer("generator_version") == 5, try m.text("kind") == "cloud-small", try m.text("name") == "cloud-small",
              try m.integer("seed") == seed, try m.integer("cards_requested") == 20, try m.integer("now_ms") == 1_735_689_600_000,
              try m.integer("utc_offset_secs") == 0, try m.text("media") == "files",
              try AgentJSON.digest(m.text("dump_sha256")), try equal(m.value("profile_sha256"), .null),
              try equal(m.value("worst_case_sha256"), .null) else { throw AgentFailure.binding }
        let counts = try m.value("counts").record("notes cards revlog notetypes decks presets tags media_refs media_files media_bytes")
        for (key, value) in [("notes", 20), ("cards", 20), ("revlog", 60), ("notetypes", 1), ("decks", 2),
                                             ("presets", 1), ("tags", 20), ("media_refs", 5), ("media_files", 5), ("media_bytes", 51_200)] as [(String, UInt64)] {
            guard try counts.integer(key) == value else { throw AgentFailure.binding }
        }
        let observed = try g.value("observed").record("counts dump_sha256")
        guard try equal(observed.value("counts"), m.value("counts")), try equal(observed.value("dump_sha256"), m.value("dump_sha256")) else { throw AgentFailure.binding }
    }

    // Confined to these fixed roles and the validated small file map. Product
    // evidence authority deliberately keeps its own four-role allowlist.
    @MainActor private final class GeneratedBaseAuthority {
        private nonisolated final class FD {
            let value: Int32
            init(_ value: Int32) throws { guard value >= 0 else { throw AgentFailure.storage }; self.value = value }
            deinit { _ = Darwin.close(value) }
        }
        private struct Stamp: Equatable {
            let dev: dev_t, ino: ino_t, uid: uid_t, gid: gid_t, mode: mode_t
            init(_ s: stat) { dev = s.st_dev; ino = s.st_ino; uid = s.st_uid; gid = s.st_gid; mode = s.st_mode }
        }
        private struct Dir { let fd: FD, parent: Int32?, name: String, stamp: Stamp }
        private struct File { let fd: FD, parent: Int32, name: String, info: stat, bytes: Data }
        private var dirs: [Dir] = [], paths: [String: Int32] = [:], files: [String: File] = [:]
        private var sets: [String: Set<String>] = [:], absences: Set<String> = []
        private let events: FD
        private var invalid = false
        init(root: URL) throws {
            events = try FD(kqueue())
            guard fcntl(events.value, F_SETFD, FD_CLOEXEC) == 0,
                  root.isFileURL, root.host == nil || root.host == "" || root.host == "localhost",
                  root.path.utf8.count <= 4096, root.pathComponents.count <= 128,
                  AgentHeader.FixtureDelivery.validPath(root.path, absolute: true) else { throw AgentFailure.storage }
            var parent: Int32?
            for name in ["/"] + root.pathComponents.dropFirst() {
                let fd = try FD(parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK) }
                    ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC))
                let s = try info(fd.value, directory: true, owned: name == root.lastPathComponent)
                try watch(fd.value, file: false)
                dirs.append(Dir(fd: fd, parent: parent, name: name, stamp: Stamp(s))); parent = fd.value
            }
            paths[""] = parent!
        }
        private func info(_ fd: Int32, directory: Bool, owned: Bool = true) throws -> stat {
            var s = stat()
            guard fstat(fd, &s) == 0, s.st_mode & S_IFMT == (directory ? S_IFDIR : S_IFREG),
                  !owned || (s.st_uid == getuid() && s.st_mode & 0o022 == 0),
                  directory || (s.st_nlink == 1 && s.st_size >= 0 && s.st_size <= 64 * 1024 * 1024) else { throw AgentFailure.storage }
            if !directory {
                var finder = [UInt8](repeating: 0, count: 32)
                let count = finder.withUnsafeMutableBytes { fgetxattr(fd, "com.apple.FinderInfo", $0.baseAddress, $0.count, 0, 0) }
                guard count == -1 && (errno == ENOATTR || errno == ENOTSUP) || count == 32 && finder[8] & 0x80 == 0 else { throw AgentFailure.storage }
            }
            return s
        }
        private func watch(_ fd: Int32, file: Bool) throws {
            var event = kevent(ident: UInt(fd), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                fflags: UInt32(NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE | NOTE_ATTRIB | (file ? NOTE_WRITE | NOTE_EXTEND | NOTE_LINK : 0)), data: 0, udata: nil)
            guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
        }
        private func parent(_ path: String) throws -> (Int32, String) {
            let url = path.split(separator: "/").map(String.init)
            let prefix = url.dropLast().joined(separator: "/")
            if paths[prefix] == nil { _ = try directory(prefix) }
            return (paths[prefix]!, url.last!)
        }
        @discardableResult func directory(_ path: String) throws -> stat {
            if let fd = paths[path] { return try info(fd, directory: true) }
            let (p, name) = try parent(path)
            let fd = try FD(openat(p, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
            let s = try info(fd.value, directory: true)
            guard !dirs.contains(where: { $0.stamp.dev == s.st_dev && $0.stamp.ino == s.st_ino }) else { throw AgentFailure.storage }
            try watch(fd.value, file: false)
            dirs.append(Dir(fd: fd, parent: p, name: name, stamp: Stamp(s))); paths[path] = fd.value
            return s
        }
        func exists(_ path: String) throws -> Bool {
            let (p, name) = try parent(path); var s = stat()
            if fstatat(p, name, &s, AT_SYMLINK_NOFOLLOW) == 0 { return true }
            guard errno == ENOENT else { throw AgentFailure.storage }; return false
        }
        func absent(_ path: String) throws {
            guard try !exists(path) else { throw AgentFailure.storage }; absences.insert(path)
        }
        func file(_ path: String) throws -> Data {
            if let file = files[path] { return file.bytes }
            let (p, name) = try parent(path)
            let fd = try FD(openat(p, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC))
            let s = try info(fd.value, directory: false)
            guard !files.values.contains(where: { $0.info.st_dev == s.st_dev && $0.info.st_ino == s.st_ino }) else { throw AgentFailure.storage }
            try watch(fd.value, file: true)
            var bytes = Data(), buffer = [UInt8](repeating: 0, count: 65_536)
            while bytes.count <= s.st_size {
                let count = buffer.withUnsafeMutableBytes { pread(fd.value, $0.baseAddress, $0.count, off_t(bytes.count)) }
                guard count >= 0, count <= 64 * 1024 * 1024 - bytes.count else { throw AgentFailure.storage }
                if count == 0 { break }; bytes.append(contentsOf: buffer.prefix(count))
            }
            guard bytes.count == s.st_size else { throw AgentFailure.storage }
            files[path] = File(fd: fd, parent: p, name: name, info: s, bytes: bytes)
            try revalidate(); return bytes
        }
        private func names(_ path: String, limit: Int = 16) throws -> Set<String> {
            _ = try directory(path)
            guard let stream = fdopendir(openat(paths[path]!, ".", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)) else { throw AgentFailure.storage }
            defer { closedir(stream) }
            var result: Set<String> = []
            while true {
                errno = 0
                guard let entry = readdir(stream) else { guard errno == 0 else { throw AgentFailure.storage }; break }
                let decoded = withUnsafePointer(to: &entry.pointee.d_name) {
                    $0.withMemoryRebound(to: CChar.self, capacity: Int(entry.pointee.d_namlen) + 1) { String(validatingCString: $0) }
                }
                guard let name = decoded else { throw AgentFailure.storage }
                if name != "." && name != ".." {
                    guard result.insert(name).inserted else { throw AgentFailure.storage }
                }
                guard result.count <= limit else { throw AgentFailure.storage }
            }
            return result
        }
        func tree(_ path: String, files expected: [String: Identity]) throws -> [String: Data] {
            let rootSet = Set(expected.keys.compactMap { $0.contains("/") ? nil : $0 }).union(["collection.media"])
            let mediaSet = Set(expected.keys.filter { $0.hasPrefix("collection.media/") }.map { String($0.dropFirst("collection.media/".count)) })
            for (dir, expectedSet) in [(path, rootSet), (path + "/collection.media", mediaSet)] {
                guard try names(dir) == expectedSet else { throw AgentFailure.binding }; sets[dir] = expectedSet
            }
            var bytes: [String: Data] = [:]
            for (name, identity) in expected {
                let data = try file(path + "/" + name)
                try AgentGeneratedBase.matches(data, hash: identity.sha256, bytes: identity.bytes); bytes[name] = data
            }
            try revalidate(); return bytes
        }
        func liveTree() throws {
            var count = 0, bytes = 0
            func visit(_ path: String, depth: Int) throws {
                guard depth <= 8 else { throw AgentFailure.storage }
                let entries = try names(path, limit: 256)
                for name in entries {
                    count += 1
                    guard count <= 256 else { throw AgentFailure.storage }
                    let child = path + "/" + name; var s = stat()
                    guard fstatat(paths[path]!, name, &s, AT_SYMLINK_NOFOLLOW) == 0 else { throw AgentFailure.storage }
                    if s.st_mode & S_IFMT == S_IFDIR { try visit(child, depth: depth + 1) }
                    else { bytes += try file(child).count }
                    guard bytes <= 128 * 1024 * 1024 else { throw AgentFailure.storage }
                }
                // Retain the live namespace through this opening boundary only.
                sets[path] = entries
            }
            try visit("data", depth: 0)
            guard files["data/collection.anki2"] != nil else { throw AgentFailure.storage }
        }
        func makeDirectory(_ path: String) throws {
            let (p, name) = try parent(path)
            guard mkdirat(p, name, 0o700) == 0, fsync(p) == 0 else { throw AgentFailure.storage }
            absences.remove(path)
            _ = try directory(path)
        }
        func writeExclusive(_ path: String, bytes: Data) throws {
            let (p, name) = try parent(path)
            let fd = try FD(openat(p, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, mode_t(0o600)))
            var offset = 0
            while offset < bytes.count {
                let count = bytes.withUnsafeBytes { Darwin.write(fd.value, $0.baseAddress!.advanced(by: offset), $0.count - offset) }
                guard count > 0 else { throw AgentFailure.storage }; offset += count
            }
            guard fsync(fd.value) == 0, fsync(p) == 0 else { throw AgentFailure.storage }
        }
        func revalidate() throws {
            do {
                guard !invalid else { throw AgentFailure.storage }
                for dir in dirs {
                    var named = stat()
                    let result = dir.parent.map { fstatat($0, dir.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
                    guard result == 0, Stamp(named) == dir.stamp,
                          Stamp(try info(dir.fd.value, directory: true, owned: false)) == dir.stamp else { throw AgentFailure.storage }
                }
                for file in files.values {
                    let s = try info(file.fd.value, directory: false); var named = stat()
                    guard fstatat(file.parent, file.name, &named, AT_SYMLINK_NOFOLLOW) == 0,
                          Self.same(s, file.info), Self.same(named, file.info) else { throw AgentFailure.storage }
                }
                for path in absences { guard try !exists(path) else { throw AgentFailure.storage } }
                for (path, expected) in sets { guard try names(path, limit: 256) == expected else { throw AgentFailure.storage } }
                var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
                guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
            } catch { invalid = true; throw AgentFailure.storage }
        }
        private static func same(_ a: stat, _ b: stat) -> Bool {
            Stamp(a) == Stamp(b) && a.st_nlink == b.st_nlink && a.st_size == b.st_size &&
            a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec &&
            a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec
        }
    }
}
