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

public nonisolated struct CardAudioSample: Sendable {
    public let position: TimeInterval
    public let observedAt: TimeInterval
    public init(position: TimeInterval, observedAt: TimeInterval) {
        self.position = position; self.observedAt = observedAt
    }
}

public protocol CardAudioClip: AnyObject {
    var duration: TimeInterval? { get }
    var seekable: Bool { get }
    var onEnd: ((Bool) -> Void)? { get set }
    func play() async throws
    func pause() async
    func stop() async
    func sample() async -> CardAudioSample
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
    /// Narrow native timing observation seam. No observer means no polling.
    /// Advancement comes from the backend timeline, not the play request.
    enum PlaybackObservation {
        case advanced(position: TimeInterval, duration: TimeInterval?, observedAt: TimeInterval)
        case completed(success: Bool, observedAt: TimeInterval)
    }
    var onPlaybackObservation: ((Entry, PlaybackObservation) -> Void)?
    private var playbackObservation: Task<Void, Never>?
    public private(set) var current: Entry?
    public private(set) var paused = false
    private let resolver: MediaPathResolver
    private let backend: any CardAudioBackend
    private let diagnostics: CardDiagnostics
    private let transport: AudioTransport
    // The task owns transport resources only; it never retains this controller.
    private var drainTask: Task<Void, Never>?
    private var callbackGeneration: UInt64 = 0
    private var queue: [Entry] = []
    private var clip: (any CardAudioClip)?
    private var loading: Task<Void, Never>?
    // Prepare at most one waiting clip; question and answer preparation remain separate.
    // It belongs to the current waiting queue and never starts itself early.
    private var nextLoad: Task<Void, Never>?
    private var nextClip: (Entry, any CardAudioClip)?
    private var nextSerial: UInt64 = 0
    private var preloads: [CardUpdate: Task<Void, Never>] = [:]
    private var prepared: [CardUpdate: (Entry, any CardAudioClip)] = [:]
    private var preloadSerials: [CardUpdate: UInt64] = [:]
    private var preparationOrder: [CardUpdate] = []
    private var generation: UInt64 = 0
    private var preloadGeneration: UInt64 = 0
    private var seekGeneration: UInt64 = 0
    private var presentation: CardUpdate?
    private var lastTransition: CardUpdate?
    private var closed = false
    private var sessionEvents: Task<Void, Never>?

    public init(resolver: MediaPathResolver, backend: any CardAudioBackend = AVQueuePlayback(),
                session: any CardAudioSession = AudioSessionCoordinator.shared, diagnostics: CardDiagnostics = CardDiagnostics()) {
        self.resolver = resolver; self.backend = backend; self.diagnostics = diagnostics
        transport = AudioTransport(session: session)
        transport.onStopped = { [weak self] in self?.advance() }
        transport.onStarted = { [weak self] clip, entry, serial in
            guard let self, self.generation == serial, self.clip === clip, !self.paused, !self.closed else { return }
            self.observePlayback(clip, entry: entry, serial: serial)
            self.prepareNext()
        }
        transport.onFailure = { [weak self] serial, sessionFailure in
            guard let self, self.generation == serial, let entry = self.current, !self.closed else { return }
            if sessionFailure {
                self.diagnostics.record("audioSession", update: self.diagnosticUpdate(entry), name: entry.name, index: entry.index)
            }
            self.complete(serial, success: false, reportFailure: !sessionFailure)
        }
        transport.onSeekEnd = { [weak self] serial in self?.complete(serial, success: true) }
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
        cancelNext()
        queue.removeAll()
        if policy.interrupt || clip == nil { replace(keepPreload: true) }
    }
    public func present(payload: CardPayload, update: CardUpdate) {
        guard !closed, presentation != update else { return }
        beginTransition(to: update, policy: payload.audioPolicy)
        presentation = update
        queue = payload.audioPolicy.autoplay ? entries(payload, update: update, side: update.side) : []
        advance()
        if clip != nil { prepareNext() }
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
                    let clip = try await Self.prepareFile(file, backend: backend)
                    guard let self, !Task.isCancelled, self.preloadSerials[update] == serial, !self.closed else { await clip.stop(); return }
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
        if let clip = prepared.removeValue(forKey: update)?.1 { closePreparedClip(clip) }
        preparationOrder.removeAll { $0 == update }
    }

