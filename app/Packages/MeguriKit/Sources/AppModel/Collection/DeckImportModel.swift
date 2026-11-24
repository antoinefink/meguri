import CoreBridge
import Foundation
import Observation

/// What the deck package import sheet needs from the app around it: its
/// sheet, the open collection's names and undo history, Browse, and Meguri
/// Sync (`package/transfer-sheets` X2). `ReviewCollection` in the app; tests
/// pass their own.
@MainActor public protocol DeckImportHost: AnyObject {
    /// The sheet is the one shown.
    var deckImportSheetShown: Bool { get }
    /// Shows the sheet on the window the file came from (`package/file-intake` K1).
    func presentDeckImport()
    func dismissDeckImport()
    /// X2.3: note type names by id, for the preview's rows the engine names by id only.
    func deckImportNotetypeNames() async -> [Int64: String]
    /// The newest undo step is `step` (I14.3's "Import"), so Undo Import can run.
    func deckImportCanUndo(step: UInt64) async -> Bool
    /// Undoes the newest step when it is still `step`. True when it was undone.
    func undoDeckImport(step: UInt64) async -> Bool
    /// X2.5's Show: Browse with a search for exactly these notes. Nil where
    /// Browse cannot be opened.
    var deckImportShowInBrowse: (([Int64]) -> Void)? { get }
    /// Meguri Sync's side (`sync/meguri-provider` MP7.1); nil without one.
    var deckImportSync: (any DeckImportMeguriSync)? { get }
    /// "Another file": the intake's picker, after the sheet closed.
    func chooseAnotherImportFile()
    /// The intake's message, shown in place of a sheet that could not open.
    func deckImportMessage(_ message: ImportIntake.Message)
    /// K5: the inspect found a whole collection: its own flow and confirmation take the file.
    func deckImportFoundCollection(_ url: URL, release: (@MainActor () -> Void)?)
}

/// Meguri Sync's side of a deck package import (MP7.1), the sync coordinator in the app.
@MainActor public protocol DeckImportMeguriSync: AnyObject {
    /// A ready binding: the apply asks about structure (I8), and a
    /// structural confirmation runs MP7.1's job.
    var deckImportSyncReady: Bool { get }
    /// D4's reachability check, made when the confirmation shows.
    func deckImportSyncReachable() async -> Bool
    /// MP7.1's job for an accepted confirmation that showed the sync paragraph.
    func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions, confirmation: DeckImportConfirmation,
                           reachable: Bool, stage: @escaping @MainActor (NoteTypeSyncStage) -> Void) async throws
        -> MeguriDeckImportOutcome
    /// Asks the running job to stop where MP7 allows.
    func cancelDeckImport()
}

/// U162's deck package import sheet (`package/transfer-sheets` X2): one
/// session per file, from the inspect to Done. Owned by `ReviewCollection`,
/// above both shell layouts.
@Observable @MainActor
public final class DeckImportModel {
    public enum Step: Equatable, Sendable {
        /// No file.
        case idle
        /// X2.1: the inspect runs (the sheet shows only once it is slow).
        case inspecting
        /// X2.1, X2.2: the facts and the options.
        case options
        /// X2.4: an apply, or MP7.1's job, runs.
        case importing
        /// X2.3: the apply asked first.
        case confirming
        /// X2.5.
        case result
        /// X2.6: refused; see `refusal`.
        case refused
        /// X2.6: committed, and the result cannot be shown. Never "unchanged",
        /// never an offer to import again.
        case importedWithoutResult
    }

    /// Where a refusal came from: Retry runs that call again (X2.6).
    public enum RefusalSource: Equatable, Sendable { case inspect, apply }

    /// Why Meguri Sync's job (MP7.1) did not import, when it is not one of I16's refusals.
    public enum SyncStop: Equatable, Sendable {
        /// A round or another structure job runs.
        case busy
        /// An earlier structure change waits for its upload (MP7 step 5).
        case uploadPending
        /// The server requires a full sync: MP6.3's choice comes first.
        case fullSyncRequired
        /// The round before the import stopped; the sync status says why.
        case roundStopped
        /// The job could not run.
        case unavailable
    }

    public enum Refusal: Equatable, Sendable {
        case engine(DeckImportRefusal)
        case sync(SyncStop)
    }

