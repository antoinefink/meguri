import CardRendering
import CoreBridge
import Foundation
import Observation
#if DEBUG
import OSLog
#endif

/// One production collection, held above shell layouts.
@Observable
@MainActor
public final class ReviewCollection: ChangeFanoutSink {
    public enum State: Equatable { case unopened, loading, noFile, ready, unavailable, creationRecovery, failed(String) }
    public private(set) var state: State = .unopened
    public private(set) var lifecycle: CollectionLifecycle = .noCollection
    #if DEBUG
    public private(set) var openFailureDiagnosticValue: String?
    #endif
    public private(set) var generation: UInt64 = 0
    public private(set) var decks: [CountedDeckNode] = [] {
        // A write changes the tree's counts or names: Edit › Undo's step may have changed.
        didSet { if decks != oldValue { undoOwner?.scheduleRefresh() } }
    }
    /// Page content may belong to a deck intentionally absent from the tree.
    public struct DeckPageDetail: Sendable, Hashable {
        public let id: Int64
        public let name: String
        public let counts: ReviewCounts
    }
    private var selectedDeckDetail: (page: DeckPageDetail, generation: UInt64, intent: UInt64)?
    private var currentSelectedDeckDetail: DeckPageDetail? {
        guard let selectedDeckDetail, selectedDeckDetail.generation == generation,
              selectedDeckDetail.intent == navigation.intentRevision,
              navigation.section == .home, navigation.homeDeckPage?.rawValue == selectedDeckDetail.page.id,
              canSaveDeckState, case .ready(let current) = lifecycle, current == generation else { return nil }
        return selectedDeckDetail.page
    }
    public struct DeckPageRefreshRequest: Sendable, Hashable {
        public let deckID: Int64
        fileprivate let generation: UInt64
        fileprivate let intent: UInt64
        fileprivate let counts: UInt64
        fileprivate let tree: UInt64
        fileprivate let selection: UInt64
    }
    private var selectedDeckEmptiness: (request: DeckPageRefreshRequest, empty: Bool)?
    /// Only the visible Home page queries storage; rows keep the normal count refresh.
    public var deckPageRefreshRequest: DeckPageRefreshRequest? {
        guard navigation.section == .home, canSaveDeckState, foreground,
              lifecycle == .ready(generation: generation) else { return nil }
        let id: Int64
        if let destination = navigation.path(.home).last {
            guard case .deckPage(let deck) = destination else { return nil }
            id = deck.rawValue
        } else {
            guard let deck = navigation.homeDeckPage else { return nil }
            id = deck.rawValue
        }
        return DeckPageRefreshRequest(deckID: id, generation: generation, intent: navigation.intentRevision,
            counts: countsRevision, tree: deckTreeRevision, selection: selectionRevision)
    }
    public var emptyDeckPageID: Int64? {
        guard let proof = selectedDeckEmptiness, proof.empty,
              proof.request == deckPageRefreshRequest else { return nil }
        return proof.request.deckID
    }
    public private(set) var review: ReviewCoordinator? {
        // A review that ends, however it ends, ends Study's walk with it (N24.3).
        didSet { if review == nil, let walk { closeWalk(walk) } }
    }
    public private(set) var previousRecord: ReviewRecord?
    /// Today's Study while it walks the decks (N24.3); its current session is `review`.
    public private(set) var walk: StudyWalk?
    /// The last walk in this launch, for Today's Previous Session (N24.2). A walk is not
    /// stored as one record: after a relaunch Today shows the last deck's session.
    public private(set) var previousWalk: StudyWalk.Summary?
    @ObservationIgnored private var walkTask: Task<Void, Never>?
    @ObservationIgnored private var deckPageOpening: Task<Bool, Never>?
    /// Deck page opens so far, and the decks of the last few, for `DeckListStudyRequest`.
    @ObservationIgnored private var deckPageOpens: UInt64 = 0
    @ObservationIgnored private var recentDeckPageOpens: [Int64] = []
    /// `stats/studied-today` T12: the Today page's numbers, read with the counts. A read
    /// that fails keeps the last value.
    public private(set) var studiedToday: StudiedToday?
    /// N24.2: while nothing waits, when the next learning card of a top-level normal deck
    /// becomes ready (`sched/counts` C7's earliest `next_learning_ready_at`).
    public private(set) var nextLearningReadyAt: Date?
    @ObservationIgnored var studiedTodayService: any StudiedTodayService = CoreStudiedTodayService()
    @ObservationIgnored var loadDeckStudyState: @MainActor (Int64) async throws -> ReviewDeckStudyState = {
        try await CoreDeckService().studyState(deckID: $0)
    }
    @ObservationIgnored private var todayTask: Task<Void, Never>?
    /// Moves with each count read that `loadCounts` applies: Today's learning-card scan
    /// belongs to the counts it started from.
    @ObservationIgnored private var countsSnapshot: UInt64 = 0
    @ObservationIgnored private var todayRevision: UInt64 = 0
    /// Turns FSRS on by itself for an open collection (`sched/fsrs-policy` A1).
    public let studyPreparation = StudyPreparation()
    public var policy: FsrsPolicyStatus? { studyPreparation.status }
    public var enabling: Bool { studyPreparation.running }
    /// A study entry point was refused because the collection could not be
    /// prepared (`sched/app-coordination` F5.3). Try Again runs the step again.
    public struct StudyRefusal: Equatable, Sendable {
        public let deckID: Int64
        /// Refused at a turn of Today's Study: Try Again starts Study again (N24.3).
        public let studyAll: Bool
        public init(deckID: Int64, studyAll: Bool = false) { self.deckID = deckID; self.studyAll = studyAll }
    }
    public private(set) var studyRefusal: StudyRefusal?
    public private(set) var progress: CoreProgress?
    public private(set) var error: String?
    public private(set) var deferredSuspend: UInt64?
    public private(set) var foreground = true
    public private(set) var resuming = false
    /// Tests may substitute the journal query; production uses CoreBridge.
    @ObservationIgnored
    public var loadMediaIndex: @MainActor () async throws -> MediaNameSnapshot
    @ObservationIgnored
    var makeReviewSurface: @MainActor (MediaPathResolver) -> any ReviewCardSurface = { ReviewPages(resolver: $0) }
    @ObservationIgnored
    var reviewSessionClient: any ReviewSessionClient = CoreReviewSessionClient()
    private var mediaRevision: UInt64 = 0
    private var mediaTask: Task<Void, Never>?
    private var mediaCloseTask: Task<Void, Never>?
    private var mediaArrivals = ReviewMediaArrivals()
    private weak var mediaArrivalReview: ReviewCoordinator?
    private var sceneActive = true
    private var applicationActive = true
    private var windowVisible = true
    private var reviewWindowActive = true
    private var reviewVisible = true
    private var visibleReviewHosts = 0
    private var reviewPresentationAllowsInput: Bool = true
    public var reviewIsActive: Bool {
        let hostAllowsInput = sceneActive && applicationActive && windowVisible && reviewWindowActive
        let sessionAllowsInput = review == nil || reviewVisible
        return foreground && hostAllowsInput && sessionAllowsInput && reviewPresentationAllowsInput
            && !syncPresentation.statusOwnsInput
    }
    var canCompleteSyncStatusHandoff: Bool {
        foreground && sceneActive && !closed && !closing && !opening && !resuming
    }
    // Notify future U151 integrations after the local flush finishes.
    public var didFlushLocally: (() -> Void)? = nil
    public let configuration: LaunchConfiguration
    public let navigation: AppNavigation
    /// Shared by the main scene, Settings, imports and every shell layout.
    public private(set) var sync: SyncCoordinator
    private var backgroundLease: UInt64 = 0
    private var backgroundDeadlineMilliseconds: Int64?
    private var backgroundLeaseExpired = false
    private let collection: CoreCollectionService
    private let providerDirectory: URL
    private let entryService: any CollectionEntryService
    private var addFlowOwner: AddNoteFlow?
    /// One initiating candidate owns this decision, including cancellation
    /// before its editor question exists. Durable effects keep their old join.
    @MainActor final class EditorReplacementDecision {
        let id = UUID()
        private(set) var cancelled = false
        private var admitted = false
        var revokeQuestion: (@MainActor () -> Void)?
        private var finished = false
        private var waiters: [CheckedContinuation<Void, Never>] = []
        @discardableResult func cancel() -> Bool {
            guard !admitted, !finished else { return false }
            cancelled = true; revokeQuestion?()
            return true
        }
        func admitEffect() throws {
            guard !cancelled, !finished, !Task.isCancelled else { throw CancellationError() }
            admitted = true; revokeQuestion = nil
        }
        func finish() {
            finished = true; revokeQuestion = nil
            let pending = waiters; waiters.removeAll()
            for waiter in pending { waiter.resume() }
        }
        func waitForFinish() async {
            if finished { return }
            await withCheckedContinuation { waiters.append($0) }
        }
    }
    private struct EditorReplacement {
        let id: UUID
        let generation: UInt64
        let editor: AddNoteFlow?
        let decision: EditorReplacementDecision
        var handle: UUID?
    }
    private var editorReplacement: EditorReplacement?
    struct EditorReplacementAdmission {
        let id: UUID
        let accepted: Bool
    }

    /// Reserve this collection before the editor guard yields. The consumer's
    /// predicate binds its task, operation and accepted candidate, not just a generation.
    func prepareEditorReplacement(operation: String, decision: EditorReplacementDecision,
                                  current: @MainActor () -> Bool) async -> EditorReplacementAdmission? {
        // review/edit-current E2: a replacement asks as Cancel does before it starts.
        if let edit = editFlowOwner, edit.isPresented {
            guard await edit.requestCancel(), !decision.cancelled, current(), !Task.isCancelled else { return nil }
        }
        guard editorReplacement == nil, addFlowOwner?.isPresented != true || foreground, !closed, !closing, !opening,
              addFlowOwner?.opening != true, !decision.cancelled, current(), !Task.isCancelled else { return nil }
        let id = decision.id, editor = addFlowOwner, expected = generation
        editorReplacement = EditorReplacement(id: id, generation: expected, editor: editor, decision: decision,
            handle: editor == nil ? nil : id)
        if let editor {
            guard let handle = await editor.prepareForReplacement(operation: operation, decision: decision) else {
                return EditorReplacementAdmission(id: id, accepted: false)
            }
            guard editorReplacement?.id == id else { editor.finishReplacement(handle); return nil }
            editorReplacement?.handle = handle
        }
        guard !opening, editorReplacementIsCurrent(id), current() else {
            return EditorReplacementAdmission(id: id, accepted: false)
        }
        return EditorReplacementAdmission(id: id, accepted: true)
    }
    func editorReplacementIsCurrent(_ id: UUID) -> Bool {
        guard let held = editorReplacement, held.id == id, held.generation == generation,
              held.editor === addFlowOwner, !held.decision.cancelled, !closed, !closing, !Task.isCancelled else { return false }
        return held.editor == nil || (held.handle != nil && held.editor?.isPresented == false && held.editor?.opening == false)
    }
    func finishEditorReplacement(_ id: UUID) {
        guard let held = editorReplacement, held.id == id else { return }
        if let handle = held.handle { held.editor?.finishReplacement(handle) }
        editorReplacement = nil
        held.decision.finish()
    }
    private func cancelPendingEditorReplacement() -> EditorReplacementDecision? {
        guard let decision = editorReplacement?.decision, decision.cancel() else { return nil }
        return decision
    }
    public var addNoteFlow: AddNoteFlow {
        if let addFlowOwner { return addFlowOwner }
        let flow = AddNoteFlow(owner: self); addFlowOwner = flow
        return flow
    }
    private var editFlowOwner: EditNoteFlow?
    /// Edit Note during a review (N20.8). Its entries all use this flow's
    /// `canOpen` and `open` (N4).
    public var editNoteFlow: EditNoteFlow {
        if let editFlowOwner { return editFlowOwner }
        let flow = EditNoteFlow(owner: self); editFlowOwner = flow
        return flow
    }
    // Accepted Add work may finish while background/close waits for it. New
    // actions require foreground admission separately from runtime ownership.
    func ownsAddGeneration(_ expected: UInt64) -> Bool {
        !closed && generation == expected && state == .ready && lifecycle == .ready(generation: expected)
    }
    var canBeginAddAction: Bool {
        let allowsSheet: Bool
        switch navigation.sheet {
        case nil, .add: allowsSheet = true
        default: allowsSheet = false
        }
        return editorReplacement == nil && ownsAddGeneration(generation) && foreground && !resuming && !closing && !opening && !starting &&
            !deckNamesReserved && !meguriSignInPreparing && !sync.providerHandoffInProgress &&
            !sync.meguriCollectionWorkHeld && entryOwner?.busy != true && entryOwner?.preview == nil &&
            entryOwner?.pickerPresented != true && manualOwner?.busy != true && manualOwner?.preview == nil &&
            exportOwner?.busy != true && allowsSheet
    }
    func refreshAfterAdding(generation expected: UInt64) async {
        guard ownsAddGeneration(expected) else { return }
        countsRevision &+= 1
        await loadCounts()
        guard ownsAddGeneration(expected), let request = deckPageRefreshRequest else { return }
        await refreshDeckPage(request)
    }
    private var entryOwner: CollectionEntryCoordinator?
    public var entry: CollectionEntryCoordinator {
        if let entryOwner { return entryOwner }
        let value = CollectionEntryCoordinator(owner: self, service: entryService, recordDiagnostic: configuration.recordDiagnostic)
        entryOwner = value
        return value
    }
    private var exportOwner: CollectionExportCoordinator?
    private var deckNamesOwner: DeckActionsModel?
    public var deckActions: DeckActionsModel {
        if let deckNamesOwner { return deckNamesOwner }
        let value = DeckActionsModel(owner: self)
        deckNamesOwner = value
        return value
    }
    private var deckOptionsOwner: DeckOptionsPageModel?
    /// The Deck Options page's draft (N20.9), above the layout switch.
    public var deckOptions: DeckOptionsPageModel {
        if let deckOptionsOwner { return deckOptionsOwner }
        let value = DeckOptionsPageModel(owner: self)
        deckOptionsOwner = value
        navigation.onDeckOptionsLeft = { [weak value] in value?.pageLeft() }
        return value
    }
    /// N20.9's entries: the deck's page, then its Deck Options pushed on it.
    public func openDeckOptions(deckID: Int64) {
        guard deckOptions.canOpen(deckID: deckID) else { return }
        Task {
            if navigation.section != .home || navigation.homeDeckPage?.rawValue != deckID {
                guard await openDeckPage(deckID: deckID).value else { return }
            }
            guard deckOptions.canOpen(deckID: deckID) else { return }
            deckOptions.open(deckID: deckID)
            navigation.setPath([.deckOptions(DeckID(deckID))], for: .home)
        }
    }
    /// Deck Options reads and saves under the deck name writers' admission.
    var deckOptionsAllowed: Bool {
        deckNamesAllowed(host: .main, route: nil) && deckNamesOwner?.busy != true
    }

