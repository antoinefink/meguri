import CardRendering
import CoreBridge
import Foundation
import Observation
#if DEBUG
import OSLog
#endif

@Observable
public final class ReviewCoordinator {
    public enum Phase: Equatable { case loading, question, answer, transition, waiting, finished, ended, error }
    public private(set) var phase: Phase = .loading
    public private(set) var item: ReviewHeadItem?
    public private(set) var counts = ReviewCounts(new: 0, learning: 0, review: 0)
    public private(set) var sessionID: UInt64?
    public private(set) var notice: String?
    public private(set) var undoStatus: ReviewUndoStatus = .unavailable(reason: .none)
    public private(set) var redoStatus: ReviewUndoStatus = .unavailable(reason: .none)
    public private(set) var record: ReviewRecord
    public private(set) var studyState: ReviewDeckStudyState?
    public private(set) var waitingUntil: Date?
    public private(set) var paused = false
    public let surface: any ReviewCardSurface
    public var onPolicyRefusal: (() -> Void)?
    // Named attachments for U154/U177; no fake summary/sync/backup providers.
    public var onRecordChanged: ((ReviewRecord) -> Void)?
    /// Collection-owned routing; pages retain only render metadata, never sync ownership.
    public var onVisibleMediaChanged: (([String]) -> Void)?
    public private(set) var visibleMediaNames: [String] = []
    private let client: any ReviewSessionClient
    private let store: ReviewRecordStore
    private let clock: AppClock
    private let nativeInput: NativeReviewInput?
    private let recordDiagnostic: FieldDiagnosticRecorder
    private let nativeCoordinator = UUID()
    private let nativeGeneration: UInt64
    var nativeInputEligibility: () -> Bool = { true }
    private let uptime: () -> TimeInterval
    private var timer = ReviewActiveTime()
    private var activeTime = ReviewActiveTime()
    private var sideIsAnswer = false
    private var presentedKey: ReviewRenderKey?
    private var terminalState: ReviewTerminalState?
    // Presentation ownership survives refresh's phase changes. Head facts remain
    // authoritative, but only an explicit action may leave terminal content.
    private var terminalPresentation: Phase?
    private enum PresentationIntent: Equatable { case automatic, explicit, restoration(UUID) }
    private var restorationID: UUID?
    private var busy = false
    private var refreshPending = false
    private var forceQuestion = false
    private var pendingAnswer: ReviewRecord.Answer?
    private var version: UInt64 = 0
    private var preload: Task<Void, Never>?
    private var operation: Task<Void, Never>?
    private var resumeOperation: Task<Void, any Error>?
    private var pagePreparation: Task<Void, any Error>?
    private var closingAdmission = false
    private var startupNeedsPreparation = false
    private(set) var summaryTask: Task<Void, Never>?
    private var deadlineTimer: Timer?
    private var endTask: Task<Bool, Never>?
    private var pauseRevision: UInt64 = 0
    private var mediaArrivals = ReviewMediaArrivals()
    private var mediaPresentationRevision: UInt64 = 0
    // An internal seam lets model tests observe retries without unhosted WebKit.
    var retryMedia: ([String]) async throws -> Bool

