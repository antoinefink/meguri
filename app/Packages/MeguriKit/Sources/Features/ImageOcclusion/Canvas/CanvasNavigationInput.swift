import SwiftUI

#if os(macOS)
import AppKit

/// Local events are filtered to this canvas and window. No cursor/keyboard
/// synthesis, global monitor or ownership of the workstation's input.
struct CanvasNavigationInput: NSViewRepresentable {
    let navigate: (Double, CGSize, CGPoint) -> Void
    let ended: () -> Void
    let panStarted: () -> Void

    func makeNSView(context: Context) -> NavigationView { NavigationView() }
    func updateNSView(_ view: NavigationView, context: Context) {
        view.navigate = navigate; view.ended = ended; view.panStarted = panStarted
    }
    static func dismantleNSView(_ view: NavigationView, coordinator: ()) { view.stop() }

    final class NavigationView: NSView {
        var navigate: ((Double, CGSize, CGPoint) -> Void)?
        var ended: (() -> Void)?
        var panStarted: (() -> Void)?
        private var monitor: Any?
        private var space = false
        private var dragging = false
        override var isFlipped: Bool { true }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow(); stop()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel, .magnify, .keyDown, .keyUp, .leftMouseDown, .leftMouseDragged, .leftMouseUp]) { [weak self] event in
                guard let self else { return event }
                return self.receive(event)
            }
        }
        func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil; space = false; dragging = false
        }
        private func receive(_ event: NSEvent) -> NSEvent? {
            guard let window, window.isKeyWindow, event.window === window, !isHiddenOrHasHiddenAncestor else { return event }
            let p = convert(event.locationInWindow, from: nil)
            let inside = bounds.contains(p) && visibleRect.contains(p)
            switch event.type {
            case .scrollWheel where inside:
                panStarted?()
                if event.modifierFlags.contains(.command) {
                    navigate?(pow(0.999, event.scrollingDeltaY), .zero, p)
                } else {
                    let shift = event.modifierFlags.contains(.shift)
                    navigate?(1, .init(width: shift ? event.scrollingDeltaY : event.scrollingDeltaX,
                                       height: shift ? 0 : event.scrollingDeltaY), p)
                }
                ended?(); return nil
            case .magnify where inside:
                panStarted?(); navigate?(1 + event.magnification, .zero, p); ended?(); return nil
            case .keyDown where inside && event.charactersIgnoringModifiers == " ":
                // Native text entry (for example the label sheet) keeps Space.
                guard !(window.firstResponder is NSTextView) else { return event }
                space = true; return nil
            case .keyUp where space && event.charactersIgnoringModifiers == " ":
                space = false; if dragging { dragging = false; ended?() }; return nil
            case .leftMouseDown where inside && space:
                dragging = true; panStarted?(); return nil
            case .leftMouseDragged where dragging:
                navigate?(1, .init(width: event.deltaX, height: event.deltaY), p); return nil
            case .leftMouseUp where dragging:
                dragging = false; ended?(); return nil
            default: return event
            }
        }
    }
}
#else
import UIKit

struct CanvasNavigationInput: UIViewRepresentable {
    let navigate: (Double, CGSize, CGPoint) -> Void
    let ended: () -> Void
    let panStarted: () -> Void
    func makeUIView(context: Context) -> NavigationView { NavigationView() }
    func updateUIView(_ view: NavigationView, context: Context) {
        view.navigate = navigate; view.ended = ended; view.panStarted = panStarted
        view.install()
    }
    static func dismantleUIView(_ view: NavigationView, coordinator: ()) { view.uninstall() }

    final class NavigationView: UIView, UIGestureRecognizerDelegate {
        var navigate: ((Double, CGSize, CGPoint) -> Void)?
        var ended: (() -> Void)?
        var panStarted: (() -> Void)?
        private weak var target: UIView?
        private lazy var pinch = UIPinchGestureRecognizer(target: self, action: #selector(pinched))
        private lazy var pan: UIPanGestureRecognizer = {
            let recognizer = UIPanGestureRecognizer(target: self, action: #selector(panned))
            recognizer.minimumNumberOfTouches = 2; recognizer.maximumNumberOfTouches = 2
            return recognizer
        }()
        override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? { nil }
        override func didMoveToSuperview() { super.didMoveToSuperview(); install() }
        func install() {
            guard target !== superview, let superview else { return }
            uninstall(); target = superview
            pinch.delegate = self; pan.delegate = self
            superview.addGestureRecognizer(pinch); superview.addGestureRecognizer(pan)
        }
        func uninstall() { target?.removeGestureRecognizer(pinch); target?.removeGestureRecognizer(pan); target = nil }
        func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer) -> Bool { true }
        func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
            bounds.contains(touch.location(in: self))
        }
        @objc private func pinched(_ recognizer: UIPinchGestureRecognizer) {
            switch recognizer.state {
            case .began: panStarted?()
            case .changed:
                navigate?(recognizer.scale, .zero, recognizer.location(in: self)); recognizer.scale = 1
            case .ended, .cancelled, .failed: ended?()
            default: break
            }
        }
        @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
            switch recognizer.state {
            case .began: panStarted?()
            case .changed:
                let p = recognizer.translation(in: self)
                navigate?(1, .init(width: p.x, height: p.y), recognizer.location(in: self))
                recognizer.setTranslation(.zero, in: self)
            case .ended, .cancelled, .failed: ended?()
            default: break
            }
        }
    }
}
#endif
