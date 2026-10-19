import Foundation
import Synchronization
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Throwaway web view support for the rendering spikes S1 and S2 (U65).
/// U67's `CardPagePair`, `CardWebConfiguration` and `MediaSchemeHandler`
/// replace it; the spike tests then run against those.
enum Spike {
    static let scheme = "meguri-media"
    /// The origin form S2(f) chose: the scheme with a fixed host.
    static let host = "media"
    static let origin = URL(string: "\(scheme)://\(host)/")!
    static let timeout: Duration = .seconds(60)

    struct Failure: Error, CustomStringConvertible {
        let description: String
        init(_ description: String) { self.description = description }
    }

    /// A configuration for card pages: the scheme handler, no user action
    /// needed to play media (R560), `store` (non-persistent by default).
    static func configuration(
        handler: SpikeMediaHandler, scheme: String = scheme, store: WKWebsiteDataStore = .nonPersistent()
    ) -> WKWebViewConfiguration {
        let configuration = WKWebViewConfiguration()
        configuration.setURLSchemeHandler(handler, forURLScheme: scheme)
        configuration.websiteDataStore = store
        configuration.mediaTypesRequiringUserActionForPlayback = []
        #if !os(macOS)
        configuration.allowsInlineMediaPlayback = true
        #endif
        return configuration
    }

    /// Waits for a WebKit callback, failing after `timeout` instead of
    /// hanging the run.
    static func within<T: Sendable>(_ what: String, timeout: Duration = timeout, _ start: (@escaping (Result<T, any Error>) -> Void) -> Void)
        async throws -> T
    {
        let once = Once<T>()
        let timer = Task { @MainActor in
            try await Task.sleep(for: timeout)
            once.finish(.failure(Failure("\(what) did not finish within \(timeout)")))
        }
        defer { timer.cancel() }
        return try await withCheckedThrowingContinuation { continuation in
            once.continuation = continuation
            start { once.finish($0) }
        }
    }

    /// Runs `body`, failing after `timeout` instead of hanging the run (the
    /// body itself keeps running).
    static func deadline<T: Sendable>(_ what: String, timeout: Duration = timeout, _ body: @escaping @Sendable () async throws -> T)
        async throws -> T
    {
        try await within(what, timeout: timeout) { finish in
            Task { @MainActor in
                do { finish(.success(try await body())) } catch { finish(.failure(error)) }
            }
        }
    }

    /// A continuation resumed by whichever comes first.
    private final class Once<T: Sendable> {
        var continuation: CheckedContinuation<T, any Error>?

        func finish(_ result: Result<T, any Error>) {
            continuation?.resume(with: result)
            continuation = nil
        }
    }

    /// Seconds on the clock input events and frame times use.
    static var now: TimeInterval { ProcessInfo.processInfo.systemUptime }
}

/// A card page: a web view that counts its live instances and every move
/// into or out of a window (S1(f): a swap must never reparent a page).
final class SpikeWebView: WKWebView {
    /// Pages created in this process so far.
    nonisolated static let created = Atomic<Int>(0)

    private(set) var windowMoves = 0
    private let navigation = SpikeNavigation()

    override init(frame: CGRect, configuration: WKWebViewConfiguration) {
        Self.created.add(1, ordering: .relaxed)
        super.init(frame: frame, configuration: configuration)
        navigationDelegate = navigation
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

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

    /// Loads `url` and waits for the navigation to finish.
    func open(_ url: URL) async throws {
        try await Spike.within("loading \(url)") { finish in
            navigation.finish = finish
            load(URLRequest(url: url))
        }
    }

    /// Loads `html` as a document at `baseURL` and waits for it to finish.
    func open(html: String, baseURL: URL) async throws {
        try await Spike.within("loading an HTML string at \(baseURL)") { finish in
            navigation.finish = finish
            loadHTMLString(html, baseURL: baseURL)
        }
    }

    /// Runs `body` as an async function in `world` and returns its result,
    /// which must be a JSON string (`return JSON.stringify(...)`), decoded.
    func json<T: Decodable>(
        _ type: T.Type, _ body: String, arguments: [String: Any] = [:], in world: WKContentWorld = .page
    ) async throws -> T {
        let text = try await string(body, arguments: arguments, in: world)
        do {
            return try JSONDecoder().decode(T.self, from: Data(text.utf8))
        } catch {
            throw Spike.Failure("the page returned \(text), not a \(T.self): \(error)")
        }
    }

    /// Runs `body` and returns the string it returns.
    func string(_ body: String, arguments: [String: Any] = [:], in world: WKContentWorld = .page) async throws -> String {
        try await Spike.within("a script") { finish in
            callAsyncJavaScript(body, arguments: arguments, in: nil, in: world) { result in
                finish(
                    result.flatMap { value in
                        guard let text = value as? String else {
                            return .failure(Spike.Failure("the script returned \(String(describing: value)), not a string"))
                        }
                        return .success(text)
                    })
            }
        }
    }

    /// Runs `body` and discards its result.
    func run(_ body: String, arguments: [String: Any] = [:], in world: WKContentWorld = .page) async throws {
        _ = try await Spike.within("a script") { (finish: @escaping (Result<Void, any Error>) -> Void) in
            callAsyncJavaScript(body, arguments: arguments, in: nil, in: world) { result in
                finish(result.map { _ in () })
            }
        }
    }
}

/// Finishes the pending load of its web view.
final class SpikeNavigation: NSObject, WKNavigationDelegate {
    var finish: ((Result<Void, any Error>) -> Void)?
    private(set) var terminations = 0

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        finish?(.success(()))
        finish = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        finish?(.failure(Spike.Failure("the page failed to load: \(error)")))
        finish = nil
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        finish?(.failure(Spike.Failure("the page failed to load: \(error)")))
        finish = nil
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        terminations += 1
        finish?(.failure(Spike.Failure("the web content process terminated")))
        finish = nil
    }
}

