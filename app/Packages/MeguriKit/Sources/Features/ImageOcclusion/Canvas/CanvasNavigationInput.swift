import SwiftUI

#if os(macOS)
import AppKit

/// App-local input, owned only by this canvas in its key window.
struct CanvasNavigationInput: NSViewRepresentable {
    let navigate: (Double, CGSize, CGPoint) -> Void
    let ended: () -> Void
    let panStarted: () -> Void
    var acceptsKeyboard: () -> Bool = { true }
    var enabled = true
    var pressBegan: ((CGPoint, Bool, Int) -> Void)?
    var pressChanged: ((CGPoint) -> Void)?
    var pressEnded: ((CGPoint) -> Void)?
    var pressCancelled: (() -> Void)?
    var cursor: (CGPoint) -> NSCursor = { _ in .arrow }
    var pointerMoved: (CGPoint?) -> Void = { _ in }
    var contextPoint: (CGPoint) -> Void = { _ in }

    func makeNSView(context: Context) -> NavigationView {
        let view = NavigationView()
        configure(view) // Callbacks must exist before viewDidMoveToWindow.
        return view
    }
    func updateNSView(_ view: NavigationView, context: Context) { configure(view) }
    private func configure(_ view: NavigationView) {
        // Cancel with the old owner's callbacks before replacing them.
        view.setEnabled(enabled)
        view.navigate = navigate; view.ended = ended; view.panStarted = panStarted
        view.acceptsKeyboard = acceptsKeyboard
        view.pressBegan = pressBegan; view.pressChanged = pressChanged
        view.pressEnded = pressEnded; view.pressCancelled = pressCancelled
        view.cursor = cursor; view.pointerMoved = pointerMoved; view.contextPoint = contextPoint
        view.refreshCursor()
    }
    static func dismantleNSView(_ view: NavigationView, coordinator: ()) { view.stop() }

