#if os(macOS)
import Foundation
import Network

/// All socket state belongs to queue. Only value DTOs cross to the receiver;
/// no network read, parsing or frame work runs on the main actor.
nonisolated final class MiningListener: @unchecked Sendable {
    nonisolated enum State: Sendable, Equatable { case off, starting, listening(UInt16), portInUse }
    typealias Handler = @Sendable (MiningHTTPRequest) async -> MiningHTTPResponse
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.mining", qos: .utility)
    private let handler: Handler
    private let changed: @Sendable (State) -> Void
    private let port: UInt16
    private var listener: NWListener?
    private var probe: NWConnection?
    private var peers: [UUID: Peer] = [:]
    private var running = false
    private var epoch = UUID()
    private var retry: DispatchWorkItem?
    private nonisolated final class Peer: @unchecked Sendable {
        let connection: NWConnection
        var data = Data()
        var head: (MiningHTTPRequest, Int)?
        var websocket: MiningWebSocket?
        var task: Task<Void, Never>?
        var timer: DispatchWorkItem?
        var continued = false
        var headStarted = false
        init(_ connection: NWConnection) { self.connection = connection }
    }
    init(port: UInt16 = 44432, handler: @escaping Handler, changed: @escaping @Sendable (State) -> Void) {
        self.port = port; self.handler = handler; self.changed = changed
    }
    func start() { queue.async { [self] in guard !running else { return }; running = true; changed(.starting); bind() } }
    func stop() async {
        await withCheckedContinuation { continuation in
            queue.async { [self] in
                running = false; epoch = UUID(); retry?.cancel(); retry = nil
                probe?.cancel(); probe = nil; listener?.cancel(); listener = nil
                for (id, peer) in peers {
                    peer.task?.cancel()
                    if peer.websocket != nil { send(MiningWebSocket.close(1001), id: id, close: true) }
                    else { remove(id) }
                }
                changed(.off); continuation.resume()
            }
        }
    }
    private func bind() {
        guard running, listener == nil, probe == nil else { return }
        if port == 0 { listen(); return }
        let attempt = epoch
        // Probe without sending bytes so a wildcard listener cannot be stolen
        // by a more-specific loopback bind on macOS.
        let connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
        probe = connection
        let finish: @Sendable (Bool) -> Void = { [weak self, weak connection] free in
            guard let self, let connection, self.probe === connection, self.epoch == attempt else { return }
            self.probe = nil; connection.cancel()
            if free { self.listen() } else { self.conflict() }
        }
        // Network calls this handler on queue; the timeout is on queue too.
        connection.stateUpdateHandler = { state in
            switch state {
            case .ready: finish(false)
            case .failed(let error):
                if case .posix(.ECONNREFUSED) = error { finish(true) } else { finish(false) }
            default: break
            }
        }
        connection.start(queue: queue)
        queue.asyncAfter(deadline: .now() + .milliseconds(500)) { finish(false) }
        // The identity check above consumes completion exactly once.
    }
    private func listen() {
        guard running else { return }
        do {
            let parameters = NWParameters.tcp
            parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
            parameters.allowLocalEndpointReuse = false
            let value = try NWListener(using: parameters)
            listener = value
            value.stateUpdateHandler = { [weak self, weak value] state in
                guard let self, let value, self.listener === value else { return }
                switch state {
                case .ready: self.changed(.listening(value.port?.rawValue ?? self.port))
                case .failed: value.cancel(); self.listener = nil; self.conflict()
                default: break
                }
            }
            value.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
            value.start(queue: queue)
        } catch { conflict() }
    }
    private func conflict() {
        guard running else { return }; changed(.portInUse)
        let work = DispatchWorkItem { [weak self] in self?.bind() }
        retry = work; queue.asyncAfter(deadline: .now() + .seconds(10), execute: work)
    }
    private func accept(_ connection: NWConnection) {
        guard running, peers.count < 128, case .hostPort(let host, _) = connection.endpoint,
              host == .ipv4(.loopback) else { connection.cancel(); return }
        let id = UUID(); peers[id] = Peer(connection)
        connection.stateUpdateHandler = { [weak self] state in
            if case .failed = state { self?.remove(id) }
            if case .cancelled = state { self?.remove(id) }
        }
        connection.start(queue: queue); timeout(id, seconds: 5); receive(id)
    }
    private func timeout(_ id: UUID, seconds: Int) {
        guard let peer = peers[id] else { return }; peer.timer?.cancel()
        let item = DispatchWorkItem { [weak self] in self?.remove(id) }
        peer.timer = item; queue.asyncAfter(deadline: .now() + .seconds(seconds), execute: item)
    }
    private func remove(_ id: UUID) {
        guard let peer = peers.removeValue(forKey: id) else { return }
        peer.timer?.cancel(); peer.task?.cancel(); peer.connection.cancel()
    }
    private func send(_ bytes: Data, id: UUID, close: Bool) {
        guard let peer = peers[id] else { return }
        timeout(id, seconds: 30)
        peer.connection.send(content: bytes, completion: .contentProcessed { [weak self] error in
            guard let self else { return }
            if close || error != nil { self.remove(id) }
            else if peer.websocket != nil { peer.timer?.cancel() }
        })
    }
    private func receive(_ id: UUID) {
        guard let peer = peers[id] else { return }
        peer.connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, complete, error in
            guard let self, self.peers[id] === peer else { return }
            if let data, !data.isEmpty { self.consume(data, id: id) }
            if error != nil || (complete && peer.task == nil) { self.remove(id); return }
            if self.peers[id] != nil, peer.task == nil { self.receive(id) }
        }
    }
    private func consume(_ bytes: Data, id: UUID) {
        guard let peer = peers[id] else { return }
        if var socket = peer.websocket {
            let events = socket.receive(bytes); peer.websocket = socket
            for event in events {
                switch event { case .send(let data): send(data, id: id, close: false)
                case .close(let data): send(data, id: id, close: true) }
            }; return
        }
        if !peer.headStarted { peer.headStarted = true; timeout(id, seconds: 10) }
        if peers.values.reduce(0, { $0 + $1.data.count }) + bytes.count > 268_435_456 {
            send(MiningHTTPResponse.error(503, "Mining request memory limit reached.").bytes, id: id, close: true); return
        }
        peer.data.append(bytes)
        do {
            if peer.head == nil {
                guard let head = try MiningHTTP.head(peer.data) else { return }
                peer.head = head
                let sockets = peers.values.filter { $0.websocket != nil }.count
                if let response = MiningHTTP.preliminary(head.0, port: listener?.port?.rawValue ?? port, sockets: sockets) {
                    if response.upgrade {
                        peer.websocket = MiningWebSocket(); peer.timer?.cancel()
                        let extra = Data(peer.data.dropFirst(head.1)); peer.data = Data()
                        send(response.bytes, id: id, close: false)
                        if !extra.isEmpty { consume(extra, id: id) }
                    } else { send(response.bytes, id: id, close: true) }
                    return
                }
                if let length = head.0.header("content-length").flatMap(UInt64.init), length > MiningHTTP.bodyLimit {
                    send(MiningHTTP.cors(.error(400, "Request body is too large."), request: head.0).bytes, id: id, close: true); return
                }
                if head.0.header("expect")?.lowercased() == "100-continue", !peer.continued {
                    peer.continued = true; send(Data("HTTP/1.1 100 Continue\r\n\r\n".utf8), id: id, close: false)
                }
            }
            guard let (head, offset) = peer.head else { return }
            if let length = head.header("content-length").flatMap(Int.init), peer.data.count - offset < length {
                timeout(id, seconds: 30); return
            }
            guard peer.data.count <= MiningHTTP.bodyLimit + 1_048_576 else { throw MiningHTTP.ParseError.status(413) }
            guard let body = try MiningHTTP.body(Data(peer.data.dropFirst(offset)), request: head) else { timeout(id, seconds: 30); return }
            var completedRequest = head; completedRequest.body = body
            let request = completedRequest
            peer.data = Data(); peer.timer?.cancel()
            let handler = self.handler, attempt = epoch
            peer.task = Task { [weak self] in
                let response = await handler(request)
                self?.queue.async { [weak self] in
                    guard let self, self.running, self.epoch == attempt, self.peers[id] === peer else { return }
                    self.send(MiningHTTP.cors(response, request: request).bytes, id: id, close: true)
                }
            }
        } catch MiningHTTP.ParseError.status(let status) {
            let response: MiningHTTPResponse
            if status == 413, let head = peer.head?.0 { response = MiningHTTP.cors(.error(400, "Request body is too large."), request: head) }
            else { response = .init(status: status) }
            send(response.bytes, id: id, close: true)
        } catch { send(MiningHTTPResponse(status: 400).bytes, id: id, close: true) }
    }
}
#endif
