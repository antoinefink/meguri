#if os(macOS)
import AppKit
@testable import AppModel
import Testing

/// N24.4's Return exception and H12's guards: a shortcut modifier, a text input, a sheet,
/// a review and a key's repeat keep Return from starting study.
@Suite struct SidebarReturnKeyTests {
    private func acts(keyCode: UInt16 = 36, isRepeat: Bool = false, modifiers: NSEvent.ModifierFlags = [],
                      inMainWindow: Bool = true, windowSheet: Bool = false, appSheet: Bool = false,
                      review: Bool = false, textInput: Bool = false) -> Bool {
        SidebarReturnKey.acts(keyCode: keyCode, isRepeat: isRepeat, modifiers: modifiers, inMainWindow: inMainWindow,
                              windowSheet: windowSheet, appSheet: appSheet, review: review, textInput: textInput)
    }

    @Test func returnAndEnterAct() {
        #expect(acts())
        #expect(acts(keyCode: 76))
        #expect(!acts(keyCode: 1))
    }

    /// Caps Lock, the keypad's flag and Fn leave Return plain.
    @Test func capsLockAndKeypadFlagsStillAct() {
        #expect(acts(modifiers: .capsLock))
        #expect(acts(keyCode: 76, modifiers: [.numericPad, .function]))
        #expect(acts(modifiers: [.capsLock, .numericPad]))
    }

    @Test(arguments: [NSEvent.ModifierFlags.command, .option, .control, .shift])
    func aShortcutModifierKeepsTheKey(modifier: NSEvent.ModifierFlags) {
        #expect(!acts(modifiers: modifier))
        #expect(!acts(modifiers: [modifier, .capsLock]))
    }

    @Test func everyGuardKeepsTheKey() {
        #expect(!acts(isRepeat: true))
        #expect(!acts(inMainWindow: false))
        #expect(!acts(windowSheet: true))
        #expect(!acts(appSheet: true))
        #expect(!acts(review: true))
        #expect(!acts(textInput: true))
    }
}
#endif
