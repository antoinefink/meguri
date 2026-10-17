import AVFoundation
import CardRendering
import Foundation
import Testing
import WebKit

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Every suite that drives WebKit in this process. They share one window
/// server, the web content processes and this process's memory, and some
/// measure time: they run one test at a time (the rendering spikes nest here
/// too).
@Suite(.serialized) enum WebKitSuites {}

/// The card runtime's suites (U66, specs/render/card-runtime.md C16).
extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) enum CardRuntimeSuites {}
}

/// A card page set up as native sets it up (U67 builds the production
/// version): the document from `CardRuntime.pageDocument`, the runtime
/// assets under `/_anki/`, the interaction script in its own world with the
/// signal handlers, `cb` in the page world on iOS and iPadOS, and the web
/// view's appearance. Native's calls (`_showQuestion` by name, identity,
/// presentation, warming) go through the same channels as in the app.
@MainActor
final class WebTestHarness {
    static let mediaRoot = URL(string: "meguri-media://media/")!
    /// A second origin that is not the media origin (C7.4).
    static let remoteScheme = "meguri-remote"
    static let size = CGSize(width: 400, height: 600)

    let platform: CardRuntime.Platform
    let appearance: CardRuntime.Appearance
    let media = TestMediaServer()
    let remote = TestMediaServer()
    let configuration: WKWebViewConfiguration
    let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)
    let signals = SignalLog()
    /// Messages posted to AnkiMobile's `cb` handler (iOS and iPadOS pages).
    let ankiMobile = SignalLog()
    private(set) var pages: [TestPage] = []

    /// A harness whose pages run the runtime, or, with `runtime` false, the
    /// same skeleton and configuration without the runtime and jQuery (the
    /// C9.4 baseline). With `frameClock`, each page also counts animation
    /// frames itself (`TestPage.frameStamps()`).
    init(
        platform: CardRuntime.Platform = .mac, appearance: CardRuntime.Appearance = .light, reviewExtra: String = "",
        runtime: Bool = true, frameClock: Bool = false
    ) {
        self.platform = platform
        self.appearance = appearance
        var document = CardRuntime.pageDocument(
            platform: platform, appearance: appearance, mediaRoot: Self.mediaRoot, reviewExtra: reviewExtra)
        if !runtime {
            document = document.split(separator: "\n", omittingEmptySubsequences: false)
                .filter { !$0.hasPrefix("<script src=\"/_anki/") }
                .joined(separator: "\n")
        }
        // The document's own URL is the origin root: WebKit resolves
        // import() in inline scripts against the document URL, not the
        // base element (R520), so a page under /_anki/ would send
        // `import("./_m.js")` there.
        media.add("/", text: document, "text/html")
        for path in CardRuntime.assetPaths {
            let asset = CardRuntime.asset(at: path)!
            media.add(CardRuntime.assetPrefix + path, asset.data, asset.mimeType)
        }
        remote.holdsEverything = true

        let configuration = WKWebViewConfiguration()
        configuration.setURLSchemeHandler(media, forURLScheme: "meguri-media")
        configuration.setURLSchemeHandler(remote, forURLScheme: Self.remoteScheme)
        configuration.websiteDataStore = .nonPersistent()
        configuration.mediaTypesRequiringUserActionForPlayback = []
        #if !os(macOS)
        configuration.allowsInlineMediaPlayback = true
        #endif
        let controller = configuration.userContentController
        if frameClock {
            controller.addUserScript(
                WKUserScript(source: Self.frameClockScript, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        }
        controller.addUserScript(
            WKUserScript(source: CardRuntime.interactionScript, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        controller.add(signals, contentWorld: world, name: CardRuntime.signalHandlerName)
        controller.add(signals, contentWorld: world, name: CardRuntime.confirmationHandlerName)
        if platform != .mac {
            controller.add(ankiMobile, contentWorld: .page, name: CardRuntime.ankiMobileHandlerName)
        }
        self.configuration = configuration
    }

    /// A new page with this harness's configuration, loaded and past its
    /// `load` event (C2.4).
    func openPage(size: CGSize = WebTestHarness.size) async throws -> TestPage {
        let page = TestPage(harness: self, size: size)
        pages.append(page)
        try await page.load(Self.mediaRoot)
        return page
    }

    /// Closes every page; tests call it in a `defer`.
    func close() {
        for page in pages { page.close() }
        pages.removeAll()
    }

    /// The test's own frame clock, in the interaction world: a
    /// `requestAnimationFrame` loop that counts frames from document start,
    /// and the count at each moment a test compares: `#qa` turning visible
    /// (step 8), each message the interaction script posts, and the marks a
    /// test sets. It relies on nothing the runtime or the interaction script
    /// says about frames.
    ///
    /// The wrapped handlers stay reachable from `meguriTestFrames`: WebKit
    /// drops an unreachable handler object at a collection and gives the
    /// interaction script a fresh one without the wrapper, so its posts
    /// would go unstamped.
    static let frameClockScript = """
        (function () {
          "use strict";
          let frames = 0;
          const stamps = [];
          const handlers = [];
          function tick() {
            requestAnimationFrame(function () {
              frames += 1;
              tick();
            });
          }
          tick();
          function wrap(name) {
            const handler = window.webkit.messageHandlers[name];
            const post = handler.postMessage;
            handler.postMessage = function (message) {
              const what = message && (message.signal || message.phase);
              stamps.push({ event: name + ":" + String(what), frame: frames });
              return post.call(handler, message);
            };
            handlers.push(handler);
          }
          wrap("\(CardRuntime.signalHandlerName)");
          wrap("\(CardRuntime.confirmationHandlerName)");
          let qaVisible = false;
          new MutationObserver(function () {
            const qa = document.getElementById("qa");
            const visible = !!qa && qa.style.opacity === "1";
            if (visible && !qaVisible) stamps.push({ event: "qaVisible", frame: frames });
            qaVisible = visible;
          }).observe(document, { subtree: true, attributes: true, attributeFilter: ["style"] });
          window.meguriTestFrames = {
            stamps: stamps,
            handlers: handlers,
            mark: function (event) {
              stamps.push({ event: event, frame: frames });
            },
          };
        })();
        """

    // MARK: Signals

    /// The `meguriCard` signals named `name` that `page` sent, in order.
    func signals(_ name: String, from page: TestPage? = nil) -> [Signal] {
        signals.received.filter {
            $0.handler == CardRuntime.signalHandlerName && $0.name == name && (page == nil || $0.webView === page?.webView)
        }
    }

    /// Waits for the `count`th `meguriCard` signal named `name` from `page`
    /// (any page when nil) and returns it.
    @discardableResult
    func signal(_ name: String, count: Int = 1, from page: TestPage? = nil, timeout: Duration = .seconds(10)) async throws -> Signal {
        try await until("signal \(name) #\(count)", timeout: timeout) { self.signals(name, from: page).count >= count }
        return signals(name, from: page)[count - 1]
    }

    /// Polls `condition` every 5 ms until it holds, failing after `timeout`.
    func until(_ what: String, timeout: Duration = .seconds(10), _ condition: () async throws -> Bool) async throws {
        let deadline = ContinuousClock.now + timeout
        while !(try await condition()) {
            if ContinuousClock.now > deadline { throw HarnessFailure("\(what) did not happen within \(timeout)") }
            try await Task.sleep(for: .milliseconds(5))
        }
    }
}

/// Whether this process runs in a virtual machine, such as the test VM
/// and its simulators (`kern.hv_vmm_present`).
nonisolated func runsInVirtualMachine() -> Bool {
    var value: Int32 = 0
    var size = MemoryLayout<Int32>.size
    return sysctlbyname("kern.hv_vmm_present", &value, &size, nil, 0) == 0 && value == 1
}

/// Resumes a continuation once, with whichever result comes first.
@MainActor
private final class ContinuationBox<T> {
    var continuation: CheckedContinuation<Unchecked<T>, any Error>?

    func finish(_ result: Result<T, any Error>) {
        continuation?.resume(with: result.map { Unchecked(value: $0) })
        continuation = nil
    }
}

/// WebKit's results (`Any`) moved across the continuation on the main actor.
private struct Unchecked<T>: @unchecked Sendable {
    let value: T
}

struct HarnessFailure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

/// One message to a script message handler.
struct Signal: @unchecked Sendable {
    let handler: String
    let body: [String: Any]
    /// When it arrived, in seconds of system uptime.
    let time: TimeInterval
    weak var webView: WKWebView?

    /// `body["signal"]` for `meguriCard` messages.
    var name: String { body["signal"] as? String ?? "" }
    var side: String? { body["side"] as? String }
    var card: Int64? { (body["card"] as? NSNumber)?.int64Value }
    var generation: Int64? { (body["generation"] as? NSNumber)?.int64Value }
    subscript(key: String) -> Any? { body[key] }
}

/// Collects script messages as they arrive.
@MainActor
final class SignalLog: NSObject, WKScriptMessageHandler {
    private(set) var received: [Signal] = []

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        let body = (message.body as? [String: Any]) ?? ["value": message.body]
        received.append(
            Signal(handler: message.name, body: body, time: ProcessInfo.processInfo.systemUptime, webView: message.webView))
    }

    /// Messages to `handler`.
    func messages(to handler: String) -> [Signal] { received.filter { $0.handler == handler } }
}

/// One card page: a web view in a window of its own.
@MainActor
final class TestPage {
    unowned let harness: WebTestHarness
    let webView: WKWebView
    private let window: TestWindow
    private let navigation = Navigation()

    init(harness: WebTestHarness, size: CGSize) {
        self.harness = harness
        webView = WKWebView(frame: CGRect(origin: .zero, size: size), configuration: harness.configuration)
        webView.navigationDelegate = navigation
        // Native sets the web view's appearance, which is what
        // prefers-color-scheme follows (R489, C5).
        #if os(macOS)
        webView.appearance = NSAppearance(named: harness.appearance == .dark ? .darkAqua : .aqua)
        #else
        webView.overrideUserInterfaceStyle = harness.appearance == .dark ? .dark : .light
        #endif
        window = TestWindow(webView)
    }

    func load(_ url: URL) async throws {
        try await within("loading \(url)") { finish in
            navigation.finish = finish
            webView.load(URLRequest(url: url))
        }
    }

    func close() { window.close() }

    // MARK: Scripts

    /// Runs `body` as an async function in the page world and returns its
    /// result.
    @discardableResult
    func run(_ body: String, _ arguments: [String: Any] = [:]) async throws -> Any? {
        try await call(body, arguments, in: .page)
    }

    /// Runs `body` in the interaction script's world.
    @discardableResult
    func runIsolated(_ body: String, _ arguments: [String: Any] = [:]) async throws -> Any? {
        try await call(body, arguments, in: harness.world)
    }

    /// Runs `body`, which returns `JSON.stringify(...)`, and decodes it.
    func json<T: Decodable>(_ type: T.Type, _ body: String, _ arguments: [String: Any] = [:]) async throws -> T {
        let value = try await run(body, arguments)
        guard let text = value as? String else { throw HarnessFailure("the page returned \(String(describing: value)), not JSON") }
        do {
            return try JSONDecoder().decode(T.self, from: Data(text.utf8))
        } catch {
            throw HarnessFailure("the page returned \(text), not a \(T.self): \(error)")
        }
    }

    /// The string value of a page-world expression.
    func string(_ expression: String) async throws -> String {
        let value = try await run("return String(\(expression));")
        return value as? String ?? ""
    }

    /// The boolean value of a page-world expression.
    func bool(_ expression: String) async throws -> Bool {
        try await run("return Boolean(\(expression));") as? Bool ?? false
    }

    private func call(_ body: String, _ arguments: [String: Any], in world: WKContentWorld) async throws -> Any? {
        try await within("a script") { finish in
            webView.callAsyncJavaScript(body, arguments: arguments, in: nil, in: world) { result in
                finish(result.map { $0 as Any? })
            }
        }
    }

    // MARK: The log cases write to

    /// Defines `window.__log` and `log(x)` (C16), which append to it.
    func installLog() async throws {
        try await run("window.__log = []; window.log = function (x) { window.__log.push(x); };")
    }

    /// The log, each value converted with `String`.
    func log() async throws -> [String] {
        try await json([String].self, "return JSON.stringify((window.__log || []).map(String));")
    }

    // MARK: Native's calls (U67)

    /// `_showQuestion(q, a, bodyclass)`, called by name in the page world.
    func showQuestion(_ question: String, answer: String = "", bodyClass: String = "card card1") async throws {
        try await run("_showQuestion(q, a, b);", ["q": question, "a": answer, "b": bodyClass])
    }

    /// `_showAnswer(a)` or `_showAnswer(a, bodyclass)`.
    func showAnswer(_ answer: String, bodyClass: String? = nil) async throws {
        if let bodyClass {
            try await run("_showAnswer(a, b);", ["a": answer, "b": bodyClass])
        } else {
            try await run("_showAnswer(a);", ["a": answer])
        }
    }

    /// Announces the identity of the next queued update (C6.8).
    func announce(card: Int64, generation: Int64) async throws {
        try await runIsolated("meguriCard.identity(card, generation);", ["card": card, "generation": generation])
    }

    /// Marks the page presented (C6.6); `token` is the swap's (U12).
    func present(token: Int = 1) async throws {
        try await runIsolated("meguriCard.present(token);", ["token": token])
    }

    func unpresent() async throws {
        try await runIsolated("meguriCard.unpresent();")
    }

    // MARK: The frame clock (`frameClock` harnesses)

    struct FrameStamp: Decodable, Equatable {
        let event: String
        let frame: Int
    }

    /// Everything the frame clock stamped, in order.
    func frameStamps() async throws -> [FrameStamp] {
        let text = try await runIsolated("return JSON.stringify(window.meguriTestFrames.stamps);")
        guard let text = text as? String else { throw HarnessFailure("this page has no frame clock") }
        return try JSONDecoder().decode([FrameStamp].self, from: Data(text.utf8))
    }

    /// The frame count of the first stamp named `event`.
    func frame(of event: String) async throws -> Int {
        let stamps = try await frameStamps()
        guard let stamp = stamps.first(where: { $0.event == event }) else {
            throw HarnessFailure("no frame stamp \(event) in \(stamps)")
        }
        return stamp.frame
    }

    /// `present(token:)`, stamped `present` in the same task.
    func presentStamped(token: Int = 1) async throws {
        try await runIsolated("meguriTestFrames.mark('present'); meguriCard.present(token);", ["token": token])
    }

    /// Asks the page to warm its caches with `html` (C7.5).
    func warm(_ html: String) async throws {
        try await runIsolated("meguriCard.warm(html);", ["html": html])
    }

    /// The listener registry's answer for `types` (`"tap"`, `"key"` or one
    /// event type) aimed at the element `selector` finds, or at the focused
    /// element (`<body>` when none) without a selector (C13).
    func hasCardListener(_ types: String, at selector: String? = nil) async throws -> Bool {
        try await runIsolated(
            "return meguriCard.hasCardListener(types, selector === null ? null : document.querySelector(selector));",
            ["types": types, "selector": selector as Any? ?? NSNull()]) as? Bool ?? true
    }

    // MARK: Waiting

    /// Waits for a WebKit callback, failing after `timeout` instead of
    /// hanging the run.
    private func within<T>(_ what: String, timeout: Duration = .seconds(30), _ start: (@escaping (Result<T, any Error>) -> Void) -> Void)
        async throws -> T
    {
        let box = ContinuationBox<T>()
        let timer = Task { @MainActor in
            try await Task.sleep(for: timeout)
            box.finish(.failure(HarnessFailure("\(what) did not finish within \(timeout)")))
        }
        defer { timer.cancel() }
        let result = try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Unchecked<T>, any Error>) in
            box.continuation = continuation
            start { box.finish($0) }
        }
        return result.value
    }

    private final class Navigation: NSObject, WKNavigationDelegate {
        var finish: ((Result<Void, any Error>) -> Void)?

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            finish?(.success(()))
            finish = nil
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
            finish?(.failure(HarnessFailure("the page failed to load: \(error)")))
            finish = nil
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
            finish?(.failure(HarnessFailure("the page failed to load: \(error)")))
            finish = nil
        }

        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            finish?(.failure(HarnessFailure("the web content process terminated")))
            finish = nil
        }
    }
}

