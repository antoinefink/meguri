#if os(macOS)
import Darwin
import Foundation
import Testing

@MainActor @Suite(.serialized) struct OwnerSetupChannelTests {
    private struct Run { let root: URL, stage: URL }

    private func server(at stage: URL) throws -> Int32 {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw AgentFailure.storage }
        do {
            let previous = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC)
            guard previous >= 0 else { throw AgentFailure.storage }
            defer { _ = fchdir(previous); _ = Darwin.close(previous) }
            guard chdir(stage.path) == 0 else { throw AgentFailure.storage }
            var address = sockaddr_un()
            address.sun_family = sa_family_t(AF_UNIX)
            address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
            withUnsafeMutableBytes(of: &address.sun_path) { bytes in
                _ = bytes.initializeMemory(as: UInt8.self, repeating: 0)
                for (index, byte) in "channel".utf8.enumerated() { bytes[index] = byte }
            }
            let bound = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
                }
            }
            guard bound == 0, chmod("channel", 0o600) == 0, listen(fd, 1) == 0 else { throw AgentFailure.storage }
            return fd
        } catch {
            _ = Darwin.close(fd)
            throw error
        }
    }

    private func withRun(_ body: (Run, OwnerSetupChannel) throws -> Void) throws {
        guard let canonical = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
        defer { free(canonical) }
        let root = URL(fileURLWithPath: String(cString: canonical), isDirectory: true)
            .appendingPathComponent("meguri-owner-channel-" + UUID().uuidString, isDirectory: true)
        guard mkdir(root.path, 0o700) == 0 else { throw AgentFailure.storage }
        defer { try? FileManager.default.removeItem(at: root) }
        let leaf = ".meguri-account-" + UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
        let stage = root.appendingPathComponent(leaf, isDirectory: true)
        guard mkdir(stage.path, 0o700) == 0 else { throw AgentFailure.storage }
        let listener = try server(at: stage)
        defer { _ = Darwin.close(listener) }
        let channel = try OwnerSetupChannel.generatedUnitOnly(path: stage.path, trustedRoot: { root })
        defer { channel.close() }
        try body(Run(root: root, stage: stage), channel)
    }

    @Test func osAncestorMetadataUpdatesKeepCurrentAuthority() throws {
        try withRun { run, channel in
            try channel.check()
            #expect(chmod(run.root.path, 0o700) == 0)
            #expect(utimes(run.root.path, nil) == 0)
            try channel.check()
            try channel.check()
        }
    }

    @Test func changedAncestorPermissionsRefuseAndCannotBeFreshened() throws {
        try withRun { run, channel in
            #expect(chmod(run.root.path, 0o755) == 0)
            defer { _ = chmod(run.root.path, 0o700) }
            #expect(throws: AgentFailure.storage) { try channel.check() }
            #expect(chmod(run.root.path, 0o700) == 0)
            #expect(throws: AgentFailure.storage) { try channel.check() }
        }
    }

    @Test func privateStagePermissionChangeAndRestoreStillRefuses() throws {
        try withRun { run, channel in
            #expect(chmod(run.stage.path, 0o755) == 0)
            #expect(chmod(run.stage.path, 0o700) == 0)
            #expect(throws: AgentFailure.storage) { try channel.check() }
            #expect(throws: AgentFailure.storage) { try channel.check() }
        }
    }

    @Test func privateStageMetadataEventStillRefuses() throws {
        try withRun { run, channel in
            #expect(utimes(run.stage.path, nil) == 0)
            #expect(throws: AgentFailure.storage) { try channel.check() }
        }
    }

    @Test func ancestorRenameAndRestoreStillRefuses() throws {
        try withRun { run, channel in
            let moved = run.root.appendingPathExtension("moved")
            guard rename(run.root.path, moved.path) == 0 else { throw AgentFailure.storage }
            guard rename(moved.path, run.root.path) == 0 else {
                _ = rename(moved.path, run.root.path)
                throw AgentFailure.storage
            }
            #expect(throws: AgentFailure.storage) { try channel.check() }
        }
    }

    @Test func socketReplacementCannotChangeTheRetainedChannel() throws {
        try withRun { run, channel in
            let socket = run.stage.appendingPathComponent("channel")
            let saved = run.stage.appendingPathComponent("channel.saved")
            guard rename(socket.path, saved.path) == 0 else { throw AgentFailure.storage }
            let replacement = try server(at: run.stage)
            defer { _ = Darwin.close(replacement) }
            #expect(throws: AgentFailure.storage) { try channel.check() }
            #expect(throws: AgentFailure.storage) { try channel.check() }
        }
    }
}
#endif
