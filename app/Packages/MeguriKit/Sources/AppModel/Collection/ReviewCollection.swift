import CardRendering
import CoreBridge
import Foundation
import Observation

/// One production collection and event consumer, held above shell layouts.
@Observable
@MainActor
public final class ReviewCollection {
    public enum State: Equatable { case unopened, loading, noFile, ready, unavailable, failed(String) }
    public private(set) var state: State = .unopened
    public private(set) var lifecycle: CollectionLifecycle = .noCollection
    public private(set) var generation: UInt64 = 0
    public private(set) var decks: [CountedDeckNode] = []
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
    public private(set) var review: ReviewCoordinator?
    public private(set) var previousRecord: ReviewRecord?
    public private(set) var policy: FsrsPolicyStatus?
    public private(set) var enabling = false
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
    }
    // Notify future U151 integrations after the local flush finishes.
    public var didFlushLocally: (() -> Void)? = nil
    public let configuration: LaunchConfiguration
    public let navigation: AppNavigation
    private let collection: CoreCollectionService
    private let entryService: any CollectionEntryService
    private var entryOwner: CollectionEntryCoordinator?
    public var entry: CollectionEntryCoordinator {
        if let entryOwner { return entryOwner }
        let value = CollectionEntryCoordinator(owner: self, service: entryService)
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
    private var deckNamesReserved: Bool {
        if case .deckName = navigation.sheet { return true }
        return deckNamesOwner?.busy == true
    }
    func deckNamesAllowed(host: AppNavigation.SheetHost, route: SheetRoute?) -> Bool {
        guard host == .main, foreground, !resuming, !closed, !closing,
              !opening, !starting, !enabling, state == .ready,
              case .ready(let current) = lifecycle, current == generation,
              review == nil, navigation.review == nil,
              entryOwner?.busy != true, entryOwner?.pickerPresented != true, entryOwner?.preview == nil,
              exportOwner?.busy != true,
              exportOwner?.artifact == nil, exportOwner?.providerLease == nil,
              manualOwner?.busy != true, manualOwner?.preview == nil, manualOwner?.mediaPreview == nil,
              navigation.sheet == route else { return false }
        return route == nil || navigation.sheetHost == .main
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
        let value = CollectionExportCoordinator(owner: self, service: exportService)
        exportOwner = value
        return value
    }
    private var manualOwner: ManualTransferCoordinator?
    private var releasedManualHostState: State?
    public var manualTransfers: ManualTransferCoordinator {
        if let manualOwner { return manualOwner }
        let location = CoreBridge.CollectionLocation(collectionDirectory: configuration.collectionLocation.directory,
            backupsDirectory: configuration.collectionLocation.backupsDirectory,
            appGroupSnapshot: configuration.collectionLocation.directory.appending(path: "meguri-widget-unavailable.json"))
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
        try? await collection.close()
        lifecycle = .noCollection; state = restored
    }
    var exportAllowed: Bool {
        guard !opening, !enabling, !deckNamesReserved, state == .ready, case .ready = lifecycle else { return false }
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
    private var events: Task<Void, Never>?
    private var countsTask: Task<Void, Never>?
    private var countTimer: Timer?
    private var countsRevision: UInt64 = 0
    private var deckTreeRevision: UInt64 = 0
    private var lifecycleTask: Task<Void, Never>?
    private var openTask: Task<Void, Never>?
    private var opening = false
    private var preparedGeneration: UInt64?
    private var starting = false
    private var policyReadAfterSession = false
    private var pendingDeck: Int64?
    private var closed = false
    private var closing = false
    private var day: ReviewDayMonitor?
    private var recoveryDrops: [Int64] = []

    public init(configuration: LaunchConfiguration, navigation: AppNavigation, entryService: (any CollectionEntryService)? = nil, exportService: (any CollectionExportService)? = nil) {
        loadMediaIndex = { @MainActor in try await CoreMediaService().trackedNames() }
        loadDeckTree = { @MainActor in try await CoreDeckService().treeWithCounts() }
        saveSelectedDeck = { @MainActor id in try await CoreDeckService().selectDeck(id: id).generation }
        saveStudyCollapse = { @MainActor ids, collapsed in
            try await CoreDeckService().collapseDecks(ids: ids, scope: .study, collapsed: collapsed).generation
        }
        self.exportService = exportService ?? CoreCollectionExportService()
        self.configuration = configuration; self.navigation = navigation
        self.entryService = entryService ?? CoreCollectionEntryService(clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone))
        collection = CoreCollectionService(clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone))
        records = ReviewRecordStore(file: configuration.collectionLocation.directory.appending(path: "meguri-review-session.json"))
        navigation.deckSelectionAction = { [weak self] deck in
            self?.openDeckPage(deckID: deck.rawValue)
        }
    }

    public func start() async {
        guard state == .unopened || state == .noFile || isFailed, !opening else { return }
        opening = true
        let task = Task { await openCollection() }
        openTask = task
        await task.value
    }
    private func openCollection() async {
        opening = true; state = .loading; error = nil; closed = false; releasedManualHostState = nil
        defer { opening = false }
        do {
            try await subscribe()
            let location = try await Self.prepareOpenLocation(configuration.collectionLocation)
            guard await Self.exists(configuration.collectionLocation.collectionFile) else { state = .noFile; return }
            try await collection.startOpen(location, randomSeed: UInt64.random(in: 0...UInt64.max)).value()
            try await completeOpenedHost(loadRecord: true)
        } catch {
            let message = Self.openExplanation(error)
            state = .failed(message); self.error = message
        }
    }
    private func subscribe() async throws {
        try await CoreEventStream.shared.start()
        if events == nil {
            events = Task { [weak self] in
                for await event in CoreEventStream.shared.events {
                    guard let self, !Task.isCancelled else { return }
                    self.receive(event)
                }
            }
        }
    }
    /// Called only after the FFI job retains the host, for open AND first import.
    private func completeOpenedHost(loadRecord: Bool) async throws {
        lifecycle = try await collection.lifecycle()
        guard case .ready(let value) = lifecycle else { state = .unavailable; return }
        if generation != value { invalidateDeckActions(); exportOwner?.generationChanged() }
        generation = value; state = .ready; error = nil; closed = false; releasedManualHostState = nil
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
        await readPolicy()
        await loadCounts()
        preparedGeneration = generation
    }
    func prepareEntry() async throws -> CoreBridge.CollectionLocation {
        try await subscribe()
        return try await Self.prepareOpenLocation(configuration.collectionLocation)
    }
    var entryAllowed: Bool { (state == .ready || manualOwner?.preview == nil) && manualAllowed && state != .unavailable }
    var manualAllowed: Bool { !deckNamesReserved && entryOwner?.busy != true && exportOwner?.busy != true && !opening && !starting && !enabling && review == nil && (state == .ready || state == .noFile || state == .unavailable || isFailed) }
    func beginManualActivation() throws {
        guard review == nil, !starting else { throw BridgeError.sessionActive }
        guard manualAllowed else { throw BridgeError.busy(runningOperation: "collection preparation") }
        opening = true
        day?.pause(); countTimer?.invalidate(); countsTask?.cancel()
    }
    func beginEntryActivation() throws {
        guard review == nil, !starting else { throw BridgeError.sessionActive }
        guard entryAllowed else { throw BridgeError.busy(runningOperation: "collection preparation") }
        opening = true
        day?.pause(); countTimer?.invalidate(); countsTask?.cancel()
    }
    @discardableResult func finishEntryActivation(candidateActive: Bool) async -> Bool {
        defer { opening = false }
        do {
            if candidateActive {
                navigation.collectionReplaced()
                previousRecord = nil; pendingDeck = nil; recoveryDrops.removeAll()
                try await records.remove()
                try await completeOpenedHost(loadRecord: false)
            } else {
                lifecycle = try await collection.lifecycle()
                switch lifecycle {
                case .ready(let next): generation = next; state = .ready
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
            return "This collection uses an older format that Meguri can’t open. Update it in a compatible app, then import a latest-format collection-only .colpkg in Settings."
        case .tooNew:
            return "This collection uses a newer format than Meguri supports. Check for a Meguri update."
        case .v1Scheduler:
            return "This collection uses an unsupported older scheduler. Update its scheduler in a compatible app before opening it in Meguri."
        case .notACollection:
            return "This file isn’t a supported collection. Import a latest-format collection-only .colpkg in Settings."
        case .corrupt:
            return "This collection appears to be damaged or incomplete. Import a known-good collection-only .colpkg in Settings."
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

    private func receive(_ event: CoreEvent) {
        guard !closed else { return }
        // Never drop review commit failures behind a coalesced deck refresh.
        review?.receive(event)
        switch event.kind {
        case .lifecycle(let value):
            receiveLifecycle(value)
        case .nextDayAtChanged(let instant): if !opening { day?.event(nextDayAt: instant) }
        case .rolledOver, .timeZoneChanged: refreshCounts()
        case .changes(let changes):
            refreshCounts()
            if changes.wholeCollection || !changes.mediaAdded.isEmpty || !changes.mediaRemoved.isEmpty {
                scheduleMediaRefresh(names: changes.mediaAdded, generation: event.generation)
            }
        case .mediaArrived(let names): scheduleMediaRefresh(names: names, generation: event.generation)
        case .collectionReplaced(let next):
            guard preparedGeneration != next else { return }
            exportOwner?.generationChanged()
            invalidateDeckActions()
            generation = next; day?.pause()
            invalidateMedia()
            review?.generationEnded(); review = nil; navigation.endReview()
            previousRecord = nil; pendingDeck = nil; policyReadAfterSession = false
            policy = nil; decks = []; countTimer?.invalidate(); countsTask?.cancel()
            navigation.collectionReplaced(keepingEntrySheet: opening)
            if !opening {
                navigation.dismissSheet()
                entryOwner?.destinationChanged()
                Task { [weak self] in
                    guard let self else { return }
                    await self.mediaCloseTask?.value
                    guard self.generation == next, !self.closed, !self.opening else { return }
                    await self.day?.checkNow(); await self.readPolicy(); await self.loadCounts()
                }
            }
        case .recoveryDropped(_, let cardID, _): recoveryDrops.append(cardID)
        case .sessionEnded:
            if policyReadAfterSession, !opening { Task { await readPolicy() } }
        default: break
        }
    }

    // Also used by focused owner tests; the event consumer is the production caller.
    func receiveLifecycle(_ value: CollectionLifecycle) {
        let wasReady: Bool = { if case .ready = lifecycle { true } else { false } }()
        lifecycle = value
        switch value {
        case .ready(let next):
            if generation != next {
                invalidateDeckActions(); invalidateMedia(); exportOwner?.generationChanged()
            }
            generation = next
            if !opening {
                state = .ready
                if !wasReady && foreground { Task { await day?.checkNow() } }
            }
        case .opening, .upgrading: invalidateDeckActions(); state = .loading
        case .exclusiveBusy: invalidateDeckActions(); state = .unavailable; review?.requestRefresh()
        case .failed(let reason): invalidateDeckActions(); state = .failed(Self.explanation(reason))
        case .noCollection: invalidateDeckActions(); state = releasedManualHostState ?? .unopened
        }
    }

    private var canSaveDeckState: Bool { state == .ready && !opening && !closed && !closing && !resuming }

    private func invalidateDeckActions() {
        deckNamesOwner?.invalidate()
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

    private func openDeckPage(deckID: Int64, detail: DeckPageDetail?) -> Task<Bool, Never> {
        let intent = navigation.beginDeckSelection()
        let selection = selectDeck(deckID: deckID, intent: intent)
        let expected = generation, revision = selectionRevision
        return Task {
            guard await selection.value, expected == generation, revision == selectionRevision,
                  canSaveDeckState, navigation.intentRevision == intent else { return false }
            if navigation.sidebarSelection != .deck(DeckID(deckID)) {
                navigation.openDeckPage(DeckID(deckID))
            }
            selectedDeckDetail = deck(deckID) == nil
                ? detail.map { (page: $0, generation: expected, intent: navigation.intentRevision) } : nil
            return true
        }
    }

    /// Serial writes preserve core selection order; superseded queued requests need no write.
    private func selectDeck(deckID: Int64, intent: UInt64) -> Task<Bool, Never> {
        selectionRevision &+= 1
        let revision = selectionRevision, expected = generation, previous = selectionTask
        let task = Task {
            _ = await previous?.value
            guard revision == selectionRevision, expected == generation, canSaveDeckState,
                  navigation.intentRevision == intent else { return false }
            do {
                let savedGeneration = try await saveSelectedDeck(deckID)
                guard revision == selectionRevision, expected == generation,
                      savedGeneration == expected, canSaveDeckState,
                      navigation.intentRevision == intent else { return false }
                if error == "Couldn’t select this deck. Try again." { error = nil }
                return true
            } catch {
                if revision == selectionRevision, expected == generation, canSaveDeckState,
                   navigation.intentRevision == intent {
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

    public func deckPage(_ id: Int64) -> DeckPageDetail? {
        if let node = deck(id) {
            return DeckPageDetail(id: node.id, name: node.name, counts: node.counts)
        }
        let detail = currentSelectedDeckDetail
        return detail?.id == id ? detail : nil
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
        if let node = deck(mutation.id) {
            return DeckPageDetail(id: node.id, name: node.name, counts: node.counts)
        }
        // The mutation supplies the stored identity/name; studyState reads that
        // exact ID even when the empty Default deck is hidden from tree queries.
        let expected = generation
        let state = try await CoreDeckService().studyState(deckID: mutation.id)
        guard expected == generation, mutation.summary.generation == expected,
              canSaveDeckState, state.deck == mutation.id else { throw BridgeError.staleGeneration(planned: expected, current: generation) }
        return DeckPageDetail(id: mutation.id, name: mutation.name, counts: state.listCounts)
    }

    public func readPolicy() async {
        guard !enabling else { return }
        let expected = generation
        do {
            let status = try await Core.fsrsPolicyStatus()
            guard expected == generation, !closed else { return }
            policy = status
            if review?.sessionID != nil {
                policyReadAfterSession = status != .ok
            } else if status != .ok { navigation.present(.fsrsAdmission) }
        } catch { if expected == generation, !closed { self.error = String(describing: error) } }
    }
    public func enableFsrs() async {
        guard !enabling, review?.sessionID == nil else { return }
        enabling = true; error = nil
        defer { enabling = false; progress = nil }
        do {
            let job = try await Core.enableFsrs()
            let progressTask = Task { for await value in job.progress { progress = value } }
            defer { progressTask.cancel() }
            try await job.value()
            policy = .ok; navigation.dismissSheet()
            refreshCounts()
            if let deck = pendingDeck { pendingDeck = nil; Task { await startStudy(deckID: deck) } }
        } catch { self.error = String(describing: error) }
    }
    public var canStudy: Bool { !deckNamesReserved && !opening && entryOwner?.busy != true && navigation.sheet != .collectionEntry && state == .ready && reviewIsActive && !resuming && !starting && review == nil && policy == .ok }
    public func startStudy(deckID: Int64) async {
        guard canStudy else { return }
        guard policy == .ok else { pendingDeck = deckID; await readPolicy(); return }
        starting = true
        defer { starting = false }
        do {
            let expected = generation, intent = navigation.intentRevision
            guard await selectDeck(deckID: deckID, intent: intent).value,
                  expected == generation, intent == navigation.intentRevision else { return }
            guard let resolver = await mediaResolverForReview(), reviewIsActive, state == .ready,
                  expected == generation, intent == navigation.intentRevision else { return }
            previousRecord = nil // The visible Home record has now been acknowledged.
            let model = ReviewCoordinator(deckID: deckID, surface: makeReviewSurface(resolver),
                store: records, clock: configuration.clock)
            model.onPolicyRefusal = { [weak self] in self?.pendingDeck = deckID }
            presentReview(model)
            await model.start()
            if !reviewIsActive { model.beginPause(); try await model.pause() }
            if model.sessionID == nil {
                navigation.endReview(); review = nil
                if pendingDeck != nil { await readPolicy() }
                else { error = model.notice }
            }
        } catch { self.error = String(describing: error) }
    }
    func presentReview(_ model: ReviewCoordinator) {
        reviewVisible = true
        review = model
        navigation.startReview(ReviewPresentation(deck: DeckID(rawValue: model.record.deckID), session: model))
    }
    public func finishReview() async {
        guard let review else { return }
        guard await review.end() else { return }
        previousRecord = review.record
        self.review = nil; navigation.endReview()
        if policyReadAfterSession { await readPolicy(); policyReadAfterSession = false }
        refreshCounts()
    }
    public func navigationChanged() {
        if navigation.review == nil, review != nil { Task { await finishReview() } }
    }
    public func setSceneActive(_ active: Bool) { sceneActive = active; updateReviewActivity() }
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
        resuming = active
        if !active { review?.beginPause(); day?.pause(); countTimer?.invalidate() }
        let previous = lifecycleTask
        lifecycleTask = Task {
            await previous?.value
            if !active { await deckNamesOwner?.cancelAndDrain(); await manualOwner?.cancelAndDrain(); await entryOwner?.cancelAndDrain(); await exportOwner?.cancelAndDrain() }
            await openTask?.value
            guard !closed, state != .unopened, state != .noFile else { resuming = false; return }
            do {
                if active {
                    let began = day?.beginResult()
                    let report = try await collection.resume()
                    lifecycle = try await collection.lifecycle()
                    if let began { day?.result(nextDayAt: report.nextDayAt, beganAt: began) }
                    if report.activeSession != review?.sessionID { review?.generationEnded(); review = nil; navigation.endReview() }
                    await refreshMedia()
                    if reviewIsActive, resolver != nil { try await review?.resume() }
                    resuming = false
                    deferredSuspend = nil
                    refreshCounts()
                    deckNamesOwner?.resumeResult()
                } else {
                    do { try await review?.pause() }
                    catch { self.error = String(describing: error) }
                    try await collection.flushForBackground()
                    didFlushLocally?()
                    switch try await collection.suspend(timeoutMilliseconds: 1_500) {
                    case .suspended: deferredSuspend = nil
                    case .deferred(let id): deferredSuspend = id
                    }
                }
            } catch { self.error = String(describing: error) }
        }
    }
    public func waitForLifecycle() async { await lifecycleTask?.value }
    public func systemTimeChanged() {
        NSTimeZone.resetSystemTimeZone()
        guard foreground, state == .ready else { return }
        Task { await day?.checkNow() }
    }
    @discardableResult public func close() async -> Bool {
        closing = true
        defer { closing = false }
        invalidateDeckActions()
        await deckNamesOwner?.cancelAndDrain()
        _ = await selectionTask?.value
        await collapseTask?.value
        await manualOwner?.cancelAndDrain()
        await entryOwner?.cancelAndDrain()
        await exportOwner?.cancelAndDrain()
        await openTask?.value
        await lifecycleTask?.value
        if state == .noFile || state == .unopened {
            closed = true; events?.cancel(); events = nil
            review?.generationEnded(); review = nil; navigation.endReview()
            invalidateMedia(); await mediaCloseTask?.value
            return true
        }
        if review != nil { await finishReview(); if review != nil { return false } }
        do {
            try await collection.flushForBackground()
            try await collection.close()
            closed = true; events?.cancel(); events = nil; day?.close(); countTimer?.invalidate(); countsTask?.cancel()
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
    @concurrent private nonisolated static func prepareOpenLocation(_ location: CollectionLocation) async throws -> CoreBridge.CollectionLocation {
        // Only the app-private destination root is prepared here. Rust preflights
        // the combined import budget before source copy/inspection/backup writes;
        // the real backup writer owns creation of its supplied backup parents.
        try FileManager.default.createDirectory(at: location.directory, withIntermediateDirectories: true)
        return CoreBridge.CollectionLocation(collectionDirectory: location.directory, backupsDirectory: location.backupsDirectory,
            appGroupSnapshot: location.directory.appending(path: "meguri-widget-unavailable.json"))
    }
    @concurrent private nonisolated static func exists(_ url: URL) async -> Bool { FileManager.default.fileExists(atPath: url.path) }
}

private nonisolated struct LaunchCollectionClock: CollectionClock {
    let clock: AppClock
    let zone: TimeZone?
    func nowMilliseconds() -> Int64 { Int64(clock.now().timeIntervalSince1970 * 1_000) }
    func utcOffsetSeconds(atMilliseconds value: Int64) -> Int32 {
        Int32((zone ?? .autoupdatingCurrent).secondsFromGMT(for: Date(timeIntervalSince1970: Double(value) / 1_000)))
    }
    func zoneIdentifier() -> String { (zone ?? .autoupdatingCurrent).identifier }
}
