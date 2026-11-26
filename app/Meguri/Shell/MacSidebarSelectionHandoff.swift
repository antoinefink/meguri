#if os(macOS)
import AppKit
import AppModel
import QuartzCore
import SwiftUI

/// A window-local display fence, not a timer. SwiftUI first applies the native
/// List selection without changing the detail's identity. On a display callback
/// we submit that visual transaction; only a later display frame may navigate.
struct MacSidebarSelectionHandoff: NSViewRepresentable {
    let navigation: AppNavigation

    func makeNSView(context: Context) -> SelectionFrameView {
        SelectionFrameView(navigation: navigation)
    }

    func updateNSView(_ view: SelectionFrameView, context: Context) {
        view.request(navigation.macSidebarSelectionID)
    }

    static func dismantleNSView(_ view: SelectionFrameView, coordinator: ()) {
        view.stop()
    }
}

@MainActor final class SelectionFrameView: NSView {
    private let navigation: AppNavigation
    private var link: CADisplayLink?
    private var intent: UInt64?
    private var submittedFrame: CFTimeInterval?
    private var committed = false
    private var backMonitor: Any?

    init(navigation: AppNavigation) {
        self.navigation = navigation
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        stop()
        guard window != nil else {
            navigation.cancelMacSidebarSelection()
            return
        }
        request(navigation.macSidebarSelectionID)
    }

    func request(_ next: UInt64?) {
        guard next != intent || (next != nil && link == nil) else { return }
        stop()
        intent = next
        guard next != nil, window != nil else { return }
        let link = displayLink(target: self, selector: #selector(frame(_:)))
        self.link = link
        // Common modes keep native input/menu tracking from bypassing the fence.
        link.add(to: .main, forMode: .common)
        backMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            MainActor.assumeIsolated {
                guard let self, event.window === self.window,
                      self.navigation.sidebarInteractionPending,
                      event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
                      event.charactersIgnoringModifiers == "[" else { return event }
                self.navigation.cancelMacSidebarSelection()
                self.stop()
                return nil
            }
        }
    }

    func stop() {
        link?.invalidate()
        link = nil
        if let backMonitor { NSEvent.removeMonitor(backMonitor) }
        backMonitor = nil
        intent = nil
        submittedFrame = nil
        committed = false
    }

    @objc private func frame(_ link: CADisplayLink) {
        guard let intent, navigation.macSidebarSelectionID == intent,
              let window, window.isVisible else {
            if navigation.macSidebarSelectionID == intent {
                navigation.cancelMacSidebarSelection()
            }
            stop()
            return
        }
        if let submittedFrame {
            // A display callback is a pre-display callback. Starting expensive
            // work in the first one would still starve the selection's frame.
            guard committed, link.timestamp >= submittedFrame else { return }
            stop()
            navigation.commitMacSidebarSelection(intent)
            return
        }
        submittedFrame = link.targetTimestamp
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        CATransaction.setCompletionBlock { [weak self] in
            Task { @MainActor [weak self] in
                guard let self, self.intent == intent else { return }
                self.committed = true
            }
        }
        window.contentView?.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        CATransaction.commit()
        CATransaction.flush()
    }
}
#endif
