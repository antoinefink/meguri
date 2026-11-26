@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite struct MediaSchemeHandlerTests {
        final class RecordingTask: NSObject, WKURLSchemeTask {
            let request: URLRequest
            var response: HTTPURLResponse?
            var body = Data()
            var chunks: [Int] = []
            var retainBody = true
            var countChunks = true
            var totalBytes: UInt64 = 0
            var largestChunk = 0
            var terminal = 0
            var failure: (any Error)?
            var afterChunk: (() -> Void)?
            var completion: CheckedContinuation<Void, Never>?
            init(_ request: URLRequest) { self.request = request }
            func didReceive(_ response: URLResponse) { self.response = response as? HTTPURLResponse }
            func didReceive(_ data: Data) {
                if countChunks { chunks.append(data.count) }
                if retainBody { body.append(data) }
                totalBytes += UInt64(data.count); largestChunk = max(largestChunk, data.count)
                afterChunk?()
            }
            func didFinish() { terminal += 1; completion?.resume(); completion = nil }
            func didFailWithError(_ error: any Error) { terminal += 1; failure = error; completion?.resume(); completion = nil }
        }
        nonisolated struct Case: Sendable {
            let range: String?
            let status: Int
            let contentRange: String?
            let body: String
        }
        nonisolated static let cases = [
            Case(range: nil, status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "bytes=2-5", status: 206, contentRange: "bytes 2-5/10", body: "2345"),
            Case(range: "bytes=0-0", status: 206, contentRange: "bytes 0-0/10", body: "0"),
            Case(range: "bytes=8-20", status: 206, contentRange: "bytes 8-9/10", body: "89"),
            Case(range: "bytes=7-", status: 206, contentRange: "bytes 7-9/10", body: "789"),
            Case(range: "bytes=0-", status: 206, contentRange: "bytes 0-9/10", body: "0123456789"),
            Case(range: "bytes=-3", status: 206, contentRange: "bytes 7-9/10", body: "789"),
            Case(range: "bytes=-20", status: 206, contentRange: "bytes 0-9/10", body: "0123456789"),
            Case(range: "bytes=10-", status: 416, contentRange: "bytes */10", body: ""),
            Case(range: "bytes=-0", status: 416, contentRange: "bytes */10", body: ""),
            Case(range: "bytes=5-2", status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "bytes=x-", status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "items=1-2", status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "bytes=0-1,8-9", status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "bytes=999999999999999999999999-", status: 416, contentRange: "bytes */10", body: ""),
            Case(range: "bytes=999999999999999999999999-888888888888888888888888", status: 200, contentRange: nil, body: "0123456789"),
            Case(range: "BYTES=\t -999999999999999999999999 \t", status: 206, contentRange: "bytes 0-9/10", body: "0123456789"),
        ]
        static func taskPage() -> WKWebView {
            let configuration = WKWebViewConfiguration()
            configuration.websiteDataStore = .nonPersistent()
            return WKWebView(frame: .zero, configuration: configuration)
        }
        static func answer(_ request: URLRequest, fixture: MediaFixture) async throws -> RecordingTask {
            let page = taskPage()
            try await fixture.bind(page, document: "<!doctype html><p>runtime</p>")
            let task = RecordingTask(request)
            await withCheckedContinuation { continuation in
                task.completion = continuation
                fixture.handler.webView(page, start: task)
            }
            fixture.handler.unbind(page)
            return task
        }
        @Test(arguments: cases) func exactResponse(_ item: Case) async throws {
            let fixture = MediaFixture()
            fixture.add("/digits.bin", Data("0123456789".utf8), "application/octet-stream")
            var request = URLRequest(url: MediaOrigin.url.appending(path: "digits.bin"))
            request.setValue(item.range, forHTTPHeaderField: "Range")
            let task = try await Self.answer(request, fixture: fixture)
            let response = try #require(task.response)
            #expect(task.terminal == 1 && task.failure == nil)
            #expect(response.statusCode == item.status)
            #expect(response.value(forHTTPHeaderField: "Content-Range") == item.contentRange)
            #expect(response.value(forHTTPHeaderField: "Content-Length") == String(item.body.utf8.count))
            #expect(response.value(forHTTPHeaderField: "Accept-Ranges") == "bytes")
            #expect(task.body == Data(item.body.utf8))
            // Independent suffix sensitivity control (a one-byte error).
            if item.range == "bytes=-3" { #expect(task.body != Data("6789".utf8)) }
        }
        @Test func routesMethodsPoliciesAndFreshMisses() async throws {
            let fixture = MediaFixture()
            for name in ["x.svg", "x.html", "x.xhtml", "x.js", "x.woff2", "empty.bin"] { fixture.add("/" + name, Data(), "") }
            for (path, status, policy) in [("/", 200, false), ("/_anki/runtime.js", 200, false),
                                           ("/_anki/image-occlusion.js", 200, false), ("/_anki/no.js", 404, false),
                                           ("/_addons/x.js", 404, false), ("/%5Faddons/x.js", 404, false), ("/_ANKI/x.js", 404, false),
                                           ("/x.svg", 200, true), ("/x.html", 200, true), ("/x.xhtml", 200, true), ("/x.js", 200, false), ("/x.woff2", 200, false)] {
                let task = try await Self.answer(URLRequest(url: URL(string: "meguri-media://media" + path)!), fixture: fixture)
                #expect(task.response?.statusCode == status)
                #expect((task.response?.value(forHTTPHeaderField: "Content-Security-Policy") == "script-src 'none'") == policy)
                #expect(task.response?.value(forHTTPHeaderField: "Cache-Control") == "no-store")
                if path == "/x.js" { #expect(task.response?.value(forHTTPHeaderField: "Content-Type") == "text/javascript; charset=utf-8") }
                if path == "/x.woff2" {
                    #expect(task.response?.value(forHTTPHeaderField: "Content-Type") == "font/woff2")
                    #expect(task.response?.value(forHTTPHeaderField: "Access-Control-Allow-Origin") == "*")
                    #expect(task.response?.value(forHTTPHeaderField: "Access-Control-Expose-Headers") == "Accept-Ranges, Content-Range, Content-Length")
                }
            }
            for origin in ["meguri-media:///", "meguri-media://Media.Local/", "meguri-media://user@media/", "meguri-media://media:8765/"] {
                let refused = try await Self.answer(URLRequest(url: URL(string: origin)!), fixture: fixture)
                #expect(refused.response?.statusCode == 404 && refused.body.isEmpty)
            }
            var request = URLRequest(url: MediaOrigin.url.appending(path: "empty.bin"))
            request.setValue("bytes=10-", forHTTPHeaderField: "Range")
            #expect(try await Self.answer(request, fixture: fixture).response?.statusCode == 200)
            fixture.add("/digits.bin", Data("0123456789".utf8), "")
            request.url = MediaOrigin.url.appending(path: "digits.bin")
            request.httpMethod = "HEAD"
            let head = try await Self.answer(request, fixture: fixture)
            #expect(head.response?.statusCode == 200 && head.body.isEmpty)
            #expect(head.response?.value(forHTTPHeaderField: "Content-Length") == "10")
            #expect(head.response?.value(forHTTPHeaderField: "Content-Range") == nil)
            request.httpMethod = "POST"
            let post = try await Self.answer(request, fixture: fixture)
            #expect(post.response?.statusCode == 405 && post.body.isEmpty)
            #expect(post.response?.value(forHTTPHeaderField: "Allow") == "GET, HEAD")
            request.httpMethod = "GET"
            request.setValue("bytes=2-5", forHTTPHeaderField: "Range")
            request.setValue("anything", forHTTPHeaderField: "If-Range")
            #expect(try await Self.answer(request, fixture: fixture).body == Data("0123456789".utf8))
            request.url = MediaOrigin.url.appending(path: "late.png")
            #expect(try await Self.answer(request, fixture: fixture).response?.statusCode == 404)
            fixture.add("/late.png", Data("arrival".utf8), "")
            request.setValue(nil, forHTTPHeaderField: "Range")
            #expect(try await Self.answer(request, fixture: fixture).body == Data("arrival".utf8))
        }

        @Test func bundledMathRoutesAreConfinedAndNeverUseCollectionMedia() async throws {
            let fixture = MediaFixture()
            let paths = CardRuntime.assetPaths.filter { $0.hasPrefix("mathjax/") }
            #expect(paths.count == 85)
            // Runs against Bundle.module on both Mac and the hosted iOS suite.
            for path in paths {
                let asset = try #require(CardRuntime.asset(at: path))
                #expect(!asset.data.isEmpty, "\(path)")
                let task = try await Self.answer(URLRequest(url: URL(string: "meguri-media://media/_anki/" + path)!), fixture: fixture)
                #expect(task.response?.statusCode == 200)
                #expect(task.response?.value(forHTTPHeaderField: "Content-Type") == "text/javascript; charset=utf-8")
                #expect(task.body == asset.data)
            }
            for path in ["mathjax/input/tex/extensions/texhtml.js", "mathjax/input/tex/extensions/setoptions.js",
                         "mathjax/../runtime.js", "mathjax/font/svg/dynamic/../../tex-svg.js", "mathjax/tex-svg.js/extra",
                         "mathjax/LICENSE", "mathjax/a11y/speech.js", "mathjax/no.js", "mathjax/%74ex-svg.js"] {
                #expect(CardRuntime.asset(at: path) == nil, "\(path)")
            }
            fixture.add("/no.js", Data("collection fallback forbidden".utf8), "text/javascript")
            for path in ["no.js", "input/tex/extensions/texhtml.js", "input/tex/extensions/setoptions.js", "a11y/speech.js"] {
                let task = try await Self.answer(URLRequest(url: URL(string: "meguri-media://media/_anki/mathjax/" + path)!), fixture: fixture)
                #expect(task.response?.statusCode == 404 && task.body.isEmpty)
            }
            let glyphs = CardRuntime.assetPaths.filter { $0.hasPrefix("mathjax/font/svg/dynamic/") }
            #expect(!glyphs.isEmpty)
            for path in glyphs { #expect(CardRuntime.asset(at: path)?.mimeType == "text/javascript") }
        }
        @Test func stopRevokesAlreadyPendingRangeDelivery() async throws {
            let fixture = MediaFixture()
            fixture.add("/large.bin", Data(repeating: 42, count: 2_000_000), "")
            let page = Self.taskPage()
            try await fixture.bind(page, document: "root")
            var request = URLRequest(url: MediaOrigin.url.appending(path: "large.bin"))
            request.setValue("bytes=1-1500000", forHTTPHeaderField: "Range")
            let task = RecordingTask(request)
            task.afterChunk = { fixture.handler.webView(page, stop: task) }
            fixture.handler.webView(page, start: task)
            try await Task.sleep(for: .milliseconds(100))
            let bytes = task.body.count
            #expect(bytes > 0 && bytes <= 65_536)
            #expect(task.terminal == 0)
            try await Task.sleep(for: .milliseconds(50))
            #expect(task.body.count == bytes && task.terminal == 0)
            #expect(task.chunks.allSatisfy { $0 <= 65_536 })
            task.afterChunk = nil
            fixture.handler.unbind(page)
        }

        @Test func largeSparseFileStreamsWithoutCollectingItsBody() async throws {
            let fixture = MediaFixture()
            let file = fixture.root.appending(path: "sparse.bin")
            #expect(FileManager.default.createFile(atPath: file.path, contents: nil))
            let writer = try FileHandle(forWritingTo: file)
            try writer.truncate(atOffset: 64 * 1024 * 1024)
            try writer.close()
            let page = Self.taskPage()
            try await fixture.bind(page, document: "root")
            defer { fixture.handler.unbind(page) }
            let task = RecordingTask(URLRequest(url: MediaOrigin.url.appending(path: "sparse.bin")))
            task.retainBody = false; task.countChunks = false
            await withCheckedContinuation { continuation in
                task.completion = continuation
                fixture.handler.webView(page, start: task)
            }
            #expect(task.totalBytes == 64 * 1024 * 1024 && task.terminal == 1 && task.failure == nil)
            #expect(task.largestChunk <= 65_536 && task.body.isEmpty)
        }
    }
}
