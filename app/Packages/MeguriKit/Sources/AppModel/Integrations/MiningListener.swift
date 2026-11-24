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
    typealias Launcher = @Sendable (MiningHTTPRequest, UUID?) -> Task<MiningHTTPResponse, Never>
    private let launch: Launcher
    private let changed: @Sendable (State) -> Void
    private let accepting = Mutex(true)
    private let admission: MiningAdmission?
    private let admissionEpoch: UUID?
    private let port: UInt16
    private var listener: NWListener?
    private var ipv6Listener: NWListener?
    private var probe: NWConnection?
    private var peers: [UUID: Peer] = [:]
    private var running = false
    private var epoch = UUID()
    private var retry: DispatchWorkItem?
    private var ipv6Retry: DispatchWorkItem?
    private var stopWaiters: [AsyncStream<Void>.Continuation] = []
    private nonisolated final class Peer: @unchecked Sendable {
        let connection: NWConnection
        var data = Data()
        var head: MiningHTTPRequest?
        var body: MiningHTTPBody?
        var busy = false
        var receiving = false
        var inputEnded = false
        var websocket: MiningWebSocket?
        var task: Task<Void, Never>?
        var timer: DispatchWorkItem?
        var headStarted = false
        var headBytes = 0
        var closing = false
        var requestBytes = 0
        var outgoingBytes = 0
        var retainedBytes: Int {
            data.count + headBytes + (body?.retainedBytes ?? 0) + (websocket?.retainedBytes ?? 0) + requestBytes + outgoingBytes
        }
        init(_ connection: NWConnection) { self.connection = connection }
    }

    convenience init(
        port: UInt16 = 44432,
        admission: MiningAdmission? = nil,
        handler: @escaping Handler,
        changed: @escaping @Sendable (State) -> Void
    ) {
        self.init(port: port, admission: admission,
            launch: { request, token in Task { await handler(request, token) } }, changed: changed)
    }

    init(port: UInt16 = 44432, admission: MiningAdmission? = nil,
         launch: @escaping Launcher, changed: @escaping @Sendable (State) -> Void) {
        self.port = port
        self.admission = admission
        admissionEpoch = admission?.listenerEpoch
        self.launch = launch
        self.changed = changed
    }

    private var isAdmitting: Bool {
        guard accepting.withLock({ $0 }) else { return false }
        guard let admission else { return true }
        return admissionEpoch.flatMap { admission.current(for: $0) } != nil
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
        let completion = AsyncStream<Void>.makeStream()
        queue.async { [self] in
            stopWaiters.append(completion.continuation)
            stopOnQueue()
            finishStop()
        }
        for await _ in completion.stream { }
    }

    private func stopOnQueue() {
        running = false
        epoch = UUID()
        retry?.cancel()
        retry = nil
        ipv6Retry?.cancel()
        ipv6Retry = nil
        probe?.cancel()
        probe = nil
        listener?.cancel()
        listener = nil
        ipv6Listener?.cancel()
        ipv6Listener = nil
        for (id, peer) in peers {
            if peer.closing { continue }
            if peer.websocket != nil {
                send(MiningWebSocket.close(1001), id: id, close: true)
            } else if !peer.busy {
                remove(id)
            }
            // An admitted handler still owns its terminal reply. Pending
            // writes are retired by the receiver; an executing write finishes.
        }
        changed(.off)
    }

    private func finishStop() {
        guard !running, peers.isEmpty else { return }
        let waiters = stopWaiters
        stopWaiters.removeAll()
        waiters.forEach { $0.finish() }
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
            parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1",
                port: port == 0 ? NWEndpoint.Port.any : NWEndpoint.Port(rawValue: port)!)
            parameters.allowLocalEndpointReuse = false
            let value = try NWListener(using: parameters)
            listener = value
            value.stateUpdateHandler = { [weak self, weak value] state in
                guard let self, let value, self.listener === value else {
                    return
                }
                switch state {
                case .ready:
                    guard let boundPort = value.port?.rawValue, boundPort != 0 else {
                        value.cancel()
                        self.listener = nil
                        self.conflict()
                        return
                    }
                    // H22 ephemeral tests bind IPv4 only.
                    if self.port != 0 { self.listenIPv6(port: boundPort) }
                    self.changed(.listening(boundPort))
                case .waiting, .failed:
                    value.cancel()
                    self.listener = nil
                    self.ipv6Listener?.cancel()
                    self.ipv6Listener = nil
                    self.ipv6Retry?.cancel()
                    self.ipv6Retry = nil
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
        guard running, isAdmitting, listener?.port?.rawValue == port, ipv6Listener == nil else {
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
                switch state {
                case .waiting, .failed:
                    value.cancel()
                    self.ipv6Listener = nil
                    self.retryIPv6(port: port)
                default: break
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
            retryIPv6(port: port)
        }
    }

    private func retryIPv6(port: UInt16) {
        ipv6Retry?.cancel()
        let attempt = epoch
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.epoch == attempt else { return }
            self.ipv6Retry = nil
            self.listenIPv6(port: port)
        }
        ipv6Retry = work
        queue.asyncAfter(deadline: .now() + .seconds(10), execute: work)
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
        guard running, isAdmitting, httpCount < 128, case .hostPort(let host, _) = connection.endpoint,
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
        finishStop()
    }

    private var retainedBytes: Int {
        peers.values.reduce(0) { $0 + $1.retainedBytes }
    }

    private var httpCount: Int { peers.values.filter { $0.websocket == nil }.count }
    private var socketCount: Int { peers.values.filter { $0.websocket != nil }.count }

    private func send(_ bytes: Data, id: UUID, close: Bool,
                      completion: (@Sendable () -> Void)? = nil) {
        guard let peer = peers[id], !peer.closing else { return }
        guard retainedBytes + bytes.count <= 268_435_456 else { remove(id); return }
        peer.closing = close
        peer.outgoingBytes += bytes.count
        if close {
            peer.data = Data()
            peer.head = nil
            peer.headBytes = 0
            peer.body = nil
        }
        timeout(id, seconds: 30)
        peer.connection.send(content: bytes, completion: .contentProcessed { [self] error in
            guard peers[id] === peer else { return }
            peer.outgoingBytes -= bytes.count
            if close || error != nil { remove(id); return }
            if peer.websocket != nil {
                // A pong completion must not disarm a later pending close/pong.
                if peer.outgoingBytes == 0 { peer.timer?.cancel() }
                else { timeout(id, seconds: 30) }
            }
            completion?()
        })
    }

    private func receive(_ id: UUID) {
        guard let peer = peers[id], !peer.closing, !peer.busy, !peer.receiving,
              !peer.inputEnded, running, isAdmitting else { return }
        peer.receiving = true
        peer.connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, complete, error in
            guard let self, self.peers[id] === peer else { return }
            peer.receiving = false
            peer.inputEnded = complete
            if error != nil { self.remove(id); return }
            if let data, !data.isEmpty { self.consume(data, id: id) }
            guard self.peers[id] === peer else { return }
            if complete, !peer.busy, peer.outgoingBytes == 0 { self.remove(id); return }
            self.receive(id)
        }
    }

    private func consume(_ bytes: Data, id: UUID) {
        guard let peer = peers[id], !peer.closing, !peer.busy, running, isAdmitting else { return }
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
        peer.data.append(bytes)
        processHTTP(id)
    }

    private func processHTTP(_ id: UUID) {
        guard let peer = peers[id], !peer.closing, !peer.busy, running, isAdmitting else { return }
        if !peer.headStarted, !peer.data.isEmpty {
            peer.headStarted = true
            timeout(id, seconds: 10)
        }
        do {
            if peer.head == nil {
                guard let (head, offset) = try MiningHTTP.head(peer.data) else { return }
                peer.head = head
                peer.headBytes = offset
                peer.data = Data(peer.data.dropFirst(offset))
                if let response = MiningHTTP.preliminary(head, port: listener?.port?.rawValue ?? port, sockets: socketCount) {
                    if response.upgrade {
                        peer.websocket = MiningWebSocket()
                        peer.head = nil
                        peer.headBytes = 0
                        let extra = peer.data
                        peer.data = Data()
                        send(response.bytes, id: id, close: false)
                        if !extra.isEmpty { consume(extra, id: id) }
                    } else { send(response.bytes, id: id, close: true) }
                    return
                }
                peer.body = try MiningHTTPBody(request: head)
                if head.header("expect")?.lowercased() == "100-continue", peer.body?.complete == false {
                    send(Data("HTTP/1.1 100 Continue\r\n\r\n".utf8), id: id, close: false)
                }
            }
            guard let head = peer.head, var decoder = peer.body else { return }
            // Move the decoder out before mutating to avoid copying a retained
            // decoded body on every receive through Swift's copy-on-write.
            peer.body = nil
            let count = try decoder.consume(peer.data)
            peer.data = Data(peer.data.dropFirst(count))
            guard decoder.complete else {
                peer.body = decoder
                timeout(id, seconds: 30)
                return
            }
            var completedRequest = head
            completedRequest.body = decoder.body
            completedRequest.localPort = listener?.port?.rawValue ?? port
            let request = completedRequest
            peer.head = nil
            peer.requestBytes = request.body.count + peer.headBytes
            peer.headBytes = 0
            peer.busy = true
            peer.timer?.cancel()
            let admissionToken = admissionEpoch.flatMap { admission?.current(for: $0) }
            guard admission == nil || admissionToken != nil else { remove(id); return }
            // Register in body-completion order on the socket queue, before any
            // unstructured task can reorder writes at the receiver's actor hop.
            let response = launch(request, admissionToken)
            peer.task = Task { [self] in
                let value = await response.value
                queue.async { [self] in
                    guard peers[id] === peer else { return }
                    peer.requestBytes = 0
                    let closing = !request.keepsAlive || (peer.inputEnded && peer.data.isEmpty) || !running || !isAdmitting
                    send(MiningHTTP.cors(value, request: request).bytes(closing: closing), id: id, close: closing) { [self] in
                        guard peers[id] === peer else { return }
                        peer.task = nil
                        peer.busy = false
                        peer.headStarted = false
                        guard running, isAdmitting else { remove(id); return }
                        timeout(id, seconds: 5)
                        // Only the completed send unlocks the next request.
                        if !peer.data.isEmpty { processHTTP(id) }
                        receive(id)
                    }
                }
            }
        } catch MiningHTTP.ParseError.status(let status) {
            let response: MiningHTTPResponse
            if status == 413, let head = peer.head { response = MiningHTTP.cors(.error(400, "Request body is too large."), request: head) }
            else { response = .init(status: status) }
            send(response.bytes, id: id, close: true)
        } catch { send(MiningHTTPResponse(status: 400).bytes, id: id, close: true) }
    }
}
#endif