#if os(macOS)
/// A borderless window that is never ordered in, so nothing reaches this
/// Mac's screen (root AGENTS.md), but that tells WebKit it is visible and
/// unoccluded. WebKit then treats its pages as visible: they run animation
/// frames (the settle and visible confirmations need them, C6.6, C14) and
/// `document.visibilityState` is `visible`, as for the occluded page in the
/// app (S1).
private final class ReportedVisibleWindow: NSWindow {
    override var isVisible: Bool { true }
    override var occlusionState: NSWindow.OcclusionState { .visible }
}
#endif

/// The window a test page lives in.
@MainActor
private final class TestWindow {
    #if os(macOS)
    private let window: NSWindow
    #else
    /// Nil without a window scene: a UIWindow needs one (`init(frame:)` is
    /// deprecated since iOS 26), and only a run with an app host has one.
    private let window: UIWindow?
    #endif

    init(_ content: WKWebView) {
        #if os(macOS)
        window = ReportedVisibleWindow(
            contentRect: CGRect(origin: CGPoint(x: -30_000, y: -30_000), size: content.frame.size),
            styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = content
        #else
        window = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first.map { UIWindow(windowScene: $0) }
        guard let window else { return }
        window.frame = content.frame
        let controller = UIViewController()
        controller.view = content
        window.rootViewController = controller
        window.isHidden = false
        #endif
    }

    func close() {
        #if os(macOS)
        window.contentView = nil
        window.close()
        #else
        window?.isHidden = true
        window?.rootViewController = nil
        #endif
    }
}

/// A `meguri-media:` scheme handler for tests: files from memory, a delay or
/// no answer at all per path, single `Range` requests answered with 206, and
/// a log of every request.
@MainActor
final class TestMediaServer: NSObject, WKURLSchemeHandler {
    struct File {
        let data: Data
        let mimeType: String
    }

