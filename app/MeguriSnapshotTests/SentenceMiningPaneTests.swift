#if MEGURI_TEST_HOOKS && os(macOS)
@testable import AppModel
import AppKit
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
#endif
