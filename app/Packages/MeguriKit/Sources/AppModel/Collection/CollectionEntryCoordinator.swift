import CoreBridge
import Foundation
import Observation

/// Owned by ReviewCollection, above both shell layouts and all sheet hosts.
@Observable @MainActor
public final class CollectionEntryCoordinator {
    public enum EntryStage: Equatable {
        case awaitingFile, readingPackage, awaitingApproval, applyingPackage, refused
    }
    public private(set) var stage: EntryStage = .awaitingFile
    public var pickerPresented = false
    public private(set) var preview: InspectedCollection?
    public private(set) var lastActivation: CollectionActivation?
    public private(set) var progress: CoreProgress?
    public private(set) var failure: CollectionEntryFailure?
    public private(set) var message: String?
    public private(set) var needsReinspection = false
    public var busy: Bool { stage == .readingPackage || stage == .applyingPackage }
    public var canCancel: Bool {
        if stage == .applyingPackage, let lifecycle = owner?.lifecycle, case .exclusiveBusy(_, _, let cancellable) = lifecycle, !cancellable { return false }
        return busy && progress?.cancellable != false && failure?.candidateActive != true
    }
    public var canChoose: Bool { !busy && failure?.candidateActive != true }
    public var canPresent: Bool { owner?.entryAllowed == true && !busy }
    private weak var owner: ReviewCollection?
    private let service: any CollectionEntryService
    private var operation: CollectionEntryOperation?
    private var task: Task<Void, Never>?
    private var revision: UInt64 = 0
    private var selectedSource: URL?

