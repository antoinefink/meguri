#if os(macOS)
import AppKit

/// N24.4's exception, `decks/deck-list` H12: on the Mac a row's click leaves keyboard focus
/// with the row's own button, not the sidebar list, so neither the list nor its primary
/// action sees Return. The sidebar's window takes it instead, only when this says so.
public enum SidebarReturnKey {
    /// Return (36) and the keypad's Enter (76).
    public static let keyCodes: Set<UInt16> = [36, 76]

    /// The modifiers that make Return another shortcut. Caps Lock, the keypad and Fn
    /// leave it plain Return.
    public static let shortcutModifiers: NSEvent.ModifierFlags = [.command, .option, .control, .shift]

    /// Whether a key-down should start the selection's study: Return with no shortcut
    /// modifier, not a repeat, in the main window with no sheet, and nothing else owning
    /// the keys (a text input, a sheet of the app's own, a review).
    public static func acts(keyCode: UInt16, isRepeat: Bool, modifiers: NSEvent.ModifierFlags, inMainWindow: Bool,
                            windowSheet: Bool, appSheet: Bool, review: Bool, textInput: Bool) -> Bool {
        keyCodes.contains(keyCode) && !isRepeat && modifiers.isDisjoint(with: shortcutModifiers) && inMainWindow
            && !windowSheet && !appSheet && !review && !textInput
    }
}
#endif