    struct Request {
        let path: String
        /// Seconds of system uptime.
        let time: TimeInterval
    }

    private var files: [String: File] = [:]
    private var delays: [String: Duration] = [:]
    private var held: Set<String> = []
    /// Answers nothing at all (C7.4's remote origin).
    var holdsEverything = false
    private(set) var requests: [Request] = []
    private var active: Set<ObjectIdentifier> = []

    func add(_ path: String, _ data: Data, _ mimeType: String) {
        files[path] = File(data: data, mimeType: mimeType)
    }

    func add(_ path: String, text: String, _ mimeType: String = "text/javascript") {
        add(path, Data(text.utf8), mimeType)
    }

    /// Answers `path` only after `delay`.
    func delay(_ path: String, by delay: Duration) { delays[path] = delay }

    /// Never answers `path`.
    func hold(_ path: String) { held.insert(path) }

    func requests(for path: String) -> [Request] { requests.filter { $0.path == path } }

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url else {
            task.didFailWithError(URLError(.badURL))
            return
        }
        let path = url.path(percentEncoded: false)
        requests.append(Request(path: path, time: ProcessInfo.processInfo.systemUptime))
        let id = ObjectIdentifier(task)
        active.insert(id)
        if holdsEverything || held.contains(path) { return }
        let range = task.request.value(forHTTPHeaderField: "Range")
        if let delay = delays[path] {
            Task { @MainActor in
                try? await Task.sleep(for: delay)
                self.respond(task, url: url, path: path, range: range)
            }
        } else {
            respond(task, url: url, path: path, range: range)
        }
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {
        active.remove(ObjectIdentifier(task))
    }

