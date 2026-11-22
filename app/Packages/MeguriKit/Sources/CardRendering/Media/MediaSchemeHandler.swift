import Foundation
import WebKit

/// One handler for all card pages. Bindings and delivery permission live on
/// WebKit's main executor; each awaited read supplies at most one 64KiB chunk.
public final class MediaSchemeHandler: NSObject, WKURLSchemeHandler {
    public struct Binding {
        public let resolver: MediaPathResolver?
        public var document: String
        public var update: CardUpdate?
        public let diagnostics: CardDiagnostics
        public init(resolver: MediaPathResolver?, document: String, update: CardUpdate? = nil, diagnostics: CardDiagnostics) {
            self.resolver = resolver; self.document = document; self.update = update; self.diagnostics = diagnostics
        }
    }
    public nonisolated struct Receipt: Sendable {
        public let url: URL
        public let range: String?
        public let status: Int
        public let contentRange: String?
        public let bodyLength: UInt64
    }
    /// Optional bounded measurement sink, owned by the caller (S2).
    public var onResponse: ((Receipt) -> Void)?
    private var bindings: [ObjectIdentifier: Binding] = [:]
    private struct Running {
        let page: ObjectIdentifier
        let nonce: UUID
        let work: Task<Void, Never>
    }
    private var running: [ObjectIdentifier: Running] = [:]

    public func bind(_ page: WKWebView, to binding: Binding) {
        unbind(page)
        bindings[ObjectIdentifier(page)] = binding
    }
    public func setUpdate(_ update: CardUpdate, for page: WKWebView) { bindings[ObjectIdentifier(page)]?.update = update }
    public func unbind(_ page: WKWebView) {
        let id = ObjectIdentifier(page)
        bindings[id] = nil
        for key in running.keys.filter({ running[$0]?.page == id }) { running.removeValue(forKey: key)?.work.cancel() }
    }
    public func close() {
        bindings.removeAll()
        let tasks = running.values.map(\.work)
        running.removeAll() // Revoke delivery before cancellation.
        for task in tasks { task.cancel() }
    }

    public func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        let key = ObjectIdentifier(task)
        let page = ObjectIdentifier(webView)
        let nonce = UUID()
        let request = task.request
        let binding = bindings[page] // Immutable root/update binding for this request.
        let work = Task { [weak self] in
            let response: MediaResponse
            do { response = try await MediaResponse.open(request, binding: binding.map { ($0.resolver, $0.document) }) }
            catch { response = MediaResponse.failure(404) }
            guard let self, self.running[key]?.nonce == nonce, !Task.isCancelled else { await response.close(); return }
            if response.status == 404 {
                binding?.diagnostics.record("missingMedia", update: binding?.update, name: request.url?.absoluteString ?? "")
            }
            guard let url = request.url, let metadata = HTTPURLResponse(url: url, statusCode: response.status, httpVersion: "HTTP/1.1", headerFields: response.headers) else {
                self.running[key] = nil
                task.didFailWithError(URLError(.badURL))
                await response.close()
                return
            }
            self.onResponse?(Receipt(url: url, range: request.value(forHTTPHeaderField: "Range"), status: response.status,
                                     contentRange: response.headers["Content-Range"], bodyLength: response.bodyCount))
            // A sink can close the context synchronously.
            guard self.running[key]?.nonce == nonce else { await response.close(); return }
            task.didReceive(metadata)
            var sent: UInt64 = 0
            do {
                while sent < response.bodyCount {
                    guard self.running[key]?.nonce == nonce, !Task.isCancelled else { await response.close(); return }
                    let chunk = try await response.read(offset: sent, count: Int(min(65_536, response.bodyCount - sent)))
                    guard self.running[key]?.nonce == nonce, !Task.isCancelled else { await response.close(); return }
                    guard !chunk.isEmpty else { throw MediaPathResolver.Failure.read }
                    task.didReceive(chunk)
                    sent += UInt64(chunk.count)
                }
                guard self.running[key]?.nonce == nonce, !Task.isCancelled else { await response.close(); return }
                self.running[key] = nil
                task.didFinish()
            } catch {
                if self.running[key]?.nonce == nonce, !Task.isCancelled {
                    self.running[key] = nil
                    task.didFailWithError(URLError(.cannotDecodeContentData))
                }
            }
            await response.close()
        }
        running[key] = Running(page: page, nonce: nonce, work: work)
    }

    public func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {
        running.removeValue(forKey: ObjectIdentifier(task))?.work.cancel()
    }
}