    public init(deckID: Int64, surface: any ReviewCardSurface, store: ReviewRecordStore,
                clock: AppClock = .system, client: any ReviewSessionClient = CoreReviewSessionClient(),
                uptime: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                nativeInput: NativeReviewInput? = nil, nativeGeneration: UInt64 = 0,
                recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        self.recordDiagnostic = recordDiagnostic
        self.nativeInput = nativeInput; self.nativeGeneration = nativeGeneration
        self.surface = surface; self.store = store; self.clock = clock; self.client = client; self.uptime = uptime
        retryMedia = { names in
            guard let pages = surface as? ReviewPages else { return false }
            return try await pages.retryMedia(names)
        }
        record = ReviewRecord(deckID: deckID, startedAt: clock.now())
        surface.onAction = { [weak self] action in
            switch action {
            case .reveal: self?.reveal()
            case .again: self?.rate(.again)
            case .good: self?.rate(.good)
            case .play: break // CardPagePair already owns playback.
            }
        }
        surface.onRecovery = { [weak self] in self?.requestRefresh(question: true) }
        (surface as? ReviewPages)?.onMediaReady = { [weak self] in self?.mediaPresentationReady() }
        if nativeInput != nil, let pair = surface.pair {
            let previous = pair.onPlaybackObservation
            pair.onPlaybackObservation = { [weak self, weak pair] entry, observation in
                previous?(entry, observation)
                guard let self, let pair, let session = self.sessionID else { return }
                self.nativeInput?.observePlayback(pair: pair, entry: entry, observation: observation,
                    coordinator: self.nativeCoordinator, session: session, generation: self.nativeGeneration)
            }
        }
    }
    public var pair: CardPagePair? { surface.pair }
    public var canFinish: Bool { !busy && phase != .ended }
    public var continueAvailable: Bool {
        guard terminalPresentation != nil else { return false }
        switch terminalState {
        case .active: return true
        case .waitingUntil(let instant): return Double(instant) <= clock.now().timeIntervalSince1970
        default: return false
        }
    }
    public var canContinue: Bool {
        continueAvailable && !busy && !paused && !closingAdmission && sessionID != nil &&
            (phase == .waiting || phase == .finished)
    }
    public var canRefresh: Bool { !busy && !paused && !closingAdmission && sessionID != nil && phase != .ended }
    public var canReveal: Bool { phase == .question && !busy && !paused }
    public var canRate: Bool { phase == .answer && !busy && !paused }
    public var canUndo: Bool { if case .available = undoStatus { return !busy && !paused && phase != .ended }; return false }
    public var canRedo: Bool { if case .available = redoStatus { return !busy && !paused && phase != .ended }; return false }
    public var progress: Double { sessionProgress.fraction }
    /// Answers given and cards left in this session, for the bottom-edge bar (N16.1).
    public var sessionProgress: ReviewSessionProgress {
        ReviewSessionProgress(answered: record.answers.count,
            remaining: Int(counts.new) + Int(counts.learning) + Int(counts.review))
    }
    /// What the end of a session states (N16.5). Active time is the monotonic
    /// study clock, which stops while the session is paused or at its end.
    public var sessionSummary: ReviewSessionSummary {
        ReviewSessionSummary(cards: Set(record.answers.map(\.cardID)).count,
            seconds: activeTime.milliseconds(at: uptime(), capSeconds: UInt32.max) / 1_000)
    }
    #if DEBUG
    private var startObservation: ReviewStartObservation?
    public var debugSessionStatus: String {
        switch phase {
        case .loading:
            if sessionID != nil { return "cardLoad" }
            let stage = startObservation?.stage
            if stage == .maintenanceQueued,
               let call = MaintenanceCallObservation.shared.currentCall {
                return "maintenanceQueued:\(call.name)"
            }
            return stage?.rawValue ?? "coreStart"
        case .question: return "question"
        case .answer: return "answer"
        case .transition: return "transition"
        case .waiting: return "waiting"
        case .finished: return "finished"
        case .ended: return "ended"
        case .error: return "error"
        }
    }
    #endif

    public func start() async {
        guard !closingAdmission, sessionID == nil, !busy, phase == .loading else { return }
        recordDiagnostic(.init(component: .review, operation: .start, stage: .requested, outcome: .observed))
        busy = true
        // Core start owns admission through its terminal result, even if pause
        // arrives before either task executes. Only page preparation is cancelled.
        let coreStart = Task {
            #if DEBUG
            let observation = ReviewStartObservation()
            startObservation = observation
            return try await client.start(deckID: record.deckID, observation: observation)
            #else
            return try await client.start(deckID: record.deckID)
            #endif
        }
        let task = Task { await startOwned(coreStart) }
        operation = task
        await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
    }
    private func startOwned(_ coreStart: Task<ReviewStartResult, any Error>) async {
        var failureStage = FieldDiagnosticObservation.Stage.admission
        #if DEBUG
        let logger = Logger(subsystem: "Meguri", category: "ReviewStartup")
        logger.notice("start.begin")
        #endif
        do {
            let result = try await coreStart.value
            failureStage = .presentation
            #if DEBUG
            logger.notice("core.returned")
            #endif
            switch result {
            case .refused(.fsrsPolicyPending):
                recordDiagnostic(.init(component: .review, operation: .start, stage: .admission, outcome: .refused))
                onPolicyRefusal?(); phase = .error; notice = "Meguri couldn’t prepare this collection for study."
            case .refused(.presetUnsupported):
                recordDiagnostic(.init(component: .review, operation: .start, stage: .admission, outcome: .refused))
                phase = .error; notice = "This preset needs a newer Meguri"
            case .started(let id, let generation, let head, _):
                recordDiagnostic(.init(component: .review, operation: .start, stage: .coreAccepted, outcome: .succeeded))
                sessionID = id
                try nativeInput?.bindSession(coordinator: nativeCoordinator, session: id, generation: generation, deck: record.deckID)
                startupNeedsPreparation = true
                activeTime.start(at: uptime())
                if paused { activeTime.pause(at: uptime()) }
                #if DEBUG
                logger.notice("surface.begin")
                #endif
                let loaded = try await preparePages { try await self.surface.load() }
                if loaded { startupNeedsPreparation = false }
                #if DEBUG
                logger.notice("surface.end")
                logger.notice("apply.begin")
                #endif
                try await apply(head, question: true)
                #if DEBUG
                logger.notice("apply.end")
                logger.notice("persist.begin")
                #endif
                failureStage = .persistence
                try await persist()
                #if DEBUG
                logger.notice("persist.end")
                #endif
            }
        } catch {
            recordDiagnostic(.init(component: .review, operation: .start,
                stage: failureStage, outcome: .failed, failure: .other))
            #if DEBUG
            logger.notice("start.failed")
            #endif
            fail(error)
        }
        completeOperation()
        #if DEBUG
        logger.notice("start.complete")
        #endif
    }

