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
        struct NativeTreeObservation {
            var links: [any NSAccessibilityProtocol] = []
            var nodes: [String] = []
            var skippedOpaqueChildren = 0
            var repeatedNodes = 0
            var nodeTruncations = 0
            var depthTruncations = 0

            var truncated: Bool { nodeTruncations > 0 || depthTruncations > 0 }
            var counts: String {
                "nodes=\(nodes.count) links=\(links.count) skippedOpaqueChildren=\(skippedOpaqueChildren) repeatedNodes=\(repeatedNodes) nodeTruncations=\(nodeTruncations) depthTruncations=\(depthTruncations)"
            }
        }

        static func hierarchy(_ root: any NSAccessibilityProtocol) -> NativeTreeObservation {
            var observation = NativeTreeObservation()
            var seen = Set<ObjectIdentifier>()
            var retained: [AnyObject] = []
            func visit(_ object: AnyObject, depth: Int) {
                guard depth <= 32 else { observation.depthTruncations += 1; return }
                guard !seen.contains(ObjectIdentifier(object)) else { observation.repeatedNodes += 1; return }
                guard observation.nodes.count < 256 else { observation.nodeTruncations += 1; return }
                seen.insert(ObjectIdentifier(object))
                retained.append(object)
                let native = object as? NSObject
                let className = native.map { NSStringFromClass(type(of: $0)) } ?? "non-NSObject"
                guard let element = object as? any NSAccessibilityProtocol else {
                    if depth > 0 { observation.skippedOpaqueChildren += 1 }
                    let count = native.map { String($0.accessibilityArrayAttributeCount(.children)) } ?? "unavailable"
                    observation.nodes.append("depth=\(depth) class=\(className) NSAccessibilityProtocol=false AXRole=unavailable children=unavailable publicArrayChildCount=\(count)")
                    return
                }
                let role = element.accessibilityRole()
                let children = element.accessibilityChildren() ?? []
                observation.nodes.append("depth=\(depth) class=\(className) NSAccessibilityProtocol=true AXRole=\(role?.rawValue ?? "nil") children=\(children.count)")
                if role == .link { observation.links.append(element) }
                for child in children {
                    visit(child as AnyObject, depth: depth + 1)
                    if observation.truncated { break }
                }
            }
            visit(root as AnyObject, depth: 0)
            withExtendedLifetime(retained) {}
            return observation
        }

        struct PublicAXFacts: Sendable {
            var trusted = false
            var windows = 0, roots = 0, nodes = 0, roles = 0, links = 0
            var expectedNativeIDs = 0, localizedLabels = 0
            var repeatedNodes = 0, nodeTruncations = 0, depthTruncations = 0
            var possibleArrayTruncations = 0
            var deadlineReached = false
            var typeMismatches = 0, missingValues = 0
            var errors: [String: Int] = [:]

            var counts: String {
                let codes = errors.sorted { $0.key < $1.key }.map { "\($0.key):\($0.value)" }.joined(separator: ",")
                return "trusted=\(trusted) windows=\(windows) roots=\(roots) nodes=\(nodes) roles=\(roles) links=\(links) expectedNativeIDs=\(expectedNativeIDs) localizedLabels=\(localizedLabels) repeatedNodes=\(repeatedNodes) nodeTruncations=\(nodeTruncations) depthTruncations=\(depthTruncations) possibleArrayTruncations=\(possibleArrayTruncations) deadlineReached=\(deadlineReached) typeMismatches=\(typeMismatches) missingValues=\(missingValues) AXErrors[operation/code:count]=[\(codes)]"
            }
        }

        nonisolated static func publicAXFacts(expectedIDs: [String], localizedLabel: String,
                                              deadline: ContinuousClock.Instant) -> PublicAXFacts {
            var facts = PublicAXFacts()
            facts.trusted = AXIsProcessTrusted()
            let application = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
            func record(_ error: AXError, operation: String) -> Bool {
                guard error != .success else { return true }
                facts.errors["\(operation)/\(error.rawValue)", default: 0] += 1
                return false
            }
            func prepare(_ node: AXUIElement) -> Bool {
                let remaining = ContinuousClock.now.duration(to: deadline).components
                let seconds = Double(remaining.seconds) + Double(remaining.attoseconds) / 1e18 - 0.001
                guard seconds > 0 else { facts.deadlineReached = true; return false }
                let timeout = Float(min(seconds, 0.25)).nextDown
                guard timeout > 0 else { facts.deadlineReached = true; return false }
                return record(AXUIElementSetMessagingTimeout(node, timeout), operation: "timeout")
            }
            func readString(_ node: AXUIElement, _ attribute: String) -> String? {
                guard prepare(node) else { return nil }
                var value: CFTypeRef?
                guard record(AXUIElementCopyAttributeValue(node, attribute as CFString, &value), operation: attribute) else { return nil }
                guard let value else { facts.missingValues += 1; return nil }
                guard CFGetTypeID(value) == CFStringGetTypeID(), let string = value as? String else {
                    facts.typeMismatches += 1; return nil
                }
                return string
            }
            func readChildren(_ node: AXUIElement, _ attribute: String, nodeBudget: Int = 255) -> [AXUIElement] {
                let limit = min(256, nodeBudget)
                guard limit > 0 else { facts.nodeTruncations += 1; return [] }
                guard prepare(node) else { return [] }
                var count: CFIndex = 0
                let countError = AXUIElementGetAttributeValueCount(node, attribute as CFString, &count)
                let countKnown = record(countError, operation: "\(attribute).count")
                guard countKnown || countError == .notImplemented else { return [] }
                if countKnown && count <= 0 { return [] }
                if countKnown && count > limit { facts.nodeTruncations += 1 }
                let requested = countKnown ? min(count, limit) : limit
                guard prepare(node) else { return [] }
                var value: CFArray?
                guard record(AXUIElementCopyAttributeValues(node, attribute as CFString, 0, requested, &value), operation: "\(attribute).values") else { return [] }
                guard let value else { facts.missingValues += 1; return [] }
                let array: CFTypeRef = value
                guard CFGetTypeID(array) == CFArrayGetTypeID(), let values = array as? [AnyObject] else {
                    facts.typeMismatches += 1; return []
                }
                if !countKnown && values.count >= requested { facts.possibleArrayTruncations += 1 }
                var children: [AXUIElement] = []
                for child in values.prefix(requested) {
                    guard CFGetTypeID(child) == AXUIElementGetTypeID() else {
                        facts.typeMismatches += 1; continue
                    }
                    children.append(child as! AXUIElement)
                }
                return children
            }
            let windows = readChildren(application, kAXWindowsAttribute)
            facts.windows = windows.count
            let roots = readChildren(application, kAXChildrenAttribute)
            facts.roots = roots.count
            guard !windows.isEmpty || !roots.isEmpty else { return facts }
            var pending: [(AXUIElement, Int)] = [(application, 0)]
            var seen: [AXUIElement] = []
            func enqueue(_ children: [AXUIElement], depth: Int) {
                guard !children.isEmpty else { return }
                guard depth <= 32 else { facts.depthTruncations += 1; return }
                for child in children {
                    if seen.contains(where: { CFEqual($0, child) }) || pending.contains(where: { CFEqual($0.0, child) }) {
                        facts.repeatedNodes += 1; continue
                    }
                    guard seen.count + pending.count < 256 else { facts.nodeTruncations += 1; break }
                    pending.append((child, depth))
                }
            }
            while !pending.isEmpty && !facts.deadlineReached {
                guard ContinuousClock.now < deadline else { facts.deadlineReached = true; break }
                let (node, depth) = pending.removeFirst()
                seen.append(node)
                facts.nodes += 1
                let role = readString(node, kAXRoleAttribute)
                if role != nil { facts.roles += 1 }
                if role == NSAccessibility.Role.link.rawValue {
                    facts.links += 1
                    if let identifier = readString(node, kAXIdentifierAttribute), expectedIDs.contains(identifier) {
                        facts.expectedNativeIDs += 1
                    }
                    let label = readString(node, kAXDescriptionAttribute)
                    let title = readString(node, kAXTitleAttribute)
                    if label == localizedLabel || title == localizedLabel { facts.localizedLabels += 1 }
                }
                if depth == 0 {
                    enqueue(windows, depth: 1)
                    enqueue(roots, depth: 1)
                } else {
                    enqueue(readChildren(node, kAXChildrenAttribute, nodeBudget: 256 - seen.count - pending.count), depth: depth + 1)
                }
            }
            return facts
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
            var needsPublicAXDiagnostic = true
            for (side, commands) in [("q", frontCommands), ("a", answerCommands), ("q", frontCommands), ("a", answerCommands)] {
                let ids = commands.indices.map { "review.card.replay.\(side)\($0)" }
                let html = zip(ids, commands).map { Self.control($0.0, command: $0.1, platform: .mac) }.joined()
                let shown = harness.signals("shown", from: page).count
                // Shown and native publication share the existing fixture's
                // ten-second budget. Shown/rAF alone establishes no AX readiness.
                let deadline = ContinuousClock.now + .seconds(10)
                if side == "q" {
                    try await page.run("_showQuestion(html, '', 'card card1', ids);", ["html": html, "ids": ids])
                } else {
                    try await page.run("_showAnswer(html, 'card card1', ids);", ["html": html, "ids": ids])
                }
                var tree = Self.hierarchy(page.webView)
                var firstNativeCounts = "unavailable"
                var observations = 0
                defer {
                    print("Replay AX fixture: observations=\(observations) expectedNativeLinks=\(ids.count) initial={\(firstNativeCounts)} final={\(tree.counts)}")
                    print("Opaque inspection limit: NSAccessibilityProtocol getters and press unavailable; public array count only; no opaque descendants enumerated.")
                    for node in tree.nodes { print("Replay AX node: \(node)") }
                }
                try await harness.signal("shown", count: shown + 1, from: page,
                                         timeout: max(.zero, ContinuousClock.now.duration(to: deadline)))
                let label = CardRuntime.replayLabel
                let publicAXTask: Task<PublicAXFacts, Never>? = needsPublicAXDiagnostic ? Task.detached {
                    Self.publicAXFacts(expectedIDs: ids, localizedLabel: label, deadline: deadline)
                } : nil
                needsPublicAXDiagnostic = false
                tree = Self.hierarchy(page.webView)
                firstNativeCounts = tree.counts
                observations = 1
                // Observe asynchronous native publication, not framework reruns.
                // IDs and names are deliberately absent from this condition:
                // a published link with a missing ID must fail immediately.
                while tree.links.count != ids.count && !tree.truncated && ContinuousClock.now < deadline {
                    try await Task.sleep(for: max(.zero, min(.milliseconds(5), ContinuousClock.now.duration(to: deadline))))
                    tree = Self.hierarchy(page.webView)
                    observations += 1
                }
                if let publicAXTask {
                    let facts = await publicAXTask.value
                    print("Replay public own-app AX diagnostic: \(facts.counts)")
                }
                let links = tree.links
                let observed = "\(tree.counts) identifiedLinks=\(links.filter { $0.accessibilityIdentifier() != nil }.count) expectedIDMatches=\(links.filter { ids.contains($0.accessibilityIdentifier() ?? "") }.count) localizedLinks=\(links.filter { $0.accessibilityLabel() == CardRuntime.replayLabel || $0.accessibilityTitle() == CardRuntime.replayLabel }.count)"
                #expect(!tree.truncated, "Native AX traversal exceeded 256 nodes or depth 32: \(observed)")
                // AXDOMIdentifier alone still fails the original native checks.
                #expect(links.count == ids.count, "Real WebKit AX links: \(observed)")
                #expect(Set(links.compactMap { $0.accessibilityIdentifier() }) == Set(ids), "HTML IDs must reach AXIdentifier: \(observed)")
                for (id, command) in zip(ids, commands) {
                    let matching = links.filter { $0.accessibilityIdentifier() == id }
                    let link = try #require(matching.count == 1 ? matching.first : nil, "Missing/ambiguous native AX identifier: \(observed)")
                    // WebKit can represent a link's accessible name as its
                    // label or title. Both are native AX properties.
                    #expect(link.accessibilityLabel() == CardRuntime.replayLabel || link.accessibilityTitle() == CardRuntime.replayLabel)
                    let count = harness.signals("command", from: page).count
                    try #require(link.accessibilityPerformPress(), "Native AX press failed")
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