    /// X2.3's sync paragraph: which of its two first sentences.
    public enum SyncParagraph: Equatable, Sendable { case replaces, unreachable }

    /// What the progress says (X2.4, I14.5; MP7.1's three parts).
    public enum ProgressStage: Equatable, Sendable {
        case reading, planning, media, backup, committing, syncing, uploading
    }

    public private(set) var step: Step = .idle
    public private(set) var fileName: String?
    /// K4: more than one file was given; this one is imported.
    public private(set) var otherFilesGiven = false
    public private(set) var facts: DeckImportFacts?
    /// The options the sheet shows. Changing one drops a shown confirmation:
    /// its token holds the options it was given for (X2.3, I8).
    public var options = DeckImportOptions() {
        didSet {
            guard options != oldValue else { return }
            if step == .confirming { step = .options }
            confirmation = nil; syncParagraph = nil; cancelledNotice = false
        }
    }
    public private(set) var progressStage: ProgressStage?
    public private(set) var progressCompleted: UInt64 = 0
    public private(set) var progressTotal: UInt64?
    /// False from the write phase on (I14.5): Cancel is not offered then.
    public private(set) var cancellable = true
    /// The snappy rule: progress shows only once the work is slow; until then
    /// the sheet keeps what it showed.
    public private(set) var progressVisible = false
    public private(set) var cancelling = false
    public private(set) var confirmation: DeckImportConfirmation?
    /// X2.3: names the preview gives by id only, read from the collection.
    public private(set) var notetypeNames: [Int64: String] = [:]
    public private(set) var syncParagraph: SyncParagraph?
    /// X2.6 `Cancelled` at the apply: the options again, saying the collection is unchanged.
    public private(set) var cancelledNotice = false
    public private(set) var result: DeckImportResult?
    /// MP7.1: what became of the upload, after the job.
    public private(set) var upload: MeguriStructureUpload?
    public private(set) var refusal: Refusal?
    /// The options of the apply that was refused (X2.6's `TooLarge{rows}` reads its learning progress).
    public private(set) var appliedOptions: DeckImportOptions?
    public private(set) var refusalSource: RefusalSource?
    public private(set) var canUndo = false
    public private(set) var undoing = false
    public private(set) var undone = false
    /// X2.5's table, read in pages (I13).
    public private(set) var log: [DeckImportLogCategory: [DeckImportLogEntry]] = [:]
    public private(set) var logLoading: Set<DeckImportLogCategory> = []

    /// Work is running that the sheet waits for.
    public var busy: Bool { step == .inspecting || step == .importing || undoing }
    public var active: Bool { step != .idle }
    /// The sheet can be dismissed by the system (a swipe): only where Cancel or Done would close it.
    public var dismissible: Bool { step == .options || step == .refused || step == .result || step == .importedWithoutResult }
    /// X2.3: the confirm button has the destructive role only with losses (D3).
    public var confirmationHasLosses: Bool { confirmation.map { !$0.preview.isEmpty } ?? false }

    @ObservationIgnored private weak var host: (any DeckImportHost)?
    @ObservationIgnored private let service: any DeckPackageImportService
    @ObservationIgnored private var source: URL?
    @ObservationIgnored private var release: (@MainActor () -> Void)?
    @ObservationIgnored private var session: DeckImportSession?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var inspection: DeckImportTask<DeckImportSession>?
    @ObservationIgnored private var application: DeckImportTask<DeckImportApplyOutcome>?
    @ObservationIgnored private var syncJob: (any DeckImportMeguriSync)?
    @ObservationIgnored private var revealTask: Task<Void, Never>?
    @ObservationIgnored private var revision: UInt64 = 0
    /// How long work runs before its progress shows.
    @ObservationIgnored public var progressDelay: Duration = .milliseconds(350)
    /// The page size of the log.
    @ObservationIgnored public var logPageSize: UInt32 = 50

    public init(host: (any DeckImportHost)?, service: any DeckPackageImportService) {
        self.host = host; self.service = service
    }

    // MARK: Inspect

    /// K3 step 5: a file the intake classified as a deck package. `release`
    /// ends the intake's hold on the file (K3 steps 1 and 6) when the flow ends.
    /// Answers the message the intake shows when no sheet opens.
    @discardableResult
    public func begin(_ url: URL, otherFilesGiven: Bool = false, release: (@MainActor () -> Void)? = nil) -> Bool {
        guard step == .idle else { release?(); return false }
        source = url; self.release = release
        self.otherFilesGiven = otherFilesGiven
        fileName = url.lastPathComponent
        inspect(url)
        return true
    }