    public func reveal() {
        guard phase == .question, !busy, !paused else { return }
        busy = true; phase = .transition
        let inputTime = uptime()
        operation = Task {
            #if DEBUG
            let logger = Logger(subsystem: "Meguri", category: "ReviewAnswerStage")
            #endif
            do {
                #if DEBUG
                logger.debug("surfaceReveal.begin")
                #endif
                _ = try await preparePages { [self] in
                    try await surface.reveal(inputTime: inputTime)
                    try Task.checkCancellation()
                    #if DEBUG
                    logger.debug("surfaceReveal.end")
                    #endif
                    if let nativeInput, let pair = surface.pair, let sessionID {
                        #if DEBUG
                        logger.debug("nativeImage.begin")
                        #endif
                        try await nativeInput.captureImage(pair: pair, coordinator: nativeCoordinator,
                            session: sessionID, generation: nativeGeneration)
                        #if DEBUG
                        logger.debug("nativeImage.end")
                        #endif
                    }
                    try Task.checkCancellation()
                    sideIsAnswer = true
                    phase = .answer
                }
            } catch { nativeInput?.failed(error); fail(error) }
            completeOperation()
            #if DEBUG
            logger.debug("reveal.complete")
            #endif
        }
    }

    public func rate(_ rating: ReviewRating) {
        guard phase == .answer, !busy, !paused, let item, let sessionID else { return }
        // All input paths enter this synchronous gate, before any suspension.
        busy = true; phase = .transition
        let inputTime = uptime()
        let instant = clock.now()
        let use: NativeReviewInput.AnswerUse?
        do {
            if let nativeInput {
                use = try surface.admitNativeInput(nativeInput, coordinator: nativeCoordinator, session: sessionID,
                    generation: nativeGeneration, deck: record.deckID, item: item, rating: rating,
                    instant: Int64(instant.timeIntervalSince1970 * 1000),
                    eligible: nativeInputEligibility() && terminalState == .active && sideIsAnswer && !refreshPending &&
                        presentedKey == item.renderKey)
            } else { use = nil }
        } catch { busy = false; nativeInput?.failed(error); fail(error); return }
        timer.pause(at: inputTime)
        let elapsed = use?.elapsed ?? timer.milliseconds(at: inputTime, capSeconds: item.homePreset.maximumAnswerSeconds)
        let answer = ReviewRecord.Answer(cardID: item.cardId, rating: Self.name(rating), kind: String(describing: item.kind),
                                         answeredAt: instant, elapsedMilliseconds: elapsed)
        pendingAnswer = answer
        operation = Task {
            var answerReturned = false
            do {
                let outcome = try await client.answer(sessionID: sessionID, item: item, rating: rating,
                    at: Int64(instant.timeIntervalSince1970 * 1_000), elapsed: elapsed)
                answerReturned = true
                switch outcome {
                case .accepted(let diff, let leech, let suspended, _):
                    recordDiagnostic(.init(component: .review, operation: .answer, stage: .coreAccepted, outcome: .succeeded))
                    if let use { nativeInput?.completed(use, outcome: "accepted") }
                    // A failure may arrive while answer() returns. Never resurrect it.
                    if let pendingAnswer, pendingAnswer.id == answer.id {
                        record.answers.append(pendingAnswer); record.undone.removeAll()
                        self.pendingAnswer = nil
                    }
                    if leech { notice = suspended ? "Card was a leech. It has been suspended." : "Card was a leech." }
                    if suspended { record.suspended.insert(item.cardId) }
                    enqueuePersistence(diagnosticAnswer: true)
                    let needsRefresh = refreshPending
                    let intent = restorationIntent
                    refreshPending = false; forceQuestion = false
                    do {
                        let head = needsRefresh ? try await client.head(sessionID: sessionID) : diff.head
                        try await apply(head, question: true, inputTime: inputTime, intent: intent)
                        recordDiagnostic(.init(component: .review, operation: .answer, stage: .presentation,
                            outcome: paused || closingAdmission ? .waiting : .succeeded))
                    } catch {
                        recordDiagnostic(.init(component: .review, operation: .answer, stage: .presentation, outcome: .failed, failure: .other))
                        // Presentation failure cannot skip the accepted record.
                        try await persist(diagnosticAnswer: true)
                        throw error
                    }
                    try await persist(diagnosticAnswer: true)
                case .stale(let head, let showNotice):
                    recordDiagnostic(.init(component: .review, operation: .answer, stage: .coreAccepted, outcome: .stale))
                    if let use { nativeInput?.completed(use, outcome: "stale") }
                    pendingAnswer = nil
                    if showNotice { notice = "Card changed, please answer again" }
                    try await apply(head, question: true)
                case .notAtHead(let head):
                    recordDiagnostic(.init(component: .review, operation: .answer, stage: .coreAccepted, outcome: .refused))
                    if let use { nativeInput?.completed(use, outcome: "not-at-head") }
                    pendingAnswer = nil
                    try await apply(head, question: true)
                }
            } catch {
                if !answerReturned {
                    recordDiagnostic(.init(component: .review, operation: .answer, stage: .coreAccepted, outcome: .failed, failure: .other))
                }
                if let use { nativeInput?.completed(use, outcome: "failed") }
                pendingAnswer = nil
                notice = String(describing: error)
                // An uncertain answer is never retried; ask the authoritative head.
                let intent = restorationIntent
                refreshPending = false; forceQuestion = false
                do { try await apply(client.head(sessionID: sessionID), question: true, intent: intent) }
                catch { fail(error) }
            }
            completeOperation()
        }
    }

