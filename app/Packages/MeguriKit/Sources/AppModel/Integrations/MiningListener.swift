#if os(macOS)
import Foundation
import Network
import Synchronization

/// All socket state belongs to queue. Only value DTOs cross to the receiver;
/// no network read, parsing or frame work runs on the main actor.
nonisolated final class MiningListener: @unchecked Sendable {
    nonisolated enum State: Sendable, Equatable {
        case off, starting, listening(UInt16), portInUse
    }
    typealias Handler = @Sendable (MiningHTTPRequest, UUID?) async -> MiningHTTPResponse
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.mining", qos: .utility)
    private let handler: Handler
    private let changed: @Sendable (State) -> Void
    private let accepting = Mutex(true)
    private let admission: MiningAdmission?
    private let port: UInt16
    private var listener: NWListener?
    private var ipv6Listener: NWListener?
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
        var closing = false
        var requestBytes = 0
        var outgoingBytes = 0
        var retainedBytes: Int {
            data.count + (websocket?.retainedBytes ?? 0) + requestBytes + outgoingBytes
        }
        init(_ connection: NWConnection) { self.connection = connection }
    }

    init(
        port: UInt16 = 44432,
        admission: MiningAdmission? = nil,
        handler: @escaping Handler,
        changed: @escaping @Sendable (State) -> Void
    ) {
        self.port = port
        self.admission = admission
        self.handler = handler
        self.changed = changed
    }

    private var isAdmitting: Bool {
        accepting.withLock { $0 } && (admission == nil || admission?.current() != nil)
    }

    func start() {
        queue.async { [self] in
            guard !running, isAdmitting else {
                return
            }
            running = true
            changed(.starting)
            bind()
        }
    }

    /// Enqueue shutdown directly from the Settings change, before actor work.
    func requestStop() {
        accepting.withLock { $0 = false }
        queue.async { [self] in stopOnQueue() }
    }

    func stop() async {
        accepting.withLock { $0 = false }
        await withCheckedContinuation { continuation in
            queue.async { [self] in
                stopOnQueue()
                continuation.resume()
            }
        }
    }

    private func stopOnQueue() {
        running = false
        epoch = UUID()
        retry?.cancel()
        retry = nil
        probe?.cancel()
        probe = nil
        listener?.cancel()
        listener = nil
        ipv6Listener?.cancel()
        ipv6Listener = nil
        for (id, peer) in peers {
            if peer.websocket != nil {
                send(MiningWebSocket.close(1001), id: id, close: true)
            } else if peer.task == nil {
                remove(id)
            }
            // An admitted handler still owns its terminal reply. Pending
            // writes are retired by the receiver; an executing write finishes.
        }
        changed(.off)
    }

    private func bind() {
        guard running, isAdmitting, listener == nil, probe == nil else {
            return
        }
        if port == 0 {
            listen()
            return
        }
        let attempt = epoch
        // Probe without sending bytes so a wildcard listener cannot be stolen
        // by a more-specific loopback bind on macOS.
        let connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
        probe = connection
        let finish: @Sendable (Bool) -> Void = { [weak self, weak connection] free in
            guard let self, let connection, self.probe === connection, self.epoch == attempt else {
                return
            }
            self.probe = nil
            connection.cancel()
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
        guard running, isAdmitting else {
            return
        }
        do {
            let parameters = NWParameters.tcp
            parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
            parameters.allowLocalEndpointReuse = false
            let value = try NWListener(using: parameters)
            listener = value
            value.stateUpdateHandler = { [weak self, weak value] state in
                guard let self, let value, self.listener === value else {
                    return
                }
                switch state {
                case .ready:
                    let boundPort = value.port?.rawValue ?? self.port
                    self.listenIPv6(port: boundPort)
                    self.changed(.listening(boundPort))
                case .failed:
                    value.cancel()
                    self.listener = nil
                    self.ipv6Listener?.cancel()
                    self.ipv6Listener = nil
                    self.conflict()
                default: break
                }
            }
            value.newConnectionHandler = { [weak self, weak value] connection in
                guard let self, let value, self.listener === value else {
                    connection.cancel()
                    return
                }
                self.accept(connection)
            }
            value.start(queue: queue)
        } catch { conflict() }
    }

    /// A separate, explicitly loopback-only bind keeps IPv4 usable when the
    /// host has no IPv6 support or another process already owns ::1's port.
    private func listenIPv6(port: UInt16) {
        guard running, isAdmitting, ipv6Listener == nil else {
            return
        }
        do {
            let parameters = NWParameters.tcp
            parameters.requiredLocalEndpoint = .hostPort(host: "::1", port: NWEndpoint.Port(rawValue: port)!)
            parameters.allowLocalEndpointReuse = false
            let value = try NWListener(using: parameters)
            ipv6Listener = value
            value.stateUpdateHandler = { [weak self, weak value] state in
                guard let self, let value, self.ipv6Listener === value else {
                    return
                }
                if case .failed = state {
                    value.cancel()
                    self.ipv6Listener = nil
                }
            }
            value.newConnectionHandler = { [weak self, weak value] connection in
                guard let self, let value, self.ipv6Listener === value else {
                    connection.cancel()
                    return
                }
                self.accept(connection)
            }
            value.start(queue: queue)
        } catch {
            // IPv6 is best effort. The IPv4 listener remains authoritative.
        }
    }

    private func conflict() {
        guard running, isAdmitting else {
            return
        }
        changed(.portInUse)
        let work = DispatchWorkItem { [weak self] in self?.bind() }
        retry = work
        queue.asyncAfter(deadline: .now() + .seconds(10), execute: work)
    }

    private func accept(_ connection: NWConnection) {
        guard running, isAdmitting, peers.count < 128, case .hostPort(let host, _) = connection.endpoint,
              (host == .ipv4(.loopback) || host == .ipv6(.loopback)) else {
            connection.cancel()
            return
        }
        let id = UUID()
        peers[id] = Peer(connection)
        connection.stateUpdateHandler = { [weak self] state in
            if case .failed = state { self?.remove(id) }
            if case .cancelled = state { self?.remove(id) }
        }
        connection.start(queue: queue)
        timeout(id, seconds: 5)
        receive(id)
    }

    private func timeout(_ id: UUID, seconds: Int) {
        guard let peer = peers[id] else {
            return
        }
        peer.timer?.cancel()
        let item = DispatchWorkItem { [weak self] in self?.remove(id) }
        peer.timer = item
        queue.asyncAfter(deadline: .now() + .seconds(seconds), execute: item)
    }

    private func remove(_ id: UUID) {
        guard let peer = peers.removeValue(forKey: id) else {
            return
        }
        peer.timer?.cancel()
        peer.task?.cancel()
        peer.connection.cancel()
    }

    private var retainedBytes: Int {
        peers.values.reduce(0) { $0 + $1.retainedBytes }
    }

    private func send(_ bytes: Data, id: UUID, close: Bool) {
        guard let peer = peers[id], !peer.closing else {
            return
        }
        guard retainedBytes + bytes.count <= 268_435_456 else {
            remove(id)
            return
        }
        peer.closing = close
        peer.outgoingBytes += bytes.count
        if close {
            peer.data = Data()
            peer.head = nil
            peer.websocket = nil
        }
        timeout(id, seconds: 30)
        peer.connection.send(content: bytes, completion: .contentProcessed { [self] error in
            guard peers[id] === peer else {
                return
            }
            peer.outgoingBytes -= bytes.count
            if close || error != nil { remove(id) }
            else if peer.websocket != nil { peer.timer?.cancel() }
        })
    }

    private func receive(_ id: UUID) {
        guard let peer = peers[id], !peer.closing, running, isAdmitting else {
            return
        }
        peer.connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, complete, error in
            guard let self, self.peers[id] === peer else {
                return
            }
            if let data, !data.isEmpty { self.consume(data, id: id) }
            if error != nil || (complete && peer.task == nil) { self.remove(id); return }
            if self.peers[id] != nil, peer.task == nil, !peer.closing { self.receive(id) }
        }
    }

    private func consume(_ bytes: Data, id: UUID) {
        guard let peer = peers[id], !peer.closing, running, isAdmitting else {
            return
        }
        // Include HTTP bodies, WebSocket fragments and queued socket sends.
        // Check before dispatching to either parser so upgrades cannot bypass it.
        guard retainedBytes + bytes.count <= 268_435_456 else {
            if peer.websocket != nil { send(MiningWebSocket.close(1009), id: id, close: true) }
            else { send(MiningHTTPResponse.error(503, "Mining request memory limit reached.").bytes, id: id, close: true) }
            return
        }
        if var socket = peer.websocket {
            let events = socket.receive(bytes)
            peer.websocket = socket
            for event in events {
                switch event {
                case .send(let data): send(data, id: id, close: false)
                case .close(let data): send(data, id: id, close: true)
                }
            }
            return
        }
        if !peer.headStarted {
            peer.headStarted = true
            timeout(id, seconds: 10)
        }
        peer.data.append(bytes)
        do {
            if peer.head == nil {
                guard let head = try MiningHTTP.head(peer.data) else {
                    return
                }
                peer.head = head
                let sockets = peers.values.filter { $0.websocket != nil }.count
                if let response = MiningHTTP.preliminary(head.0, port: listener?.port?.rawValue ?? port, sockets: sockets) {
                    if response.upgrade {
                        peer.websocket = MiningWebSocket()
                        peer.head = nil
                        peer.timer?.cancel()
                        let extra = Data(peer.data.dropFirst(head.1))
                        peer.data = Data()
                        send(response.bytes, id: id, close: false)
                        if !extra.isEmpty { consume(extra, id: id) }
                    } else { send(response.bytes, id: id, close: true) }
                    return
                }
                if let length = head.0.header("content-length").flatMap(UInt64.init), length > MiningHTTP.bodyLimit {
                    send(MiningHTTP.cors(.error(400, "Request body is too large."), request: head.0).bytes, id: id, close: true)
                    return
                }
                if head.0.header("expect")?.lowercased() == "100-continue", !peer.continued {
                    peer.continued = true
                    send(Data("HTTP/1.1 100 Continue\r\n\r\n".utf8), id: id, close: false)
                }
            }
            guard let (head, offset) = peer.head else {
                return
            }
            if let length = head.header("content-length").flatMap(Int.init), peer.data.count - offset < length {
                timeout(id, seconds: 30)
                return
            }
            guard peer.data.count <= MiningHTTP.bodyLimit + 1_048_576 else {
                throw MiningHTTP.ParseError.status(413)
            }
            guard let body = try MiningHTTP.body(Data(peer.data.dropFirst(offset)), request: head) else {
                timeout(id, seconds: 30)
                return
            }
            var completedRequest = head
            completedRequest.body = body
            let request = completedRequest
            peer.data = Data()
            peer.head = nil
            peer.requestBytes = body.count
            peer.timer?.cancel()
            let handler = self.handler
            let admissionToken = admission?.current()
            guard admission == nil || admissionToken != nil else {
                remove(id)
                return
            }
            peer.task = Task { [self] in
                let response = await handler(request, admissionToken)
                queue.async { [self] in
                    guard peers[id] === peer else {
                        return
                    }
                    peer.requestBytes = 0
                    send(MiningHTTP.cors(response, request: request).bytes, id: id, close: true)
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