    private func inspect(_ url: URL) {
        revision &+= 1
        let expected = revision
        resetForInspect()
        step = .inspecting
        progressStage = .reading
        let inspection = service.inspect(file: url)
        self.inspection = inspection
        reveal(expected, presenting: true)
        follow(inspection.progress, expected)
        task = Task {
            // I5: the options' starting values, read when the sheet opens.
            async let stored = try? service.storedOptions()
            let outcome: Result<DeckImportSession, DeckImportRefusal>
            do { outcome = .success(try await inspection.value()) }
            catch let refusal as DeckImportRefusal { outcome = .failure(refusal) }
            catch { outcome = .failure(.unreadableFile) }
            let options = await stored
            guard expected == revision else {
                if case .success(let session) = outcome { await service.close(session) }
                return
            }
            self.inspection = nil; endProgress()
            switch outcome {
            case .success(let session):
                guard let options else {
                    // X2.6: `read_options` failed: the intake's message, no sheet.
                    await service.close(session)
                    finish(message: .collectionUnavailable)
                    return
                }
                self.session = session
                facts = session.facts
                fileName = session.fileName
                self.options = options
                step = .options
            case .failure(.cancelled):
                // X2.6: an inspect's Cancel offers nothing: the sheet closes.
                finish(message: nil)
                return
            case .failure(.collectionPackage):
                // K5: a whole collection by its content goes to the collection flow.
                let release = self.release; self.release = nil
                finish(message: nil)
                host?.deckImportFoundCollection(url, release: release)
                return
            case .failure(let refusal):
                self.refusal = .engine(refusal); refusalSource = .inspect
                step = .refused
            }
            ensurePresented()
            task = nil
        }
    }

    private func resetForInspect() {
        facts = nil; confirmation = nil; syncParagraph = nil; notetypeNames = [:]; cancelledNotice = false
        result = nil; upload = nil; refusal = nil; refusalSource = nil
        canUndo = false; undoing = false; undone = false; log = [:]; logLoading = []
        progressVisible = false; cancelling = false; cancellable = true
        progressCompleted = 0; progressTotal = nil
    }

    // MARK: Import

    /// The sheet's Import (X2.3's first apply, with no token).
    public func importPackage() {
        guard step == .options else { return }
        apply(confirmation: nil)
    }

    /// The confirmation's button: the apply again with its token, or MP7.1's
    /// job when it showed the sync paragraph.
    public func confirm() {
        guard step == .confirming, let confirmation else { return }
        if confirmation.reasons.isEmpty { apply(confirmation: confirmation); return }
        // Shown again without its sync paragraph (`.notReady`): the import runs alone.
        if syncParagraph == nil { apply(confirmation: confirmation, askStructure: false); return }
        // MP7.1: the service is read when the confirmation is accepted.
        guard let sync = host?.deckImportSync, sync.deckImportSyncReady else {
            runAlone(confirmation)
            return
        }
        runSyncJob(sync, confirmation: confirmation, reachable: syncParagraph != .unreachable)
    }

    /// MP7.1 `.notReady`: the import runs alone, after the confirmation is
    /// shown again without its sync paragraph when it still has rows.
    private func runAlone(_ confirmation: DeckImportConfirmation) {
        if syncParagraph != nil, !confirmation.preview.isEmpty {
            syncParagraph = nil
            step = .confirming
            return
        }
        apply(confirmation: confirmation, askStructure: false)
    }

    /// The confirmation's Cancel: calls nothing; the session stays open (X2.3).
    public func cancelConfirmation() {
        guard step == .confirming else { return }
        confirmation = nil; syncParagraph = nil
        step = .options
    }

    private func apply(confirmation: DeckImportConfirmation?, askStructure: Bool? = nil) {
        guard let session else { return }
        revision &+= 1
        let expected = revision
        let ask = askStructure ?? (host?.deckImportSync?.deckImportSyncReady == true)
        let options = options
        appliedOptions = options
        beginImporting(expected)
        let application = service.apply(session, options: options, confirmation: confirmation, askStructure: ask)
        self.application = application
        follow(application.progress, expected)
        task = Task {
            let outcome: DeckImportApplyOutcome
            do { outcome = try await application.value() }
            catch let refusal as DeckImportRefusal { outcome = .refused(refusal) }
            catch { outcome = .refused(.engine(.internal("Apply ended without an outcome"))) }
            guard expected == revision else { return }
            self.application = nil
            await receive(outcome, options: options, expected: expected)
            if expected == revision { task = nil; ensurePresented() }
        }
    }