    public func restore(redo: Bool = false) {
        guard (redo ? canRedo : canUndo), let sessionID else { return }
        busy = true; phase = .transition
        let intent = beginRestoration()
        timer.pause(at: uptime())
        operation = Task {
            do {
                let head = try await client.restore(sessionID: sessionID, redo: redo)
                if redo {
                    if var answer = record.undone.popLast() { answer.durable = false; record.answers.append(answer) }
                } else if let answer = record.answers.popLast() { record.undone.append(answer) }
                do { try await apply(head, question: true, intent: intent) }
                catch {
                    // The original Undo/Redo record must drain even if rendering fails.
                    try await persist()
                    throw error
                }
                try await persist()
            } catch {
                notice = String(describing: error)
                let recoveryIntent = restorationIntent
                refreshPending = false; forceQuestion = false
                do { try await apply(client.head(sessionID: sessionID), question: true, intent: recoveryIntent) }
                catch { fail(error) }
            }
            completeOperation()
        }
    }

    /// Ordered fan-out calls this synchronously, including while rendering awaits.
    public func receive(_ event: CoreEvent) {
        switch event.kind {
        case .commitFailed(let id, let cardID, let error) where id == sessionID:
            commitFailed(cardID: cardID, error: error)
        case .queueHeadChanged(let id, _) where id == sessionID:
            requestRefresh()
        case .currentCardChanged(let id, _, _) where id == sessionID:
            requestRefresh()
        case .presetUnsupported(let id, _) where id == sessionID:
            notice = "This preset needs a newer Meguri"; phase = .error
        case .waitingUntil(let id, let instant) where id == sessionID:
            terminalChanged(.waitingUntil(instant: instant))
        case .finished(let id, _) where id == sessionID:
            terminalChanged(.finished)
        case .changes(let change):
            // Sync changes, including cloud rekeys, cannot acknowledge a local answer commit.
            guard change.source != .syncIncoming, change.source != .cloudIncoming,
                  change.source != .cloudRekey else { break }
            // Review commits are ordered. Retain accepted-vs-durable distinction.
            for card in change.cards where !change.revlogAdded.isEmpty {
                if let index = record.answers.firstIndex(where: { $0.cardID == card.id && !$0.durable }) {
                    record.answers[index].durable = true
                } else if pendingAnswer?.cardID == card.id { pendingAnswer?.durable = true }
            }
        default: break
        }
    }