    private var deckDeletionOwner: DeckDeletionModel?
    public var deckDeletion: DeckDeletionModel {
        if let deckDeletionOwner { return deckDeletionOwner }
        let value = DeckDeletionModel(owner: self)
        deckDeletionOwner = value
        return value
    }

    private var deckNamesReserved: Bool {
        if case .deckName = navigation.sheet { return true }
        if deckOptionsOwner?.phase == .saving { return true }
        return deckNamesOwner?.busy == true || undoOwner?.busy == true || deckDeletionOwner?.busy == true
    }
    private var undoOwner: CollectionUndoModel?
    /// Edit › Undo and Redo outside review (N20.4, R173).
    public var collectionUndo: CollectionUndoModel {
        if let undoOwner { return undoOwner }
        let value = CollectionUndoModel(owner: self)
        undoOwner = value
        value.scheduleRefresh()
        return value
    }
    /// After an undo or redo: the tree and the shown page read the collection again.
    /// `intent` is the navigation when the undo or redo started.
    func collectionUndoFinished(intent: UInt64) async {
        await loadCounts()
        // The page shown reads again, unless the person went elsewhere since the
        // command started, or is not on Home.
        if intent == navigation.intentRevision, navigation.section == .home, let deck = navigation.homeDeckPage?.rawValue, self.deck(deck) != nil {
            _ = await openDeckPage(deckID: deck).value
        }
    }
    /// Find…, ⇧⌘O and "/" (N20.1, N20.4): the deck workspace shows (the Home
    /// section), with no review and no sheet or panel owning the keys. The tab
    /// layout's Search belongs to its deck list (N14.2).
    public func deckSearchAvailable(layout: ShellLayout) -> Bool {
        guard case .ready = state, review == nil, navigation.review == nil, navigation.sheet == nil,
              navigation.section == .home else { return false }
        return layout == .sidebar || !decks.isEmpty
    }
    func collectionUndoFailed() { error = "Couldn’t undo or redo that change. Try again." }
    func deckNamesAllowed(host: AppNavigation.SheetHost, route: SheetRoute?) -> Bool {
        guard host == .main, foreground, !resuming, !closed, !closing, !sync.providerHandoffInProgress,
              !sync.meguriCollectionWorkHeld,
              !opening, !starting, state == .ready,
              case .ready(let current) = lifecycle, current == generation,
              review == nil, navigation.review == nil, undoOwner?.busy != true,
              entryOwner?.busy != true, entryOwner?.pickerPresented != true, entryOwner?.preview == nil,
              exportOwner?.busy != true,
              exportOwner?.artifact == nil, exportOwner?.providerLease == nil,
              manualOwner?.busy != true, manualOwner?.preview == nil, manualOwner?.mediaPreview == nil,
              deckOptionsOwner?.phase != .saving,
              deckDeletionOwner?.busy != true,
              navigation.sheet == route else { return false }
        return route == nil || navigation.sheetHost == .main
    }
    /// A deck write with no sheet of its own (N20.7): no other deck write runs.
    func deckWriteAllowed(host: AppNavigation.SheetHost) -> Bool {
        deckNamesOwner?.busy != true && deckNamesAllowed(host: host, route: nil)
    }
    /// After a deck write outside the name form: Edit's titles peek again.
    func deckWriteFinished() { undoOwner?.scheduleRefresh() }
    /// The deck and its descendants in the tree.
    func deckSubtree(_ id: Int64) -> Set<Int64> {
        func ids(_ node: CountedDeckNode) -> [Int64] { [node.id] + node.children.flatMap(ids) }
        return Set(deck(id).map(ids) ?? [id])
    }

