import Foundation
import Synchronization

/// SDK source failures allow pa retry; destination failures still stop intake.
enum CloudMediaSourceFailure: Error {
    case unavailable(underlying: any Error)
    case shortRead
    case invalidShape
    case invalidSize
}

/// Only this run's copied SDK file is removed. Rust uploads have a separate
/// durable owner and never enter this native temporary-file lease.
public final class CloudMediaStagedAsset: Sendable {
    public let url: URL
    private let release: @Sendable () -> Void
    init(url: URL, release: @escaping @Sendable () -> Void) { self.url = url; self.release = release }
    deinit { release() }
}

/// Files live on the caller's media volume. Production allocators share byte
/// and count reservations while retaining their own directories and leases.
public final class CloudMediaStaging: Sendable {
    final class Reservations: Sendable {
        fileprivate struct State { var bytes: UInt64 = 0; var active = 0 }
        fileprivate let state = Mutex(State())
    }
    private static let productionReservations = Reservations()
    private static let shared = Mutex<CloudMediaStaging?>(nil)
    static func sharedDefault() throws -> CloudMediaStaging {
        try initializedDefault(in: shared) {
            try CloudMediaStaging(directory: FileManager.default.temporaryDirectory
                .appendingPathComponent("MeguriCloudPayloads", isDirectory: true))
        }
    }
    // Cache only a successful local initialization. A later caller can try again
    // after a filesystem failure; no CloudKit operation is submitted here.
    static func initializedDefault(in cache: borrowing Mutex<CloudMediaStaging?>,
                                   create: () throws -> CloudMediaStaging) throws -> CloudMediaStaging {
        try cache.withLock { value in
            if let value { return value }
            let staging = try create()
            value = staging
            return staging
        }
    }
    static let assetLimit: UInt64 = 100 * 1_024 * 1_024
    static let reserve: UInt64 = 512 * 1_024 * 1_024
    static let stagedLimit: UInt64 = 256 * 1_024 * 1_024
    private let directory: URL
    private let available: @Sendable () throws -> UInt64
    enum SourceReadBoundary { case open, read(FileHandle) }
    // Internal fault seam: observes real source operations, never supplies bytes.
    private let beforeSourceRead: (@Sendable (URL, SourceReadBoundary) throws -> Void)?
    private let reservations: Reservations
    public init(directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory
        reservations = Self.productionReservations
        beforeSourceRead = nil
        available = {
            let values = try directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            guard let capacity = values.volumeAvailableCapacityForImportantUsage, capacity >= 0 else { throw CloudMediaFailure.noSpace }
            return UInt64(capacity)
        }
    }
    // Injected filesystem tests get an isolated owner unless they explicitly
    // share one to exercise reservation accounting across directories.
    init(directory: URL, available: @Sendable @escaping () throws -> UInt64,
         reservations: Reservations = Reservations(),
         beforeSourceRead: (@Sendable (URL, SourceReadBoundary) throws -> Void)? = nil) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory; self.available = available
        self.reservations = reservations
        self.beforeSourceRead = beforeSourceRead
    }
    func copy(_ source: URL, cancelled: @Sendable () -> Bool, retainedAssetLimit: Int = 2) throws -> CloudMediaStagedAsset {
        do {
            return try copyPayload(source, cancelled: cancelled, retainedAssetLimit: retainedAssetLimit)
        } catch let error as CloudMediaSourceFailure {
            // Existing media callers retain their validation and raw I/O errors.
            switch error {
            case .unavailable(let underlying): throw underlying
            case .invalidShape, .shortRead: throw CloudMediaFailure.invalidAsset
            case .invalidSize: throw CloudMediaFailure.assetTooLarge
            }
        }
    }

    // Fetched pa intake distinguishes source failures from destination failures.
    func copyPayload(_ source: URL, cancelled: @Sendable () -> Bool, retainedAssetLimit: Int = 2) throws -> CloudMediaStagedAsset {
        if cancelled() { throw CancellationError() }
        let values = try sourceOperation {
            try source.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        }
        guard values.isRegularFile == true, values.isSymbolicLink != true, let length = values.fileSize, length > 0 else { throw CloudMediaSourceFailure.invalidShape }
        let size = UInt64(length)
        guard size <= Self.assetLimit else { throw CloudMediaSourceFailure.invalidSize }
        guard try available() >= Self.reserve + size else { throw CloudMediaFailure.noSpace }
        try reservations.state.withLock { value in
            guard value.active < retainedAssetLimit, value.bytes <= Self.stagedLimit - size else { throw CloudMediaFailure.stagingLimit }
            value.bytes += size; value.active += 1
        }
        let run = directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let target = run.appendingPathComponent("asset")
        var retained = false
        defer {
            if !retained {
                try? FileManager.default.removeItem(at: run)
                reservations.state.withLock { $0.bytes -= size; $0.active -= 1 }
            }
        }
        try FileManager.default.createDirectory(at: run, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        guard FileManager.default.createFile(atPath: target.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw CloudMediaFailure.invalidAsset }
        let input = try sourceOperation {
            try beforeSourceRead?(source, .open)
            return try FileHandle(forReadingFrom: source)
        }
        defer { try? input.close() }
        let output = try FileHandle(forWritingTo: target)
        defer { try? output.close() }
        var count: UInt64 = 0
        while true {
            if cancelled() { throw CancellationError() }
            let next = try sourceOperation {
                try beforeSourceRead?(source, .read(input))
                return try input.read(upToCount: 65_536)
            }
            guard let chunk = next, !chunk.isEmpty else { break }
            count += UInt64(chunk.count)
            guard count <= Self.assetLimit, count <= size else { throw CloudMediaSourceFailure.invalidSize }
            guard try available() >= Self.reserve else { throw CloudMediaFailure.noSpace }
            try output.write(contentsOf: chunk)
        }
        guard count == size else { throw CloudMediaSourceFailure.shortRead }
        try output.synchronize()
        try output.close()
        retained = true
        return CloudMediaStagedAsset(url: target) { [self] in
            try? FileManager.default.removeItem(at: run)
            reservations.state.withLock { $0.bytes -= size; $0.active -= 1 }
        }
    }

    private func sourceOperation<T>(_ operation: () throws -> T) throws -> T {
        do { return try operation() }
        catch is CancellationError { throw CancellationError() }
        catch { throw CloudMediaSourceFailure.unavailable(underlying: error) }
    }
}
