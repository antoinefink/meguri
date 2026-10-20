@testable import CardRendering
import CoreBridge
import Foundation
import MediaPlayer
import Testing

@Suite struct CardAudioControllerTests {
    final class Session: CardAudioSession {
        var owners: Set<UUID> = []
        var activations = 0
        var hold = false
        var held: CheckedContinuation<Void, Never>?
        var onHold: (() -> Void)?
        func activate(_ owner: UUID) async {
            owners.insert(owner); activations += 1
            if hold { await withCheckedContinuation { held = $0; onHold?() } }
        }
        func release(_ owner: UUID) { owners.remove(owner) }
    }
    final class Clip: CardAudioClip {
        let name: String
        weak var backend: Backend?
        var time: TimeInterval = 0
        var duration: TimeInterval? = 4
        var seekable = true
        var onEnd: ((Bool) -> Void)?
        var stopped = false
        var onStop: (() -> Void)?
        var isPaused = false
        init(_ name: String, backend: Backend) { self.name = name; self.backend = backend }
        func play() { isPaused = false; backend?.played(name) }
        func pause() { isPaused = true }
        func stop() { stopped = true; onStop?() }
        func seek(to time: TimeInterval) async throws { self.time = time }
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
            let clip = Clip(file.name, backend: self)
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

    @Test func transitionRetainsOnlyCurrentAndPausedClipDoesNotResume() async throws {
        let rig = try await self.rig()
        defer { rig.controller.close() }
        let payload = self.payload()
        rig.controller.present(payload: payload, update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        let q1 = try #require(rig.backend.clips.last(where: { $0.onEnd != nil }))
        q1.time = 1
        rig.controller.pause()
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
        clip.time = 1
        rig.controller.sessionEvent(.pause)
        rig.controller.sessionEvent(.available)
        #expect(rig.controller.paused && clip.time == 1 && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3"])
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        rig.controller.sessionEvent(.stop)
        rig.controller.sessionEvent(.available)
        #expect(clip.stopped && rig.controller.current == nil && rig.session.owners.isEmpty)
        #expect(rig.backend.events == ["q1.mp3", "q1.mp3"])
        rig.controller.replay(payload: payload(), update: question)
        #expect(await rig.backend.nextPlay() == "q1.mp3")
    }

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
        await rig.controller.seek(by: -5)
        #expect(clip.time == 0 && clip.isPaused)
        rig.controller.resume()
        #expect(await rig.backend.nextPlay() == "q1.mp3")
        await rig.controller.seek(by: 5)
        #expect(await rig.backend.nextPlay() == "x%20y.mp3")
        #expect(rig.controller.current?.index == 3)
        #expect(rig.diagnostics.entries.contains { $0.kind == "missingAudio" && $0.index == 2 })
        rig.controller.playEntry(payload: payload, update: question, side: .question, index: 0)
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
        let next = CardUpdate(card: 2, generation: 3, side: .question)
        let third = rig.controller.prepare(payload: CardPayload(cardID: 2, question: "", answer: "", questionAudio: [.sound(name: "q3.mp3")]), update: next)
        await third?.value
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
        #expect(await rig.backend.nextPlay() == "a1.mp3")
        held.resume()
        try await Task.sleep(for: .milliseconds(20))
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
        #expect(rig.session.owners.count == 1 && other.current?.update.card == 2)
        other.stop()
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
        #expect(thirdStart >= firstEnd - 0.020)
        #expect(thirdStart - firstEnd <= 0.100)
        #expect(try #require(completions[0]) >= firstEnd - 0.020)
        #expect(diagnostics.entries.contains { $0.kind == "missingAudio" && $0.index == 1 && $0.update == update })
        #expect(controller.current == nil)
        #expect(MPNowPlayingInfoCenter.default().nowPlayingInfo == nil)
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
            let clip = try await AVQueuePlayback().prepare(file)
            #expect((clip.duration ?? 0) > 3, "the native player must use the open inode, never the substituted symlink")
            let owner = UUID()
            defer { clip.stop(); AudioSessionCoordinator.shared.release(owner) }
            try await AudioSessionCoordinator.shared.activate(owner)
            let requested = ProcessInfo.processInfo.systemUptime
            try clip.play()
            try await Task.sleep(for: .milliseconds(150))
            #expect(clip.time > 0.05)
            let observedStartup = ProcessInfo.processInfo.systemUptime - requested - clip.time
            #expect(observedStartup <= 0.05)
            #expect(MPNowPlayingInfoCenter.default().nowPlayingInfo == nil)
            clip.pause()
            let paused = clip.time
            try await Task.sleep(for: .milliseconds(40))
            #expect(abs(clip.time - paused) < 0.02)
            try await clip.seek(to: 1)
            #expect(abs(clip.time - 1) < 0.05)
            try clip.play()
            try await Task.sleep(for: .milliseconds(100))
            #expect(clip.time > 1.05)
        }
}