    public func fullDeckName(_ id: Int64) -> String? {
        func find(_ nodes: [CountedDeckNode], parents: [String]) -> String? {
            for node in nodes {
                let path = node.name.contains("::") ? node.name : (parents + [node.name]).joined(separator: "::")
                if node.id == id { return path }
                if let match = find(node.children, parents: path.components(separatedBy: "::")) { return match }
            }
            return nil
        }
        return find(decks, parents: []) ?? currentSelectedDeckDetail.flatMap { $0.id == id ? $0.name : nil }
    }
    func refreshDeckNames(generation expected: UInt64) async throws -> Bool {
        deckTreeRevision &+= 1; countsRevision &+= 1
        let revision = deckTreeRevision
        let tree = try await loadDeckTree()
        guard expected == generation, tree.generation == expected, revision == deckTreeRevision,
              foreground, !closed, !closing, !resuming else { return false }
        decks = tree.nodes
        return true
    }
    private let exportService: any CollectionExportService
    public var collectionExport: CollectionExportCoordinator {
        if let exportOwner { return exportOwner }
        let value = CollectionExportCoordinator(owner: self, service: exportService, recordDiagnostic: configuration.recordDiagnostic)
        exportOwner = value
        return value
    }
    private var manualOwner: ManualTransferCoordinator?
    private var releasedManualHostState: State?
    public var manualTransfers: ManualTransferCoordinator {
        if let manualOwner { return manualOwner }
        let location = CoreBridge.CollectionLocation(collectionDirectory: configuration.collectionLocation.directory,
            backupsDirectory: configuration.collectionLocation.backupsDirectory,
            appGroupSnapshot: configuration.collectionLocation.directory.appending(path: "meguri-widget-unavailable.json"),
            manualTransferDirectory: providerDirectory)
        let value = ManualTransferCoordinator(owner: self, service: CoreManualTransferService(location: location,
            clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone)))
        manualOwner = value
        return value
    }
    /// A failed first server preparation must not reserve the host against Files.
    /// Called after terminal drain only, with no remaining preview authority.
    func releaseEmptyManualHost() async {
        guard !opening else { return }
        guard let current = try? await collection.lifecycle() else { return }
        let restored: State
        switch current {
        case .noCollection: restored = .noFile
        case .failed(let reason): restored = .failed(Self.explanation(reason))
        default: return
        }
        releasedManualHostState = restored
        guard await sync.cancelAndDrain() else { return }
        do { try await collection.close() }
        catch { self.error = "Couldn’t close the collection. Try again."; return }
        lifecycle = .noCollection; state = restored
    }
    var exportAllowed: Bool {
        guard !opening, !deckNamesReserved, !sync.providerHandoffInProgress,
              !sync.meguriCollectionWorkHeld, state == .ready, case .ready = lifecycle else { return false }
        return true
    }
    @ObservationIgnored var loadDeckTree: @MainActor () async throws -> CountedDeckTree
    @ObservationIgnored var saveSelectedDeck: @MainActor (Int64) async throws -> UInt64
    @ObservationIgnored var saveStudyCollapse: @MainActor ([Int64], Bool) async throws -> UInt64
    private var selectionRevision: UInt64 = 0
    private var selectionTask: Task<Bool, Never>?
    private var collapseRevision: UInt64 = 0
    private var collapseTask: Task<Void, Never>?
    private var pendingStudyCollapse: [Int64: (revision: UInt64, collapsed: Bool)] = [:]
    private let records: ReviewRecordStore
    private var resolver: MediaPathResolver?
    private var countsTask: Task<Void, Never>?
    private var countTimer: Timer?
    private var countsRevision: UInt64 = 0
    private var deckTreeRevision: UInt64 = 0
    private var lifecycleTask: Task<Void, Never>?
    private var openTask: Task<Void, Never>?
    private var cloudPreparationTasks: [ObjectIdentifier: Task<Void, any Error>] = [:]
    private var cloudHostPreparationTask: Task<Void, any Error>?
    private var meguriSignInPreparationTask: Task<Void, Never>?
    public private(set) var meguriSignInPreparing = false
    private var cloudActivationCompletion: Task<Void, Never>?
    private var meguriActivationAttachment: (sync: SyncCoordinator, generation: UInt64, noFile: Bool)?
    private var opening = false
    private var preparedGeneration: UInt64?
    private var starting = false
    @ObservationIgnored private var studySyncFacts: StudyPreparationSyncFacts?
    private var closed = false
    private var closing = false
    private var closeTask: Task<Bool, Never>?
    private var day: ReviewDayMonitor?
    /// The activity heatmap's cache (N14.11), kept above the layouts so a revisited page draws at once.
    public let activity = ActivityHeatmapStore()
    private var recoveryDrops: [Int64] = []

    public init(configuration: LaunchConfiguration, navigation: AppNavigation, entryService: (any CollectionEntryService)? = nil, exportService: (any CollectionExportService)? = nil, sync: SyncCoordinator? = nil, providerDirectory: URL? = nil) {
        loadMediaIndex = { @MainActor in try await CoreMediaService().trackedNames() }
        loadDeckTree = { @MainActor in try await CoreDeckService().treeWithCounts() }
        saveSelectedDeck = { @MainActor id in try await CoreDeckService().selectDeck(id: id).generation }
        saveStudyCollapse = { @MainActor ids, collapsed in
            try await CoreDeckService().collapseDecks(ids: ids, scope: .study, collapsed: collapsed).generation
        }
        self.exportService = exportService ?? CoreCollectionExportService()
        self.configuration = configuration; self.navigation = navigation
        self.providerDirectory = providerDirectory ?? CoreBridge.CollectionLocation(
            collectionDirectory: configuration.collectionLocation.directory,
            backupsDirectory: configuration.collectionLocation.backupsDirectory,
            appGroupSnapshot: configuration.collectionLocation.directory.appending(path: "meguri-widget-unavailable.json")).manualTransferDirectory
        self.sync = sync ?? SyncCoordinator(directory: configuration.collectionLocation.directory, clock: configuration.clock,
            recordDiagnostic: configuration.recordDiagnostic)
        self.entryService = entryService ?? CoreCollectionEntryService(clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone),
            inspectionSeed: { [input = configuration.nativeReviewInput] in
                if let input { return try await input.inspectionSeed() }
                return UInt64.random(in: 0...UInt64.max)
            })
        collection = CoreCollectionService(clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone))
        records = ReviewRecordStore(file: configuration.collectionLocation.directory.appending(path: "meguri-review-session.json"))
        configureSyncOwnership()
        configureStudyPreparation()
        navigation.deckSelectionAction = { [weak self] deck in
            self?.openDeckPage(deckID: deck.rawValue)
        }
    }

    private func configureSyncOwnership() {
        sync.configureProviderDirectory(providerDirectory)
        let originalSync = sync
        sync.prepareEditorReplacement = { [weak self, weak originalSync] operation, decision, current in
            guard let self, let originalSync, self.sync === originalSync else { return nil }
            return await self.prepareEditorReplacement(operation: operation, decision: decision) {
                self.sync === originalSync && current()
            }
        }
        sync.finishEditorReplacement = { [weak self] id in self?.finishEditorReplacement(id) }
        sync.editorReplacementIsCurrent = { [weak self, weak originalSync] id in
            guard let self, let originalSync, self.sync === originalSync else { return false }
            return self.editorReplacementIsCurrent(id)
        }
        sync.finishCloudEditorReplacement = { [weak self] id in
            guard let self else { return }
            await self.cloudActivationCompletion?.value
            self.finishEditorReplacement(id)
        }
        sync.beginMeguriActivation = { [weak self, weak originalSync] reservation in
            guard let self, let originalSync, self.sync === originalSync else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
            try self.beginManualActivation(replacement: reservation)
            self.meguriActivationAttachment = (self.sync, self.generation,
                self.state == .noFile && self.lifecycle == .noCollection)
        }
        sync.finishMeguriActivation = { [weak self] candidateActive in
            guard let self else { return nil }
            let original = self.meguriActivationAttachment
            defer { self.meguriActivationAttachment = nil }
            await self.sync.waitForReplacement()
            await self.mediaCloseTask?.value
            // Lifecycle is actual evidence even when cancellation or a thrown
            // terminal read could not report whether activation happened.
            let resulting: CollectionLifecycle
            do { resulting = try await self.collection.lifecycle() }
            catch { _ = await self.finishEntryActivation(candidateActive: candidateActive); return nil }
            let changed: Bool
            if case .ready(let next) = resulting { changed = next != self.preparedGeneration }
            else { changed = false }
            let adopted = await self.finishEntryActivation(candidateActive: candidateActive || changed)
            guard !self.closing, !self.closed else { return nil }
            if adopted { return self.sync }
            // A failed first download can retain the original valid Host with
            // no collection. This is local refresh authority, not activation or
            // permission to reprepare. A new/failed/retired attachment refuses.
            guard !candidateActive, !changed, let original, original.noFile,
                  self.sync === original.sync, self.generation == original.generation,
                  self.state == .noFile, self.lifecycle == .noCollection else { return nil }
            do {
                guard try await original.sync.validateRetainedMeguriAttachment(generation: original.generation),
                      self.sync === original.sync, self.generation == original.generation,
                      !self.closing, !self.closed, self.state == .noFile,
                      self.lifecycle == .noCollection else { return nil }
                return original.sync
            } catch { return nil }
        }
        sync.providerChangeAllowed = { [weak self] in
            guard let self else { return false }
            return !closed && !closing && !opening && !starting && !enabling && !resuming && !meguriSignInPreparing
                && review == nil && navigation.review == nil && !deckNamesReserved
                && entryOwner?.busy != true && entryOwner?.preview == nil && entryOwner?.pickerPresented != true
                && exportOwner?.busy != true && exportOwner?.artifact == nil && exportOwner?.providerLease == nil
                && manualOwner?.busy != true && manualOwner?.preview == nil && manualOwner?.mediaPreview == nil
        }
        sync.drainManualProviderOwner = { [weak self] in await self?.manualOwner?.cancelAndDrain() }
        sync.attachProviderSuccessor = { [weak self] original in
            guard let self, self.sync === original, !self.closing, !self.closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            let expected = self.generation
            let current = try await original.makeProviderSuccessor(generation: expected)
            guard self.sync === original, self.generation == expected, !self.closing, !self.closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            self.sync = current
            self.configureSyncOwnership()
            try await self.attachCurrentProvider(current)
        }
        sync.retryProviderAttachment = { [weak self] current in
            guard let self, self.sync === current, !self.closing, !self.closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            let expected = self.generation
            // A failed refresh may still own a fresh native backend. Join that
            // successor before retrying its factory; the original stays closed.
            guard await current.drainProviderAttachment() else {
                throw BridgeError.unavailable(reason: "Provider attachment drain failed")
            }
            guard self.sync === current, expected == self.generation, !self.closing, !self.closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            try await self.attachCurrentProvider(current)
        }
    }

    private func attachCurrentProvider(_ current: SyncCoordinator) async throws {
        let expected = generation
        do {
            guard sync === current, !closing, !closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            await current.preparePlatform()
            guard sync === current, generation == expected, !closing, !closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            // Use the retained Host's ordinary setup path. Attachment failures
            // belong to this current coordinator and its explicit Retry action.
            try await prepareLocalCloudConnectionHost()
            guard sync === current, generation == expected, !closing, !closed else {
                throw BridgeError.unavailable(reason: "collectionUnavailable")
            }
            try current.requireProviderAttachment()
        } catch {
            if sync === current, generation == expected, !closing, !closed {
                current.providerAttachmentFailed(error)
            }
            throw error
        }
    }

    public func start() async {
        guard state == .unopened || state == .noFile || isFailed, !opening else { return }
        #if DEBUG
        openFailureDiagnosticValue = nil
        #endif
        opening = true
        let task = Task { await openCollection() }
        openTask = task
        await task.value
        // The collection is presented and usable by now (F5.1). Only this
        // caller waits, so that a launch ends with the step that follows it.
        await studyPreparation.waitForJob()
    }
    public var canStartEmpty: Bool {
        state == .noFile && lifecycle == .noCollection && foreground && sceneActive && applicationActive
            && !closed && !closing && !resuming && entryAllowed
            && navigation.sheet == nil && navigation.review == nil
            && entryOwner?.preview == nil && entryOwner?.pickerPresented != true
            && entryOwner?.requiresRecovery != true
            && exportOwner?.artifact == nil && exportOwner?.providerLease == nil
            && manualOwner?.preview == nil && manualOwner?.mediaPreview == nil
            && !sync.meguriBusy && !sync.deviceSettingsSettling
    }
    /// Reserve synchronously before yielding, just like entry confirmation.
    /// Close/background already join openTask, including host completion.
    public func startEmpty() async {
        guard canStartEmpty else { return }
        do { try beginEntryActivation() }
        catch { return }
        state = .loading; error = nil
        let task = Task { await createEmptyCollection() }
        openTask = task
        await task.value
    }
    private func createEmptyCollection() async {
        do {
            try await drainEntryActivation()
            let location = try await prepareEntry()
            try Task.checkCancellation()
            guard foreground, sceneActive, applicationActive, !closing, !closed else {
                throw BridgeError.unavailable(reason: "collection creation is no longer foreground")
            }
            let seed = try await configuration.nativeReviewInput?.reopenSeed() ?? UInt64.random(in: 0...UInt64.max)
            let job = try await collection.startCreateEmpty(location, randomSeed: seed)
            let updates = Task { for await update in job.progress { progress = update } }
            let outcome = await job.outcome()
            await updates.value
            progress = nil
            switch outcome {
            case .succeeded:
                _ = await finishEntryActivation(candidateActive: true)
                if state != .ready {
                    state = .creationRecovery
                    error = "Meguri couldn’t finish starting an empty collection."
                }
                return
            case .cancelled: throw BridgeError.cancelled
            case .failed(let error): throw error
            }
        } catch {
            progress = nil
            await reconcileEmptyCreationFailure(error)
        }
    }
    // Also exercised with real retained lifecycles by the focused regression.
    func reconcileEmptyCreationFailure(_ failure: any Error) async {
        // Tracking can fail after Rust retained/opened the Host. Complete
        // only the actual ready lifecycle; never invent a generation.
        let actual = try? await collection.lifecycle()
        let ready: Bool = { if case .ready? = actual { true } else { false } }()
        _ = await finishEntryActivation(candidateActive: ready)
        // Keep reconciled NoCollection/Ready usable. A failed or unknown
        // lifecycle may retain a published file and Host: ordinary startOpen
        // cannot reserve that Host, and creation must never be repeated there.
        if state != .noFile && state != .ready { state = .creationRecovery }
        error = Self.emptyCreationExplanation(failure)
    }
    static func emptyCreationExplanation(_ error: any Error) -> String {
        if let bridge = error as? BridgeError {
            switch bridge {
            case .cancelled: return "Starting an empty collection was cancelled."
            case .busy, .unavailable, .sessionActive:
                return "Meguri couldn’t start an empty collection while the collection is busy or unavailable."
            case .insufficientSpace:
                return "There isn’t enough free space to start an empty collection."
            case .open:
                return "Meguri couldn’t open the collection while starting empty."
            default: break
            }
        }
        return "Meguri couldn’t finish starting an empty collection."
    }
    private func openCollection() async {
        opening = true; state = .loading; error = nil; closed = false; releasedManualHostState = nil
        defer { opening = false }
        configuration.recordDiagnostic(.init(component: .startup, operation: .open, stage: .requested, outcome: .observed))
        #if DEBUG
        var openPhase = OpenFailureDiagnostic.Phase.platformPreparation
        #endif
        do {
            await sync.preparePlatform()
            try await configuration.nativeReviewInput?.prepare()
            #if DEBUG
            openPhase = .eventSubscription
            #endif
            try await subscribe()
            #if DEBUG
            openPhase = .pathPreparation
            #endif
            let location = try await Self.prepareOpenLocation(configuration.collectionLocation, providerDirectory: providerDirectory)
            guard await Self.exists(configuration.collectionLocation.collectionFile) else {
                state = .noFile
                configuration.recordDiagnostic(.init(component: .startup, operation: .open, stage: .admission, outcome: .refused, failure: .unavailable))
                return
            }
            #if DEBUG
            openPhase = .seedPreparation
            #endif
            let seed = try await configuration.nativeReviewInput?.reopenSeed() ?? UInt64.random(in: 0...UInt64.max)
            #if DEBUG
            openPhase = .coreStartOpen
            do {
                let job = try await collection.startOpen(location, randomSeed: seed)
                openPhase = .coreResult
                try await job.value()
            }
            openPhase = .postOpenHostSetup
            #else
            try await collection.startOpen(location, randomSeed: seed).value()
            #endif
            try await completeStartupHost()
            #if DEBUG
            openFailureDiagnosticValue = nil
            #endif
        } catch {
            configuration.recordDiagnostic(.init(component: .startup, operation: .open, stage: .activation, outcome: .failed, failure: .other))
            let message = Self.openExplanation(error)
            state = .failed(message); self.error = message
            #if DEBUG
            // Query the global lifecycle, including when no ready Host survived.
            // Event delivery may already have been replaced by the generic catch.
            let snapshot = try? await collection.lifecycle()
            openFailureDiagnosticValue = OpenFailureDiagnostic.value(phase: openPhase, error: error, lifecycle: snapshot)
            #endif
        }
    }
    // Also used by focused tests with the real retained lifecycle, including
    // the nonthrowing unavailable exit of host completion.
    func completeStartupHost() async throws {
        try await completeOpenedHost(loadRecord: true)
        let ready = state == .ready && lifecycle == .ready(generation: generation)
        configuration.recordDiagnostic(.init(component: .startup, operation: .open, stage: .activation,
            outcome: ready ? .succeeded : .unknown, failure: ready ? nil : .unavailable))
    }
    private func subscribe() async throws {
        try await ChangeFanout.shared.subscribe(self)
    }
    /// Called only after the FFI job retains the host, for open AND first import.
    private func completeOpenedHost(loadRecord: Bool) async throws {
        lifecycle = try await collection.lifecycle()
        guard case .ready(let value) = lifecycle else { state = .unavailable; return }
        let changed = generation != value
        if changed { invalidateDeckActions(); exportOwner?.generationChanged() }
        generation = value; state = .ready; error = nil; closed = false; releasedManualHostState = nil
        // The retained host's history, also after a replacement that kept the generation.
        undoOwner?.scheduleRefresh()
        try sync.releaseEntryActivation()
        sync.setMeguriLocalCollection(true)
        let startupForeground: (@MainActor () -> Bool)?
        if loadRecord {
            startupForeground = { [weak self] in
                guard let self else { return false }
                return self.foreground && !self.closed && !self.closing && self.state == .ready
            }
        } else {
            startupForeground = nil
        }
        // The open-time exchange may end before the steps below do (F5.1).
        studyPreparation.hostOpened()
        await sync.completeOpenedHost(generation: value, startupForeground: startupForeground)
        if loadRecord {
            do { previousRecord = try await records.load() }
            catch { self.error = "Couldn’t read the previous session record. \(error)" }
            if var previousRecord, !recoveryDrops.isEmpty {
                for card in recoveryDrops { previousRecord.reject(cardID: card) }
                self.previousRecord = previousRecord
                try await records.save(previousRecord, version: 1)
            }
        }
        recoveryDrops.removeAll()
        day?.close()
        day = ReviewDayMonitor(clock: configuration.clock, zone: { [configuration] in configuration.timeZone ?? .autoupdatingCurrent },
            didCheck: { [weak self] in self?.refreshCounts(); self?.review?.requestRefresh() })
        if foreground { await day?.checkNow() }
        await refreshMedia()
        // Presented whatever the status: the read never waits for the job (F5.1).
        await studyPreparation.collectionOpened()
        await loadCounts()
        preparedGeneration = generation
    }
    func prepareEntry() async throws -> CoreBridge.CollectionLocation {
        await sync.preparePlatform()
        try await configuration.nativeReviewInput?.prepare()
        try await subscribe()
        return try await Self.prepareOpenLocation(configuration.collectionLocation, providerDirectory: providerDirectory)
    }
    /// Selection needs local Host admission before handoff can cancel the old
    /// account owner. It must not join that owner's account-authority wait.
    public func prepareSyncServiceSelection() async throws { try await prepareLocalCloudConnectionHost() }
    /// Explicit account action only. Preparing a NoCollection Host grants no
    /// collection choice, transfer or permission to create a collection.
    public func signInMeguri(endpoint: String, username: String, password: String) {
        prepareMeguriLocalAction { original in
            guard original.meguriSignInAvailable else { throw MeguriSignInLocalFailure.busy }
            original.signInMeguri(endpoint: endpoint, username: username, password: password)
        }
    }
    /// Sign-in has one local preparation owner. The attached
    /// coordinator retains its ordinary authentication and settings-write lanes.
    private func prepareMeguriLocalAction(action: @escaping @MainActor (SyncCoordinator) throws -> Void) {
        guard meguriSignInPreparationTask == nil else { return }
        let original = sync
        guard foreground, !closing, !closed,
              original.providerState?.selection == .meguri, !original.providerNeedsAttention,
              !original.settings.paused, !original.meguriBusy else { return }
        guard !opening, !starting, !enabling, !resuming else {
            original.failMeguriSignInPreparation(.busy); return
        }
        let edit = original.meguriSignInAddressRevision
        let initialGeneration = generation, initialRevision = original.meguriSignInAttachmentRevision
        original.clearMeguriSignInPreparationError()
        meguriSignInPreparing = true
        meguriSignInPreparationTask = Task {
            defer { meguriSignInPreparing = false; meguriSignInPreparationTask = nil }
            @MainActor func current() -> Bool {
                sync === original && foreground && !closing && !closed && !opening && !starting
                    && !enabling && !resuming && !Task.isCancelled
                    && edit == original.meguriSignInAddressRevision
            }
            do {
                guard current(), generation == initialGeneration,
                      original.meguriSignInAttachmentRevision == initialRevision else { return }
                if !original.meguriSignInAttached { try await prepareCloudConnectionHost() }
                guard current() else { return }
                let expected = generation, revision = original.meguriSignInAttachmentRevision
                // Attachment may have issued its ordinary offline connection
                // load. Join that actual task before the explicit local action.
                await original.waitForMeguriAction()
                guard current(), generation == expected, original.generation == expected,
                      original.meguriSignInAttachmentRevision == revision else { return }
                guard original.meguriSignInAttached else { throw MeguriSignInLocalFailure.attachment }
                guard original.meguriLocalActionAvailable else { throw MeguriSignInLocalFailure.busy }
                try action(original)
            } catch {
                guard current() else { return }
                let failure: MeguriSignInLocalFailure
                if let local = error as? MeguriSignInLocalFailure { failure = local }
                else if case BridgeError.busy = error { failure = .busy }
                else if error is MeguriProviderCredentialFailure { failure = .credentialStore }
                else { failure = .attachment }
                original.failMeguriSignInPreparation(failure)
            }
        }
    }
    func waitForMeguriSignInPreparation() async { await meguriSignInPreparationTask?.value }
    /// Account-dependent callers share this wait, separately from local Host
    /// readiness so selection can reach the guarded handoff's cancellation.
    func prepareCloudConnectionHost() async throws {
        let original = sync, owner = ObjectIdentifier(sync)
        if let task = cloudPreparationTasks[owner] { try await task.value; return }
        guard !closing, !closed else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
        let task = Task {
            defer { cloudPreparationTasks[owner] = nil }
            try await prepareLocalCloudConnectionHost()
            let expected = generation, revision = original.meguriSignInAttachmentRevision
            guard sync === original, original.generation == expected else { throw CancellationError() }
            await original.prepareConnectionAuthority()
            guard sync === original, generation == expected, original.generation == expected,
                  original.meguriSignInAttachmentRevision == revision else { throw CancellationError() }
            guard original.connectionService != nil else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
        }
        // Keep retired waits retained until their actual completion, without
        // making a successor join them or letting their cleanup erase its wait.
        cloudPreparationTasks[owner] = task
        try await task.value
    }
    private func prepareLocalCloudConnectionHost() async throws {
        if let cloudHostPreparationTask { try await cloudHostPreparationTask.value; return }
        guard !closing, !closed else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
        let task = Task {
            defer { cloudHostPreparationTask = nil }
            if let openTask { await openTask.value }
            guard !closing, !closed, !opening else { throw BridgeError.busy(runningOperation: "collection preparation") }
            let location = try await prepareEntry()
            try Task.checkCancellation()
            guard !closing, !closed else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
            guard sync.setupState != .configurationUnavailable else { throw BridgeError.unavailable(reason: "configuration") }
            if sync.connectionService != nil { return }
            // An existing file follows the owner's ordinary open path. Neither
            // discovery nor its UI may create or replace that file.
            if await Self.exists(configuration.collectionLocation.collectionFile), state != .ready {
                await start()
                guard sync.connectionService != nil else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
                return
            }
            let seed = try await configuration.nativeReviewInput?.reopenSeed() ?? UInt64.random(in: 0...UInt64.max)
            let retainedGeneration = try await collection.prepareCloudConnectionHost(location, randomSeed: seed)
            guard !closing, !closed else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
            lifecycle = try await collection.lifecycle()
            if case .noCollection = lifecycle { state = .noFile; generation = retainedGeneration }
            sync.setMeguriLocalCollection(false)
            await sync.completeOpenedHost(generation: retainedGeneration)
            try sync.requireProviderAttachment()
            guard sync.connectionService != nil else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
        }
        cloudHostPreparationTask = task
        try await task.value
    }
    var entryAllowed: Bool { allowsEntryActivation() }
    var manualAllowed: Bool { allowsManualActivation() }
    private func allowsManualActivation(entry: CollectionEntryCoordinator? = nil) -> Bool {
        !meguriSignInPreparing && !sync.providerHandoffInProgress && cloudPreparationTasks.isEmpty && cloudHostPreparationTask == nil && !deckNamesReserved &&
            (entryOwner?.busy != true || (entry != nil && entryOwner === entry)) && exportOwner?.busy != true &&
            !opening && !starting && review == nil && (state == .ready || state == .noFile || state == .unavailable || isFailed)
    }
    private func allowsEntryActivation(entry: CollectionEntryCoordinator? = nil) -> Bool {
        !sync.meguriCollectionWorkHeld && (state == .ready || manualOwner?.preview == nil) &&
            allowsManualActivation(entry: entry) && state != .unavailable
    }
    func beginManualActivation(replacement: UUID? = nil, discovery: Bool = false) throws {
        if let replacement, !editorReplacementIsCurrent(replacement) { throw BridgeError.busy(runningOperation: "Add Note") }
        guard !closed, !closing, addFlowOwner?.opening != true,
              editorReplacement?.id == replacement,
              discovery || addFlowOwner?.isPresented != true else { throw BridgeError.busy(runningOperation: "Add Note") }
        guard review == nil, !starting else { throw BridgeError.sessionActive }
        guard manualAllowed else { throw BridgeError.busy(runningOperation: "collection preparation") }
        opening = true
        day?.pause(); countTimer?.invalidate(); countsTask?.cancel()
    }
    func beginEntryActivation(replacement: UUID? = nil, entry: CollectionEntryCoordinator? = nil) throws {
        if let replacement, !editorReplacementIsCurrent(replacement) { throw BridgeError.busy(runningOperation: "Add Note") }
        guard !closed, !closing, addFlowOwner?.opening != true,
              editorReplacement?.id == replacement,
              addFlowOwner?.isPresented != true else { throw BridgeError.busy(runningOperation: "Add Note") }
        guard review == nil, !starting else { throw BridgeError.sessionActive }
        guard (replacement == nil && entry == nil && entryAllowed) ||
                (replacement != nil && entryOwner === entry && allowsEntryActivation(entry: entry)) else {
            throw BridgeError.busy(runningOperation: "collection preparation")
        }
        opening = true
        day?.pause(); countTimer?.invalidate(); countsTask?.cancel()
        sync.beginEntryActivation()
    }
    func drainEntryActivation() async throws {
        guard await sync.drainEntryActivation() else { throw BridgeError.busy(runningOperation: "iCloud drain") }
    }
    @discardableResult func finishEntryActivation(candidateActive: Bool) async -> Bool {
        defer { opening = false }
        do {
            if candidateActive {
                navigation.collectionReplaced()
                previousRecord = nil; recoveryDrops.removeAll(); dropWalk()
                try await records.remove()
                try await completeOpenedHost(loadRecord: false)
            } else {
                lifecycle = try await collection.lifecycle()
                let syncWasHeld = try sync.releaseEntryActivation()
                switch lifecycle {
                case .ready(let next): generation = next; state = .ready
                    if syncWasHeld { await sync.completeOpenedHost(generation: next) }
                    if foreground { await day?.checkNow() }
                    await loadCounts()
                case .noCollection: state = .noFile
                default: state = .unavailable
                }
            }
            return state == .ready && error == nil
        } catch {
            let message = Self.openExplanation(error)
            self.error = message; state = .failed(message); return false
        }
    }
    // Open/lifecycle presentation accepts typed failures only; diagnostic detail
    // can contain paths or internal messages and must never reach Home.
    static func openExplanation(_ error: any Error) -> String {
        if let bridge = error as? BridgeError {
            switch bridge {
            case .open(let failure): return explanation(failure)
            case .busy, .unavailable:
                return "The collection is busy or unavailable. Wait for the current operation to finish, then try again."
            default: break
            }
        }
        return "Meguri couldn’t open this collection. Try again, or import a collection in Settings."
    }
    static func explanation(_ reason: CollectionFailedReason) -> String {
        switch reason {
        case .open(let failure): return explanation(failure)
        case .corrupt: return explanation(CollectionOpenFailure.corrupt(detail: ""))
        case .openStep:
            return "Meguri couldn’t finish opening this collection. Try again, or quit and reopen Meguri."
        case .replace:
            return "Meguri couldn’t finish replacing this collection. Try opening it again, or import a collection in Settings."
        }
    }
    static func explanation(_ failure: CollectionOpenFailure) -> String {
        switch failure {
        case .openInAnotherProgram:
            return "This collection is open in another app. Close it there, then try again."
        case .tooOld:
            return "This collection uses an older format that Meguri can’t open. Update it in a compatible app, then import a latest-format .colpkg in Settings."
        case .tooNew:
            return "This collection uses a newer format than Meguri supports. Check for a Meguri update."
        case .v1Scheduler:
            return "This collection uses an unsupported older scheduler. Update its scheduler in a compatible app before opening it in Meguri."
        case .notACollection:
            return "This file isn’t a supported collection. Import a latest-format .colpkg in Settings."
        case .corrupt:
            return "This collection appears to be damaged or incomplete. Import a known-good .colpkg in Settings."
        case .upgradeFailed:
            return "Meguri couldn’t finish updating this collection’s format. Check free space and file access, then try again."
        case .connectionInit:
            return "Meguri couldn’t prepare this collection for use. Quit and reopen Meguri, then try again."
        case .notFound:
            return "The collection file is no longer available. Try again, or import a collection in Settings."
        case .io:
            return "Meguri couldn’t read this collection. Check free space and file access, then try again."
        }
    }
    private var isFailed: Bool { if case .failed = state { return true }; return false }

    func receive(_ event: CoreEvent) {
        guard !closed else { return }
        // Never drop review commit failures behind a coalesced deck refresh.
        review?.receive(event)
        sync.coreEvent(event)
        routeActivity(event)
        switch event.kind {
        case .lifecycle(let value):
            receiveLifecycle(value)
        case .nextDayAtChanged(let instant): if !opening { day?.event(nextDayAt: instant) }
        case .rolledOver, .timeZoneChanged: refreshCounts()
        case .changes(let changes):
            refreshCounts()
            // Any write may be a new step (a text-only note edit changes no count); coalesced.
            if event.generation == generation { undoOwner?.scheduleRefresh() }
            if event.generation == generation { deckOptionsOwner?.changesCommitted(changes, generation: event.generation) }
            if event.generation == generation,
               changes.wholeCollection || !changes.notetypes.isEmpty || !changes.decks.isEmpty {
                addFlowOwner?.selectionChanged(generation: event.generation)
            }
            if event.generation == generation {
                editFlowOwner?.receive(changes, seq: event.seq, generation: event.generation)
            }
            if changes.wholeCollection || !changes.mediaAdded.isEmpty || !changes.mediaRemoved.isEmpty {
                scheduleMediaRefresh(names: changes.mediaAdded, generation: event.generation)
            }
            if event.generation == generation, !opening,
               changes.source == .cloudIncoming || changes.source == .syncIncoming {
                Task { await studyPreparation.changesCommitted(changes) }
            }
        case .mediaArrived(let names): scheduleMediaRefresh(names: names, generation: event.generation)
        case .collectionReplaced(let next):
            sync.collectionReplaced(next)
            guard preparedGeneration != next else { return }
            exportOwner?.generationChanged()
            invalidateDeckActions()
            addFlowOwner?.generationEnded()
            editFlowOwner?.generationEnded()
            generation = next; day?.pause()
            invalidateMedia()
            review?.generationEnded(); review = nil; navigation.endReview()
            previousRecord = nil; studyRefusal = nil; dropWalk()
            studyPreparation.collectionEnded()
            decks = []; countTimer?.invalidate(); countsTask?.cancel()
            navigation.collectionReplaced(keepingEntrySheet: opening)
            if !opening {
                navigation.dismissSheet()
                entryOwner?.destinationChanged()
                cloudActivationCompletion = Task { [weak self] in
                    guard let self else { return }
                    defer { self.cloudActivationCompletion = nil }
                    // The original NoCollection Host is retained already, but
                    // R6 reopen/step 9 and the old SDK drain must finish first.
                    await self.sync.waitForReplacement()
                    await self.mediaCloseTask?.value
                    guard self.generation == next, !self.closed, !self.closing, !self.opening else { return }
                    do { try await self.completeOpenedHost(loadRecord: false) }
                    catch { if self.generation == next { self.error = Self.openExplanation(error) } }
                }
            }
        case .recoveryDropped(_, let cardID, _): recoveryDrops.append(cardID)
        case .sessionEnded:
            if !opening { Task { await studyPreparation.sessionEnded() } }
        default: break
        }
    }

    /// The heatmap cache follows the collection's own change events (N14.11).
    private func routeActivity(_ event: CoreEvent) {
        switch event.kind {
        case .changes(let changes): activity.changesCommitted(changes)
        case .rolledOver, .timeZoneChanged, .nextDayAtChanged: activity.dayChanged()
        case .collectionReplaced(let next): activity.generationChanged(to: next)
        case .sessionEnded: activity.sessionEnded()
        // A long op released the collection: a read it refused is asked again.
        case .available: activity.collectionReady()
        default: break
        }
    }

    // Also used by focused owner tests; the event consumer is the production caller.
    func receiveLifecycle(_ value: CollectionLifecycle) {
        sync.receiveLifecycle(value)
        let wasReady: Bool = { if case .ready = lifecycle { true } else { false } }()
        lifecycle = value
        // Late create events may update observed lifecycle, but cannot restore
        // Retry against the retained Host. Recovery ends with an explicit close.
        if state == .creationRecovery { return }
        switch value {
        case .ready(let next):
            activity.generationChanged(to: next)
            activity.collectionReady()
            if generation != next {
                addFlowOwner?.generationEnded()
                editFlowOwner?.generationEnded()
                invalidateDeckActions(); invalidateMedia(); exportOwner?.generationChanged()
            }
            generation = next
            // A new collection has its own history, even with the same deck tree,
            // and a collection ready again reads its history again.
            undoOwner?.scheduleRefresh()
            if !opening {
                state = .ready
                if let review { review.onVisibleMediaChanged?(review.visibleMediaNames) }
                if !wasReady && foreground { Task { await day?.checkNow() } }
            }
        case .opening, .upgrading: invalidateDeckActions(); state = .loading
        case .exclusiveBusy: invalidateDeckActions(); state = .unavailable; review?.requestRefresh()
        case .failed(let reason): invalidateDeckActions(); state = .failed(Self.explanation(reason))
        case .noCollection: invalidateDeckActions(); activity.generationChanged(to: nil); state = releasedManualHostState ?? .unopened
        }
    }

    private var canSaveDeckState: Bool { state == .ready && !opening && !closed && !closing && !resuming }

    private func invalidateDeckActions() {
        undoOwner?.generationEnded()
        deckNamesOwner?.invalidate()
        deckOptionsOwner?.invalidate()
        deckDeletionOwner?.invalidate()
        selectedDeckDetail = nil
        selectionRevision &+= 1; collapseRevision &+= 1; countsRevision &+= 1; deckTreeRevision &+= 1
        pendingStudyCollapse.removeAll()
    }

    /// Admission is synchronous so a later row/section click wins even before a task runs.
    @discardableResult public func openDeckPage(deckID: Int64) -> Task<Bool, Never> {
        let detail = currentSelectedDeckDetail
        return openDeckPage(deckID: deckID, detail: detail?.id == deckID ? detail : nil)
    }

    func openDeckPage(_ detail: DeckPageDetail) -> Task<Bool, Never> {
        openDeckPage(deckID: detail.id, detail: detail)
    }

    private func openDeckPage(deckID: Int64, detail: DeckPageDetail?,
                              admission: @escaping @MainActor () -> Bool = { true },
                              didOpen: @escaping @MainActor () -> Void = {}) -> Task<Bool, Never> {
        let intent = navigation.beginDeckSelection()
        let selection = selectDeck(deckID: deckID, intent: intent, admission: admission)
        let expected = generation, revision = selectionRevision
        let task = Task {
            guard await selection.value, expected == generation, revision == selectionRevision,
                  canSaveDeckState, admission(), navigation.intentRevision == intent else { return false }
            if navigation.sidebarSelection != .deck(DeckID(deckID)) {
                navigation.openDeckPage(DeckID(deckID))
            }
            selectedDeckDetail = deck(deckID) == nil
                ? detail.map { (page: $0, generation: expected, intent: navigation.intentRevision) } : nil
            didOpen()
            return true
        }
        deckPageOpening = task
        deckPageOpens &+= 1
        recentDeckPageOpens.append(deckID)
        if recentDeckPageOpens.count > 16 { recentDeckPageOpens.removeFirst() }
        return task
    }

    /// Waits until no deck page is opening. A page that opens after a review started ends
    /// it, so a double-click's second click and Return or S just after a selection wait
    /// for the open their first click or the selection asked for (H12).
    public func deckPageOpened() async {
        while let task = deckPageOpening {
            _ = await task.value
            if deckPageOpening == task { deckPageOpening = nil; return }
        }
    }

    /// Serial writes preserve core selection order; superseded queued requests need no write.
    private func selectDeck(deckID: Int64, intent: UInt64, admission: @escaping @MainActor () -> Bool = { true }) -> Task<Bool, Never> {
        selectionRevision &+= 1
        let revision = selectionRevision, expected = generation, previous = selectionTask
        let task = Task {
            _ = await previous?.value
            guard revision == selectionRevision, expected == generation, canSaveDeckState,
                  navigation.intentRevision == intent, admission() else { return false }
            do {
                try configuration.nativeReviewInput?.selecting(deck: deckID, name: fullDeckName(deckID))
                let savedGeneration = try await saveSelectedDeck(deckID)
                guard revision == selectionRevision, expected == generation,
                      savedGeneration == expected, canSaveDeckState,
                      navigation.intentRevision == intent, admission() else { return false }
                if error == "Couldn’t select this deck. Try again." { error = nil }
                return true
            } catch {
                if revision == selectionRevision, expected == generation, canSaveDeckState,
                   navigation.intentRevision == intent, admission() {
                    self.error = "Couldn’t select this deck. Try again."
                }
                return false
            }
        }
        selectionTask = task
        return task
    }

    public func studyExpanded(deckID: Int64) -> Bool {
        if let pending = pendingStudyCollapse[deckID] { return !pending.collapsed }
        return deck(deckID).map { !$0.studyCollapsed } ?? false
    }

    public func isStudyExpansionPending(deckID: Int64) -> Bool {
        pendingStudyCollapse[deckID] != nil
    }

    /// Each native disclosure binds to the same owner in Home and the sidebar.
    @discardableResult public func setStudyExpanded(deckID: Int64, expanded: Bool,
                                                   includingDescendants: Bool = false) -> Task<Void, Never> {
        guard canSaveDeckState, let node = deck(deckID) else { return Task {} }
        func ids(_ node: CountedDeckNode) -> [Int64] { [node.id] + node.children.flatMap(ids) }
        let affected = includingDescendants ? ids(node) : [deckID]
        collapseRevision &+= 1; countsRevision &+= 1; deckTreeRevision &+= 1
        let revision = collapseRevision, expected = generation, previous = collapseTask
        for id in affected { pendingStudyCollapse[id] = (revision, !expanded) }
        let task = Task {
            await previous?.value
            guard expected == generation, canSaveDeckState else {
                clearPendingCollapse(affected, revision: revision)
                return
            }
            let current = affected.filter { pendingStudyCollapse[$0]?.revision == revision }
            guard !current.isEmpty else { return }
            do {
                let savedGeneration = try await saveStudyCollapse(current, !expanded)
                guard expected == generation, savedGeneration == expected, canSaveDeckState else {
                    clearPendingCollapse(current, revision: revision)
                    if expected == generation { deckTreeRevision &+= 1; refreshCounts() }
                    return
                }
                if current.contains(where: { pendingStudyCollapse[$0]?.revision == revision }),
                   error == "Couldn’t save deck expansion. Try again." { error = nil }
                // Keep identities/order/counts and browser state intact until the next query.
                decks = applyingStudyCollapse(Set(current), collapsed: !expanded, to: decks)
            } catch {
                if expected == generation, canSaveDeckState,
                   current.contains(where: { pendingStudyCollapse[$0]?.revision == revision }) {
                    self.error = "Couldn’t save deck expansion. Try again."
                }
            }
            guard expected == generation else { return }
            deckTreeRevision &+= 1
            clearPendingCollapse(current, revision: revision)
            refreshCounts()
        }
        collapseTask = task
        return task
    }

    private func clearPendingCollapse(_ ids: [Int64], revision: UInt64) {
        for id in ids where pendingStudyCollapse[id]?.revision == revision { pendingStudyCollapse[id] = nil }
    }
    private func applyingStudyCollapse(_ ids: Set<Int64>, collapsed: Bool, to nodes: [CountedDeckNode]) -> [CountedDeckNode] {
        nodes.map { node in
            CountedDeckNode(id: node.id, name: node.name, filtered: node.filtered,
                studyCollapsed: ids.contains(node.id) ? collapsed : node.studyCollapsed,
                browserCollapsed: node.browserCollapsed, counts: node.counts,
                children: applyingStudyCollapse(ids, collapsed: collapsed, to: node.children))
        }
    }

    public func refreshCounts() {
        countsRevision &+= 1
        guard countsTask == nil, foreground, state == .ready, !opening else { return }
        countsTask = Task {
            var observed: UInt64
            repeat {
                observed = countsRevision
                await loadCounts()
            } while observed != countsRevision && foreground && state == .ready && !opening && !Task.isCancelled
            countsTask = nil
        }
    }
    func loadCounts() async {
        let expected = generation, revision = deckTreeRevision
        do {
            let tree = try await loadDeckTree()
            guard expected == generation, tree.generation == generation, revision == deckTreeRevision, !closed else { return }
            decks = tree.nodes
            countsSnapshot &+= 1
            refreshTodayNumbers()
            countTimer?.invalidate(); countTimer = nil
            if foreground && !configuration.clock.isFixed && Double(tree.nextChangeAt) < Date.distantFuture.timeIntervalSince1970 {
                let date = max(Date(timeIntervalSince1970: Double(tree.nextChangeAt)),
                    Date(timeIntervalSince1970: floor(configuration.clock.now().timeIntervalSince1970) + 1))
                countTimer = Timer(fire: date, interval: 0, repeats: false) { [weak self] _ in
                    Task { @MainActor in self?.refreshCounts() }
                }
                if let countTimer { RunLoop.main.add(countTimer, forMode: .common) }
            }
        } catch { if expected == generation, revision == deckTreeRevision, !closed { self.error = "Couldn’t refresh decks. Try again." } }
    }
    public func deck(_ id: Int64) -> CountedDeckNode? {
        func find(_ nodes: [CountedDeckNode]) -> CountedDeckNode? {
            for node in nodes { if node.id == id { return node }; if let found = find(node.children) { return found } }
            return nil
        }
        return find(decks)
    }

    /// What waits in the deck, from its tree or page counts. A deck outside the
    /// tree (hidden, with a page projection) counts as a regular deck.
    public func deckWaiting(_ id: Int64) -> DeckWaiting? {
        deckPage(id).map { DeckWaiting(counts: $0.counts, filtered: deck(id)?.filtered == true) }
    }
    /// N14.3: Study is offered unless a regular deck has nothing waiting; the deck
    /// page, the deck menu and the Mac's Study command all ask here.
    public func offersStudy(deckID: Int64) -> Bool { deckWaiting(deckID)?.offersAddNote != true }

    public func deckPage(_ id: Int64) -> DeckPageDetail? {
        if let node = deck(id) {
            return DeckPageDetail(id: node.id, name: node.name, counts: node.counts)
        }
        let detail = currentSelectedDeckDetail
        return detail?.id == id ? detail : nil
    }

    public func refreshDeckPage(_ request: DeckPageRefreshRequest) async {
        let current = { self.deckPageRefreshRequest == request && !Task.isCancelled }
        guard current() else { return }
        selectedDeckEmptiness = nil
        do {
            let service = CoreDeckService()
            let before = try await service.allDeckNames()
            guard current(), before.generation == request.generation else { return }
            guard let entry = before.decks.first(where: { $0.id == request.deckID }) else {
                // N14.1: the deck is gone (deleted, or removed by a sync). Its page has
                // nothing to show, so the selection returns to the section's root.
                if navigation.review == nil, navigation.homeDeckPage?.rawValue == request.deckID {
                    navigation.select(.section(.home))
                }
                return
            }
            let state = try await service.studyState(deckID: request.deckID)
            guard current(), state.deck == request.deckID else { return }
            // studyState has no generation. Bracket it with actual stored deck
            // identity snapshots and a final ready-generation check.
            let after = try await service.allDeckNames()
            guard current(), after.generation == request.generation,
                  after.decks.first(where: { $0.id == request.deckID }) == entry else { return }
            let actualLifecycle = try await CoreCollectionService().lifecycle()
            guard current(), actualLifecycle == .ready(generation: request.generation) else { return }
            selectedDeckEmptiness = (request: request, empty: state.empty && !entry.filtered)
            // A hidden empty deck needs a page projection. Tree counts remain
            // owned by loadCounts, including after a real card is added.
            if deck(request.deckID) == nil, navigation.homeDeckPage?.rawValue == request.deckID {
                let page = DeckPageDetail(id: entry.id, name: entry.fullName, counts: state.listCounts)
                selectedDeckDetail = (page: page, generation: request.generation, intent: request.intent)
            }
        } catch {
            if current() {
                selectedDeckEmptiness = nil
                self.error = "Couldn’t refresh the deck. Try again."
            }
        }
    }

    /// Rename refreshes page authority without selecting another deck or changing navigation.
    func refreshRenamedDeckPage(_ mutation: DeckMutation) async throws {
        guard navigation.homeDeckPage?.rawValue == mutation.id else { return }
        let expected = generation, intent = navigation.intentRevision
        let detail = try await resolveDeckPage(mutation)
        guard expected == generation, intent == navigation.intentRevision,
              navigation.homeDeckPage?.rawValue == mutation.id, canSaveDeckState else { return }
        selectedDeckDetail = deck(mutation.id) == nil ? (page: detail, generation: expected, intent: intent) : nil
    }

    func resolveDeckPage(_ mutation: DeckMutation) async throws -> DeckPageDetail {
        try await resolveDeckPage(id: mutation.id, fullName: mutation.name, generation: mutation.summary.generation)
    }

    private func resolveDeckPage(id: Int64, fullName: String, generation expected: UInt64) async throws -> DeckPageDetail {
        guard expected == generation, canSaveDeckState else {
            throw BridgeError.staleGeneration(planned: expected, current: generation)
        }
        if let node = deck(id) { return DeckPageDetail(id: id, name: fullName, counts: node.counts) }
        let state = try await CoreDeckService().studyState(deckID: id)
        guard expected == generation, canSaveDeckState, state.deck == id else {
            throw BridgeError.staleGeneration(planned: expected, current: generation)
        }
        return DeckPageDetail(id: id, name: fullName, counts: state.listCounts)
    }

    /// Runs the preparation now and waits for it, as a study entry point does.
    public func enableFsrs() async { _ = await studyPreparation.prepare(for: .tryAgain) }
    public func dismissStudyRefusal() { studyRefusal = nil }
    /// Try Again of F5.3: runs the step again and goes on to study when it commits.
    public func retryRefusedStudy() async {
        guard let refusal = studyRefusal else { return }
        studyRefusal = nil
        if refusal.studyAll { await startStudyAll(entry: .tryAgain) }
        else { _ = await startStudy(deckID: refusal.deckID, entry: .tryAgain) }
    }
    private func configureStudyPreparation() {
        studyPreparation.environment = .init(
            status: { try await Core.fsrsPolicyStatus() },
            start: {
                let job = try await Core.enableFsrs()
                let reported = Task { @MainActor in
                    var last: UInt64?
                    for await value in job.progress { last = value.completed }
                    return last
                }
                return .init(cancel: { try? await job.cancel() }, finish: {
                    try await job.value()
                    return await reported.value
                })
            },
            sessionOpen: { [weak self] in self?.review?.sessionID != nil },
            syncFacts: { [weak self] in self?.sync.studyPreparationFacts ?? .none },
            nowMilliseconds: { [configuration] in Int64(configuration.clock.now().timeIntervalSince1970 * 1_000) },
            record: configuration.recordDiagnostic,
            committed: { [weak self] in self?.refreshCounts() })
        observeStudySyncFacts()
    }
    /// Reads what the service already publishes; no timer and no request (F5.1).
    private func observeStudySyncFacts() {
        let facts = withObservationTracking { sync.studyPreparationFacts } onChange: { [weak self] in
            Task { @MainActor in self?.observeStudySyncFacts() }
        }
        guard facts != studySyncFacts else { return }
        studySyncFacts = facts
        Task { await studyPreparation.syncChanged() }
    }
    public var canStudy: Bool { studyAdmissible && review == nil && walk == nil }
    /// What every start needs, a walk's later turns included (they start while its review shows).
    private var studyAdmissible: Bool { !sync.meguriCollectionWorkHeld && !deckNamesReserved && !opening && entryOwner?.busy != true && navigation.sheet != .collectionEntry && state == .ready && reviewIsActive && !resuming && !starting }
    public func startStudy(deckID: Int64) async {
        _ = await startStudy(deckID: deckID, entry: .study)
    }

    private enum StudyAttempt { case started, stopped, notReady, refusedByCore }

    /// Every study entry point ends here. A collection that is not ready for
    /// study is prepared first; the entry point shows the loading state of a
    /// starting session meanwhile and never waits for sync (F4, F5.1). When
    /// that fails, study is refused with Try Again and nothing else changes (F5.3).
    private func startStudy(deckID: Int64, entry: StudyPreparation.Entry, walk: StudyWalk? = nil) async -> Bool {
        var entry = entry
        var retry = false
        while true {
            switch await attemptStudy(deckID: deckID, entry: entry, walk: walk, retry: retry) {
            case .started: return true
            case .stopped: return false
            case .refusedByCore where entry != .refusedByCore:
                // The core found the switch off after our last read: read
                // again, run the step and go on (F1 item 5).
                entry = .refusedByCore; retry = true
            case .notReady, .refusedByCore:
                studyRefusal = .init(deckID: deckID, studyAll: walk != nil)
                return false
            }
        }
    }

    /// `retry`: the same start again after the core refused it; for a walk, the same turn.
    private func attemptStudy(deckID: Int64, entry: StudyPreparation.Entry, walk: StudyWalk? = nil,
                              retry: Bool = false) async -> StudyAttempt {
        // A walk's turn starts while its own review shows, never after it stopped.
        let walkCurrent: @MainActor @Sendable () -> Bool = { walk.map { self.walk === $0 && !$0.stopped } ?? true }
        let admitted = walk.map { _ in studyAdmissible && walkCurrent() && (review == nil || review === self.walk?.current) } ?? canStudy
        guard admitted, foreground,
              sceneActive, applicationActive, windowVisible, reviewWindowActive else { return .stopped }
        // Refused before anything is written: with no open collection to
        // prepare, and after a failed step, where Try Again runs it (F5.3).
        if !studyPreparation.available || (entry == .study && studyPreparation.phase == .failed) { return .notReady }
        starting = true
        defer { starting = false }
        let expected = generation, intent = navigation.intentRevision
        let current: @MainActor @Sendable () -> Bool = {
            expected == self.generation && intent == self.navigation.intentRevision &&
                self.canSaveDeckState && self.foreground && self.sceneActive && self.applicationActive &&
                self.windowVisible && self.reviewWindowActive && self.reviewIsActive && walkCurrent()
        }
        do {
            try configuration.nativeReviewInput?.admitStudy(deck: deckID, name: fullDeckName(deckID))
            guard await selectDeck(deckID: deckID, intent: intent, admission: current).value,
                  current(), let resolver = await mediaResolverForReview(), current() else { return .stopped }
            // A deck's own session replaces the previous one, a walk's included (N24.2). A
            // walk's turn leaves it until the walk has an answer (`closeWalk`).
            if walk == nil { previousRecord = nil; previousWalk = nil }
            let model = ReviewCoordinator(deckID: deckID, surface: makeReviewSurface(resolver),
                store: records, clock: configuration.clock, client: reviewSessionClient,
                nativeInput: configuration.nativeReviewInput, nativeGeneration: expected,
                recordDiagnostic: configuration.recordDiagnostic)
            model.nativeInputEligibility = { [weak self, weak model] in
                guard let self, let model else { return false }
                return self.generation == expected && self.reviewIsActive && self.foreground &&
                    self.sceneActive && self.applicationActive && self.windowVisible && self.reviewWindowActive &&
                    self.review === model
            }
            var refusedByCore = false
            model.onPolicyRefusal = { refusedByCore = true }
            if let walk {
                // Undo reaches the open deck's answers only (N24.3), and waits while the walk
                // moves between decks.
                model.sessionScopedUndo = true
                model.undoHeld = { [weak walk] in walk?.advancing ?? false }
                model.onTerminal = { [weak self, weak walk, weak model] in
                    guard let self, let walk, let model, walk.current === model else { return }
                    self.requestAdvance(walk)
                }
            }
            presentReview(model, walk: walk, retry: retry)
            // The session's own loading state shows while the collection is
            // prepared. No session is open yet, so the step may run.
            let prepared = await studyPreparation.prepare(for: entry)
            guard !closed, !closing, expected == generation, review === model, model.phase != .ended else { return .stopped }
            guard prepared else {
                navigation.endReview(); review = nil
                return .notReady
            }
            await model.start()
            #if DEBUG
            logStartupActivity("startup.complete", model: model, generation: expected)
            #endif
            guard !closed, !closing, expected == generation, review === model, model.phase != .ended else { return .stopped }
            // Startup has released its operation. Serialize with lifecycle work
            // now: a pause may have awaited it, or coalesced resumes may have
            // returned before the session ID arrived. Never await this in start().
            let previous = lifecycleTask
            var activityFailure: (any Error)?
            let reconciliation = Task {
                await previous?.value
                guard !closed, !closing, expected == generation, review === model, model.phase != .ended else { return }
                do {
                    if !reviewIsActive { try await model.pause() }
                    else if foreground, !resuming, self.resolver != nil { try await model.resume() }
                } catch { activityFailure = error }
            }
            lifecycleTask = reconciliation
            await reconciliation.value
            #if DEBUG
            logStartupActivity("startup.reconciled", model: model, generation: expected)
            #endif
            guard !closed, !closing, expected == generation, review === model, model.phase != .ended else { return .stopped }
            if let activityFailure { throw activityFailure }
            if model.sessionID == nil {
                // A walk keeps its review up while the step runs again for this turn.
                if refusedByCore, walk != nil { return .refusedByCore }
                navigation.endReview(); review = nil
                if refusedByCore { return .refusedByCore }
                error = model.notice
                return .stopped
            }
            return .started
        } catch {
            self.error = String(describing: error)
            return .stopped
        }
    }
    #if DEBUG
    private func logStartupActivity(_ stage: String, model: ReviewCoordinator, generation expected: UInt64) {
        let currentOwner = !closed && !closing && expected == generation && review === model && model.phase != .ended
        let logger = Logger(subsystem: "Meguri", category: "ReviewActivityStage")
        logger.debug("\(stage, privacy: .public) currentOwner=\(currentOwner, privacy: .public) reviewIsActive=\(self.reviewIsActive, privacy: .public) paused=\(model.paused, privacy: .public) foreground=\(self.foreground, privacy: .public) sceneActive=\(self.sceneActive, privacy: .public) applicationActive=\(self.applicationActive, privacy: .public) windowVisible=\(self.windowVisible, privacy: .public) reviewWindowActive=\(self.reviewWindowActive, privacy: .public) reviewVisible=\(self.reviewVisible, privacy: .public) hostCountZero=\(self.visibleReviewHosts == 0, privacy: .public) presentationAllowsInput=\(self.reviewPresentationAllowsInput, privacy: .public)")
    }
    #endif
    func presentReview(_ model: ReviewCoordinator, walk: StudyWalk? = nil, retry: Bool = false) {
        reviewVisible = true
        review = model
        let expected = generation
        model.onVisibleMediaChanged = { [weak self, weak model] names in
            guard let self, let model, !self.closed, !self.closing,
                  self.generation == expected, self.review === model else { return }
            if !names.isEmpty {
                guard case .ready(let current) = self.lifecycle, current == expected else { return }
            }
            self.sync.visibleMedia(names, generation: expected)
        }
        sync.visibleMedia(model.visibleMediaNames, generation: expected)
        if let walk {
            // The walk is the presentation: its next deck keeps the same review on screen.
            walk.present(model, retry: retry)
            if navigation.review?.session !== walk {
                navigation.startReview(ReviewPresentation(deck: DeckID(rawValue: model.record.deckID), session: walk))
            }
        } else {
            navigation.startReview(ReviewPresentation(deck: DeckID(rawValue: model.record.deckID), session: model))
        }
    }
    public func finishReview() async {
        guard let review else { return }
        // E7: leaving the review closes Edit Note as Cancel does, asking first.
        // The question lives in the review's form: a navigation that already
        // dropped the review (another row, another section) presents it again
        // first, so the question shows and Keep Editing leaves it on screen.
        if editFlowOwner?.isPresented == true, navigation.review == nil {
            // During a walk the presentation's session is the walk (N24.3).
            let session: AnyObject = walk ?? review
            navigation.startReview(ReviewPresentation(deck: DeckID(rawValue: review.record.deckID), session: session))
        }
        guard await editFlowOwner?.requestCancel() != false, self.review === review else { return }
        // Back to Decks, Esc and Done stop the walk: no other deck starts (N24.3).
        walk?.stop()
        let expected = generation
        guard await review.end() else { return }
        // Another owner (a new collection, a successor review) can have taken over meanwhile.
        guard self.review === review, expected == generation else { return }
        // A deck page shows a previous session only for a single-deck session (N24.2), and
        // a single-deck session replaces an earlier walk on Today. A walk's is written
        // when it closes, only if it answered a card.
        if walk == nil { previousRecord = review.record; previousWalk = nil }
        self.review = nil; navigation.endReview()
        await studyPreparation.sessionEnded()
        refreshCounts()
    }

    // MARK: Study All Decks (N24.3)

    /// The decks Today's Study walks, from the deck list's counts (N24.2's first line).
    public var studyAllDecks: [StudyWalk.Deck] { StudyWalk.decks(from: decks) }

    /// Today's Study button and its keys: available while a deck waits and Study is.
    public var studyAllAvailable: Bool {
        !studyAllDecks.isEmpty && canStudy && navigation.sheet == nil && !addNotePresentation.opening
    }

    /// Today's Study: walks the top-level normal decks with cards waiting, each as one
    /// ordinary session, in one review presentation. Its availability is read again when
    /// it runs, not only when the click or key arrived.
    public func startStudyAll() async {
        guard studyAllAvailable else { return }
        await startStudyAll(entry: .study)
    }

    // MARK: Starting a deck from the deck list (H12)

    /// A double-click, or Return or S on a selected deck row, as it was asked for.
    public struct DeckListStudyRequest: Sendable {
        public let deckID: Int64
        fileprivate let opens: UInt64
    }

    /// Taken when the click or key arrives.
    public func deckListStudyRequest(deckID: Int64) -> DeckListStudyRequest {
        DeckListStudyRequest(deckID: deckID, opens: deckPageOpens)
    }

    /// Starts the request's deck once no deck page is opening (a page that opened later would
    /// end the review), unless a later selection took the window elsewhere. The open of the
    /// double-click's own second click is the same deck's and keeps the request. A filtered
    /// deck shows R53's explanation and a deck with nothing to study its page's state, on its
    /// page (H3); any other goes through the deck page's Study path.
    public func startFromDeckList(_ request: DeckListStudyRequest) async {
        await Task.yield()
        await deckPageOpened()
        guard deckListRequestIsCurrent(request), let node = deck(request.deckID) else { return }
        guard !node.filtered, offersStudy(deckID: request.deckID) else {
            if navigation.homeDeckPage?.rawValue != request.deckID { openDeckPage(deckID: request.deckID) }
            return
        }
        guard canStudy, navigation.sheet == nil, !addNotePresentation.opening else { return }
        await startStudy(deckID: request.deckID)
    }

    func deckListRequestIsCurrent(_ request: DeckListStudyRequest) -> Bool {
        let since = deckPageOpens &- request.opens
        guard since <= UInt64(recentDeckPageOpens.count),
              recentDeckPageOpens.suffix(Int(since)).allSatisfy({ $0 == request.deckID }) else { return false }
        return navigation.sidebarSelection == .deck(DeckID(request.deckID))
    }

    private func startStudyAll(entry: StudyPreparation.Entry) async {
        let list = studyAllDecks
        guard canStudy, !list.isEmpty else { return }
        let walk = StudyWalk(decks: list)
        self.walk = walk
        let task = Task { await advance(walk, entry: entry) }
        walkTask = task
        await task.value
    }

    /// The open deck came to its end: move on once whatever runs now has finished.
    private func requestAdvance(_ walk: StudyWalk) {
        guard self.walk === walk, !walk.stopped else { return }
        // Synchronously, with the terminal state: no frame shows that deck's end screen.
        walk.markAdvancing()
        let previous = walkTask
        walkTask = Task { await previous?.value; await advance(walk) }
    }

    /// One step of the walk: end the open deck's session if another deck has a card ready
    /// now and start that one, or show the walk's finished screen on the open deck.
    private func advance(_ walk: StudyWalk, entry: StudyPreparation.Entry = .study) async {
        guard self.walk === walk, !walk.stopped else { return }
        let open = walk.current
        // Keep Going (or an error) can take the open deck away from its end at any await.
        func openStillEnded() -> Bool {
            guard let open else { return true }
            return open.phase == .waiting || open.phase == .finished
        }
        guard openStillEnded() else { walk.settle(); return }
        walk.beginAdvance()
        while let deck = walk.nextDeck() {
            // The open deck's own Keep Going covers it.
            if open?.record.deckID == deck.id { continue }
            let readiness = await readiness(of: deck.id)
            guard self.walk === walk, !walk.stopped else { return }
            guard openStillEnded() else { walk.settle(); return }
            guard readiness.ready else { walk.skip(deck, learningLater: readiness.learningLater); continue }
            if let open {
                // Read before end(), which leaves the phase `.ended`. Counted after it: end()
                // waits for pending work, and a commit that fails meanwhile takes a rejected
                // answer out of the record.
                let leftForEnd = open.phase == .waiting
                guard await open.end() else {
                    // The session did not end: it stays on screen with its error, as after
                    // Back to Decks, which tries again; no other deck starts.
                    walk.stop()
                    return
                }
                walk.close(open, leftForEnd: leftForEnd)
                await studyPreparation.sessionEnded()
                guard self.walk === walk, !walk.stopped else { return }
            }
            let started = await startStudy(deckID: deck.id, entry: entry, walk: walk)
            guard self.walk === walk, !walk.stopped else { return }
            guard started else {
                // A start that stopped with its session open (a pause that could not save)
                // stays on screen, as a failed end does, so Back to Decks closes it.
                if let model = review, model.sessionID != nil, model.phase != .ended {
                    walk.stop()
                    return
                }
                endWalkAtToday(walk)
                return
            }
            // A start that found no card has asked for the next step already.
            if let model = walk.current, model.phase != .waiting, model.phase != .finished { walk.settle() }
            return
        }
        guard walk.current != nil else {
            // No deck had a card ready after all: nothing was presented.
            self.walk = nil
            refreshCounts()
            return
        }
        walk.finish()
    }

    /// Whether a deck has a card to show now (`sched/counts` C7: not finished, not empty).
    /// A deck gone from the list before its turn has none. A read that fails says nothing
    /// about the deck: its start decides, and a start that finds no card is skipped.
    private func readiness(of deckID: Int64) async -> (ready: Bool, learningLater: Bool) {
        guard deck(deckID) != nil else { return (false, false) }
        guard let state = try? await reviewSessionClient.studyState(deckID: deckID), state.deck == deckID else {
            return (true, false)
        }
        return (!state.finished && !state.empty, state.learningLaterToday > 0)
    }

    /// The next deck could not start: the window returns to Today, with what was answered.
    private func endWalkAtToday(_ walk: StudyWalk) {
        if review != nil { review = nil } else { closeWalk(walk) }
        navigation.endReview()
        refreshCounts()
    }

    private func closeWalk(_ walk: StudyWalk) {
        walk.stop()
        if self.walk === walk {
            // Only a walk that answered a card replaces the previous session (N24.2).
            if let summary = walk.previousSession(at: configuration.clock.now()) {
                previousWalk = summary; previousRecord = nil
            }
            self.walk = nil
        }
    }

    /// Another collection: no walk and no record of one applies.
    private func dropWalk() {
        walk?.stop(); walk = nil; previousWalk = nil
        // Today's numbers were the other collection's.
        studiedToday = nil; nextLearningReadyAt = nil
    }

    // MARK: Today's numbers (T12, N24.2)

    /// Reads T12's numbers again, and while nothing waits the next learning card's time;
    /// coalesced, and never on the page's path (the counts show first).
    func refreshTodayNumbers() {
        todayRevision &+= 1
        guard todayTask == nil else { return }
        todayTask = Task {
            var observed: UInt64
            repeat {
                observed = todayRevision
                await loadTodayNumbers()
            } while observed != todayRevision && !closed && !Task.isCancelled
            todayTask = nil
        }
    }

    func loadTodayNumbers() async {
        let expected = generation, snapshot = countsSnapshot
        if let numbers = try? await studiedTodayService.numbers(),
           expected == generation, numbers.generation == expected, !closed {
            studiedToday = numbers
        }
        guard expected == generation, !closed else { return }
        guard studyAllDecks.isEmpty else { nextLearningReadyAt = nil; return }
        // Only while nothing waits: one C7 read per top-level normal deck. A deck that could
        // not be read leaves the time unknown, so none shows until a later refresh reads it.
        var earliest: Int64?
        var unread = false
        for node in decks where !node.filtered {
            guard let state = try? await loadDeckStudyState(node.id) else { unread = true; break }
            if let ready = state.nextLearningReadyAt { earliest = min(earliest ?? ready, ready) }
        }
        // A newer count read asked for its own scan meanwhile: this one is dropped.
        guard expected == generation, snapshot == countsSnapshot, !closed, studyAllDecks.isEmpty else { return }
        nextLearningReadyAt = unread ? nil : earliest.map { Date(timeIntervalSince1970: Double($0)) }
    }
    public func navigationChanged() {
        if navigation.review == nil, review != nil { Task { await finishReview() } }
    }
    public func setSceneActive(_ active: Bool) {
        if !active { syncPresentation.cancelStatusHandoff() }
        sceneActive = active
        updateReviewActivity()
    }
    public func setApplicationActive(_ active: Bool) { applicationActive = active; updateReviewActivity() }
    public func setReviewVisible(_ visible: Bool) {
        visibleReviewHosts = max(0, visibleReviewHosts + (visible ? 1 : -1))
        reviewVisible = visibleReviewHosts > 0
        updateReviewActivity()
    }
    public func setReviewWindowVisible(_ visible: Bool) { windowVisible = visible; updateReviewActivity() }
    public func setReviewWindowActive(_ active: Bool) { reviewWindowActive = active; updateReviewActivity() }
    public func setReviewObscured(_ value: Bool) {
        reviewPresentationAllowsInput = !value
        updateReviewActivity()
    }
    private func updateReviewActivity() {
        if !reviewIsActive { review?.beginPause() }
        let previous = lifecycleTask
        lifecycleTask = Task {
            await previous?.value
            guard foreground, !resuming, !closed, let review else { return }
            do {
                if reviewIsActive, resolver != nil { try await review.resume() }
                else { try await review.pause() }
            } catch { self.error = String(describing: error) }
        }
    }
    public func setForeground(_ active: Bool) {
        guard active != foreground else { return }
        foreground = active
        let cancelledEditor = active ? nil : cancelPendingEditorReplacement()
        if !active { syncPresentation.cancelStatusHandoff(); meguriSignInPreparationTask?.cancel() }
        resuming = active
        if !active { review?.beginPause(); day?.pause(); countTimer?.invalidate() }
        let previous = lifecycleTask
        lifecycleTask = Task {
            await cancelledEditor?.waitForFinish()
            await previous?.value
            if !active {
                await meguriSignInPreparationTask?.value
                await deckNamesOwner?.cancelAndDrain(); await deckOptionsOwner?.cancelAndDrain(); await deckDeletionOwner?.cancelAndDrain(); await manualOwner?.cancelAndDrain(); await entryOwner?.cancelAndDrain(); await exportOwner?.cancelAndDrain()
            }
            await openTask?.value
            guard !closed, state != .unopened else { resuming = false; return }
            if state == .creationRecovery {
                // There may be no writer to resume/suspend. Preserve the bounded
                // create error and retained resources until the drained close.
                if !active {
                    if case .ready? = try? await collection.lifecycle() { try? await collection.flushForBackground() }
                    _ = await sync.cancelAndDrain()
                }
                resuming = false
                return
            }
            if state == .noFile {
                // A retained NoCollection Host has cloud callbacks to drain,
                // even though there is no local writer to flush or suspend.
                if sync.connectionService != nil {
                    if active { await sync.foregroundAfterLocalResume() }
                    else { _ = await sync.backgroundAfterLocalFlush(deadlineMilliseconds: backgroundLeaseExpired ? nil : backgroundDeadlineMilliseconds) }
                }
                resuming = false
                return
            }
            do {
                if active {
                    let began = day?.beginResult()
                    let report = try await collection.resume()
                    lifecycle = try await collection.lifecycle()
                    if let began { day?.result(nextDayAt: report.nextDayAt, beganAt: began) }
                    if report.activeSession != review?.sessionID {
                        editFlowOwner?.generationEnded()
                        review?.generationEnded(); review = nil; navigation.endReview()
                    }
                    await refreshMedia()
                    if reviewIsActive, resolver != nil { try await review?.resume() }
                    await sync.foregroundAfterLocalResume()
                    await addFlowOwner?.resumeInput()
                    resuming = false
                    deferredSuspend = nil
                    refreshCounts()
                    deckNamesOwner?.resumeResult()
                } else {
                    do { try await review?.pause() }
                    catch { self.error = String(describing: error) }
                    guard await addFlowOwner?.flushForBackground() != false else {
                        throw BridgeError.unavailable(reason: "The note editor could not flush. Your draft has been kept.")
                    }
                    try await collection.flushForBackground()
                    didFlushLocally?()
                    guard await sync.backgroundAfterLocalFlush(deadlineMilliseconds: backgroundLeaseExpired ? nil : backgroundDeadlineMilliseconds) else {
                        throw BridgeError.cloudPolicy(.coreCapabilityUnavailable)
                    }
                    switch try await collection.suspend(timeoutMilliseconds: 1_500) {
                    case .suspended: deferredSuspend = nil
                    case .deferred(let id): deferredSuspend = id
                    }
                }
            } catch { self.error = String(describing: error) }
        }
    }
    public func waitForLifecycle() async { await lifecycleTask?.value }
    func waitForTodayNumbers() async { await todayTask?.value }
    /// Waits for the walk's queued steps, the ones they queue included.
    func waitForWalk() async {
        while let task = walkTask {
            await task.value
            if walkTask == task { return }
        }
    }
    func waitForCloudActivation() async { await cloudActivationCompletion?.value }
    /// The UIKit assertion owner supplies the OS's remaining time, never a
    /// guessed duration. Tokens keep a late expiry out of a later foreground.
    public func beginBackgroundTask(deadlineMilliseconds: Int64?) -> UInt64? {
        guard foreground else { return nil }
        backgroundLease &+= 1
        backgroundDeadlineMilliseconds = deadlineMilliseconds
        backgroundLeaseExpired = false
        let token = backgroundLease
        setForeground(false)
        return token
    }
    public func backgroundTaskExpired(_ token: UInt64) {
        guard token == backgroundLease, !foreground else { return }
        backgroundLeaseExpired = true
        sync.backgroundExpired()
    }
    public func systemTimeChanged() {
        NSTimeZone.resetSystemTimeZone()
        guard foreground, state == .ready else { return }
        Task { await day?.checkNow() }
    }
    @discardableResult public func close() async -> Bool { await close(quitting: false) }
    @discardableResult public func closeForQuit() async -> Bool { await close(quitting: true) }
    private func close(quitting: Bool) async -> Bool {
        syncPresentation.cancelStatusHandoff()
        if let closeTask { return await closeTask.value }
        if closed { return true }
        meguriSignInPreparationTask?.cancel()
        let task = Task { await performClose(quitting: quitting) }
        closeTask = task
        let result = await task.value
        closeTask = nil
        return result
    }
    private func performClose(quitting: Bool) async -> Bool {
        closing = true
        defer { closing = false }
        if let cancelledEditor = cancelPendingEditorReplacement() {
            // Its guard joins accepted Add work and releases the same host;
            // its candidate cleanup owns the reservation until completion.
            await cancelledEditor.waitForFinish()
            if addFlowOwner?.isPresented == true { return false }
        }
        // The editor guard precedes cancellation, Core flush and Core close.
        // Keep Editing cancels quit without retiring the collection owner.
        guard await editFlowOwner?.requestCancel() != false else { return false }
        guard await addFlowOwner?.requestClose() != false else { return false }
        meguriSignInPreparationTask?.cancel()
        await meguriSignInPreparationTask?.value
        invalidateDeckActions()
        await deckNamesOwner?.cancelAndDrain()
        await deckOptionsOwner?.cancelAndDrain()
        await deckDeletionOwner?.cancelAndDrain()
        _ = await selectionTask?.value
        await collapseTask?.value
        await manualOwner?.cancelAndDrain()
        await entryOwner?.cancelAndDrain()
        await exportOwner?.cancelAndDrain()
        await openTask?.value
        _ = await cloudHostPreparationTask?.result
        for task in Array(cloudPreparationTasks.values) { _ = await task.result }
        await cloudActivationCompletion?.value
        await lifecycleTask?.value
        // Preserve the owner's no-file/unopened cleanup admission, including a
        // failed open whose absent host still has a historical failed lifecycle.
        // Other presentation states need a fresh no-collection read after joins.
        let noCollectionCleanup: Bool
        // Core close also supports a failed creation owner, drains its jobs and
        // flushes a writer if one survived. Preflush requires an open collection
        // and would otherwise prevent the recovery screen's Quit instruction.
        if state == .noFile || state == .unopened || state == .creationRecovery {
            noCollectionCleanup = true
        } else {
            do { noCollectionCleanup = try await collection.lifecycle() == .noCollection }
            catch { self.error = String(describing: error); return false }
        }
        if noCollectionCleanup {
            guard await sync.cancelAndDrain() else { return false }
            do { try await collection.close() }
            catch { self.error = "Couldn’t close the collection. Try again."; return false }
            closed = true; ChangeFanout.shared.unsubscribe(self)
            review?.generationEnded(); review = nil; navigation.endReview()
            invalidateMedia(); await mediaCloseTask?.value
            return true
        }
        if review != nil { await finishReview(); if review != nil { return false } }
        do {
            try await collection.flushForBackground()
            if quitting {
                var quitAllowed = false
                await sync.quitAfterLocalFlush { quitAllowed = true }
                guard quitAllowed else { return false }
            }
            guard await sync.cancelAndDrain() else { return false }
            // A close is the one reason to stop the step; the next open starts it again (F5.1).
            studyPreparation.collectionEnded(); await studyPreparation.drain()
            studyRefusal = nil
            do { try await collection.close() }
            catch {
                // Still open: study must stay reachable.
                await studyPreparation.collectionOpened()
                throw error
            }
            ChangeFanout.shared.unsubscribe(self)
            try await configuration.nativeReviewInput?.closed()
            closed = true; day?.close(); countTimer?.invalidate(); countsTask?.cancel()
            invalidateMedia(); await mediaCloseTask?.value
            state = .unopened; lifecycle = .noCollection
            return true
        } catch { self.error = String(describing: error); return false }
    }
    private func invalidateMedia() {
        mediaRevision &+= 1
        mediaArrivals = ReviewMediaArrivals(); mediaArrivalReview = nil
        let old = resolver; resolver = nil
        let previous = mediaCloseTask
        mediaCloseTask = Task { await previous?.value; await old?.close() }
    }
    // The ordered event consumer and focused media-binding tests share admission.
    func scheduleMediaRefresh(names: [String], generation expected: UInt64) {
        guard expected == generation, !closed, !closing else { return }
        if mediaArrivalReview !== review {
            mediaArrivals = ReviewMediaArrivals(); mediaArrivalReview = review
        }
        if review != nil, !mediaArrivals.insert(names) {
            mediaArrivals = ReviewMediaArrivals(); mediaArrivalReview = nil
            review?.mediaUnavailable()
        }
        mediaRevision &+= 1
        guard foreground, state == .ready, !opening else { return }
        // One refresh task owns all subsequent revisions; no task per event.
        if mediaTask == nil { beginMediaRefresh() }
    }
    func mediaResolverForReview() async -> MediaPathResolver? {
        await refreshMedia()
        return resolver
    }
    private func refreshMedia() async {
        if mediaTask == nil { beginMediaRefresh() }
        await mediaTask?.value
    }
    private func beginMediaRefresh() {
        let task = Task {
            defer { mediaTask = nil }
            var observed: UInt64
            repeat {
                observed = mediaRevision
                let expected = generation
                do {
                    await mediaCloseTask?.value
                    let snapshot = try await loadMediaIndex()
                    let root = configuration.collectionLocation.directory.appending(path: "collection.media", directoryHint: .isDirectory)
                    guard expected == generation, observed == mediaRevision, !closed, !closing, foreground else { continue }
                    guard Self.acceptsMedia(snapshot, generation: expected, root: root) else {
                        throw BridgeError.unavailable(reason: "Media journal binding differs from the open collection")
                    }
                    let names = snapshot.names.map { MediaPathResolver.TrackedName(name: $0.name, caseKey: $0.caseKey) }
                    if let resolver {
                        await resolver.update(trackedNames: names)
                        guard expected == generation, observed == mediaRevision, !closed, !closing, foreground,
                              self.resolver === resolver else { continue }
                    } else {
                        let bound = try await MediaPathResolver.open(root: root, trackedNames: names)
                        guard expected == generation, observed == mediaRevision, !closed, !closing, foreground else { await bound.close(); continue }
                        resolver = bound
                    }
                    let arrived = mediaArrivals.take()
                    if let review, mediaArrivalReview === review { review.mediaArrived(arrived) }
                    mediaArrivalReview = nil
                } catch {
                    if expected == generation, observed == mediaRevision, !closed, !closing {
                        invalidateMedia()
                        self.error = "Couldn’t read the collection media journal. \(error)"
                        review?.mediaUnavailable()
                        break
                    }
                }
            } while observed != mediaRevision && foreground && !closed && !closing
        }
        mediaTask = task
    }
    static func acceptsMedia(_ snapshot: MediaNameSnapshot, generation: UInt64, root: URL) -> Bool {
        snapshot.generation == generation && snapshot.root.isFileURL &&
            Data(snapshot.root.standardizedFileURL.path.utf8) == Data(root.standardizedFileURL.path.utf8)
    }
    @concurrent private nonisolated static func prepareOpenLocation(_ location: CollectionLocation, providerDirectory: URL) async throws -> CoreBridge.CollectionLocation {
        // Only the app-private destination root is prepared here. Rust preflights
        // the combined import budget before source copy/inspection/backup writes;
        // the real backup writer owns creation of its supplied backup parents.
        try FileManager.default.createDirectory(at: location.directory, withIntermediateDirectories: true)
        return CoreBridge.CollectionLocation(collectionDirectory: location.directory, backupsDirectory: location.backupsDirectory,
            appGroupSnapshot: location.directory.appending(path: "meguri-widget-unavailable.json"), manualTransferDirectory: providerDirectory)
    }
    @concurrent private nonisolated static func exists(_ url: URL) async -> Bool { FileManager.default.fileExists(atPath: url.path) }
}

