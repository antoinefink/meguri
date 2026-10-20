#if MEGURI_TEST_HOOKS
import CardRendering
import Foundation
import Testing
import WebKit

@testable import Meguri

/// The S2 spike's production media proxy (U65, `TestHooks.MediaOriginSpike`)
/// against fixed answers (R495). The app's media checks
/// (CardSwapTimingSpikeTests' seeks) judge each ranged answer with the
/// handler's own range parser, so the parser is held here to answers worked
/// out by hand, for a 10-byte file whose bytes are the digits 0 to 9.
/// Real WebKit pages also check navigation ownership and script deadlines.
@MainActor
@Suite struct MediaOriginSpikeHandlerTests {
    nonisolated static let file = Data("0123456789".utf8)

    /// A scheme task that keeps what the handler sends it.
    final class RecordingTask: NSObject, WKURLSchemeTask {
        let request: URLRequest
        var response: HTTPURLResponse?
        var body = Data()
        var finished = false
        var failure: (any Error)?
        var completion: CheckedContinuation<Void, Never>?

        init(_ request: URLRequest) { self.request = request }

        func didReceive(_ response: URLResponse) { self.response = response as? HTTPURLResponse }
        func didReceive(_ data: Data) { body.append(data) }
        func didFinish() { finished = true; completion?.resume(); completion = nil }
        func didFailWithError(_ error: any Error) { failure = error; completion?.resume(); completion = nil }
    }

    struct Case: CustomTestStringConvertible, Sendable {
        let range: String?
        let status: Int
        let contentRange: String?
        let body: String
        var testDescription: String { range ?? "no Range" }
    }

    nonisolated static let cases: [Case] = [
        // Bounded.
        Case(range: "bytes=2-5", status: 206, contentRange: "bytes 2-5/10", body: "2345"),
        Case(range: "bytes=0-0", status: 206, contentRange: "bytes 0-0/10", body: "0"),
        // Bounded past the end: up to the last byte.
        Case(range: "bytes=8-20", status: 206, contentRange: "bytes 8-9/10", body: "89"),
        // Open-ended.
        Case(range: "bytes=7-", status: 206, contentRange: "bytes 7-9/10", body: "789"),
        Case(range: "bytes=0-", status: 206, contentRange: "bytes 0-9/10", body: "0123456789"),
        // Suffix: the last n bytes, the whole file when n is larger.
        Case(range: "bytes=-3", status: 206, contentRange: "bytes 7-9/10", body: "789"),
        Case(range: "bytes=-20", status: 206, contentRange: "bytes 0-9/10", body: "0123456789"),
        // No Range: the whole file.
        Case(range: nil, status: 200, contentRange: nil, body: "0123456789"),
    ]

    func answer(_ range: String?, handler: TestHooks.MediaOriginSpike.Handler) async throws -> RecordingTask {
        var request = URLRequest(url: TestHooks.MediaOriginSpike.origin.appending(path: "_digits.bin"))
        if let range { request.setValue(range, forHTTPHeaderField: "Range") }
        let task = RecordingTask(request)
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let page = WKWebView(frame: .zero, configuration: configuration)
        try await handler.bind(page, document: "<!doctype html>")
        await withCheckedContinuation { continuation in
            task.completion = continuation
            handler.webView(page, start: task)
        }
        return task
    }

    func handler() -> TestHooks.MediaOriginSpike.Handler {
        let handler = TestHooks.MediaOriginSpike.Handler()
        handler.files["/_digits.bin"] = (Self.file, "application/octet-stream")
        return handler
    }

    @Test(arguments: cases)
    func aRangeGetsTheBytesItAsksFor(_ expected: Case) async throws {
        let handler = handler()
        let task = try await answer(expected.range, handler: handler)
        let response = try #require(task.response)
        #expect(task.finished && task.failure == nil)
        #expect(response.statusCode == expected.status)
        #expect(response.value(forHTTPHeaderField: "Content-Range") == expected.contentRange)
        #expect(response.value(forHTTPHeaderField: "Accept-Ranges") == "bytes")
        #expect(response.value(forHTTPHeaderField: "Content-Length") == String(expected.body.utf8.count))
        #expect(String(decoding: task.body, as: UTF8.self) == expected.body)
        // The answers the app's checks judge pass them.
        #expect(handler.rangeProblems("/_digits.bin") == [])
    }

    typealias Page = TestHooks.MediaOriginSpike.Page

