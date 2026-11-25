#if os(macOS)
@testable import AppModel
import Darwin
import Foundation
import Testing
import Synchronization

@MainActor @Suite struct SentenceMiningPortTests {
    @Test func independentDevicePreferencesPersistAcrossRestarts() throws {
        let suite = "SentenceMiningPortTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let fresh = SentenceMining(defaults: defaults, collectionKey: "synthetic-first")
        #expect(!fresh.migaku.enabled && !fresh.ankiConnect.enabled && !fresh.enabled)
        #expect(fresh.migaku.port == 44432 && fresh.ankiConnect.port == 8765)
        for migaku in [false, true] {
            for ankiConnect in [false, true] {
                fresh.setEnabled(migaku, for: .migaku)
                fresh.setEnabled(ankiConnect, for: .ankiConnect)
                let reopened = SentenceMining(defaults: defaults, collectionKey: "synthetic-other")
                #expect(reopened.migaku.enabled == migaku && reopened.ankiConnect.enabled == ankiConnect)
                #expect(reopened.enabled == (migaku || ankiConnect))
                #expect(defaults.bool(forKey: "mining.migaku.enabled") == migaku)
                #expect(defaults.bool(forKey: "mining.ankiConnect.enabled") == ankiConnect)
            }
        }
        // Test overrides affect bound ports only, never saved enable choices.
        let ephemeral = SentenceMining(defaults: defaults, collectionKey: "synthetic", ports: (0, 0))
        #expect(ephemeral.migaku.port == 0 && ephemeral.ankiConnect.port == 0)
        #expect(ephemeral.migaku.enabled && ephemeral.ankiConnect.enabled)
    }

    @Test func legacyKnownPortsMigrateOnceAndUnknownPortsStayOff() throws {
        let suite = "SentenceMiningLegacyTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let ports: [Any?] = [nil, 44432, 8765, 12345, "partial", 44432.5, true]
        for enabled in [false, true] {
            for port in ports {
                defaults.removePersistentDomain(forName: suite)
                defaults.set(enabled, forKey: "mining.enabled")
                if let port { defaults.set(port, forKey: "mining.port") }
                let known = port.map { String(describing: $0) } ?? "44432"
                let model = SentenceMining(defaults: defaults, collectionKey: "synthetic")
                #expect(model.migaku.enabled == (enabled && known == "44432"))
                #expect(model.ankiConnect.enabled == (enabled && known == "8765"))
                #expect(defaults.object(forKey: "mining.enabled") == nil)
                #expect(defaults.object(forKey: "mining.port") == nil)
                model.setEnabled(true, for: .migaku)
                model.setEnabled(true, for: .ankiConnect)
                // A stale legacy value cannot override the independent bits.
                defaults.set(true, forKey: "mining.enabled")
                defaults.set(12345, forKey: "mining.port")
                let reopened = SentenceMining(defaults: defaults, collectionKey: "synthetic")
                #expect(reopened.migaku.enabled && reopened.ankiConnect.enabled)
            }
        }
    }

