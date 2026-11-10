import Foundation
import XCTest
#if os(macOS)
import AppKit
import ApplicationServices
#endif

/// The UI hierarchy as JSON for agents and `hierarchy-check` (U8, R1964,
/// R1965): every element with its type, identifier, label, value, frame, hit
/// point and state, nested as in the app. Frames and hit points are in screen
/// points; `screenshot` says where the step's screenshot starts and how many
/// pixels it has per point.
///
/// ```json
/// {"schema": 1, "platform": "ios", "captured_at": 1894701600.5,
///  "window": {"x": 0, "y": 0, "width": 402, "height": 874},
///  "screenshot": {"origin": {"x": 0, "y": 0}, "scale": 3},
///  "root": {"type": "application", "identifier": "", "label": "Meguri", "value": null,
///           "frame": {...}, "hit_point": {"x": 201, "y": 437}, "enabled": true,
///           "selected": false, "focused": false, "children": [...]}}
/// ```
///
/// The Mac menu bar is left out (system UI that lists the user's recent
/// files).
///
/// `hit_point` is the centre of the part of the element's frame inside its
/// window (a Mac sheet is a window of its own), or null when none of it is;
/// ordinary taps go there. D4 generated replays use the retained native AX
/// link's press instead; see tools/scripts/hierarchy-check/native-web-schema.md.
enum HierarchyExport {
    static let schema = 1

    struct Node {
        let type: String
        let identifier: String
        let label: String
        let value: String?
        let frame: CGRect
        let hitPoint: CGPoint?
        let enabled: Bool
        let selected: Bool
        let focused: Bool
        let children: [Node]
        var axDOMIdentifier: String? = nil
        var axDOMIdentifierError: Int? = nil
        var axDOMIdentifierWrongType: Bool? = nil
        var nativeAX: Bool? = nil
        var axIdentifier: String? = nil
        var axIdentifierError: Int? = nil
        var axIdentifierWrongType: Bool? = nil

        /// Depth-first, the node itself first.
        func allNodes() -> [Node] {
            [self] + children.flatMap { $0.allNodes() }
        }
    }

    struct Tree {
        let root: Node
        /// The app's first window.
        let window: CGRect
        var xcuiRoot: Node? = nil
        #if os(macOS)
        var nativeWeb: NativeWebCapture? = nil
        #endif

        /// The elements whose identifier is `identifier`, depth-first.
        @MainActor func elements(_ identifier: String) -> [Node] {
            root.allNodes().filter { node in
                #if os(macOS)
                if nativeWeb?.binding.generatedIDs.contains(identifier) == true,
                   node.nativeAX == true, node.type == "link" {
                    return node.axDOMIdentifier == identifier && node.axDOMIdentifierError == 0
                        && node.axDOMIdentifierWrongType == false
                }
                #endif
                return node.identifier == identifier
            }
        }
    }

    @MainActor
    static func tree(_ app: XCUIApplication, requireNativeBinding: Bool = true) throws -> Tree {
        let snapshot = try app.snapshot()
        let window = snapshot.children.first { $0.elementType == .window }?.frame ?? snapshot.frame
        let xcui = node(snapshot, window: window)
        #if os(macOS)
        let binding: NativeWebCapture.Binding?
        if requireNativeBinding { binding = try NativeWebCapture.binding(in: app) }
        else { binding = try? NativeWebCapture.binding(in: app) }
        if let binding {
            let native = try NativeWebCapture(app: app, binding: binding, window: window)
            guard try NativeWebCapture.binding(in: app) == binding else { throw NativeWebCapture.Failure("Stale native web binding during capture") }
            var replacements = 0
            func replace(_ node: Node) -> Node {
                if node.type == "webView", node.identifier == binding.page {
                    replacements += 1
                    return native.root
                }
                return Node(type: node.type, identifier: node.identifier, label: node.label,
                    value: node.value, frame: node.frame, hitPoint: node.hitPoint, enabled: node.enabled,
                    selected: node.selected, focused: node.focused, children: node.children.map(replace))
            }
            let root = replace(xcui)
            guard replacements == 1 else { throw NativeWebCapture.Failure("Owned XCTest web view must be unique") }
            return Tree(root: root, window: window, xcuiRoot: xcui, nativeWeb: native)
        }
        #endif
        return Tree(root: xcui, window: window)
    }

