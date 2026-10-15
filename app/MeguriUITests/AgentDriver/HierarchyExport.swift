import Foundation
import XCTest

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
/// taps go there and nowhere else.
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

        /// Depth-first, the node itself first.
        func allNodes() -> [Node] {
            [self] + children.flatMap { $0.allNodes() }
        }
    }

    struct Tree {
        let root: Node
        /// The app's first window.
        let window: CGRect

        /// The elements whose identifier is `identifier`, depth-first.
        func elements(_ identifier: String) -> [Node] {
            root.allNodes().filter { $0.identifier == identifier }
        }
    }

    @MainActor
    static func tree(_ app: XCUIApplication) throws -> Tree {
        let snapshot = try app.snapshot()
        let window = snapshot.children.first { $0.elementType == .window }?.frame ?? snapshot.frame
        return Tree(root: node(snapshot, window: window), window: window)
    }

    /// The JSON document for a tree and the screenshot taken with it.
    static func json(_ tree: Tree, platform: String, screenshotOrigin: CGPoint, screenshotScale: Double) throws -> Data {
        let document = Document(
            schema: schema,
            platform: platform,
            capturedAt: Date().timeIntervalSince1970,
            window: Rect(tree.window),
            screenshot: Screenshot(origin: Point(screenshotOrigin), scale: screenshotScale),
            root: Element(tree.root)
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return try encoder.encode(document)
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
        }

        enum CodingKeys: String, CodingKey {
            case type, identifier, label, value, frame, hitPoint, enabled, selected, focused, children
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