    private func advance() {
        guard !closed, current == nil, transport.isEmpty else { return }
        guard !queue.isEmpty else { transport.releaseIfIdle(); return }
        // A lookahead already resolving/decoding this exact queue completes
        // advance itself; never cancel it and reopen the same missing file.
        guard nextLoad == nil else { return }
        let entry = queue.removeFirst()
        current = entry
        generation += 1
        let serial = generation
        if let ready = nextClip, ready.0 == entry {
            nextClip = nil
            start(ready.1, entry: entry, serial: serial)
            return
        }
        if let ready = takePreparation(for: entry) {
            start(ready, entry: entry, serial: serial)
            return
        }
        cancelPreparation(entry.update)
        let resolver = self.resolver, backend = self.backend
        loading = Task { [weak self] in
            do {
                try Task.checkCancellation()
                let file = try await resolver.openAudioName(entry.name)
                let clip = try await Self.prepareFile(file, backend: backend)
                guard let self, !Task.isCancelled, self.generation == serial, !self.closed else { await clip.stop(); return }
                self.start(clip, entry: entry, serial: serial)
            } catch {
                guard let self, !Task.isCancelled, self.generation == serial else { return }
                self.diagnose(error, entry: entry)
                self.complete(serial, success: false, reportFailure: false)
            }
        }
    }

    private func takePreparation(for entry: Entry) -> (any CardAudioClip)? {
        guard let ready = prepared[entry.update], ready.0 == entry else { return nil }
        prepared[entry.update] = nil
        preloadSerials[entry.update] = nil
        preloads[entry.update] = nil
        preparationOrder.removeAll { $0 == entry.update }
        return ready.1
    }

    private func cancelNext() {
        nextSerial += 1
        nextLoad?.cancel(); nextLoad = nil
        if let clip = nextClip?.1 { closePreparedClip(clip) }
        nextClip = nil
    }

    private func prepareNext() {
        guard !closed, nextLoad == nil, nextClip == nil, let first = queue.first else { return }
        if let ready = takePreparation(for: first) {
            nextClip = (first, ready)
            return
        }
        nextSerial += 1
        let serial = nextSerial, entries = queue
        let resolver = self.resolver, backend = self.backend
        nextLoad = Task { [weak self] in
            for entry in entries {
                do {
                    try Task.checkCancellation()
                    let file = try await resolver.openAudioName(entry.name)
                    let clip = try await Self.prepareFile(file, backend: backend)
                    guard let self, !Task.isCancelled, self.nextSerial == serial, !self.closed,
                          self.queue.first == entry else { await clip.stop(); return }
                    self.nextClip = (entry, clip)
                    self.nextLoad = nil
                    self.advance()
                    return
                } catch {
                    guard let self, !Task.isCancelled, self.nextSerial == serial, !self.closed,
                          self.queue.first == entry else { return }
                    self.diagnose(error, entry: entry)
                    self.queue.removeFirst()
                }
            }
            guard let self, self.nextSerial == serial, !Task.isCancelled else { return }
            self.nextLoad = nil
            self.advance()
        }
    }

