#if os(macOS)
import AppKit
import CardRendering
import Darwin
import Foundation
import WebKit

struct NativeReplayFailure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

/// The Mac replay protocol's synthetic runtime, hosted by the actual app.
/// Unlike offscreen snapshot/package fixtures, this publishes a native window.
@MainActor
final class NativeReplayFixture {
    static let mediaRoot = URL(string: "meguri-media://media/")!
    let configuration: WKWebViewConfiguration
    let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)
    private let log = SignalLog()
    private let media = RuntimeAssets()
    private let windowIdentifier: String
    private var pages: [Page] = []

    init(nativeAccessibilityWindowIdentifier: String) throws {
        guard Self.runsInVirtualMachine() else {
            throw NativeReplayFailure("Native accessibility fixture requires a proven virtual machine")
        }
        windowIdentifier = nativeAccessibilityWindowIdentifier
        configuration = WKWebViewConfiguration()
        // Scheme registration lasts until the owned configuration/web views are released.
        configuration.setURLSchemeHandler(media, forURLScheme: "meguri-media")
        configuration.websiteDataStore = .nonPersistent()
        configuration.mediaTypesRequiringUserActionForPlayback = []
        let controller = configuration.userContentController
        controller.addUserScript(WKUserScript(source: CardRuntime.interactionScript,
            injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        controller.add(log, contentWorld: world, name: CardRuntime.signalHandlerName)
        controller.add(log, contentWorld: world, name: CardRuntime.confirmationHandlerName)
    }

    nonisolated static func runsInVirtualMachine() -> Bool {
        var value: Int32 = 0
        var size = MemoryLayout<Int32>.size
        return sysctlbyname("kern.hv_vmm_present", &value, &size, nil, 0) == 0 && value == 1
    }

    func openPage() async throws -> Page {
        let page = Page(harness: self, identifier: windowIdentifier)
        pages.append(page) // Own it before loading can fail.
        try await page.load()
        return page
    }

    func close() {
        for page in pages { page.close() }
        pages.removeAll()
        let controller = configuration.userContentController
        controller.removeScriptMessageHandler(forName: CardRuntime.signalHandlerName, contentWorld: world)
        controller.removeScriptMessageHandler(forName: CardRuntime.confirmationHandlerName, contentWorld: world)
        controller.removeAllUserScripts()
    }

    struct Signal: @unchecked Sendable {
        let handler: String
        let body: [String: Any]
        weak var webView: WKWebView?
        var name: String { body["signal"] as? String ?? "" }
        var side: String? { body["side"] as? String }
        var card: Int64? { (body["card"] as? NSNumber)?.int64Value }
        var generation: Int64? { (body["generation"] as? NSNumber)?.int64Value }
        subscript(key: String) -> Any? { body[key] }
    }

    private final class SignalLog: NSObject, WKScriptMessageHandler {
        var received: [Signal] = []
        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            let body = (message.body as? [String: Any]) ?? ["value": message.body]
            received.append(Signal(handler: message.name, body: body, webView: message.webView))
        }
    }

    func signals(_ name: String, from page: Page? = nil) -> [Signal] {
        log.received.filter {
            $0.handler == CardRuntime.signalHandlerName && $0.name == name && (page == nil || $0.webView === page?.webView)
        }
    }

    var diagnostics: [Signal] { signals("error") }

    @discardableResult
    func signal(_ name: String, count: Int = 1, from page: Page? = nil, timeout: Duration = .seconds(10)) async throws -> Signal {
        let deadline = ContinuousClock.now + timeout
        while signals(name, from: page).count < count {
            if ContinuousClock.now > deadline { throw NativeReplayFailure("signal \(name) #\(count) did not happen within \(timeout)") }
            try await Task.sleep(for: .milliseconds(5))
        }
        return signals(name, from: page)[count - 1]
    }

    /// Only the runtime's real document and assets, all at the package
    /// fixture's origin root. No collection, media files or remote requests.
    private final class RuntimeAssets: NSObject, WKURLSchemeHandler {
        func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
            guard let url = task.request.url else {
                task.didFailWithError(URLError(.badURL))
                return
            }
            let path = url.path(percentEncoded: false)
            let data: Data
            let mime: String
            if path == "/" {
                data = Data(CardRuntime.pageDocument(platform: .mac, appearance: .light,
                    mediaRoot: NativeReplayFixture.mediaRoot, reviewExtra: "", replayLabel: CardRuntime.replayLabel).utf8)
                mime = "text/html"
            } else if path.hasPrefix(CardRuntime.assetPrefix),
                      let asset = CardRuntime.asset(at: String(path.dropFirst(CardRuntime.assetPrefix.count))) {
                data = asset.data
                mime = asset.mimeType
            } else {
                task.didReceive(HTTPURLResponse(url: url, statusCode: 404, httpVersion: "HTTP/1.1",
                    headerFields: ["Content-Type": "text/plain", "Content-Length": "0"])!)
                task.didFinish()
                return
            }
            task.didReceive(HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: [
                "Content-Type": mime, "Content-Length": String(data.count), "Accept-Ranges": "bytes",
                "Access-Control-Allow-Origin": "*", "Cache-Control": "no-store"
            ])!)
            if !data.isEmpty { task.didReceive(data) }
            task.didFinish()
        }

        func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}
    }

    @MainActor
    final class Page {
        unowned let harness: NativeReplayFixture
        let webView: WKWebView
        private let window: NSWindow
        private let navigation = Navigation()

        fileprivate init(harness: NativeReplayFixture, identifier: String) {
            // Defend creation/publication even if a future caller bypasses init.
            precondition(NativeReplayFixture.runsInVirtualMachine(), "Native accessibility window requires a proven virtual machine")
            self.harness = harness
            let size = CGSize(width: 400, height: 600)
            webView = WKWebView(frame: CGRect(origin: .zero, size: size), configuration: harness.configuration)
            webView.navigationDelegate = navigation
            webView.appearance = NSAppearance(named: .aqua)
            window = NSWindow(contentRect: CGRect(origin: .zero, size: size),
                styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
            window.setAccessibilityIdentifier(identifier)
            window.isReleasedWhenClosed = false
            window.contentView = webView
            window.center()
            window.orderFront(nil)
        }

        fileprivate func load() async throws {
            try await within("loading \(NativeReplayFixture.mediaRoot)") { finish in
                navigation.finish = finish
                webView.load(URLRequest(url: NativeReplayFixture.mediaRoot))
            }
        }

        fileprivate func close() {
            webView.stopLoading()
            webView.navigationDelegate = nil
            navigation.finish = nil
            webView.removeFromSuperview()
            window.contentView = nil
            window.close()
            // WKWebView copies its configuration; detach its handlers too.
            let controller = webView.configuration.userContentController
            controller.removeScriptMessageHandler(forName: CardRuntime.signalHandlerName, contentWorld: harness.world)
            controller.removeScriptMessageHandler(forName: CardRuntime.confirmationHandlerName, contentWorld: harness.world)
            controller.removeAllUserScripts()
        }

        @discardableResult
        func run(_ body: String, _ arguments: [String: Any] = [:]) async throws -> Any? {
            try await call(body, arguments, in: .page)
        }

        func announce(card: Int64, generation: Int64) async throws {
            _ = try await call("meguriCard.identity(card, generation);", ["card": card, "generation": generation], in: harness.world)
        }

        func present() async throws {
            _ = try await call("meguriCard.present(token);", ["token": 1], in: harness.world)
        }

        private func call(_ body: String, _ arguments: [String: Any], in world: WKContentWorld) async throws -> Any? {
            try await within("a script") { finish in
                webView.callAsyncJavaScript(body, arguments: arguments, in: nil, in: world) { result in
                    finish(result.map { $0 as Any? })
                }
            }
        }

        private struct Unchecked<T>: @unchecked Sendable { let value: T }
        private final class Once<T> {
            var continuation: CheckedContinuation<Unchecked<T>, any Error>?
            func finish(_ result: Result<T, any Error>) {
                continuation?.resume(with: result.map { Unchecked(value: $0) })
                continuation = nil
            }
        }

        private func within<T>(_ what: String, timeout: Duration = .seconds(30),
            _ start: (@escaping (Result<T, any Error>) -> Void) -> Void) async throws -> T {
            let box = Once<T>()
            let timer = Task { @MainActor in
                try await Task.sleep(for: timeout)
                box.finish(.failure(NativeReplayFailure("\(what) did not finish within \(timeout)")))
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
                finish?(.success(())); finish = nil
            }
            func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
                finish?(.failure(NativeReplayFailure("the page failed to load: \(error)"))); finish = nil
            }
            func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
                finish?(.failure(NativeReplayFailure("the page failed to load: \(error)"))); finish = nil
            }
            func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
                finish?(.failure(NativeReplayFailure("the web content process terminated"))); finish = nil
            }
        }
    }
}
#endif
