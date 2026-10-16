#if MEGURI_TEST_HOOKS
import DesignSystem
import Foundation
import Instrumentation
import Metal
import QuartzCore
import SwiftUI
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
import notify
#endif

extension TestHooks {
    /// Spike S1's in-app harness (U65, KTD11): two full-frame, opaque card
    /// pages sharing one configuration, the idle one beneath, swapped by
    /// z-order from an input handler; and, beside them, a pair of plain
    /// native views swapped the same way as the baseline (R1807). Inputs
    /// alternate: odd ones swap the pages, even ones the baseline.
    ///
    /// Each swap marks a Metal drawable that presents with the swap's Core
    /// Animation commit (`presentsWithTransaction`): its presented time is
    /// the display frame that shows the commit, where the interval ends
    /// (KTD11's call-out, VisibleConfirmation). A display link's next target
    /// time is recorded too, to compare. The page swap runs through
    /// `SwapRecorder` under `review.answerToNextVisible`; the front page
    /// confirms with a double requestAnimationFrame from an isolated world.
    /// The next card is prepared beneath and counts as settled only after a
    /// double requestAnimationFrame there (S1(a)).
    ///
    /// Throwaway: U67's CardPagePair and U243's frame probe replace it.
    /// CardSwapTimingSpikeTests drives it and reads `reportJSON`.
    @Observable
    final class CardSwapSpike: NSObject, WKScriptMessageHandler {
        enum Mode: String {
            case timing
            /// iOS: each page swap also captures the screen at the presented
            /// frame and checks the card's colour (S1(c)).
            case capture
            /// Cards with `#typeans`, focused from page script after the
            /// swap (S1(e), R534).
            case typein
        }

        struct Stats: Codable {
            var count = 0
            var p50Frames = 0
            var p95Frames = 0
            var p99Frames = 0
            var maxFrames = 0
            var p50Ms = 0.0
            var p95Ms = 0.0
            var p99Ms = 0.0
            var displayLinkP95Frames = 0
            /// From the handler instead of the event's timestamp: XCUITest's
            /// synthesized events on the simulator reach the app late.
            var fromHandlerP95Frames = 0
            var fromHandlerP99Frames = 0
        }

        struct Report: Codable {
            var mode = ""
            var ready = false
            var probe = "none"
            var periodMs = 0.0
            var inputs = 0
            var web = Stats()
            var baseline = Stats()
            /// Page swaps asked for while the next card was not settled.
            var unsettled = 0
            /// Swaps the Metal probe gave no presented time for (their frame
            /// is the display link's estimate).
            var unpresented = 0
            var confirmed = 0
            var wrongCard = 0
            var unconfirmed = 0
            var confirmP95Ms = 0.0
            var inputToHandlerP95Ms = 0.0
            var frontRAFPerSecond = 0
            var beneathRAFPerSecond = 0
            var captured = 0
            var blank = 0
            var stale = 0
            var captureControl = ""
            var focused = ""
            /// Confirmation messages received, and what the last confirmation
            /// script returned.
            var messages = 0
            var lastConfirm = ""
            var terminations = 0
            var burst: Burst?
        }

        struct Burst: Codable {
            var swaps = 0
            var footprintAt100KB = 0
            var footprintAt1000KB = 0
            var nodesAt100 = 0
            var nodesAt1000 = 0
            var pages = 0
            var windowMoves: [Int] = []
        }

        private(set) var reportJSON = "{}"
        private(set) var swaps = 0

        @ObservationIgnored let mode: Mode
        @ObservationIgnored let stage = Stage()
        @ObservationIgnored private var report = Report()
        @ObservationIgnored private let handler = MediaOriginSpike.Handler()
        @ObservationIgnored private let probeWorld = WKContentWorld.world(name: "meguri")
        @ObservationIgnored private var front: SpikePage!
        @ObservationIgnored private var back: SpikePage!
        @ObservationIgnored private lazy var recorder = SwapRecorder { [weak self] sample in self?.record(sample) }
        @ObservationIgnored private let probe = FrameProbe()
        @ObservationIgnored private var displayLink: AnyObject?
        @ObservationIgnored private var period: TimeInterval = 1.0 / 60
        @ObservationIgnored private var nextCard: Int64 = 2
        @ObservationIgnored private var settled = false
        @ObservationIgnored private var confirmations: [UInt64: CheckedContinuation<Void, Never>] = [:]
        @ObservationIgnored private var records: [Record] = []
        @ObservationIgnored private var started = false
        @ObservationIgnored private var triggers: [SpikeTrigger] = []