    @Test(.timeLimit(.minutes(1))) func independentEndpointsRetainPeersThroughNoOpDisableAndConflict() async throws {
        let first = try MiningPortReservation()
        let occupied = try MiningPortReservation()
        defer { first.close(); occupied.close() }
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic-wire", ports: (first.port, occupied.port))
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 5
        configuration.timeoutIntervalForResource = 5
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        let socket = MiningWireTestClient(port: first.port)
        defer { socket.close() }
        do {
            model.firstOpenAttemptEnded()
            await model.settle()
            #expect(!model.enabled)
            first.close()
            let stillFree = try MiningPortReservation(port: first.port)
            stillFree.close()
            model.setEnabled(true, for: .migaku)
            await model.settle()
            try await waitForStatus(model.migaku) { $0.hasPrefix("Listening") }
            #expect(model.migaku.statusState == .listening && model.ankiConnect.statusState == .off)
            #expect(!model.ankiConnect.enabled)
            try await socket.start()
            try await socket.upgrade(port: first.port)
            try await socket.frame(1, payload: Data(#"{"id":1,"msg":"hello","data":{}}"#.utf8))
            let ping = Data("retained-peer".utf8)
            model.setEnabled(true, for: .migaku) // No-op retains the same FD.
            await model.settle()
            try await socket.frame(9, payload: ping)
            try await socket.expectFrame(10, payload: ping)
            let initialStatus = model.migaku.status

            model.setEnabled(true, for: .ankiConnect)
            await model.settle()
            try await waitForStatus(model.ankiConnect) { $0.contains("is in use") }
            #expect(model.ankiConnect.statusState == .portInUse && model.migaku.statusState == .listening)
            #expect(model.enabled && model.migaku.status == initialStatus)
            try await socket.frame(9, payload: ping)
            try await socket.expectFrame(10, payload: ping)
            #expect(try await exchange(session, port: first.port, path: "/anki-hello").status == 200)
            // Automatic retry is per endpoint and cannot replace its sibling.
            occupied.close()
            try await waitForStatus(model.ankiConnect, timeout: .seconds(15)) { $0.hasPrefix("Listening") }
            #expect(model.ankiConnect.statusState == .listening)
            #expect(model.migaku.port == first.port && model.ankiConnect.port == occupied.port)
            for port in [first.port, occupied.port] {
                let hello = try await exchange(session, port: port, path: "/anki-hello")
                #expect(hello.status == 200 && hello.json?["port"] as? Int == Int(port))
                let wrongPort = port == first.port ? occupied.port : first.port
                #expect(try await exchange(session, port: port, path: "/anki-hello", hostPort: wrongPort).status == 403)
                let version = try await exchange(session, port: port, path: "/", body: Data(#"{"action":"version","version":6}"#.utf8))
                #expect(version.status == 200 && version.json?["result"] as? Int == 6)
            }
            let sibling = MiningWireTestClient(port: occupied.port)
            defer { sibling.close() }
            try await sibling.start()
            try await sibling.upgrade(port: occupied.port)
            model.setEnabled(false, for: .migaku)
            await model.settle()
            try await socket.expectFrame(8, payload: Data([3, 233]))
            try await socket.expectEnd()
            try expectMiningPortClosed(first.port)
            try await sibling.frame(9, payload: ping)
            try await sibling.expectFrame(10, payload: ping)
            #expect(model.enabled && model.ankiConnect.status.hasPrefix("Listening"))
            model.setEnabled(true, for: .migaku)
            await model.settle()
            try await waitForStatus(model.migaku) { $0.hasPrefix("Listening") }
            model.setEnabled(false, for: .ankiConnect)
            await model.settle()
            try await sibling.expectFrame(8, payload: Data([3, 233]))
            try await sibling.expectEnd()
            #expect(try await exchange(session, port: first.port, path: "/anki-hello").status == 200)
            await model.shutdown()
            try expectMiningPortClosed(first.port)
            try expectMiningPortClosed(occupied.port)
        } catch {
            await model.shutdown()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func retiringEndpointKeepsItsAdmittedReplyAndCannotRestart() async throws {
        let gate = MiningPortReplyGate()
        let admission = MiningAdmission()
        _ = admission.update(enabled: true)
        let oldEvents = Mutex<[MiningListener.State]>([])
        let newEvents = Mutex<[MiningListener.State]>([])
        let old = MiningListener(port: 0, admission: admission, handler: { _, _ in
            await gate.hold()
            return .json(200, ["finished": true])
        }, changed: { state in oldEvents.withLock { $0.append(state) } })
        var replacement: MiningListener?
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 5
        configuration.timeoutIntervalForResource = 5
        let session = URLSession(configuration: configuration)
        defer {
            session.invalidateAndCancel()
        }
        old.start()
        do {
            let oldPort = try await listeningPort(oldEvents)
            let url = URL(string: "http://127.0.0.1:\(oldPort)/anki-hello")!
            let pending = Task { try await session.data(from: url) }
            try await gate.waitUntilEntered()
            _ = admission.update(enabled: true)
            replacement = MiningListener(port: 0, admission: admission, handler: { _, _ in
                .json(200, ["replacement": true])
            }, changed: { state in newEvents.withLock { $0.append(state) } })
            await #expect(throws: (any Error).self) {
                _ = try await exchange(session, port: oldPort, path: "/anki-hello")
            }
            #expect(oldEvents.withLock { $0.contains(.off) })
            old.requestStop()
            let stopped = Task { await old.stop() }
            replacement?.start()
            let newPort = try await listeningPort(newEvents)
            #expect(newPort != oldPort)
            try expectMiningPortClosed(oldPort)
            // Join a late start callback; a retired listener remains closed.
            old.start()
            try expectMiningPortClosed(oldPort)
            await gate.release()
            await stopped.value
            let (data, response) = try await pending.value
            #expect((response as? HTTPURLResponse)?.statusCode == 200)
            #expect((try JSONSerialization.jsonObject(with: data) as? [String: Bool])?["finished"] == true)
            let active = try await exchange(session, port: newPort, path: "/anki-hello")
            #expect(active.status == 200 && active.json?["replacement"] as? Bool == true)
            await replacement?.stop()
        } catch {
            await gate.release()
            await old.stop()
            await replacement?.stop()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func oldListenerCannotBorrowTheReplacementAdmissionEpoch() async throws {
        let admission = MiningAdmission()
        _ = admission.update(enabled: true)
        let calls = Mutex(0)
        let events = Mutex<[MiningListener.State]>([])
        let old = MiningListener(port: 0, admission: admission, handler: { _, _ in
            calls.withLock { $0 += 1 }
            return .json(200, ["unexpected": true])
        }, changed: { state in events.withLock { $0.append(state) } })
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForResource = 2
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        old.start()
        do {
            let port = try await listeningPort(events)
            // Deliberately delay requestStop: the synchronous admission fence
            // alone must refuse dispatch from this listener's old epoch.
            _ = admission.update(enabled: true)
            await #expect(throws: (any Error).self) {
                _ = try await exchange(session, port: port, path: "/anki-hello")
            }
            #expect(calls.withLock { $0 } == 0)
            // Readability after retirement must cancel accepting sources,
            // rather than spinning until a later explicit shutdown.
            #expect(events.withLock { $0.contains(.off) })
            let released = try MiningPortReservation(port: port)
            released.close()
            await old.stop()
        } catch {
            await old.stop()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func cancellingTestWaitsNeedsNoGateReleaseOrStatusChange() async throws {
        let gate = MiningPortReplyGate()
        let observer = Task { try await gate.waitUntilEntered() }
        observer.cancel()
        await #expect(throws: CancellationError.self) { try await observer.value }
        let held = Task { await gate.hold() }
        try await gate.waitUntilEntered()
        held.cancel()
        await held.value
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic-cancellation")
        let status = Task { try await waitForStatus(model.migaku) { $0.hasPrefix("Listening") } }
        status.cancel()
        await #expect(throws: CancellationError.self) { try await status.value }
    }

    @Test(.timeLimit(.minutes(1))) func transientIPv6ConflictRecoversWithoutReplacingIPv4() async throws {
        let reservation = try MiningPortReservation()
        defer { reservation.close() }
        let ipv6 = Darwin.socket(AF_INET6, SOCK_STREAM, 0)
        try #require(ipv6 >= 0)
        var ipv6Open = true
        defer { if ipv6Open { Darwin.close(ipv6) } }
        var yes: Int32 = 1
        try #require(setsockopt(ipv6, Int32(IPPROTO_IPV6), IPV6_V6ONLY, &yes, socklen_t(MemoryLayout.size(ofValue: yes))) == 0)
        var address = sockaddr_in6()
        address.sin6_len = UInt8(MemoryLayout<sockaddr_in6>.size)
        address.sin6_family = sa_family_t(AF_INET6)
        address.sin6_port = reservation.port.bigEndian
        address.sin6_addr = in6addr_loopback
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(ipv6, $0, socklen_t(MemoryLayout<sockaddr_in6>.size))
            }
        }
        try #require(bound == 0 && Darwin.listen(ipv6, 8) == 0)
        let events = Mutex<[MiningListener.State]>([])
        let listener = MiningListener(port: reservation.port, handler: { _, _ in
            .json(200, ["sameListener": true])
        }, changed: { state in events.withLock { $0.append(state) } })
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForResource = 1
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        reservation.close()
        listener.start()
        do {
            let port = try await listeningPort(events)
            #expect(port == reservation.port)
            #expect(try await exchange(session, port: port, path: "/anki-hello").status == 200)
            var request = URLRequest(url: URL(string: "http://[::1]:\(port)/anki-hello")!)
            request.timeoutInterval = 0.25
            // The foreign socket accepts TCP but cannot produce our reply.
            await #expect(throws: (any Error).self) { _ = try await session.data(for: request) }
            Darwin.close(ipv6)
            ipv6Open = false
            let deadline = ContinuousClock.now + .seconds(15)
            var recovered = false
            while !recovered, ContinuousClock.now < deadline {
                try Task.checkCancellation()
                if let (_, reply) = try? await session.data(for: request) {
                    recovered = (reply as? HTTPURLResponse)?.statusCode == 200
                }
                if !recovered { try await Task.sleep(for: .milliseconds(50)) }
            }
            #expect(recovered)
            #expect(try await exchange(session, port: port, path: "/anki-hello").status == 200)
            #expect(events.withLock { $0 } == [.starting, .listening(port)])
            await listener.stop()
        } catch {
            await listener.stop()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func socketDeadlineCompletesWithoutPongCallbackAndIgnoresLateCallback() async throws {
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        // No connection is started: stand in for Foundation retaining its
        // callback indefinitely, then delivering it after cancellation.
        let socket = session.webSocketTask(with: URL(string: "ws://127.0.0.1/anki-connect")!)
        let callback = Mutex<(@Sendable ((any Error)?) -> Void)?>(nil)
        let exited = Mutex(false)
        await #expect(throws: MiningSocketDeadlineExceeded.self) {
            try await withSocketDeadline(socket, stage: "missing pong", timeout: .milliseconds(100)) {
                defer { exited.withLock { $0 = true } }
                try await miningAwaitPong { value in callback.withLock { $0 = value } }
            }
        }
        let deadline = ContinuousClock.now + .seconds(1)
        while !exited.withLock({ $0 }), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(exited.withLock { $0 })
        let late = try #require(callback.withLock { $0 })
        late(nil)
        late(URLError(.cancelled))
        let cancellationCallback = Mutex<(@Sendable ((any Error)?) -> Void)?>(nil)
        let cancelled = Task {
            try await miningAwaitPong { value in cancellationCallback.withLock { $0 = value } }
        }
        let cancellationDeadline = ContinuousClock.now + .seconds(1)
        while cancellationCallback.withLock({ $0 == nil }), ContinuousClock.now < cancellationDeadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        let afterCancellation = try #require(cancellationCallback.withLock { $0 })
        cancelled.cancel()
        await #expect(throws: CancellationError.self) { try await cancelled.value }
        afterCancellation(nil)
    }

    private func listeningPort(_ events: borrowing Mutex<[MiningListener.State]>) async throws -> UInt16 {
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            if let port = events.withLock({ $0.compactMap { state -> UInt16? in
                if case .listening(let port) = state { return port }
                return nil
            }.last }) {
                try #require(port != 0)
                return port
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw MiningFailure("Mining listener did not become ready.")
    }

    private func waitForStatus(_ model: SentenceMining.Endpoint, timeout: Duration = .seconds(5), matching predicate: (String) -> Bool) async throws {
        let deadline = ContinuousClock.now + timeout
        while !predicate(model.status) {
            try Task.checkCancellation()
            guard ContinuousClock.now < deadline else { throw MiningFailure("Mining status did not settle: \(model.status)") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    private func exchange(_ session: URLSession, port: UInt16, path: String, hostPort: UInt16? = nil, body: Data? = nil) async throws -> (status: Int, json: [String: Any]?) {
        var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)\(path)")!)
        request.timeoutInterval = 5
        if let hostPort { request.setValue("127.0.0.1:\(hostPort)", forHTTPHeaderField: "Host") }
        if let body {
            request.httpMethod = "POST"
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await session.data(for: request)
        return (try #require((response as? HTTPURLResponse)?.statusCode),
                (try? JSONSerialization.jsonObject(with: data)) as? [String: Any])
    }

}

func miningSendPing(_ socket: URLSessionWebSocketTask, stage: String = "WebSocket ping") async throws {
    try await withSocketDeadline(socket, stage: stage) {
        try await miningAwaitPong { socket.sendPing(pongReceiveHandler: $0) }
    }
}

// Finishing a stream releases the waiter even when Foundation never calls back;
// a late callback after cancellation is ignored by the finished continuation.
nonisolated func miningAwaitPong(_ start: @Sendable (@escaping @Sendable ((any Error)?) -> Void) -> Void) async throws {
    let reply = AsyncThrowingStream<Void, any Error>.makeStream()
    try await withTaskCancellationHandler {
        try Task.checkCancellation()
        start { error in
            if let error { reply.continuation.finish(throwing: error) }
            else { reply.continuation.yield(()); reply.continuation.finish() }
        }
        for try await _ in reply.stream { return }
        throw CancellationError()
    } onCancel: {
        reply.continuation.finish(throwing: CancellationError())
    }
}

nonisolated struct MiningSocketDeadlineExceeded: Error {
    let stage: String
}

func withSocketDeadline(_ socket: URLSessionWebSocketTask,
                        stage: String = "WebSocket operation", timeout: Duration = .seconds(5),
                        operation: @escaping @MainActor @Sendable () async throws -> Void) async throws {
    let reply = AsyncStream<Result<Void, any Error>>.makeStream()
    // An unstructured worker lets the caller finish without joining a stuck
    // Foundation callback. Cancellation still retires the worker and socket.
    let worker = Task {
        do {
            try Task.checkCancellation()
            try await operation()
            reply.continuation.yield(.success(()))
        } catch { reply.continuation.yield(.failure(error)) }
        reply.continuation.finish()
    }
    let deadline = Task {
        do { try await Task.sleep(for: timeout) } catch { return }
        reply.continuation.yield(.failure(MiningSocketDeadlineExceeded(stage: stage)))
        reply.continuation.finish()
        worker.cancel()
        socket.cancel(with: .goingAway, reason: nil)
    }
    defer { worker.cancel(); deadline.cancel(); reply.continuation.finish() }
    try await withTaskCancellationHandler {
        try Task.checkCancellation()
        for await result in reply.stream {
            try Task.checkCancellation()
            try result.get()
            return
        }
        throw CancellationError()
    } onCancel: {
        reply.continuation.finish()
        socket.cancel(with: .goingAway, reason: nil)
    }
}

private actor MiningPortReplyGate {
    private var entered = false
    private var released = false

    // No stored continuations: both sides terminate on cancellation and have
    // their own deadline, including cleanup after an earlier assertion fails.
    func hold() async {
        entered = true
        let deadline = ContinuousClock.now + .seconds(10)
        while !released, ContinuousClock.now < deadline {
            do { try await Task.sleep(for: .milliseconds(10)) }
            catch { return }
        }
        if !released { Issue.record("Held mining reply exceeded its deadline") }
    }

    func waitUntilEntered() async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while !entered {
            guard ContinuousClock.now < deadline else { throw MiningFailure("Reply gate was not entered.") }
            try await Task.sleep(for: .milliseconds(10))
        }
        try Task.checkCancellation()
    }

    func release() { released = true }
}

/// Synthetic loopback sockets reserve OS-assigned ports without a listener
/// that could be mistaken for a working mining endpoint.
@MainActor private final class MiningPortReservation {
    let port: UInt16
    private var descriptor: Int32

    init(port: UInt16 = 0) throws {
        let descriptor = Darwin.socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw POSIXError(.ENFILE) }
        var address = miningLoopbackAddress(port)
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard result == 0, Darwin.listen(descriptor, 8) == 0 else {
            let error = POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
            Darwin.close(descriptor)
            throw error
        }
        var size = socklen_t(MemoryLayout<sockaddr_in>.size)
        let named = withUnsafeMutablePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(descriptor, $0, &size) }
        }
        guard named == 0, address.sin_port != 0 else {
            Darwin.close(descriptor)
            throw POSIXError(.EIO)
        }
        self.port = UInt16(bigEndian: address.sin_port)
        self.descriptor = descriptor
    }

    func close() {
        if descriptor >= 0 { Darwin.close(descriptor); descriptor = -1 }
    }
}

private func miningLoopbackAddress(_ port: UInt16) -> sockaddr_in {
    var address = sockaddr_in()
    address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    address.sin_family = sa_family_t(AF_INET)
    address.sin_port = port.bigEndian
    address.sin_addr = in_addr(s_addr: inet_addr("127.0.0.1"))
    return address
}

func expectMiningPortClosed(_ port: UInt16) throws {
    let descriptor = Darwin.socket(AF_INET, SOCK_STREAM, 0)
    try #require(descriptor >= 0)
    defer { Darwin.close(descriptor) }
    var address = miningLoopbackAddress(port)
    let connected = withUnsafePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
        }
    }
    #expect(connected == -1 && errno == ECONNREFUSED)
}
#endif
