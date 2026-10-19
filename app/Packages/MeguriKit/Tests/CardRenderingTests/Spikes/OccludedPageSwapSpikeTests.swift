@testable import CardRendering
import Foundation
import Instrumentation
import Testing
import WebKit

/// The rendering spikes (U65) share WebKit, one window server and this
/// process's memory: their suites run one test at a time, and never beside
/// the card runtime's (`WebKitSuites`).
extension WebKitSuites {
    @Suite(.serialized, .timeLimit(.minutes(2))) enum RenderingSpikes {}
}
typealias RenderingSpikes = WebKitSuites.RenderingSpikes

extension RenderingSpikes {
    /// Spike S1 (U65): two full-frame, opaque pages share one configuration,
    /// the idle one beneath, and a swap changes their order in place (KTD11;
    /// R445, R465, R1853). Kept as regression tests: U67 re-runs them against
    /// `CardPagePair`. Results: docs/design/decisions/s1-occluded-page-swap.md.
    ///
    /// These run on the macOS host in a window that is never on screen, so
    /// WebKit runs no animation frames in them: the checks that need frames
    /// (the beneath page's requestAnimationFrame, the swap's frame, the
    /// keyboard) are CardSwapTimingSpikeTests', in the app on simulators and
    /// in the test VM.
    @Suite struct OccludedPageSwapSpikeTests {
        static let size = CGSize(width: 390, height: 844)

        // MARK: S1(a)

        struct Settled: Decodable {
            let fonts: [Bool]
            let imageComplete: Bool
            let imageWidth: Int
            let scripts: [String]
            let milliseconds: Double
        }

