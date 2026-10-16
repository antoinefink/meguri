import Foundation
import WebKit

/// The rendering spikes' throwaway `meguri-media:` handler (U65, S2). It
/// serves files from memory at the root of one origin, answers a single
/// `Range` with 206 and logs every request, so a test can see what WebKit
/// asked for and what it got. U67's production handler replaces it, and the
/// S2 tests are re-run against that one.
final class SpikeMediaHandler: NSObject, WKURLSchemeHandler {
    struct File {
        let data: Data
        let mimeType: String
    }

    /// One request as WebKit made it and as the handler answered it.
    struct Request: Equatable, CustomStringConvertible {
        let url: String
        let path: String
        let range: String?
        let status: Int
        let contentRange: String?
        let bodyLength: Int

        var description: String {
            "\(path) range=\(range ?? "-") → \(status) \(contentRange ?? "") \(bodyLength) bytes"
        }
    }

    /// Files by decoded path (`/photo.png`).
    var files: [String: File] = [:]
    /// Whether a `Range` request gets 206 (S2(a)); off, every request gets
    /// the whole file with 200.
    var answersRanges = true
    /// The policy SVG and HTML media carry when opened as documents (R497).
    /// Runtime pages under `runtimePrefix` never get one (R504).
    var mediaDocumentPolicy: String? = "script-src 'none'"
    /// The reserved prefix of bundled runtime pages (R496).
    static let runtimePrefix = "/_anki/"

    private(set) var requests: [Request] = []

    func add(_ path: String, _ data: Data, _ mimeType: String) {
        files[path] = File(data: data, mimeType: mimeType)
    }

    func add(_ path: String, text: String, _ mimeType: String) {
        add(path, Data(text.utf8), mimeType)
    }

    /// The requests for `path`.
    func requests(for path: String) -> [Request] {
        requests.filter { $0.path == path }
    }

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url else {
            task.didFailWithError(URLError(.badURL))
            return
        }
        let path = url.path(percentEncoded: false)
        let range = task.request.value(forHTTPHeaderField: "Range")
        guard let file = files[path] else {
            respond(task, url: url, path: path, range: range, status: 404, headers: [:], body: Data())
            return
        }
        let total = file.data.count
        var headers = [
            "Content-Type": file.mimeType,
            "Accept-Ranges": "bytes",
            // Fonts and module scripts are fetched in CORS mode (R495).
            "Access-Control-Allow-Origin": "*",
            "Cache-Control": "no-store",
        ]
        if let policy = mediaDocumentPolicy, !path.hasPrefix(Self.runtimePrefix),
            file.mimeType == "image/svg+xml" || file.mimeType == "text/html"
        {
            headers["Content-Security-Policy"] = policy
        }
        if answersRanges, let range, let bounds = Self.bounds(of: range, total: total) {
            guard let bounds else {
                headers["Content-Range"] = "bytes */\(total)"
                respond(task, url: url, path: path, range: range, status: 416, headers: headers, body: Data())
                return
            }
            headers["Content-Range"] = "bytes \(bounds.lowerBound)-\(bounds.upperBound - 1)/\(total)"
            respond(task, url: url, path: path, range: range, status: 206, headers: headers, body: file.data.subdata(in: bounds))
            return
        }
        respond(task, url: url, path: path, range: range, status: 200, headers: headers, body: file.data)
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {
        // Every response is sent synchronously from `start`: nothing to stop.
    }

    private func respond(
        _ task: any WKURLSchemeTask, url: URL, path: String, range: String?, status: Int, headers: [String: String], body: Data
    ) {
        var headers = headers
        headers["Content-Length"] = String(body.count)
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
        requests.append(
            Request(
                url: url.absoluteString, path: path, range: range, status: status, contentRange: headers["Content-Range"],
                bodyLength: body.count))
        task.didReceive(response)
        // In chunks, as the production handler streams from disk.
        let chunk = 256 * 1024
        var start = 0
        while start < body.count {
            let end = min(start + chunk, body.count)
            task.didReceive(body.subdata(in: start..<end))
            start = end
        }
        task.didFinish()
    }

    /// The byte range a single-range `Range` header asks for: nil when the
    /// header is not one the handler understands (answered with the whole
    /// file), `.some(nil)` when it cannot be satisfied (416).
    static func bounds(of header: String, total: Int) -> Range<Int>?? {
        guard let spec = header.wholeMatch(of: /bytes=(\d*)-(\d*)/), !(spec.1.isEmpty && spec.2.isEmpty) else { return nil }
        if spec.1.isEmpty {
            // A suffix: the last N bytes.
            guard let suffix = Int(spec.2), suffix > 0, total > 0 else { return .some(nil) }
            return .some(max(0, total - suffix)..<total)
        }
        guard let first = Int(spec.1), first < total else { return .some(nil) }
        let last = spec.2.isEmpty ? total - 1 : min(Int(spec.2) ?? (total - 1), total - 1)
        guard last >= first else { return .some(nil) }
        return .some(first..<(last + 1))
    }
}
