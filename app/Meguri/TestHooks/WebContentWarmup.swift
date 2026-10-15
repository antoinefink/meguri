#if MEGURI_TEST_HOOKS
import Foundation
import WebKit
#if !os(macOS)
import notify
#endif

extension TestHooks {
    /// The warmed web content process R1866 asks of performance tests (U12).
    /// On the notification `<bundle id>.testhooks.warmWebContent` (Darwin on
    /// iOS, distributed on the Mac) the app loads a page into a web view it
    /// keeps for the rest of its run, waits until the page has loaded and its
    /// web content process has answered a script, then posts
    /// `<bundle id>.testhooks.webContentReady`. Later requests check that the
    /// same process still answers; one that terminated is replaced. Without
    /// a ready process the app posts nothing, and `PerfTestCase` fails before
    /// measuring.
    ///
    /// Until CardRendering keeps the review page pair (U67), this page is the
    /// app's only web content. U67 makes the warm-up load its pages through
    /// the pair's shared configuration, so that the process warmed is the
    /// one the reviewer renders in.
    final class WebContentWarmup: NSObject, WKNavigationDelegate {
        static let shared = WebContentWarmup()

        static var requestName: String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.warmWebContent" }
        static var readyName: String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.webContentReady" }

        /// A page with script, layout and text: the process parses, lays out
        /// and runs JavaScript before it answers.
        static let page = """
            <!doctype html><html><head><meta charset="utf-8">
            <meta name="viewport" content="width=device-width"></head>
            <body><p id="warm">Meguri</p><script>document.body.dataset.warm = "1";</script></body></html>
            """

        private var webView: WKWebView?
        private var loading: CheckedContinuation<Bool, Never>?
        private var current: Task<Bool, Never>?
        private var listening = false
        #if os(macOS)
        private var observer: NSObjectProtocol?
        #else
        private var token: Int32 = NOTIFY_TOKEN_INVALID
        #endif

        /// Answers warm-up requests from now on; a second call does nothing.
        func listen() {
            guard !listening else { return }
            listening = true
            #if os(macOS)
            observer = DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name(Self.requestName), object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.requested() }
            }
            #else
            notify_register_dispatch(Self.requestName, &token, .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.requested() }
            }
            #endif
        }

        private func requested() {
            Task {
                guard await warm() else { return }
                #if os(macOS)
                // Delivered at once: the test runner is usually not active.
                DistributedNotificationCenter.default().postNotificationName(
                    Notification.Name(Self.readyName), object: nil, userInfo: nil, deliverImmediately: true)
                #else
                notify_post(Self.readyName)
                #endif
            }
        }

        /// Whether a web content process is loaded and answering, after
        /// starting one if needed. Concurrent requests share one warm-up.
        func warm() async -> Bool {
            if let current {
                return await current.value
            }
            let task = Task { await warmOnce() }
            current = task
            let ready = await task.value
            current = nil
            return ready
        }

        private func warmOnce() async -> Bool {
            if let webView, await answers(webView) {
                return true
            }
            let webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 480), configuration: WKWebViewConfiguration())
            webView.navigationDelegate = self
            self.webView = webView
            let loaded = await withCheckedContinuation { continuation in
                loading = continuation
                webView.loadHTMLString(Self.page, baseURL: nil)
            }
            guard loaded, await answers(webView) else {
                if self.webView === webView { self.webView = nil }
                return false
            }
            return true
        }

        /// A round trip through the page's web content process.
        private func answers(_ webView: WKWebView) async -> Bool {
            let warm = try? await webView.evaluateJavaScript("document.readyState + ':' + document.body.dataset.warm")
            return warm as? String == "complete:1"
        }

        private func finishLoading(_ loaded: Bool) {
            loading?.resume(returning: loaded)
            loading = nil
        }

        // MARK: WKNavigationDelegate

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            finishLoading(true)
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
            finishLoading(false)
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
            finishLoading(false)
        }

        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            if self.webView === webView { self.webView = nil }
            finishLoading(false)
        }
    }
}
#endif