    func terminalChanged(_ hint: ReviewTerminalState) {
        // head() itself emits terminal events. A hint already satisfied by
        // the applied head must not start another polling cycle.
        if terminalState != hint { requestRefresh() }
    }

    func commitFailed(cardID: Int64, error: BridgeError) {
        nativeInput?.failed(error)
        record.reject(cardID: cardID)
        pendingAnswer = nil
        notice = "Couldn’t save the answer. \(error)"
        _ = beginRestoration()
        requestRefresh(question: true)
        enqueuePersistence()
    }

    public func requestRefresh(question: Bool = false) {
        guard !closingAdmission, sessionID != nil, phase != .ended else { return }
        refreshPending = true; forceQuestion = forceQuestion || question
        if !paused && terminalPresentation == nil { phase = .transition }
        if !busy && !paused { refresh() }
    }
    public func retry() {
        // Check Now refreshes facts. Error Retry explicitly recovers presentation.
        if phase == .error { _ = beginRestoration() }
        requestRefresh(question: true)
    }
    public func continueReview() {
        guard canContinue else { return }
        forceQuestion = true
        refresh(intent: .explicit)
    }
    private var restorationIntent: PresentationIntent {
        restorationID.map { .restoration($0) } ?? .automatic
    }
    private func beginRestoration() -> PresentationIntent {
        guard !closingAdmission, sessionID != nil, phase != .ended else { return .automatic }
        let id = UUID()
        restorationID = id
        return .restoration(id)
    }
    private func finishRestoration(_ intent: PresentationIntent) {
        // Older reads/presentations cannot consume a newer recovery event.
        if case .restoration(let id) = intent, restorationID == id { restorationID = nil }
    }
    private func refresh(intent: PresentationIntent = .automatic) {
        guard !closingAdmission, let sessionID, !busy, !paused else { return }
        let restoration = restorationIntent
        let presentationIntent = restoration == .automatic ? intent : restoration
        busy = true
        if terminalPresentation == nil { phase = .transition }
        let question = forceQuestion; forceQuestion = false; refreshPending = false
        operation = Task {
            do { try await apply(client.head(sessionID: sessionID), question: question, intent: presentationIntent); try await persist() }
            catch { fail(error) }
            completeOperation()
        }
    }
    private func completeOperation() {
        busy = false
        if refreshPending && !paused { refresh() }
        else { retryArrivedMedia() }
    }

    func mediaArrived(_ names: [String]) {
        guard phase != .ended else { return }
        guard mediaArrivals.insert(names) else { mediaUnavailable(); return }
        retryArrivedMedia()
    }
    func mediaPresentationReady() {
        mediaPresentationRevision &+= 1
        retryArrivedMedia()
    }
    private func retryArrivedMedia() {
        guard !busy, !paused, sessionID != nil, phase == .question || phase == .answer,
              !mediaArrivals.isEmpty else { return }
        let names = mediaArrivals.take(), revision = pauseRevision, expected = sessionID
        let presentation = mediaPresentationRevision
        busy = true
        operation = Task {
            var deferred = false
            do {
                deferred = !(try await retryMedia(names))
            } catch {
                if revision == pauseRevision, sessionID == expected, phase != .ended {
                    notice = "Couldn’t refresh card media. Try again, or return to Home."
                    phase = .error
                }
            }
            guard sessionID == expected, phase != .ended else { busy = false; return }
            if deferred || revision != pauseRevision {
                if !mediaArrivals.insert(names) { mediaUnavailable() }
                // Presentation/resume owns the next attempt; never spin on readiness.
                busy = false
                if refreshPending && !paused { refresh() }
                else if presentation != mediaPresentationRevision { retryArrivedMedia() }
            } else { completeOperation() }
        }
    }

