#if os(macOS)
import AppKit
import AppModel
import DesignSystem
import SwiftUI

/// All Mac Add entry points retain the same collection-owned draft and runtime.
/// Only its presentation moves out of the workspace; close still goes through
/// the held-flush/discard guard, and Add still leaves the window open.
public struct AddNoteWindowPresentation: ViewModifier {
    let collection: ReviewCollection?
    let enabled: Bool
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @Environment(\.appClock) private var clock
    public init(collection: ReviewCollection?, enabled: Bool) {
        self.collection = collection; self.enabled = enabled
    }
    public func body(content: Content) -> some View {
        content.onChange(of: collection?.addNotePresentation.windowRequest, initial: true) { _, _ in
            guard enabled, let collection, case .add = collection.navigation.sheet,
                  collection.addNoteFlow.isPresented else { return }
            AddNoteWindow.show(AddNoteView(collection: collection)
                .environment(\.locale, locale).environment(\.timeZone, timeZone).environment(\.appClock, clock),
                collection: collection, over: NSApp.mainWindow)
        }
    }
}

@MainActor
final class AddNoteWindow: NSObject, NSWindowDelegate {
    private(set) static var current: AddNoteWindow?
    private let window: NSWindow
    // The collection outlives the native window, including an asynchronous close.
    private let collection: ReviewCollection
    private weak var parent: NSWindow?
    private var closing = false

    static func show(_ form: some View, collection: ReviewCollection, over parent: NSWindow?) {
        if let current, current.collection === collection {
            // The workspace may have closed and reopened while Add stayed open.
            if let parent, parent.isVisible { current.parent = parent }
            current.window.makeKeyAndOrderFront(nil)
            return
        }
        // A collection transition drains its editor before replacing its owner.
        guard current == nil else { return }
        current = AddNoteWindow(form: form, collection: collection, over: parent)
    }
    private init(form: some View, collection: ReviewCollection, over parent: NSWindow?) {
        self.collection = collection; self.parent = parent
        let controller = NSHostingController(rootView: AnyView(EmptyView()))
        window = AddNoteKeyWindow(contentViewController: controller)
        super.init()
        controller.rootView = AnyView(form.modifier(AddNoteWindowLifetime(flow: collection.addNoteFlow) { [weak self] in
            self?.close()
        }))
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.title = String(localized: "Add Note", bundle: .module)
        window.identifier = NSUserInterfaceItemIdentifier("editor.add.window")
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.delegate = self
        if !window.setFrameUsingName("MeguriAddNote") {
            window.setContentSize(controller.view.fittingSize)
            if let parent {
                window.setFrameOrigin(NSPoint(x: parent.frame.midX - window.frame.width / 2,
                                              y: parent.frame.midY - window.frame.height / 2))
            } else { window.center() }
        }
        window.setFrameAutosaveName("MeguriAddNote")
        window.makeKeyAndOrderFront(nil)
    }
    private func close() {
        guard !closing else { return }
        closing = true
        window.close()
        if Self.current === self { Self.current = nil }
        // Closing Add must not reopen a workspace the person already closed.
        if let parent, parent.isVisible { parent.makeKeyAndOrderFront(nil) }
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if closing || !collection.addNoteFlow.isPresented { return true }
        collection.navigation.dismissSheet()
        return false
    }
    func windowWillClose(_ notification: Notification) {
        closing = true
        if Self.current === self { Self.current = nil }
    }
}

private final class AddNoteKeyWindow: NSWindow {
    // Add takes the keyboard; the workspace remains the main document window.
    override var canBecomeMain: Bool { false }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
           event.charactersIgnoringModifiers == "w" {
            if !event.isARepeat { performClose(nil) }
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

/// Read the observable state during SwiftUI evaluation, not once in the
/// NSWindow owner's initializer. A child form update cannot replace a captured
/// onChange value, including after Add hands its page to a fresh draft.
private struct AddNoteWindowLifetime: ViewModifier {
    let flow: AddNoteFlow
    let close: () -> Void

    func body(content: Content) -> some View {
        content.onChange(of: flow.isPresented, initial: true) { _, presented in
            if !presented { close() }
        }
    }
}
#endif