    final class NavigationView: NSView {
        var navigate: ((Double, CGSize, CGPoint) -> Void)?
        var ended: (() -> Void)?
        var panStarted: (() -> Void)?
        var acceptsKeyboard: () -> Bool = { true }
        var pressBegan: ((CGPoint, Bool, Int) -> Void)?
        var pressChanged: ((CGPoint) -> Void)?
        var pressEnded: ((CGPoint) -> Void)?
        var pressCancelled: (() -> Void)?
        var cursor: (CGPoint) -> NSCursor = { _ in .arrow }
        var pointerMoved: (CGPoint?) -> Void = { _ in }
        var contextPoint: (CGPoint) -> Void = { _ in }
        private var enabled = true
        private var monitor: Any?
        private var resignKeyObserver: NSObjectProtocol?
        private var tracking: NSTrackingArea?
        private var space = false
        private enum Press: Equatable { case editing, panning, cancelled }
        private var press: Press?
        private var pressCursor: NSCursor?
        private var pointer: CGPoint?
        private var ownsCursor = false
        override var isFlipped: Bool { true }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func updateTrackingAreas() {
            super.updateTrackingAreas()
            if let tracking { removeTrackingArea(tracking) }
            let area = NSTrackingArea(rect: .zero,
                options: [.activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited, .mouseMoved, .cursorUpdate],
                owner: self, userInfo: nil)
            addTrackingArea(area); tracking = area
        }
        override func mouseEntered(with event: NSEvent) { hover(event) }
        override func mouseMoved(with event: NSEvent) { hover(event) }
        override func cursorUpdate(with event: NSEvent) { hover(event) }
        override func mouseExited(with event: NSEvent) {
            pointer = nil; pointerMoved(nil)
            if press == nil || press == .cancelled { releaseCursor() }
        }
        private func hover(_ event: NSEvent) {
            guard enabled, window?.isKeyWindow == true, !isHiddenOrHasHiddenAncestor else { return }
            let point = convert(event.locationInWindow, from: nil)
            guard bounds.contains(point), visibleRect.contains(point) else { return }
            pointer = point; pointerMoved(point); refreshCursor()
        }
        func refreshCursor() {
            guard enabled, window?.isKeyWindow == true, window?.isVisible == true,
                  !isHiddenOrHasHiddenAncestor else { releaseCursor(); return }
            if let pressCursor, press == .editing || press == .panning {
                pressCursor.set(); ownsCursor = true
            } else if let pointer, bounds.contains(pointer), visibleRect.contains(pointer) {
                (space ? NSCursor.openHand : cursor(pointer)).set(); ownsCursor = true
            } else { releaseCursor() }
        }
        private func releaseCursor() {
            guard ownsCursor else { return }
            ownsCursor = false
            if window?.isKeyWindow == true, window?.isVisible == true, !isHiddenOrHasHiddenAncestor {
                NSCursor.arrow.set()
            }
            window?.invalidateCursorRects(for: self)
        }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow(); stop()
            guard let window else { return }
            resignKeyObserver = NotificationCenter.default.addObserver(
                forName: NSWindow.didResignKeyNotification, object: window, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.cancel(); self?.space = false; self?.pointer = nil; self?.releaseCursor() }
            }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel, .magnify, .keyDown, .keyUp,
                .leftMouseDown, .leftMouseDragged, .leftMouseUp, .rightMouseDown]) { [weak self] event in
                guard let self else { return event }
                return self.receive(event)
            }
        }
        func setEnabled(_ value: Bool) {
            if !value { cancel(); space = false; pointer = nil; releaseCursor() }
            enabled = value
        }
        func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            if let resignKeyObserver { NotificationCenter.default.removeObserver(resignKeyObserver) }
            monitor = nil; resignKeyObserver = nil
            cancel(); press = nil; space = false; pointer = nil; releaseCursor()
        }
        private func cancel() {
            switch press {
            case .editing: press = .cancelled; pressCancelled?()
            case .panning: press = .cancelled; ended?()
            default: break
            }
            pressCursor = nil
        }
        private func startNavigation() {
            cancel() // Must precede the model's navigationStarted callback.
            panStarted?()
        }
        func receive(_ event: NSEvent) -> NSEvent? {
            guard let window, event.window === window else { return event }
            // A fresh down ends the previous sequence even outside the canvas
            // or when control-click passes through to a native control.
            if event.type == .leftMouseDown, press != nil { cancel(); press = nil }
            if !enabled || !window.isKeyWindow || isHiddenOrHasHiddenAncestor || (space && !acceptsKeyboard()) {
                cancel(); space = false; releaseCursor()
            }
            // Retain the tail of our own press even outside the canvas, after
            // cancellation/disable, or while this window is no longer key.
            if event.type == .leftMouseUp, let active = press {
                let p = convert(event.locationInWindow, from: nil)
                press = nil; pressCursor = nil
                if active == .editing { pressEnded?(p) }
                if active == .panning { ended?() }
                pointer = bounds.contains(p) && visibleRect.contains(p) ? p : nil
                refreshCursor(); return nil
            }
            if event.type == .leftMouseDragged, let active = press {
                if active == .cancelled { pointer = nil; releaseCursor(); return nil }
                let p = convert(event.locationInWindow, from: nil)
                if active == .editing { pressChanged?(p) }
                if active == .panning { navigate?(1, .init(width: event.deltaX, height: event.deltaY), p) }
                refreshCursor(); return nil
            }
            guard enabled, window.isKeyWindow, !isHiddenOrHasHiddenAncestor else { return event }
            let p = convert(event.locationInWindow, from: nil)
            let inside = bounds.contains(p) && visibleRect.contains(p)
            switch event.type {
            case .scrollWheel where inside:
                startNavigation()
                if event.modifierFlags.contains(.command) {
                    navigate?(pow(0.999, event.scrollingDeltaY), .zero, p)
                } else {
                    let shift = event.modifierFlags.contains(.shift)
                    navigate?(1, .init(width: shift ? event.scrollingDeltaY : event.scrollingDeltaX,
                                       height: shift ? 0 : event.scrollingDeltaY), p)
                }
                ended?(); refreshCursor(); return nil
            case .magnify where inside:
                startNavigation(); navigate?(1 + event.magnification, .zero, p); ended?(); refreshCursor(); return nil
            case .keyDown where inside && event.charactersIgnoringModifiers == " ":
                guard acceptsKeyboard(), !(window.firstResponder is NSTextView) else { return event }
                if !space { cancel(); space = true }
                refreshCursor(); return nil
            case .keyUp where space && event.charactersIgnoringModifiers == " ":
                space = false; if press == .panning { cancel() }; refreshCursor(); return nil
            case .rightMouseDown where inside:
                contextPoint(p); return event
            case .leftMouseDown where inside:
                if event.modifierFlags.contains(.control) { contextPoint(p); return event }
                pointer = p
                if space {
                    startNavigation(); press = .panning; pressCursor = .closedHand
                } else if let pressBegan {
                    press = .editing
                    pressBegan(p, !event.modifierFlags.intersection([.shift, .command]).isEmpty, event.clickCount)
                    pressCursor = cursor(p)
                } else { return event }
                refreshCursor(); return nil
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
    var acceptsKeyboard: () -> Bool = { true }
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