    private func apply(_ head: ReviewHead, question: Bool, inputTime: TimeInterval? = nil,
                       whilePaused: Bool = false, intent: PresentationIntent = .automatic) async throws {
        guard !closingAdmission, sessionID != nil, phase != .ended else { return }
        summaryTask?.cancel(); summaryTask = nil
        counts = head.counts
        terminalState = head.terminalState
        guard let sessionID else { return }
        let previous = item
        item = head.items.first
        switch head.terminalState {
        case .active:
            guard item != nil else { throw BridgeError.internal("Active review head has no item") }
            studyState = nil; waitingUntil = nil
            if let terminalPresentation, intent == .automatic {
                // No render, present or autoplay while terminal content owns the
                // screen, including deadline, event and resume reads.
                phase = terminalPresentation
                timer.pause(at: uptime()); activeTime.pause(at: uptime())
                clearVisibleMedia()
            } else {
                _ = try await preparePages(whilePaused: whilePaused) {
                    try await self.preparePresentation(head, sessionID: sessionID, previous: previous,
                                                       question: question, inputTime: inputTime, intent: intent)
                }
            }
        case .waitingUntil, .finished:
            clearVisibleMedia()
            preload?.cancel(); await preload?.value; preload = nil
            timer.pause(at: uptime()); activeTime.pause(at: uptime()); surface.pair?.pauseAudio()
            studyState = nil
            if case .waitingUntil(let instant) = head.terminalState {
                waitingUntil = Date(timeIntervalSince1970: Double(instant)); phase = .waiting
            } else { waitingUntil = nil; phase = .finished }
            terminalPresentation = phase
            // An empty head settles the restoration it belongs to.
            // Later work still requires a new explicit action.
            finishRestoration(intent)
            // Committed facts are optional and must not hold operation/lifecycle admission.
            let revision = pauseRevision
            summaryTask = Task { [weak self, client, deckID = record.deckID] in
                do {
                    let state = try await client.studyState(deckID: deckID)
                    guard let self, !Task.isCancelled, self.sessionID == sessionID,
                          self.pauseRevision == revision,
                          self.phase == .waiting || self.phase == .finished else { return }
                    self.studyState = state
                    if self.phase == .finished, self.record.finished != state.finished {
                        self.record.finished = state.finished
                        self.enqueuePersistence()
                    }
                } catch {
                    guard let self, !Task.isCancelled, self.sessionID == sessionID,
                          self.pauseRevision == revision,
                          self.phase == .waiting || self.phase == .finished else { return }
                    self.notice = "Couldn’t load the study summary. \(error)"
                }
            }
        }
        undoStatus = try await client.undoStatus(sessionID: sessionID, redo: false)
        redoStatus = try await client.undoStatus(sessionID: sessionID, redo: true)
        arm(at: min(Date(timeIntervalSince1970: Double(head.validUntil)), waitingUntil ?? .distantFuture))
    }

    /// One child owns only visual preparation. Its parent retains Core calls and
    /// record writes, and joins this child before lifecycle physical suspension.
    @discardableResult private func preparePages(
        whilePaused: Bool = false,
        _ work: @escaping @MainActor () async throws -> Void
    ) async throws -> Bool {
        guard !closingAdmission, whilePaused || !paused else { forceQuestion = true; return false }
        let preparation = Task {
            try Task.checkCancellation()
            try await work()
            try Task.checkCancellation()
        }
        pagePreparation = preparation
        let result = await withTaskCancellationHandler {
            await preparation.result
        } onCancel: {
            preparation.cancel()
        }
        pagePreparation = nil
        if case .failure(let error) = result {
            // Cancellation belongs to this child, never to a Core result or
            // persistence failure observed after a lifecycle change.
            if error is CancellationError && preparation.isCancelled && paused {
                forceQuestion = true
                return false
            }
            throw error
        }
        return true
    }

