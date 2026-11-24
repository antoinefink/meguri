#if MEGURI_TEST_HOOKS && os(macOS)
@testable import AppModel
import AppKit
import Darwin
import DesignSystem
import Foundation
import Settings
import SwiftUI
import Testing

/// VM-runnable production pane captures and an isolated native toggle test.
/// The toggle uses synthetic defaults, no collection and an ephemeral port. `tools/mg snapshots --platform mac --record
/// 'settings.integrations/*'` retains the actual images for review.
@MainActor @Suite(.serialized) struct SentenceMiningPaneTests {
    @Test(arguments: [false, true]) func actualPane(portConflict: Bool) async {
        let state = portConflict ? "port-conflict" : "listening"
        let model = SentenceMining.renderFixture(portConflict: portConflict)
        let cell = MatrixCell(device: .macDefault)
        await assertSnapshot(subject: "settings.integrations", state: state, cell: cell, layout: .screen) {
            NavigationStack { SentenceMiningPane(model: model) }
        }
    }

    @Test(.timeLimit(.minutes(1))) func nativeToggleControlsTheProductionReceiver() async throws {
        let suite = "SentenceMiningPaneTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = SentenceMining(defaults: defaults, collectionKey: "synthetic-toggle", ports: (0, 0))
        model.firstOpenAttemptEnded()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 600),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentViewController = NSHostingController(rootView: NavigationStack { SentenceMiningPane(model: model) })
        defer { window.contentViewController = nil; window.close() }
        window.orderFront(nil)
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForResource = 2
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            await model.settle()
            #expect(!model.enabled)
            // All four combinations, driven through actual native checkboxes.
            for id in [SentenceMining.EndpointID.migaku, .ankiConnect, .migaku, .ankiConnect] {
                try await pressMiningToggle(in: window, id: id)
                await model.settle()
                for endpointID in SentenceMining.EndpointID.allCases {
                    let endpoint = model.endpoint(endpointID)
                    if endpoint.enabled {
                        try await waitUntilListening(endpoint)
                        let url = URL(string: "http://127.0.0.1:\(endpoint.port)/srs-check")!
                        let (data, response) = try await session.data(from: url)
                        #expect((response as? HTTPURLResponse)?.statusCode == 200)
                        #expect((try JSONSerialization.jsonObject(with: data) as? [String: Any])?["version"] as? String == "4")
                    } else { #expect(endpoint.status.hasPrefix("Off")) }
                }
                let restarted = SentenceMining(defaults: defaults, collectionKey: "synthetic-other")
                #expect(restarted.migaku.enabled == model.migaku.enabled)
                #expect(restarted.ankiConnect.enabled == model.ankiConnect.enabled)
            }
            #expect(!model.enabled)
            await model.shutdown()
        } catch {
            await model.shutdown()
            throw error
        }
    }

    /// Select alone through mac-hosted-test in the VM. Both exact ports and
    /// families must first be ours; no request is sent to an occupied endpoint.
    @Test(.timeLimit(.minutes(1))) func exactFixedPortsUseTheProductionReceiverAndRelease() async throws {
        let reservations = try [FixedMiningPort(44432), FixedMiningPort(8765)]
        defer { reservations.forEach { $0.close() } }
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic-fixed-ports")
        model.firstOpenAttemptEnded()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 1
        configuration.timeoutIntervalForResource = 2
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            reservations.forEach { $0.close() }
            model.setEnabled(true, for: .migaku)
            model.setEnabled(true, for: .ankiConnect)
            await model.settle()
            try await waitUntilListening(model.migaku)
            try await waitUntilListening(model.ankiConnect)
            #expect(model.migaku.port == 44432 && model.ankiConnect.port == 8765)
            for port in [UInt16(44432), 8765] {
                let (data, response) = try await session.data(from: URL(string: "http://127.0.0.1:\(port)/srs-check")!)
                #expect((response as? HTTPURLResponse)?.statusCode == 200)
                let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
                #expect(object["version"] as? String == "4" && object["ok"] as? Bool == false)
                let (hello, _) = try await session.data(from: URL(string: "http://127.0.0.1:\(port)/anki-hello")!)
                #expect((try JSONSerialization.jsonObject(with: hello) as? [String: Any])?["port"] as? Int == Int(port))
                try await checkHandshake(session, port: port)
                try await checkVersion(session, port: port)
                do {
                    let duplicate = try MiningSocket.listen(port: port, ipv6: false)
                    Darwin.close(duplicate.fd)
                    Issue.record("A second listener shared the active mining endpoint")
                } catch let error as POSIXError { #expect(error.code == .EADDRINUSE) }
            }
            model.setEnabled(false, for: .migaku)
            await model.settle()
            #expect(!model.migaku.enabled && model.ankiConnect.status.hasPrefix("Listening"))
            let releasedMigaku = try FixedMiningPort(44432)
            releasedMigaku.close()
            try await checkVersion(session, port: 8765)
            await model.shutdown()
            #expect(model.migaku.status.hasPrefix("Off") && model.ankiConnect.status.hasPrefix("Off"))
            for port in [UInt16(44432), 8765] {
                let released = try FixedMiningPort(port)
                released.close()
            }
        } catch {
            await model.shutdown()
            throw error
        }
    }

    private func waitUntilListening(_ endpoint: SentenceMining.Endpoint) async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while !endpoint.status.hasPrefix("Listening"), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        try #require(endpoint.status.hasPrefix("Listening") && endpoint.port != 0)
    }

    private func checkHandshake(_ session: URLSession, port: UInt16) async throws {
        var request = URLRequest(url: URL(string: "ws://127.0.0.1:\(port)/anki-connect")!)
        request.setValue("chrome-extension://abcdefghijklmnopabcdefghijklmnop", forHTTPHeaderField: "Origin")
        let socket = session.webSocketTask(with: request)
        let reply = AsyncThrowingStream<Void, any Error>.makeStream()
        socket.resume()
        let worker = Task {
            do {
                try await socket.send(.string(#"{"id":1,"msg":"hello","data":{}}"#))
                socket.sendPing { error in
                    if let error { reply.continuation.finish(throwing: error) }
                    else { reply.continuation.yield(()); reply.continuation.finish() }
                }
            } catch { reply.continuation.finish(throwing: error) }
        }
        let deadline = Task {
            do { try await Task.sleep(for: .seconds(5)) } catch { return }
            reply.continuation.finish(throwing: URLError(.timedOut))
            socket.cancel(with: .goingAway, reason: nil)
        }
        defer {
            worker.cancel(); deadline.cancel(); reply.continuation.finish()
            socket.cancel(with: .goingAway, reason: nil)
        }
        for try await _ in reply.stream { return }
        throw URLError(.cannotConnectToHost)
    }

    private func checkVersion(_ session: URLSession, port: UInt16) async throws {
        var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/")!)
        request.httpMethod = "POST"
        request.httpBody = Data(#"{"action":"version","version":6}"#.utf8)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let (data, response) = try await session.data(for: request)
        #expect((response as? HTTPURLResponse)?.statusCode == 200)
        #expect((try JSONSerialization.jsonObject(with: data) as? [String: Any])?["result"] as? Int == 6)
    }

    /// Press the actual SwiftUI Toggle through its native accessibility action;
    /// no cursor, keyboard, global window lookup or production app launch.
    private func pressMiningToggle(in window: NSWindow, id: SentenceMining.EndpointID) async throws {
        let identifier = id == .migaku ? "settings.integrations.migaku.enabled" : "settings.integrations.ankiConnect.enabled"
        let deadline = ContinuousClock.now + .seconds(5)
        while ContinuousClock.now < deadline {
            window.layoutIfNeeded()
            window.contentView?.layoutSubtreeIfNeeded()
            var pending: [NSObject] = window.contentView.map { [$0] } ?? []
            var visited = Set<ObjectIdentifier>()
            var matches: [NSObject] = []
            while let object = pending.popLast(), visited.count < 512 {
                guard visited.insert(ObjectIdentifier(object)).inserted else { continue }
                if let view = object as? NSView { pending.append(contentsOf: view.subviews) }
                let identifierSelector = #selector(NSAccessibilityProtocol.accessibilityIdentifier)
                let roleSelector = #selector(NSAccessibilityProtocol.accessibilityRole)
                if object.responds(to: identifierSelector), object.responds(to: roleSelector),
                   object.perform(identifierSelector)?.takeUnretainedValue() as? String == identifier,
                   let role = object.perform(roleSelector)?.takeUnretainedValue() as? String,
                   role == "AXCheckBox" {
                    matches.append(object)
                }
                let childrenSelector = #selector(NSAccessibilityProtocol.accessibilityChildren)
                if object.responds(to: childrenSelector),
                   let children = object.perform(childrenSelector)?.takeUnretainedValue() as? [NSObject] {
                    pending.append(contentsOf: children)
                }
            }
            if matches.count == 1, let toggle = matches.first {
                if let control = toggle as? any NSAccessibilityProtocol {
                    try #require(control.accessibilityPerformPress())
                    return
                }
                if let control = toggle as? any NSAccessibilityButton {
                    try #require(control.accessibilityPerformPress())
                    return
                }
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        Issue.record("The production mining Toggle was not exposed as one native control")
        throw CocoaError(.featureUnsupported)
    }
}
/// Each reservation owns only synthetic loopback listening FDs. It never
/// accepts, connects to or sends bytes to an unrelated service.
@MainActor private final class FixedMiningPort {
    let port: UInt16
    private var descriptors: [Int32] = []

    init(_ port: UInt16) throws {
        self.port = port
        let ipv4 = try MiningSocket.listen(port: port, ipv6: false)
        do {
            let ipv6 = try MiningSocket.listen(port: port, ipv6: true)
            descriptors = [ipv4.fd, ipv6.fd]
        } catch {
            Darwin.close(ipv4.fd)
            throw error
        }
    }

    func close() {
        descriptors.forEach { Darwin.close($0) }
        descriptors.removeAll()
    }

    deinit { descriptors.forEach { Darwin.close($0) } }
}
#endif
