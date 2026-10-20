@testable import CardRendering
import CoreBridge
import Foundation
import MediaPlayer
import Testing
#if !os(macOS)
import AVFoundation
#endif

@Suite struct CardAudioControllerTests {
    /// Deliberately ignores task cancellation: native synchronous calls cannot
    /// be preempted, and their late returns must still cross cleanup barriers.
    final class Hold {
        private var blocked: CheckedContinuation<Void, Never>?
        private var observer: CheckedContinuation<Void, Never>?
        private var entered = false
        func wait() async {
            await withCheckedContinuation {
                blocked = $0; entered = true
                observer?.resume(); observer = nil
            }
        }
        func reached() async {
            if !entered { await withCheckedContinuation { observer = $0 } }
        }
        func release() { blocked?.resume(); blocked = nil }
    }
    final class Session: CardAudioSession {
        var owners: Set<UUID> = []
        var activations = 0
        var hold = false
        var held: CheckedContinuation<Void, Never>?
        var onHold: (() -> Void)?
        var onRelease: (() -> Void)?
        var fail = false
        enum Failure: Error { case activation }
        func activate(_ owner: UUID) async throws {
            let fail = self.fail
            owners.insert(owner); activations += 1
            if hold { await withCheckedContinuation { held = $0; onHold?() } }
            if fail { throw Failure.activation }
        }
        func release(_ owner: UUID) {
            if owners.remove(owner) != nil { onRelease?() }
        }
    }
    final class Clip: CardAudioClip {
        let name: String
        let file: MediaFile
        weak var backend: Backend?
        var time: TimeInterval = 0
        var duration: TimeInterval? = 4
        var seekable = true
        var onEnd: ((Bool) -> Void)?
        var stopped = false
        var onStop: (() -> Void)?
        var isPaused = false
        var playHold: Hold?
        var pauseHold: Hold?
        var stopHold: Hold?
        var seekHold: Hold?
        var sampleHold: Hold?
        var onSample: (() -> Void)?
        var plays = 0, pauses = 0, stops = 0, seeks = 0, samples = 0
        init(_ file: MediaFile, backend: Backend) { self.name = file.name; self.file = file; self.backend = backend }
        func play() async {
            plays += 1
            await playHold?.wait()
            isPaused = false; backend?.played(name)
        }
        func pause() async {
            pauses += 1
            await pauseHold?.wait()
            isPaused = true
        }
        func stop() async {
            guard !stopped else { return }
            stops += 1
            await stopHold?.wait()
            stopped = true
            await file.close()
            onStop?()
        }
        func seek(to time: TimeInterval) async throws {
            seeks += 1
            let hold = seekHold; seekHold = nil
            await hold?.wait()
            self.time = time
        }
        func sample() async -> CardAudioSample {
            samples += 1
            let hold = sampleHold; sampleHold = nil
            await hold?.wait()
            onSample?()
            return .init(position: time, observedAt: ProcessInfo.processInfo.systemUptime)
        }
    }
    final class Backend: CardAudioBackend {
        var clips: [Clip] = []
        var events: [String] = []
        var unread: [String] = []
        var waiter: CheckedContinuation<String, Never>?
        var hold = false
        var held: CheckedContinuation<Void, Never>?
        var onHold: (() -> Void)?
        var onPrepare: ((Clip) -> Void)?
        func prepare(_ file: MediaFile) async throws -> any CardAudioClip {
            let clip = Clip(file, backend: self)
            clips.append(clip)
            onPrepare?(clip)
            if hold { await withCheckedContinuation { held = $0; onHold?() } }
            return clip
        }
        func played(_ name: String) {
            events.append(name)
            if let waiter { self.waiter = nil; waiter.resume(returning: name) }
            else { unread.append(name) }
        }
        func nextPlay() async -> String {
            if !unread.isEmpty { return unread.removeFirst() }
            return await withCheckedContinuation { waiter = $0 }
        }
    }
    struct Rig {
        let fixture: MediaFixture
        let controller: CardAudioController
        let backend: Backend
        let session: Session
        let diagnostics: CardDiagnostics
    }
    func rig() async throws -> Rig {
        let fixture = MediaFixture()
        for name in ["q1.mp3", "q2.mp3", "q3.mp3", "a1.mp3", "x%20y.mp3"] { fixture.add("/" + name, Data([1]), "") }
        let backend = Backend(), session = Session(), diagnostics = CardDiagnostics()
        return Rig(fixture: fixture, controller: CardAudioController(resolver: try await fixture.resolved(), backend: backend, session: session, diagnostics: diagnostics),
                   backend: backend, session: session, diagnostics: diagnostics)
    }
    func payload(interrupt: Bool = false, autoplay: Bool = true, replayq: Bool = true) -> CardPayload {
        CardPayload(cardID: 1, question: "q", answer: "a", questionAudio: [.sound(name: "q1.mp3"), .sound(name: "q2.mp3")],
                    answerAudio: [.sound(name: "a1.mp3")], audioPolicy: .init(autoplay: autoplay, interrupt: interrupt, replayQuestion: replayq))
    }
    let question = CardUpdate(card: 1, generation: 1, side: .question)
    let answer = CardUpdate(card: 1, generation: 2, side: .answer)