    private func preparePresentation(_ head: ReviewHead, sessionID: UInt64, previous: ReviewHeadItem?,
                                     question: Bool, inputTime: TimeInterval?, intent: PresentationIntent) async throws {
        guard let item = head.items.first else { throw BridgeError.internal("Active review head has no item") }
        #if DEBUG
        let logger = Logger(subsystem: "Meguri", category: "ReviewStartup")
        logger.notice("render.begin")
        #endif
        try await surface.render(sessionID: sessionID, items: head.items)
        try Task.checkCancellation()
        #if DEBUG
        logger.notice("render.end")
        #endif
        guard self.sessionID == sessionID else { throw BridgeError.unavailable(reason: "Session ended during rendering") }
        publishVisibleMedia()
        let needsPresentation = question || previous?.cardId != item.cardId || presentedKey != item.renderKey
        if needsPresentation {
            preload?.cancel(); await preload?.value; preload = nil
            try Task.checkCancellation()
            let keepAnswer = !question && sideIsAnswer && previous?.cardId == item.cardId
            #if DEBUG
            logger.notice("present.begin")
            #endif
            try await surface.present(item: item, inputTime: inputTime ?? uptime())
            try Task.checkCancellation()
            #if DEBUG
            logger.notice("present.end")
            #endif
            guard self.sessionID == sessionID else { throw BridgeError.unavailable(reason: "Session ended during presentation") }
            presentedKey = item.renderKey
            publishVisibleMedia()
            sideIsAnswer = false
            timer.start(at: uptime())
            if paused { timer.pause(at: uptime()) }
            if keepAnswer { try await surface.reveal(inputTime: uptime()); sideIsAnswer = true }
        }
        try Task.checkCancellation()
        terminalPresentation = nil
        phase = sideIsAnswer ? .answer : .question
        finishRestoration(intent)
        if !paused { activeTime.resume(at: uptime()) }
        if let next = head.items.dropFirst().first {
            preload?.cancel(); await preload?.value
            try Task.checkCancellation()
            preload = Task { try? await surface.preload(item: next) }
        }
    }

