#if os(macOS)
import AppKit
import ApplicationServices
import CardRendering
import Foundation
import Testing
import WebKit

// Mandatory app-hosted Snapshot VM proof. The package retains its DOM and
// mobile cases; only this fixture publishes a real Mac window.
@MainActor @Suite(.serialized, .timeLimit(.minutes(2)))
struct NativeReplayAccessibilityTests {
    // The D4 literal with explicit synthetic transform metadata, as supplied
    // by CardPayload in production. Rust tests separately pin its emission.
    static func control(_ id: String, command: String, platform: CardRuntime.Platform) -> String {
        let action = platform == .mac ? "pycmd('\(command)')" : "window.webkit.messageHandlers.meguri.postMessage('\(command)')"
        return """
            <a class="replay-button soundLink" id="\(id)" data-meguri-replay="true" aria-label="Play audio" href=# onclick="\(action); return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>
            """
    }

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

    static func diagnosticRole(_ value: Any?) -> String {
        guard let value else { return "nil" }
        guard let role = value as? String else { return "wrong-type" }
        switch role {
        case "AXLink": return "AXLink"
        case "AXWebArea": return "AXWebArea"
        case "AXScrollArea": return "AXScrollArea"
        case "AXGroup": return "AXGroup"
        default: return "other"
        }
    }

    static func diagnosticValueType(_ value: Any?) -> String {
        guard let value else { return "nil" }
        if value is String { return "string" }
        if value is NSArray { return "array" }
        return "other"
    }

    // Retain exact receivers and unmodified getter results locally. These never
    // enter PublicAXObservation or provide an identity/press target to the proof.
    struct NativeDiagnosticNode {
        let object: NSObject
        let path: String
        let depth: Int
        var hasGetter = false
        var roleRead = false
        var identifierRead = false
        var domIdentifierRead = false
        var descriptionRead = false
        var titleRead = false
        var childrenRead = false
        var role: Any?
        var identifier: Any?
        var domIdentifier: Any?
        var description: Any?
        var title: Any?
        var children: Any?
    }

