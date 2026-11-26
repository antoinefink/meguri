#if os(macOS)
import AppKit
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct CanvasNavigationInputTests {
    @Test(arguments: [false, true])
    func resigningKeyClearsSpaceEvenWithoutKeyUp(dragging: Bool) throws {
        // Never order or activate a window, or post input to the application.
        _ = NSApplication.shared
        let window = NavigationWindow(contentRect: .init(x: 0, y: 0, width: 200, height: 200),
                                      styleMask: [.titled], backing: .buffered, defer: false)
        let other = NavigationWindow(contentRect: .zero, styleMask: [], backing: .buffered, defer: false)
        let view = CanvasNavigationInput.NavigationView(frame: .init(x: 0, y: 0, width: 200, height: 200))
        window.contentView = view
        defer { view.stop(); window.contentView = nil }
        var starts = 0, ends = 0, moves = 0
        view.panStarted = { starts += 1 }; view.ended = { ends += 1 }
        view.navigate = { _, _, _ in moves += 1 }
        let point = view.convert(NSPoint(x: 30, y: 30), to: nil)
        func mouse(_ type: NSEvent.EventType) throws -> NSEvent {
            try #require(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
        }
        let space = try #require(NSEvent.keyEvent(with: .keyDown, location: point, modifierFlags: [], timestamp: 0,
            windowNumber: window.windowNumber, context: nil, characters: " ", charactersIgnoringModifiers: " ",
            isARepeat: false, keyCode: 49))
        #expect(view.receive(space) == nil)
        NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: other)
        let down = try mouse(.leftMouseDown)
        if dragging {
            #expect(view.receive(down) == nil)
            #expect(starts == 1)
        }
        window.key = false
        NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: window)
        #expect(ends == (dragging ? 1 : 0))
        window.key = true
        // Key-up happened elsewhere. Returning with an ordinary drag must
        // pass through rather than pan or finish the previous gesture again.
        #expect(view.receive(down) === down)
        let drag = try mouse(.leftMouseDragged), up = try mouse(.leftMouseUp)
        #expect(view.receive(drag) === drag && view.receive(up) === up)
        #expect(moves == 0 && starts == (dragging ? 1 : 0))
        #expect(ends == (dragging ? 1 : 0))
        // A newly pressed Space still starts and finishes one normal pan.
        #expect(view.receive(space) == nil && view.receive(down) == nil)
        #expect(view.receive(drag) == nil && view.receive(up) == nil)
        #expect(moves == 1 && starts == (dragging ? 2 : 1) && ends == (dragging ? 2 : 1))
        view.stop()
        #expect(ends == (dragging ? 2 : 1))
    }

    private final class NavigationWindow: NSWindow {
        var key = true
        override var isKeyWindow: Bool { key }
    }
}
#endif
