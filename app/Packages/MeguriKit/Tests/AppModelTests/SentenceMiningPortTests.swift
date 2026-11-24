#if os(macOS)
@testable import AppModel
import Darwin
import Foundation
import Observation
import Testing

@MainActor @Suite struct SentenceMiningPortTests {
    @Test func devicePreferencePersistsAndRejectsInvalidEntries() throws {
        let suite = "SentenceMiningPortTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = SentenceMining(defaults: defaults, collectionKey: "synthetic-first")
        #expect(model.configuredPort == 44432 && model.port == 44432 && !model.enabled)
        for port in [1024, 8765, 65535, 44432] {
            #expect(model.applyPort(String(port)))
            #expect(defaults.integer(forKey: "mining.port") == port)
            // The preference belongs to the device, not a collection.
            let reopened = SentenceMining(defaults: defaults, collectionKey: "synthetic-other")
            #expect(reopened.configuredPort == UInt16(port) && reopened.port == UInt16(port))
            #expect(!reopened.enabled && reopened.status == "Off · Port \(port)")
        }
        for entry in ["", "4", "1023", "0", "65536", "999999999999999999999", "44x", "44432.0", "-1234", "+1234", " 44432", "４４４３２"] {
            #expect(!model.applyPort(entry))
            #expect(model.configuredPort == 44432 && model.port == 44432 && model.status == "Off · Port 44432")
            #expect(defaults.integer(forKey: "mining.port") == 44432)
        }
        let invalidValues: [Any] = [0, 1023, 65536, "partial", 44432.5, true]
        for invalid in invalidValues {
            defaults.set(invalid, forKey: "mining.port")
            #expect(SentenceMining(defaults: defaults, collectionKey: "synthetic").configuredPort == 44432)
        }
        let ephemeral = SentenceMining(defaults: defaults, collectionKey: "synthetic", port: 0)
        #expect(ephemeral.configuredPort == 0) // Explicit test seam only.
        #expect(!ephemeral.applyPort("0"))
    }

