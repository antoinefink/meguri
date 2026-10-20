@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    /// Intent ordering uses real pages and independently held native probes.
    @Suite(.timeLimit(.minutes(2))) struct CardIntentOrderingTests {
        @Test func bufferedStopAndAvailabilityPausePlayingVideoBeforeResumeWithoutReplay() async throws {
            let fixture = try await CardBehaviorFixture(video: true)
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1,
                question: "<video id=media src=clip.mp4 muted playsinline loop></video>", answer: "a"))
            let page = fixture.pair.current, binding = page.update, document = page.documentID
            try await fixture.script("""
                window.video = document.getElementById('media');
                window.playEvents = 0;
                video.addEventListener('play', () => playEvents++);
                await video.play();
                window.playingTime = video.currentTime;
                """)
            try await CardBehaviorFixture.until("synthetic video must actually be playing before the burst") {
                try await fixture.bool("return !video.paused && !video.ended && video.loop && video.currentTime > playingTime && playEvents === 1;") == true
            }

            var held: (@MainActor (Result<Bool, any Error>) -> Void)?
            let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { entered.continuation.finish() }
            page.livenessEvaluation = { held = $0; entered.continuation.yield(()) }
            // Deliberately no await between intents: availability is buffered
            // before the event reader can start asynchronous stop cleanup.
            AudioSessionCoordinator.shared.emit(.stop)
            AudioSessionCoordinator.shared.emit(.available)
            try await Spike.deadline("burst availability must reach the held native probe", timeout: .seconds(3)) {
                for await _ in entered.stream { return }
                throw Spike.Failure("missing burst probe")
            }
            #expect(held != nil && !page.presented && !page.behaviorAvailable)
            #expect(try await fixture.bool("return video.paused && !video.ended && playEvents === 1;") == true,
                    "physical pause must complete before foreground is allowed past its probe")
            held?(.success(true))
            try await CardBehaviorFixture.until("burst resume did not finish") { page.behaviorAvailable }
            try await fixture.script("await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))); ")
            #expect(try await fixture.bool("return video === document.getElementById('media') && video.paused && !video.ended && playEvents === 1;") == true)
            #expect(fixture.pair.pages.count == 2 && fixture.pair.current === page && page.update == binding && page.documentID == document)
        }

        @Test func publicResumeHeldInsideNativeProbeCannotUndoNewerSuspend() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            await fixture.pair.suspend()
            let page = fixture.pair.current
            var held: (@MainActor (Result<Bool, any Error>) -> Void)?
            let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            page.livenessEvaluation = { held = $0; entered.continuation.yield(()) }
            let oldResume = Task { await fixture.pair.resume() }
            defer { oldResume.cancel(); entered.continuation.finish() }
            try await Spike.deadline("resume must enter its native probe", timeout: .seconds(3)) {
                for await _ in entered.stream { return }
                throw Spike.Failure("missing held probe")
            }
            #expect(held != nil && !page.livenessCancellations.isEmpty)
            await fixture.pair.suspend()
            held?(.success(true))
            await oldResume.value
            #expect(!page.presented && !page.behaviorAvailable)
            #expect(try await page.string("return String(document.getElementById('qa').textContent);") == "q")
        }

        @Test func availableHeldInsideNativeProbeConsumesStopBeforeOldProbeCompletes() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            let page = fixture.pair.current
            var held: (@MainActor (Result<Bool, any Error>) -> Void)?
            let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            let drained = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { entered.continuation.finish(); drained.continuation.finish() }
            page.livenessEvaluation = { held = $0; entered.continuation.yield(()) }
            AudioSessionCoordinator.shared.emit(.available)
            try await Spike.deadline("available must enter its native probe", timeout: .seconds(3)) {
                for await _ in entered.stream { return }
                throw Spike.Failure("missing available probe")
            }
            #expect(held != nil)
            let cancellations = page.livenessCancellations
            #expect(!cancellations.isEmpty)
            // Observe the existing cancellation boundary. A probe timeout
            // merely removes this entry; it cannot satisfy this signal.
            for (id, cancel) in cancellations {
                page.livenessCancellations[id] = { cancel(); drained.continuation.yield(()) }
            }
            AudioSessionCoordinator.shared.emit(.stop)
            try await Spike.deadline("stop must drain the held probe before its old callback", timeout: .seconds(3)) {
                for await _ in drained.stream { return }
                throw Spike.Failure("stop stayed queued behind available")
            }
            #expect(!page.presented && !page.behaviorAvailable)
            held?(.success(true))
            // A WebKit round trip lets the cancelled resume continuation run;
            // the request is never released before the newer stop is observed.
            _ = try await page.string("return document.readyState;")
            #expect(!page.presented && !page.behaviorAvailable)
        }

        @Test func closeDuringAStartedPublicResumeRejectsItsLateNativeCallback() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            var held: (@MainActor (Result<Bool, any Error>) -> Void)?
            let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            defer { entered.continuation.finish() }
            fixture.pair.current.livenessEvaluation = { held = $0; entered.continuation.yield(()) }
            let oldResume = Task { await fixture.pair.resume() }
            defer { oldResume.cancel() }
            try await Spike.deadline("close regression must hold a running probe", timeout: .seconds(3)) {
                for await _ in entered.stream { return }
                throw Spike.Failure("missing probe")
            }
            fixture.pair.close()
            held?(.success(true))
            await oldResume.value
            #expect(fixture.pair.pages.allSatisfy { $0.closed && !$0.presented })
        }
    }
}