private nonisolated struct MediaResponse: Sendable {
    let status: Int
    let headers: [String: String]
    let bodyCount: UInt64
    let offset: UInt64
    let file: MediaFile?
    let data: Data?

    static func failure(_ status: Int) -> Self {
        Self(status: status, headers: ["Content-Length": "0", "Cache-Control": "no-store", "Access-Control-Allow-Origin": "*"],
             bodyCount: 0, offset: 0, file: nil, data: nil)
    }

    @concurrent static func open(_ request: URLRequest, binding: (MediaPathResolver?, String)?) async throws -> Self {
        guard let url = request.url, let path = MediaOrigin.path(url), let binding else { return failure(404) }
        let method = request.httpMethod ?? "GET"
        guard method == "GET" || method == "HEAD" else {
            var failure = failure(405).headers
            failure["Allow"] = "GET, HEAD"
            return Self(status: 405, headers: failure, bodyCount: 0, offset: 0, file: nil, data: nil)
        }
        let file: MediaFile?
        let data: Data?
        let mime: String
        let media: Bool
        let first = path.split(separator: "/").first?.lowercased()
        if path == "/" || path.isEmpty {
            file = nil; data = Data(binding.1.utf8); mime = "text/html; charset=utf-8"; media = false
        } else if first == "_anki" {
            guard path.hasPrefix(CardRuntime.assetPrefix), let asset = CardRuntime.asset(at: String(path.dropFirst(CardRuntime.assetPrefix.count))) else { return failure(404) }
            file = nil; data = asset.data
            mime = asset.mimeType.hasPrefix("text/") ? asset.mimeType + "; charset=utf-8" : asset.mimeType
            media = false
        } else if first == "_addons" { return failure(404) }
        else {
            // The hidden object is inventory only; PDF bytes never enter WebKit.
            guard !CardAttachmentPDF.accepts(String(path.dropFirst())) else { return failure(404) }
            guard let resolver = binding.0 else { return failure(404) }
            let opened = try await resolver.openURLPath(path)
            file = opened; data = nil; mime = MediaMIMETypes.type(for: opened.name); media = true
        }
        let total = file?.length ?? UInt64(data?.count ?? 0)
        let range = RangeResponder(total: total, header: request.value(forHTTPHeaderField: "Range"),
                                   ifRange: request.value(forHTTPHeaderField: "If-Range"), head: method == "HEAD")
        var headers = [
            "Content-Type": mime, "Content-Length": String(range.count), "Accept-Ranges": "bytes",
            "Access-Control-Allow-Origin": "*", "Access-Control-Expose-Headers": "Accept-Ranges, Content-Range, Content-Length",
            "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff",
        ]
        if let contentRange = range.contentRange { headers["Content-Range"] = contentRange }
        if media && MediaMIMETypes.blocksScripts(mime) { headers["Content-Security-Policy"] = "script-src 'none'" }
        return Self(status: range.status, headers: headers, bodyCount: method == "HEAD" ? 0 : range.count,
                    offset: range.offset, file: file, data: data)
    }

    func read(offset: UInt64, count: Int) async throws -> Data {
        if let file { return try await file.read(offset: self.offset + offset, count: count) }
        guard let data else { return Data() }
        let start = Int(self.offset + offset)
        return data.subdata(in: start..<(start + count))
    }
    func close() async { await file?.close() }
}
