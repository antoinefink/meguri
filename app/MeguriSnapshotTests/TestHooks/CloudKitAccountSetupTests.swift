#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
@testable import Meguri
import AppModel
import CloudKit
import CoreBridge
import Darwin
import Foundation
import XCTest

// Explicit owner selection only. Never include this class in a normal plan's
// selectedTests; Snapshot excludes it. Failure messages are fixed and no test
// attachment, account value, SDK description or private path is recorded.
@MainActor final class CloudKitAccountSetupTests: XCTestCase {
    func testOwnerSetup() async {
        guard CloudAccountSetupStartup.requested(), CloudAccountSetupStartup.excludedNormalStartup else {
            // An accidental normal selection cannot perform any account lookup.
            XCTFail("Owner setup requires its separate startup selection")
            return
        }
        do {
            guard let configuration = CloudBuildConfiguration.fromBundle,
                  case .development = configuration.environment, configuration.schemaConfigured,
                  let path = ProcessInfo.processInfo.environment["MEGURI_CLOUD_ACCOUNT_DIRECTORY"] else {
                throw AgentFailure.binding
            }
            let original = try AgentRunningProductInspector.observe()
            let channel = try OwnerSetupChannel(path: path)
            defer { channel.close() }
            func observation() throws -> [String: Any] {
                try channel.check()
                guard try AgentRunningProductInspector.observe() == original,
                      let current = CloudBuildConfiguration.fromBundle,
                      current.container == configuration.container, current.environment == configuration.environment,
                      current.schemaConfigured == configuration.schemaConfigured else { throw AgentFailure.binding }
                return try OwnerSetupObservation.encode(original)
            }
            try channel.send(["pid": getpid(), "container": configuration.container, "observation": observation()])
            // Only the original parent, after actual product/signature/profile,
            // destination and loaded-image admission, sends this first command.
            guard try channel.action() == "capture" else { throw AgentFailure.binding }
            let container = CKContainer(identifier: configuration.container)
            func read() async throws -> [String: Any] {
                _ = try observation()
                var status = "failed", identity: [UInt8] = []
                do {
                    switch try await container.accountStatus() {
                    case .available:
                        let record = try await container.userRecordID()
                        identity = Array(record.recordName.utf8) // Exact V9.1 encoding.
                        guard !identity.isEmpty else { throw AgentFailure.binding }
                        status = "available"
                    case .noAccount: status = "no-account"
                    default: status = "unavailable"
                    }
                } catch {
                    // SDK errors and any private markers never reach XCTest.
                    status = "failed"; identity = []
                }
                try Task.checkCancellation()
                return ["status": status, "identity": identity, "observation": try observation()]
            }
            try channel.send(try await read())
            guard try channel.action() == "fresh" else { throw AgentFailure.binding }
            try channel.send(try await read())
            while true {
                switch try channel.action() {
                case "hold": try channel.send(["observation": observation()])
                case "finish":
                    _ = try observation()
                    try channel.send(["finished": true])
                    return
                default: throw AgentFailure.binding
                }
            }
        } catch {
            XCTFail("Owner setup refused; no signed execution evidence")
        }
    }
}

// Same actual loaded-product observer as preflight/normal CloudAgent, with the
// same bounded private observation consumed by admit_preflight on the parent.
@MainActor private enum OwnerSetupObservation {
    static func encode(_ observation: AgentRunningProductObservation) throws -> [String: Any] {
        guard case .mac(let os) = observation.os, let main = observation.images.first else { throw AgentFailure.binding }
        return try AgentRunDirectory.withOSApplicationSupportRoot(create: true) { root, chain in
            func bundle(_ value: AgentRunningProductObservation.BundleIdentity) -> [String: String] {
                ["root": value.root, "canonical_root": value.canonicalRoot,
                 "bundle_id": value.bundleID, "executable": value.executable]
            }
            let images: [[String: Any]] = observation.images.filter {
                $0.path.hasPrefix(observation.host.root + "/") || $0.path.hasPrefix(observation.host.canonicalRoot + "/")
            }.map {
                ["path": $0.path, "cpu": $0.identity.cpu, "subtype": $0.identity.subtype,
                 "uuid": $0.identity.uuid.map { String(format: "%02x", $0) }.joined()]
            }
            guard !images.isEmpty, images.count <= 128 else { throw AgentFailure.binding }
            return ["protocol_version": 1, "scope": "PATH-PREPARATION-ONLY",
                "application_support": root.path, "application_support_chain": chain,
                "host": bundle(observation.host), "agent": bundle(observation.agent),
                "app_build": observation.appBuild, "os_build": os, "executable_path": observation.executablePath,
                "anchor_image_path": observation.anchorImagePath, "main_image_path": main.path, "images": images]
        }
    }
}