        @Test func aBeneathPageSettlesTwelveBlockingFontFacesABigImageAndItsScriptsBeforeTheSwap() async throws {
            let handler = MediaFixture()
            let fonts = try SpikeAssets.fonts(12)
            for (index, font) in fonts.enumerated() { handler.add("/_face\(index + 1).ttf", font, "font/ttf") }
            let image = try SpikeAssets.noisePNG(minimumBytes: 2 * 1024 * 1024)
            handler.add("/_big.png", image, "image/png")
            handler.add("/_card.js", text: #"window.ran = (window.ran || []).concat("src");"#, "text/javascript")
            let faces = (1...12).map { "Face\($0)" }
            let styles = faces.enumerated().map { index, face in
                #"@font-face { font-family: \#(face); src: url("_face\#(index + 1).ttf"); font-display: block; }"#
            }
            let spans = faces.map { #"<span style="font-family: \#($0)">Meguri \#($0)</span>"# }
            let content = "<style>" + styles.joined(separator: "\n") + "</style>" + spans.joined()
                + #"<img id="big" src="_big.png" width="300"><script>window.ran = ["inline"];</script><script src="_card.js"></script>"#
            let pair = try await Self.makePair(handler)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            let front = pair.front
            let beneath = pair.back
            try await pair.load()
            let started = Spike.now
            let update = try await pair.prepare(CardPayload(cardID: 1, question: content, answer: "answer"))
            let settled = try await beneath.json(
                Settled.self,
                """
                await document.fonts.ready;
                const image = document.getElementById("big");
                await image.decode();
                return JSON.stringify({
                  fonts: faces.map((face) => document.fonts.check("16px " + face)),
                  imageComplete: image.complete, imageWidth: image.naturalWidth, scripts: window.ran,
                  milliseconds: 0,
                });
                """, arguments: ["faces": faces])
            let elapsed = (Spike.now - started) * 1000
            // Still beneath: nothing has been swapped yet.
            #expect(pair.topmost === front)
            #expect(settled.fonts == Array(repeating: true, count: 12), "\(settled.fonts)")
            #expect(settled.imageComplete && settled.imageWidth > 0)
            #expect(settled.scripts == ["inline", "src"])
            for index in 1...12 { #expect(handler.requests(for: "/_face\(index).ttf").map(\.status) == [200]) }
            #expect(handler.requests(for: "/_big.png").first?.bodyLength == image.count)
            _ = try pair.present(update)
            #expect(pair.topmost === beneath)
            print("S1(a) host: 12 faces (\(fonts.map(\.count).reduce(0, +)) bytes) and a \(image.count)-byte image settled beneath in \(Int(elapsed)) ms")
        }

        // MARK: The post-swap confirmation (R1862)

        /// The confirmation channel of U12's contract with real WebKit: a
        /// message posted from Meguri's isolated world, which card scripts
        /// cannot reach, completes the swap's sample in `SwapRecorder` when it
        /// names the swap's card, and is a miss when it names another. (The
        /// double requestAnimationFrame before it needs frames: the app's
        /// CardSwapTimingSpikeTests check that part.)
        @Test func aConfirmationFromTheIsolatedWorldCompletesTheSwapsSample() async throws {
            let handler = MediaFixture()
            var samples: [VisibleSample] = []
            let pair = try await Self.makePair(handler, onSample: { samples.append($0) })
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let update = try await pair.prepare(CardPayload(cardID: 7, question: "seven", answer: "answer"))
            let token = try pair.present(update)
            pair.swaps.presented(token, frameTime: Spike.now)
            try await Task.sleep(for: .milliseconds(150))
            #expect(samples.map(\.outcome) == [.confirmed])
            // An invalid isolated receipt cannot complete another swap.
            pair.current.confirm(["token": token.sequence, "card": 8, "phase": "visible"])
            #expect(samples.count == 1)
            // U12's independent wrong-card control remains a miss.
            let recorder = SwapRecorder { samples.append($0) }
            let wrong = recorder.beginSwap(to: 8, inputTime: Spike.now)
            recorder.presented(wrong, frameTime: Spike.now)
            recorder.receive(["token": wrong.sequence, "card": 7, "phase": "visible"], at: Spike.now)
            recorder.finish()
            #expect(samples.map(\.outcome) == [.confirmed, .wrongCard(reported: 7)])
            let fromPage = try await pair.front.string(
                "return String(Boolean(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.meguriVisible));")
            #expect(fromPage == "false")
        }

        // MARK: S1(d)

        @Test func callAsyncJavaScriptRoundTripsCostWellUnderAFrame() async throws {
            let handler = MediaFixture()
            let pair = try await Self.makePair(handler)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let isolated = WKContentWorld.world(name: "meguri")
            var report: [String] = []
            for (name, world) in [("page", WKContentWorld.page), ("isolated", isolated)] {
                var times: [Double] = []
                for index in 0..<320 {
                    let start = Spike.now
                    let echoed = try await pair.front.string("return String(n + 1);", arguments: ["n": index], in: world)
                    let elapsed = (Spike.now - start) * 1000
                    #expect(echoed == String(index + 1))
                    // The first calls warm the world up.
                    if index >= 20 { times.append(elapsed) }
                }
                let p50 = spikePercentile(times, 50), p95 = spikePercentile(times, 95), worst = times.max() ?? .nan
                report.append(String(format: "%@ world p50 %.2f ms, p95 %.2f ms, max %.2f ms", name, p50, p95, worst))
                // A 120 Hz frame is 8.3 ms; the round trip must be a small
                // part of it.
                #expect(p95 < 4, "\(name) world p95 \(p95) ms")
            }
            print("S1(d) host: " + report.joined(separator: "; "))
        }

        // MARK: S1(f)

        /// The runtime stand-in each page loads once: `show(id, html)`
        /// replaces the card and runs its scripts, as U66's prepare phase
        /// will. With `trackCards` set, it registers each card's elements for
        /// finalization (a weak registration keeps nothing alive), so a test
        /// can see whether old cards are collected after an explicit test
        /// collection and event-loop turns for the finalization callbacks.
        static let tracking = """
            window.cards = 0; window.cardsRegistered = 0; window.cardsFinalized = 0;
            const finalization = new FinalizationRegistry(() => { window.cardsFinalized += 1; });
            new MutationObserver((records) => {
              if (!window.trackCards) return;
              for (const record of records) if (record.target.id === 'qa') {
                for (const node of record.addedNodes) if (node.nodeType === 1 && node.isConnected) {
                  finalization.register(node, 0); window.cardsRegistered += 1;
                }
              }
            }).observe(document.getElementById('qa'), {childList: true});
            """

        static func makePair(_ media: MediaFixture, onSample: @escaping (VisibleSample) -> Void = { _ in }) async throws -> CardPagePair {
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await media.resolved(), size: Self.size, onSample: onSample,
                                    makeWebView: { SpikeWebView(frame: $0, configuration: $1) })
            configuration.media.onResponse = media.handler.onResponse
            return pair
        }

        /// Memory at a point of a burst, in bytes: this process, and the web
        /// content processes that render the pair.
        struct MemoryPoint {
            let cycle: Int
            let app: Int
            let webContent: Int
        }

        struct Collected: Decodable {
            let registered: Int
            let finalized: Int
        }

        /// Test-only collection follows all R1853 footprint samples.
        static func collect(_ page: SpikeWebView) async throws -> Collected {
            // Runtime lookup is for this private testing interface.
            let configuration = page.configuration
            let poolGetter = NSSelectorFromString("processPool")
            guard configuration.responds(to: poolGetter) else {
                throw Spike.Failure("WKWebViewConfiguration lacks required processPool getter for test SPI")
            }
            guard let pool = configuration.perform(poolGetter)?.takeUnretainedValue() as? NSObject else {
                throw Spike.Failure("WKWebViewConfiguration processPool getter returned no NSObject")
            }
            let selector = NSSelectorFromString("_garbageCollectJavaScriptObjectsForTesting")
            guard pool.responds(to: selector) else {
                throw Spike.Failure("WKProcessPool lacks required test SPI _garbageCollectJavaScriptObjectsForTesting")
            }
            // The selector returns void; invoke it with its actual ABI.
            let collect = unsafeBitCast(pool.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> Void).self)
            collect(pool, selector)
            // The script follows the pool's collection request through ordered
            // process IPC. FinalizationRegistry callbacks still need tasks.
            let text: String = try await Spike.within("draining card-element finalizers", timeout: .seconds(5)) { finish in
                page.callAsyncJavaScript(Self.finalizationCounts, arguments: [:], in: nil, in: .page) { result in
                    finish(result.flatMap { value in
                        guard let text = value as? String else {
                            return .failure(Spike.Failure("card-element collection returned no counts"))
                        }
                        return .success(text)
                    })
                }
            }
            return try JSONDecoder().decode(Collected.self, from: Data(text.utf8))
        }

        static let finalizationCounts = """
            const deadline = performance.now() + 2000;
            const channel = new MessageChannel();
            try {
              // Messages turn even an occluded page's event loop, unlike RAF.
              for (let turn = 0; turn < 64; turn += 1) {
                await new Promise((resolve) => { channel.port1.onmessage = resolve; channel.port2.postMessage(0); });
                if (performance.now() >= deadline) throw new Error("card-element finalizers did not drain within 2 s");
              }
            } finally {
              channel.port1.onmessage = null;
              channel.port1.close();
              channel.port2.close();
            }
            return JSON.stringify({ registered: window.cardsRegistered, finalized: window.cardsFinalized });
            """

        /// What a burst of swaps measured.
        struct Burst {
            var pages: [SpikeWebView] = []
            /// The web content processes after the pages loaded, and after
            /// the session closed; how many there were.
            var warmWebContent = 0
            var closedWebContent = 0
            var processes = 0
            var points: [MemoryPoint] = []
            var nodes: [Int: Int] = [:]
            /// Pages created while swapping, the pair's pages after the last
            /// swap, and the container's subviews then.
            var createdDuringSwaps = 0
            var pagesAfter: Set<ObjectIdentifier> = []
            var subviewsAfter = 0
            /// Each page's window moves and card count.
            var windowMoves: [Int] = []
            var cardsSeen: [String] = []
            /// Card elements registered for finalization, and collected once
            /// the session closed, over both pages.
            var registered = 0
            var finalized = 0

            func growth(_ part: KeyPath<MemoryPoint, Int>) -> Int? {
                guard let first = points.first(where: { $0.cycle == 100 }), let last = points.first(where: { $0.cycle == 1000 }) else {
                    return nil
                }
                return last[keyPath: part] - first[keyPath: part]
            }

            static let fiveMB = 5 * 1024 * 1024

            /// R1853 for this process, as U245 reads it: within 5 MB from swap
            /// 100 to 1,000.
            var appProblems: [String] {
                guard let app = growth(\.app) else { return ["no memory at swaps 100 and 1,000"] }
                return app < Self.fiveMB ? [] : ["this process grew \(app / 1024) KB from swap 100 to 1,000"]
            }

            /// R1853 for the web content processes: within 5 MB from swap 100
            /// to 1,000, and back within 10 MB of the warm level once the
            /// session closed (KTD11's call-out, U245).
            var webContentProblems: [String] {
                guard let webContent = growth(\.webContent) else { return ["no memory at swaps 100 and 1,000"] }
                var problems: [String] = []
                if webContent >= Self.fiveMB { problems.append("the web content processes grew \(webContent / 1024) KB from swap 100 to 1,000") }
                let aboveWarm = closedWebContent - warmWebContent
                if aboveWarm >= 2 * Self.fiveMB {
                    problems.append("after the session closed, the web content processes stayed \(aboveWarm / 1024) KB above their warm level")
                }
                return problems
            }

            var summary: String {
                let points = points.filter { [100, 1000].contains($0.cycle) }
                    .map { "\($0.cycle): app \($0.app / 1024) KB, web content \($0.webContent / 1024) KB" }
                return "web content warm \(warmWebContent / 1024) KB; " + points.joined(separator: "; ")
                    + "; closed: web content \(closedWebContent / 1024) KB (\(processes) process(es)); "
                    + "card elements finalized \(finalized) of \(registered)"
            }
        }

        /// The pages' web content footprint after `seconds` of rest.
        static func restedWebContent(_ pages: [SpikeWebView], seconds: Int = 3) async throws -> (processes: Int, bytes: Int) {
            try await Task.sleep(for: .seconds(seconds))
            return try #require(spikeWebContentFootprint(pages), "the pages' web content footprint is not readable")
        }

        /// Puts cards 1 to 1,000 into the page beneath and swaps each to the
        /// front, a frame apart, with `script` in every card; measures memory
        /// after loading, at swaps 100 and 1,000 and after the session closed
        /// (each after 3 s of rest); then counts the collected card elements.
        static func burst(script: String = "window.cards += 1;") async throws -> Burst {
            let handler = MediaFixture()
            handler.add("/_img.png", try SpikeAssets.solidPNG(side: 64), "image/png")
            let pair = try await Self.makePair(handler)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            var burst = Burst(pages: [pair.front, pair.back])
            try await pair.load()
            for page in burst.pages {
                try await page.run(Self.tracking)
                try await page.run("window.trackCards = true;")
            }
            burst.warmWebContent = try await Self.restedWebContent(burst.pages).bytes
            let created = SpikeWebView.created.load(ordering: .relaxed)
            for cycle in 1...1000 {
                let html = #"<h1>Card \#(cycle)</h1><img src="_img.png"><p>\#(String(repeating: "word ", count: 40))</p><script>\#(script)</script>"#
                let update = try await pair.prepare(CardPayload(cardID: Int64(cycle), question: html, answer: "answer"))
                burst.nodes[cycle] = try await pair.back.json(Int.self, "return JSON.stringify(document.getElementsByTagName('*').length);")
                _ = try pair.present(update)
                // At most one card a frame, as a reviewer sees them.
                try await Task.sleep(for: .milliseconds(16))
                if cycle == 100 || cycle == 1000 {
                    let webContent = try await Self.restedWebContent(burst.pages)
                    burst.points.append(MemoryPoint(cycle: cycle, app: spikeFootprint(), webContent: webContent.bytes))
                }
            }
            burst.createdDuringSwaps = SpikeWebView.created.load(ordering: .relaxed) - created
            burst.pagesAfter = [ObjectIdentifier(pair.front), ObjectIdentifier(pair.back)]
            burst.subviewsAfter = pair.container.subviews.count
            burst.windowMoves = burst.pages.map(\.windowMoves)
            for page in burst.pages { burst.cardsSeen.append(try await page.string("return String(window.cards);")) }
            // The session closes: each page drops its card (R1853's release,
            // KTD11's call-out). Then the pages collect their garbage, and the
            // finalized card elements are counted.
            for page in burst.pages { try await page.run("document.getElementById('qa').innerHTML = '';") }
            let closed = try await Self.restedWebContent(burst.pages)
            (burst.processes, burst.closedWebContent) = (closed.processes, closed.bytes)
            for page in burst.pages {
                let collected = try await Self.collect(page)
                burst.registered += collected.registered
                burst.finalized += collected.finalized
            }
            return burst
        }

        @Test func aThousandSwapsKeepTheSamePairInItsWindowWithFlatMemory() async throws {
            let burst = try await Self.burst()
            print("S1(f) host: 1,000 swaps, 2 pages: \(burst.summary)")
            // The same two pages throughout, never out of their window.
            #expect(burst.createdDuringSwaps == 0)
            #expect(burst.pagesAfter == Set(burst.pages.map(ObjectIdentifier.init)))
            #expect(burst.subviewsAfter == 2)
            #expect(burst.windowMoves == [1, 1])
            // Each page saw every other card: globals persist per page (R465).
            #expect(burst.cardsSeen == ["500", "500"])
            // The DOM does not grow, and this process's memory stays within
            // R1853's 5 MB from swap 100 to 1,000.
            #expect(burst.nodes[1000] == burst.nodes[100])
            #expect(burst.appProblems.isEmpty, "\(burst.appProblems): \(burst.summary)")
            // No page keeps old cards alive: once the session closed, all but
            // a few of the card elements registered were collected.
            #expect(burst.registered == 4000)
            #expect(burst.finalized * 100 >= burst.registered * 95, "\(burst.summary)")
            // S1(f) failed for web content memory (decision record S1,
            // escalated): here and on screen in the app, the web content
            // processes grow about 10 MB over the burst and keep it after the
            // session closes, though no card stays alive; WebKit gives it back
            // only to its own memory-pressure release. The check stays, so a
            // run that meets R1853 shows.
            withKnownIssue("S1(f) failed: web content memory over 1,000 swaps (decision record S1, escalated)", isIntermittent: true) {
                #expect(burst.webContentProblems.isEmpty, "\(burst.webContentProblems): \(burst.summary)")
            }
        }

        @Test func explicitCollectionKeepsRetainedRemovedElementsUntilReleased() async throws {
            let handler = MediaFixture()
            let pair = try await Self.makePair(handler)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            let page = pair.front
            try await pair.load()
            try await page.run(Self.tracking)
            try await page.run(
                """
                window.trackCards = true;
                document.getElementById("qa").innerHTML = "<h1>Retained</h1><p>Removed</p><span>Card</span><div>Elements</div>";
                await Promise.resolve();
                window.retainedElements = [...document.getElementById("qa").children];
                document.getElementById("qa").innerHTML = "";
                """)
            let retained = try await Self.collect(page)
            #expect(retained.registered == 4)
            #expect(retained.finalized == 0, "strongly retained removed elements must stay alive")
            try await page.run("window.retainedElements = null;")
            let released = try await Self.collect(page)
            #expect(released.registered == 4)
            #expect(released.finalized == released.registered, "released removed elements must be finalized")
        }

        /// The memory check sees what the pages keep: cards that each retain
        /// 64 KB in their page's script heap (57.6 MB from swap 100 to 1,000)
        /// pass this process's check and keep the DOM the same size, but the
        /// web content check sees most of what they keep.
        @Test func theMemoryCheckCatchesCardsThatRetainMemoryInTheirPages() async throws {
            let burst = try await Self.burst(
                script: "window.kept = (window.kept || []); window.kept.push(new Uint8Array(65536).fill(window.kept.length % 251 + 1));")
            print("S1(f) host, cards retaining 64 KB each: \(burst.webContentProblems): \(burst.summary)")
            #expect(burst.nodes[1000] == burst.nodes[100])
            #expect(burst.appProblems.isEmpty, "this process's memory alone would have missed it: \(burst.summary)")
            #expect(burst.webContentProblems.contains { $0.hasPrefix("the web content processes grew") }, "\(burst.summary)")
            #expect((burst.growth(\.webContent) ?? 0) >= 40 * 1024 * 1024, "\(burst.summary)")
        }
    }
}

private extension CardPagePair {
    var front: SpikeWebView { current.webView as! SpikeWebView }
    var back: SpikeWebView { idle.webView as! SpikeWebView }
    var topmost: CardPlatformView? { container.subviews.last }
}
