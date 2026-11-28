#if os(macOS)
import AppKit
import Testing
@testable import Review

@MainActor @Suite struct ReviewTerminalKeyboardTests {
    private func key(_ input: String, at timestamp: Double = 101, repeated: Bool = false) throws -> NSEvent {
        try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: timestamp,
            windowNumber: 0, context: nil, characters: input, charactersIgnoringModifiers: input,
            isARepeat: repeated, keyCode: input == " " ? 49 : 36))
    }

    @Test func explicitlyFocusedContinueKeepsFreshNativeSpaceButConsumesHeldGrade() throws {
        let owner = ReviewTerminalKeyboard.Coordinator(presentedAt: 100)
        owner.enabled = true
        owner.continueFocused = true
        var finished = 0
        owner.done = { finished += 1 }
        let space = try key(" ")
        #expect(owner.route(space) === space, "Native Continue activation keeps the original event")
        #expect(owner.route(try key(" ", repeated: true)) == nil)
        #expect(owner.route(try key(" ", at: 100)) == nil, "The grade that created the summary is consumed")
        #expect(finished == 0)
        // Moving focus back to content restores Done; native Space did not
        // consume Done's one-shot admission.
        owner.continueFocused = false
        #expect(owner.route(try key(" ", at: 102)) == nil)
        #expect(finished == 1)
    }

    @Test(arguments: [" ", "\r", "\n", "\u{3}"])
    func contentKeysFinishOnceAndRepeatsNeverFinish(input: String) throws {
        let owner = ReviewTerminalKeyboard.Coordinator(presentedAt: 100)
        owner.enabled = true
        var finished = 0
        owner.done = { finished += 1 }
        #expect(owner.route(try key(input, repeated: true)) == nil)
        #expect(owner.route(try key(input, at: 100)) == nil)
        #expect(finished == 0)
        #expect(owner.route(try key(input)) == nil)
        #expect(owner.route(try key(input, at: 102)) == nil)
        #expect(finished == 1)
    }

    @Test(arguments: ["\r", "\n", "\u{3}"])
    func continueFocusOnlyYieldsSpace(input: String) throws {
        let owner = ReviewTerminalKeyboard.Coordinator(presentedAt: 100)
        owner.enabled = true
        owner.continueFocused = true
        var finished = 0
        owner.done = { finished += 1 }
        #expect(owner.route(try key(input)) == nil)
        #expect(finished == 1)
    }
}
#endif