    /// The JSON document for a tree and the screenshot taken with it.
    @MainActor static func json(_ tree: Tree, platform: String, screenshotOrigin: CGPoint, screenshotScale: Double) throws -> Data {
        let document = Document(
            schema: schema,
            platform: platform,
            capturedAt: Date().timeIntervalSince1970,
            window: Rect(tree.window),
            screenshot: Screenshot(origin: Point(screenshotOrigin), scale: screenshotScale),
            root: Element(tree.root), xcuiRoot: tree.xcuiRoot.map(Element.init)
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.keyEncodingStrategy = .convertToSnakeCase
        var data = try encoder.encode(document)
        #if os(macOS)
        if let native = tree.nativeWeb {
            var object = try JSONSerialization.jsonObject(with: data) as! [String: Any]
            object["native_web"] = try JSONSerialization.jsonObject(with: encoder.encode(native.binding))
            object["native_web_binding_at_end"] = try JSONSerialization.jsonObject(with: encoder.encode(native.bindingAtEnd))
            object["native_web_rule"] = "render/display-transform#D4"
            data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
        }
        #endif
        return data
    }

    /// The width in pixels of a PNG, from its IHDR chunk.
    static func pngPixelWidth(_ png: Data) -> Int? {
        let bytes = [UInt8](png.prefix(24))
        guard bytes.count == 24, bytes[0 ..< 8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] else { return nil }
        return bytes[16 ..< 20].reduce(0) { $0 << 8 | Int($1) }
    }

    // MARK: Building

    @MainActor
    private static func node(_ snapshot: XCUIElementSnapshot, window inherited: CGRect) -> Node {
        let window = [.window, .sheet].contains(snapshot.elementType) ? snapshot.frame : inherited
        let visible = snapshot.frame.intersection(window)
        let hitPoint = visible.isNull || visible.isEmpty ? nil : CGPoint(x: visible.midX, y: visible.midY)
        return Node(
            type: typeName(snapshot.elementType),
            identifier: snapshot.identifier,
            label: snapshot.label,
            value: snapshot.value.map { "\($0)" },
            frame: snapshot.frame,
            hitPoint: hitPoint,
            enabled: snapshot.isEnabled,
            selected: snapshot.isSelected,
            focused: hasFocus(snapshot),
            // The menu bar is system UI, and its Apple menu lists the user's
            // recent files: it is never exported.
            children: snapshot.children.filter { $0.elementType != .menuBar }.map { node($0, window: window) }
        )
    }

    /// Keyboard focus; XCUITest reports it on iOS only (always false on the Mac).
    @MainActor
    private static func hasFocus(_ snapshot: XCUIElementSnapshot) -> Bool {
        #if os(macOS)
        false
        #else
        snapshot.hasFocus
        #endif
    }

    // MARK: JSON

    private struct Document: Encodable {
        let schema: Int
        let platform: String
        let capturedAt: Double
        let window: Rect
        let screenshot: Screenshot
        let root: Element
        let xcuiRoot: Element?
    }

    private struct Screenshot: Encodable {
        let origin: Point
        let scale: Double
    }

    private struct Point: Encodable {
        let x: Double, y: Double

        init(_ point: CGPoint) {
            x = round2(point.x)
            y = round2(point.y)
        }
    }

    private struct Rect: Encodable {
        let x: Double, y: Double, width: Double, height: Double

        init(_ rect: CGRect) {
            guard !rect.isNull, rect.minX.isFinite, rect.minY.isFinite else {
                (x, y, width, height) = (0, 0, 0, 0)
                return
            }
            (x, y, width, height) = (round2(rect.minX), round2(rect.minY), round2(rect.width), round2(rect.height))
        }
    }

    private struct Element: Encodable {
        let type: String
        let identifier: String
        let label: String
        let value: String?
        let frame: Rect
        let hitPoint: Point?
        let enabled: Bool
        let selected: Bool
        let focused: Bool
        let children: [Element]
        let axDOMIdentifier: String?
        let axDOMIdentifierError: Int?
        let axDOMIdentifierWrongType: Bool?
        let nativeAX: Bool?
        let axIdentifier: String?
        let axIdentifierError: Int?
        let axIdentifierWrongType: Bool?

        init(_ node: Node) {
            type = node.type
            identifier = node.identifier
            label = node.label
            value = node.value
            frame = Rect(node.frame)
            hitPoint = node.hitPoint.map(Point.init)
            enabled = node.enabled
            selected = node.selected
            focused = node.focused
            children = node.children.map(Element.init)
            axDOMIdentifier = node.axDOMIdentifier
            axDOMIdentifierError = node.axDOMIdentifierError
            axDOMIdentifierWrongType = node.axDOMIdentifierWrongType
            nativeAX = node.nativeAX
            axIdentifier = node.axIdentifier
            axIdentifierError = node.axIdentifierError
            axIdentifierWrongType = node.axIdentifierWrongType
        }

        enum CodingKeys: String, CodingKey {
            case type, identifier, label, value, frame, hitPoint, enabled, selected, focused, children
            case axDOMIdentifier, axDOMIdentifierError, axDOMIdentifierWrongType, nativeAX
            case axIdentifier, axIdentifierError, axIdentifierWrongType
        }

        /// Writes `value` and `hit_point` as null rather than leaving them out.
        func encode(to encoder: any Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(type, forKey: .type)
            try container.encode(identifier, forKey: .identifier)
            try container.encode(label, forKey: .label)
            try container.encode(value, forKey: .value)
            try container.encode(frame, forKey: .frame)
            try container.encode(hitPoint, forKey: .hitPoint)
            try container.encode(enabled, forKey: .enabled)
            try container.encode(selected, forKey: .selected)
            try container.encode(focused, forKey: .focused)
            try container.encode(children, forKey: .children)
            if nativeAX == true {
                try container.encode(nativeAX, forKey: .nativeAX)
                try container.encode(axDOMIdentifier, forKey: .axDOMIdentifier)
                try container.encode(axDOMIdentifierError, forKey: .axDOMIdentifierError)
                try container.encode(axDOMIdentifierWrongType, forKey: .axDOMIdentifierWrongType)
                try container.encode(axIdentifier, forKey: .axIdentifier)
                try container.encode(axIdentifierError, forKey: .axIdentifierError)
                try container.encode(axIdentifierWrongType, forKey: .axIdentifierWrongType)
            }
        }
    }

    private static func round2(_ value: CGFloat) -> Double {
        (Double(value) * 100).rounded() / 100
    }

    // MARK: Element types

    /// The `XCUIElement.ElementType` case name, which `hierarchy-check` reads.
    static func typeName(_ type: XCUIElement.ElementType) -> String {
        switch type {
        case .any: "any"
        case .other: "other"
        case .application: "application"
        case .group: "group"
        case .window: "window"
        case .sheet: "sheet"
        case .drawer: "drawer"
        case .alert: "alert"
        case .dialog: "dialog"
        case .button: "button"
        case .radioButton: "radioButton"
        case .radioGroup: "radioGroup"
        case .checkBox: "checkBox"
        case .disclosureTriangle: "disclosureTriangle"
        case .popUpButton: "popUpButton"
        case .comboBox: "comboBox"
        case .menuButton: "menuButton"
        case .toolbarButton: "toolbarButton"
        case .popover: "popover"
        case .keyboard: "keyboard"
        case .key: "key"
        case .navigationBar: "navigationBar"
        case .tabBar: "tabBar"
        case .tabGroup: "tabGroup"
        case .toolbar: "toolbar"
        case .statusBar: "statusBar"
        case .table: "table"
        case .tableRow: "tableRow"
        case .tableColumn: "tableColumn"
        case .outline: "outline"
        case .outlineRow: "outlineRow"
        case .browser: "browser"
        case .collectionView: "collectionView"
        case .slider: "slider"
        case .pageIndicator: "pageIndicator"
        case .progressIndicator: "progressIndicator"
        case .activityIndicator: "activityIndicator"
        case .segmentedControl: "segmentedControl"
        case .picker: "picker"
        case .pickerWheel: "pickerWheel"
        case .switch: "switch"
        case .toggle: "toggle"
        case .link: "link"
        case .image: "image"
        case .icon: "icon"
        case .searchField: "searchField"
        case .scrollView: "scrollView"
        case .scrollBar: "scrollBar"
        case .staticText: "staticText"
        case .textField: "textField"
        case .secureTextField: "secureTextField"
        case .datePicker: "datePicker"
        case .textView: "textView"
        case .menu: "menu"
        case .menuItem: "menuItem"
        case .menuBar: "menuBar"
        case .menuBarItem: "menuBarItem"
        case .map: "map"
        case .webView: "webView"
        case .incrementArrow: "incrementArrow"
        case .decrementArrow: "decrementArrow"
        case .timeline: "timeline"
        case .ratingIndicator: "ratingIndicator"
        case .valueIndicator: "valueIndicator"
        case .splitGroup: "splitGroup"
        case .splitter: "splitter"
        case .relevanceIndicator: "relevanceIndicator"
        case .colorWell: "colorWell"
        case .helpTag: "helpTag"
        case .matte: "matte"
        case .dockItem: "dockItem"
        case .ruler: "ruler"
        case .rulerMarker: "rulerMarker"
        case .grid: "grid"
        case .levelIndicator: "levelIndicator"
        case .cell: "cell"
        case .layoutArea: "layoutArea"
        case .layoutItem: "layoutItem"
        case .handle: "handle"
        case .stepper: "stepper"
        case .tab: "tab"
        case .touchBar: "touchBar"
        case .statusItem: "statusItem"
        @unknown default: "type-\(type.rawValue)"
        }
    }
}

#if os(macOS)
/// Poll observations only. A missing publication can wait; an invalid
/// observation throws immediately. No action is performed or retried here.
@MainActor
enum NativeReplayWait {
    static func until(id: String, timeout: TimeInterval, gone: Bool,
                      now: () -> Date = { Date() },
                      pause: (TimeInterval) -> Void = { RunLoop.current.run(until: Date().addingTimeInterval($0)) },
                      observe: (Date) throws -> Bool) throws {
        let deadline = now().addingTimeInterval(timeout)
        while now() < deadline {
            let present = try observe(deadline)
            guard now() <= deadline else { break }
            if present != gone { return }
            let remaining = deadline.timeIntervalSince(now())
            if remaining > 0 { pause(min(0.05, remaining)) }
        }
        throw NativeWebCapture.Failure("Native replay '\(id)' did not \(gone ? "disappear" : "appear") within \(timeout) s")
    }
}

/// Public AX access is scoped to this launched app's windows and exact owned
/// page identifier. Retain original AX elements; never correlate leaves by
/// labels, order, frames, hit points or a JavaScript inventory.
@MainActor
final class NativeWebCapture {
    private struct PublicationPending: Error {}
    nonisolated struct Failure: Error, CustomStringConvertible {
        let description: String
        init(_ description: String) { self.description = description }
    }
    nonisolated struct Binding: Codable, Equatable, Sendable {
        let page: String
        let document: String
        let card: Int64
        let generation: UInt64
        let side: String
        let root: String
        let generatedIDs: [String]
        enum CodingKeys: String, CodingKey {
            case page, document, card, generation, side, root
            case generatedIDs = "generatedIds"
        }
    }
    let binding: Binding
    let bindingAtEnd: Binding
    let root: HierarchyExport.Node
    private let elements: [(AXUIElement, HierarchyExport.Node)]
    private let process: pid_t

