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
import CoreMedia
import ScreenCaptureKit
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
            /// Each page swap's frame is captured and must show the new
            /// card's marker (S1(c)): on iOS a render of the window at the
            /// swap's frame, on the Mac every frame of the app's own window
            /// (ScreenCaptureKit, no permission needed for one's own windows).
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
            /// S1(c): page swaps whose frame showed the new card's marker;
            /// captures that showed no card's marker (white, the page's own
            /// background, anything else), or an older card's.
            var captured = 0
            var blank = 0
            var stale = 0
            /// The check run on known screens before any swap, as
            /// `classify` named them: card 1 shown and expected (`card`),
            /// card 1 shown while card 2 or card 3 is expected (`stale`
            /// twice), the front page's card hidden (`blank`), and card 1
            /// again (`card`).
            var captureControls: [String] = []
            /// The Mac's window capture: the frames it saw, those that came
            /// with no display time (their arrival time stands in), those
            /// that still showed the previous card less than a frame after
            /// the swap's estimated frame (display-link estimates run late),
            /// and why the capture did not start, if it did not.
            var capturedFrames = 0
            var framesWithoutDisplayTime = 0
            var lateWithinAFrame = 0
            var captureError = ""
            var focused = ""
            /// Confirmation messages received, and what the last confirmation
            /// script returned.
            var messages = 0
            var lastConfirm = ""
            var terminations = 0
            var burst: Burst?
        }

        struct Burst: Codable {
            /// Each card kept 64 KB in its page (the memory check's control).
            var retaining = false
            var swaps = 0
            var footprintAt100KB = 0
            var footprintAt1000KB = 0
            /// The pages' web content processes: after loading (warm), at
            /// swaps 100 and 1,000, and after the session closed (both pages
            /// dropped their card). -1 where the system does not tell (iOS).
            var webContentWarmKB = -1
            var webContentAt100KB = -1
            var webContentAt1000KB = -1
            var webContentClosedKB = -1
            /// Why the web content processes' footprint is not readable.
            var webContentNote = ""
            var nodesAt100 = 0
            var nodesAt1000 = 0
            var pages = 0
            var windowMoves: [Int] = []
            /// Card elements the pages registered for finalization, and how
            /// many were collected once the session closed: a page that kept
            /// old cards alive would collect none.
            var cardElementsRegistered = 0
            var cardElementsFinalized = 0
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
                // The memory check's control: every card keeps 64 KB in its
                // page's script heap.
                SpikeTrigger("cardSwapBurstRetaining") { [weak self] in Task { await self?.burst(retaining: true) } },
            ]
            Task { await load() }
        }

        func stop() {
            triggers.forEach { $0.stop() }
            triggers = []
            (displayLink as? CADisplayLink)?.invalidate()
            displayLink = nil
            #if os(macOS)
            windowCapture?.stop()
            windowCapture = nil
            #endif
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
            if mode == .capture { await captureControls() }
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

        /// Cards keep 64 KB each in their page (the burst's memory control).
        @ObservationIgnored private var retaining = false

        /// Puts `card` in the page beneath; settled after a double
        /// requestAnimationFrame there.
        private func prepare(_ card: Int64) async {
            let result = await back.run(Self.prepareScript, ["id": card, "html": Self.card(card, mode: mode, retaining: retaining)])
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
            #if os(macOS)
            if mode == .capture { checkFrames() }
            #endif
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
        /// swap 100 and 1,000, each after 3 s of rest; then the session
        /// closes (both pages drop their card) and memory is read again
        /// after 3 s; last, the pages collect their garbage and the
        /// finalized card elements are counted.
        private func burst(retaining: Bool = false) async {
            var burst = Burst()
            burst.retaining = retaining
            let pages = [ObjectIdentifier(front), ObjectIdentifier(back)]
            let views: [WKWebView] = [front.webView, back.webView]
            for page in [front!, back!] { _ = await page.run("window.trackCards = true; return 'ok';") }
            let warm = await restedWebContentKB(views, &burst)
            burst.webContentWarmKB = warm
            for cycle in 1...1000 {
                while !settled { try? await Task.sleep(for: .milliseconds(2)) }
                if cycle == 1, retaining {
                    // The card already beneath was made before: remake it.
                    self.retaining = true
                    await prepare(nextCard)
                }
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
                    let webContent = await restedWebContentKB(views, &burst)
                    let footprint = Self.footprint() / 1024
                    if cycle == 100 {
                        burst.footprintAt100KB = footprint
                        burst.webContentAt100KB = webContent
                        burst.nodesAt100 = nodes
                    } else {
                        burst.footprintAt1000KB = footprint
                        burst.webContentAt1000KB = webContent
                        burst.nodesAt1000 = nodes
                    }
                }
                burst.swaps = cycle
            }
            burst.pages = Set([ObjectIdentifier(front), ObjectIdentifier(back)]).union(pages).count
            burst.windowMoves = [front.windowMoves, back.windowMoves]
            // The session closes: each page drops its card (R1853's release,
            // KTD11 call-out). Then the pages collect their garbage, and the
            // finalized card elements are counted.
            while !settled { try? await Task.sleep(for: .milliseconds(2)) }
            for page in [front!, back!] { _ = await page.run("document.getElementById('qa').innerHTML = ''; return 'ok';") }
            let closed = await restedWebContentKB(views, &burst)
            burst.webContentClosedKB = closed
            for page in [front!, back!] {
                let counts = await Self.collectCardElements(page)
                let parts = counts.split(separator: " ")
                guard parts.count == 2, let registered = Int(parts[0]), let finalized = Int(parts[1]),
                    registered >= 0, finalized >= 0, finalized <= registered
                else {
                    report.lastConfirm = "error: card-element collection: \(counts)"
                    report.burst = burst
                    publish()
                    return
                }
                burst.cardElementsRegistered += registered
                burst.cardElementsFinalized += finalized
            }
            report.burst = burst
            publish()
        }

        /// PRIVATE TEST SPI: request collection only after all memory samples,
        /// then let finalization callbacks run. A stuck process must report an
        /// error rather than leave the UI waiting for the burst forever.
        private static func collectCardElements(_ page: SpikePage) async -> String {
            let pool = page.webView.configuration.processPool
            let selector = NSSelectorFromString("_garbageCollectJavaScriptObjectsForTesting")
            guard pool.responds(to: selector) else {
                return "error: WKProcessPool lacks required test SPI _garbageCollectJavaScriptObjectsForTesting"
            }
            let collect = unsafeBitCast(pool.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> Void).self)
            collect(pool, selector)
            let reply = CollectionReply()
            let timeout = Task {
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
                reply.finish("error: card-element finalizers did not drain within 5 s")
            }
            defer { timeout.cancel() }
            return await withCheckedContinuation { continuation in
                reply.continuation = continuation
                // Ordered IPC puts this script after the pool's GC request.
                page.webView.callAsyncJavaScript(Self.collectScript, arguments: [:], in: nil, in: .page) { result in
                    switch result {
                    case .success(let value): reply.finish(value as? String ?? "error: collection returned no counts")
                    case .failure(let error): reply.finish("error: \(error.localizedDescription)")
                    }
                }
            }
        }

        private final class CollectionReply {
            var continuation: CheckedContinuation<String, Never>?

            func finish(_ value: String) {
                continuation?.resume(returning: value)
                continuation = nil
            }
        }

        /// Bounded event-loop turns, then actual registered/finalized counts.
        static let collectScript = """
            const deadline = performance.now() + 2000;
            const channel = new MessageChannel();
            try {
              for (let turn = 0; turn < 64; turn += 1) {
                await new Promise((resolve) => { channel.port1.onmessage = resolve; channel.port2.postMessage(0); });
                if (performance.now() >= deadline) throw new Error("card-element finalizers did not drain within 2 s");
              }
            } finally {
              channel.port1.onmessage = null;
              channel.port1.close();
              channel.port2.close();
            }
            return window.cardsRegistered + " " + window.cardsFinalized;
            """

        /// The pages' web content footprint in KB after 3 s of rest, or -1
        /// (`webContentNote` says why).
        private func restedWebContentKB(_ views: [WKWebView], _ burst: inout Burst) async -> Int {
            try? await Task.sleep(for: .seconds(3))
            do {
                return try Self.webContentFootprint(views) / 1024
            } catch {
                burst.webContentNote = error.why
                return -1
            }
        }

        struct Unreadable: Error {
            let why: String
        }

        /// The footprint of the web content processes that render `views`,
        /// in bytes, or nil when the system does not tell. Each pid comes
        /// from `_webProcessIdentifier` (WebKit SPI, read only in test hooks)
        /// and its footprint from `proc_pid_rusage`. iOS has no public way
        /// to read another process's footprint, but the simulator's
        /// processes are this Mac's (or the test VM's), unsandboxed, and its
        /// libSystem has the call; the Mac's app sandbox refuses it.
        static func webContentFootprint(_ views: [WKWebView]) throws(Unreadable) -> Int {
            #if os(macOS) || targetEnvironment(simulator)
            let selector = NSSelectorFromString("_webProcessIdentifier")
            var pids: Set<pid_t> = []
            for view in views {
                guard view.responds(to: selector), let pid = (view.value(forKey: "_webProcessIdentifier") as? NSNumber)?.int32Value, pid > 0
                else { throw Unreadable(why: "no web content process identifier") }
                pids.insert(pid)
            }
            #if os(macOS)
            let rusage: @convention(c) (Int32, Int32, UnsafeMutableRawPointer?) -> Int32 = { pid, flavor, buffer in
                proc_pid_rusage(pid, flavor, buffer?.assumingMemoryBound(to: rusage_info_t?.self))
            }
            #else
            // libproc.h is not in the iOS SDK.
            guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "proc_pid_rusage") else {
                throw Unreadable(why: "no proc_pid_rusage in this simulator's libSystem")
            }
            let rusage = unsafeBitCast(symbol, to: (@convention(c) (Int32, Int32, UnsafeMutableRawPointer?) -> Int32).self)
            #endif
            var bytes = 0
            for pid in pids {
                var usage = rusage_info_v4()
                let result = withUnsafeMutablePointer(to: &usage) { rusage(pid, RUSAGE_INFO_V4, UnsafeMutableRawPointer($0)) }
                guard result == 0 else { throw Unreadable(why: "proc_pid_rusage refused: errno \(errno)") }
                bytes += Int(usage.ri_phys_footprint)
            }
            return bytes
            #else
            throw Unreadable(why: "iOS offers no way to read another process's footprint")
            #endif
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

        // MARK: S1(c)

        /// What a capture of the pages' middle showed, against the card that
        /// should be there.
        enum Shown: String {
            /// The expected card's marker.
            case card
            /// The marker of one of the two cards before it: the other page's
            /// last card, or this page's.
            case stale
            /// No card's marker: white, the page's own background, anything
            /// else.
            case blank
        }

        static func classify(_ color: RGB, expected card: Int64) -> Shown {
            func near(_ a: RGB, _ b: RGB) -> Bool { abs(a.0 - b.0) <= 16 && abs(a.1 - b.1) <= 16 && abs(a.2 - b.2) <= 16 }
            if near(color, marker(card)) { return .card }
            if near(color, marker(card - 1)) || near(color, marker(card - 2)) { return .stale }
            return .blank
        }

        /// The check, run on known screens before any swap: card 1 in
        /// front. Each must come out as named (`Report.captureControls`), or
        /// the check could not tell a stale or blank frame from the card.
        private func captureControls() async {
            #if os(macOS)
            await startWindowCapture()
            #endif
            var controls: [String] = []
            guard let shown = await captureNow() else {
                report.captureControls = ["no capture"]
                return
            }
            for expected: Int64 in [1, 2, 3] { controls.append(Self.classify(shown, expected: expected).rawValue) }
            // The front page's card hidden: only the page's background shows.
            _ = await front.run("document.getElementById('qa').style.visibility = 'hidden';" + Self.painted)
            controls.append((await captureNow()).map { Self.classify($0, expected: 1).rawValue } ?? "no capture")
            _ = await front.run("document.getElementById('qa').style.visibility = '';" + Self.painted)
            controls.append((await captureNow()).map { Self.classify($0, expected: 1).rawValue } ?? "no capture")
            report.captureControls = controls
        }

        /// Waits until the page has painted its last change.
        static let painted = """
            await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
            return "painted";
            """

        #if os(macOS)
        @ObservationIgnored private var windowCapture: WindowCapture?
        /// Every frame the window capture saw, in order.
        @ObservationIgnored private var frames: [WindowCapture.Frame] = []

        private func startWindowCapture() async {
            // A hidden launch (tools/mg xcode-check's route launches on this
            // Mac) has nothing on screen to check: never capture there.
            guard !NSApp.isHidden else {
                report.captureError = "the app is hidden"
                return
            }
            guard let window = stage.window else {
                report.captureError = "the stage has no window"
                return
            }
            let capture = WindowCapture()
            do {
                let middle = stage.pagesArea.convert(CGPoint(x: stage.pagesArea.bounds.midX, y: stage.pagesArea.bounds.midY), to: nil)
                // Window coordinates from the frame's top left.
                try await capture.start(
                    windowID: CGWindowID(window.windowNumber), scale: window.backingScaleFactor,
                    at: CGPoint(x: middle.x, y: window.frame.height - middle.y)
                ) { [weak self] frame in
                    self?.received(frame)
                }
                windowCapture = capture
            } catch {
                report.captureError = "\(error)"
            }
        }

        /// What the window shows now: the capture sends a frame whenever the
        /// window changes, so once the last change has had time to arrive,
        /// the last frame is the screen.
        private func captureNow() async -> RGB? {
            guard windowCapture != nil else { return nil }
            try? await Task.sleep(for: .milliseconds(300))
            return frames.last?.color
        }

        /// A frame arrived: the S1(c) counts are published when they change
        /// (publishing changes the report on screen, which sends a frame).
        private func received(_ frame: WindowCapture.Frame) {
            frames.append(frame)
            let before = (report.captured, report.blank, report.stale, report.lateWithinAFrame)
            checkFrames()
            if before != (report.captured, report.blank, report.stale, report.lateWithinAFrame) { publish() }
        }

        /// S1(c) on the Mac, over every captured frame from the first page
        /// swap on. A frame shows the card of the last page swap handled
        /// before it; until one frame after that swap's frame (or while its
        /// frame is not known yet) it may still show the card before. No
        /// frame may show no card's marker (`blank`) or an older card
        /// (`stale`). Frames that showed the card before after the swap's
        /// estimated frame, but within one frame of it, are counted apart
        /// (`lateWithinAFrame`): display-link estimates run late.
        private func checkFrames() {
            let swaps = records.filter(\.web)
            report.capturedFrames = frames.count
            report.framesWithoutDisplayTime = frames.filter { !$0.displayed }.count
            guard windowCapture != nil, let first = swaps.first else { return }
            var blank = 0, stale = 0, late = 0
            var seen: Set<Int64> = []
            for frame in frames where frame.time >= first.handled {
                guard let swap = swaps.last(where: { $0.handled <= frame.time }) else { continue }
                switch Self.classify(frame.color, expected: swap.card) {
                case .card:
                    seen.insert(swap.card)
                case .blank:
                    blank += 1
                case .stale:
                    guard Self.classify(frame.color, expected: swap.card - 1) == .card else {
                        stale += 1
                        continue
                    }
                    // The card before: fine until the swap's frame.
                    guard let shown = swap.presented, frame.time >= shown else { continue }
                    if frame.time < shown + period { late += 1 } else { stale += 1 }
                }
            }
            report.captured = seen.count
            report.blank = blank
            report.stale = stale
            report.lateWithinAFrame = late
        }
        #else
        /// What the screen shows at the middle of the pages, from a render
        /// of the window's current frame.
        private func captureColor() -> RGB? {
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

        /// The screen as it is now, once the last commit is on it.
        private func captureNow() async -> RGB? {
            try? await Task.sleep(for: .milliseconds(100))
            return captureColor()
        }

        private func check(card: Int64) {
            guard let color = captureColor() else { return }
            switch Self.classify(color, expected: card) {
            case .card: report.captured += 1
            case .stale: report.stale += 1
            case .blank: report.blank += 1
            }
        }
        #endif

        // MARK: Pages

        typealias RGB = (Int, Int, Int)

        /// Each card's marker colour: a band across the middle of the page
        /// whose colour names the card. Each channel is one of 64, 112, 160
        /// and 208, so any two cards fewer than 64 apart differ by 48 or
        /// more in a channel, and none is near white, black or the page's
        /// own background (`skeleton`): a blank page, a page whose card did
        /// not paint, and a page still showing an older card all fail.
        static func marker(_ card: Int64) -> RGB {
            let card = Int(((card % 64) + 64) % 64)
            return (64 + 48 * (card % 4), 64 + 48 * (card / 4 % 4), 64 + 48 * (card / 16))
        }

        static func card(_ id: Int64, mode: Mode, retaining: Bool = false) -> String {
            let (red, green, blue) = marker(id)
            let typein = mode == .typein ? #"<input id="typeans" type="text" autocomplete="off">"# : ""
            let keep = retaining ? " window.kept = (window.kept || []); window.kept.push(new Uint8Array(65536).fill(window.cards % 251 + 1));" : ""
            return """
                <div class="marker" style="background: rgb(\(red), \(green), \(blue))"></div>
                <h1>Card \(id)</h1>\(typein)<img src="_img.png" width="64" height="64">
                <p>\(String(repeating: "Meguri ", count: 30))</p><script>window.cards += 1;\(keep)</script>
                """
        }

        /// The runtime stand-in each page loads once: `show(id, html)`
        /// replaces the card and runs its scripts, as U66's prepare will.
        static let skeleton = """
            <!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
            <base href="meguri-media://media/">
            <style>
            html, body { margin: 0; height: 100%; font: -apple-system-body; background: rgb(253, 246, 227); }
            #qa { padding: 16px; }
            /* S1(c): each card's marker, a band across the middle of the page. */
            .marker { position: fixed; left: 0; right: 0; top: 30%; height: 40%; }
            </style>
            <script>
            window.cards = 0;
            window.cardsRegistered = 0;
            window.cardsFinalized = 0;
            const finalization = new FinalizationRegistry(() => { window.cardsFinalized += 1; });
            window.show = (id, html) => {
              const qa = document.getElementById("qa");
              qa.innerHTML = html;
              qa.dataset.card = String(id);
              for (const old of qa.querySelectorAll("script")) {
                const script = document.createElement("script");
                script.textContent = old.textContent;
                old.replaceWith(script);
              }
              // S1(f): each card's elements, to see whether old cards are
              // collected (a weak registration keeps nothing alive). Called
              // through .call so tools/mg xcode-check's route scan, which
              // reads every register( in TestHooks, does not take it for one.
              if (window.trackCards) {
                for (const element of qa.children) { finalization.register.call(finalization, element, 0); window.cardsRegistered += 1; }
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

    #if os(macOS)
    /// S1(c) on the Mac: every frame of the app's own window, through
    /// ScreenCaptureKit. `SCShareableContent.currentProcess` offers a
    /// process its own windows without the Screen Recording permission,
    /// so this runs in the test VM and on this Mac alike. Each frame
    /// reports the colour at one point of the window and when it was
    /// displayed.
    nonisolated final class WindowCapture: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
        struct Frame: Sendable {
            /// When the frame was displayed (its `displayTime`), in seconds
            /// on `CACurrentMediaTime`'s clock, and when it arrived here.
            let time: TimeInterval
            let arrived: TimeInterval
            let color: (Int, Int, Int)
            /// Whether `time` is the frame's display time.
            let displayed: Bool
        }

        struct Failure: Error, CustomStringConvertible {
            let description: String
        }

        // Set before the capture starts, read on its queue after.
        private var stream: SCStream?
        private let queue = DispatchQueue(label: "spike.windowCapture")
        /// The sampled point, in points from the window frame's top left.
        private var point = CGPoint.zero
        private var onFrame: (@MainActor @Sendable (Frame) -> Void)?
        private static let timebase: mach_timebase_info_data_t = {
            var info = mach_timebase_info_data_t()
            mach_timebase_info(&info)
            return info
        }()

        /// Starts capturing the window `windowID` (`NSWindow.windowNumber`)
        /// at its backing `scale`, sampling `point`.
        func start(
            windowID: CGWindowID, scale: CGFloat, at point: CGPoint, onFrame: @escaping @MainActor @Sendable (Frame) -> Void
        ) async throws {
            self.point = point
            self.onFrame = onFrame
            let content = try await SCShareableContent.currentProcess
            guard let target = content.windows.first(where: { $0.windowID == windowID }) else {
                throw Failure(description: "ScreenCaptureKit does not offer the app's own window (\(content.windows.count) windows offered)")
            }
            let filter = SCContentFilter(desktopIndependentWindow: target)
            let configuration = SCStreamConfiguration()
            configuration.width = Int(target.frame.width * scale)
            configuration.height = Int(target.frame.height * scale)
            configuration.minimumFrameInterval = CMTime(value: 1, timescale: 120)
            configuration.pixelFormat = kCVPixelFormatType_32BGRA
            configuration.colorSpaceName = CGColorSpace.sRGB
            configuration.showsCursor = false
            configuration.ignoreShadowsSingleWindow = true
            configuration.queueDepth = 8
            let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
            try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
            try await stream.startCapture()
            self.stream = stream
        }

        func stop() {
            stream?.stopCapture { _ in }
            stream = nil
        }

        func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
            let arrived = CACurrentMediaTime()
            guard type == .screen, sampleBuffer.isValid,
                let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
                let info = attachments.first,
                let raw = info[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
                let pixels = sampleBuffer.imageBuffer
            else { return }
            // Where the window is in the buffer, and how points map to it.
            let contentRect = (info[.contentRect] as? NSDictionary).flatMap { CGRect(dictionaryRepresentation: $0 as CFDictionary) } ?? .zero
            let contentScale = info[.contentScale] as? CGFloat ?? 1
            let scaleFactor = info[.scaleFactor] as? CGFloat ?? 1
            let x = Int((contentRect.minX + point.x * contentScale) * scaleFactor)
            let y = Int((contentRect.minY + point.y * contentScale) * scaleFactor)
            CVPixelBufferLockBaseAddress(pixels, .readOnly)
            defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
            guard let base = CVPixelBufferGetBaseAddress(pixels), x >= 0, y >= 0, x < CVPixelBufferGetWidth(pixels), y < CVPixelBufferGetHeight(pixels)
            else { return }
            let pixel = base.advanced(by: y * CVPixelBufferGetBytesPerRow(pixels) + x * 4).assumingMemoryBound(to: UInt8.self)
            // BGRA.
            let color = (Int(pixel[2]), Int(pixel[1]), Int(pixel[0]))
            var time = arrived
            var displayed = false
            if let display = info[.displayTime] as? UInt64, display > 0 {
                time = Double(display) * Double(Self.timebase.numer) / Double(Self.timebase.denom) / 1e9
                displayed = true
            }
            let frame = Frame(time: time, arrived: arrived, color: color, displayed: displayed)
            Task { @MainActor [onFrame] in onFrame?(frame) }
        }

        func stream(_ stream: SCStream, didStopWithError error: any Error) {}
    }
    #endif

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