    private func start(_ clip: any CardAudioClip, entry: Entry, serial: UInt64) {
        self.clip = clip
        armCompletion(clip, serial: serial)
        transport.install(clip, entry: entry, serial: serial, paused: paused)
        drain()
    }
    private func armCompletion(_ clip: any CardAudioClip, serial: UInt64) {
        let callback = callbackGeneration
        clip.onEnd = { [weak self, weak clip] success in
            guard let self, let clip, self.clip === clip, self.callbackGeneration == callback, !self.paused, !self.closed else { return }
            self.complete(serial, success: success)
        }
    }
    private func drain() {
        if let task = transport.run() { drainTask = task }
    }
    /// Await physical effects when an async caller needs their completion.
    /// Public transport intents themselves remain synchronous and coalesced.
    func transportSettled() async { await drainTask?.value }
    private static func prepareFile(_ file: MediaFile, backend: any CardAudioBackend) async throws -> any CardAudioClip {
        do {
            try Task.checkCancellation()
            return try await backend.prepare(file)
        } catch {
            await file.close()
            throw error
        }
    }
    private func closePreparedClip(_ clip: any CardAudioClip) {
        clip.onEnd = nil
        // A prepared clip was never activated/played. Its one cleanup task
        // owns only that bounded preparation resource, never this controller.
        Task { await clip.stop() }
    }
    private func observePlayback(_ clip: any CardAudioClip, entry: Entry, serial: UInt64) {
        playbackObservation?.cancel()
        guard onPlaybackObservation != nil else { return }
        playbackObservation = Task { [weak self, weak clip] in
            var previous: TimeInterval = 0
            while !Task.isCancelled {
                guard let clip else { return }
                guard self?.generation == serial, self?.clip === clip, self?.paused == false else { return }
                let sample = await clip.sample()
                guard !Task.isCancelled, self?.generation == serial, self?.clip === clip,
                      self?.paused == false, self?.closed == false else { return }
                let position = sample.position
                if position.isFinite, position > previous {
                    previous = position
                    self?.onPlaybackObservation?(entry, .advanced(position: position, duration: clip.duration,
                                                                  observedAt: sample.observedAt))
                }
                do { try await Task.sleep(for: .milliseconds(5)) } catch { return }
            }
        }
    }

    private func complete(_ serial: UInt64, success: Bool, reportFailure: Bool = true) {
        guard generation == serial, let entry = current else { return }
        playbackObservation?.cancel(); playbackObservation = nil
        if clip != nil { onPlaybackObservation?(entry, .completed(success: success, observedAt: ProcessInfo.processInfo.systemUptime)) }
        generation += 1; seekGeneration += 1; callbackGeneration += 1
        if !success && reportFailure { diagnostics.record("audioDecode", update: diagnosticUpdate(entry), name: entry.name, index: entry.index) }
        clip?.onEnd = nil; clip = nil
        current = nil; paused = paused && !queue.isEmpty
        // Continue with the same lease only after the physical stop barrier.
        transport.stop(keepSession: !paused && !queue.isEmpty)
        drain()
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
        paused = true; callbackGeneration += 1
        clip?.onEnd = nil
        playbackObservation?.cancel(); playbackObservation = nil
        transport.pause()
        drain()
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
        paused = false
        if let clip {
            armCompletion(clip, serial: generation)
            transport.resume()
            drain()
        }
    }
    public func seek(by seconds: TimeInterval) async { await requestSeek(by: seconds)?.value }
    @discardableResult func requestSeek(by seconds: TimeInterval) -> Task<Void, Never>? {
        guard let clip, clip.seekable, seconds.isFinite,
              let duration = clip.duration, duration.isFinite, duration >= 0 else { return nil }
        seekGeneration += 1
        transport.seek(by: seconds, revision: seekGeneration)
        drain()
        return drainTask
    }

    private func replace(keepPreload: Bool = false) {
        playbackObservation?.cancel(); playbackObservation = nil
        cancelNext()
        if !keepPreload { cancelPreload() }
        generation += 1; seekGeneration += 1; callbackGeneration += 1
        loading?.cancel(); loading = nil
        clip?.onEnd = nil; clip = nil
        current = nil; paused = false; queue.removeAll()
        transport.stop(keepSession: false)
        drain()
    }
    private func cancelPreload() {
        for update in preparationOrder { cancelPreparation(update) }
    }
    public func stop() { replace(); cancelPreload() }
    public func close() {
        guard !closed else { return }
        closed = true; stop()
        onPlaybackObservation = nil
        sessionEvents?.cancel(); sessionEvents = nil
    }
}

