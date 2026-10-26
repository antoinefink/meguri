import CardRendering
import CoreBridge
import Foundation
import Observation

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
    private let client: any ReviewSessionClient
    private let store: ReviewRecordStore
    private let clock: AppClock
    private let nativeInput: NativeReviewInput?
    private let nativeCoordinator = UUID()
    private let nativeGeneration: UInt64
    var nativeInputEligibility: () -> Bool = { true }
    private let uptime: () -> TimeInterval
    private var timer = ReviewActiveTime()
    private var activeTime = ReviewActiveTime()
    private var sideIsAnswer = false
    private var frozenDisplaySeconds: UInt64?
    private var presentedKey: ReviewRenderKey?
    private var terminalState: ReviewTerminalState?
    private var busy = false
    private var refreshPending = false
    private var forceQuestion = false
    private var pendingAnswer: ReviewRecord.Answer?
    private var version: UInt64 = 0
    private var preload: Task<Void, Never>?
    private var operation: Task<Void, Never>?
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
                nativeInput: NativeReviewInput? = nil, nativeGeneration: UInt64 = 0) {
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
    }
    public var pair: CardPagePair? { surface.pair }
    public var canFinish: Bool { !busy && phase != .ended }
    public var canReveal: Bool { phase == .question && !busy && !paused }
    public var canRate: Bool { phase == .answer && !busy && !paused }
    public var canUndo: Bool { if case .available = undoStatus { return !busy && !paused && phase != .ended }; return false }
    public var canRedo: Bool { if case .available = redoStatus { return !busy && !paused && phase != .ended }; return false }
    public var progress: Double {
        let answered = Double(record.answers.count)
        let remaining = Double(counts.new) + Double(counts.learning) + Double(counts.review)
        return answered + remaining == 0 ? 0 : answered / (answered + remaining)
    }
    public var timerText: String {
        let cap = item?.homePreset.maximumAnswerSeconds ?? 60
        let seconds = frozenDisplaySeconds ?? timer.milliseconds(at: uptime(), capSeconds: cap) / 1_000
        return String(format: "%02llu:%02llu", seconds / 60, seconds % 60)
    }
    public var remainingDescription: String { "\(counts.new) new, \(counts.learning) learning, \(counts.review) to review" }

    public func start() async {
        guard sessionID == nil, !busy, phase == .loading else { return }
        busy = true
        let task = Task { await startOwned() }
        operation = task
        await task.value
    }
    private func startOwned() async {
        do {
            switch try await client.start(deckID: record.deckID) {
            case .refused(.fsrsPolicyPending): onPolicyRefusal?(); phase = .error; notice = "Enable FSRS before studying."
            case .refused(.presetUnsupported): phase = .error; notice = "This preset needs a newer Meguri"
            case .started(let id, let generation, let head, _):
                sessionID = id
                try nativeInput?.bindSession(coordinator: nativeCoordinator, session: id, generation: generation, deck: record.deckID)
                activeTime.start(at: uptime())
                if paused { activeTime.pause(at: uptime()) }
                try await surface.load()
                try await apply(head, question: true)
                try await persist()
            }
        } catch { fail(error) }
        completeOperation()
    }

    public func reveal() {
        guard phase == .question, !busy, !paused else { return }
        busy = true; phase = .transition
        let inputTime = uptime()
        operation = Task {
            do {
                try await surface.reveal(inputTime: inputTime)
                sideIsAnswer = true
                if item?.homePreset.stopTimerOnAnswer == true {
                    frozenDisplaySeconds = timer.milliseconds(at: uptime(), capSeconds: item?.homePreset.maximumAnswerSeconds ?? 60) / 1_000
                }
                phase = .answer
            } catch { fail(error) }
            completeOperation()
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
            use = try nativeInput?.admit(coordinator: nativeCoordinator, session: sessionID, generation: nativeGeneration,
                deck: record.deckID, item: item, rating: rating, instant: Int64(instant.timeIntervalSince1970 * 1000),
                eligible: nativeInputEligibility() && terminalState == .active && sideIsAnswer && !refreshPending &&
                    presentedKey == item.renderKey)
        } catch { busy = false; nativeInput?.failed(error); fail(error); return }
        timer.pause(at: inputTime)
        let elapsed = use?.elapsed ?? timer.milliseconds(at: inputTime, capSeconds: item.homePreset.maximumAnswerSeconds)
        let answer = ReviewRecord.Answer(cardID: item.cardId, rating: Self.name(rating), kind: String(describing: item.kind),
                                         answeredAt: instant, elapsedMilliseconds: elapsed)
        pendingAnswer = answer
        operation = Task {
            do {
                let outcome = try await client.answer(sessionID: sessionID, item: item, rating: rating,
                    at: Int64(instant.timeIntervalSince1970 * 1_000), elapsed: elapsed)
                switch outcome {
                case .accepted(let diff, let leech, let suspended, _):
                    if let use { nativeInput?.completed(use, outcome: "accepted") }
                    // A failure may arrive while answer() returns. Never resurrect it.
                    if let pendingAnswer, pendingAnswer.id == answer.id {
                        record.answers.append(pendingAnswer); record.undone.removeAll()
                        self.pendingAnswer = nil
                    }
                    if leech { notice = suspended ? "Card was a leech. It has been suspended." : "Card was a leech." }
                    if suspended { record.suspended.insert(item.cardId) }
                    enqueuePersistence()
                    let head = refreshPending ? try await client.head(sessionID: sessionID) : diff.head
                    refreshPending = false
                    try await apply(head, question: true, inputTime: inputTime)
                    try await persist()
                case .stale(let head, let showNotice):
                    if let use { nativeInput?.completed(use, outcome: "stale") }
                    pendingAnswer = nil
                    if showNotice { notice = "Card changed, please answer again" }
                    try await apply(head, question: true)
                case .notAtHead(let head):
                    if let use { nativeInput?.completed(use, outcome: "not-at-head") }
                    pendingAnswer = nil
                    try await apply(head, question: true)
                }
            } catch {
                if let use { nativeInput?.completed(use, outcome: "failed") }
                pendingAnswer = nil
                notice = String(describing: error)
                // An uncertain answer is never retried; ask the authoritative head.
                do { try await apply(client.head(sessionID: sessionID), question: true) }
                catch { fail(error) }
            }
            completeOperation()
        }
    }

    public func restore(redo: Bool = false) {
        guard (redo ? canRedo : canUndo), let sessionID else { return }
        busy = true; phase = .transition
        timer.pause(at: uptime())
        operation = Task {
            do {
                let head = try await client.restore(sessionID: sessionID, redo: redo)
                if redo {
                    if var answer = record.undone.popLast() { answer.durable = false; record.answers.append(answer) }
                } else if let answer = record.answers.popLast() { record.undone.append(answer) }
                try await apply(head, question: true)
                try await persist()
            } catch {
                notice = String(describing: error)
                do { try await apply(client.head(sessionID: sessionID), question: true) }
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
        requestRefresh(question: true)
        enqueuePersistence()
    }

    public func requestRefresh(question: Bool = false) {
        guard sessionID != nil, phase != .ended else { return }
        refreshPending = true; forceQuestion = forceQuestion || question
        if !paused { phase = .transition }
        if !busy && !paused { refresh() }
    }
    public func retry() { requestRefresh(question: true) }
    private func refresh() {
        guard let sessionID, !busy, !paused else { return }
        busy = true; phase = .transition
        let question = forceQuestion; forceQuestion = false; refreshPending = false
        operation = Task {
            do { try await apply(client.head(sessionID: sessionID), question: question); try await persist() }
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

    private func apply(_ head: ReviewHead, question: Bool, inputTime: TimeInterval? = nil) async throws {
        summaryTask?.cancel(); summaryTask = nil
        counts = head.counts
        terminalState = head.terminalState
        guard let sessionID else { return }
        let previous = item
        item = head.items.first
        switch head.terminalState {
        case .active:
            guard let item else { throw BridgeError.internal("Active review head has no item") }
            if !paused { activeTime.resume(at: uptime()) }
            studyState = nil; waitingUntil = nil
            try await surface.render(sessionID: sessionID, items: head.items)
            guard self.sessionID == sessionID else { throw BridgeError.unavailable(reason: "Session ended during rendering") }
            let needsPresentation = question || previous?.cardId != item.cardId || presentedKey != item.renderKey
            if needsPresentation {
                preload?.cancel(); await preload?.value; preload = nil
                let keepAnswer = !question && sideIsAnswer && previous?.cardId == item.cardId
                try await surface.present(item: item, inputTime: inputTime ?? uptime())
                guard self.sessionID == sessionID else { throw BridgeError.unavailable(reason: "Session ended during presentation") }
                presentedKey = item.renderKey
                sideIsAnswer = false
                timer.start(at: uptime())
                if paused { timer.pause(at: uptime()) }
                frozenDisplaySeconds = nil
                if keepAnswer { try await surface.reveal(inputTime: uptime()); sideIsAnswer = true }
            }
            phase = sideIsAnswer ? .answer : .question
            if let next = head.items.dropFirst().first {
                preload?.cancel(); await preload?.value
                preload = Task { try? await surface.preload(item: next) }
            }
        case .waitingUntil, .finished:
            preload?.cancel(); await preload?.value; preload = nil
            timer.pause(at: uptime()); activeTime.pause(at: uptime()); surface.pair?.pauseAudio()
            studyState = nil
            if case .waitingUntil(let instant) = head.terminalState {
                waitingUntil = Date(timeIntervalSince1970: Double(instant)); phase = .waiting
            } else { waitingUntil = nil; phase = .finished }
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
        timer.pause(at: uptime()); activeTime.pause(at: uptime())
        preload?.cancel(); await preload?.value; preload = nil
        await surface.suspend()
        try await persist()
    }
    public func resume() async throws {
        guard let sessionID, phase != .ended, paused else { return }
        let revision = pauseRevision
        await operation?.value
        guard revision == pauseRevision else { return }
        busy = true
        do {
            await surface.resume()
            try await apply(client.head(sessionID: sessionID), question: forceQuestion)
            forceQuestion = false; refreshPending = false
            guard revision == pauseRevision else {
                await surface.suspend()
                busy = false
                return
            }
            paused = false
            (surface as? ReviewPages)?.inputActive = true
            if item != nil, phase == .question || phase == .answer {
                timer.resume(at: uptime()); activeTime.resume(at: uptime())
            }
            busy = false
            retryArrivedMedia()
        } catch { paused = true; busy = false; fail(error); throw error }
    }
    public func mediaUnavailable() {
        mediaArrivals = ReviewMediaArrivals()
        beginPause()
        notice = "Collection media is unavailable. Return to Home and try again."
        phase = .error
    }

    /// Idempotent across Close, sidebar navigation, window close and app quit.
    @discardableResult public func end() async -> Bool {
        if let endTask { return await endTask.value }
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
        summaryTask?.cancel()
        paused = true; deadlineTimer?.invalidate(); preload?.cancel()
        sessionID = nil; phase = .ended; surface.close()
        mediaArrivals = ReviewMediaArrivals()
    }
    private func persist() async throws {
        record.activeMilliseconds = activeTime.milliseconds(at: uptime(), capSeconds: UInt32.max)
        if version == 0 { try await store.begin(record) }
        version &+= 1
        let snapshot = record; let revision = version
        try await store.save(snapshot, version: revision)
        onRecordChanged?(snapshot)
    }
    private func enqueuePersistence() {
        version &+= 1
        let snapshot = record; let revision = version; let store = store
        Task { [weak self] in
            do { try await store.save(snapshot, version: revision) }
            catch { self?.notice = "Couldn’t save the session record. \(error)" }
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