    func descriptorIsClosed(_ clip: Clip) async -> Bool {
        do { _ = try await clip.file.read(offset: 0, count: 1); return false }
        catch { return true }
    }

    @Test(arguments: [true, false])
    func nativeDelegateQueuedBeforePauseCannotUseResumedCompletion(_ success: Bool) async {
        let completion = AVQueuePlayback.Completion()
        var oldResults: [Bool] = [], resumedResults: [Bool] = []
        completion.onEnd = { oldResults.append($0) }
        let old = completion.makeDelegate()
        // enqueue is the actual nonisolated AVAudioPlayerDelegate -> MainActor
        // hop, shared by finish and decode-error, with no player/hardware needed.
        let queued = old.enqueue(success: success)
        completion.onEnd = nil // Logical pause, before the physical barrier.
        completion.onEnd = { resumedResults.append($0) } // Immediate resume.
        await queued.value
        #expect(oldResults.isEmpty && resumedResults.isEmpty)

        completion.invalidate() // Physical pause still precedes the resumed play.
        let resumed = completion.makeDelegate()
        await resumed.enqueue(success: success).value
        await resumed.enqueue(success: !success).value
        #expect(oldResults.isEmpty && resumedResults == [success])
    }

    @Test(arguments: [true, false])
    func nativeDelegateQueuedBeforeSeekCannotCompleteNewPosition(_ success: Bool) async {
        let completion = AVQueuePlayback.Completion()
        var results: [Bool] = []
        completion.onEnd = { results.append($0) }
        let playing = completion.makeDelegate()
        let old = playing.enqueue(success: success)
        // NativeClip.seek installs a new immutable delegate on the same player
        // Each seek changes the epoch; onEnd stays installed.
        let firstSeek = completion.makeDelegate()
        let intermediate = firstSeek.enqueue(success: success)
        let latestSeek = completion.makeDelegate()
        await old.value; await intermediate.value
        #expect(results.isEmpty)
        await latestSeek.enqueue(success: success).value
        await latestSeek.enqueue(success: !success).value
        #expect(results == [success])
    }

    @Test func nativeDelegateQueuedBeforeReplacementOrCloseIsInvalidated() async {
        let oldCompletion = AVQueuePlayback.Completion()
        var oldResults: [Bool] = [], newResults: [Bool] = []
        oldCompletion.onEnd = { oldResults.append($0) }
        let old = oldCompletion.makeDelegate()
        let replaced = old.enqueue(success: true)
        oldCompletion.onEnd = nil

        let newCompletion = AVQueuePlayback.Completion()
        newCompletion.onEnd = { newResults.append($0) }
        let current = newCompletion.makeDelegate()
        await replaced.value
        #expect(oldResults.isEmpty && newResults.isEmpty)
        let closing = current.enqueue(success: false)
        newCompletion.onEnd = nil
        await closing.value
        await old.enqueue(success: false).value
        #expect(oldResults.isEmpty && newResults.isEmpty)
    }

