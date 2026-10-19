import CoreBridge
import Foundation

/// The caller supplies the home-deck and collection playback policy.
public nonisolated struct CardAudioPolicy: Sendable, Equatable {
    public var autoplay: Bool
    public var interrupt: Bool
    public var replayQuestion: Bool
    public init(autoplay: Bool = true, interrupt: Bool = true, replayQuestion: Bool = true) {
        self.autoplay = autoplay; self.interrupt = interrupt; self.replayQuestion = replayQuestion
    }
}

public protocol CardAudioClip: AnyObject {
    var time: TimeInterval { get }
    var duration: TimeInterval? { get }
    var seekable: Bool { get }
    var onEnd: ((Bool) -> Void)? { get set }
    func play() throws
    func pause()
    func stop()
    func seek(to time: TimeInterval) async throws
}
public protocol CardAudioBackend: AnyObject {
    func prepare(_ file: MediaFile) async throws -> any CardAudioClip
}
public protocol CardAudioSession: AnyObject {
    func activate(_ owner: UUID) async throws
    func release(_ owner: UUID)
}

public final class CardAudioController {
    public nonisolated struct Entry: Sendable, Equatable {
        public let update: CardUpdate
        public let side: CardRenderSide
        public let index: Int
        public let name: String
    }
    public private(set) var current: Entry?
    public private(set) var paused = false
    private let resolver: MediaPathResolver
    private let backend: any CardAudioBackend
    private let session: any CardAudioSession
    private let diagnostics: CardDiagnostics
    private var sessionOwner: UUID?
    private var activation: Task<Void, Never>?
    private var queue: [Entry] = []
    private var clip: (any CardAudioClip)?
    private var loading: Task<Void, Never>?
    private var preloads: [CardUpdate: Task<Void, Never>] = [:]
    private var prepared: [CardUpdate: (Entry, any CardAudioClip)] = [:]
    private var preloadSerials: [CardUpdate: UInt64] = [:]
    private var preparationOrder: [CardUpdate] = []
    private var generation: UInt64 = 0
    private var preloadGeneration: UInt64 = 0
    private var seekGeneration: UInt64 = 0
    private var seeking: Task<Void, Never>?
    private var presentation: CardUpdate?
    private var lastTransition: CardUpdate?
    private var closed = false
    private var sessionEvents: Task<Void, Never>?

    public init(resolver: MediaPathResolver, backend: any CardAudioBackend = AVQueuePlayback(),
                session: any CardAudioSession = AudioSessionCoordinator.shared, diagnostics: CardDiagnostics = CardDiagnostics()) {
        self.resolver = resolver; self.backend = backend; self.session = session; self.diagnostics = diagnostics
        if let coordinator = session as? AudioSessionCoordinator {
            let events = coordinator.events()
            sessionEvents = Task { [weak self] in
                for await event in events {
                    guard let self else { return }
                    self.sessionEvent(event)
                }
            }
        }
    }
    isolated deinit { close() }

    private func entries(_ payload: CardPayload, update: CardUpdate, side: CardRenderSide) -> [Entry] {
        payload.audio(on: side).enumerated().compactMap { index, audio in
            guard case .sound(let name) = audio else { return nil }
            return Entry(update: update, side: side, index: index, name: name)
        }
    }

    /// Departure can precede a slow prepare. A presentation of the same
    /// transition never resets its surviving old clip twice.
    public func beginTransition(to update: CardUpdate, policy: CardAudioPolicy) {
        guard !closed, lastTransition != update else { return }
        lastTransition = update
        queue.removeAll()
        if policy.interrupt || clip == nil { replace(keepPreload: true) }
    }
    public func present(payload: CardPayload, update: CardUpdate) {
        guard !closed, presentation != update else { return }
        beginTransition(to: update, policy: payload.audioPolicy)
        presentation = update
        queue = payload.audioPolicy.autoplay ? entries(payload, update: update, side: update.side) : []
        advance()
    }

    public func replay(payload: CardPayload, update: CardUpdate) {
        guard !closed else { return }
        replace()
        if update.side == .answer, payload.audioPolicy.replayQuestion {
            queue = entries(payload, update: update, side: .question) + entries(payload, update: update, side: .answer)
        } else { queue = entries(payload, update: update, side: update.side) }
        advance()
    }

    public func playEntry(payload: CardPayload, update: CardUpdate, side: CardRenderSide, index: Int) {
        guard !closed, payload.audio(on: side).indices.contains(index) else { return }
        replace()
        if case .sound(let name) = payload.audio(on: side)[index] { queue = [Entry(update: update, side: side, index: index, name: name)] }
        advance()
    }

    @discardableResult public func prepare(payload: CardPayload, update: CardUpdate) -> Task<Void, Never>? {
        cancelPreparation(update)
        guard !closed else { return nil }
        while preparationOrder.count >= 2 { cancelPreparation(preparationOrder[0]) }
        preparationOrder.append(update)
        preloadGeneration += 1
        let serial = preloadGeneration
        preloadSerials[update] = serial
        let entries = self.entries(payload, update: update, side: update.side)
        let resolver = self.resolver, backend = self.backend
        let task = Task { [weak self] in
            for entry in entries {
                do {
                    try Task.checkCancellation()
                    let file = try await resolver.openAudioName(entry.name)
                    try Task.checkCancellation()
                    let clip = try await backend.prepare(file)
                    guard let self, !Task.isCancelled, self.preloadSerials[update] == serial, !self.closed else { clip.stop(); return }
                    self.prepared[update] = (entry, clip)
                    return
                } catch {
                    guard !Task.isCancelled else { return }
                    self?.diagnose(error, entry: entry)
                }
            }
        }
        preloads[update] = task
        return task
    }
    public func cancelPreparation(_ update: CardUpdate) {
        preloadSerials[update] = nil
        preloads.removeValue(forKey: update)?.cancel()
        prepared.removeValue(forKey: update)?.1.stop()
        preparationOrder.removeAll { $0 == update }
    }

