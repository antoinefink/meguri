import Foundation
import Network

/// Only a response to an unauthenticated HEAD request; never account or sync validation.
nonisolated public enum EndpointReachabilityResult: Equatable, Sendable {
    case response(Int)
    case unavailable
    case timedOut
    case secureConnectionFailed
    case cancelled

    public var description: String {
        switch self {
        case .response(let code): "HTTP response received (\(code)). Login and server compatibility have not been verified. Redirects are not followed."
        case .unavailable: "No HTTP response received. Check the address, network and local network permission."
        case .timedOut: "The connection test timed out without an HTTP response."
        case .secureConnectionFailed: "A trusted secure connection could not be established."
        case .cancelled: "Connection test cancelled."
        }
    }
}

nonisolated public enum EndpointReachability {
    public static func check(_ normalizedEndpoint: String) async -> EndpointReachabilityResult {
        guard let url = URL(string: normalizedEndpoint),
              let scheme = url.scheme, ["http", "https"].contains(scheme), url.host != nil,
              url.user == nil, url.password == nil, url.query == nil, url.fragment == nil else { return .unavailable }
        if scheme == "http" {
            return await PlainHTTPProbe(url: url).response()
        }
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCredentialStorage = nil
        configuration.urlCache = nil
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 10
        configuration.tlsMinimumSupportedProtocolVersion = .TLSv12
        let session = URLSession(configuration: configuration, delegate: ReachabilityDelegate(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
        request.httpMethod = "HEAD"
        request.httpShouldHandleCookies = false
        do {
            // Stop at response headers. Never collect or diagnose a response body.
            let (_, response) = try await session.bytes(for: request)
            guard let http = response as? HTTPURLResponse else { return .unavailable }
            return .response(http.statusCode)
        } catch let error as URLError {
            switch error.code {
            case .cancelled: return .cancelled
            case .timedOut: return .timedOut
            case .secureConnectionFailed, .serverCertificateUntrusted, .serverCertificateHasBadDate,
                 .serverCertificateHasUnknownRoot, .serverCertificateNotYetValid: return .secureConnectionFailed
            default: return .unavailable
            }
        } catch { return .unavailable }
    }
}

private nonisolated final class ReachabilityDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                    completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, didReceive value: URLAuthenticationChallenge,
                    completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        // Platform trust evaluation only. A nil credential continues without
        // answering server/proxy authentication or consulting a credential store.
        completionHandler(value.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust
            ? .performDefaultHandling : .useCredential, nil)
    }
}

/// All mutable state is confined to queue, including cancellation and timeout.
private nonisolated final class PlainHTTPProbe: @unchecked Sendable {
    private let queue = DispatchQueue(label: "Meguri.connection-test")
    private let url: URL
    private var connection: NWConnection?
    private var continuation: CheckedContinuation<EndpointReachabilityResult, Never>?
    private var finished = false
    private var bytes = Data()
    init(url: URL) { self.url = url }
    func response() async -> EndpointReachabilityResult {
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                queue.async { self.start(continuation) }
            }
        } onCancel: { self.queue.async { self.finish(.cancelled) } }
    }
    private func start(_ continuation: CheckedContinuation<EndpointReachabilityResult, Never>) {
        guard !finished else { continuation.resume(returning: .cancelled); return }
        self.continuation = continuation
        guard let host = url.host, let port = NWEndpoint.Port(rawValue: UInt16(exactly: url.port ?? 80) ?? 0),
              port.rawValue != 0, let parts = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
            finish(.unavailable); return
        }
        let networkHost = host.hasPrefix("[") && host.hasSuffix("]") ? String(host.dropFirst().dropLast()) : host
        let connection = NWConnection(host: NWEndpoint.Host(networkHost), port: port, using: .tcp)
        self.connection = connection
        connection.stateUpdateHandler = { state in
            switch state {
            case .ready:
                let path = parts.percentEncodedPath.isEmpty ? "/" : parts.percentEncodedPath
                let authority = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
                let request = "HEAD \(path) HTTP/1.1\r\nHost: \(authority):\(port.rawValue)\r\nConnection: close\r\n\r\n"
                connection.send(content: Data(request.utf8), completion: .contentProcessed { error in
                    if error != nil { self.finish(.unavailable) } else { self.receive() }
                })
            case .failed: self.finish(.unavailable)
            default: break
            }
        }
        connection.start(queue: queue)
        queue.asyncAfter(deadline: .now() + 10) { self.finish(.timedOut) }
    }
    private func receive() {
        guard !finished else { return }
        connection?.receive(minimumIncompleteLength: 1, maximumLength: 1024) { data, _, complete, error in
            if let data { self.bytes.append(data) }
            if let end = self.bytes.range(of: Data("\r\n".utf8)) {
                let line = String(decoding: self.bytes[..<end.lowerBound], as: UTF8.self).split(separator: " ")
                guard line.count >= 2, ["HTTP/1.0", "HTTP/1.1"].contains(String(line[0])),
                      line[1].count == 3, let code = Int(line[1]), (100...599).contains(code) else {
                    self.finish(.unavailable); return
                }
                self.finish(.response(code))
            } else if error != nil || complete || self.bytes.count >= 8192 { self.finish(.unavailable) }
            else { self.receive() }
        }
    }
    private func finish(_ result: EndpointReachabilityResult) {
        guard !finished else { return }
        finished = true
        connection?.stateUpdateHandler = nil
        connection?.cancel(); connection = nil
        continuation?.resume(returning: result); continuation = nil
        bytes.removeAll()
    }
}
