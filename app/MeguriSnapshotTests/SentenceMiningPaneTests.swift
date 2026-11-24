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
        defaults.set(false, forKey: "mining.enabled")
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = SentenceMining(defaults: defaults, collectionKey: "synthetic-toggle", port: 0)
        model.firstOpenAttemptEnded()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 600),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentViewController = NSHostingController(rootView: NavigationStack {
            SentenceMiningPane(model: model)
        })
        defer {
            window.contentViewController = nil
            window.close()
        }
        window.orderFront(nil)
        do {
            await model.settle()
            #expect(!model.enabled && model.status == "Off · Port 0")
            try await pressMiningToggle(in: window)
            await model.settle()
            let deadline = ContinuousClock.now + .seconds(5)
            while !model.status.hasPrefix("Listening"), ContinuousClock.now < deadline {
                try await Task.sleep(for: .milliseconds(10))
            }
            try #require(model.status.hasPrefix("Listening") && model.port != 0)
            #expect(defaults.bool(forKey: "mining.enabled"))
            let configuration = URLSessionConfiguration.ephemeral
            configuration.connectionProxyDictionary = [:]
            let session = URLSession(configuration: configuration)
            defer { session.invalidateAndCancel() }
            let url = try #require(URL(string: "http://127.0.0.1:\(model.port)/srs-check"))
            let (data, response) = try await session.data(from: url)
            #expect((response as? HTTPURLResponse)?.statusCode == 200)
            #expect((try JSONSerialization.jsonObject(with: data) as? [String: Any])?["version"] as? String == "4")
            try await pressMiningToggle(in: window)
            await model.settle()
            #expect(!model.enabled && model.status == "Off · Port \(model.port)" && !defaults.bool(forKey: "mining.enabled"))
            await #expect(throws: (any Error).self) { _ = try await session.data(from: url) }
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
        let suite = "SentenceMiningFixedPorts." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defaults.set(false, forKey: "mining.enabled")
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = SentenceMining(defaults: defaults, collectionKey: "synthetic-fixed-ports")
        model.firstOpenAttemptEnded()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 1
        configuration.timeoutIntervalForResource = 2
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            for reservation in reservations {
                let port = reservation.port
                try #require(model.applyPort(String(port)))
                reservation.close()
                model.setEnabled(true)
                await model.settle()
                let deadline = ContinuousClock.now + .seconds(5)
                while !model.status.hasPrefix("Listening"), ContinuousClock.now < deadline {
                    try await Task.sleep(for: .milliseconds(10))
                }
                try #require(model.status.hasPrefix("Listening") && model.port == port)
                let url = URL(string: "http://127.0.0.1:\(port)/srs-check")!
                let (data, response) = try await session.data(from: url)
                #expect((response as? HTTPURLResponse)?.statusCode == 200)
                let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
                #expect(object["version"] as? String == "4" && object["ok"] as? Bool == false)
                var version = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/")!)
                version.httpMethod = "POST"
                version.httpBody = Data(#"{"action":"version","version":6}"#.utf8)
                version.setValue("application/json", forHTTPHeaderField: "Content-Type")
                let (body, reply) = try await session.data(for: version)
                #expect((reply as? HTTPURLResponse)?.statusCode == 200)
                #expect((try JSONSerialization.jsonObject(with: body) as? [String: Any])?["result"] as? Int == 6)
                // A second listener cannot share the active IPv4 endpoint.
                do {
                    let duplicate = try MiningSocket.listen(port: port, ipv6: false)
                    Darwin.close(duplicate.fd)
                    Issue.record("A second listener shared the active mining endpoint")
                } catch let error as POSIXError {
                    #expect(error.code == .EADDRINUSE)
                }
                model.setEnabled(false)
                await model.settle()
                #expect(!model.enabled && model.status == "Off · Port \(port)")
                // Reclaim both listening addresses after stop, including any
                // TIME_WAIT left by this test's real HTTP conversations.
                let released = try FixedMiningPort(port)
                released.close()
            }
            await model.shutdown()
        } catch {
            await model.shutdown()
            throw error
        }
    }

    /// Press the actual SwiftUI Toggle through its native accessibility action;
    /// no cursor, keyboard, global window lookup or production app launch.
    private func pressMiningToggle(in window: NSWindow) async throws {
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
                   object.perform(identifierSelector)?.takeUnretainedValue() as? String == "settings.integrations.enabled",
                   let role = object.perform(roleSelector)?.takeUnretainedValue() as? String,
                   ["AXCheckBox", "AXSwitch"].contains(role) {
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