/// One physical clip, one drain, one latest seek. The drain task retains this
/// resource state. Its callbacks use weak references to the controller.
private final class AudioTransport {
    private enum Intent { case playing, paused, stopped }
    private final class OwnedClip {
        let clip: any CardAudioClip
        let entry: CardAudioController.Entry
        let serial: UInt64
        var intent: Intent
        var playing = false
        var pauseRequired = false
        var keepSession = false
        var seek: (seconds: TimeInterval, revision: UInt64)?
        var seekRevision: UInt64 = 0
        init(_ clip: any CardAudioClip, entry: CardAudioController.Entry, serial: UInt64, paused: Bool) {
            self.clip = clip; self.entry = entry; self.serial = serial
            intent = paused ? .paused : .playing
        }
    }
    private let session: any CardAudioSession
    private var lease: UUID?
    private var owned: OwnedClip?
    private var running = false
    var isEmpty: Bool { owned == nil }
    var onStopped: (() -> Void)?
    var onStarted: (((any CardAudioClip), CardAudioController.Entry, UInt64) -> Void)?
    var onFailure: ((UInt64, Bool) -> Void)?
    var onSeekEnd: ((UInt64) -> Void)?
    init(session: any CardAudioSession) { self.session = session }

    func install(_ clip: any CardAudioClip, entry: CardAudioController.Entry, serial: UInt64, paused: Bool) {
        precondition(owned == nil)
        owned = OwnedClip(clip, entry: entry, serial: serial, paused: paused)
    }
    func pause() {
        guard let owned, owned.intent != .stopped else { return }
        owned.intent = .paused
        // Even a following resume must cross this barrier after a held play.
        owned.pauseRequired = true
    }
    func resume() {
        guard let owned, owned.intent != .stopped else { return }
        owned.intent = .playing
    }
    func seek(by seconds: TimeInterval, revision: UInt64) {
        guard let owned, owned.intent != .stopped else { return }
        owned.seekRevision = revision
        owned.seek = (seconds, revision)
    }
    func stop(keepSession: Bool) {
        guard let owned else { if !keepSession { releaseIfIdle() }; return }
        owned.intent = .stopped
        owned.keepSession = keepSession
        owned.seek = nil
        owned.clip.onEnd = nil
    }
    func releaseIfIdle() {
        guard owned == nil else { return }
        release()
    }
    private func release() {
        if let lease { self.lease = nil; session.release(lease) }
    }
    func run() -> Task<Void, Never>? {
        guard !running, owned != nil else { return nil }
        running = true
        // Only the controller stores the task, so close/deinit can release its
        // controller while the task finishes a held native stop and file close.
        return Task { await self.drain() }
    }
    private func drain() async {
        defer { running = false }
        while let item = owned {
            if item.intent == .stopped {
                await item.clip.stop()
                owned = nil
                if !item.keepSession { release() }
                onStopped?()
                // onStopped may synchronously install the next prepared clip.
                continue
            }
            if item.pauseRequired || (item.intent == .paused && item.playing) {
                item.pauseRequired = false
                await item.clip.pause()
                item.playing = false
                release()
                continue
            }
            // A paused activation that never played still relinquishes its lease.
            if item.intent == .paused { release() }
            if let seek = item.seek {
                item.seek = nil
                let sample = await item.clip.sample()
                guard item.intent != .stopped else { continue }
                if item.pauseRequired {
                    if item.seekRevision == seek.revision { item.seek = seek }
                    continue
                }
                guard item.seekRevision == seek.revision, sample.position.isFinite,
                      let duration = item.clip.duration, duration.isFinite, duration >= 0 else { continue }
                let target = min(duration, max(0, sample.position + seek.seconds))
                do { try await item.clip.seek(to: target) } catch { continue }
                guard item.intent != .stopped, item.seekRevision == seek.revision else { continue }
                if target == duration { onSeekEnd?(item.serial) }
                continue
            }
            guard item.intent == .playing, !item.playing else { return }
            if lease == nil {
                let owner = UUID()
                lease = owner
                do { try await session.activate(owner) }
                catch {
                    release()
                    if item.intent != .stopped { onFailure?(item.serial, true) }
                    continue
                }
                // Re-evaluate pause/stop/seek intent after activation.
                continue
            }
            do {
                try await item.clip.play()
                item.playing = true
                if item.intent == .playing, !item.pauseRequired {
                    onStarted?(item.clip, item.entry, item.serial)
                }
            } catch {
                if item.intent != .stopped { onFailure?(item.serial, false) }
            }
        }
    }
}
