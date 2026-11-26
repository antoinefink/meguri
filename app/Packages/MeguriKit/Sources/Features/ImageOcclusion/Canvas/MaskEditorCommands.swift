import SwiftUI

/// M7.1 registration, exposed to the host's shortcut conflict check and Edit
/// menu routing. All dispatch goes through the visible editor's enabled guards.
public enum MaskEditorCommand: String, CaseIterable {
    case select, rectangle, ellipse, polygon, text, fill
    case undo, redo, selectAll, delete, duplicate, group, ungroup
    case zoomOut, zoomIn, zoomToFit, translucent, closePolygon
    case alignLeft, alignCenter, alignRight, alignTop, alignMiddle, alignBottom

    public static let context = "imageOcclusion.maskEditor"
    var tool: MaskEditorTool? { MaskEditorTool(rawValue: rawValue) }
    var alignment: MaskAlignment? {
        switch self {
        case .alignLeft: .left
        case .alignCenter: .center
        case .alignRight: .right
        case .alignTop: .top
        case .alignMiddle: .middle
        case .alignBottom: .bottom
        default: nil
        }
    }
    var key: String { tool?.key ?? "io.command.\(rawValue)" }

    /// Alternate bindings are registered once alongside their primary command.
    public static var bindings: [MaskEditorKeyBinding] {
        let plain: [(Self, String)] = [
            (.select, "s"), (.rectangle, "r"), (.ellipse, "e"), (.polygon, "p"), (.text, "t"), (.fill, "c"),
            (.selectAll, "a"), (.delete, "\u{7f}"), (.duplicate, "d"), (.group, "g"), (.ungroup, "u"),
            (.zoomOut, "["), (.zoomIn, "]"), (.zoomToFit, "f"), (.translucent, "l"), (.closePolygon, "\r"),
        ]
        let shifted: [(Self, String)] = [
            (.alignLeft, "l"), (.alignCenter, "h"), (.alignRight, "r"),
            (.alignTop, "t"), (.alignMiddle, "v"), (.alignBottom, "b"),
        ]
        return plain.map { .init(command: $0.0, character: $0.1, modifiers: []) }
            + shifted.map { .init(command: $0.0, character: $0.1, modifiers: .shift) }
            + [
                .init(command: .undo, character: "z", modifiers: .command),
                .init(command: .redo, character: "z", modifiers: [.command, .shift]),
                .init(command: .redo, character: "y", modifiers: .command),
                .init(command: .selectAll, character: "a", modifiers: .command),
                .init(command: .delete, character: "\u{f728}", modifiers: []),
            ]
    }
}

public struct MaskEditorKeyBinding {
    public let command: MaskEditorCommand
    public let character: String
    public let modifiers: EventModifiers
    var shortcut: KeyboardShortcut { .init(KeyEquivalent(Character(character)), modifiers: modifiers) }
}

/// The focused editor only. The host should prefer its enabled Undo/Redo over
/// collection history, falling through when disabled. A text input keeps its
/// own responder undo; no window-wide registration is used here.
public struct MaskEditorCommandContext {
    private let enabled: (MaskEditorCommand) -> Bool
    private let action: (MaskEditorCommand) -> Void
    init(enabled: @escaping (MaskEditorCommand) -> Bool, action: @escaping (MaskEditorCommand) -> Void) {
        self.enabled = enabled; self.action = action
    }
    public func isEnabled(_ command: MaskEditorCommand) -> Bool { enabled(command) }
    public func perform(_ command: MaskEditorCommand) {
        guard enabled(command) else { return }
        action(command)
    }
}

private struct MaskEditorCommandKey: FocusedValueKey { typealias Value = MaskEditorCommandContext }
extension FocusedValues {
    public var maskEditorCommands: MaskEditorCommandContext? {
        get { self[MaskEditorCommandKey.self] }
        set { self[MaskEditorCommandKey.self] = newValue }
    }
}

/// Install once in the app scene. Focused values keep other windows, fields,
/// label sheets and popovers outside this command context.
public struct MaskEditorCommands: Commands {
    @FocusedValue(\.maskEditorCommands) private var target
    public init() {}
    public var body: some Commands {
        CommandMenu(ioString("io.commands")) {
            ForEach(MaskEditorCommand.allCases.filter { $0 != .undo && $0 != .redo }, id: \.self) { command in
                Button(ioString(command.key)) { target?.perform(command) }
                    .keyboardShortcut(MaskEditorCommand.bindings.first { $0.command == command }?.shortcut)
                    .disabled(target?.isEnabled(command) != true)
            }
        }
    }
}

#if os(macOS)
import AppKit

final class MaskEditorWindowScope { weak var window: NSWindow? }
struct MaskEditorWindowReader: NSViewRepresentable {
    let scope: MaskEditorWindowScope
    func makeNSView(context: Context) -> Reader { Reader(scope: scope) }
    func updateNSView(_ view: Reader, context: Context) {}
    final class Reader: NSView {
        let scope: MaskEditorWindowScope
        init(scope: MaskEditorWindowScope) { self.scope = scope; super.init(frame: .zero) }
        required init?(coder: NSCoder) { return nil }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); scope.window = window }
        override func isAccessibilityElement() -> Bool { false }
    }
}
#endif
