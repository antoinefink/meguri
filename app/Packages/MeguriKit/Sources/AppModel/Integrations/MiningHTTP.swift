#if os(macOS)
import Foundation
import CryptoKit

nonisolated struct MiningHTTPRequest: Sendable {
    var method: String
    var target: String
    var headers: [(String, String)]
    var body: Data
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
    var bytes: Data {
        let reason = [101: "Switching Protocols", 200: "OK", 204: "No Content", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 431: "Request Header Fields Too Large", 501: "Not Implemented", 503: "Service Unavailable", 505: "HTTP Version Not Supported"][status] ?? "Error"
        var head = "HTTP/1.1 \(status) \(reason)\r\n"
        for (name, value) in headers { head += "\(name): \(value)\r\n" }
        if !upgrade { head += "Content-Length: \(body.count)\r\nConnection: close\r\n" }
        return Data((head + "\r\n").utf8) + body
    }
}

nonisolated enum MiningHTTP {
    static let bodyLimit = 134_217_728
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
        let token = Set("!#$%&'*+-.^_`|~0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".utf8)
        for line in lines.dropFirst() {
            guard let colon = line.firstIndex(of: ":"), colon != line.startIndex else { throw ParseError.status(400) }
            let name = String(line[..<colon]); let raw = line[line.index(after: colon)...]
            guard name.utf8.allSatisfy({ token.contains($0) }),
                  raw.utf8.allSatisfy({ $0 == 9 || (32...126).contains($0) }) else { throw ParseError.status(400) }
            headers.append((name.lowercased(), raw.trimmingCharacters(in: .whitespaces)))
        }
        var request = MiningHTTPRequest(method: String(first[0]), target: String(first[1]), headers: headers, body: Data())
        if request.target.hasPrefix("http://") || request.target.hasPrefix("https://") {
            guard let url = URLComponents(string: request.target), url.scheme == "http", let host = url.host,
                  url.user == nil, url.password == nil, let port = url.port else { throw ParseError.status(403) }
            request.headers.append(("absolute-host", "\(host.contains(":") ? "[\(host)]" : host):\(port)"))
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
        if let length = request.header("content-length").flatMap(UInt64.init) {
            guard length <= bodyLimit else { throw ParseError.status(413) }
            return data.count >= Int(length) ? Data(data.prefix(Int(length))) : nil
        }
        guard request.header("transfer-encoding") != nil else { return Data() }
        var offset = data.startIndex, decoded = Data()
        while true {
            guard let line = data[offset...].range(of: Data("\r\n".utf8)) else {
                if data.count - offset > 8192 { throw ParseError.status(400) }; return nil
            }
            guard line.lowerBound - offset <= 8192,
                  let sizeText = String(data: data[offset..<line.lowerBound], encoding: .ascii),
                  let digits = sizeText.split(separator: ";", maxSplits: 1).first,
                  !digits.isEmpty, digits.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }),
                  let count = UInt64(digits, radix: 16) else { throw ParseError.status(400) }
            guard count <= bodyLimit - decoded.count else { throw ParseError.status(413) }
            offset = line.upperBound
            if count == 0 {
                if data.count >= offset + 2 && data[offset..<offset+2] == Data("\r\n".utf8) { return decoded }
                guard let trailers = data[offset...].range(of: Data("\r\n\r\n".utf8)) else {
                    if data.count - offset > 65_536 { throw ParseError.status(431) }; return nil
                }
                guard trailers.upperBound - offset <= 65_536 else { throw ParseError.status(431) }
                return decoded
            }
            guard data.count >= offset + Int(count) + 2 else { return nil }
            guard data[offset+Int(count)..<offset+Int(count)+2] == Data("\r\n".utf8) else { throw ParseError.status(400) }
            decoded.append(data[offset..<offset+Int(count)]); offset += Int(count) + 2
        }
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
              request.values("absolute-host").allSatisfy(validHost), request.values("origin").count <= 1,
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
#endif