    static func binding(in app: XCUIApplication) throws -> Binding? {
        let reports = app.descendants(matching: .any).matching(identifier: "testhooks.replay.report")
        guard reports.count > 0 else { return nil }
        guard reports.count == 1, let text = reports.firstMatch.value as? String,
              let report = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
              report["ready"] as? Bool == true, report["failure"] as? String == "",
              let raw = report["nativeWebBinding"] as? [String: Any] else {
            throw Failure("Native replay host binding unavailable")
        }
        let binding = try JSONDecoder().decode(Binding.self, from: JSONSerialization.data(withJSONObject: raw))
        guard !binding.page.isEmpty, UUID(uuidString: binding.document) != nil,
              binding.side == "question" || binding.side == "answer",
              !binding.root.isEmpty, Set(binding.generatedIDs).count == binding.generatedIDs.count,
              binding.generatedIDs.allSatisfy({ !$0.isEmpty }) else { throw Failure("Invalid trusted replay binding") }
        return binding
    }

    private static func read(_ element: AXUIElement, _ attribute: String, deadline: Date? = nil) -> (CFTypeRef?, AXError) {
        if let deadline {
            let remaining = deadline.timeIntervalSinceNow
            guard remaining > 0,
                  AXUIElementSetMessagingTimeout(element, Float(min(0.25, remaining))) == .success else {
                return (nil, .cannotComplete)
            }
        }
        var value: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
        return (value, error)
    }
    private static func string(_ element: AXUIElement, _ attribute: String, deadline: Date? = nil) -> String? {
        let (value, error) = read(element, attribute, deadline: deadline)
        guard error == .success, let value, CFGetTypeID(value) == CFStringGetTypeID() else { return nil }
        return value as? String
    }
    private static func children(_ element: AXUIElement, _ attribute: String = kAXChildrenAttribute,
                                 deadline: Date? = nil) throws -> [AXUIElement] {
        let (value, error) = read(element, attribute, deadline: deadline)
        if error == .attributeUnsupported || error == .noValue { return [] }
        guard error == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID(),
              let children = value as? [AXUIElement], children.count <= 4096 else {
            throw Failure("Native AX children unavailable or truncated: \(attribute) error=\(error.rawValue)")
        }
        return children
    }

