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
        // Space autorepeat during a held pan must not cancel its owned drag.
        #expect(view.receive(space) == nil)
        #expect(view.receive(drag) == nil && view.receive(up) == nil)
        #expect(moves == 1 && starts == (dragging ? 2 : 1) && ends == (dragging ? 2 : 1))
        view.stop()
        #expect(ends == (dragging ? 2 : 1))
    }

    @Test func nativeDownSelectsBeforeUpAndOnlyOwnedSequenceIsConsumed() throws {
        let fixture = NativeFixture()
        defer { fixture.stop() }
        let original = fixture.session.document, field = fixture.session.field
        let id = try #require(original.shapes.first?.id)
        let point = CGPoint(x: 200, y: 150)
        let down = try fixture.mouse(.leftMouseDown, at: point)
        #expect(fixture.view.receive(down) == nil)
        // This is deliberately before delivering any drag/up callback.
        #expect(fixture.session.selection == [id] && fixture.focuses == 1)
        #expect(fixture.session.document == original && fixture.session.field == field)
        #expect(!fixture.session.canUndo && fixture.writes == 0 && fixture.session.preview == nil)
        #expect(fixture.interaction.nativeCue == .closedHand)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseUp, at: point)) == nil)
        #expect(fixture.writes == 0 && !fixture.session.canUndo)
        let outside = try fixture.mouse(.leftMouseDown, at: .init(x: 1100, y: 1100))
        #expect(fixture.view.receive(outside) === outside)
        let foreign = try fixture.mouse(.leftMouseDown, at: point, otherWindow: true)
        #expect(fixture.view.receive(foreign) === foreign)
        // Only a press that began here owns drag/up outside our visible rect.
        #expect(fixture.view.receive(down) == nil)
        let drag = try fixture.mouse(.leftMouseDragged, at: .init(x: 1100, y: 1100))
        #expect(fixture.view.receive(drag) == nil && fixture.session.preview != nil)
        #expect(fixture.focuses == 2 && fixture.writes == 0)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseUp, at: .init(x: 1100, y: 1100))) == nil)
        #expect(fixture.writes == 1 && fixture.session.canUndo)
        #expect(fixture.view.receive(drag) === drag)
    }

    @Test(arguments: ["navigation", "disable", "resign", "stop"])
    func cancelledNativePressCannotWriteFromLateEvents(reason: String) throws {
        let fixture = NativeFixture()
        defer { fixture.stop() }
        let point = CGPoint(x: 200, y: 150), end = CGPoint(x: 230, y: 180)
        let original = fixture.session.document
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseDown, at: point)) == nil)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseDragged, at: end)) == nil)
        #expect(fixture.session.preview != nil)
        switch reason {
        case "navigation":
            let space = try #require(NSEvent.keyEvent(with: .keyDown, location: point, modifierFlags: [], timestamp: 0,
                windowNumber: fixture.window.windowNumber, context: nil, characters: " ", charactersIgnoringModifiers: " ",
                isARepeat: false, keyCode: 49))
            #expect(fixture.view.receive(space) == nil)
        case "disable": fixture.view.setEnabled(false)
        case "resign":
            fixture.window.key = false
            NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: fixture.window)
        default: fixture.view.stop()
        }
        #expect(fixture.session.preview == nil && fixture.session.selection.isEmpty)
        let drag = try fixture.mouse(.leftMouseDragged, at: end), up = try fixture.mouse(.leftMouseUp, at: end)
        if reason == "stop" {
            #expect(fixture.view.receive(drag) === drag && fixture.view.receive(up) === up)
        } else {
            #expect(fixture.view.receive(drag) == nil && fixture.view.receive(up) == nil)
        }
        #expect(fixture.session.document == original && fixture.writes == 0 && !fixture.session.canUndo)
        fixture.window.key = true
        fixture.view.setEnabled(false); fixture.view.setEnabled(true)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseDown, at: point)) == nil)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseDragged, at: end)) == nil)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseUp, at: end)) == nil)
        #expect(fixture.writes == 1 && fixture.session.canUndo)
    }

    @Test func nativeInputKeepsControlClickRightClickAndClickCount() throws {
        let fixture = NativeFixture()
        defer { fixture.stop() }
        let point = CGPoint(x: 200, y: 150)
        var contextPoints: [CGPoint] = [], counts: [Int] = []
        fixture.view.contextPoint = { contextPoints.append($0) }
        for type in [NSEvent.EventType.leftMouseDown, .rightMouseDown] {
            let event = try fixture.mouse(type, at: point, modifiers: type == .leftMouseDown ? [.control] : [])
            #expect(fixture.view.receive(event) === event)
        }
        #expect(contextPoints == [point, point] && fixture.focuses == 0 && fixture.session.selection.isEmpty)
        fixture.view.pressBegan = { _, _, count in counts.append(count) }
        for count in [1, 2] {
            #expect(fixture.view.receive(try fixture.mouse(.leftMouseDown, at: point, count: count)) == nil)
            #expect(fixture.view.receive(try fixture.mouse(.leftMouseUp, at: point, count: count)) == nil)
        }
        #expect(counts == [1, 2])
    }

    @Test(arguments: [false, true])
    func freshExternalDownReleasesCancelledPressWithMissingTail(controlClick: Bool) throws {
        let fixture = NativeFixture()
        defer { fixture.stop() }
        let point = CGPoint(x: 200, y: 150)
        #expect(fixture.view.receive(try fixture.mouse(.leftMouseDown, at: point)) == nil)
        fixture.window.key = false
        NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: fixture.window)
        let foreignUp = try fixture.mouse(.leftMouseUp, at: point, otherWindow: true)
        #expect(fixture.view.receive(foreignUp) === foreignUp)
        fixture.window.key = true
        let external = controlClick ? point : CGPoint(x: 1100, y: 1100)
        let modifiers: NSEvent.ModifierFlags = controlClick ? [.control] : []
        let down = try fixture.mouse(.leftMouseDown, at: external, modifiers: modifiers)
        let up = try fixture.mouse(.leftMouseUp, at: external, modifiers: modifiers)
        #expect(fixture.view.receive(down) === down)
        #expect(fixture.view.receive(up) === up)
        #expect(fixture.focuses == 1 && fixture.writes == 0 && !fixture.session.canUndo)
    }

    @Test func offscreenHoverAndLeaveDeliverPointerWithoutRoutingCursor() throws {
        let fixture = NativeFixture()
        defer { fixture.stop() }
        #expect(fixture.window.isKeyWindow && !fixture.window.isVisible)
        var points: [CGPoint?] = []
        var cursorRequests = 0
        fixture.view.pointerMoved = { points.append($0) }
        fixture.view.cursor = { _ in cursorRequests += 1; return .openHand }
        let point = CGPoint(x: 200, y: 150)
        let event = try fixture.mouse(.mouseMoved, at: point)
        fixture.view.mouseMoved(with: event)
        #expect(points == [point])
        fixture.view.refreshCursor()
        fixture.view.mouseExited(with: event)
        #expect(points == [point, nil] && cursorRequests == 0)
    }

    /// Direct dispatch to the production receiver; never post events, activate
    /// a window, run input synthesis or touch the workstation's input.
    @MainActor private final class NativeFixture {
        let window: NavigationWindow
        let other: NavigationWindow
        let view: CanvasNavigationInput.NavigationView
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        let interaction = MaskCanvasInteraction()
        var focuses = 0
        var writes = 0
        init() {
            _ = NSApplication.shared
            window = NavigationWindow(contentRect: .init(x: 0, y: 0, width: 1000, height: 1000),
                styleMask: [.titled], backing: .buffered, defer: false)
            other = NavigationWindow(contentRect: .zero, styleMask: [], backing: .buffered, defer: false)
            view = CanvasNavigationInput.NavigationView(frame: .init(x: 0, y: 0, width: 1000, height: 1000))
            let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
            view.pressBegan = { [unowned self] point, extending, _ in
                focuses += 1
                interaction.nativeBegan(at: point, viewport: viewport, extending: extending, session: session)
            }
            view.pressChanged = { [unowned self] in interaction.nativeChanged(at: $0, viewport: viewport, session: session) }
            view.pressEnded = { [unowned self] in
                _ = interaction.nativeEnded(at: $0, viewport: viewport, fill: "#000000", session: session)
            }
            view.pressCancelled = { [unowned self] in interaction.nativeCancelled(session: session) }
            view.panStarted = { [unowned self] in interaction.navigationStarted(session: session) }
            view.ended = { [unowned self] in interaction.navigationEnded() }
            session.onWrite = { [unowned self] _ in writes += 1 }
            window.contentView = view
        }
        func stop() { view.stop(); window.contentView = nil }
        func mouse(_ type: NSEvent.EventType, at point: CGPoint, modifiers: NSEvent.ModifierFlags = [],
                   count: Int = 1, otherWindow: Bool = false) throws -> NSEvent {
            try #require(NSEvent.mouseEvent(with: type, location: view.convert(point, to: nil), modifierFlags: modifiers,
                timestamp: 0, windowNumber: (otherWindow ? other : window).windowNumber, context: nil,
                eventNumber: 1, clickCount: count, pressure: 1))
        }
    }

    private final class NavigationWindow: NSWindow {
        var key = true
        override var isKeyWindow: Bool { key }
    }
}
#endif
