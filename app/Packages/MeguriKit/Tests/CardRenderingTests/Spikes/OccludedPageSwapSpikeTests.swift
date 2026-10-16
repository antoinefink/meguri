import Foundation
import Instrumentation
import Testing
import WebKit

/// The rendering spikes (U65) share WebKit, one window server and this
/// process's memory: their suites run one test at a time.
@Suite(.serialized, .timeLimit(.minutes(2))) enum RenderingSpikes {}

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

        /// A card page under `/_anki/`, the media root as its base.
        static func card(_ body: String, head: String = "") -> String {
            """
            <!doctype html><html><head><meta charset="utf-8"><base href="\(Spike.origin.absoluteString)">
            <style>html, body { margin: 0; background: #fdf6e3; }</style>\(head)</head><body>\(body)</body></html>
            """
        }

        // MARK: S1(a)

        struct Settled: Decodable {
            let fonts: [Bool]
            let imageComplete: Bool
            let imageWidth: Int
            let scripts: [String]
            let milliseconds: Double
        }

        @Test func aBeneathPageSettlesTwelveBlockingFontFacesABigImageAndItsScriptsBeforeTheSwap() async throws {
            let handler = SpikeMediaHandler()
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
            handler.add("/_anki/front.html", text: Self.card("<p>Card A</p>"), "text/html")
            handler.add(
                "/_anki/beneath.html",
                text: Self.card(
                    spans.joined() + #"<img id="big" src="_big.png" width="300"><script>window.ran = ["inline"];</script><script src="_card.js"></script>"#,
                    head: "<style>\(styles.joined(separator: "\n"))</style>"),
                "text/html")

            let pair = SpikePagePair(configuration: Spike.configuration(handler: handler), size: Self.size)
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            let front = pair.front
            let beneath = pair.back
            try await front.open(Spike.origin.appending(path: "_anki/front.html"))
            let started = Spike.now
            try await beneath.open(Spike.origin.appending(path: "_anki/beneath.html"))
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
            pair.swap()
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
            let handler = SpikeMediaHandler()
            handler.add("/_anki/card.html", text: Self.card(#"<div id="qa" data-card="7"></div>"#), "text/html")
            let configuration = Spike.configuration(handler: handler)
            let world = WKContentWorld.world(name: "meguri")
            let messages = SpikeMessages()
            configuration.userContentController.add(messages, contentWorld: world, name: VisibleConfirmation.messageName)
            let pair = SpikePagePair(configuration: configuration, size: Self.size)
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            for page in [pair.front, pair.back] { try await page.open(Spike.origin.appending(path: "_anki/card.html")) }
            var samples: [VisibleSample] = []
            let recorder = SwapRecorder(emitter: IntervalEmitter(begin: Signposts.begin, end: Signposts.end)) { samples.append($0) }
            messages.onReceive = { recorder.receive($0.body, at: $0.time) }
            let post = """
                const card = Number(document.getElementById("qa").dataset.card);
                window.webkit.messageHandlers.meguriVisible.postMessage({ token, card, phase: "visible" });
                return typeof window.show;
                """
            // The right card, then a swap whose page names another card.
            for expected in [Int64(7), 8] {
                let token = recorder.beginSwap(to: expected, inputTime: Spike.now)
                pair.swap()
                recorder.presented(token, frameTime: Spike.now)
                // The page world's globals are not visible from the probe's world.
                #expect(try await pair.front.string(post, arguments: ["token": token.sequence], in: world) == "undefined")
                try await Task.sleep(for: .milliseconds(100))
            }
            recorder.finish()
            #expect(samples.map(\.outcome) == [.confirmed, .wrongCard(reported: 7)])
            #expect(messages.received.allSatisfy { $0.webView === pair.back || $0.webView === pair.front })
            // A page-world script cannot post to the probe's handler.
            let fromPage = try await pair.front.string(
                "return String(Boolean(window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.meguriVisible));")
            #expect(fromPage == "false")
        }

        // MARK: S1(d)

        @Test func callAsyncJavaScriptRoundTripsCostWellUnderAFrame() async throws {
            let handler = SpikeMediaHandler()
            handler.add("/_anki/card.html", text: Self.card("<p>Card</p>"), "text/html")
            let pair = SpikePagePair(configuration: Spike.configuration(handler: handler), size: Self.size)
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            try await pair.front.open(Spike.origin.appending(path: "_anki/card.html"))
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
        /// will.
        static let skeleton = card(
            #"<div id="qa"></div>"#,
            head: """
                <script>
                window.cards = 0;
                window.show = (id, html) => {
                  const qa = document.getElementById("qa");
                  qa.innerHTML = html;
                  qa.dataset.card = id;
                  for (const old of qa.querySelectorAll("script")) {
                    const script = document.createElement("script");
                    script.textContent = old.textContent;
                    old.replaceWith(script);
                  }
                  return document.getElementsByTagName("*").length;
                };
                </script>
                """)

        @Test func aThousandSwapsKeepTheSamePairInItsWindowWithFlatMemory() async throws {
            let handler = SpikeMediaHandler()
            handler.add("/_anki/card.html", text: Self.skeleton, "text/html")
            handler.add("/_img.png", try SpikeAssets.solidPNG(side: 64), "image/png")
            let pair = SpikePagePair(configuration: Spike.configuration(handler: handler), size: Self.size)
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            let pages = [pair.front, pair.back]
            for page in pages { try await page.open(Spike.origin.appending(path: "_anki/card.html")) }
            let created = SpikeWebView.created.load(ordering: .relaxed)
            var footprints: [(cycle: Int, bytes: Int)] = []
            var nodes: [Int: Int] = [:]
            for cycle in 1...1000 {
                let html = #"<h1>Card \#(cycle)</h1><img src="_img.png"><p>\#(String(repeating: "word ", count: 40))</p><script>window.cards += 1;</script>"#
                let count = try await pair.back.string("return String(show(id, html));", arguments: ["id": cycle, "html": html])
                nodes[cycle] = Int(count)
                pair.swap()
                if cycle % 100 == 0 { footprints.append((cycle, spikeFootprint())) }
            }
            // The same two pages throughout, never out of their window.
            #expect(SpikeWebView.created.load(ordering: .relaxed) == created)
            #expect(Set([ObjectIdentifier(pair.front), ObjectIdentifier(pair.back)]) == Set(pages.map(ObjectIdentifier.init)))
            #expect(pair.container.subviews.count == 2)
            #expect(pages.map(\.windowMoves) == [1, 1])
            // Each page saw every other card: globals persist per page (R465).
            for page in pages {
                let seen = try await page.string("return String(window.cards);")
                #expect(seen == "500")
            }
            // The DOM does not grow, and neither does this process's memory
            // after the first hundred swaps (R1853's 5 MB over 1,000 cards).
            #expect(nodes[1000] == nodes[100])
            let growth = (footprints.last?.bytes ?? 0) - (footprints.first?.bytes ?? 0)
            #expect(growth < 5 * 1024 * 1024, "footprint grew \(growth) bytes: \(footprints)")
            print("S1(f) host: 1,000 swaps, 2 pages, footprint growth \(growth / 1024) KB from swap 100 to 1,000: \(footprints.map { "\($0.cycle): \($0.bytes / 1024) KB" })")
        }
    }
}