    init(app: XCUIApplication, binding: Binding, window: CGRect,
         deadline: Date = Date().addingTimeInterval(5)) throws {
        self.binding = binding
        let bundle = try BuildUnderTest.current().bundleIdentifier
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
            .filter { !$0.isTerminated }
        guard running.count == 1, let application = running.first else { throw Failure("Owned app process must be unique") }
        process = application.processIdentifier
        let applicationAX = AXUIElementCreateApplication(process)
        let remaining = deadline.timeIntervalSinceNow
        guard remaining > 0 else { throw Failure("Native AX observation deadline expired") }
        guard AXUIElementSetMessagingTimeout(applicationAX, Float(min(0.25, remaining))) == .success else { throw Failure("Native AX timeout unavailable") }
        var visited: [AXUIElement] = []
        var roots: [AXUIElement] = []
        func find(_ element: AXUIElement, depth: Int) throws {
            guard Date() < deadline, depth <= 64, visited.count < 4096 else { throw Failure("Native AX traversal exceeded bound") }
            if visited.contains(where: { CFEqual($0, element) }) { return }
            visited.append(element)
            if Self.string(element, kAXIdentifierAttribute, deadline: deadline) == binding.page { roots.append(element); return }
            for child in try Self.children(element, deadline: deadline) { try find(child, depth: depth + 1) }
        }
        for ownedWindow in try Self.children(applicationAX, kAXWindowsAttribute, deadline: deadline) { try find(ownedWindow, depth: 0) }
        if roots.isEmpty { throw PublicationPending() }
        guard roots.count == 1, let owned = roots.first else { throw Failure("Exact owned native page root ambiguous") }
        visited.removeAll()
        var retained: [(AXUIElement, HierarchyExport.Node)] = []
        func capture(_ element: AXUIElement, depth: Int) throws -> HierarchyExport.Node {
            guard Date() < deadline, depth <= 64, visited.count < 4096,
                  !visited.contains(where: { CFEqual($0, element) }) else { throw Failure("Native web tree truncated or cyclic") }
            visited.append(element)
            let role = try XCTUnwrap(Self.string(element, kAXRoleAttribute, deadline: deadline), "Native AX role unavailable")
            let types = ["AXWebArea": "webView", "AXLink": "link", "AXStaticText": "staticText",
                         "AXGroup": "group", "AXScrollArea": "scrollView", "AXImage": "image",
                         "AXButton": "button", "AXTextField": "textField", "AXTextArea": "textView",
                         "AXCheckBox": "checkBox", "AXRadioButton": "radioButton", "AXRadioGroup": "radioGroup",
                         "AXPopUpButton": "popUpButton", "AXComboBox": "comboBox", "AXMenuButton": "menuButton",
                         "AXSlider": "slider", "AXIncrementor": "stepper", "AXDisclosureTriangle": "disclosureTriangle",
                         "AXTable": "table", "AXRow": "tableRow", "AXCell": "cell", "AXOutline": "outline",
                         "AXScrollBar": "scrollBar", "AXMenu": "menu", "AXMenuItem": "menuItem",
                         "AXTabGroup": "tabGroup", "AXProgressIndicator": "progressIndicator"]
            func geometry<T>(_ attribute: String, type: AXValueType, initial: T) throws -> T {
                let (raw, error) = Self.read(element, attribute, deadline: deadline)
                guard error == .success, let raw, CFGetTypeID(raw) == AXValueGetTypeID() else {
                    throw Failure("Native AX geometry unavailable: \(attribute)")
                }
                var value = initial
                guard AXValueGetValue(raw as! AXValue, type, &value) else { throw Failure("Native AX geometry has wrong type") }
                return value
            }
            let point = try geometry(kAXPositionAttribute, type: .cgPoint, initial: CGPoint.zero)
            let size = try geometry(kAXSizeAttribute, type: .cgSize, initial: CGSize.zero)
            let frame = CGRect(origin: point, size: size)
            let visible = frame.intersection(window)
            let (rawID, idError) = Self.read(element, kAXIdentifierAttribute, deadline: deadline)
            let (rawDOM, domError) = Self.read(element, kAXDOMIdentifierAttribute, deadline: deadline)
            let domIsString = rawDOM.map { CFGetTypeID($0) == CFStringGetTypeID() } ?? false
            let children = try Self.children(element, deadline: deadline).map { try capture($0, depth: depth + 1) }
            var node = HierarchyExport.Node(type: types[role] ?? role,
                identifier: idError == .success ? (rawID as? String ?? "") : "",
                label: Self.string(element, kAXDescriptionAttribute, deadline: deadline).flatMap { $0.isEmpty ? nil : $0 }
                    ?? Self.string(element, kAXTitleAttribute, deadline: deadline) ?? "",
                value: Self.string(element, kAXValueAttribute, deadline: deadline), frame: frame,
                hitPoint: visible.isNull || visible.isEmpty ? nil : CGPoint(x: visible.midX, y: visible.midY),
                enabled: Self.read(element, kAXEnabledAttribute, deadline: deadline).0 as? Bool ?? false,
                selected: Self.read(element, kAXSelectedAttribute, deadline: deadline).0 as? Bool ?? false,
                focused: Self.read(element, kAXFocusedAttribute, deadline: deadline).0 as? Bool ?? false, children: children)
            node.nativeAX = true
            let idIsString = rawID.map { CFGetTypeID($0) == CFStringGetTypeID() } ?? false
            node.axIdentifier = idError == .success && idIsString ? rawID as? String : nil
            node.axIdentifierError = Int(idError.rawValue)
            node.axIdentifierWrongType = idError == .success && !idIsString
            node.axDOMIdentifier = domError == .success && domIsString ? rawDOM as? String : nil
            node.axDOMIdentifierError = Int(domError.rawValue)
            node.axDOMIdentifierWrongType = domError == .success && !domIsString
            retained.append((element, node))
            return node
        }
        let captured = try capture(owned, depth: 0)
        root = captured
        elements = retained
        guard captured.allNodes().filter({ $0.type == "webView" }).count == 1 else { throw Failure("Owned page must contain one native web area") }
        guard let current = try Self.binding(in: app), current == binding else { throw Failure("Native page binding changed during AX read") }
        bindingAtEnd = current
    }

