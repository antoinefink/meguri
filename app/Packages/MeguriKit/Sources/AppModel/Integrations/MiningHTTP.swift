#if os(macOS)
import Foundation
import CryptoKit

nonisolated struct MiningHTTPRequest: Sendable {
    var method: String
    var target: String
    var headers: [(String, String)]
    var body: Data
    var version = "HTTP/1.1"
    var absoluteAuthority: String?
    var keepsAlive: Bool {
        let tokens = values("connection").flatMap { $0.lowercased().split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } }
        return !tokens.contains("close") && (version == "HTTP/1.1" || tokens.contains("keep-alive"))
    }
    /// Filled by the listener, never taken from a client header.
    var localPort: UInt16 = 44432
    func values(_ name: String) -> [String] { headers.filter { $0.0 == name.lowercased() }.map(\.1) }
    func header(_ name: String) -> String? { values(name).first }
    var path: String { target.split(separator: "?", maxSplits: 1).first.map(String.init) ?? target }
}
nonisolated struct MiningHTTPResponse: Sendable {
    var status: Int
    var headers: [(String, String)] = []
    var body = Data()
    var upgrade = false
    static func json(_ status: Int, _ value: Any) -> Self {
        .init(status: status, headers: [("Content-Type", "application/json; charset=UTF-8")],
              body: (try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys])) ?? Data())
    }
    static func error(_ status: Int, _ message: String) -> Self { json(status, ["success": false, "error": message]) }
    var bytes: Data { bytes(closing: true) }
    func bytes(closing: Bool) -> Data {
        let reason = [101: "Switching Protocols", 200: "OK", 204: "No Content", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 431: "Request Header Fields Too Large", 501: "Not Implemented", 503: "Service Unavailable", 505: "HTTP Version Not Supported"][status] ?? "Error"
        var head = "HTTP/1.1 \(status) \(reason)\r\n"
        for (name, value) in headers { head += "\(name): \(value)\r\n" }
        if !upgrade {
            if status != 204 { head += "Content-Length: \(body.count)\r\n" }
            if closing { head += "Connection: close\r\n" }
        }
        return Data((head + "\r\n").utf8) + (status == 204 ? Data() : body)
    }
}

