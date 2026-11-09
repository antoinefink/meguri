import CardRendering
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

extension WebKitSuites.CardRuntimeSuites {
    @MainActor @Suite struct ReplayAccessibilityTests {
        // The D4 literal with explicit synthetic transform metadata, as supplied
        // by CardPayload in production. Rust tests separately pin its emission.
        static func control(_ id: String, command: String, platform: CardRuntime.Platform) -> String {
            let action = platform == .mac ? "pycmd('\(command)')" : "window.webkit.messageHandlers.meguri.postMessage('\(command)')"
            return """
                <a class="replay-button soundLink" id="\(id)" data-meguri-replay="true" aria-label="Play audio" href=# onclick="\(action); return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>
                """
        }

        @Test(arguments: CardRuntime.Platform.allCases)
        func native_generated_replay_controls_use_catalog_localization(platform: CardRuntime.Platform) async throws {
            let unsafeHTMLCopy = "Lire \"猫\" & l'audio <img id='injected' src=x onerror='window.injected=true'> 日本語"
            for label in [CardRuntime.replayLabel, unsafeHTMLCopy] {
                let harness = WebTestHarness(platform: platform, replayLabel: label)
                defer { harness.close() }
                let page = try await harness.openPage()
                try await page.installLog()
                // An exact marker/class/prefix lookalike, a prefix alone and
                // a class alone are all authored. None is in the metadata.
                let authored = """
                    <a class="replay-button soundLink" id="review.card.replay.q0" data-meguri-replay="true" aria-label="Author &amp; 猫" href="#own" style="color:purple">own</a>
                    <a id="review.card.replay.q20" aria-label="Prefix" href="#prefix">prefix</a>
                    <a class="replay-button soundLink" data-meguri-replay="true" aria-label="Marker" href="#marker">marker</a>
                    """
                let generatedID = "review.card.replay.q1" // q0 was reserved by the authored control.
                let html = authored + Self.control(generatedID, command: "play:q:00", platform: platform) + """
                    <script>
                    log(document.getElementById('review.card.replay.q1').getAttribute('aria-label'));
                    onUpdateHook.push(function () { log(document.getElementById('review.card.replay.q1').getAttribute('aria-label')); });
                    onShownHook.push(function () { log(document.getElementById('review.card.replay.q1').getAttribute('aria-label')); });
                    </script>
                    """
                try await page.present()
                try await page.run("_showQuestion(html, '', 'card card1', ids);", ["html": html, "ids": [generatedID]])
                try await harness.signal("shown", from: page)
                // Inline scripts and both hooks saw the final name, before
                // the shown/readiness receipt. No late DOM repair is involved.
                #expect(try await page.log() == [label, label, label])
                let labels = try await page.json([String].self, "return JSON.stringify(Array.from(document.querySelectorAll('#qa a'), a => a.getAttribute('aria-label')));")
                #expect(labels == ["Author & 猫", "Prefix", "Marker", label])
                #expect(try await page.bool("document.querySelector('#qa a').getAttribute('style') === 'color:purple'"))
                #expect(try await page.bool("document.getElementById('injected') === null && window.injected === undefined"))
                #expect(try await page.bool("document.getElementById('review.card.replay.q1').attributes.length === 7"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        #if os(macOS)
        /// Actual AppKit/WebKit accessibility objects: never infer an AX
        /// identifier from a DOM id, AXDOMIdentifier or an aria-label.
        static func hierarchy(_ root: any NSAccessibilityProtocol) -> [any NSAccessibilityProtocol] {
            var pending: [any NSAccessibilityProtocol] = [root]
            var seen = Set<ObjectIdentifier>()
            var result: [any NSAccessibilityProtocol] = []
            while let element = pending.popLast() {
                guard seen.insert(ObjectIdentifier(element as AnyObject)).inserted else { continue }
                result.append(element)
                for child in element.accessibilityChildren() ?? [] {
                    if let accessible = child as? any NSAccessibilityProtocol { pending.append(accessible) }
                }
            }
            return result
        }

        @Test func native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            let page = try await harness.openPage()
            // Request the real native tree before showing the first side.
            _ = page.webView.accessibilityChildren()
            let frontCommands = ["play:q:00", "play:q:1", "play:q:00"]
            let answerCommands = frontCommands + frontCommands + ["play:a:000"]
            try await page.present()
            for (side, commands) in [("q", frontCommands), ("a", answerCommands), ("q", frontCommands), ("a", answerCommands)] {
                let ids = commands.indices.map { "review.card.replay.\(side)\($0)" }
                let html = zip(ids, commands).map { Self.control($0.0, command: $0.1, platform: .mac) }.joined()
                let shown = harness.signals("shown", from: page).count
                if side == "q" {
                    try await page.run("_showQuestion(html, '', 'card card1', ids);", ["html": html, "ids": ids])
                } else {
                    try await page.run("_showAnswer(html, 'card card1', ids);", ["html": html, "ids": ids])
                }
                try await harness.signal("shown", count: shown + 1, from: page)
                let tree = Self.hierarchy(page.webView)
                let links = tree.filter { $0.accessibilityRole() == .link }
                let observed = links.map { "id=\($0.accessibilityIdentifier() ?? "nil"), label=\($0.accessibilityLabel() ?? "nil"), title=\($0.accessibilityTitle() ?? "nil")" }
                // A WebKit implementation exposing only AXDOMIdentifier must
                // fail here, with its real observed identifiers in the report.
                #expect(links.count == ids.count, "Real WebKit AX links: \(observed)")
                #expect(Set(links.compactMap { $0.accessibilityIdentifier() }) == Set(ids), "HTML IDs must reach AXIdentifier: \(observed)")
                for (id, command) in zip(ids, commands) {
                    let matching = links.filter { $0.accessibilityIdentifier() == id }
                    let link = try #require(matching.count == 1 ? matching.first : nil, "Missing/ambiguous native AX identifier \(id): \(observed)")
                    // WebKit can represent a link's accessible name as its
                    // label or title. Both are native AX properties.
                    #expect(link.accessibilityLabel() == CardRuntime.replayLabel || link.accessibilityTitle() == CardRuntime.replayLabel)
                    let count = harness.signals("command", from: page).count
                    try #require(link.accessibilityPerformPress(), "Native AX press failed for \(id)")
                    let messages = try await harness.signal("command", count: count + 1, from: page)
                    #expect(messages["command"] as? String == command)
                    #expect(harness.signals("command", from: page).count == count + 1)
                }
            }
            #expect(harness.diagnostics.isEmpty)
        }
        #else
        static func hierarchy(_ root: NSObject) -> [NSObject] {
            var pending = [root]
            var seen = Set<ObjectIdentifier>()
            var result: [NSObject] = []
            while let element = pending.popLast() {
                guard seen.insert(ObjectIdentifier(element)).inserted else { continue }
                result.append(element)
                if let children = element.accessibilityElements {
                    pending += children.compactMap { $0 as? NSObject }
                } else {
                    let count = element.accessibilityElementCount()
                    if count != NSNotFound && count > 0 {
                        for index in 0..<count {
                            if let child = element.accessibilityElement(at: index) as? NSObject { pending.append(child) }
                        }
                    }
                }
            }
            return result
        }

        @Test func native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy() async throws {
            let platform: CardRuntime.Platform = UIDevice.current.userInterfaceIdiom == .pad ? .iPad : .iPhone
            let harness = WebTestHarness(platform: platform)
            harness.configuration.userContentController.add(harness.signals, contentWorld: .page, name: "meguri")
            defer { harness.close() }
            let page = try await harness.openPage()
            _ = page.webView.accessibilityElements
            let frontCommands = ["play:q:00", "play:q:1", "play:q:00"]
            let answerCommands = frontCommands + frontCommands + ["play:a:000"]
            try await page.present()
            for (side, commands) in [("q", frontCommands), ("a", answerCommands), ("q", frontCommands), ("a", answerCommands)] {
                let ids = commands.indices.map { "review.card.replay.\(side)\($0)" }
                let html = zip(ids, commands).map { Self.control($0.0, command: $0.1, platform: platform) }.joined()
                let shown = harness.signals("shown", from: page).count
                if side == "q" {
                    try await page.run("_showQuestion(html, '', 'card card1', ids);", ["html": html, "ids": ids])
                } else {
                    try await page.run("_showAnswer(html, 'card card1', ids);", ["html": html, "ids": ids])
                }
                try await harness.signal("shown", count: shown + 1, from: page)
                let links = Self.hierarchy(page.webView).filter { $0.accessibilityTraits.contains(.link) }
                func identifier(_ element: NSObject) -> String? { (element as? any UIAccessibilityIdentification)?.accessibilityIdentifier }
                let observed = links.map { "id=\(identifier($0) ?? "nil"), label=\($0.accessibilityLabel ?? "nil")" }
                #expect(links.count == ids.count, "Real WebKit AX links: \(observed)")
                #expect(Set(links.compactMap(identifier)) == Set(ids), "HTML IDs must reach native accessibilityIdentifier: \(observed)")
                for (id, command) in zip(ids, commands) {
                    let matching = links.filter { identifier($0) == id }
                    let link = try #require(matching.count == 1 ? matching.first : nil, "Missing/ambiguous native AX identifier \(id): \(observed)")
                    #expect(link.accessibilityLabel == CardRuntime.replayLabel)
                    let count = harness.signals.messages(to: "meguri").count
                    try #require(link.accessibilityActivate(), "Native AX activation failed for \(id)")
                    try await harness.until("native replay command") { harness.signals.messages(to: "meguri").count > count }
                    #expect(harness.signals.messages(to: "meguri").count == count + 1)
                    #expect(harness.signals.messages(to: "meguri").last?["value"] as? String == command)
                }
            }
            #expect(harness.diagnostics.isEmpty)
        }
        #endif
    }
}
