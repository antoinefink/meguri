import Foundation
import Instrumentation
internal import MeguriCoreBindings
import Synchronization

public nonisolated struct FetchedEditorImage: Sendable {
    public let name: String
    public let bytes: Data
}

/// One picker attempt, including its fetch and final media-write admission.
/// Cancellation fences queued writes synchronously. If a write already owns
/// admission, it finishes before cancel returns; that file was already added.
public final class ImageSourceRequest: Sendable {
    private struct State: Sendable {
        var cancelled = false
        var job: CoreJob?
    }
    private let state = Mutex(State())
    public init() {}
    public var isCancelled: Bool { state.withLock { $0.cancelled } }

    public func cancel() {
        let job = state.withLock { state in
            state.cancelled = true
            return state.job
        }
        if let job { Task { try? await job.cancel() } }
    }

    public func fetch(_ url: URL) async throws -> FetchedEditorImage {
        try await withTaskCancellationHandler {
            try check()
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.startEditorImageFetch) {
                try MeguriCoreBindings.startEditorImageFetch(url: url.absoluteString)
            }
            let job = try await Jobs.trackDraining(started.handle)
            let cancelled = state.withLock { state in
                state.job = job
                return state.cancelled
            }
            if cancelled { try await job.cancel() }
            // Always drain the Rust job, including a cancelled fetch.
            try await job.value()
            try check()
            return try await MaintenanceLane.shared.call(.editorImageFetchResult) {
                guard let result = try started.result.result() else {
                    throw BridgeError.internal("The image fetch returned no result.")
                }
                return FetchedEditorImage(name: result.name, bytes: result.bytes)
            }
        } onCancel: { self.cancel() }
    }

    public func add(name: String, bytes: Data, generation: UInt64) async throws -> String {
        try await withTaskCancellationHandler {
            try await InteractiveLane.shared.call(.editorAddPicture) {
                // Hold admission through the synchronous writer. A cancellation
                // between preparation and lane entry cannot leave an orphan.
                try state.withLock { state in
                    guard !state.cancelled else { throw BridgeError.cancelled }
                    return try MeguriCoreBindings.editorAddPicture(expectedGeneration: generation, name: name, bytes: bytes)
                }
            }
        } onCancel: { self.cancel() }
    }

    private func check() throws {
        if state.withLock({ $0.cancelled }) { throw CancellationError() }
        try Task.checkCancellation()
    }
}