        /// One swap's times, in seconds on the input events' clock.
        private struct Record {
            let web: Bool
            let input: TimeInterval
            let handled: TimeInterval
            /// The page swap's recorder token, or the baseline's signpost.
            let token: SwapToken?
            let signpost: IntervalToken?
            let card: Int64
            var presented: TimeInterval?
            var displayLink: TimeInterval?
            /// The probe gave no presented time: the display link's estimate
            /// ends the interval instead.
            var estimated = false
        }

        init(mode: Mode) {
            self.mode = mode
            super.init()
            report.mode = mode.rawValue
            publish()
        }

        // MARK: Setup

        func start() {
            guard !started else { return }
            started = true
            handler.files["/_anki/card.html"] = (Data(Self.skeleton.utf8), "text/html")
            handler.files["/_img.png"] = (Self.png, "image/png")
            let configuration = MediaOriginSpike.configuration(handler, persistent: false)
            configuration.userContentController.add(self, contentWorld: probeWorld, name: VisibleConfirmation.messageName)
            back = SpikePage(configuration)
            front = SpikePage(configuration)
            stage.install(pages: [back.webView, front.webView], probe: probe.view) { [weak self] time in
                self?.input(at: time)
            }
            report.probe = probe.available ? "metal" : "displayLink"
            startDisplayLink()
            triggers = [
                SpikeTrigger("cardSwapDrive") { [weak self] in self?.drive() },
                SpikeTrigger("cardSwapBurst") { [weak self] in Task { await self?.burst() } },
            ]
            Task { await load() }
        }

        func stop() {
            triggers.forEach { $0.stop() }
            triggers = []
            (displayLink as? CADisplayLink)?.invalidate()
            displayLink = nil
        }

        private func load() async {
            guard await front.open(), await back.open() else {
                report.ready = false
                publish()
                return
            }
            _ = await front.run("show(id, html); return 'ok';", ["id": 1, "html": Self.card(1, mode: mode)])
            await prepare(nextCard)
            // S1(a): animation frames in each page over one second.
            async let frontRate = front.run(Self.rafCounter)
            async let beneathRate = back.run(Self.rafCounter)
            report.frontRAFPerSecond = Int(await frontRate) ?? -1
            report.beneathRAFPerSecond = Int(await beneathRate) ?? -1
            #if !os(macOS)
            if mode == .capture { report.captureControl = captureColor().map(Self.describe) ?? "none" }
            #endif
            report.ready = true
            publish()
        }

