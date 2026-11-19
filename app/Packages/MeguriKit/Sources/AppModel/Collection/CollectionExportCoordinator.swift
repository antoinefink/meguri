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
    private let recordDiagnostic: FieldDiagnosticRecorder
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
    public var canSave: Bool {
        guard let owner, let artifact else { return false }
        return !busy && providerLease == nil && !stale && !discardRequested && owner.generation == artifact.metadata.sourceGeneration
    }
    public var canRetryCompletion: Bool { !busy && completionRetryNeeded && providerLease != nil }
    /// N17.3: one plain line; the phase detail stays in diagnostics.
    public var plainProgressTitle: String {
        switch stage {
        case .saving: "Saving…"
        case .discarding: "Cancelling…"
        default: destinationPresented ? "Saving…" : "Preparing Export…"
        }
    }
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
         recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in },
         finishLease: @escaping @Sendable (CollectionExportReadLease) async throws -> Void = { try await $0.finish() }) {
        self.owner = owner; self.service = service; self.finishLease = finishLease
        self.recordDiagnostic = recordDiagnostic
    }
    public var canPresent: Bool { owner?.exportAllowed == true && !busy }
    public func present() {
        guard canPresent else { return }
        owner?.navigation.present(.collectionExport)
    }

    // MARK: One continuous export (N17.3)

    /// Set by `start()`: preparation hands straight to the system save dialog,
    /// and a finished or cancelled save discards the prepared file and closes.
    public private(set) var continuous = false
    /// Whether the save dialog can be shown again for the retained file.
    public var canChooseAgain: Bool { continuous && canSave && stage != .saved }
    /// Export Collection: prepare at once, with progress, then ask where to save.
    public func start() {
        guard canPresent else { return }
        continuous = true
        owner?.navigation.present(.collectionExport)
        guard artifact != nil || stage != .idle else { prepare(); return }
        // A file left by an earlier, interrupted export is never offered again.
        Task {
            await discard()
            if stage == .idle, continuous { prepare() }
        }
    }
    /// Cancel at any step: stop the work, discard the prepared file, close.
    public func cancel() async {
        guard !destinationPresented else { return }
        // First, so a preparation that finishes meanwhile opens no save dialog.
        continuous = false
        await cancelAndDrain()
        if providerLease == nil { await discard() }
        if owner?.navigation.sheet == .collectionExport { owner?.navigation.dismissSheet() }
    }
    /// After the save dialog: a clean save or a cancel ends the export. A
    /// failure keeps the sheet, its message and the file for another try.
    private func finishContinuous(cancelled: Bool) async {
        guard continuous, providerLease == nil, !completionRetryNeeded, !publicationUncertain, !stale,
              cancelled ? stage == .ready : (stage == .saved && message == nil) else { return }
        task = nil
        await discard()
        guard stage == .idle else { return }
        continuous = false
        if owner?.navigation.sheet == .collectionExport { owner?.navigation.dismissSheet() }
    }
    public func prepare() {
        guard canPrepare, let owner else { return }
        stale = false; message = nil; publicationUncertain = false; stage = .preparing; progress = nil
        let generation = owner.generation
        let operation = CollectionExportOperation(); self.operation = operation
        task = Task { [self] in
            do {
                let output = try await service.prepare(operation: operation, progress: progressReceiver())
                guard !stale, owner.generation == generation, output.metadata.sourceGeneration == generation else {
                    try? await output.discard(); throw BridgeError.staleGeneration(planned: generation, current: owner.generation)
                }
                artifact = output; stage = .ready
                recordDiagnostic(.init(component: .collectionExport, operation: .prepare, stage: .preparation, outcome: .succeeded))
            } catch {
                recordDiagnostic(.init(component: .collectionExport, operation: .prepare, stage: .preparation, outcome: .failed, failure: .other))
                report(error, destination: false)
            }
            self.operation = nil; task = nil; progress = nil
            if continuous, stage == .ready, canSave { chooseDestination() }
        }
    }
    private func progressReceiver() -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in
            Task { @MainActor in
                guard let self, self.stage == .preparing else { return }
                self.progress = value
            }
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
                recordDiagnostic(.init(component: .collectionExport, operation: .publish, stage: .admission, outcome: .failed, failure: .other))
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
                recordDiagnostic(.init(component: .collectionExport, operation: .publish, stage: .publication, outcome: .succeeded))
            } catch {
                report(error, destination: true)
                recordDiagnostic(.init(component: .collectionExport, operation: .publish, stage: .publication,
                    outcome: publicationUncertain ? .unknown : .failed, failure: .other))
            }
            await finishProvider()
            self.operation = nil; task = nil
            await finishContinuous(cancelled: false)
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
            recordDiagnostic(.init(component: .collectionExport, operation: .publish, stage: .publication,
                outcome: saved ? .succeeded : .cancelled))
            if saved {
                message = stale ? "A copy of the earlier collection was saved. Export again to save the current collection." : nil
            }
            await finishProvider()
            task = nil
            await finishContinuous(cancelled: !saved)
        }
    }
    public func nativeFailed(_ error: any Error) {
        guard destinationPresented else { return }
        destinationPresented = false; stage = .saving
        task = Task {
            report(error, destination: true)
            recordDiagnostic(.init(component: .collectionExport, operation: .publish, stage: .publication,
                outcome: publicationUncertain ? .unknown : .failed, failure: .other))
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
            recordDiagnostic(.init(component: .collectionExport, operation: .cleanup, stage: .cleanup, outcome: .succeeded))
            message = completionMessage; completionMessage = nil
        } catch {
            completionRetryNeeded = true
            recordDiagnostic(.init(component: .collectionExport, operation: .cleanup, stage: .cleanup, outcome: .cleanupPending, failure: .other))
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
            ? "The earlier collection was saved. Export again to save the current collection."
            : "The collection was replaced. Export again to save the current collection."
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
            message = "The collection was replaced. Export again to save the current collection."
        } else if destination {
            message = "The file could not be saved. The completed export is retained; choose another destination and try again."
        } else if let cause = (error as? CollectionExportFailure)?.cause ?? (error as? BridgeError),
                  let refusal = sourceRefusalMessage(cause) {
            message = refusal
        } else if let failure = error as? CollectionExportFailure,
                  case .busy(let operation) = failure.cause {
            message = "Export is blocked by \(operation). Output is retained to protect any provider still reading it."
        } else {
            message = "The collection could not be exported. Check available space and collection access, then try again. Unused prepared output from an earlier launch is recovered when you prepare again."
        }
    }
    private func sourceRefusalMessage(_ cause: BridgeError) -> String? {
        switch cause {
        case .exportSourceChanged:
            return "The media source changed during export. Start a fresh export after changes finish."
        case .exportSourceInvalid:
            return "A media item is unsafe or unreadable. Correct its name, type or read access, then start a fresh export."
        case .exportFormatLimit:
            return "The collection or media exceeds the package format limits. Reduce the size or number of media files, then start a fresh export."
        default: return nil
        }
    }
    public var availabilityMessage: String? {
        guard owner?.exportAllowed != true else { return nil }
        return switch owner?.state {
        case .noFile?, .unopened?: "Open or import a collection before exporting."
        case .failed?, .unavailable?: "The collection is busy or unavailable. Wait for the current operation to finish, then try again."
        default: "Wait for the collection to finish opening."
        }
    }
}
