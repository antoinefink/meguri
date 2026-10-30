import Foundation
import Synchronization

/// Only this run's copied SDK file is removed. Rust uploads have a separate
/// durable owner and never enter this native temporary-file lease.
public final class CloudMediaStagedAsset: Sendable {
    public let url: URL
    private let release: @Sendable () -> Void
    init(url: URL, release: @escaping @Sendable () -> Void) { self.url = url; self.release = release }
    deinit { release() }
}

/// Files live on the caller's media volume. The executor admits one read at a
/// time; this shared reservation also bounds multiple users of the same seam.
public final class CloudMediaStaging: Sendable {
    static let assetLimit: UInt64 = 100 * 1_024 * 1_024
    static let reserve: UInt64 = 512 * 1_024 * 1_024
    static let stagedLimit: UInt64 = 256 * 1_024 * 1_024
    private let directory: URL
    private let available: @Sendable () throws -> UInt64
    private struct State { var bytes: UInt64 = 0; var active = 0 }
    private let state = Mutex(State())
    public init(directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory
        available = {
            let values = try directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            guard let capacity = values.volumeAvailableCapacityForImportantUsage, capacity >= 0 else { throw CloudMediaFailure.noSpace }
            return UInt64(capacity)
        }
    }
    init(directory: URL, available: @Sendable @escaping () throws -> UInt64) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory; self.available = available
    }
    func copy(_ source: URL, cancelled: @Sendable () -> Bool) throws -> CloudMediaStagedAsset {
        if cancelled() { throw CancellationError() }
        let values = try source.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true, let length = values.fileSize, length > 0 else { throw CloudMediaFailure.invalidAsset }
        let size = UInt64(length)
        guard size <= Self.assetLimit else { throw CloudMediaFailure.assetTooLarge }
        guard try available() >= Self.reserve + size else { throw CloudMediaFailure.noSpace }
        try state.withLock { value in
            guard value.active < 2, value.bytes <= Self.stagedLimit - size else { throw CloudMediaFailure.stagingLimit }
            value.bytes += size; value.active += 1
        }
        let run = directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let target = run.appendingPathComponent("asset")
        var retained = false
        defer {
            if !retained {
                try? FileManager.default.removeItem(at: run)
                state.withLock { $0.bytes -= size; $0.active -= 1 }
            }
        }
        try FileManager.default.createDirectory(at: run, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        guard FileManager.default.createFile(atPath: target.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw CloudMediaFailure.invalidAsset }
        let input = try FileHandle(forReadingFrom: source)
        defer { try? input.close() }
        let output = try FileHandle(forWritingTo: target)
        defer { try? output.close() }
        var count: UInt64 = 0
        while true {
            if cancelled() { throw CancellationError() }
            guard let chunk = try input.read(upToCount: 65_536), !chunk.isEmpty else { break }
            count += UInt64(chunk.count)
            guard count <= Self.assetLimit, count <= size else { throw CloudMediaFailure.assetTooLarge }
            guard try available() >= Self.reserve else { throw CloudMediaFailure.noSpace }
            try output.write(contentsOf: chunk)
        }
        guard count == size else { throw CloudMediaFailure.invalidAsset }
        try output.synchronize()
        retained = true
        return CloudMediaStagedAsset(url: target) { [self] in
            try? FileManager.default.removeItem(at: run)
            state.withLock { $0.bytes -= size; $0.active -= 1 }
        }
    }
}
