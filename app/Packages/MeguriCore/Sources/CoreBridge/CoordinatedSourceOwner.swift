import Dispatch
import Foundation
import Synchronization

/// The accessor is synchronous and owns scope until its body returns. This seam
/// also lets lifetime tests hold an accessor open without a document provider.
public protocol CoordinatedSourceAccess: Sendable {
    func read(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws
}
protocol SourceSecurityScope: Sendable {
    func start(_ url: URL) -> Bool
    func stop(_ url: URL)
}
private struct FoundationSecurityScope: SourceSecurityScope {
    func start(_ url: URL) -> Bool { url.startAccessingSecurityScopedResource() }
    func stop(_ url: URL) { url.stopAccessingSecurityScopedResource() }
}
public struct FoundationSourceAccess: CoordinatedSourceAccess {
    private let scope: any SourceSecurityScope
    public init() { scope = FoundationSecurityScope() }
    init(scope: any SourceSecurityScope) { self.scope = scope }
    public func read(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws {
        let scoped = scope.start(url)
        defer { if scoped { scope.stop(url) } }
        var error: NSError?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &error, byAccessor: accessor)
        if let error { throw error }
    }
}

private final class SourceReadResult<Value: Sendable>: Sendable {
    let value = Mutex<Result<Value, any Error>?>(nil)
}

/// A separate dispatch worker holds the synchronous accessor while its async
/// consumer drains Rust work. Bridge lanes remain available during that wait.
public final class CoordinatedSourceOwner: Sendable {
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.collection-source", qos: .userInitiated)
    private let access: any CoordinatedSourceAccess
    public init(access: any CoordinatedSourceAccess = FoundationSourceAccess()) { self.access = access }
    public func withReadablePackage<Output: Sendable>(
        at selectedPackage: URL, consume: @escaping @Sendable (URL) async throws -> Output
    ) async throws -> Output {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [access] in
                let result = SourceReadResult<Output>()
                do {
                    try access.read(selectedPackage) { coordinatedURL in
                        let terminal = DispatchSemaphore(value: 0)
                        Task {
                            defer { terminal.signal() }
                            do { let value = try await consume(coordinatedURL); result.value.withLock { $0 = .success(value) } }
                            catch { result.value.withLock { $0 = .failure(error) } }
                        }
                        terminal.wait()
                    }
                    continuation.resume(with: result.value.withLock { $0 } ?? .failure(BridgeError.unavailable(reason: "Source accessor did not run")))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
}

/// A cancellation request is remembered before a starter returns. All callers
/// still drain the job, even if cancellation itself fails. No lock crosses FFI.
public final class CollectionEntryOperation: Sendable {
    private struct State { var cancelled = false; var job: CoreJob?; var committing = false }
    private let state = Mutex(State())
    public init() {}
    public func cancel() async {
        let job = state.withLock { state -> CoreJob? in
            guard !state.committing else { return nil }
            state.cancelled = true
            return state.job
        }
        try? await job?.cancel()
    }
    func attach(_ job: CoreJob) async {
        let cancel = state.withLock { state -> Bool in state.job = job; return state.cancelled }
        if cancel { try? await job.cancel() }
    }
    func update(_ progress: CoreProgress) {
        if !progress.cancellable { state.withLock { $0.committing = true } }
    }
    func checkCancellation() throws {
        if state.withLock({ $0.cancelled }) { throw CollectionEntryFailure(issue: .cancelled) }
    }
}