#if DEBUG
/// Categories only: never format an error payload or return an input step name.
enum OpenFailureDiagnostic {
    enum Phase: String, CaseIterable {
        case platformPreparation, eventSubscription, pathPreparation, seedPreparation
        case coreStartOpen, coreResult, postOpenHostSetup
    }

    static func value(phase: Phase, error: any Error, lifecycle: CollectionLifecycle?) -> String {
        let category: String
        var step = "none"
        switch lifecycle {
        case nil: category = "queryFailed"
        case .noCollection: category = "noCollection"
        case .opening: category = "opening"
        case .upgrading: category = "upgrading"
        case .ready: category = "ready"
        case .exclusiveBusy: category = "exclusiveBusy"
        case .failed(let reason):
            switch reason {
            case .open(let failure): category = "failed.open.\(openCategory(failure))"
            case .openStep(let name, _): category = "failed.openStep"; step = stepCategory(name)
            case .replace: category = "failed.replace"
            case .corrupt: category = "failed.corrupt"
            }
        }
        return "phase:\(phase.rawValue);bridge:\(bridgeCategory(error));lifecycle:\(category);step:\(step);reason:\(reasonCategory(error))"
    }

    private static func reasonCategory(_ error: any Error) -> String {
        guard let error = error as? BridgeError else { return "other" }
        guard case .invalidArgument(let message) = error else { return "none" }
        // frozen::error adds this exact prefix to journal.rs's fixed markers.
        // Match the whole message; attached details are never trusted as markers.
        switch message {
        case "review journal: pending_answer_identity_mismatch": return "pending_answer_identity_mismatch"
        case "review journal: pending_answer_identity_mismatch_path": return "pending_answer_identity_mismatch_path"
        case "review journal: pending_answer_identity_mismatch_native": return "pending_answer_identity_mismatch_native"
        case "review journal: pending_answer_identity_mismatch_path_native": return "pending_answer_identity_mismatch_path_native"
        case "review journal: pending_answer_journal_corruption": return "pending_answer_journal_corruption"
        case "review journal: pending_answer_identity_migration_io": return "pending_answer_identity_migration_io"
        default: return message.hasPrefix("review journal: ") ? "review_journal_other" : "review_service_other"
        }
    }