    @Test(.timeLimit(.minutes(1))) func productionReducerRebindsOnlyOnCommittedChanges() async throws {
        let suite = "SentenceMiningPortTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        // Keep each endpoint reserved until its turn. No fixed socket ports,
        // sleeps or competition with another test's ephemeral listener.
        let first = try MiningPortReservation()
        let occupied = try MiningPortReservation()
        let next = try MiningPortReservation()
        let disabled = try MiningPortReservation()
        defer { first.close(); occupied.close(); next.close(); disabled.close() }
        let model = SentenceMining(defaults: defaults, collectionKey: "synthetic-wire")
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            model.firstOpenAttemptEnded()
            #expect(model.applyPort(String(first.port)))
            await model.settle()
            #expect(!model.enabled && model.status == "Off · Port \(first.port)")
            first.close()
            // The disabled preference change left this port free to bind.
            let stillFree = try MiningPortReservation(port: first.port)
            stillFree.close()
            model.setEnabled(true)
            await model.settle()
            await waitForStatus(model) { $0.hasPrefix("Listening") }
            #expect(model.port == first.port && model.status.contains(String(first.port)))
            let socket = session.webSocketTask(with: URL(string: "ws://127.0.0.1:\(first.port)/anki-connect")!)
            socket.resume()
            defer { socket.cancel(with: .goingAway, reason: nil) }
            try await socket.sendPing()
            let initialStatus = model.status
            #expect(!model.applyPort(""))
            #expect(!model.applyPort("65536"))
            #expect(model.applyPort(String(first.port)))
            await model.settle()
            #expect(model.status == initialStatus)
            try await socket.sendPing() // A no-op must preserve the listener.

            // A real bind conflict retains the selected preference and retires
            // the previous listener, including its WebSocket and Host policy.
            #expect(model.applyPort(String(occupied.port)))
            await model.settle()
            await waitForStatus(model) { $0.contains("is in use") }
            #expect(model.port == occupied.port && model.status.contains(String(occupied.port)))
            #expect(defaults.integer(forKey: "mining.port") == Int(occupied.port))
            await #expect(throws: (any Error).self) { _ = try await socket.receive() }
            try expectMiningPortClosed(first.port)
            #expect(!model.applyPort("12"))
            #expect(model.applyPort(String(occupied.port)))
            #expect(model.status.contains("Port \(occupied.port) is in use"))

            next.close()
            #expect(model.applyPort(String(next.port)))
            await model.settle()
            await waitForStatus(model) { $0.hasPrefix("Listening") }
            #expect(model.port == next.port && model.status.contains(String(next.port)))
            let hello = try await exchange(session, port: next.port, path: "/anki-hello")
            #expect(hello.status == 200 && hello.json?["port"] as? Int == Int(next.port))
            let rejectedHost = try await exchange(session, port: next.port, path: "/anki-hello", hostPort: first.port)
            #expect(rejectedHost.status == 403)
            let version = try await exchange(session, port: next.port, path: "/", body: Data(#"{"action":"version","version":6}"#.utf8))
            #expect(version.status == 200 && version.json?["result"] as? Int == 6)
            try expectMiningPortClosed(first.port)
            let restored = SentenceMining(defaults: defaults, collectionKey: "synthetic-new-collection")
            #expect(restored.enabled && restored.configuredPort == next.port)

            model.setEnabled(false)
            #expect(model.applyPort(String(disabled.port)))
            await model.settle()
            #expect(!model.enabled && model.status == "Off · Port \(disabled.port)" && model.port == disabled.port)
            try expectMiningPortClosed(first.port)
            try expectMiningPortClosed(next.port)
            disabled.close()
            let offPort = try MiningPortReservation(port: disabled.port)
            offPort.close()
            await model.shutdown()
        } catch {
            await model.shutdown()
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1))) func retiringEndpointKeepsItsAdmittedReplyAndCannotRestart() async throws {
        let gate = MiningPortReplyGate()
        let admission = MiningAdmission()
        _ = admission.update(enabled: true)
        let oldEvents = AsyncStream<MiningListener.State>.makeStream()
        let newEvents = AsyncStream<MiningListener.State>.makeStream()
        let old = MiningListener(port: 0, admission: admission, handler: { _, _ in
            await gate.hold()
            return .json(200, ["finished": true])
        }, changed: { oldEvents.continuation.yield($0) })
        let replacement = MiningListener(port: 0, admission: admission, handler: { _, _ in
            .json(200, ["replacement": true])
        }, changed: { newEvents.continuation.yield($0) })
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        let session = URLSession(configuration: configuration)
        defer {
            session.invalidateAndCancel()
            oldEvents.continuation.finish()
            newEvents.continuation.finish()
        }
        old.start()
        do {
            let oldPort = try await listeningPort(oldEvents.stream)
            let url = URL(string: "http://127.0.0.1:\(oldPort)/anki-hello")!
            let pending = Task { try await session.data(from: url) }
            await gate.waitUntilEntered()
            _ = admission.update(enabled: true)
            old.requestStop()
            await old.stop()
            replacement.start()
            let newPort = try await listeningPort(newEvents.stream)
            #expect(newPort != oldPort)
            try expectMiningPortClosed(oldPort)
            // Join a late start callback; a retired listener remains closed.
            old.start()
            await old.stop()
            try expectMiningPortClosed(oldPort)
            await gate.release()
            let (data, response) = try await pending.value
            #expect((response as? HTTPURLResponse)?.statusCode == 200)
            #expect((try JSONSerialization.jsonObject(with: data) as? [String: Bool])?["finished"] == true)
            let active = try await exchange(session, port: newPort, path: "/anki-hello")
            #expect(active.status == 200 && active.json?["replacement"] as? Bool == true)
            await replacement.stop()
        } catch {
            await gate.release()
            await old.stop()
            await replacement.stop()
            throw error
        }
    }

    private func listeningPort(_ events: AsyncStream<MiningListener.State>) async throws -> UInt16 {
        for await state in events {
            if case .listening(let port) = state { return port }
        }
        throw CancellationError()
    }

    /// Observation wakes on the real reducer's status publication. Register
    /// and inspect on MainActor so a state change cannot fall between them.
    private func waitForStatus(_ model: SentenceMining, matching predicate: (String) -> Bool) async {
        while !predicate(model.status) {
            let changes = AsyncStream<Void>.makeStream()
            withObservationTracking { _ = model.status } onChange: {
                changes.continuation.yield(())
            }
            for await _ in changes.stream { break }
            changes.continuation.finish()
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

private actor MiningPortReplyGate {
    private var entered = false
    private var observers: [CheckedContinuation<Void, Never>] = []
    private var reply: CheckedContinuation<Void, Never>?

    func hold() async {
        entered = true
        observers.forEach { $0.resume() }
        observers.removeAll()
        await withCheckedContinuation { reply = $0 }
    }

    func waitUntilEntered() async {
        if !entered { await withCheckedContinuation { observers.append($0) } }
    }

    func release() {
        reply?.resume()
        reply = nil
    }
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
        guard named == 0 else {
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

private func expectMiningPortClosed(_ port: UInt16) throws {
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
