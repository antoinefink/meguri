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
                element.setAccessibilityCustomActions(item.actions.enumerated().map { index, action in
                    NSAccessibilityCustomAction(name: action.0) { [weak element] in
                        element?.performCustomAction(at: index) ?? false
                    }
                })
                element.enabled = enabled
                return element
            }
            setAccessibilityChildren(elements)
            if changed { NSAccessibility.post(element: self, notification: .layoutChanged) }
        }
        nonisolated override func accessibilityHitTest(_ point: NSPoint) -> Any? {
            let hit: Element? = NativeCanvasAccessibility.onMain {
                self.elements.reversed().first { $0.accessibilityFrame().contains(point) }
            }
            if let hit { return hit }
            return self
        }
    }

    // AppKit may pass these identities between AX threads. The unchecked
    // conformance also permits the concrete hit-test result through onMain.
    // All mutable canvas state stays in the MainActor-isolated State.
    nonisolated final class Element: NSAccessibilityElement, @unchecked Sendable {
        private nonisolated let state: State
        let identifier: String
        @MainActor var item: Item? {
            get { state.item }
            set { state.item = newValue }
        }
        @MainActor var enabled: Bool {
            get { state.enabled }
            set { state.enabled = newValue }
        }
        @MainActor init(owner: Container, identifier: String) {
            self.state = State(owner: owner); self.identifier = identifier
            super.init()
            setAccessibilityElement(true)
            setAccessibilityIdentifier(identifier)
        }
        override func accessibilityParent() -> Any? {
            let parent: Container? = NativeCanvasAccessibility.onMain { [state] in state.owner }
            return parent
        }
        override func accessibilityFrame() -> NSRect {
            NativeCanvasAccessibility.onMain { [state] in state.frame() }
        }
        override func accessibilityPerformPress() -> Bool {
            NativeCanvasAccessibility.onMain { [state] in state.press() }
        }
        override func accessibilityPerformIncrement() -> Bool {
            NativeCanvasAccessibility.onMain { [state] in state.adjust(true) }
        }
        override func accessibilityPerformDecrement() -> Bool {
            NativeCanvasAccessibility.onMain { [state] in state.adjust(false) }
        }
        fileprivate func performCustomAction(at index: Int) -> Bool {
            NativeCanvasAccessibility.onMain { [state] in state.performCustomAction(at: index) }
        }

        @MainActor private final class State {
            weak var owner: Container?
            var item: Item?
            var enabled = true

            init(owner: Container) { self.owner = owner }

            // Resolve on every AX read: window moves, scrolling and flipped
            // local coordinates must not leave a cached global frame behind.
            func frame() -> NSRect {
                guard let owner, let window = owner.window, let item else { return .zero }
                return window.convertToScreen(owner.convert(item.frame, to: nil))
            }
            func press() -> Bool {
                guard enabled, let press = item?.press else { return false }
                press(); return true
            }
            func adjust(_ increment: Bool) -> Bool {
                guard enabled, let action = item?.adjust else { return false }
                action(increment); return true
            }
            func performCustomAction(at index: Int) -> Bool {
                guard enabled, let item, item.actions.indices.contains(index) else { return false }
                item.actions[index].1(); return true
            }
        }
    }

    // AX entry points are synchronous and need not arrive on the main thread.
    // Only assume isolation after checking that thread or dispatching to it.
    // Concrete Sendable results keep Any out of this boundary.
    private nonisolated static func onMain<Value: Sendable>(
        _ body: @MainActor @Sendable () -> Value
    ) -> Value {
        if Thread.isMainThread { return MainActor.assumeIsolated(body) }
        return DispatchQueue.main.sync { MainActor.assumeIsolated(body) }
    }
}
#endif