    static func bridgeCategory(_ error: any Error) -> String {
        guard let error = error as? BridgeError else { return "other" }
        switch error {
        case .cloudPolicy: return "cloudPolicy"
        case .cloudConnection: return "cloudConnection"
        case .cloudSDKCallbackRejected: return "cloudSDKCallbackRejected"
        case .maintenance: return "maintenance"
        case .exportSourceChanged: return "exportSourceChanged"
        case .exportSourceInvalid: return "exportSourceInvalid"
        case .exportFormatLimit: return "exportFormatLimit"
        case .deckOptionsInvalid: return "deckOptionsInvalid"
        case .stale: return "stale"
        case .staleGeneration: return "staleGeneration"
        case .undoUnavailable: return "undoUnavailable"
        case .notetypeRefused: return "notetypeRefused"
        case .deckRefused: return "deckRefused"
        case .notetypeStructureRefused: return "notetypeStructureRefused"
        case .noteNotFound: return "noteNotFound"
        case .notFound: return "notFound"
        case .insufficientSpace: return "insufficientSpace"
        case .open: return "open"
        case .suspendDeadline: return "suspendDeadline"
        case .schedOpRefused: return "schedOpRefused"
        case .sessionActive: return "sessionActive"
        case .invalidTypeAnswerReplacement: return "invalidTypeAnswerReplacement"
        case .cancelled: return "cancelled"
        case .busy: return "busy"
        case .unavailable: return "unavailable"
        case .unsupported: return "unsupported"
        case .invalidArgument: return "invalidArgument"
        case .internal: return "internal"
        case .mediaCommitPending: return "mediaCommitPending"
        case .commitCompleted: return "commitCompleted"
        }
    }

