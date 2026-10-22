import CardRendering
import Testing
@testable import AppModel
#if os(macOS)
import AppKit
#else
import UIKit
#endif

@MainActor
@Suite struct ReviewNativeFocusTests {
    @Test func detachedContainmentKeepsNativeControlsOutsideCard() {
        #if os(macOS)
        let root = NSView()
        let card = NSView()
        let input = NSView()
        let close = NSButton(title: "Close Review", target: nil, action: nil)
        let back = NSButton(title: "Back", target: nil, action: nil)
        #else
        let root = UIView()
        let card = UIView()
        let input = UITextField()
        let close = UIButton(type: .system)
        let back = UIButton(type: .system)
        #endif
        root.addSubview(card); card.addSubview(input); root.addSubview(close); root.addSubview(back)
        #expect(ReviewInputOwnership.contains(card, in: card))
        #expect(ReviewInputOwnership.contains(input, in: card))
        for control in [close, back] {
            let owned = ReviewInputOwnership.contains(control, in: card)
            #expect(!owned)
            #expect(!ReviewInputOwnership.admits(cardFocused: owned, editing: false, paused: false, repeated: false))
        }
        #expect(!ReviewInputOwnership.cardFocused(in: card))
        #expect(ReviewInputOwnership.initialNativeInputOwned(in: card) == nil)
    }

    @Test func initialFocusWaitsForPresentationAttachmentAndActiveInputThenResolvesOnce() {
        var focus = ReviewInitialFocus()
        #expect(!focus.claim(presented: false, active: true, nativeInputOwned: false))
        #expect(!focus.claim(presented: true, active: true, nativeInputOwned: nil))
        #expect(!focus.claim(presented: true, active: false, nativeInputOwned: false))
        #expect(!focus.resolved)
        #expect(focus.claim(presented: true, active: true, nativeInputOwned: false))
        #expect(focus.resolved)
        // Subsequent cards, resumes and layout attachment cannot reacquire focus.
        #expect(!focus.claim(presented: true, active: true, nativeInputOwned: false))
    }

    @Test func nativeInputOwnerResolvesInitialFocusWithoutStealingIt() {
        var focus = ReviewInitialFocus()
        #expect(!focus.claim(presented: true, active: true, nativeInputOwned: true))
        #expect(focus.resolved)
        #expect(!focus.claim(presented: true, active: true, nativeInputOwned: false))
        #expect(ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: false, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: true, paused: false, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: true, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: false, repeated: true))
    }
}
