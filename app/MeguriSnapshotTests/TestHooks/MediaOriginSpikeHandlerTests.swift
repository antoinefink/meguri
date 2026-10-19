#if MEGURI_TEST_HOOKS
import Foundation
import Testing
import WebKit

@testable import Meguri

/// The S2 spike's in-app media handler (U65, `TestHooks.MediaOriginSpike`)
/// against fixed answers (R495). The app's media checks
/// (CardSwapTimingSpikeTests' seeks) judge each ranged answer with the
/// handler's own range parser, so the parser is held here to answers worked
/// out by hand, for a 10-byte file whose bytes are the digits 0 to 9.
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
}
#endif