    private func receive(_ outcome: DeckImportApplyOutcome, options: DeckImportOptions, expected: UInt64) async {
        switch outcome {
        case .imported(let result):
            await show(result, upload: nil, expected: expected)
        case .confirmationRequired(let confirmation):
            await ask(confirmation, options: options, expected: expected)
        case .refused(.cancelled):
            endProgress()
            cancelledNotice = true
            step = .options
        case .refused(.committedWithoutResult):
            endProgress()
            closeSession()
            step = .importedWithoutResult
        case .refused(let refusal):
            endProgress()
            self.refusal = .engine(refusal); refusalSource = .apply
            if refusal == .sessionClosed { closeSession() }
            step = .refused
        }
    }

    /// X2.3: shows the preview; with structural reasons, the sync paragraph,
    /// after D4's reachability check.
    private func ask(_ confirmation: DeckImportConfirmation, options: DeckImportOptions, expected: UInt64) async {
        var paragraph: SyncParagraph?
        // Reasons come only from a ready Meguri Sync binding (I8); without Meguri
        // Sync chosen any more, the confirmation shows its rows alone.
        if !confirmation.reasons.isEmpty, let sync = host?.deckImportSync {
            paragraph = await sync.deckImportSyncReachable() ? .replaces : .unreachable
        }
        let unnamed = confirmation.preview.removedNotetypes.contains { $0.movedToName == nil }
            || confirmation.preview.droppedValues.contains { $0.notetypeName == nil }
        let names = unnamed ? await host?.deckImportNotetypeNames() ?? [:] : [:]
        guard expected == revision else { return }
        endProgress()
        // An option changed meanwhile: the token is not for these options.
        guard options == self.options else { step = .options; return }
        // Nothing to show: no rows, and Meguri Sync is no longer chosen (D3 asks nothing then).
        if confirmation.preview.isEmpty, paragraph == nil {
            apply(confirmation: confirmation, askStructure: false)
            return
        }
        notetypeNames = names
        syncParagraph = paragraph
        self.confirmation = confirmation
        step = .confirming
    }

    private func runSyncJob(_ sync: any DeckImportMeguriSync, confirmation: DeckImportConfirmation, reachable: Bool) {
        guard let session else { return }
        revision &+= 1
        let expected = revision
        let options = options
        beginImporting(expected)
        progressStage = reachable ? .syncing : .committing
        syncJob = sync
        task = Task {
            let outcome: MeguriDeckImportOutcome
            do {
                outcome = try await sync.importDeckPackage(session, options: options, confirmation: confirmation,
                    reachable: reachable) { stage in
                        guard self.revision == expected else { return }
                        self.progressStage = switch stage { case .syncing: .syncing; case .saving: .committing; case .uploading: .uploading }
                        self.cancellable = stage == .syncing
                        self.progressCompleted = 0; self.progressTotal = nil
                    }
            } catch {
                outcome = .notStarted(.unavailable)
            }
            guard expected == revision else { return }
            syncJob = nil
            await receive(outcome, options: options, expected: expected)
            if expected == revision { task = nil; ensurePresented() }
        }
    }

    private func receive(_ outcome: MeguriDeckImportOutcome, options: DeckImportOptions, expected: UInt64) async {
        func stop(_ value: SyncStop) {
            endProgress()
            refusal = .sync(value); refusalSource = .apply
            step = .refused
        }
        switch outcome {
        case .notStarted(.notReady):
            endProgress()
            if let confirmation { runAlone(confirmation) } else { step = .options }
        case .notStarted(.busy): stop(.busy)
        case .notStarted(.uploadPending): stop(.uploadPending)
        case .notStarted: stop(.unavailable)
        case .fullSyncRequired: stop(.fullSyncRequired)
        case .roundStopped: stop(.roundStopped)
        case .unavailable: stop(.unavailable)
        case .cancelled:
            endProgress()
            cancelledNotice = true
            confirmation = nil; syncParagraph = nil
            step = .options
        case .notImported(let answer):
            await receive(answer, options: options, expected: expected)
        case .imported(let result, let upload):
            await show(result, upload: upload, expected: expected)
        case .importedWithoutResult:
            endProgress()
            closeSession()
            step = .importedWithoutResult
        }
    }