        private func startDisplayLink() {
            #if os(macOS)
            let link = stage.displayLink(target: self, selector: #selector(tick(_:)))
            #else
            let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
            #endif
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            link.add(to: .main, forMode: .common)
            displayLink = link
        }

        /// The first record the display link has not estimated yet (records
        /// are estimated in order). The link runs all the time: one resumed
        /// from a pause starts late, which would bias its estimate.
        @ObservationIgnored private var nextEstimate = 0

        @objc private func tick(_ link: CADisplayLink) {
            period = max(link.targetTimestamp - link.timestamp, 1.0 / 240)
            while nextEstimate < records.count, records[nextEstimate].handled < link.timestamp {
                // The commit made while handling the input is composited in
                // the frame this callback starts.
                records[nextEstimate].displayLink = link.targetTimestamp
                if !probe.available || records[nextEstimate].estimated { resolve(nextEstimate, frame: link.targetTimestamp) }
                nextEstimate += 1
            }
        }

        // MARK: Input

        private var inputs = 0

        /// An input arrived (touch-down on iOS, mouse-down on the Mac) with
        /// the event's timestamp.
        private func input(at time: TimeInterval) {
            guard report.ready else { return }
            inputs += 1
            report.inputs = inputs
            if inputs % 2 == 1 { swapPages(input: time) } else { swapBaseline(input: time) }
            // SwiftUI chrome changes in the same commit.
            swaps += 1
        }

        @discardableResult
        private func swapPages(input: TimeInterval) -> SwapToken? {
            guard settled else {
                report.unsettled += 1
                publish()
                return nil
            }
            settled = false
            let card = nextCard
            let token = recorder.beginSwap(to: card, inputTime: input)
            (front, back) = (back, front)
            stage.bringToFront(front.webView)
            #if !os(macOS)
            if mode == .typein { _ = front.webView.becomeFirstResponder() }
            #endif
            mark(Record(web: true, input: input, handled: CACurrentMediaTime(), token: token, signpost: nil, card: card))
            CATransaction.setCompletionBlock { [weak self] in
                // After the commit: shown hooks, the confirmation, and the
                // next card beneath (no web-content call before the swap).
                Task { await self?.afterSwap(token: token, card: card) }
            }
            return token
        }

        private func swapBaseline(input: TimeInterval) {
            let signpost = Signposts.begin(IntervalNames.probeNativeSwap)
            stage.swapBaseline()
            mark(Record(web: false, input: input, handled: CACurrentMediaTime(), token: nil, signpost: signpost, card: 0))
        }

        /// Records a swap and marks the probe in its commit. The frame is the
        /// probe's presented time, or the display link's estimate when there
        /// is no Metal device.
        private func mark(_ record: Record) {
            records.append(record)
            let index = records.count - 1
            probe.mark { [weak self] presented in
                guard let self else { return }
                if let presented {
                    self.resolve(index, frame: presented)
                    return
                }
                // No presented time (none on the simulator; zero in the test
                // VM): the display link's estimate stands in.
                if self.probe.available { self.report.unpresented += 1 }
                self.records[index].estimated = true
                if let estimate = self.records[index].displayLink { self.resolve(index, frame: estimate) }
            }
        }

        /// The frame that showed swap `index`: ends its interval.
        private func resolve(_ index: Int, frame: TimeInterval) {
            guard records[index].presented == nil else { return }
            records[index].presented = frame
            if let token = records[index].token { recorder.presented(token, frameTime: frame) }
            if let signpost = records[index].signpost { Signposts.end(signpost) }
            #if !os(macOS)
            if mode == .capture, records[index].web { check(card: records[index].card) }
            #endif
            publish()
        }

        private func afterSwap(token: SwapToken, card: Int64) async {
            let page: SpikePage = front
            if mode == .typein {
                // The shown hook (R534).
                report.focused = await page.run("document.getElementById('typeans').focus(); return document.activeElement.id || 'none';")
            }
            Task {
                let result = await page.run(Self.confirm, ["token": NSNumber(value: token.sequence)], in: probeWorld)
                report.lastConfirm = result
                publish()
            }
            nextCard += 1
            await prepare(nextCard)
        }

        /// Puts `card` in the page beneath; settled after a double
        /// requestAnimationFrame there.
        private func prepare(_ card: Int64) async {
            let result = await back.run(Self.prepareScript, ["id": card, "html": Self.card(card, mode: mode)])
            settled = result == String(card)
            publish()
        }

        // MARK: Messages and samples

        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            report.messages += 1
            guard let parsed = recorder.receive(message.body, at: CACurrentMediaTime()) else {
                report.lastConfirm = "unparsed \(message.body)"
                return
            }
            confirmations.removeValue(forKey: parsed.token)?.resume()
        }

        private func record(_ sample: VisibleSample) {
            switch sample.outcome {
            case .confirmed: report.confirmed += 1
            case .wrongCard: report.wrongCard += 1
            case .unconfirmed: report.unconfirmed += 1
            }
            if let confirmedTime = sample.confirmedTime { confirmLags.append(confirmedTime - sample.presentedTime) }
            publish()
        }