    @Test func heldPlayKeepsUIResponsiveAndBurstPauseResumeCoalesces() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        await rig.controller.prepare(payload: payload(), update: question)?.value
        let clip = try #require(rig.backend.clips.first)
        let play = Hold()
        clip.playHold = play
        rig.controller.present(payload: payload(), update: question)
        await play.reached()
        let stale = clip.onEnd
        var uiPulse = false
        await Task { @MainActor in uiPulse = true }.value
        #expect(uiPulse && clip.plays == 1 && rig.session.owners.count == 1)
        for _ in 0..<1_000 { rig.controller.pause(); rig.controller.resume() }
        rig.controller.pause()
        rig.controller.sessionEvent(.available)
        #expect(rig.controller.paused && clip.pauses == 0 && !clip.stopped)
        play.release()
        await rig.controller.transportSettled()
        #expect(clip.plays == 1 && clip.pauses == 1 && clip.isPaused)
        #expect(rig.session.owners.isEmpty && rig.backend.clips.count <= 2)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        clip.playHold = nil
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        stale?(true)
        #expect(rig.controller.current?.name == "q1.mp3")
    }

    @Test func heldPauseMustFinishBeforeResumeAndRetainsLeaseUntilThen() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.first)
        let pause = Hold()
        clip.pauseHold = pause
        rig.controller.pause()
        await pause.reached()
        rig.controller.resume()
        #expect(rig.session.owners.count == 1 && clip.plays == 1)
        await Task { @MainActor in #expect(!clip.isPaused) }.value
        pause.release(); clip.pauseHold = nil
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        await rig.controller.transportSettled()
        #expect(clip.pauses == 1 && clip.plays == 2 && rig.session.owners.count == 1)
        #expect(rig.session.activations == 2)
    }

    @Test func heldSeekKeepsOnlyLatestPendingTargetAndNeverUnpauses() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.first)
        clip.time = 1
        let seek = Hold()
        clip.seekHold = seek
        let first = rig.controller.requestSeek(by: -0.5)
        await seek.reached()
        for _ in 0..<100 { _ = rig.controller.requestSeek(by: 1) }
        let latest = rig.controller.requestSeek(by: 2)
        rig.controller.pause()
        await Task { @MainActor in #expect(clip.seeks == 1 && clip.samples == 1) }.value
        seek.release()
        await first?.value; await latest?.value
        #expect(clip.seeks == 2 && clip.samples == 2 && clip.time == 2.5)
        #expect(clip.isPaused && rig.controller.paused && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3"])
    }

    @Test func heldStopRetainsDescriptorAndBothSurfaceLeasesBeforeReplacement() async throws {
        let rig = try await self.rig()
        let otherBackend = Backend()
        let other = CardAudioController(resolver: try await rig.fixture.resolved(), backend: otherBackend, session: rig.session)
        defer { rig.controller.close(); other.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        other.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(await otherBackend.nextPlay() == "a1.mp3")
        let old = try #require(rig.backend.clips.first)
        let stop = Hold()
        old.stopHold = stop
        let stale = old.onEnd
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(rig.controller.current == nil && old.onEnd == nil)
        await stop.reached()
        #expect(try await old.file.read(offset: 0, count: 1) == Data([1]))
        #expect(rig.session.owners.count == 2 && rig.backend.events == ["q1.mp3"])
        for _ in 0..<100 {
            rig.controller.replay(payload: payload(), update: question)
            rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        }
        stale?(true)
        #expect(old.stops == 1 && rig.backend.clips.filter { $0.name == "a1.mp3" }.isEmpty)
        stop.release()
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        #expect(await descriptorIsClosed(old))
        #expect(rig.session.owners.count == 2 && other.current?.name == "a1.mp3")
        rig.controller.close()
        await rig.controller.transportSettled()
        #expect(rig.session.owners.count == 1)
        other.close(); await other.transportSettled()
        #expect(rig.session.owners.isEmpty)
    }

    @Test func closeDuringHeldSeekAndStopDropsReplacementAndLateCallback() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.first)
        let stale = clip.onEnd
        let seek = Hold(), stop = Hold()
        clip.seekHold = seek; clip.stopHold = stop
        let seeking = rig.controller.requestSeek(by: 4)
        await seek.reached()
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(rig.controller.current == nil && rig.backend.events == ["q1.mp3"])
        seek.release()
        await stop.reached()
        rig.controller.close()
        rig.controller.sessionEvent(.available)
        stale?(true)
        #expect(try await clip.file.read(offset: 0, count: 1) == Data([1]))
        #expect(rig.session.owners.count == 1)
        stop.release()
        await seeking?.value
        #expect(await descriptorIsClosed(clip))
        #expect(rig.controller.current == nil && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3"])
    }

    @Test func pausedIntentWinsWhileActualPositionSampleIsHeld() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.first)
        clip.time = 1
        let sample = Hold(), pause = Hold()
        clip.sampleHold = sample; clip.pauseHold = pause
        let seeking = rig.controller.requestSeek(by: 1)
        await sample.reached()
        rig.controller.pause()
        sample.release()
        await pause.reached()
        #expect(clip.seeks == 0 && rig.session.owners.count == 1)
        pause.release(); clip.pauseHold = nil
        await seeking?.value
        #expect(clip.time == 2 && clip.isPaused && rig.controller.paused)
        #expect(rig.session.owners.isEmpty && rig.backend.events == ["q1.mp3"])
    }

    @Test func lateActualSampleCannotObserveReplacementOrClosedPlayback() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        await rig.controller.prepare(payload: payload(), update: question)?.value
        let old = try #require(rig.backend.clips.first)
        let sample = Hold()
        old.time = 1; old.sampleHold = sample
        var observed: [CardAudioController.Entry] = []
        rig.controller.onPlaybackObservation = { entry, event in
            if case .advanced = event { observed.append(entry) }
        }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        await sample.reached()
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        rig.controller.close(); await rig.controller.transportSettled()
        let returned = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        old.onSample = { returned.continuation.yield(()) }
        sample.release()
        var iterator = returned.stream.makeAsyncIterator()
        _ = await iterator.next()
        returned.continuation.finish()
        #expect(observed.isEmpty && rig.controller.current == nil && rig.session.owners.isEmpty)
    }

    @Test func failedCurrentActivationSkipsClipAndBalancesItsLease() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.session.hold = true; rig.session.fail = true
        await withCheckedContinuation { continuation in
            rig.session.onHold = { continuation.resume() }
            rig.controller.present(payload: payload(), update: question)
        }
        let activation = try #require(rig.session.held)
        rig.session.hold = false; rig.session.fail = false; rig.session.onHold = nil
        activation.resume()
        #expect(await rig.backend.nextPlay() == "q2.mp3")
        #expect(rig.backend.events == ["q2.mp3"] && rig.session.owners.count == 1)
        #expect(rig.diagnostics.entries.contains { $0.kind == "audioSession" && $0.index == 0 })
        rig.controller.close(); await rig.controller.transportSettled()
        #expect(rig.session.owners.isEmpty)
    }

    @Test func deinitDuringHeldPlayHandsOffCleanupWithoutRetainingController() async throws {
        let fixture = MediaFixture()
        fixture.add("/q1.mp3", Data([1]), "")
        let backend = Backend(), session = Session()
        var controller: CardAudioController? = CardAudioController(resolver: try await fixture.resolved(), backend: backend, session: session)
        weak let weakController = controller
        await controller?.prepare(payload: payload(), update: question)?.value
        let clip = try #require(backend.clips.first)
        let play = Hold(), stop = Hold()
        clip.playHold = play; clip.stopHold = stop
        controller?.present(payload: payload(), update: question)
        await play.reached()
        controller = nil
        #expect(weakController == nil && session.owners.count == 1)
        play.release()
        await stop.reached()
        #expect(try await clip.file.read(offset: 0, count: 1) == Data([1]))
        let disposed = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        clip.onStop = { disposed.continuation.yield(()) }
        stop.release()
        var iterator = disposed.stream.makeAsyncIterator()
        _ = await iterator.next()
        disposed.continuation.finish()
        // Stop's completion resumes the same drain before this observer runs.
        #expect(await descriptorIsClosed(clip))
        #expect(session.owners.isEmpty)
    }

    @Test func failedHeldActivationDoesNotReleaseAnotherSurfaceOrStartOldClip() async throws {
        let rig = try await self.rig()
        let otherBackend = Backend()
        let other = CardAudioController(resolver: try await rig.fixture.resolved(), backend: otherBackend, session: rig.session)
        defer { rig.controller.close(); other.close() }
        rig.session.hold = true; rig.session.fail = true
        await withCheckedContinuation { continuation in
            rig.session.onHold = { continuation.resume() }
            rig.controller.present(payload: payload(), update: question)
        }
        let activation = try #require(rig.session.held)
        rig.session.hold = false; rig.session.fail = false; rig.session.onHold = nil
        other.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(await otherBackend.nextPlay() == "a1.mp3")
        rig.controller.stop()
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(rig.backend.events.isEmpty && rig.session.owners.count == 2)
        activation.resume()
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        #expect(rig.session.owners.count == 2 && rig.backend.events == ["a1.mp3"])
        rig.controller.close(); await rig.controller.transportSettled()
        #expect(rig.session.owners.count == 1)
    }

    @Test func transitionRetainsOnlyCurrentAndPausedClipDoesNotResume() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let payload = self.payload()
        rig.controller.present(payload: payload, update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let q1 = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        q1.time = 1
        rig.controller.pause()
        await rig.controller.transportSettled()
        rig.controller.present(payload: payload, update: answer)
        #expect(rig.controller.current?.name == "q1.mp3" && rig.controller.paused)
        #expect(rig.backend.events == ["q1.mp3"] && rig.session.owners.isEmpty)
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        q1.onEnd?(true)
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        #expect(!rig.backend.events.contains("q2.mp3"))
        rig.controller.present(payload: payload, update: answer) // duplicate shown
        #expect(rig.backend.events == ["q1.mp3", "q1.mp3", "a1.mp3"])
    }

    @Test func interruptDisabledAutoplayAndManualForcedReplacement() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(interrupt: true), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let q1 = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        let stale = q1.onEnd
        rig.controller.present(payload: payload(interrupt: true, autoplay: false), update: answer)
        await rig.controller.transportSettled()
        #expect(q1.stopped && rig.controller.current == nil)
        rig.controller.replay(payload: payload(autoplay: false, replayq: false), update: answer)
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        stale?(true); stale?(true)
        #expect(rig.controller.current?.name == "a1.mp3")
        rig.controller.replay(payload: payload(), update: answer)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        rig.backend.clips.last(where: { $0.onEnd != nil })?.onEnd?(true)
        #expect(await rig.backend.nextPlay() == "q2.mp3")
        rig.backend.clips.last(where: { $0.onEnd != nil })?.onEnd?(true)
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        rig.controller.playEntry(payload: payload(), update: answer, side: .question, index: 1)
        #expect(await rig.backend.nextPlay() == "q2.mp3")
        rig.backend.clips.last(where: { $0.onEnd != nil })?.onEnd?(true)
        await rig.controller.transportSettled()
        #expect(rig.controller.current == nil && rig.session.owners.isEmpty)
    }

    @Test func nextCardDropsEveryWaitingOldClipEvenWithEmptyNewList() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let q1 = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        rig.controller.present(payload: payload(), update: answer)
        let next = CardPayload(cardID: 2, question: "", answer: "", audioPolicy: .init(autoplay: false, interrupt: false))
        rig.controller.present(payload: next, update: .init(card: 2, generation: 3, side: .question))
        q1.onEnd?(true)
        #expect(rig.controller.current == nil && rig.backend.events == ["q1.mp3"])
    }

    @Test func routeLossPausesAndInterruptionCannotAutomaticallyResume() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        let completion = try #require(clip.onEnd)
        clip.time = 1
        rig.controller.sessionEvent(.pause)
        await rig.controller.transportSettled()
        rig.controller.sessionEvent(.available)
        #expect(rig.controller.paused && clip.time == 1 && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3"])
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        rig.controller.sessionEvent(.stop)
        await rig.controller.transportSettled()
        rig.controller.sessionEvent(.pause) // Later route/inactivity cannot undo a stop.
        rig.controller.sessionEvent(.available)
        completion(true) // Neither a recommendation nor an old completion revives the queue.
        #expect(clip.stopped && rig.controller.current == nil && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3", "q1.mp3"])
        rig.controller.replay(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
    }

    #if !os(macOS)
    @Test func typedDeactivationPolicyDistinguishesReleaseInactivityAndInterruption() {
        #expect(AudioSessionCoordinator.deactivationEvent(source: .app, hasInterruption: false, reason: nil) == nil)
        #expect(AudioSessionCoordinator.deactivationEvent(source: .app, hasInterruption: true, reason: nil) == nil)
        #expect(AudioSessionCoordinator.deactivationEvent(source: .app, hasInterruption: true, reason: .routeDisconnected) == nil)
        #expect(AudioSessionCoordinator.deactivationEvent(source: .system, hasInterruption: false, reason: nil) == .pause)
        #expect(AudioSessionCoordinator.deactivationEvent(source: .system, hasInterruption: true, reason: nil) == .stop)
        #expect(AudioSessionCoordinator.deactivationEvent(source: .system, hasInterruption: true, reason: .routeDisconnected) == .pause)
        #expect(AudioSessionCoordinator.deactivationEvent(Notification(name: AVAudioSession.didBecomeInactiveNotification)) == nil)
        #expect(AudioSessionCoordinator.deactivationEvent(Notification(name: AVAudioSession.didBecomeInactiveNotification,
            userInfo: [AVAudioSession.deactivationContextKey: "invalid context"])) == nil)
    }

    @Test(arguments: [AVAudioSession.ResumptionRecommendation.shouldResume, .shouldNotResume])
    func foregroundRecommendationRestoresEligibilityWithoutNativePlayback(_ recommendation: AVAudioSession.ResumptionRecommendation) async throws {
        #expect(AudioSessionCoordinator.resumptionEvent(recommendation: recommendation, isActive: false) == nil)
        let available = try #require(AudioSessionCoordinator.resumptionEvent(recommendation: recommendation, isActive: true))
        #expect(available == .available)
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        clip.time = 1
        let pause = try #require(AudioSessionCoordinator.deactivationEvent(source: .system, hasInterruption: false, reason: nil))
        rig.controller.sessionEvent(pause)
        await rig.controller.transportSettled()
        rig.controller.sessionEvent(available)
        #expect(rig.controller.paused && clip.time == 1 && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3"])
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let stop = try #require(AudioSessionCoordinator.deactivationEvent(source: .system, hasInterruption: true, reason: nil))
        rig.controller.sessionEvent(stop)
        await rig.controller.transportSettled()
        rig.controller.sessionEvent(available)
        #expect(clip.stopped && rig.controller.current == nil && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3", "q1.mp3"])
        rig.controller.replay(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
    }

    @Test func ownDeactivationDuringPauseDoesNotClearCurrentOrWaitingPlayback() async throws {
        let rig = try await self.rig()
        defer { rig.session.onRelease = nil; rig.controller.close() }
        rig.session.onRelease = {
            if let event = AudioSessionCoordinator.deactivationEvent(source: .app, hasInterruption: false, reason: nil) {
                rig.controller.sessionEvent(event)
            }
        }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let clip = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        clip.time = 1
        rig.controller.pause()
        await rig.controller.transportSettled()
        try #require(rig.controller.current?.name == "q1.mp3" && rig.controller.paused)
        #expect(clip.time == 1 && !clip.stopped && rig.session.owners.isEmpty)
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        clip.onEnd?(true)
        #expect(await rig.backend.nextPlay() == "q2.mp3")
    }
    #endif

    @Test func missingTTSIndicesRawPercentAndSeek() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let payload = CardPayload(cardID: 1, question: "", answer: "", questionAudio: [.tts(language: "en", content: "silent"),
                                 .sound(name: "q1.mp3"), .sound(name: "absent.mp3"), .sound(name: "x%20y.mp3")])
        rig.controller.present(payload: payload, update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        #expect(rig.controller.current?.index == 1)
        let clip = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        clip.time = 1
        rig.controller.pause()
        await rig.controller.transportSettled()
        await rig.controller.seek(by: -5)
        #expect(clip.time == 0 && clip.isPaused)
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        await rig.controller.seek(by: 5)
        #expect(await rig.backend.nextPlay() == "x%20y.mp3")
        #expect(rig.controller.current?.index == 3)
        #expect(rig.diagnostics.entries.contains { $0.kind == "missingAudio" && $0.index == 2 })
        rig.controller.playEntry(payload: payload, update: question, side: .question, index: 0)
        await rig.controller.transportSettled()
        #expect(rig.controller.current == nil && rig.session.owners.isEmpty)
    }

    @Test func prepareIsSilentAndStopInvalidatesPendingLoads() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let prepared = rig.controller.prepare(payload: payload(), update: question)
        await prepared?.value
        #expect(rig.backend.clips.count == 1 && rig.backend.events.isEmpty && rig.session.activations == 0)
        let preparedClip = try #require(rig.backend.clips.first)
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        #expect(rig.backend.clips.first === preparedClip && preparedClip.onEnd != nil)
        #expect(rig.backend.clips.filter { $0.name == "q1.mp3" }.count == 1) // no second decode of the prepared entry
        rig.controller.stop()
        rig.backend.hold = true
        await withCheckedContinuation { continuation in
            rig.backend.onHold = { continuation.resume() }
            rig.controller.replay(payload: payload(), update: question)
        }
        rig.backend.onHold = nil
        let held = try #require(rig.backend.held)
        rig.controller.stop() // background/interruption, never automatic resume
        await rig.controller.transportSettled()
        held.resume()
        try await Task.sleep(for: .milliseconds(20))
        #expect(rig.backend.events == ["q1.mp3"] && rig.controller.current == nil && rig.session.owners.isEmpty)
    }

    @Test func currentAnswerAndNextQuestionPreparationStayBoundedAndSilent() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let first = rig.controller.prepare(payload: payload(), update: question)
        await first?.value
        let second = rig.controller.prepare(payload: payload(), update: answer)
        await second?.value
        let old = try #require(rig.backend.clips.first)
        let disposed = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { disposed.continuation.finish() }
        old.onStop = { disposed.continuation.yield(()) }
        let next = CardUpdate(card: 2, generation: 3, side: .question)
        let third = rig.controller.prepare(payload: CardPayload(cardID: 2, question: "", answer: "", questionAudio: [.sound(name: "q3.mp3")]), update: next)
        await third?.value
        var releaseEvents = disposed.stream.makeAsyncIterator()
        _ = await releaseEvents.next()
        #expect(old.stopped && rig.backend.events.isEmpty && rig.session.activations == 0)
        rig.controller.cancelPreparation(question) // already superseded
        rig.controller.present(payload: payload(), update: answer)
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        #expect(rig.backend.clips.count == 3)
    }

    @Test func missingMiddleIsPreparedDuringPlaybackWithoutSessionChurn() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let payload = CardPayload(cardID: 1, question: "q", answer: "a", questionAudio: [
            .sound(name: "q1.mp3"), .sound(name: "absent.mp3"), .sound(name: "q3.mp3")])
        await rig.controller.prepare(payload: payload, update: question)?.value
        let first = try #require(rig.backend.clips.first)
        let waiting = AsyncStream<Clip>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { waiting.continuation.finish() }
        rig.backend.onPrepare = { clip in if clip.name == "q3.mp3" { waiting.continuation.yield(clip) } }
        rig.controller.present(payload: payload, update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let third = try await Spike.deadline("next playable lookahead", timeout: .seconds(3)) {
            for await clip in waiting.stream { return clip }
            throw Spike.Failure("missing third preparation")
        }
        #expect(rig.backend.events == ["q1.mp3"] && third.onEnd == nil)
        #expect(rig.backend.clips.count == 2 && rig.session.activations == 1)
        let owners = rig.session.owners
        #expect(owners.count == 1)
        first.onEnd?(true)
        #expect(await rig.backend.nextPlay() == "q3.mp3")
        #expect(third.onEnd != nil && rig.backend.clips.count == 2)
        #expect(rig.session.owners == owners && rig.session.activations == 1)
        #expect(rig.diagnostics.entries.filter { $0.kind == "missingAudio" && $0.index == 1 }.count == 1)
        third.onEnd?(true)
        await rig.controller.transportSettled()
        #expect(rig.controller.current == nil && rig.session.owners.isEmpty)
    }

    @Test func replacingQueueCancelsHeldLookaheadWithoutPlayingIt() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        await rig.controller.prepare(payload: payload(), update: question)?.value
        rig.backend.hold = true
        await withCheckedContinuation { continuation in
            rig.backend.onHold = { continuation.resume() }
            rig.controller.present(payload: payload(), update: question)
        }
        let waiting = try #require(rig.backend.clips.last)
        #expect(waiting.name == "q2.mp3" && waiting.onEnd == nil)
        let held = try #require(rig.backend.held)
        rig.backend.hold = false; rig.backend.onHold = nil
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        let disposed = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { disposed.continuation.finish() }
        waiting.onStop = { disposed.continuation.yield(()) }
        held.resume()
        try await Spike.deadline("stale queued clip released", timeout: .seconds(3)) {
            for await _ in disposed.stream { return }
            throw Spike.Failure("stale lookahead retained")
        }
        #expect(waiting.stopped && rig.backend.events == ["q1.mp3", "a1.mp3"])
        #expect(rig.controller.current?.name == "a1.mp3" && rig.session.owners.count == 1)
    }

    @Test func delayedSessionActivationCannotRestartStoppedOrReplacementAudio() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        rig.session.hold = true
        await withCheckedContinuation { continuation in
            rig.session.onHold = { continuation.resume() }
            rig.controller.present(payload: payload(), update: question)
        }
        let held = try #require(rig.session.held)
        rig.session.hold = false; rig.session.onHold = nil
        rig.controller.stop()
        rig.controller.playEntry(payload: payload(), update: answer, side: .answer, index: 0)
        #expect(rig.backend.events.isEmpty && rig.controller.current == nil)
        held.resume()
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        await rig.controller.transportSettled()
        #expect(rig.backend.events == ["a1.mp3"] && rig.session.owners.count == 1)
    }

    @Test func twoSurfacesBalanceIndependentSessionLeases() async throws {
        let rig = try await self.rig()
        let otherBackend = Backend()
        let other = CardAudioController(resolver: try await rig.fixture.resolved(), backend: otherBackend, session: rig.session)
        defer { rig.controller.close(); other.close() }
        rig.controller.present(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let otherCard = CardPayload(cardID: 2, question: "", answer: "", questionAudio: [.sound(name: "q3.mp3")])
        other.present(payload: otherCard, update: .init(card: 2, generation: 1, side: .question))
        #expect(await otherBackend.nextPlay() == "q3.mp3")
        #expect(rig.session.owners.count == 2)
        rig.controller.pause()
        await rig.controller.transportSettled()
        #expect(rig.session.owners.count == 1 && other.current?.update.card == 2)
        other.stop()
        await other.transportSettled()
        #expect(rig.session.owners.isEmpty)
    }
}