    private func show(_ result: DeckImportResult, upload: MeguriStructureUpload?, expected: UInt64) async {
        // X2.5: Undo only when the result names a step, and only while it is
        // the newest one (MP7.1's upload clears the history).
        var undo = false
        if let step = result.undoStep, !step.isEmpty, let host { undo = await host.deckImportCanUndo(step: result.sequence) }
        guard expected == revision else { return }
        endProgress()
        self.result = result; self.upload = upload
        confirmation = nil; syncParagraph = nil
        canUndo = undo
        step = .result
    }

    /// X2.4's Cancel while an apply or the inspect runs. Not once the write
    /// phase began; the work still ends with its own outcome.
    public func cancelWork() {
        guard busy, cancellable, !undoing else { return }
        cancelling = true
        if step == .inspecting { inspection?.cancel(); return }
        if let syncJob { syncJob.cancelDeckImport() } else { application?.cancel() }
    }

    // MARK: After a refusal (X2.6)

    /// Retry: the apply again on the same session, or the inspect again on the same file.
    public func retry() {
        guard step == .refused else { return }
        switch refusalSource {
        case .inspect?:
            guard let source else { return }
            inspect(source)
        case .apply?:
            guard session != nil else { return }
            refusal = nil; refusalSource = nil
            step = .options
            importPackage()
        case nil: return
        }
    }

    /// `TooLarge{rows}` at the apply: Import any learning progress off, then the import again.
    public func importWithoutLearningProgress() {
        guard step == .refused, session != nil else { return }
        refusal = nil; refusalSource = nil
        step = .options
        options.withScheduling = false
        importPackage()
    }

    /// `NotetypeKindConflict`: Merge note types off, then the import again.
    public func importWithoutMerging() {
        guard step == .refused, session != nil else { return }
        refusal = nil; refusalSource = nil
        step = .options
        options.mergeNotetypes = false
        importPackage()
    }

    /// "Another file": closes the session and goes back to the picker.
    public func chooseAnotherFile() {
        guard !busy else { return }
        let host = host
        finish(message: nil)
        host?.chooseAnotherImportFile()
    }

    // MARK: Result (X2.5)

    public func undoImport() {
        guard step == .result, canUndo, !undoing, let result, let host else { return }
        undoing = true
        let expected = revision
        Task {
            let done = await host.undoDeckImport(step: result.sequence)
            guard expected == revision else { return }
            undoing = false
            canUndo = false
            undone = done
        }
    }

    /// The next page of a category of the log, after the entries already read.
    public func loadLog(_ category: DeckImportLogCategory) {
        guard let session, result != nil, !logLoading.contains(category) else { return }
        let total = result?.log[category] ?? 0
        let have = UInt64(log[category]?.count ?? 0)
        guard have < total else { return }
        logLoading.insert(category)
        let expected = revision, size = logPageSize
        Task {
            let page = (try? await service.logPage(session, category: category, offset: have, count: size)) ?? []
            guard expected == revision else { return }
            logLoading.remove(category)
            log[category, default: []].append(contentsOf: page)
        }
    }

    public func moreLog(_ category: DeckImportLogCategory) -> Bool {
        UInt64(log[category]?.count ?? 0) < (result?.log[category] ?? 0)
    }

    public func showInBrowse(_ noteIds: [Int64]) {
        guard !noteIds.isEmpty, let show = host?.deckImportShowInBrowse else { return }
        show(noteIds)
        done()
    }
    /// A line's Show: Browse with every note of these categories, read in pages first.
    public func showCategories(_ categories: [DeckImportLogCategory]) {
        guard let session, let result, host?.deckImportShowInBrowse != nil else { return }
        let expected = revision, service = service
        Task {
            var ids: [Int64] = []
            for category in categories {
                var offset: UInt64 = 0
                while offset < result.log[category] {
                    let page = (try? await service.logPage(session, category: category, offset: offset, count: 500)) ?? []
                    if page.isEmpty { break }
                    ids += page.map(\.noteId); offset += UInt64(page.count)
                }
            }
            guard expected == revision else { return }
            showInBrowse(ids)
        }
    }
    public var canShowInBrowse: Bool { host?.deckImportShowInBrowse != nil && !undone }