// A single live socket in this app's OS-derived support root. Candidates live
// only in memory/kernel buffers. Relative connect avoids macOS sun_path limits
// without accepting another root or placing a socket outside the sandbox.
@MainActor private final class OwnerSetupChannel {
    private struct Pin: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, mode: mode_t
        init(_ value: stat) { device = value.st_dev; inode = value.st_ino; owner = value.st_uid; mode = value.st_mode }
    }
    private struct Directory { let parent: Int32?, name: String, fd: Int32, pin: Pin }
    private var directories: [Directory] = []
    private var socketFD: Int32 = -1
    private var socketPin: Pin?
    private var closed = false
    private let path: String
    private let events: Int32
    private var buffered = Data()
    private static let limit = 128 * 1024

    init(path: String) throws {
        self.path = path
        events = kqueue()
        guard events >= 0 else { throw AgentFailure.storage }
        do {
            let root = try AgentRunDirectory.osApplicationSupportRoot()
            guard path.hasPrefix(root.path + "/.meguri-account-"),
                  String(path.dropFirst(root.path.count + 1)).range(of: #"^\.meguri-account-[0-9a-f]{32}$"#,
                       options: .regularExpression) != nil else { throw AgentFailure.storage }
            var parent: Int32?
            for name in ["/"] + path.split(separator: "/").map(String.init) {
                let fd = parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK) }
                    ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                guard fd >= 0 else { throw AgentFailure.storage }
                var info = stat()
                guard fstat(fd, &info) == 0 else { _ = Darwin.close(fd); throw AgentFailure.storage }
                directories.append(Directory(parent: parent, name: name, fd: fd, pin: Pin(info)))
                var event = kevent(ident: UInt(fd), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                    fflags: UInt32(NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE | NOTE_ATTRIB), data: 0, udata: nil)
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
        guard !closed, try AgentRunDirectory.osApplicationSupportRoot().path + "/" + URL(fileURLWithPath: path).lastPathComponent == path else {
            throw AgentFailure.storage
        }
        for directory in directories {
            var held = stat(), named = stat()
            guard fstat(directory.fd, &held) == 0, Pin(held) == directory.pin else { throw AgentFailure.storage }
            let result = directory.parent.map { fstatat($0, directory.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
            guard result == 0, Pin(named) == directory.pin else { throw AgentFailure.storage }
        }
        var named = stat(), event = kevent(), zero = timespec(tv_sec: 0, tv_nsec: 0)
        guard let directory = directories.last, fstatat(directory.fd, "channel", &named, AT_SYMLINK_NOFOLLOW) == 0,
              Pin(named) == socketPin, named.st_nlink == 1,
              kevent(events, nil, 0, &event, 1, &zero) == 0 else { throw AgentFailure.storage }
    }
    func send(_ value: [String: Any]) throws {
        try check()
        let bytes = try JSONSerialization.data(withJSONObject: value.merging(["bytes": 0]) { _, new in new }, options: [.sortedKeys]) + Data([10])
        guard bytes.count <= Self.limit else { throw AgentFailure.storage }
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
        while !buffered.contains(10) {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let count = chunk.withUnsafeMutableBytes { Darwin.recv(socketFD, $0.baseAddress, $0.count, 0) }
            guard count > 0, count <= Self.limit - buffered.count else { throw AgentFailure.storage }
            buffered.append(contentsOf: chunk.prefix(count))
        }
        let end = buffered.firstIndex(of: 10)!
        let line = Data(buffered[..<end])
        buffered.removeSubrange(...end)
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