    func page() -> Page {
        let handler = handler()
        handler.files["/_anki/page.html"] = (Data("<!doctype html><title>original</title><body></body>".utf8), "text/html")
        handler.files["/_anki/replacement.html"] = (Data("<!doctype html><title>replacement</title><body></body>".utf8), "text/html")
        return Page(TestHooks.MediaOriginSpike.configuration(handler, persistent: false), handler: handler)
    }

    @Test
    func productionRootLoadsTheDocumentAndRunsJavaScript() async throws {
        let handler = handler()
        handler.files["/_anki/page.html"] = (Data("<!doctype html><title>original</title><body></body>".utf8), "text/html")
        let configuration = TestHooks.MediaOriginSpike.configuration(handler, persistent: false)
        let original = handler.engine.configuration
        let scheme = TestHooks.MediaOriginSpike.scheme
        #expect(configuration !== original)
        #expect(configuration.urlSchemeHandler(forURLScheme: scheme) === handler)
        #expect(original.urlSchemeHandler(forURLScheme: scheme) === handler.engine.media)
        #expect(configuration.websiteDataStore === original.websiteDataStore)
        #expect(configuration.userContentController === original.userContentController)
        #expect(configuration.mediaTypesRequiringUserActionForPlayback == original.mediaTypesRequiringUserActionForPlayback)
        #if !os(macOS)
        #expect(configuration.defaultWebpagePreferences.preferredContentMode == original.defaultWebpagePreferences.preferredContentMode)
        #expect(configuration.allowsInlineMediaPlayback == original.allowsInlineMediaPlayback)
        #endif
        let page = Page(configuration, handler: handler)
        try await page.open("_anki/page.html")
        let result = await page.run("""
            document.body.dataset.roundtrip = value;
            return location.href + "|" + document.title + "|" + document.body.dataset.roundtrip;
            """, ["value": "loaded"], within: 2)
        #expect(result == "meguri-media://media/|original|loaded")
        #expect(page.handler.answers.contains { $0.path == "/" && $0.status == 200 })
    }

    @Test
    func concurrentOpenIsRejectedWithoutStealingTheFirstWaiter() async throws {
        let page = page()
        var competing: Task<Void, any Error>?
        try await page.open("_anki/page.html") { stage in
            // Queue the competing caller before the binding worker. The
            // first attempt already owns the page, even before navigation.
            if stage == .binding {
                competing = Task { try await page.open("_anki/replacement.html") }
            }
        }
        let competitor = try #require(competing)
        do {
            try await competitor.value
            Issue.record("a concurrent open replaced the active attempt")
        } catch Page.OpenError.alreadyOpening {
            // The rejected caller must not rebind the production handler.
        }
        let title = await page.run("return document.title;", within: 2)
        #expect(title == "original")
    }

    func cancelThenReplace(at checkpoint: Page.OpenStage) async throws {
        let page = page()
        var first: Task<Void, any Error>?
        first = Task {
            try await page.open("_anki/page.html") { stage in
                if stage == checkpoint { first?.cancel() }
            }
        }
        let cancelled = try #require(first)
        do {
            try await cancelled.value
            Issue.record("the cancelled open reported success")
        } catch is CancellationError {
            // The waiter resolves before a replacement may acquire it.
        }
        try await page.open("_anki/replacement.html")
        let title = await page.run("return document.title;", within: 2)
        #expect(title == "replacement")
    }

    @Test
    func cancellationDuringBindingDoesNotRebindTheReplacement() async throws {
        try await cancelThenReplace(at: .binding)
    }

    @Test
    func cancelledNavigationCannotResolveTheReplacementWaiter() async throws {
        try await cancelThenReplace(at: .navigation)
    }

    @Test
    func expiredOpenFailsAndReleasesOwnershipForTheNextNavigation() async throws {
        let page = page()
        do {
            try await page.open("_anki/page.html", within: .zero)
            Issue.record("an expired open reported success")
        } catch Page.OpenError.timedOut(let stage) {
            #expect(stage == .binding)
        }
        try await page.open("_anki/replacement.html")
        let title = await page.run("return document.title;", within: 2)
        #expect(title == "replacement")
    }

    @Test
    func aStalledScriptReportsTimeoutOnARealLoadedPage() async throws {
        let page = page()
        try await page.open("_anki/page.html")
        let result = await page.run("return await new Promise(() => {});", within: 1)
        #expect(result == #"{"error":"timed out after 1 s"}"#)
        let title = await page.run("return document.title;", within: 2)
        #expect(title == "original")
    }
}
#endif
