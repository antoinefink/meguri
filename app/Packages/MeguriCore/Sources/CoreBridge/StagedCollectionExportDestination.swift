import Darwin
import Dispatch
import Foundation
import Synchronization

/// The seam covers only publication; tests still exercise real staging and files.
protocol CollectionExportPublication: Sendable {
    func publish(_ staged: URL, to destination: URL, replacing: Bool) throws
}
struct FoundationCollectionExportPublication: CollectionExportPublication {
    func publish(_ staged: URL, to destination: URL, replacing: Bool) throws {
        if replacing {
            _ = try FileManager().replaceItemAt(destination, withItemAt: staged)
        } else {
            // RENAME_EXCL makes the same-volume move atomic and refuses a new
            // concurrent item, including a dangling symlink. Never fall back to copy.
            let status: Int32 = staged.withUnsafeFileSystemRepresentation { source in
                guard let source else { errno = EINVAL; return -1 }
                return destination.withUnsafeFileSystemRepresentation { target in
                    guard let target else { errno = EINVAL; return -1 }
                    return renameatx_np(AT_FDCWD, source, AT_FDCWD, target, UInt32(RENAME_EXCL))
                }
            }
            if status != 0 { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
        }
    }
}

private final class StagedExportResult: Sendable {
    let value = Mutex<Result<Void, any Error>?>(nil)
}

/// Called inside synchronous destination coordination. Neither staging nor final
/// publication runs on the main actor; Rust owns the bounded copy/hash/full sync.
final class StagedCollectionExportDestination: Sendable {
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.export-publication", qos: .userInitiated)
    private let publication: any CollectionExportPublication
    init(publication: any CollectionExportPublication = FoundationCollectionExportPublication()) {
        self.publication = publication
    }
    func save(to destination: URL, operation: any CollectionExportCancellationChecking, deckPackage: Bool = false,
              beforePublication: @escaping @Sendable () async -> Void = {},
              stage: @escaping @Sendable (URL) async throws -> Void) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            queue.async { [publication] in
                do {
                    try operation.check()
                    let files = FileManager()
                    let replacing: Bool
                    do {
                        _ = try files.attributesOfItem(atPath: destination.path)
                        replacing = true
                    } catch {
                        let failure = error as NSError
                        guard failure.domain == NSCocoaErrorDomain,
                              [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(failure.code) else { throw error }
                        replacing = false
                    }
                    let directory = try files.url(for: .itemReplacementDirectory, in: .userDomainMask,
                                                  appropriateFor: destination, create: true)
                    let staged = directory.appendingPathComponent(deckPackage ? "Meguri-Deck.apkg" : "Meguri-Collection.colpkg")
                    var retainRecovery = false
                    defer {
                        if !retainRecovery {
                            // This unique directory is ours. No publication API has
                            // failed ambiguously on this path when cleanup is allowed.
                            try? files.removeItem(at: directory)
                        }
                    }
                    let result = StagedExportResult()
                    let finished = DispatchSemaphore(value: 0)
                    Task {
                        defer { finished.signal() }
                        do {
                            try await stage(staged)
                            await beforePublication()
                            result.value.withLock { $0 = .success(()) }
                        }
                        catch { result.value.withLock { $0 = .failure(error) } }
                    }
                    finished.wait() // Actual worker completion, including cancellation/draining.
                    try result.value.withLock { $0! }.get()
                    try operation.check() // Last cancellation boundary before native publication.
                    do {
                        try publication.publish(staged, to: destination, replacing: replacing)
                    } catch {
                        let failure = error as NSError
                        if !replacing && failure.domain == NSPOSIXErrorDomain && failure.code == Int(EEXIST) {
                            throw error // Exclusive rename refused without touching either item.
                        }
                        // Replacement can move the original before failing. A failed
                        // new-file completion may also have published. Keep all recovery.
                        retainRecovery = true
                        throw ExportDestinationCompletionUncertain(destination: destination, recoveryDirectory: directory,
                                                                   underlyingError: error)
                    }
                    // Success wins cancellation during publication. Remove only an
                    // empty directory; preserve any platform-created recovery contents.
                    retainRecovery = true
                    if (try? files.contentsOfDirectory(atPath: directory.path).isEmpty) == true {
                        try? files.removeItem(at: directory)
                    }
                    continuation.resume()
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
}