        @ObservationIgnored private var confirmLags: [TimeInterval] = []

        private func publish() {
            report.periodMs = period * 1000
            report.web = stats(records.filter(\.web))
            report.baseline = stats(records.filter { !$0.web })
            report.confirmP95Ms = Self.percentile(confirmLags, 95) * 1000
            report.inputToHandlerP95Ms = Self.percentile(records.map { $0.handled - $0.input }, 95) * 1000
            report.terminations = (front?.terminations ?? 0) + (back?.terminations ?? 0)
            let encoder = JSONEncoder()
            encoder.outputFormatting = .sortedKeys
            reportJSON = (try? encoder.encode(report)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        }

        private func stats(_ records: [Record]) -> Stats {
            let presented = records.compactMap { record in record.presented.map { $0 - record.input } }
            let frames = presented.map { Int(($0 / period).rounded()) }
            let linked = records.compactMap { record in record.displayLink.map { Int((($0 - record.input) / period).rounded()) } }
            let handled = records.compactMap { record in record.presented.map { Int((($0 - record.handled) / period).rounded()) } }
            return Stats(
                count: presented.count,
                p50Frames: Self.percentile(frames, 50), p95Frames: Self.percentile(frames, 95), p99Frames: Self.percentile(frames, 99),
                maxFrames: frames.max() ?? 0,
                p50Ms: Self.percentile(presented, 50) * 1000, p95Ms: Self.percentile(presented, 95) * 1000,
                p99Ms: Self.percentile(presented, 99) * 1000, displayLinkP95Frames: Self.percentile(linked, 95),
                fromHandlerP95Frames: Self.percentile(handled, 95), fromHandlerP99Frames: Self.percentile(handled, 99))
        }

        static func percentile<T: Comparable & Numeric>(_ values: [T], _ p: Double) -> T {
            guard !values.isEmpty else { return 0 }
            let sorted = values.sorted()
            let rank = Int((p / 100 * Double(sorted.count)).rounded(.up))
            return sorted[min(max(rank, 1), sorted.count) - 1]
        }

        // MARK: Driving without synthesized input

        /// The Mac: posts 40 mouse-downs to the input view, 150 ms apart,
        /// through the app's own event queue (macOS drops XCUITest's
        /// synthesized input on this toolchain, app/AGENTS.md). Each event's
        /// timestamp is its posting time. A page-swap input waits until the
        /// next card has settled, as a reviewer answers only a card on
        /// screen: an input that still finds it unsettled is a real miss.
        private func drive() {
            #if os(macOS)
            Task {
                for index in 0..<40 {
                    if (inputs + index) % 2 == 0 {
                        let until = CACurrentMediaTime() + 2
                        while !settled, CACurrentMediaTime() < until { try? await Task.sleep(for: .milliseconds(2)) }
                    }
                    stage.postMouseDown()
                    try? await Task.sleep(for: .milliseconds(150))
                }
            }
            #endif
        }

        /// S1(f): 1,000 page swaps with no input, each after the next card
        /// settled and the previous one confirmed; memory and DOM size at
        /// swap 100 and 1,000.
        private func burst() async {
            var burst = Burst()
            let pages = [ObjectIdentifier(front), ObjectIdentifier(back)]
            for cycle in 1...1000 {
                while !settled { try? await Task.sleep(for: .milliseconds(2)) }
                guard let token = swapPages(input: CACurrentMediaTime()) else { continue }
                // The confirmation comes after the commit and two frames:
                // registered in this turn, it cannot be missed. A second
                // without it stops waiting (an unconfirmed miss).
                let timeout = Task {
                    try? await Task.sleep(for: .seconds(1))
                    self.confirmations.removeValue(forKey: token.sequence)?.resume()
                }
                await withCheckedContinuation { continuation in confirmations[token.sequence] = continuation }
                timeout.cancel()
                if cycle == 100 || cycle == 1000 {
                    while !settled { try? await Task.sleep(for: .milliseconds(2)) }
                    let nodes = Int(await back.run("return String(document.getElementsByTagName('*').length);")) ?? -1
                    let footprint = Self.footprint() / 1024
                    if cycle == 100 {
                        burst.footprintAt100KB = footprint
                        burst.nodesAt100 = nodes
                    } else {
                        burst.footprintAt1000KB = footprint
                        burst.nodesAt1000 = nodes
                    }
                }
                burst.swaps = cycle
            }
            burst.pages = Set([ObjectIdentifier(front), ObjectIdentifier(back)]).union(pages).count
            burst.windowMoves = [front.windowMoves, back.windowMoves]
            report.burst = burst
            publish()
        }

        static func footprint() -> Int {
            var info = task_vm_info_data_t()
            var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
            let result = withUnsafeMutablePointer(to: &info) {
                $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                    task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
                }
            }
            return result == KERN_SUCCESS ? Int(info.phys_footprint) : 0
        }