    private func respond(_ task: any WKURLSchemeTask, url: URL, path: String, range: String?) {
        // A stopped task must never be answered: WebKit raises an exception.
        guard active.remove(ObjectIdentifier(task)) != nil else { return }
        guard let file = files[path] else {
            send(task, url: url, status: 404, headers: ["Content-Type": "text/plain"], body: Data())
            return
        }
        var headers = [
            "Content-Type": file.mimeType,
            "Accept-Ranges": "bytes",
            // Fonts and module scripts are fetched in CORS mode.
            "Access-Control-Allow-Origin": "*",
            "Cache-Control": "no-store",
        ]
        let total = file.data.count
        if let range, let match = range.wholeMatch(of: /bytes=(\d*)-(\d*)/), total > 0 {
            var first = Int(match.1) ?? 0
            var last = Int(match.2) ?? (total - 1)
            if match.1.isEmpty, let suffix = Int(match.2) {
                first = max(0, total - suffix)
                last = total - 1
            }
            last = min(last, total - 1)
            if first <= last {
                headers["Content-Range"] = "bytes \(first)-\(last)/\(total)"
                send(task, url: url, status: 206, headers: headers, body: file.data.subdata(in: first..<(last + 1)))
                return
            }
        }
        send(task, url: url, status: 200, headers: headers, body: file.data)
    }