    // MARK: Closing

    /// Cancel in the options, Done in the result: ends the flow.
    public func done() {
        guard !busy else { return }
        finish(message: nil)
    }

    /// The sheet was dismissed by the system, or another took the window.
    public func presentationChanged() {
        guard step != .idle, step != .inspecting, host?.deckImportSheetShown == false else { return }
        // Work that runs ends in a step the person must see: shown again then (`ensurePresented`).
        if busy { return }
        finish(message: nil, dismiss: false)
    }

    /// After an outcome: the sheet shows it. When another sheet holds the
    /// window, the flow ends with a message rather than waiting unseen.
    private func ensurePresented() {
        guard step != .idle, step != .inspecting, step != .importing, let host else { return }
        host.presentDeckImport()
        guard !host.deckImportSheetShown else { return }
        let imported = step == .result || step == .importedWithoutResult
        let name = fileName ?? ""
        finish(message: imported ? .imported(fileName: name) : .alreadyOpen, dismiss: false)
    }

    /// Waits until no apply, inspect or job of the sheet runs.
    public func waitForWork() async {
        while let task {
            await task.value
            if self.task == task { break }
        }
    }

    /// The app goes to the background: running work stops where it may; the
    /// session stays for the sheet.
    public func cancelAndDrain() async {
        if busy, cancellable { cancelWork() }
        await task?.value
    }

    /// The collection closes: every session ends with it.
    public func collectionClosing() async {
        if busy, cancellable { cancelWork() }
        await task?.value
        if let session { self.session = nil; await service.close(session) }
        finish(message: nil)
    }

    private func finish(message: ImportIntake.Message?, dismiss: Bool = true) {
        revision &+= 1
        task?.cancel(); task = nil
        inspection?.cancel(); inspection = nil; application = nil
        revealTask?.cancel(); revealTask = nil
        closeSession()
        let shown = host?.deckImportSheetShown == true
        step = .idle
        resetForInspect()
        fileName = nil; otherFilesGiven = false; progressStage = nil
        source = nil
        let release = release; self.release = nil
        release?()
        if dismiss, shown { host?.dismissDeckImport() }
        if let message { host?.deckImportMessage(message) }
    }

    private func closeSession() {
        guard let session else { return }
        self.session = nil
        let service = service
        Task { await service.close(session) }
    }

    // MARK: Progress

    private func beginImporting(_ expected: UInt64) {
        cancelledNotice = false
        refusal = nil; refusalSource = nil
        step = .importing
        progressStage = .planning; progressCompleted = 0; progressTotal = nil
        cancellable = true; cancelling = false
        reveal(expected, presenting: false)
    }

    private func reveal(_ expected: UInt64, presenting: Bool) {
        progressVisible = false
        revealTask?.cancel()
        let delay = progressDelay
        revealTask = Task {
            if delay > .zero { try? await Task.sleep(for: delay) }
            guard !Task.isCancelled, expected == revision, busy else { return }
            progressVisible = true
            if presenting { host?.presentDeckImport() }
        }
    }

    private func follow(_ stream: AsyncStream<DeckImportProgress>, _ expected: UInt64) {
        Task {
            for await update in stream {
                guard expected == revision, busy else { continue }
                // Once the write phase began, Cancel never comes back.
                if !cancellable && update.cancellable { continue }
                progressStage = switch update.stage {
                case .extracting, .analysing: .reading
                case .planning: .planning
                case .media: .media
                case .backup: .backup
                case .committing: .committing
                case .other: progressStage ?? .planning
                }
                progressCompleted = update.completed; progressTotal = update.total
                cancellable = update.cancellable
            }
        }
    }

    private func endProgress() {
        revealTask?.cancel(); revealTask = nil
        progressVisible = false; cancelling = false; cancellable = true
        progressCompleted = 0; progressTotal = nil
    }
}

extension DeckImportResult {
    /// X2.5's "could not be imported": `conflicting`, `missing_notetype`
    /// and `empty_first_field` together.
    public var notImported: UInt64 { log[.conflicting] + log[.missingNotetype] + log[.emptyFirstField] }
}
