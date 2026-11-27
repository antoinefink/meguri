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
    private var clickMonitor: Any?

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
        // The press ended without this view's commit: it was cancelled.
        if intent != nil, next == nil { MacSidebarNativePress.restore() }
        stop()
        intent = next
        guard next != nil, window != nil else { return }
        let link = displayLink(target: self, selector: #selector(frame(_:)))
        self.link = link
        // Common modes keep native input/menu tracking from bypassing the fence.
        link.add(to: .main, forMode: .common)
        backMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            let used = MainActor.assumeIsolated { () -> Bool in
                guard let self, event.window === self.window,
                      self.navigation.sidebarInteractionPending,
                      event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
                      event.charactersIgnoringModifiers == "[" else { return false }
                self.navigation.cancelMacSidebarSelection()
                MacSidebarNativePress.restore()
                self.stop()
                return true
            }
            return used ? nil : event
        }
        // The former destination stays drawn, and enabled, until the commit:
        // disabling it would redraw the whole outgoing page in the row's frame.
        // A click outside the sidebar meanwhile (one queued behind a busy main
        // thread) must not act on it.
        clickMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            let stale = MainActor.assumeIsolated { () -> Bool in
                guard let self, let window = self.window, event.window === window,
                      self.navigation.sidebarInteractionPending else { return false }
                // This view is the sidebar column's background: its frame is the column's.
                return !self.convert(self.bounds, to: nil).contains(event.locationInWindow)
            }
            return stale ? nil : event
        }
    }

    func stop() {
        link?.invalidate()
        link = nil
        if let backMonitor { NSEvent.removeMonitor(backMonitor) }
        backMonitor = nil
        if let clickMonitor { NSEvent.removeMonitor(clickMonitor) }
        clickMonitor = nil
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
            MacSidebarNativePress.restore()
            stop()
            return
        }
        if let submittedFrame {
            // A display callback is a pre-display callback. Starting expensive
            // work in the first one would still starve the selection's frame.
            guard committed, link.timestamp >= submittedFrame else { return }
            stop()
            let item = navigation.macSidebarSelection
            navigation.commitMacSidebarSelection(intent)
            // Leaving Browse replaces its list; Browse's own row keeps it, as it was.
            if item == .section(.browse) { MacSidebarNativePress.restore() } else { MacSidebarNativePress.forget() }
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

/// Browse's sidebar list draws a pressed section row's selection itself, in
/// AppKit, in the press's own frame. Its SwiftUI selection changes only with
/// the route: a change there updates every one of its filter rows (a large
/// collection's thousands of tags), which held the row's highlight back by
/// up to a quarter of a second. Leaving Browse replaces that list; a press
/// that is cancelled, or that ends on Browse's own row, puts back the rows
/// the list showed, unless something else selected rows in it meanwhile.
@MainActor enum MacSidebarNativePress {
    private static weak var table: NSTableView?
    private static var saved: IndexSet?
    private static var shown: IndexSet?

    /// Selects the row under the current mouse-down in its list.
    static func selectPressedRow() {
        guard let event = NSApp.currentEvent, event.type == .leftMouseDown, let window = event.window,
              let frame = window.contentView?.superview ?? window.contentView,
              var view = frame.hitTest(event.locationInWindow) else { return }
        while !(view is NSTableView), let parent = view.superview { view = parent }
        guard let table = view as? NSTableView else { return }
        let row = table.row(at: table.convert(event.locationInWindow, from: nil))
        guard row >= 0 else { return }
        // A second press before the first's commit keeps the rows from before the first.
        if self.table !== table || saved == nil {
            self.table = table
            saved = table.selectedRowIndexes
        }
        let rows = IndexSet(integer: row)
        shown = rows
        table.selectRowIndexes(rows, byExtendingSelection: false)
    }

    static func forget() {
        table = nil
        saved = nil
        shown = nil
    }

    static func restore() {
        defer { forget() }
        guard let table, let saved, let shown, table.selectedRowIndexes == shown else { return }
        table.selectRowIndexes(saved, byExtendingSelection: false)
    }
}
#endif
