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
    private var mediaRevision: UInt64 = 0
    private var mediaTask: Task<Void, Never>?
    private var mediaCloseTask: Task<Void, Never>?
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
    private let deckService = CoreDeckService()
    private let records: ReviewRecordStore
    private var resolver: MediaPathResolver?
    private var events: Task<Void, Never>?
    private var countsTask: Task<Void, Never>?
    private var countTimer: Timer?
    private var countsRevision: UInt64 = 0
    private var lifecycleTask: Task<Void, Never>?
    private var openTask: Task<Void, Never>?
    private var opening = false
    private var starting = false
    private var policyReadAfterSession = false
    private var pendingDeck: Int64?
    private var closed = false
    private var day: ReviewDayMonitor?
    private var recoveryDrops: [Int64] = []

    public init(configuration: LaunchConfiguration, navigation: AppNavigation) {
        loadMediaIndex = { @MainActor in try await CoreMediaService().trackedNames() }
        self.configuration = configuration; self.navigation = navigation
        collection = CoreCollectionService(clock: LaunchCollectionClock(clock: configuration.clock, zone: configuration.timeZone))
        records = ReviewRecordStore(file: configuration.collectionLocation.directory.appending(path: "meguri-review-session.json"))
    }

    public func start() async {
        guard state == .unopened || isFailed, !opening else { return }
        opening = true
        let task = Task { await openCollection() }
        openTask = task
        await task.value
    }
    private func openCollection() async {
        opening = true; state = .loading; error = nil; closed = false
        defer { opening = false }
        do {
            guard await Self.exists(configuration.collectionLocation.collectionFile) else { state = .noFile; return }
            try await CoreEventStream.shared.start()
            if events == nil {
                events = Task { [weak self] in
                    for await event in CoreEventStream.shared.events {
                        guard let self, !Task.isCancelled else { return }
                        self.receive(event)
                    }
                }
            }
            let location = try await Self.prepareOpenLocation(configuration.collectionLocation.directory)
            try await collection.startOpen(location, randomSeed: UInt64.random(in: 0...UInt64.max)).value()
            lifecycle = try await collection.lifecycle()
            if case .ready(let value) = lifecycle { generation = value; state = .ready }
            else { state = .unavailable; return }
            do { previousRecord = try await records.load() }
            catch { self.error = "Couldn’t read the previous session record. \(error)" }
            if var previousRecord, !recoveryDrops.isEmpty {
                for card in recoveryDrops { previousRecord.reject(cardID: card) }
                self.previousRecord = previousRecord
                try await records.save(previousRecord, version: 1)
            }
            recoveryDrops.removeAll()
            day = ReviewDayMonitor(clock: configuration.clock, zone: { [configuration] in configuration.timeZone ?? .autoupdatingCurrent },
                didCheck: { [weak self] in self?.refreshCounts(); self?.review?.requestRefresh() })
            await day?.checkNow()
            await readPolicy()
            await loadCounts()
        } catch { state = .failed(String(describing: error)); self.error = String(describing: error) }
    }
    private var isFailed: Bool { if case .failed = state { return true }; return false }

    private func receive(_ event: CoreEvent) {
        guard !closed else { return }
        // Never drop review commit failures behind a coalesced deck refresh.
        review?.receive(event)
        switch event.kind {
        case .lifecycle(let value):
            let wasReady: Bool = { if case .ready = lifecycle { true } else { false } }()
            lifecycle = value
            switch value {
            case .ready(let next):
                if generation != next { invalidateMedia() }
                generation = next
                if !opening {
                    state = .ready
                    if !wasReady && foreground { Task { await day?.checkNow() } }
                }
            case .opening, .upgrading: state = .loading
            case .exclusiveBusy: state = .unavailable; review?.requestRefresh()
            case .failed(let reason): state = .failed(String(describing: reason))
            case .noCollection: state = .unopened
            }
        case .nextDayAtChanged(let instant): day?.event(nextDayAt: instant)
        case .rolledOver, .timeZoneChanged: refreshCounts()
        case .changes(let changes):
            refreshCounts()
            if changes.wholeCollection || !changes.mediaAdded.isEmpty || !changes.mediaRemoved.isEmpty { scheduleMediaRefresh() }
        case .mediaArrived: scheduleMediaRefresh()
        case .collectionReplaced(let next):
            generation = next; day?.pause()
            invalidateMedia()
            review?.generationEnded(); review = nil; navigation.endReview()
            navigation.dismissSheet(); policy = nil
            Task { await mediaCloseTask?.value; await day?.checkNow(); await readPolicy(); await loadCounts() }
        case .recoveryDropped(_, let cardID, _): recoveryDrops.append(cardID)
        case .sessionEnded:
            if policyReadAfterSession { Task { await readPolicy() } }
        default: break
        }
    }

    public func refreshCounts() {
        countsRevision &+= 1
        guard countsTask == nil, foreground, state == .ready else { return }
        countsTask = Task {
            var observed: UInt64
            repeat {
                observed = countsRevision
                await loadCounts()
            } while observed != countsRevision && foreground && state == .ready
            countsTask = nil
        }
    }
    private func loadCounts() async {
        let expected = generation
        do {
            let tree = try await deckService.treeWithCounts()
            guard expected == generation, tree.generation == generation, !closed else { return }
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
        } catch { self.error = String(describing: error) }
    }
    public func deck(_ id: Int64) -> CountedDeckNode? {
        func find(_ nodes: [CountedDeckNode]) -> CountedDeckNode? {
            for node in nodes { if node.id == id { return node }; if let found = find(node.children) { return found } }
            return nil
        }
        return find(decks)
    }

    public func readPolicy() async {
        guard !enabling else { return }
        do {
            let status = try await Core.fsrsPolicyStatus()
            policy = status
            if review?.sessionID != nil {
                policyReadAfterSession = status != .ok
            } else if status != .ok { navigation.present(.fsrsAdmission) }
        } catch { self.error = String(describing: error) }
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
    public var canStudy: Bool { state == .ready && reviewIsActive && !resuming && !starting && review == nil && policy == .ok }
    public func startStudy(deckID: Int64) async {
        guard state == .ready, reviewIsActive, !resuming, !starting, review == nil else { return }
        guard policy == .ok else { pendingDeck = deckID; await readPolicy(); return }
        starting = true
        defer { starting = false }
        do {
            previousRecord = nil // The visible Home record has now been acknowledged.
            _ = try await deckService.selectDeck(id: deckID)
            guard let resolver = await mediaResolverForReview(), reviewIsActive, state == .ready else { return }
            let model = ReviewCoordinator(deckID: deckID, surface: ReviewPages(resolver: resolver),
                store: records, clock: configuration.clock)
            model.onPolicyRefusal = { [weak self] in self?.pendingDeck = deckID }
            reviewVisible = true
            review = model
            navigation.startReview(ReviewPresentation(deck: DeckID(rawValue: deckID), session: model))
            await model.start()
            if !reviewIsActive { model.beginPause(); try await model.pause() }
            if model.sessionID == nil {
                navigation.endReview(); review = nil
                if pendingDeck != nil { await readPolicy() }
                else { error = model.notice }
            }
        } catch { self.error = String(describing: error) }
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
            await openTask?.value
            guard !closed, state != .unopened, state != .noFile else { return }
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
        await openTask?.value
        await lifecycleTask?.value
        if state == .noFile || state == .unopened { return true }
        if review != nil { await finishReview(); if review != nil { return false } }
        do {
            try await collection.flushForBackground()
            try await collection.close()
            closed = true; day?.close(); countTimer?.invalidate(); countsTask?.cancel()
            invalidateMedia(); await mediaCloseTask?.value
            state = .unopened; lifecycle = .noCollection
            return true
        } catch { self.error = String(describing: error); return false }
    }
    private func invalidateMedia() {
        mediaRevision &+= 1
        let old = resolver; resolver = nil
        let previous = mediaCloseTask
        mediaCloseTask = Task { await previous?.value; await old?.close() }
    }
    private func scheduleMediaRefresh() {
        mediaRevision &+= 1
        guard foreground, state == .ready else { return }
        Task { await refreshMedia() }
    }
    func mediaResolverForReview() async -> MediaPathResolver? {
        await refreshMedia()
        return resolver
    }
    private func refreshMedia() async {
        if let mediaTask { await mediaTask.value; return }
        let task = Task {
            var observed: UInt64
            repeat {
                observed = mediaRevision
                let expected = generation
                do {
                    await mediaCloseTask?.value
                    let snapshot = try await loadMediaIndex()
                    let root = configuration.collectionLocation.directory.appending(path: "collection.media", directoryHint: .isDirectory)
                    guard expected == generation, observed == mediaRevision, !closed, foreground else { continue }
                    guard Self.acceptsMedia(snapshot, generation: expected, root: root) else {
                        throw BridgeError.unavailable(reason: "Media journal binding differs from the open collection")
                    }
                    let names = snapshot.names.map { MediaPathResolver.TrackedName(name: $0.name, caseKey: $0.caseKey) }
                    if let resolver { await resolver.update(trackedNames: names) }
                    else {
                        let bound = try await MediaPathResolver.open(root: root, trackedNames: names)
                        guard expected == generation, observed == mediaRevision, !closed else { await bound.close(); continue }
                        resolver = bound
                    }
                } catch {
                    if expected == generation, observed == mediaRevision {
                        invalidateMedia()
                        self.error = "Couldn’t read the collection media journal. \(error)"
                        review?.mediaUnavailable()
                        break
                    }
                }
            } while observed != mediaRevision && foreground && !closed
        }
        mediaTask = task
        await task.value
        mediaTask = nil
    }
    static func acceptsMedia(_ snapshot: MediaNameSnapshot, generation: UInt64, root: URL) -> Bool {
        snapshot.generation == generation && snapshot.root.isFileURL &&
            Data(snapshot.root.standardizedFileURL.path.utf8) == Data(root.standardizedFileURL.path.utf8)
    }
    @concurrent private nonisolated static func prepareOpenLocation(_ directory: URL) async throws -> CoreBridge.CollectionLocation {
        let backups = directory.deletingLastPathComponent().appending(path: "Backups", directoryHint: .isDirectory)
        // Core canonicalizes both roots before opening; the app owns this directory.
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        return CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
            appGroupSnapshot: directory.appending(path: "meguri-widget-unavailable.json"))
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