    init(owner: ReviewCollection, service: any CollectionEntryService) { self.owner = owner; self.service = service }
    public func present() {
        guard canPresent else { return }
        message = nil; failure = nil
        owner?.navigation.present(.collectionEntry)
    }
    public func chooseFile() { guard canChoose else { return }; pickerPresented = true }
    public func selection(_ result: Result<[URL], any Error>) {
        pickerPresented = false
        switch result {
        case .success(let urls):
            guard urls.count == 1, let url = urls.first else { return }
            inspect(url)
        case .failure(let error):
            if (error as NSError).code != NSUserCancelledError { report(error) }
        }
    }
    public func reinspect() { if let selectedSource, !busy { inspect(selectedSource) } }
    public func inspect(_ source: URL) {
        guard !busy, let owner, owner.entryAllowed else { return }
        selectedSource = source
        lastActivation = nil
        revision &+= 1
        let expected = revision
        let old = preview; preview = nil
        message = nil; failure = nil; needsReinspection = false; progress = nil; stage = .readingPackage
        let operation = CollectionEntryOperation(); self.operation = operation
        task = Task {
            try? await old?.discard()
            do {
                let location = try await owner.prepareEntry()
                let value = try await service.inspect(source: source, location: location, operation: operation,
                    progress: progressReceiver(expected))
                guard expected == revision else { try? await value.discard(); return }
                preview = value; stage = .awaitingApproval
            } catch { if expected == revision { report(error) } }
            if expected == revision { self.operation = nil; task = nil; progress = nil }
        }
    }
    public func confirm() {
        guard stage == .awaitingApproval, let preview, let owner else { return }
        do { try owner.beginEntryActivation() }
        catch { self.preview = nil; Task { try? await preview.discard() }; report(error); return }
        revision &+= 1
        let expected = revision
        stage = .applyingPackage; message = nil; failure = nil; progress = nil
        self.preview = nil
        let operation = CollectionEntryOperation(); self.operation = operation
        task = Task {
            do {
                let activated = try await service.activate(preview, operation: operation, progress: progressReceiver(expected))
                lastActivation = activated
                guard await owner.finishEntryActivation(candidateActive: true) else {
                    throw CollectionEntryFailure(issue: .core(.unavailable(reason: "Native host setup could not finish")),
                        candidateActive: true, generation: activated.generation, backup: activated.backup)
                }
                if expected == revision { stage = .awaitingFile; selectedSource = nil }
            } catch {
                let candidateStayedOpen = (error as? CollectionEntryFailure)?.candidateActive == true
                await owner.finishEntryActivation(candidateActive: false)
                // Keep the recovery explanation visible even when the backend
                // emitted Ready/CollectionReplaced before reporting failure.
                if expected == revision {
                    report(error)
                    if candidateStayedOpen { owner.navigation.present(.collectionEntry) }
                }
            }
            try? await preview.discard()
            if expected == revision { self.operation = nil; task = nil; progress = nil }
        }
    }
    public func waitForCompletion() async { await task?.value }
    public func cancelAndDrain() async {
        if canCancel { await operation?.cancel() }
        // After commit, finish the same job and preserve its retained host.
        await task?.value
        if failure?.candidateActive != true { await discardPreview() }
    }
    public func dismiss() async {
        await cancelAndDrain()
        guard failure?.candidateActive != true else { return }
        owner?.navigation.dismissSheet()
        stage = .awaitingFile; message = nil; failure = nil; selectedSource = nil
    }
    public func continueAfterActivation() async {
        guard failure?.candidateActive == true, let owner else { return }
        let recoveryFinished = await owner.finishEntryActivation(candidateActive: true)
        if recoveryFinished {
            failure = nil; message = nil; stage = .awaitingFile; selectedSource = nil
        } else {
            owner.navigation.present(.collectionEntry)
            message = "The new collection remains active. Quit and reopen Meguri to finish recovery. Any pre-import safety backup remains available."
        }
    }
    func destinationChanged() {
        revision &+= 1
        let expected = revision
        let old = preview; preview = nil
        let running = task; let operation = self.operation
        stage = .refused; needsReinspection = true
        message = Self.explanation(CollectionEntryFailure(issue: .staleConfirmation), fallback: "")
        task = Task {
            await operation?.cancel()
            await running?.value
            try? await old?.discard()
            if revision == expected { self.operation = nil; self.task = nil; progress = nil }
        }
    }
    public func presentationChanged() {
        guard owner?.navigation.sheet != .collectionEntry, !busy else { return }
        pickerPresented = false
        Task { await discardPreview() }
    }
    private func discardPreview() async {
        let old = preview; preview = nil
        try? await old?.discard()
        if stage == .awaitingApproval { stage = .awaitingFile }
    }
    private func progressReceiver(_ expected: UInt64) -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in
            Task { @MainActor in
                guard let self, self.revision == expected, self.busy else { return }
                // Once the commit boundary is observed, coalescing must never
                // make the UI offer cancellation again.
                if self.progress?.cancellable == false && value.cancellable { return }
                self.progress = value
            }
        }
    }
    private func report(_ error: any Error) {
        stage = .refused; preview = nil
        if let entry = error as? CollectionEntryFailure { failure = entry }
        else if let bridge = error as? BridgeError { failure = CollectionEntryFailure(issue: .core(bridge)) }
        else { failure = nil }
        needsReinspection = failure?.issue == .staleConfirmation || failure?.issue == .consumed
        message = Self.explanation(failure, fallback: "Couldn’t read the selected file. Choose it again from Files or copy it to this device first.")
    }
    public static func explanation(_ failure: CollectionEntryFailure?, fallback: String) -> String {
        guard let failure else { return fallback }
        if failure.candidateActive {
            return "The new collection remains active, but setup could not finish. Continue to retry recovery, or quit and reopen Meguri. Any pre-import safety backup is retained."
        }
        switch failure.issue {
        case .staleConfirmation, .consumed:
            return "Local content changed after this preview. Inspect the file again and confirm the updated destination before importing."
        case .cancelled, .core(.cancelled), .package(.cancelled):
            return "Import cancelled. Your previous collection is still available."
        case .core(.sessionActive):
            return "Finish the current review session, then inspect the file again."
        case .package(.unsupportedMedia):
            return "This package includes media. Choose a latest-format collection-only .colpkg with no media. Media import is not available yet."
        case .package(.unsupportedFormat), .package(.tooNew), .package(.schemaUnsupported):
            return "This format is not supported. Export a latest-format collection-only .colpkg without media, then choose that file."
        case .package(.insufficientSpace), .package(.diskFull), .core(.insufficientSpace):
            return "There isn’t enough free space to safely copy, inspect and back up this collection. Free space on this device and try again."
        case .package(.corrupt), .package(.sourceNotFinalised):
            return "This collection package is incomplete or damaged. Export a new collection-only .colpkg and choose it again. Your previous collection is unchanged."
        case .safetyBackup:
            return "Meguri couldn’t finish the required safety backup. Your previous collection is still active. Check free space and access to Backups, then inspect and try again. Any published backup is retained."
        case .core(.busy), .core(.unavailable):
            return "The collection is busy or unavailable. Wait for the current operation to finish, then inspect the file again."
        default:
            return "Import could not finish. Your previous collection is still available. Choose the file again; if this continues, export a fresh collection-only .colpkg."
        }
    }
    public var progressTitle: String {
        switch progress?.phase {
        case .committing?: "Activating collection…"
        case .named(let name)?:
            if name.localizedCaseInsensitiveContains("backup") { "Making safety backup…" }
            else if name.localizedCaseInsensitiveContains("copy") { "Copying collection…" }
            else if name.localizedCaseInsensitiveContains("inspect") { "Inspecting collection…" }
            else { stage == .readingPackage ? "Inspecting collection…" : "Preparing activation…" }
        default: stage == .readingPackage ? "Copying and inspecting collection…" : "Preparing safety backup and activation…"
        }
    }
}