    private func advance() {
        guard !closed, current == nil, !queue.isEmpty else { return }
        let entry = queue.removeFirst()
        current = entry
        generation += 1
        let serial = generation
        if let ready = prepared[entry.update], ready.0 == entry {
            prepared[entry.update] = nil
            preloadSerials[entry.update] = nil
            preloads[entry.update] = nil
            preparationOrder.removeAll { $0 == entry.update }
            start(ready.1, entry: entry, serial: serial)
            return
        }
        cancelPreparation(entry.update)
        let resolver = self.resolver, backend = self.backend
        loading = Task { [weak self] in
            do {
                try Task.checkCancellation()
                let file = try await resolver.openAudioName(entry.name)
                try Task.checkCancellation()
                let clip = try await backend.prepare(file)
                guard let self, !Task.isCancelled, self.generation == serial, !self.closed else { clip.stop(); return }
                self.start(clip, entry: entry, serial: serial)
            } catch {
                guard let self, !Task.isCancelled, self.generation == serial else { return }
                self.diagnose(error, entry: entry)
                self.complete(serial, success: false, reportFailure: false)
            }
        }
    }

    private func start(_ clip: any CardAudioClip, entry: Entry, serial: UInt64) {
        self.clip = clip
        clip.onEnd = { [weak self] success in self?.complete(serial, success: success) }
        if paused { clip.pause(); return }
        schedulePlay(clip, entry: entry, serial: serial)
    }
    private func schedulePlay(_ clip: any CardAudioClip, entry: Entry, serial: UInt64) {
        releaseSession()
        let lease = UUID(), session = self.session
        sessionOwner = lease
        activation = Task { [weak self] in
            do { try Task.checkCancellation(); try await session.activate(lease) }
            catch {
                guard let self, self.sessionOwner == lease, self.generation == serial, !Task.isCancelled else { session.release(lease); return }
                self.diagnostics.record("audioSession", update: self.diagnosticUpdate(entry), name: entry.name, index: entry.index)
                self.complete(serial, success: false, reportFailure: false)
                return
            }
            guard let self, self.sessionOwner == lease, self.generation == serial, !self.paused, !self.closed, !Task.isCancelled else { session.release(lease); return }
            do { try clip.play() }
            catch { self.complete(serial, success: false) }
        }
    }
    private func releaseSession() {
        activation?.cancel(); activation = nil
        if let sessionOwner { session.release(sessionOwner) }
        sessionOwner = nil
    }
    private func complete(_ serial: UInt64, success: Bool, reportFailure: Bool = true) {
        guard generation == serial, let entry = current else { return }
        generation += 1; seekGeneration += 1
        if !success && reportFailure { diagnostics.record("audioDecode", update: diagnosticUpdate(entry), name: entry.name, index: entry.index) }
        clip?.onEnd = nil; clip?.stop(); clip = nil
        current = nil; paused = paused && !queue.isEmpty
        releaseSession()
        advance()
    }
    private func diagnose(_ error: any Error, entry: Entry) {
        let kind = error is MediaPathResolver.Failure ? "missingAudio" : error is AVQueuePlayback.Failure ? "audioUnsupported" : "audioDecode"
        diagnostics.record(kind, update: diagnosticUpdate(entry), name: entry.name, index: entry.index)
    }
    private func diagnosticUpdate(_ entry: Entry) -> CardUpdate {
        CardUpdate(card: entry.update.card, generation: entry.update.generation, side: entry.side)
    }

    public func pause() {
        guard current != nil else { return }
        paused = true
        clip?.pause()
        releaseSession()
    }
    public func sessionEvent(_ event: AudioSessionCoordinator.Event) {
        switch event {
        case .stop: stop()
        case .pause: pause()
        case .available: break // Explicit user play is required.
        }
    }
    public func resume() {
        guard paused, current != nil else { return }
        guard let clip else { paused = false; return }
        paused = false
        if let current { schedulePlay(clip, entry: current, serial: generation) }
    }
    public func seek(by seconds: TimeInterval) async {
        guard let clip, clip.seekable, seconds.isFinite, clip.time.isFinite,
              let duration = clip.duration, duration.isFinite, duration >= 0 else { return }
        seekGeneration += 1
        let seek = seekGeneration, serial = generation
        let target = min(duration, max(0, clip.time + seconds))
        let previous = seeking
        let operation = Task { [weak self] in
            // Serialize actual backend seeks, dropping superseded waiting
            // requests; an older async seek cannot land after the newest one.
            await previous?.value
            guard let self, seek == self.seekGeneration, serial == self.generation, self.clip === clip else { return }
            do { try await clip.seek(to: target) } catch { return }
            guard seek == self.seekGeneration, serial == self.generation, self.clip === clip else { return }
            if self.paused { clip.pause() }
            if target == duration { self.complete(serial, success: true) }
        }
        seeking = operation
        await operation.value
    }

    private func replace(keepPreload: Bool = false) {
        if !keepPreload { cancelPreload() }
        generation += 1; seekGeneration += 1
        seeking?.cancel(); seeking = nil
        loading?.cancel(); loading = nil
        clip?.onEnd = nil; clip?.stop(); clip = nil
        current = nil; paused = false; queue.removeAll()
        releaseSession()
    }
    private func cancelPreload() {
        for update in preparationOrder { cancelPreparation(update) }
    }
    public func stop() { replace(); cancelPreload() }
    public func close() {
        guard !closed else { return }
        closed = true; stop()
        sessionEvents?.cancel(); sessionEvents = nil
    }
}
