#if os(macOS)
import AppKit
import SwiftUI

/// AppKit owns the AX topology; drawing and pointer gestures stay in SwiftUI.
/// In particular, neither this view nor its ancestors may be the SwiftUI
/// focusable leaf (which replaces the canvas's accessibility children).
struct NativeCanvasAccessibility: NSViewRepresentable {
    struct Item {
        let identifier: String
        let label: String
        let frame: CGRect
        var role: NSAccessibility.Role = .button
        var selected = false
        var press: (() -> Void)?
        var adjust: ((Bool) -> Void)?
        var actions: [(String, () -> Void)] = []
    }
    let items: [Item]
    let enabled: Bool

    func makeNSView(context: Context) -> Container { Container() }
    func updateNSView(_ view: Container, context: Context) { view.update(items, enabled: enabled) }

    final class Container: NSView {
        private var elements: [Element] = []
        override var isFlipped: Bool { true }
        // An AX hit is independent of a mouse hit. Never intercept a draw drag.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override init(frame: NSRect) {
            super.init(frame: frame)
            setAccessibilityElement(true)
            setAccessibilityRole(.group)
            setAccessibilityLabel(ioString("io.a11y.canvas"))
            setAccessibilityIdentifier("io.canvas")
        }
        required init?(coder: NSCoder) { nil }

        func update(_ items: [Item], enabled: Bool) {
            let changed = items.count != elements.count || zip(items, elements).contains { item, element in
                item.identifier != element.identifier || item.frame != element.item?.frame || item.selected != element.item?.selected
            }
            let previous = Dictionary(uniqueKeysWithValues: elements.map { ($0.identifier, $0) })
            elements = items.map { item in
                let element = previous[item.identifier] ?? Element(owner: self, identifier: item.identifier)
                element.item = item
                element.setAccessibilityRole(item.role)
                element.setAccessibilityLabel(item.label)
                element.setAccessibilitySelected(item.selected)
                element.setAccessibilityEnabled(enabled)
                element.setAccessibilityCustomActions(item.actions.map { name, action in
                    NSAccessibilityCustomAction(name: name) { _ in if enabled { action() }; return enabled }
                })
                element.enabled = enabled
                return element
            }
            setAccessibilityChildren(elements)
            if changed { NSAccessibility.post(element: self, notification: .layoutChanged) }
        }
        override func accessibilityHitTest(_ point: NSPoint) -> Any? {
            elements.reversed().first { $0.accessibilityFrame().contains(point) } ?? self
        }
    }

    final class Element: NSAccessibilityElement {
        private weak var owner: Container?
        let identifier: String
        var item: Item?
        var enabled = true
        init(owner: Container, identifier: String) {
            self.owner = owner; self.identifier = identifier
            super.init()
            setAccessibilityElement(true)
            setAccessibilityIdentifier(identifier)
        }
        override func accessibilityParent() -> Any? { owner }
        // Resolve on every AX read: window moves, scrolling and flipped local
        // coordinates must not leave a cached global frame behind.
        override func accessibilityFrame() -> NSRect {
            guard let owner, let window = owner.window, let item else { return .zero }
            return window.convertToScreen(owner.convert(item.frame, to: nil))
        }
        override func accessibilityPerformPress() -> Bool {
            guard enabled, let press = item?.press else { return false }
            press(); return true
        }
        override func accessibilityPerformIncrement() -> Bool { adjust(true) }
        override func accessibilityPerformDecrement() -> Bool { adjust(false) }
        private func adjust(_ increment: Bool) -> Bool {
            guard enabled, let action = item?.adjust else { return false }
            action(increment); return true
        }
    }
}
#endif