/// Collects script messages with the time they arrived.
final class SpikeMessages: NSObject, WKScriptMessageHandler {
    struct Received {
        let name: String
        let body: Any
        let time: TimeInterval
        weak var webView: WKWebView?
    }

    private(set) var received: [Received] = []
    var onReceive: ((Received) -> Void)?

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        let received = Received(name: message.name, body: message.body, time: Spike.now, webView: message.webView)
        self.received.append(received)
        onReceive?(received)
    }
}

/// Two card pages that share one configuration, stacked full-frame and
/// opaque in one container, the idle page beneath (KTD11, S1). `swap()`
/// changes their order in place: neither page leaves the container or its
/// window.
final class SpikePagePair {
    let container: PlatformView
    private(set) var front: SpikeWebView
    private(set) var back: SpikeWebView

    init(configuration: WKWebViewConfiguration, size: CGSize) {
        let frame = CGRect(origin: .zero, size: size)
        container = PlatformView(frame: frame)
        back = SpikeWebView(frame: frame, configuration: configuration)
        front = SpikeWebView(frame: frame, configuration: configuration)
        for page in [back, front] {
            #if os(macOS)
            page.autoresizingMask = [.width, .height]
            #else
            page.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            page.isOpaque = true
            #endif
            // Bottom first: `front` ends on top.
            container.addSubview(page)
        }
    }

    /// Brings the idle page to the front.
    func swap() {
        (front, back) = (back, front)
        #if os(macOS)
        let target = Unmanaged.passUnretained(front).toOpaque()
        container.sortSubviews(
            { first, second, context in
                // The page in `context` sorts last, so it is drawn on top.
                let top = context.map { Unmanaged<NSView>.fromOpaque($0).takeUnretainedValue() }
                if first === top { return .orderedDescending }
                if second === top { return .orderedAscending }
                return .orderedSame
            }, context: target)
        #else
        container.bringSubviewToFront(front)
        #endif
    }

    /// The page drawn on top: the container's last subview.
    var topmost: PlatformView? { container.subviews.last }
}

#if os(macOS)
typealias PlatformView = NSView
#else
typealias PlatformView = UIView
#endif

/// A window for spike pages. On the Mac it is borderless, off-screen and
/// never ordered in, so a run never shows anything on this Mac's screen
/// (root AGENTS.md); WebKit therefore runs no animation frames in it, and
/// the S1 checks that need them run in the app (CardSwapTimingSpikeTests).
final class SpikeWindow {
    #if os(macOS)
    private let window: NSWindow
    #else
    /// Nil without a window scene: a UIWindow needs one (`init(frame:)` is
    /// deprecated since iOS 26), and only a run with an app host has one.
    private let window: UIWindow?
    #endif

    init(_ content: PlatformView) {
        #if os(macOS)
        window = NSWindow(
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

/// The memory footprint of the web content processes that render `pages`,
/// in bytes: R1853 counts what the pages hold there (script heaps, DOM,
/// decoded images) as well as this process. Nil when a page has no process
/// or a process's footprint cannot be read.
///
/// WebKit has no public way to name a page's process: the pid comes from
/// `WKWebView`'s `_webProcessIdentifier` (SPI, read in test code only), and
/// its footprint from `proc_pid_rusage`.
func spikeWebContentFootprint(_ pages: [WKWebView]) -> (processes: Int, bytes: Int)? {
    let selector = NSSelectorFromString("_webProcessIdentifier")
    var pids: Set<pid_t> = []
    for page in pages {
        guard page.responds(to: selector), let pid = (page.value(forKey: "_webProcessIdentifier") as? NSNumber)?.int32Value, pid > 0 else {
            return nil
        }
        pids.insert(pid)
    }
    var bytes = 0
    for pid in pids {
        guard let footprint = spikeFootprint(pid: pid) else { return nil }
        bytes += footprint
    }
    return (pids.count, bytes)
}

/// Another process's memory footprint, in bytes, or nil when the system
/// does not tell.
func spikeFootprint(pid: pid_t) -> Int? {
    #if os(macOS)
    var usage = rusage_info_v4()
    let result = withUnsafeMutablePointer(to: &usage) { pointer in
        pointer.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
    }
    return result == 0 ? Int(usage.ri_phys_footprint) : nil
    #else
    // The iOS SDK has no proc_pid_rusage.
    return nil
    #endif
}

/// This process's memory footprint (what Xcode and jetsam count), in bytes.
func spikeFootprint() -> Int {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let result = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return result == KERN_SUCCESS ? Int(info.phys_footprint) : 0
}

/// Percentile of `values` (nearest rank).
func spikePercentile(_ values: [Double], _ p: Double) -> Double {
    guard !values.isEmpty else { return .nan }
    let sorted = values.sorted()
    let rank = Int((p / 100 * Double(sorted.count)).rounded(.up))
    return sorted[min(max(rank, 1), sorted.count) - 1]
}