        // MARK: S1(c) on iOS

        #if !os(macOS)
        /// What the screen shows at the middle of the pages, from a render
        /// of the window's current frame.
        private func captureColor() -> (Int, Int, Int)? {
            let view = stage.pagesArea
            let renderer = UIGraphicsImageRenderer(bounds: view.bounds, format: { let f = UIGraphicsImageRendererFormat(); f.scale = 1; return f }())
            let image = renderer.image { _ in _ = view.drawHierarchy(in: view.bounds, afterScreenUpdates: false) }
            guard let cg = image.cgImage else { return nil }
            var pixel = [UInt8](repeating: 0, count: 4)
            guard
                let context = CGContext(
                    data: &pixel, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            else { return nil }
            context.draw(cg, in: CGRect(x: -CGFloat(cg.width / 2), y: -CGFloat(cg.height / 2), width: CGFloat(cg.width), height: CGFloat(cg.height)))
            return (Int(pixel[0]), Int(pixel[1]), Int(pixel[2]))
        }

        private func check(card: Int64) {
            guard let color = captureColor() else { return }
            report.captured += 1
            let expected = Self.rgb(card)
            let previous = Self.rgb(card - 1)
            func near(_ a: (Int, Int, Int), _ b: (Int, Int, Int)) -> Bool {
                abs(a.0 - b.0) <= 16 && abs(a.1 - b.1) <= 16 && abs(a.2 - b.2) <= 16
            }
            if near(color, expected) { return }
            if near(color, previous) { report.stale += 1 } else { report.blank += 1 }
        }

        static func describe(_ color: (Int, Int, Int)) -> String { "\(color.0),\(color.1),\(color.2)" }
        #endif

        // MARK: Pages

        /// CSS named colours: odd cards papayawhip, even ones lightsteelblue;
        /// never white, so a blank frame cannot pass for a card.
        static func rgb(_ card: Int64) -> (Int, Int, Int) { card % 2 == 1 ? (255, 239, 213) : (176, 196, 222) }

        static func card(_ id: Int64, mode: Mode) -> String {
            let background = id % 2 == 1 ? "papayawhip" : "lightsteelblue"
            let typein = mode == .typein ? #"<input id="typeans" type="text" autocomplete="off">"# : ""
            return """
                <style>body { background: \(background); }</style>
                <h1>Card \(id)</h1>\(typein)<img src="_img.png" width="64" height="64">
                <p>\(String(repeating: "Meguri ", count: 30))</p><script>window.cards += 1;</script>
                """
        }

        /// The runtime stand-in each page loads once: `show(id, html)`
        /// replaces the card and runs its scripts, as U66's prepare will.
        static let skeleton = """
            <!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
            <base href="meguri-media://media/">
            <style>html, body { margin: 0; height: 100%; font: -apple-system-body; } #qa { padding: 16px; }</style>
            <script>
            window.cards = 0;
            window.show = (id, html) => {
              const qa = document.getElementById("qa");
              qa.innerHTML = html;
              qa.dataset.card = String(id);
              for (const old of qa.querySelectorAll("script")) {
                const script = document.createElement("script");
                script.textContent = old.textContent;
                old.replaceWith(script);
              }
            };
            </script></head><body><div id="qa"></div></body></html>
            """

        static let prepareScript = """
            show(id, html);
            await Promise.all([...document.images].map((image) => image.decode().catch(() => {})));
            await document.fonts.ready;
            await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
            return String(id);
            """

        /// The post-swap confirmation, from Meguri's isolated world
        /// (VisibleConfirmation).
        static let confirm = """
            try {
              await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
              const card = Number(document.getElementById("qa").dataset.card);
              window.webkit.messageHandlers.meguriVisible.postMessage({ token, card, phase: "visible" });
              return "posted";
            } catch (error) {
              return "error " + error;
            }
            """

        /// Animation frames a second, counted over two seconds after one
        /// quiet second (a page just loaded may still be busy).
        static let rafCounter = """
            await new Promise((resolve) => setTimeout(resolve, 1000));
            let frames = 0;
            const end = performance.now() + 2000;
            await new Promise((resolve) => {
              const tick = () => { frames += 1; if (performance.now() < end) requestAnimationFrame(tick); else resolve(); };
              requestAnimationFrame(tick);
              setTimeout(resolve, 4000);
            });
            return String(Math.round(frames / 2));
            """

        static let png: Data = {
            // An 8×8 PNG made at run time (a new context is zero-filled:
            // black).
            let context = CGContext(
                data: nil, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            let output = NSMutableData()
            guard let image = context?.makeImage(),
                let destination = CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil)
            else { return Data() }
            CGImageDestinationAddImage(destination, image, nil)
            CGImageDestinationFinalize(destination)
            return output as Data
        }()
    }

