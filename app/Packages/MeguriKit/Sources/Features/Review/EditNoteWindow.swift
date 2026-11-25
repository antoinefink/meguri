#if os(macOS)
import AppKit
import AppModel
import SwiftUI

/// Edit Note's own window on the Mac (N20.8, F1, user-approved 2026-11-25).
/// One at a time. The review stays visible behind it and waits: its card
/// actions and Back to Decks hold while the flow is presented.
///
/// The window follows the flow, not the review view: it closes itself when
/// the flow ends (Save, Cancel, a discard). Its close button and ⌘W ask the
/// flow as Cancel does, so a changed draft still asks to keep or discard,
/// on this window, and the window stays until that is answered.
@MainActor
final class EditNoteWindow: NSObject, NSWindowDelegate {
    private(set) static var current: EditNoteWindow?
    private static let frameName = "MeguriEditNote"

    private let window: NSWindow
    private let flow: EditNoteFlow
    /// The review's window, which takes the keys back when this one closes.
    private weak var parent: NSWindow?
    private var closing = false

    /// Shows the flow's form in the window, or brings the open one forward.
    static func show(_ form: some View, flow: EditNoteFlow, over parent: NSWindow?) {
        if let current, current.flow === flow {
            current.window.makeKeyAndOrderFront(nil)
            return
        }
        current?.close()
        current = EditNoteWindow(form: form, flow: flow, over: parent)
    }

    private init(form: some View, flow: EditNoteFlow, over parent: NSWindow?) {
        self.flow = flow
        self.parent = parent
        let controller = NSHostingController(rootView: AnyView(EmptyView()))
        window = CloseKeyWindow(contentViewController: controller)
        super.init()
        controller.rootView = AnyView(form.modifier(CloseWhenFinished(flow: flow) { [weak self] in self?.close() }))
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.title = Self.title(flow.draft?.values.first)
        window.identifier = NSUserInterfaceItemIdentifier("review.editCurrent.window")
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.delegate = self
        if !window.setFrameUsingName(Self.frameName) { center(over: parent) }
        window.setFrameAutosaveName(Self.frameName)
        window.makeKeyAndOrderFront(nil)
    }

    /// The first time: the window's ideal size, centred on the review's window.
    private func center(over parent: NSWindow?) {
        window.setContentSize(window.contentViewController?.view.fittingSize ?? window.frame.size)
        guard let parent else { window.center(); return }
        let size = window.frame.size
        window.setFrameOrigin(NSPoint(x: parent.frame.midX - size.width / 2, y: parent.frame.midY - size.height / 2))
    }

    func close() {
        guard !closing else { return }
        closing = true
        window.close()
        if Self.current === self { Self.current = nil }
        // Main's close guard may have already continued its asynchronous close.
        if let parent, parent.isVisible { parent.makeKeyAndOrderFront(nil) }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if closing || !flow.isPresented { return true }
        Task { await flow.requestCancel() }
        return false
    }

    func windowWillClose(_ notification: Notification) {
        closing = true
        if Self.current === self { Self.current = nil }
    }

    /// The window names the note (HIG): its first field as plain text, else "Edit Note".
    static func title(_ html: String?) -> String {
        var text = html ?? ""
        text = text.replacingOccurrences(of: #"\[sound:[^\]]*\]"#, with: " ", options: .regularExpression)
        text = text.replacingOccurrences(of: "<[^>]*>", with: " ", options: .regularExpression)
        for (entity, character) in [("&nbsp;", " "), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'"), ("&amp;", "&")] {
            text = text.replacingOccurrences(of: entity, with: character)
        }
        text = text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        if text.isEmpty { return String(localized: "Edit Note", bundle: .module) }
        return text.count > 80 ? String(text.prefix(79)) + "…" : text
    }
}

/// ⌘W closes this window as its close button does (the app's File menu has
/// no Close item): the flow is asked first, so a change asks to be kept.
private final class CloseKeyWindow: NSWindow {
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
           event.charactersIgnoringModifiers == "w" {
            performClose(nil)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

/// Closes the window once the flow is no longer presented.
private struct CloseWhenFinished: ViewModifier {
    let flow: EditNoteFlow
    let close: () -> Void
    func body(content: Content) -> some View {
        content.onChange(of: flow.isPresented, initial: true) { _, presented in
            if !presented { close() }
        }
    }
}
#endif