    private func send(_ task: any WKURLSchemeTask, url: URL, status: Int, headers: [String: String], body: Data) {
        var headers = headers
        headers["Content-Length"] = String(body.count)
        task.didReceive(HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!)
        if !body.isEmpty { task.didReceive(body) }
        task.didFinish()
    }
}

/// Media made at run time: no binary fixture is committed.
enum TestMedia {
    /// A PNG of `width` × `height` pixels in one colour.
    static func png(width: Int = 4, height: Int = 4) -> Data {
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.setFillColor(CGColor(red: 0.2, green: 0.4, blue: 0.6, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        let data = NSMutableData()
        let destination = CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, context.makeImage()!, nil)
        CGImageDestinationFinalize(destination)
        return data as Data
    }

    /// A silent 16-bit mono PCM WAV file of `seconds`: never audible on the
    /// host.
    static func silentWAV(seconds: Double = 2) -> Data {
        let rate = 8_000
        let samples = Int(Double(rate) * seconds)
        var data = Data()
        func append<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) } }
        data.append(contentsOf: Array("RIFF".utf8))
        append(UInt32(36 + samples * 2))
        data.append(contentsOf: Array("WAVEfmt ".utf8))
        append(UInt32(16))
        append(UInt16(1))
        append(UInt16(1))
        append(UInt32(rate))
        append(UInt32(rate * 2))
        append(UInt16(2))
        append(UInt16(16))
        data.append(contentsOf: Array("data".utf8))
        append(UInt32(samples * 2))
        data.append(Data(count: samples * 2))
        return data
    }

    /// An H.264 MP4 of `seconds` seconds, `fps` frames a second, each second
    /// a different grey, with its index at the front of the file (fast
    /// start), as a card's `<video>` would be. It has no audio track.
    @concurrent nonisolated static func mp4(seconds: Int = 20, fps: Int = 10, size: CGSize = CGSize(width: 320, height: 240)) async throws -> Data {
        let url = FileManager.default.temporaryDirectory.appending(path: "test-media-\(UUID().uuidString).mp4")
        defer { try? FileManager.default.removeItem(at: url) }
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        writer.shouldOptimizeForNetworkUse = true
        let input = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: Int(size.width), AVVideoHeightKey: Int(size.height)])
        let attributes = CVPixelBufferCreationAttributes(
            pixelFormatType: CVPixelFormatType(rawValue: kCVPixelFormatType_32BGRA), size: CVImageSize(width: Int(size.width), height: Int(size.height)))
        let receiver = writer.inputPixelBufferReceiver(for: input, pixelBufferAttributes: attributes)
        try writer.start()
        writer.startSession(atSourceTime: .zero)
        for index in 0..<(seconds * fps) {
            let frame = try CVMutablePixelBuffer(attributes)
            let grey = Int32(truncatingIfNeeded: 40 + (index / fps) * 9)
            frame.withUnsafeBuffer { buffer in
                CVPixelBufferLockBaseAddress(buffer, [])
                memset(CVPixelBufferGetBaseAddress(buffer), grey, CVPixelBufferGetDataSize(buffer))
                CVPixelBufferUnlockBaseAddress(buffer, [])
            }
            // `append` sometimes never resumed (a hung run): poll instead.
            let ready = CVReadOnlyPixelBuffer(frame)
            let time = CMTime(value: CMTimeValue(index), timescale: CMTimeScale(fps))
            let deadline = ContinuousClock.now + .seconds(10)
            while try !receiver.appendImmediately(ready, with: time) {
                guard ContinuousClock.now < deadline else { throw HarnessFailure("the MP4 writer took no frame \(index) for 10 s") }
                try await Task.sleep(for: .milliseconds(2))
            }
        }
        receiver.finish()
        await writer.finishWriting()
        guard writer.status == .completed else { throw HarnessFailure("the MP4 did not finish: \(String(describing: writer.error))") }
        return try Data(contentsOf: url)
    }

    /// A TrueType font file of a system face, to serve as a card's
    /// `@font-face` source.
    static func font() -> Data {
        let font = CTFontCreateWithName("Helvetica" as CFString, 12, nil)
        // The array holds the tags themselves, not objects.
        let array = CTFontCopyAvailableTables(font, [])!
        let tags = (0..<CFArrayGetCount(array)).map { CTFontTableTag(UInt(bitPattern: CFArrayGetValueAtIndex(array, $0))) }
        var tables: [(CTFontTableTag, Data)] = []
        for tag in tags {
            if let table = CTFontCopyTable(font, tag, []) { tables.append((tag, table as Data)) }
        }
        return sfnt(tables)
    }

    /// An sfnt file from its tables.
    private static func sfnt(_ tables: [(CTFontTableTag, Data)]) -> Data {
        let sorted = tables.sorted { $0.0 < $1.0 }
        var data = Data()
        func append<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.bigEndian) { data.append(contentsOf: $0) } }
        let count = UInt16(sorted.count)
        var power = 1
        var log = 0
        while power * 2 <= Int(count) {
            power *= 2
            log += 1
        }
        append(UInt32(0x0001_0000))
        append(count)
        append(UInt16(power * 16))
        append(UInt16(log))
        append(UInt16(Int(count) * 16 - power * 16))
        var offset = 12 + 16 * sorted.count
        var bodies = Data()
        for (tag, table) in sorted {
            var sum: UInt32 = 0
            var padded = table
            while padded.count % 4 != 0 { padded.append(0) }
            padded.withUnsafeBytes { raw in
                for index in stride(from: 0, to: raw.count, by: 4) {
                    sum = sum &+ UInt32(bigEndian: raw.loadUnaligned(fromByteOffset: index, as: UInt32.self))
                }
            }
            append(UInt32(tag))
            append(sum)
            append(UInt32(offset))
            append(UInt32(table.count))
            bodies.append(padded)
            offset += padded.count
        }
        data.append(bodies)
        return data
    }
}
