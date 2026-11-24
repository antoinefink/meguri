import CoreBridge
import Foundation
import Observation
import Synchronization

nonisolated private final class PreparedMediaSnapshot: Sendable {
    let value = Mutex<CollectionImportMedia?>(nil)
}

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
    public private(set) var preparedMedia: CollectionImportMedia?
    private var recoveryFinished = false
    public var requiresRecovery: Bool { failure?.requiresRecovery == true && !recoveryFinished }
    public var canContinueRecovery: Bool { requiresRecovery && failure?.candidateActive == true && failure?.requiresReopenRecovery == false }
    public private(set) var progress: CoreProgress?
    public private(set) var failure: CollectionEntryFailure?
    public private(set) var message: String?
    public private(set) var needsReinspection = false
    public var busy: Bool { stage == .readingPackage || stage == .applyingPackage }
    public var canCancel: Bool {
        if stage == .applyingPackage, let lifecycle = owner?.lifecycle, case .exclusiveBusy(_, _, let cancellable) = lifecycle, !cancellable { return false }
        return busy && progress?.cancellable != false && !requiresRecovery
    }
    public var canChoose: Bool { !busy && !requiresRecovery }
    public var canPresent: Bool { !busy && (requiresRecovery || owner?.entryAllowed == true) }
    private weak var owner: ReviewCollection?
    private let service: any CollectionEntryService
    private let recordDiagnostic: FieldDiagnosticRecorder
    private var operation: CollectionEntryOperation?
    private var editorDecision: (operation: CollectionEntryOperation, decision: ReviewCollection.EditorReplacementDecision)?
    private var cancelledActivation: CollectionEntryOperation?
    private var task: Task<Void, Never>?
    private var revision: UInt64 = 0
    private var selectedSource: URL?
    /// The intake's hold on a delivered copy (K3 step 6), ended with this flow.
    private var sourceRelease: (@MainActor () -> Void)?
    func adoptRelease(_ release: (@MainActor () -> Void)?) {
        sourceRelease?()
        sourceRelease = release
    }
    private func endSourceHold() { let release = sourceRelease; sourceRelease = nil; release?() }

    init(owner: ReviewCollection, service: any CollectionEntryService,
         recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        self.owner = owner; self.service = service; self.recordDiagnostic = recordDiagnostic
    }
    /// The name of the chosen file, for the confirmation's title (N17.2).
    public var selectedFileName: String? { selectedSource?.lastPathComponent }
    /// The sheet hosts the picker while it shows; otherwise its window does.
    public var pickerInSheet: Bool { owner?.navigation.sheet == .collectionEntry }
    /// N17.2: Import opens the system file picker directly. The sheet follows
    /// once a file is chosen. Unfinished recovery still opens on its sheet.
    public func present() {
        guard canPresent else { return }
        guard !requiresRecovery else { owner?.navigation.present(.collectionEntry); return }
        message = nil; failure = nil
        if owner?.navigation.sheet == nil { owner?.navigation.sheetHost = .main }
        pickerPresented = true
    }
    public func chooseFile() { guard canChoose else { return }; pickerPresented = true }
    public func selection(_ result: Result<[URL], any Error>) {
        pickerPresented = false
        switch result {
        case .success(let urls):
            guard urls.count == 1, let url = urls.first, canChoose, owner?.entryAllowed == true else { return }
            showSheet()
            // Another sheet took the window meanwhile: the file is not read.
            guard owner?.navigation.sheet == .collectionEntry else { return }
            inspect(url)
        case .failure(let error):
            guard !requiresRecovery else { return }
            if (error as NSError).code != NSUserCancelledError { report(error); showSheet() }
        }
    }
    /// Keeps the window that showed the picker as the sheet's host.
    private func showSheet() {
        guard let navigation = owner?.navigation, navigation.sheet == nil else { return }
        let host = navigation.sheetHost
        navigation.present(.collectionEntry)
        navigation.sheetHost = host
    }
    public func reinspect() { if let selectedSource, canChoose { inspect(selectedSource) } }
    public func inspect(_ source: URL) {
        guard canChoose, let owner, owner.entryAllowed else { return }
        pickerPresented = false
        selectedSource = source
        lastActivation = nil; preparedMedia = nil; recoveryFinished = false
        revision &+= 1
        let expected = revision
        let old = preview; preview = nil
        message = nil; failure = nil; needsReinspection = false; progress = nil; stage = .readingPackage
        let operation = CollectionEntryOperation(); self.operation = operation
        task = Task {
            try? await old?.discard()
            var inspected = false
            do {
                let location = try await owner.prepareEntry()
                let value = try await service.inspect(source: source, location: location, operation: operation,
                    progress: progressReceiver(expected))
                inspected = true
                recordDiagnostic(.init(component: .collectionImport, operation: .inspect, stage: .inspection, outcome: .succeeded))
                guard expected == revision else { try? await value.discard(); return }
                do { try owner.configuration.nativeReviewInput?.inspected(value) }
                catch { try? await value.discard(); throw error }
                preview = value; stage = .awaitingApproval
            } catch {
                recordDiagnostic(.init(component: .collectionImport, operation: .inspect,
                    stage: inspected ? .admission : .inspection, outcome: .failed, failure: .other))
                if expected == revision { report(error) }
            }
            if expected == revision { self.operation = nil; task = nil; progress = nil }
        }
    }
    public func confirm() {
        guard stage == .awaitingApproval, let preview, let owner else { return }
        revision &+= 1
        let expected = revision
        stage = .applyingPackage; message = nil; failure = nil; progress = nil; preparedMedia = nil; recoveryFinished = false
        self.preview = nil
        let operation = CollectionEntryOperation(); self.operation = operation
        let actualPrepared = PreparedMediaSnapshot()
        let decision = ReviewCollection.EditorReplacementDecision()
        editorDecision = (operation, decision)
        task = Task {
            defer { if editorDecision?.operation === operation { editorDecision = nil } }
            let current: @MainActor @Sendable () -> Bool = {
                self.owner === owner && self.revision == expected && self.operation === operation &&
                    self.cancelledActivation !== operation && !Task.isCancelled
            }
            let admission = await owner.prepareEditorReplacement(operation: "Import Collection", decision: decision, current: current)
            defer { if let admission { owner.finishEditorReplacement(admission.id) } }
            guard let admission, admission.accepted else {
                try? await preview.discard()
                if expected == revision { stage = .awaitingFile; self.operation = nil; task = nil; progress = nil }
                return
            }
            let reservation = admission.id
            var activationObserved = false
            var completingHost = false
            var admitted = false
            do {
                guard current(), owner.editorReplacementIsCurrent(reservation) else { throw CancellationError() }
                try owner.configuration.nativeReviewInput?.willActivate(preview)
                try owner.beginEntryActivation(replacement: reservation, entry: self)
                admitted = true
                try await owner.drainEntryActivation()
                guard current(), owner.editorReplacementIsCurrent(reservation) else { throw CancellationError() }
                try decision.admitEffect()
                let activated = try await service.activate(preview, operation: operation, progress: progressReceiver(expected, prepared: actualPrepared))
                activationObserved = true
                recordDiagnostic(.init(component: .collectionImport, operation: .activate, stage: .activation, outcome: .succeeded))
                if let media = actualPrepared.value.withLock({ $0 }) {
                    preparedMedia = media; owner.configuration.nativeReviewInput?.prepared(media)
                }
                owner.configuration.nativeReviewInput?.activated(activated)
                lastActivation = activated
                completingHost = true
                guard await owner.finishEntryActivation(candidateActive: true) else {
                    throw CollectionEntryFailure(after: activated, issue: .core(.unavailable(reason: "Native host setup could not finish")))
                }
                if expected == revision { stage = .awaitingFile; selectedSource = nil }
            } catch {
                if let media = actualPrepared.value.withLock({ $0 }) {
                    preparedMedia = media; owner.configuration.nativeReviewInput?.prepared(media)
                }
                let entryFailure = error as? CollectionEntryFailure
                if !activationObserved, entryFailure?.candidateActive == true {
                    activationObserved = true
                    recordDiagnostic(.init(component: .collectionImport, operation: .activate, stage: .activation, outcome: .succeeded))
                }
                recordDiagnostic(.init(component: .collectionImport,
                    operation: .activate, stage: completingHost ? .presentation : .activation,
                    outcome: entryFailure?.activationOutcomeUncertain == true ? .unknown : .failed, failure: .other))
                // Keep the existing activation admission closed when recovery
                // is required. Host setup is not a media-recovery operation.
                if admitted, entryFailure?.requiresRecovery != true {
                    await owner.finishEntryActivation(candidateActive: false)
                }
                // Keep the recovery explanation visible even when the backend
                // emitted Ready/CollectionReplaced before reporting failure.
                if expected == revision || entryFailure?.requiresRecovery == true {
                    report(error)
                    if requiresRecovery {
                        // Native host setup may have released admission on its
                        // failed finish. Reassert the same owner's fence.
                        try? owner.beginEntryActivation(replacement: reservation, entry: self)
                        owner.navigation.present(.collectionEntry)
                    }
                }
            }
            try? await preview.discard()
            if expected == revision { self.operation = nil; task = nil; progress = nil }
        }
    }
    public func waitForCompletion() async { await task?.value }
    public func cancelAndDrain() async {
        if let held = editorDecision, held.operation === operation { held.decision.cancel() }
        if canCancel { cancelledActivation = operation; await operation?.cancel() }
        // After commit, finish the same job and preserve its retained host.
        await task?.value
        if !requiresRecovery { await discardPreview() }
    }
    public func dismiss() async {
        await cancelAndDrain()
        guard !requiresRecovery else { return }
        owner?.navigation.dismissSheet()
        stage = .awaitingFile; message = nil; failure = nil; selectedSource = nil
        endSourceHold()
    }
    public func continueAfterActivation() async {
        guard canContinueRecovery, let owner else { return }
        do { try await owner.drainEntryActivation() }
        catch {
            message = "The new collection remains active. Quit and reopen Meguri to finish recovery. Any pre-import safety backup remains available."
            return
        }
        let recoveryFinished = await owner.finishEntryActivation(candidateActive: true)
        if recoveryFinished {
            self.recoveryFinished = true; message = "The new collection is available. Retained import details are shown below."
            stage = .awaitingFile; selectedSource = nil
        } else {
            try? owner.beginEntryActivation()
            owner.navigation.present(.collectionEntry)
            message = "The new collection remains active. Quit and reopen Meguri to finish recovery. Any pre-import safety backup remains available."
        }
    }
    func destinationChanged() {
        guard !requiresRecovery else { owner?.navigation.present(.collectionEntry); return }
        revision &+= 1
        let expected = revision
        let old = preview; preview = nil
        let running = task; let operation = self.operation
        if let held = editorDecision, held.operation === operation { held.decision.cancel() }
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
        if !requiresRecovery { endSourceHold() }
        Task { await discardPreview() }
    }
    private func discardPreview() async {
        let old = preview; preview = nil
        try? await old?.discard()
        if stage == .awaitingApproval { stage = .awaitingFile }
    }
    private func progressReceiver(_ expected: UInt64, prepared: PreparedMediaSnapshot? = nil) -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in
            if let media = value.collectionMediaPrepared { prepared?.value.withLock { $0 = media } }
            Task { @MainActor in
                guard let self, self.revision == expected, self.busy else { return }
                // Once the commit boundary is observed, coalescing must never
                // make the UI offer cancellation again.
                if let media = value.collectionMediaPrepared {
                    self.preparedMedia = media
                    self.owner?.configuration.nativeReviewInput?.prepared(media)
                }
                if self.progress?.cancellable == false && value.cancellable { return }
                self.progress = value
            }
        }
    }
    private func report(_ error: any Error) {
        owner?.configuration.nativeReviewInput?.failed(error)
        stage = .refused; preview = nil
        if let entry = error as? CollectionEntryFailure {
            failure = entry
            if let media = entry.preparedMedia { preparedMedia = media }
        }
        else if let bridge = error as? BridgeError { failure = CollectionEntryFailure(issue: .core(bridge)) }
        else { failure = nil }
        needsReinspection = failure?.issue == .staleConfirmation || failure?.issue == .consumed
        message = Self.explanation(failure, fallback: "Couldn’t read the selected file. Choose it again from Files or copy it to this device first.")
    }
    public static func explanation(_ failure: CollectionEntryFailure?, fallback: String) -> String {
        guard let failure else { return fallback }
        if failure.activationOutcomeUncertain {
            return "Meguri couldn’t verify the import outcome. The active collection and media state need authoritative recovery verification before study. Quit and reopen Meguri; do not import the file again. Keep any saved safety backup and recovery files."
        }
        if failure.requiresReopenRecovery {
            return failure.candidateActive
                ? "The new collection remains active or its activation needs verification. Media recovery must finish before study. Quit and reopen Meguri; do not import the file again. The safety backup and recovery files are retained."
                : "The previous collection needs media recovery before it can be used. Quit and reopen Meguri to finish recovery. The safety backup and recovery files are retained. Do not import the file again."
        }
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
            return "This package’s media is not supported by this operation. Choose a supported latest-format .colpkg file."
        case .package(.unsupportedFormat), .package(.tooNew), .package(.schemaUnsupported):
            return "This format is not supported. Export a latest-format .colpkg, then choose that file."
        case .package(.insufficientSpace), .package(.diskFull), .core(.insufficientSpace):
            return "There isn’t enough free space to safely copy, inspect and back up this collection. Free space on this device and try again."
        case .package(.corrupt), .package(.sourceNotFinalised):
            return "This collection package is incomplete or damaged. Export a new .colpkg and choose it again. Your previous collection is unchanged."
        case .safetyBackup:
            return "Meguri couldn’t finish the required safety backup. Your previous collection is still active. Check free space and access to Backups, then inspect and try again. Any published backup is retained."
        case .core(.busy), .core(.unavailable):
            return "The collection is busy or unavailable. Wait for the current operation to finish, then inspect the file again."
        default:
            return "Import could not finish. Your previous collection is still available. Choose the file again; if this continues, export a fresh .colpkg."
        }
    }
    /// N17.2: one plain line per step; the phase detail stays in diagnostics.
    public var plainProgressTitle: String { stage == .readingPackage ? "Reading File…" : "Importing…" }
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