    /// A genuinely absent candidate is pending. Published malformed candidates
    /// fail closed; names, positions and raw AXIdentifier never select a leaf.
    static func publishedLinkIndex(_ id: String, binding: Binding,
                                   nodes: [HierarchyExport.Node]) throws -> Int? {
        guard binding.generatedIDs.filter({ $0 == id }).count == 1 else { throw Failure("Replay ID lacks trusted ownership") }
        for node in nodes where node.type == "link" {
            guard node.nativeAX == true, node.axDOMIdentifierError == 0,
                  node.axDOMIdentifierWrongType == false,
                  let domID = node.axDOMIdentifier, !domID.isEmpty else {
                throw Failure("Published native link has missing, unsupported or wrong-type identity")
            }
        }
        let matches = nodes.indices.filter { nodes[$0].axDOMIdentifier == id }
        guard matches.count <= 1 else { throw Failure("Duplicate native web identity: \(id)") }
        guard let index = matches.first else { return nil }
        let node = nodes[index]
        guard node.type == "link", node.nativeAX == true,
              node.axDOMIdentifierError == 0, node.axDOMIdentifierWrongType == false else {
            throw Failure("Invalid published native replay candidate: \(id)")
        }
        return index
    }

    func uniqueLink(_ id: String) throws -> (AXUIElement, HierarchyExport.Node) {
        guard let index = try Self.publishedLinkIndex(id, binding: binding, nodes: elements.map(\.1)) else {
            throw Failure("Missing native web identity: \(id)")
        }
        return elements[index]
    }

