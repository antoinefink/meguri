import CardRendering
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
import ApplicationServices
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
        nonisolated struct PublicAXLink: @unchecked Sendable {
            let element: AXUIElement
            let rawIdentifier: String?
            let rawDOMIdentifier: String?
            let domError: AXError
            let domWrongType: Bool
            let label: String?
            let title: String?
        }
        nonisolated struct PublicAXObservation: @unchecked Sendable {
            var links: [PublicAXLink] = []
            var nodes: [String] = []
        }

        /// AX calls run off the main actor so this process can answer them.
        /// Read only the fixture's own process and exact window identifier.
        nonisolated static func publicAXFacts(windowIdentifier: String,
                                              deadline: ContinuousClock.Instant) throws -> PublicAXObservation {
            var observation = PublicAXObservation()
            let application = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
            func read(_ node: AXUIElement, _ attribute: String) throws -> (CFTypeRef?, AXError) {
                let remaining = ContinuousClock.now.duration(to: deadline).components
                let seconds = Double(remaining.seconds) + Double(remaining.attoseconds) / 1e18
                guard seconds > 0 else { throw HarnessFailure("Own-window native AX deadline") }
                guard AXUIElementSetMessagingTimeout(node, Float(min(seconds, 0.25))) == .success else {
                    throw HarnessFailure("Own-window native AX timeout unavailable")
                }
                var value: CFTypeRef?
                let error = AXUIElementCopyAttributeValue(node, attribute as CFString, &value)
                return (value, error)
            }
            func string(_ node: AXUIElement, _ attribute: String) throws -> String? {
                let (value, error) = try read(node, attribute)
                guard error == .success, let value, CFGetTypeID(value) == CFStringGetTypeID() else { return nil }
                return value as? String
            }
            func children(_ node: AXUIElement, _ attribute: String = kAXChildrenAttribute) throws -> [AXUIElement] {
                let (value, error) = try read(node, attribute)
                if error == .attributeUnsupported || error == .noValue { return [] }
                guard error == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID(),
                      let result = value as? [AXUIElement], result.count <= 256 else {
                    throw HarnessFailure("Own-window native AX children unavailable/wrong-type/truncated")
                }
                return result
            }
            let windows = try children(application, kAXWindowsAttribute)
            var owned: [AXUIElement] = []
            for window in windows where try string(window, kAXIdentifierAttribute) == windowIdentifier { owned.append(window) }
            guard owned.count == 1, let window = owned.first else { throw HarnessFailure("Exact fixture AX window missing/duplicate") }
            var seen: [AXUIElement] = []
            func visit(_ node: AXUIElement, depth: Int) throws {
                guard depth <= 32, seen.count < 256, !seen.contains(where: { CFEqual($0, node) }) else {
                    throw HarnessFailure("Own-window native AX tree truncated/cyclic")
                }
                seen.append(node)
                let role = try string(node, kAXRoleAttribute)
                let identifier = try string(node, kAXIdentifierAttribute)
                // Public AXWebConstants.h: kAXDOMIdentifierAttribute is
                // CFSTR("AXDOMIdentifier"); use its exact public attribute name.
                let (dom, error) = try read(node, "AXDOMIdentifier")
                let isString = dom.map { CFGetTypeID($0) == CFStringGetTypeID() } ?? false
                let domID = error == .success && isString ? dom as? String : nil
                observation.nodes.append("role=\(role ?? "nil") AXIdentifier=\(identifier.debugDescription) AXDOMIdentifier=\(domID.debugDescription) DOMError=\(error.rawValue) DOMWrongType=\(error == .success && !isString)")
                // Public AXWebConstants.h: kAXLinkRole is CFSTR("AXLink");
                // that C macro is not imported by ApplicationServices in Swift.
                if role == "AXLink" {
                    observation.links.append(PublicAXLink(element: node, rawIdentifier: identifier,
                        rawDOMIdentifier: domID, domError: error, domWrongType: error == .success && !isString,
                        label: try string(node, kAXDescriptionAttribute), title: try string(node, kAXTitleAttribute)))
                }
                for child in try children(node) { try visit(child, depth: depth + 1) }
            }
            try visit(window, depth: 0)
            return observation
        }

        @Test func native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            let page = try await harness.openPage()
            let window = try #require(page.webView.window)
            let windowID = "testhooks.replay.window.\(UUID().uuidString)"
            window.setAccessibilityIdentifier(windowID)
            _ = page.webView.accessibilityChildren()
            let front = ["play:q:00", "play:q:1", "play:q:00"]
            let back = front + front + ["play:a:000"]
            try await page.present()
            var commands: [String] = []
            for (stage, side) in ["q", "a", "q", "a"].enumerated() {
                let current = side == "q" ? front : back
                let ids = current.indices.map { "review.card.replay.\(side)\(side == "q" ? $0 + 1 : $0)" }
                let authored = side == "q" ? "<a class='replay-button soundLink' id='review.card.replay.q0' data-meguri-replay='true' aria-label='Author &amp; 猫' href='#own' style='color:purple'>own</a>" : ""
                let html = authored + zip(ids, current).map { Self.control($0.0, command: $0.1, platform: .mac) }.joined()
                let expectedIDs = ids + (side == "q" ? ["review.card.replay.q0"] : [])
                let shown = harness.signals("shown", from: page).count
                let deadline = ContinuousClock.now + .seconds(10)
                let card = Int64(stage / 2 + 1), generation = Int64(stage + 1)
                try await page.announce(card: card, generation: generation)
                if side == "q" {
                    try await page.run("_showQuestion(html, '', 'card card1', ids);", ["html": html, "ids": ids])
                } else {
                    try await page.run("_showAnswer(html, 'card card1', ids);", ["html": html, "ids": ids])
                }
                _ = try await harness.signal("shown", count: shown + 1, from: page,
                    timeout: max(.zero, ContinuousClock.now.duration(to: deadline)))
                func observe(until observationDeadline: ContinuousClock.Instant) async throws -> PublicAXObservation {
                    try await Task.detached { try Self.publicAXFacts(windowIdentifier: windowID, deadline: observationDeadline) }.value
                }
                var tree = try await observe(until: deadline)
                // Publication waits on counts only; identity/name failures never retry.
                while tree.links.count != expectedIDs.count && ContinuousClock.now < deadline {
                    try await Task.sleep(for: .milliseconds(5))
                    tree = try await observe(until: deadline)
                }
                defer { for node in tree.nodes { print("Replay own-window native AX: \(node)") } }
                #expect(tree.links.count == expectedIDs.count)
                #expect(Set(tree.links.compactMap(\.rawDOMIdentifier)) == Set(expectedIDs))
                if side == "q" {
                    let own = tree.links.filter { $0.rawDOMIdentifier == "review.card.replay.q0" }
                    #expect(own.count == 1)
                    #expect(own.first?.label == "Author & 猫" || own.first?.title == "Author & 猫")
                }
                for (id, command) in zip(ids, current) {
                    let matches = tree.links.filter { $0.rawDOMIdentifier == id && $0.domError == .success && !$0.domWrongType }
                    let link = try #require(matches.count == 1 ? matches.first : nil, "Exact unique trusted native AXDOMIdentifier required")
                    #expect(link.label == CardRuntime.replayLabel || link.title == CardRuntime.replayLabel)
                    // Revalidate live page, native window, root, current binding,
                    // and exact same AX element before its single native press.
                    let fresh = try await observe(until: .now + .seconds(5))
                    let matching = fresh.links.filter { $0.rawDOMIdentifier == id && $0.domError == .success && !$0.domWrongType }
                    try #require(matching.count == 1 && CFEqual(matching[0].element, link.element))
                    try #require(page.webView.window === window && page.webView.url == WebTestHarness.mediaRoot)
                    let binding = try #require(harness.signals("shown", from: page).last)
                    try #require(binding.card == card && binding.generation == generation && binding.side == (side == "q" ? "question" : "answer"))
                    let count = harness.signals("command", from: page).count
                    let error = await Task.detached { AXUIElementPerformAction(link.element, kAXPressAction as CFString) }.value
                    try #require(error == .success, "Same original native AX link press failed")
                    let message = try await harness.signal("command", count: count + 1, from: page)
                    commands.append(command)
                    #expect(message["command"] as? String == command)
                    #expect(harness.signals("command", from: page).count == count + 1)
                }
            }
            #expect(commands.count == 20)
            #expect(harness.signals("command", from: page).map { $0["command"] as? String } == (front + back + front + back).map(Optional.some))
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