nonisolated enum MiningHTTP {
    static let bodyLimit = 134_217_728
    static func isHeaderToken(_ byte: UInt8) -> Bool {
        "!#$%&'*+-.^_`|~".utf8.contains(byte) || (48...57).contains(byte) || (65...90).contains(byte) || (97...122).contains(byte)
    }
    nonisolated enum ParseError: Error { case status(Int) }
    /// Head-only validation is performed as soon as CRLFCRLF arrives, before
    /// allocating a declared body, sending Continue, or dispatching anything.
    static func head(_ data: Data) throws -> (MiningHTTPRequest, Int)? {
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else {
            if data.count > 65_536 { throw ParseError.status(431) }; return nil
        }
        guard end.upperBound <= 65_536,
              let text = String(data: data[..<end.lowerBound], encoding: .ascii) else { throw ParseError.status(431) }
        let lines = text.components(separatedBy: "\r\n")
        let first = lines[0].split(separator: " ", omittingEmptySubsequences: false)
        guard first.count == 3, !first[0].isEmpty, !first[1].isEmpty else { throw ParseError.status(400) }
        guard first[2] == "HTTP/1.1" || first[2] == "HTTP/1.0" else { throw ParseError.status(505) }
        guard lines.count <= 101 else { throw ParseError.status(431) }
        var headers: [(String, String)] = []
        for line in lines.dropFirst() {
            guard let colon = line.firstIndex(of: ":"), colon != line.startIndex else { throw ParseError.status(400) }
            let name = String(line[..<colon]); let raw = line[line.index(after: colon)...]
            guard name.utf8.allSatisfy(isHeaderToken),
                  raw.utf8.allSatisfy({ $0 == 9 || (32...126).contains($0) }) else { throw ParseError.status(400) }
            headers.append((name.lowercased(), raw.trimmingCharacters(in: .whitespaces)))
        }
        var request = MiningHTTPRequest(method: String(first[0]), target: String(first[1]), headers: headers, body: Data(), version: String(first[2]))
        if request.target.lowercased().hasPrefix("http://") || request.target.lowercased().hasPrefix("https://") {
            guard let url = URLComponents(string: request.target), url.scheme?.lowercased() == "http",
                  url.host != nil, url.user == nil, url.password == nil, url.port != nil else { throw ParseError.status(403) }
            // Compare the authority as written, with IPv6's single bracket pair
            // intact. Rebuilding from URLComponents.host can double its brackets
            // and would erase a noncanonical port spelling.
            request.absoluteAuthority = String(request.target.dropFirst(7).prefix { !"/?#".contains($0) })
            request.target = (url.percentEncodedPath.isEmpty ? "/" : url.percentEncodedPath) + (url.percentEncodedQuery.map { "?" + $0 } ?? "")
        }
        let lengths = request.values("content-length")
        if !lengths.isEmpty {
            guard Set(lengths).count == 1, let length = lengths.first, !length.isEmpty,
                  length.utf8.allSatisfy({ (48...57).contains($0) }), UInt64(length) != nil else { throw ParseError.status(400) }
        }
        let encoding = request.values("transfer-encoding")
        guard encoding.isEmpty || (lengths.isEmpty && encoding.count == 1 && encoding[0].lowercased() == "chunked") else { throw ParseError.status(400) }
        return (request, end.upperBound)
    }
    static func body(_ data: Data, request: MiningHTTPRequest) throws -> Data? {
        var decoder = try MiningHTTPBody(request: request)
        _ = try decoder.consume(data)
        return decoder.complete ? decoder.body : nil
    }
    static func allowedOrigin(_ origin: String) -> Bool {
        guard !origin.isEmpty, origin.utf8.allSatisfy({ (33...126).contains($0) }) else { return false }
        let lower = origin.lowercased()
        for scheme in ["chrome-extension://", "moz-extension://", "safari-web-extension://"] where lower.hasPrefix(scheme) {
            let id = lower.dropFirst(scheme.count)
            return !id.isEmpty && !id.contains(where: { "/?#@:".contains($0) })
        }
        guard lower.hasPrefix("https://") else { return false }
        let host = lower.dropFirst(8)
        guard host == "migaku.com" || host.hasSuffix(".migaku.com") else { return false }
        return host.split(separator: ".", omittingEmptySubsequences: false).allSatisfy {
            !$0.isEmpty && $0.utf8.allSatisfy { (97...122).contains($0) || (48...57).contains($0) || $0 == 45 }
        }
    }
    static func admission(_ request: MiningHTTPRequest, port: UInt16) -> MiningHTTPResponse? {
        func validHost(_ host: String) -> Bool {
            ["127.0.0.1", "localhost", "[::1]", "host.docker.internal"].contains { host.lowercased() == "\($0):\(port)" }
        }
        guard request.values("host").count <= 1, request.values("host").allSatisfy(validHost),
              (request.absoluteAuthority.map(validHost) ?? true), request.values("origin").count <= 1,
              request.values("origin").allSatisfy(allowedOrigin) else { return .init(status: 403) }
        // The compatible root API is callable by local tools and extensions.
        // Migaku web origins only receive the narrowly scoped mining API.
        if request.path == "/", let origin = request.header("origin"), origin.lowercased().hasPrefix("https://") { return .init(status: 403) }
        return nil
    }
    static func cors(_ response: MiningHTTPResponse, request: MiningHTTPRequest) -> MiningHTTPResponse {
        guard response.status != 403 else { return response }
        var value = response
        if request.method == "HEAD" { value.body = Data() }
        value.headers += [("Access-Control-Allow-Origin", request.header("origin") ?? "*"),
            ("Access-Control-Allow-Headers", "access-control-allow-origin,content-type"), ("Access-Control-Allow-Methods", "POST, GET, OPTIONS")]
        if request.header("origin") != nil { value.headers.append(("Vary", "Origin")) }
        return value
    }
    static func preliminary(_ request: MiningHTTPRequest, port: UInt16, sockets: Int) -> MiningHTTPResponse? {
        if let refusal = admission(request, port: port) { return refusal }
        func reply(_ response: MiningHTTPResponse) -> MiningHTTPResponse { cors(response, request: request) }
        if request.method == "OPTIONS" {
            var response = MiningHTTPResponse(status: 204)
            if request.header("access-control-request-private-network")?.lowercased() == "true" { response.headers.append(("Access-Control-Allow-Private-Network", "true")) }
            return reply(response)
        }
        let routes = ["/": ["POST"], "/srs-check": ["GET"], "/receive": ["POST"], "/anki-connect": ["GET"],
            "/anki-hello": ["GET", "POST"], "/srs-import-info": ["GET", "POST"], "/srs-sample-card": ["GET", "POST"], "/srs-import": ["GET", "POST"]]
        guard let methods = routes[request.path] else { return reply(.error(404, "Not found.")) }
        guard methods.contains(request.method) else {
            var response = MiningHTTPResponse.error(405, "Method not allowed.")
            response.headers.append(("Allow", (methods + ["OPTIONS"]).joined(separator: ", ")))
            return reply(response)
        }
        if request.path == "/anki-connect" {
            guard request.values("sec-websocket-key").count == 1,
                  let key = request.header("sec-websocket-key"), Data(base64Encoded: key)?.count == 16,
                  request.header("sec-websocket-version") == "13", request.header("upgrade")?.lowercased() == "websocket",
                  request.header("connection")?.lowercased().split(separator: ",").contains(where: { $0.trimmingCharacters(in: .whitespaces) == "upgrade" }) == true else {
                var error = MiningHTTPResponse.error(400, "Expected a WebSocket upgrade.")
                error.headers.append(("Sec-WebSocket-Version", "13")); return reply(error)
            }
            guard sockets < 64 else { return reply(.error(503, "Too many open connections.")) }
            let accept = Data(Insecure.SHA1.hash(data: Data((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").utf8))).base64EncodedString()
            return reply(.init(status: 101, headers: [("Upgrade", "websocket"), ("Connection", "Upgrade"), ("Sec-WebSocket-Accept", accept)], upgrade: true))
        }
        return nil
    }
}
/// Consumes each wire byte once. Only decoded content and one bounded framing
/// line survive a receive; consumed chunk framing is never retained or decoded again.
nonisolated struct MiningHTTPBody {
    private enum State { case fixed(Int), size, chunk(Int), chunkCR, chunkLF, trailers, complete }
    private var state: State
    private var line = Data()
    private var lineCR = false
    private var trailerBytes = 0
    private var trailerLines = 0
    private let limit: Int
    private(set) var body = Data()
    var complete: Bool { if case .complete = state { true } else { false } }
    var retainedBytes: Int { body.count + line.count + (lineCR ? 1 : 0) }

    init(request: MiningHTTPRequest, limit: Int = MiningHTTP.bodyLimit) throws {
        self.limit = limit
        if let length = request.header("content-length").flatMap(UInt64.init) {
            guard length <= limit else { throw MiningHTTP.ParseError.status(413) }
            state = length == 0 ? .complete : .fixed(Int(length))
        } else { state = request.header("transfer-encoding") == nil ? .complete : .size }
    }

    /// Returns the consumed wire length, leaving a buffered next request alone.
    mutating func consume(_ bytes: Data) throws -> Int {
        var cursor = bytes.startIndex
        while cursor < bytes.endIndex, !complete {
            switch state {
            case .fixed(let remaining), .chunk(let remaining):
                let count = min(remaining, bytes.endIndex - cursor)
                body.append(bytes[cursor..<(cursor + count)])
                cursor += count
                if case .fixed = state { state = remaining == count ? .complete : .fixed(remaining - count) }
                else { state = remaining == count ? .chunkCR : .chunk(remaining - count) }
            case .chunkCR:
                guard bytes[cursor] == 13 else { throw MiningHTTP.ParseError.status(400) }
                cursor += 1
                state = .chunkLF
            case .chunkLF:
                guard bytes[cursor] == 10 else { throw MiningHTTP.ParseError.status(400) }
                cursor += 1
                state = .size
            case .size, .trailers:
                let trailers: Bool = { if case .trailers = state { true } else { false } }()
                let byte = bytes[cursor]
                cursor += 1
                if trailers {
                    trailerBytes += 1
                    guard trailerBytes <= 65_536 else { throw MiningHTTP.ParseError.status(431) }
                }
                if lineCR {
                    guard byte == 10 else { throw MiningHTTP.ParseError.status(400) }
                    lineCR = false
                    if trailers {
                        if line.isEmpty { state = .complete }
                        else {
                            trailerLines += 1
                            guard trailerLines <= 100 else { throw MiningHTTP.ParseError.status(431) }
                            guard let text = String(data: line, encoding: .ascii),
                                  let colon = text.firstIndex(of: ":"), colon != text.startIndex,
                                  text[..<colon].utf8.allSatisfy(MiningHTTP.isHeaderToken),
                                  text[text.index(after: colon)...].utf8.allSatisfy({ $0 == 9 || (32...126).contains($0) }) else {
                                throw MiningHTTP.ParseError.status(400)
                            }
                        }
                    } else {
                        guard let text = String(data: line, encoding: .ascii),
                              let digits = text.split(separator: ";", maxSplits: 1, omittingEmptySubsequences: false).first,
                              !digits.isEmpty, digits.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }),
                              let count = UInt64(digits, radix: 16) else { throw MiningHTTP.ParseError.status(400) }
                        guard count <= limit - body.count else { throw MiningHTTP.ParseError.status(413) }
                        state = count == 0 ? .trailers : .chunk(Int(count))
                    }
                    line.removeAll(keepingCapacity: false)
                } else if byte == 13 { lineCR = true }
                else {
                    guard byte != 10, byte == 9 || (32...126).contains(byte) else { throw MiningHTTP.ParseError.status(400) }
                    guard line.count < 8192 else { throw MiningHTTP.ParseError.status(400) }
                    line.append(byte)
                }
            case .complete: break
            }
        }
        return cursor - bytes.startIndex
    }
}
#endif