    static func wait(_ id: String, in app: XCUIApplication, binding: Binding,
                     timeout: TimeInterval, gone: Bool,
                     observed: (Bool) -> Void = { _ in }) throws {
        try NativeReplayWait.until(id: id, timeout: timeout, gone: gone) { deadline in
            guard try Self.binding(in: app) == binding else { throw Failure("Stale native replay wait binding") }
            let present: Bool
            do {
                let fresh = try NativeWebCapture(app: app, binding: binding,
                    window: app.windows.firstMatch.frame, deadline: deadline)
                present = try Self.publishedLinkIndex(id, binding: binding, nodes: fresh.elements.map(\.1)) != nil
            } catch is PublicationPending {
                present = false
            }
            guard try Self.binding(in: app) == binding else { throw Failure("Stale native replay wait binding") }
            observed(present)
            return present
        }
    }

    func press(_ id: String, in app: XCUIApplication) throws {
        let (original, _) = try uniqueLink(id)
        guard try Self.binding(in: app) == binding else { throw Failure("Stale page/card/generation/side binding") }
        let fresh = try NativeWebCapture(app: app, binding: binding, window: app.windows.firstMatch.frame)
        let (current, node) = try fresh.uniqueLink(id)
        guard fresh.process == process, CFEqual(current, original), node.enabled,
              node.hitPoint != nil, !node.frame.isEmpty,
              app.windows.firstMatch.frame.contains(node.frame),
              try Self.binding(in: app) == binding else { throw Failure("Stale or unavailable original native AX link") }
        let error = AXUIElementPerformAction(original, kAXPressAction as CFString)
        guard error == .success else { throw Failure("Native AX press failed: \(error.rawValue)") }
    }
}
#endif