    private static func openCategory(_ failure: CollectionOpenFailure) -> String {
        switch failure {
        case .openInAnotherProgram: return "openInAnotherProgram"
        case .tooOld: return "tooOld"
        case .tooNew: return "tooNew"
        case .v1Scheduler: return "v1Scheduler"
        case .notACollection: return "notACollection"
        case .upgradeFailed: return "upgradeFailed"
        case .corrupt: return "corrupt"
        case .connectionInit: return "connectionInit"
        case .notFound: return "notFound"
        case .io: return "io"
        }
    }

    private static func stepCategory(_ name: String) -> String {
        // Engine lifecycle's fixed steps and the ports registered by Host::open.
        switch name {
        case "open": return "open"
        case "ownership": return "ownership"
        case "open_evidence": return "open_evidence"
        case "writer": return "writer"
        case "fsrs_policy": return "fsrs_policy"
        case "day_clock": return "day_clock"
        case "derived-store": return "derived-store"
        case "media": return "media"
        case "review-session": return "review-session"
        case "cloud-note-capture": return "cloud-note-capture"
        default: return "other"
        }
    }
}
#endif

private nonisolated struct LaunchCollectionClock: CollectionClock {
    let clock: AppClock
    let zone: TimeZone?
    func nowMilliseconds() -> Int64 { Int64(clock.now().timeIntervalSince1970 * 1_000) }
    func utcOffsetSeconds(atMilliseconds value: Int64) -> Int32 {
        Int32((zone ?? .autoupdatingCurrent).secondsFromGMT(for: Date(timeIntervalSince1970: Double(value) / 1_000)))
    }
    func zoneIdentifier() -> String { (zone ?? .autoupdatingCurrent).identifier }
}