    static func nativeSnapshot(webView: WKWebView, window: NSWindow,
                               windowIdentifier: String, expectedIDs: [String],
                               deadline: ContinuousClock.Instant) {
        // Legacy getters have no documented cancellable timeout. Deadline
        // checks bound admission to each read/step, not a getter already running.
        guard ContinuousClock.now < deadline else {
            print("Replay native snapshot: complete=false reason=deadline inconclusive=true")
            return
        }
        var reason = "complete"
        func admitted() -> Bool {
            guard ContinuousClock.now < deadline else {
                reason = "deadline"
                return false
            }
            return true
        }
        guard admitted() else {
            print("Replay native snapshot: complete=false reason=deadline inconclusive=true")
            return
        }
        let attachedWindow = webView.window
        guard attachedWindow === window else {
            print("Replay native snapshot: complete=false reason=window-ownership inconclusive=true")
            return
        }
        guard admitted() else {
            print("Replay native snapshot: complete=false reason=deadline inconclusive=true")
            return
        }
        let nativeWindowIdentifier = window.accessibilityIdentifier()
        guard nativeWindowIdentifier == windowIdentifier else {
            print("Replay native snapshot: complete=false reason=window-identifier inconclusive=true")
            return
        }
        guard admitted() else {
            print("Replay native snapshot: complete=false reason=deadline inconclusive=true")
            return
        }
        let mediaRoot = webView.url
        guard mediaRoot == NativeReplayFixture.mediaRoot else {
            print("Replay native snapshot: complete=false reason=media-root inconclusive=true")
            return
        }
        var nodes: [NativeDiagnosticNode] = []
        var seen = Set<ObjectIdentifier>()
        let getter = NSSelectorFromString("accessibilityAttributeValue:")
        func nativeAttributeValue(_ receiver: NSObject, _ attribute: NSAccessibility.Attribute) -> Any? {
            guard receiver.responds(to: getter),
                  let result = receiver.perform(getter, with: attribute.rawValue as NSString) else { return nil }
            return result.takeUnretainedValue()
        }
        func visitCollection(_ entries: NSArray, path: String, depth: Int) -> Bool {
            guard admitted() else { return false }
            let count = entries.count
            guard count <= 256 else { reason = "collection-limit"; return false }
            for index in 0..<count {
                guard admitted() else { return false }
                let entry = entries[index]
                // Do not manufacture an NSObject by bridging a Swift value.
                guard type(of: entry) is AnyClass, let node = entry as? NSObject else {
                    reason = "nonnative-entry"
                    return false
                }
                guard visit(node, path: "\(path)/\(index)", depth: depth) else { return false }
            }
            return true
        }
        func visit(_ node: NSObject, path: String, depth: Int) -> Bool {
            guard admitted() else { return false }
            guard depth <= 32 else { reason = "depth-limit"; return false }
            guard nodes.count < 256 else { reason = "node-limit"; return false }
            guard seen.insert(ObjectIdentifier(node)).inserted else { reason = "cycle"; return false }
            let index = nodes.count
            nodes.append(NativeDiagnosticNode(object: node, path: path, depth: depth))
            guard admitted() else { return false }
            nodes[index].hasGetter = node.responds(to: getter)
            guard nodes[index].hasGetter else { reason = "missing-getter"; return false }
            guard admitted() else { return false }
            nodes[index].role = nativeAttributeValue(node, .role)
            nodes[index].roleRead = true
            guard admitted() else { return false }
            nodes[index].identifier = nativeAttributeValue(node, .identifier)
            nodes[index].identifierRead = true
            guard admitted() else { return false }
            nodes[index].domIdentifier = nativeAttributeValue(node, NSAccessibility.Attribute(rawValue: "AXDOMIdentifier"))
            nodes[index].domIdentifierRead = true
            if nodes[index].role as? String == "AXLink" {
                guard admitted() else { return false }
                nodes[index].description = nativeAttributeValue(node, .description)
                nodes[index].descriptionRead = true
                guard admitted() else { return false }
                nodes[index].title = nativeAttributeValue(node, .title)
                nodes[index].titleRead = true
            }
            guard admitted() else { return false }
            nodes[index].children = nativeAttributeValue(node, .children)
            nodes[index].childrenRead = true
            guard admitted() else { return false }
            // A nil legacy value has no AXError and establishes no unsupported
            // attribute result. It cannot establish a complete children list.
            guard let children = nodes[index].children as? NSArray else {
                reason = nodes[index].children == nil ? "children-nil" : "children-wrong-type"
                return false
            }
            return visitCollection(children, path: path, depth: depth + 1)
        }
        guard admitted() else {
            print("Replay native snapshot: complete=false reason=deadline inconclusive=true")
            return
        }
        let roots = webView.accessibilityChildren()
        let complete: Bool
        if !admitted() {
            complete = false
        } else if let roots {
            complete = visitCollection(roots as NSArray, path: "root", depth: 0)
        } else {
            reason = "children-nil"
            complete = false
        }
        func objectType(_ node: NSObject) -> String {
            if node is WKWebView { return "WKWebView" }
            if node is NSWindow { return "NSWindow" }
            if node is NSView { return "NSView" }
            if node is NSAccessibilityElement { return "NSAccessibilityElement" }
            return "NSObject"
        }
        func identifierClass(_ value: Any?) -> String {
            guard let value else { return "nil" }
            guard let string = value as? String else { return "wrong-type" }
            if let index = expectedIDs.firstIndex(of: string) { return "known index=\(index)" }
            return "other"
        }
        var identifierMatches = Array(repeating: 0, count: expectedIDs.count)
        var domMatches = Array(repeating: 0, count: expectedIDs.count)
        var links = 0
        for node in nodes {
            // All output is fixed classifications or bounded tree coordinates;
            // raw values and original object references remain in nodes.
            print("Replay native snapshot node: path=\(node.path) depth=\(node.depth) objectType=\(objectType(node.object)) getter=\(node.hasGetter) roleRead=\(node.roleRead) roleType=\(diagnosticValueType(node.role)) role=\(diagnosticRole(node.role)) AXIdentifierRead=\(node.identifierRead) AXIdentifierType=\(diagnosticValueType(node.identifier)) AXIdentifier=\(identifierClass(node.identifier)) AXDOMIdentifierRead=\(node.domIdentifierRead) AXDOMIdentifierType=\(diagnosticValueType(node.domIdentifier)) AXDOMIdentifier=\(identifierClass(node.domIdentifier)) childrenRead=\(node.childrenRead) childrenType=\(diagnosticValueType(node.children))")
            if node.role as? String == "AXLink" {
                links += 1
                if let id = node.identifier as? String, let index = expectedIDs.firstIndex(of: id) {
                    identifierMatches[index] += 1
                }
                if let id = node.domIdentifier as? String, let index = expectedIDs.firstIndex(of: id) {
                    domMatches[index] += 1
                }
                let description = node.description as? String
                let title = node.title as? String
                print("Replay native snapshot link: path=\(node.path) descriptionRead=\(node.descriptionRead) descriptionType=\(diagnosticValueType(node.description)) titleRead=\(node.titleRead) titleType=\(diagnosticValueType(node.title)) descriptionReplay=\(description == CardRuntime.replayLabel) titleReplay=\(title == CardRuntime.replayLabel) descriptionAuthored=\(description == "Author & 猫") titleAuthored=\(title == "Author & 猫")")
            }
        }
        for index in expectedIDs.indices {
            print("Replay native snapshot matches: index=\(index) AXIdentifierCount=\(identifierMatches[index]) AXDOMIdentifierCount=\(domMatches[index])")
        }
        print("Replay native snapshot: complete=\(complete) reason=\(reason) inconclusive=\(!complete) nodes=\(nodes.count) links=\(links) rootsType=\(diagnosticValueType(roots)) synchronousGetterPreemptible=false")
    }