    private func arm(at date: Date) {
        deadlineTimer?.invalidate(); deadlineTimer = nil
        guard !paused, date < .distantFuture, !clock.isFixed else { return }
        let fire = max(date, Date(timeIntervalSince1970: floor(clock.now().timeIntervalSince1970) + 1))
        deadlineTimer = Timer(fire: fire, interval: 0, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.requestRefresh() }
        }
        if let deadlineTimer { RunLoop.main.add(deadlineTimer, forMode: .common) }
    }

    public func beginPause() {
        summaryTask?.cancel()
        pauseRevision &+= 1
        paused = true
        // Only the shared visual child is cancelled; original Core calls and
        // accepted/Undo record persistence drain in their parent operations.
        if pagePreparation != nil {
            forceQuestion = true
            pagePreparation?.cancel()
        }
        (surface as? ReviewPages)?.inputActive = false
        surface.pair?.ownsKeyboardFocus = false
        timer.pause(at: uptime()); activeTime.pause(at: uptime())
        deadlineTimer?.invalidate(); deadlineTimer = nil
        surface.pair?.pauseAudio()
    }
    public func pause() async throws {
        beginPause()
        timer.pause(at: uptime()); activeTime.pause(at: uptime())
        deadlineTimer?.invalidate(); deadlineTimer = nil
        surface.pair?.pauseAudio()
        await operation?.value
        _ = await resumeOperation?.result
        timer.pause(at: uptime()); activeTime.pause(at: uptime())
        preload?.cancel(); await preload?.value; preload = nil
        await surface.suspend()
        try await persist()
    }
    public func resume() async throws {
        guard !closingAdmission else { return }
        if let resumeOperation { return try await resumeOperation.value }
        guard let sessionID, phase != .ended, paused else { return }
        let revision = pauseRevision
        await operation?.value
        guard !closingAdmission, self.sessionID == sessionID, phase != .ended,
              paused, revision == pauseRevision else { return }
        if let resumeOperation { return try await resumeOperation.value }
        busy = true
        let task = Task { try await resumeOwned(sessionID: sessionID, revision: revision) }
        resumeOperation = task
        try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }
    private func resumeOwned(sessionID: UInt64, revision: UInt64) async throws {
        defer { resumeOperation = nil }
        guard !closingAdmission, self.sessionID == sessionID, phase != .ended,
              paused, revision == pauseRevision else { busy = false; return }
        do {
            let ready = try await preparePages(whilePaused: true) {
                await self.surface.resume()
                try Task.checkCancellation()
                if self.startupNeedsPreparation { try await self.surface.load() }
            }
            // A second Pause cancels only preparation; caller cancellation cancels this owner too.
            try Task.checkCancellation()
            guard ready, !closingAdmission, revision == pauseRevision else { busy = false; return }
            startupNeedsPreparation = false
            // Consume only requests known before this read. Later events own
            // another fresh read, including those received during presentation.
            let intent = restorationIntent
            let question = forceQuestion
            forceQuestion = false; refreshPending = false
            // Read the retained Core session afresh, outside cancellable visuals.
            let head = try await client.head(sessionID: sessionID)
            guard !closingAdmission, self.sessionID == sessionID, phase != .ended,
                  revision == pauseRevision else { busy = false; return }
            try await apply(head, question: question, whilePaused: true, intent: intent)
            try Task.checkCancellation()
            guard !closingAdmission, revision == pauseRevision else { busy = false; return }
            paused = false
            (surface as? ReviewPages)?.inputActive = true
            if item != nil, phase == .question || phase == .answer {
                timer.resume(at: uptime()); activeTime.resume(at: uptime())
            }
            completeOperation()
        } catch {
            paused = true; busy = false
            fail(error); throw error
        }
    }
    public func mediaUnavailable() {
        mediaArrivals = ReviewMediaArrivals()
        beginPause()
        notice = "Collection media is unavailable. Return to Home and try again."
        phase = .error
    }

    private func publishVisibleMedia() {
        // An event delivered while rendering awaits already requires a newer
        // head. Do not publish that obsolete head; the next apply covers cache hits too.
        guard sessionID != nil, phase != .ended, !refreshPending,
              let pages = surface as? ReviewPages else { return }
        setVisibleMedia(pages.visibleMediaNames)
    }
    private func setVisibleMedia(_ names: [String]) {
        guard names != visibleMediaNames else { return }
        visibleMediaNames = names
        onVisibleMediaChanged?(names)
    }
    private func clearVisibleMedia() {
        (surface as? ReviewPages)?.clearVisibleMedia()
        setVisibleMedia([])
    }

    /// Idempotent across Close, sidebar navigation, window close and app quit.
    @discardableResult public func end() async -> Bool {
        if let endTask { return await endTask.value }
        closingAdmission = true
        restorationID = nil
        beginPause()
        let task = Task { () -> Bool in
            do {
                try await pause()
                if let sessionID {
                    try await client.end(sessionID: sessionID)
                    nativeInput?.ended(coordinator: nativeCoordinator, session: sessionID)
                    self.sessionID = nil
                }
                record.endedAt = clock.now()
                try await persist()
                sessionID = nil; phase = .ended; surface.close()
                clearVisibleMedia()
                mediaArrivals = ReviewMediaArrivals()
                return true
            } catch { fail(error); return false }
        }
        endTask = task
        let result = await task.value
        if !result { endTask = nil }
        return result
    }
    public func generationEnded() {
        closingAdmission = true
        restorationID = nil
        summaryTask?.cancel()
        paused = true; deadlineTimer?.invalidate(); preload?.cancel()
        sessionID = nil; phase = .ended; surface.close()
        clearVisibleMedia()
        mediaArrivals = ReviewMediaArrivals()
    }
    private func persist(diagnosticAnswer: Bool = false) async throws {
        record.activeMilliseconds = activeTime.milliseconds(at: uptime(), capSeconds: UInt32.max)
        do {
            if version == 0 { try await store.begin(record) }
            version &+= 1
            let snapshot = record; let revision = version
            try await store.save(snapshot, version: revision)
            if diagnosticAnswer { recordDiagnostic(.init(component: .review, operation: .save, stage: .persistence, outcome: .succeeded)) }
            onRecordChanged?(snapshot)
        } catch {
            if diagnosticAnswer { recordDiagnostic(.init(component: .review, operation: .save, stage: .persistence, outcome: .failed, failure: .storageUnavailable)) }
            throw error
        }
    }
    private func enqueuePersistence(diagnosticAnswer: Bool = false) {
        version &+= 1
        let snapshot = record; let revision = version; let store = store; let recordDiagnostic = recordDiagnostic
        Task { [weak self] in
            do {
                try await store.save(snapshot, version: revision)
                if diagnosticAnswer { recordDiagnostic(.init(component: .review, operation: .save, stage: .persistence, outcome: .succeeded)) }
            } catch {
                if diagnosticAnswer { recordDiagnostic(.init(component: .review, operation: .save, stage: .persistence, outcome: .failed, failure: .storageUnavailable)) }
                self?.notice = "Couldn’t save the session record. \(error)"
            }
        }
    }
    func settled() async {
        while busy { let task = operation; await task?.value; if case nil = task { await Task.yield() } }
    }
    private func fail(_ error: any Error) {
        guard phase != .ended else { return }
        notice = String(describing: error); phase = .error
    }
    public static func name(_ rating: ReviewRating) -> String {
        switch rating { case .again: "again"; case .hard: "hard"; case .good: "good"; case .easy: "easy" }
    }
}
