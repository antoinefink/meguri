import CoreBridge
import Foundation
import Observation

/// Lives with the collection, above the sheet and adaptive shell lifetimes.
@Observable @MainActor
public final class CollectionExportCoordinator {
    public enum Stage: Equatable { case idle, preparing, ready, saving, discarding, saved, failed }
    public private(set) var stage: Stage = .idle
    public private(set) var artifact: ExportedCollection?
    public private(set) var progress: CoreProgress?
    public private(set) var message: String?
    public private(set) var publicationUncertain = false
    public private(set) var destinationPresented = false
    /// Retained until native delegate completion; onDisappear never releases it.
    public private(set) var providerLease: CollectionExportReadLease?
    private weak var owner: ReviewCollection?
    private let service: any CollectionExportService
    private let finishLease: @Sendable (CollectionExportReadLease) async throws -> Void
    private var completionMessage: String?
    public private(set) var completionRetryNeeded = false
    private var operation: CollectionExportOperation?
    private var task: Task<Void, Never>?
    private var stale = false
    private var discardRequested = false
    private var discarding = false
    private var finishingProvider = false
    public var busy: Bool { stage == .preparing || stage == .saving || stage == .discarding || discarding || finishingProvider || destinationPresented }
    public var canPrepare: Bool { !busy && artifact == nil && owner?.exportAllowed == true }
    public var canSave: Bool { !busy && artifact != nil && providerLease == nil && !stale && !discardRequested }
    public var canRetryCompletion: Bool { !busy && completionRetryNeeded && providerLease != nil }
    public var progressTitle: String {
        if stage == .discarding { return "Removing prepared export…" }
        if stage == .saving { return "Saving collection…" }
        if case .named(let name)? = progress?.phase {
            if name.contains("Flushing") { return "Saving pending reviews…" }
            if name.contains("Verifying") { return "Checking completed export…" }
            if name.contains("Capturing") { return "Capturing and writing collection…" }
        }
        return "Preparing collection export…"
    }
    init(owner: ReviewCollection, service: any CollectionExportService,
         finishLease: @escaping @Sendable (CollectionExportReadLease) async throws -> Void = { try await $0.finish() }) {
        self.owner = owner; self.service = service; self.finishLease = finishLease
    }
    public func present() { owner?.navigation.present(.collectionExport) }
    public func prepare() {
        guard canPrepare, let owner else { return }
        stale = false; message = nil; publicationUncertain = false; stage = .preparing; progress = nil
        let generation = owner.generation
        let operation = CollectionExportOperation(); self.operation = operation
        task = Task {
            do {
                let output = try await service.prepare(operation: operation) { [weak self] progress in
                    Task { @MainActor in if self?.stage == .preparing { self?.progress = progress } }
                }
                guard !stale, owner.generation == generation, output.metadata.sourceGeneration == generation else {
                    try? await output.discard(); throw BridgeError.staleGeneration(planned: generation, current: owner.generation)
                }
                artifact = output; stage = .ready
            } catch { report(error, destination: false) }
            self.operation = nil; task = nil; progress = nil
        }
    }
    /// Acquires native ownership before presenting. A generation check happens in Rust.
    public func chooseDestination() {
        guard canSave, let artifact else { return }
        stage = .saving; message = nil; publicationUncertain = false
        task = Task {
            do {
                let lease = try await artifact.acquire()
                providerLease = lease
                guard !stale, owner?.generation == artifact.metadata.sourceGeneration else {
                    stale = true
                    throw BridgeError.invalidArgument("The collection was replaced")
                }
                destinationPresented = true
                stage = .ready
            } catch {
                report(error, destination: true)
                await finishProvider()
            }
            task = nil
        }
    }
    public func save(to url: URL) {
        guard destinationPresented, let artifact, !stale else { nativeCancelled(); return }
        // Keep the native lease and coordination scope through actual publication.
        destinationPresented = false; stage = .saving
        let operation = CollectionExportOperation(); self.operation = operation
        task = Task {
            do {
                try await service.save(artifact, to: url, operation: operation)
                stage = .saved; message = nil
            } catch { report(error, destination: true) }
            await finishProvider()
            self.operation = nil; task = nil
        }
    }
    /// Files has completed its independent copy. This is not a cloud-upload receipt.
    public func nativeSaved() {
        completeNative(saved: true)
    }
    public func nativeCancelled() {
        completeNative(saved: false)
    }
    private func completeNative(saved: Bool) {
        guard destinationPresented else { return }
        destinationPresented = false
        stage = .saving
        // Files' delegate is the completion boundary, never sheet disappearance.
        // The engine slot prevents generation replacement throughout selection/copy.
        task = Task {
            stage = saved ? .saved : (stale ? .failed : .ready)
            if saved {
                message = stale ? "A copy of the earlier collection was saved. Prepare a new export for the current collection." : nil
            }
            await finishProvider()
            task = nil
        }
    }
    public func nativeFailed(_ error: any Error) {
        guard destinationPresented else { return }
        destinationPresented = false; stage = .saving
        task = Task {
            report(error, destination: true)
            await finishProvider()
            task = nil
        }
    }
    private func finishProvider() async {
        guard let lease = providerLease else { return }
        finishingProvider = true
        defer { finishingProvider = false }
        if !completionRetryNeeded { completionMessage = message }
        do {
            try await finishLease(lease)
            providerLease = nil; completionRetryNeeded = false
            message = completionMessage; completionMessage = nil
        } catch {
            completionRetryNeeded = true
            message = stage == .saved
                ? "The file was saved. Export cleanup is pending. Retry cleanup before starting another export."
                : "Native file access has ended, but export cleanup is pending. Retry cleanup to continue."
        }
    }
    public func retryCompletion() {
        guard canRetryCompletion else { return }
        finishingProvider = true
        task = Task {
            await finishProvider()
            task = nil
        }
    }
    public func cancelAndDrain() async {
        await operation?.cancel()
        await task?.value
        // A native provider is independent of the Swift task and sheet lifetime.
    }
    public func waitForCompletion() async { await task?.value }
    public func discard() async {
        guard !destinationPresented, providerLease == nil, !discarding else { return }
        discarding = true
        defer { discarding = false }
        await cancelAndDrain()
        let output = artifact
        discardRequested = true; stage = .discarding
        do {
            try await output?.discard()
            artifact = nil; stage = .idle; message = nil; publicationUncertain = false; discardRequested = false
        } catch { report(error, destination: false) }
    }
    public func dismiss() async {
        guard !destinationPresented else { return }
        await cancelAndDrain()
        // A Ready output survives dismissal and can be saved on the next visit.
        owner?.navigation.dismissSheet()
    }
    func generationChanged() {
        guard artifact != nil || stage == .preparing || destinationPresented else { return }
        stale = true
        message = stage == .saved
            ? "The earlier collection was saved. Discard the prepared export to export the current collection."
            : "The collection was replaced. Discard this export and prepare a new one."
        if !busy && stage != .saved { stage = .failed }
        let operation = operation
        Task { await operation?.cancel() }
    }
    private func report(_ error: any Error, destination: Bool) {
        stage = .failed
        publicationUncertain = (error as? CollectionExportFailure)?.publicationUncertain == true || error is ExportDestinationCompletionUncertain
        if publicationUncertain {
            message = "The new file may already be saved. Check the destination before trying again. The completed export is retained."
        } else if stale {
            message = "The collection was replaced. Discard this export and prepare a new one."
        } else if destination {
            message = "The file could not be saved. The completed export is retained; choose another destination and try again."
        } else if let failure = error as? CollectionExportFailure,
                  case .busy(let operation) = failure.cause {
            message = "Export is blocked by \(operation). Output is retained to protect any provider still reading it."
        } else {
            message = "The collection could not be exported. Check available space and collection access, then try again. Unused prepared output from an earlier launch is recovered when you prepare again."
        }
    }
    public var availabilityMessage: String? {
        guard owner?.exportAllowed != true else { return nil }
        switch owner?.state {
        case .noFile?, .unopened?: "Open or import a collection before exporting."
        case .failed?, .unavailable?: "The collection is busy or unavailable. Wait for the current operation to finish, then try again."
        default: "Wait for the collection to finish opening."
        }
    }
}