    /// One card page of the spike pair.
    final class SpikePage: NSObject, WKNavigationDelegate {
        let webView: SpikeWebView
        private var loaded: CheckedContinuation<Bool, Never>?
        private(set) var terminations = 0
        var windowMoves: Int { webView.windowMoves }

        init(_ configuration: WKWebViewConfiguration) {
            webView = SpikeWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 480), configuration: configuration)
            super.init()
            webView.navigationDelegate = self
            #if !os(macOS)
            webView.isOpaque = true
            webView.scrollView.isScrollEnabled = false
            #endif
        }

        func open() async -> Bool {
            await withCheckedContinuation { continuation in
                loaded = continuation
                webView.load(URLRequest(url: MediaOriginSpike.origin.appending(path: "_anki/card.html")))
            }
        }

        func run(_ script: String, _ arguments: [String: Any] = [:], in world: WKContentWorld = .page) async -> String {
            do {
                return try await webView.callAsyncJavaScript(script, arguments: arguments, contentWorld: world) as? String ?? ""
            } catch {
                return "error: \(error.localizedDescription)"
            }
        }

        private func done(_ ok: Bool) {
            loaded?.resume(returning: ok)
            loaded = nil
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { done(true) }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) { done(false) }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) { done(false) }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { terminations += 1 }
    }

    /// A card page that counts its moves into or out of a window: a swap
    /// must never reparent it (S1(f)).
    final class SpikeWebView: WKWebView {
        private(set) var windowMoves = 0

        #if os(macOS)
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            windowMoves += 1
        }
        #else
        override func didMoveToWindow() {
            super.didMoveToWindow()
            windowMoves += 1
        }
        #endif
    }

    /// A Metal layer that presents with the current Core Animation commit;
    /// each mark reports when its drawable was shown. The iOS simulator's
    /// Metal has no presented time (`addPresentedHandler` is not in its
    /// SDK): there the probe is unavailable and the display link's estimate
    /// stands in, which is why the simulator checks S1(b) for function only.
    final class FrameProbe {
        let view = ProbeView()
        private let queue: MTLCommandQueue?
        private var flip = false
        var layer: CAMetalLayer { view.metalLayer }

        #if targetEnvironment(simulator)
        var available: Bool { false }
        #else
        var available: Bool { queue != nil }
        #endif

        init() {
            let device = MTLCreateSystemDefaultDevice()
            queue = device?.makeCommandQueue()
            layer.device = device
            layer.pixelFormat = .bgra8Unorm
            layer.framebufferOnly = true
            layer.presentsWithTransaction = true
            layer.drawableSize = CGSize(width: 4, height: 4)
            layer.maximumDrawableCount = 3
            layer.isOpaque = true
        }

        /// Draws a new drawable into the current transaction; `presented`
        /// gets its presented time, or nil when it was never shown (or the
        /// probe is unavailable).
        func mark(_ presented: @escaping @MainActor (TimeInterval?) -> Void) {
            #if targetEnvironment(simulator)
            presented(nil)
            #else
            guard let queue, let drawable = layer.nextDrawable(), let buffer = queue.makeCommandBuffer() else {
                presented(nil)
                return
            }
            flip.toggle()
            let pass = MTLRenderPassDescriptor()
            pass.colorAttachments[0].texture = drawable.texture
            pass.colorAttachments[0].loadAction = .clear
            pass.colorAttachments[0].storeAction = .store
            pass.colorAttachments[0].clearColor = MTLClearColor(red: flip ? 0 : 1, green: flip ? 0 : 1, blue: flip ? 0 : 1, alpha: 1)
            buffer.makeRenderCommandEncoder(descriptor: pass)?.endEncoding()
            drawable.addPresentedHandler { drawable in
                let time = drawable.presentedTime
                Task { @MainActor in presented(time > 0 ? time : nil) }
            }
            buffer.commit()
            // presentsWithTransaction: wait until scheduled, then present
            // the drawable itself, so it joins this commit.
            buffer.waitUntilScheduled()
            drawable.present()
            #endif
        }
    }

    #if os(macOS)
    final class ProbeView: NSView {
        let metalLayer = CAMetalLayer()

        init() {
            super.init(frame: .zero)
            wantsLayer = true
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError("not used") }

        override func makeBackingLayer() -> CALayer { metalLayer }
    }
    #else
    final class ProbeView: UIView {
        override class var layerClass: AnyClass { CAMetalLayer.self }
        var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
    }
    #endif

    #if os(macOS)
    typealias SpikeView = NSView
    #else
    typealias SpikeView = UIView
    #endif

    /// The spike's stage: the baseline strip at the top, the card pages
    /// below it, the frame probe in a corner, and over everything the view
    /// that takes the inputs, so no input reaches a page.
    final class Stage: SpikeView {
        let baselineArea = SpikeView()
        let pagesArea = SpikeView()
        private let input = InputView()
        private var probe: SpikeView?
        private let baseline = [SpikeView(), SpikeView()]
        private var baselineFront = 1

        #if os(macOS)
        override var isFlipped: Bool { true }
        #endif

        init() {
            super.init(frame: CGRect(x: 0, y: 0, width: 390, height: 700))
            #if os(macOS)
            wantsLayer = true
            for view in [baselineArea, pagesArea] { view.wantsLayer = true }
            baseline[0].wantsLayer = true
            baseline[1].wantsLayer = true
            baseline[0].layer?.backgroundColor = NSColor.systemOrange.cgColor
            baseline[1].layer?.backgroundColor = NSColor.systemTeal.cgColor
            #else
            baseline[0].backgroundColor = .systemOrange
            baseline[1].backgroundColor = .systemTeal
            #endif
            for view in baseline { baselineArea.addSubview(view) }
            addSubview(baselineArea)
            addSubview(pagesArea)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError("not used") }

        func install(pages: [SpikeView], probe: SpikeView, onInput: @escaping @MainActor (TimeInterval) -> Void) {
            for page in pages { pagesArea.addSubview(page) }
            addSubview(probe)
            self.probe = probe
            input.onInput = onInput
            addSubview(input)
            layoutPages()
        }

        /// Brings `page` to the front of the pages, in place: it stays in
        /// its superview and window.
        func bringToFront(_ page: SpikeView) {
            #if os(macOS)
            let target = Unmanaged.passUnretained(page).toOpaque()
            pagesArea.sortSubviews(
                { first, second, context in
                    let top = context.map { Unmanaged<NSView>.fromOpaque($0).takeUnretainedValue() }
                    if first === top { return .orderedDescending }
                    if second === top { return .orderedAscending }
                    return .orderedSame
                }, context: target)
            #else
            pagesArea.bringSubviewToFront(page)
            #endif
        }

        func swapBaseline() {
            baselineFront = 1 - baselineFront
            let page = baseline[baselineFront]
            #if os(macOS)
            let target = Unmanaged.passUnretained(page).toOpaque()
            baselineArea.sortSubviews(
                { first, second, context in
                    let top = context.map { Unmanaged<NSView>.fromOpaque($0).takeUnretainedValue() }
                    if first === top { return .orderedDescending }
                    if second === top { return .orderedAscending }
                    return .orderedSame
                }, context: target)
            #else
            baselineArea.bringSubviewToFront(page)
            #endif
        }

        #if os(macOS)
        override func layout() {
            super.layout()
            layoutPages()
        }

        /// A mouse-down and -up at the input view's centre, through the
        /// app's event queue.
        func postMouseDown() {
            guard let window else { return }
            let point = input.convert(CGPoint(x: input.bounds.midX, y: input.bounds.midY), to: nil)
            let time = ProcessInfo.processInfo.systemUptime
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                if let event = NSEvent.mouseEvent(
                    with: type, location: point, modifierFlags: [], timestamp: time, windowNumber: window.windowNumber, context: nil,
                    eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)
                {
                    NSApp.postEvent(event, atStart: false)
                }
            }
        }
        #else
        override func layoutSubviews() {
            super.layoutSubviews()
            layoutPages()
        }
        #endif

        private func layoutPages() {
            let strip = Spacing.xxLarge
            baselineArea.frame = CGRect(x: 0, y: 0, width: bounds.width, height: strip)
            for view in baseline { view.frame = baselineArea.bounds }
            pagesArea.frame = CGRect(x: 0, y: strip, width: bounds.width, height: max(bounds.height - strip, 0))
            for page in pagesArea.subviews { page.frame = pagesArea.bounds }
            probe?.frame = CGRect(x: bounds.width - 2, y: 0, width: 2, height: 2)
            input.frame = bounds
        }
    }

    /// Takes every input over the stage: touch-down on iOS, mouse-down on
    /// the Mac, with the event's timestamp.
    final class InputView: SpikeView {
        var onInput: (@MainActor (TimeInterval) -> Void)?

        init() {
            super.init(frame: .zero)
            #if os(macOS)
            setAccessibilityElement(true)
            setAccessibilityRole(.group)
            setAccessibilityIdentifier("spike.cardSwap.input")
            #else
            isAccessibilityElement = true
            accessibilityIdentifier = "spike.cardSwap.input"
            isMultipleTouchEnabled = false
            #endif
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError("not used") }

        #if os(macOS)
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func mouseDown(with event: NSEvent) { onInput?(event.timestamp) }
        #else
        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
            onInput?(event?.timestamp ?? CACurrentMediaTime())
        }
        #endif
    }

    /// Listens for `<bundle id>.testhooks.<name>` (Darwin on iOS,
    /// distributed on the Mac), as PerfProbeTrigger does.
    final class SpikeTrigger {
        #if os(macOS)
        private var observer: NSObjectProtocol?
        #else
        private var token: Int32 = NOTIFY_TOKEN_INVALID
        #endif

        init(_ name: String, _ action: @escaping @MainActor () -> Void) {
            let name = "\(Bundle.main.bundleIdentifier ?? "").testhooks.\(name)"
            #if os(macOS)
            observer = DistributedNotificationCenter.default().addObserver(forName: Notification.Name(name), object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { action() }
            }
            #else
            notify_register_dispatch(name, &token, .main) { _ in
                MainActor.assumeIsolated { action() }
            }
            #endif
        }

        func stop() {
            #if os(macOS)
            if let observer { DistributedNotificationCenter.default().removeObserver(observer) }
            observer = nil
            #else
            if token != NOTIFY_TOKEN_INVALID { notify_cancel(token) }
            token = NOTIFY_TOKEN_INVALID
            #endif
        }
    }
}
#endif