    /// Same-process AX reads synchronously enter AppKit/WebKit accessibility
    /// callbacks; keep them on the main actor that owns the hosted GUI.
    /// Read only the fixture's own process and exact window identifier.
    static func publicAXFacts(windowIdentifier: String,
                                          deadline: ContinuousClock.Instant) throws -> PublicAXObservation {
        var observation = PublicAXObservation()
        let application = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
        func read(_ node: AXUIElement, _ attribute: String) throws -> (CFTypeRef?, AXError) {
            let remaining = ContinuousClock.now.duration(to: deadline).components
            let seconds = Double(remaining.seconds) + Double(remaining.attoseconds) / 1e18
            guard seconds > 0 else { throw NativeReplayFailure("Own-window native AX deadline") }
            guard AXUIElementSetMessagingTimeout(node, Float(min(seconds, 0.25))) == .success else {
                throw NativeReplayFailure("Own-window native AX timeout unavailable")
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
        func children(_ node: AXUIElement, _ attribute: String = kAXChildrenAttribute,
                      path: String, depth: Int, role: String?, identifier: String?) throws -> [AXUIElement] {
            let (value, error) = try read(node, attribute)
            if error == .attributeUnsupported || error == .noValue { return [] }
            guard error == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID(),
                  let result = value as? [AXUIElement], result.count <= 256 else {
                // Diagnose only the failed read; never request a second AX value.
                let valueType = value.map { CFGetTypeID($0) == CFArrayGetTypeID() ? "array" : (CFGetTypeID($0) == CFStringGetTypeID() ? "string" : "other") } ?? "nil"
                let array = value.flatMap { CFGetTypeID($0) == CFArrayGetTypeID() ? $0 as? NSArray : nil }
                let arrayCount = array.map { String(min($0.count, 256)) } ?? "not-array"
                let arrayOverLimit = array.map { $0.count > 256 } ?? false
                let arrayCastSucceeded = (value as? [AXUIElement]) != nil
                let readKind = attribute == kAXWindowsAttribute ? "windows" : "children"
                throw NativeReplayFailure("Own-window native AX children unavailable/wrong-type/truncated: path=\(path) depth=\(depth) role=\(diagnosticRole(role)) AXIdentifier=\(identifier == nil ? "nil" : "other") read=\(readKind) AXError.rawValue=\(error.rawValue) valueIsNil=\(value == nil) CFType=\(valueType) arrayCount=\(arrayCount) arrayOverLimit=\(arrayOverLimit) AXUIElementArrayCastSucceeded=\(arrayCastSucceeded) AXIsProcessTrusted=\(AXIsProcessTrusted())")
            }
            return result
        }
        let windows = try children(application, kAXWindowsAttribute,
                                   path: "app-windows", depth: 0, role: nil, identifier: nil)
        var owned: [AXUIElement] = []
        for window in windows where try string(window, kAXIdentifierAttribute) == windowIdentifier { owned.append(window) }
        guard owned.count == 1, let window = owned.first else { throw NativeReplayFailure("Exact fixture AX window missing/duplicate") }
        var seen: [AXUIElement] = []
        func visit(_ node: AXUIElement, path: String, depth: Int) throws {
            guard depth <= 32, seen.count < 256, !seen.contains(where: { CFEqual($0, node) }) else {
                throw NativeReplayFailure("Own-window native AX tree truncated/cyclic")
            }
            seen.append(node)
            let role = try string(node, kAXRoleAttribute)
            let identifier = try string(node, kAXIdentifierAttribute)
            // Public AXWebConstants.h: kAXDOMIdentifierAttribute is
            // CFSTR("AXDOMIdentifier"); use its exact public attribute name.
            let (dom, error) = try read(node, "AXDOMIdentifier")
            let isString = dom.map { CFGetTypeID($0) == CFStringGetTypeID() } ?? false
            let domID = error == .success && isString ? dom as? String : nil
            observation.nodes.append("role=\(diagnosticRole(role)) AXIdentifier=\(identifier == nil ? "nil" : "other") AXDOMIdentifier=\(domID == nil ? "nil" : "other") DOMError=\(error.rawValue) DOMWrongType=\(error == .success && !isString)")
            // Public AXWebConstants.h: kAXLinkRole is CFSTR("AXLink");
            // that C macro is not imported by ApplicationServices in Swift.
            if role == "AXLink" {
                observation.links.append(PublicAXLink(element: node, rawIdentifier: identifier,
                    rawDOMIdentifier: domID, domError: error, domWrongType: error == .success && !isString,
                    label: try string(node, kAXDescriptionAttribute), title: try string(node, kAXTitleAttribute)))
            }
            for (index, child) in try children(node, path: path, depth: depth,
                                               role: role, identifier: identifier).enumerated() {
                try visit(child, path: "\(path)/\(index)", depth: depth + 1)
            }
        }
        try visit(window, path: "root", depth: 0)
        return observation
    }

    @Test func native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy() async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let windowID = "testhooks.replay.window.\(UUID().uuidString)"
            let harness = try NativeReplayFixture(nativeAccessibilityWindowIdentifier: windowID)
            defer { harness.close() }
            let page = try await harness.openPage()
            let window = try #require(page.webView.window)
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
                @MainActor func observe(until observationDeadline: ContinuousClock.Instant) async throws -> PublicAXObservation {
                    try Self.publicAXFacts(windowIdentifier: windowID, deadline: observationDeadline)
                }
                var tree: PublicAXObservation
                do {
                    defer {
                        if stage == 0 {
                            Self.nativeSnapshot(webView: page.webView, window: window,
                                windowIdentifier: windowID, expectedIDs: expectedIDs, deadline: deadline)
                        }
                    }
                    tree = try await observe(until: deadline)
                }
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
                    try #require(page.webView.window === window && page.webView.url == NativeReplayFixture.mediaRoot)
                    let binding = try #require(harness.signals("shown", from: page).last)
                    try #require(binding.card == card && binding.generation == generation && binding.side == (side == "q" ? "question" : "answer"))
                    let count = harness.signals("command", from: page).count
                    let error = AXUIElementPerformAction(link.element, kAXPressAction as CFString)
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
    }
}
#endif
