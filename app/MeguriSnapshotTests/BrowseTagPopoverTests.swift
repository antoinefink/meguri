#if os(macOS)
import AppKit
@testable import Browse
import SwiftUI
import Testing

/// App-hosted VM regression. Queue refreshes on both sides of a same-window
/// whitespace click without yielding, so their ordering is deterministic.
@MainActor @Suite(.serialized)
struct BrowseTagPopoverTests {
    @Test(.timeLimit(.minutes(1)))
    func outsideWhitespaceDismissalSurvivesQueuedRefreshes() async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let names = ["Spanish::Grammar::Nouns", "Spanish::Grammar::Verbs"]
            var committed: [[String]] = []
            let root = BrowseTagField(tags: [], names: names) { committed.append($0) }
            let hosting = NSHostingView(rootView: root)
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 300),
                                  styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let content = NSView(frame: NSRect(x: 0, y: 0, width: 500, height: 300))
            hosting.frame = NSRect(x: 20, y: 80, width: 360, height: 80)
            content.addSubview(hosting)
            window.contentView = content
            defer {
                window.endEditing(for: nil)
                window.contentView = nil
                window.close()
                withExtendedLifetime(hosting) {}
            }
            window.orderFront(nil)
            hosting.layoutSubtreeIfNeeded()
            let field = try #require(tokenField(in: hosting))
            let coordinator = try #require(field.delegate as? BrowseTagField.Coordinator)
            defer { coordinator.stopEditing() }
            field.selectText(nil)
            let editor = try #require(field.currentEditor() as? NSTextView)
            editor.insertText("spanish::grammar::", replacementRange: editor.selectedRange())
            coordinator.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field))
            await drainRefreshes()
            try #require(coordinator.popover.isShown)
            #expect(window.firstResponder === editor)
            #expect(coordinator.control(field, textView: editor, doCommandBy: #selector(NSResponder.moveDown(_:))))
            let before = editor.string

            coordinator.scheduleRefresh()
            let outside = try mouseDown(in: window, at: NSPoint(x: 440, y: 250))
            // Exercise exactly the handler used by the local NSEvent monitor.
            // This blank NSView neither ends editing nor changes first responder.
            coordinator.dismissIfOutside(outside)
            #expect(!coordinator.popover.isShown)
            coordinator.parent = BrowseTagField(tags: [], names: names + ["Spanish::Grammar::Tenses"]) { committed.append($0) }
            coordinator.scheduleRefresh()
            await drainRefreshes()
            #expect(!coordinator.popover.isShown)
            #expect(editor.string == before)
            #expect(committed.isEmpty)
            #expect(window.firstResponder === editor)

            // A real edit starts a fresh query; a click inside the field must
            // pass through without closing it or accepting a candidate.
            editor.insertText("VER", replacementRange: NSRange(location: 0, length: (editor.string as NSString).length))
            coordinator.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field))
            await drainRefreshes()
            try #require(coordinator.popover.isShown)
            let inside = field.convert(NSPoint(x: field.bounds.midX, y: field.bounds.midY), to: nil)
            coordinator.dismissIfOutside(try mouseDown(in: window, at: inside))
            #expect(coordinator.popover.isShown)
            #expect(window.firstResponder === editor)
            #expect(coordinator.control(field, textView: editor, doCommandBy: #selector(NSResponder.moveDown(_:))))
            #expect(coordinator.control(field, textView: editor, doCommandBy: #selector(NSResponder.insertTab(_:))))
            await drainRefreshes()
            #expect(!coordinator.popover.isShown)
            #expect((field.objectValue as? [String]) == [names[1]])
            let resumed = try #require(field.currentEditor() as? NSTextView)
            #expect(window.firstResponder === resumed, "accepting Tab keeps typing in Tags")
            resumed.insertText("Next", replacementRange: resumed.selectedRange())
            #expect(resumed.string.contains("Next"))
        }
    }

    private func tokenField(in view: NSView) -> NSTokenField? {
        if let field = view as? NSTokenField { return field }
        for child in view.subviews {
            if let field = tokenField(in: child) { return field }
        }
        return nil
    }

    private func mouseDown(in window: NSWindow, at point: NSPoint) throws -> NSEvent {
        try #require(NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
    }

    private func drainRefreshes() async {
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
    }
}
#endif