enum NativeCardAudioChecks {
    /// Invoked only by the existing confirmed-hardware S2 audio lane. Real
    /// controller + backend + timeline, with a missing original AV index 1.
    static func controllerQueueAdvancesAcrossMissingMiddleWithinBudget() async throws {
        let fixture = MediaFixture()
        let silent = SpikeAssets.silentMP3(minimumBytes: 8 * 1024)
        fixture.add("/first.mp3", silent.data, "audio/mpeg")
        fixture.add("/third.mp3", silent.data, "audio/mpeg")
        let diagnostics = CardDiagnostics()
        let controller = CardAudioController(resolver: try await fixture.resolved(), diagnostics: diagnostics)
        defer { controller.close() }
        struct Advancement {
            let position: TimeInterval
            let duration: TimeInterval
            let observedAt: TimeInterval
            var start: TimeInterval { observedAt - position }
        }
        var advances: [Int: [Advancement]] = [:]
        var completions: [Int: TimeInterval] = [:]
        controller.onPlaybackObservation = { entry, observation in
            switch observation {
            case .advanced(let position, let duration, let time):
                guard let duration else { Issue.record("Native duration must be available"); return }
                // Keep two independent advancing samples plus the latest one.
                let sample = Advancement(position: position, duration: duration, observedAt: time)
                if advances[entry.index, default: []].count < 3 { advances[entry.index, default: []].append(sample) }
                else { advances[entry.index]?[2] = sample }
            case .completed(let success, let time):
                #expect(success)
                #expect(completions[entry.index] == nil)
                completions[entry.index] = time
            }
        }
        let payload = CardPayload(cardID: 1, question: "", answer: "",
                                  questionAudio: [.sound(name: "first.mp3"), .sound(name: "absent.mp3"), .sound(name: "third.mp3")])
        let update = CardUpdate(card: 1, generation: 1, side: .question)
        await controller.prepare(payload: payload, update: update)?.value
        #expect(advances.isEmpty && completions.isEmpty)
        let presentedAt = ProcessInfo.processInfo.systemUptime
        controller.present(payload: payload, update: update)
        let deadline = ContinuousClock.now.advanced(by: .seconds(10))
        while completions[2] == nil, ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(5)) }
        #expect(completions[2] != nil, "native controller silent queue must complete")
        let first = try #require(advances[0]), third = try #require(advances[2])
        #expect(first.count >= 2 && third.count >= 2)
        #expect(first[1].position > first[0].position && third[1].position > third[0].position)
        #expect(advances[1] == nil && completions[1] == nil)
        let firstStart = try #require(first.first).start
        #expect(firstStart >= presentedAt - 0.020)
        #expect(firstStart - presentedAt <= 0.050)
        // Infer actual end/start from advancing native timelines and a
        // monotonic sample clock. Delegate delivery alone cannot prove timing.
        let lastFirst = try #require(first.last)
        let firstEnd = lastFirst.start + lastFirst.duration
        let thirdStart = try #require(third.first).start
        print("nativeAudioQueue presentedAt=\(presentedAt) firstStart=\(firstStart) firstEnd=\(firstEnd) thirdStart=\(thirdStart) thirdEnd=\(thirdStart + third[0].duration) startup=\(firstStart - presentedAt) gap=\(thirdStart - firstEnd)")
        #expect(thirdStart >= firstEnd - 0.020)
        #expect(thirdStart - firstEnd <= 0.100)
        #expect(try #require(completions[0]) >= firstEnd - 0.020)
        #expect(diagnostics.entries.contains { $0.kind == "missingAudio" && $0.index == 1 && $0.update == update })
        #expect(controller.current == nil)
        #expect(MPNowPlayingInfoCenter.default().nowPlayingInfo == nil)
        controller.close()
        await controller.transportSettled()
    }

    static func securelyOpenedSilentFileActuallyAdvancesAndSeeks() async throws {
            let fixture = MediaFixture()
            let audio = SpikeAssets.silentMP3(minimumBytes: 64 * 1024)
            fixture.add("/silent.mp3", audio.data, "audio/mpeg")
            let resolver = try await fixture.resolved()
            let file = try await resolver.openAudioName("silent.mp3")
            let outside = fixture.root.deletingLastPathComponent().appending(path: "meguri-audio-escape-\(UUID()).mp3")
            defer { try? FileManager.default.removeItem(at: outside) }
            try SpikeAssets.silentMP3(minimumBytes: 8 * 1024).data.write(to: outside)
            try FileManager.default.moveItem(at: fixture.root.appending(path: "silent.mp3"), to: fixture.root.appending(path: "original.mp3"))
            try FileManager.default.createSymbolicLink(at: fixture.root.appending(path: "silent.mp3"), withDestinationURL: outside)
            let backend = AVQueuePlayback()
            var nativePlayInterval: (start: TimeInterval, end: TimeInterval)?
            backend.onNativePlayInterval = { nativePlayInterval = ($0, $1) }
            let clip = try await backend.prepare(file)
            #expect((clip.duration ?? 0) > 3, "the native player must use the open inode, never the substituted symlink")
            let owner = UUID()
            do {
                try await AudioSessionCoordinator.shared.activate(owner)
                let beforePlayPosition = await clip.sample().position
                var heartbeats: [TimeInterval] = []
                let uiPulse = Task { @MainActor in
                    while !Task.isCancelled {
                        if heartbeats.count < 512 { heartbeats.append(ProcessInfo.processInfo.systemUptime) }
                        do { try await Task.sleep(for: .milliseconds(1)) } catch { return }
                    }
                }
                defer { uiPulse.cancel() }
                let requested = ProcessInfo.processInfo.systemUptime
                try await clip.play()
                let returnedAt = ProcessInfo.processInfo.systemUptime
                uiPulse.cancel()
                let nativeCall = try #require(nativePlayInterval)
                let duringCall = heartbeats.filter { $0 > nativeCall.start && $0 < nativeCall.end }
                let boundaries = [nativeCall.start] + duringCall + [nativeCall.end]
                let largestHeartbeatGap = zip(boundaries, boundaries.dropFirst()).map { pair in pair.1 - pair.0 }.max() ?? 0
                print("nativeAudioExecutor nativeCallStart=\(nativeCall.start) nativeCallEnd=\(nativeCall.end) mainActorHeartbeatsDuringNativeCall=\(duringCall.count) largestHeartbeatGap=\(largestHeartbeatGap) heartbeatBufferFull=\(heartbeats.count == 512)")
                let afterPlayPosition = await clip.sample().position
                try await Task.sleep(for: .milliseconds(150))
                let observed = await clip.sample()
                #expect(observed.position > 0.05)
                let observedStartup = observed.observedAt - requested - observed.position
                #expect(observedStartup <= 0.05)
                print("nativeAudioPlay requestedAt=\(requested) beforePosition=\(beforePlayPosition) returnedAt=\(returnedAt) afterPosition=\(afterPlayPosition) observedAt=\(observed.observedAt) observedPosition=\(observed.position) callDuration=\(returnedAt - requested) inferredStartup=\(observedStartup)")
                #expect(MPNowPlayingInfoCenter.default().nowPlayingInfo == nil)
                await clip.pause()
                let paused = await clip.sample().position
                try await Task.sleep(for: .milliseconds(40))
                #expect(abs(await clip.sample().position - paused) < 0.02)
                try await clip.seek(to: 1)
                #expect(abs(await clip.sample().position - 1) < 0.05)
                try await clip.play()
                try await Task.sleep(for: .milliseconds(100))
                #expect(await clip.sample().position > 1.05)
            } catch {
                await clip.stop()
                AudioSessionCoordinator.shared.release(owner)
                throw error
            }
            await clip.stop()
            AudioSessionCoordinator.shared.release(owner)
        }
}
