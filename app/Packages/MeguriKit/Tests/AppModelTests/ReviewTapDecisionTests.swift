import CardRendering
import CoreGraphics
import Foundation
import Testing
@testable import AppModel

/// Review tap zones (U180): the decision for one finished touch, and the
/// pairing of a native gesture with the page's observation of it.
struct ReviewTapDecisionTests {
    static let question = CardUpdate(card: 7, generation: 3, side: .question)
    static let answer = CardUpdate(card: 7, generation: 3, side: .answer)

    static func gesture(x: CGFloat = 300, down: TimeInterval = 10, duration: TimeInterval = 0.08, fingers: Int = 1,
                        movement: CGFloat = 2, interrupted: Bool = false, update: CardUpdate? = answer) -> ReviewTapGesture {
        ReviewTapGesture(downUptime: down, upUptime: down + duration, fingers: fingers, movement: movement,
                         interrupted: interrupted, location: CGPoint(x: x, y: 200), downUpdate: update)
    }
    static func observation(_ update: CardUpdate = answer, exempt: Bool = false, prevented: Bool = false,
                            selection: Bool = false, editable: Bool = false, composing: Bool = false) -> ReviewTapObservation {
        ReviewTapObservation(update: update, exempt: exempt, defaultPrevented: prevented, selection: selection,
                             editable: editable, composing: composing)
    }
    static func state(_ side: ReviewTapState.Side? = .answer, update: CardUpdate? = answer, width: CGFloat = 400,
                      window: TimeInterval = 0.25, lastAnswer: TimeInterval? = nil, answerShown: TimeInterval? = 5,
                      assistive: Bool = false) -> ReviewTapState {
        ReviewTapState(side: side, update: update, visibleX: 0...width, doubleTapPrevention: window,
                       lastAnswerUptime: lastAnswer, answerShownUptime: answerShown, assistiveTechnology: assistive)
    }
    static func decide(_ g: ReviewTapGesture = gesture(), _ o: ReviewTapObservation = observation(),
                       _ s: ReviewTapState = state()) -> ReviewTapAction {
        ReviewTapRules.decide(g, o, s)
    }

    // MARK: Zones (R401, R402)

    @Test func questionSideTapAnywhereReveals() {
        for x in [CGFloat(1), 199, 200, 399] {
            #expect(Self.decide(Self.gesture(x: x, update: Self.question), Self.observation(Self.question),
                                Self.state(.question, update: Self.question)) == .reveal)
        }
    }

    @Test func answerSideLeftHalfIsAgainRightHalfIsGood() {
        #expect(Self.decide(Self.gesture(x: 199)) == .again)
        #expect(Self.decide(Self.gesture(x: 201)) == .good)
        #expect(Self.decide(Self.gesture(x: 0)) == .again)
        #expect(Self.decide(Self.gesture(x: 400)) == .good)
    }

    @Test func theBoundaryIsTheMiddleOfTheVisibleWidth() {
        // A card view inset on the left (safe area): its own middle, not the screen's.
        var state = Self.state()
        state.visibleX = 100...500
        #expect(Self.decide(Self.gesture(x: 299), Self.observation(), state) == .again)
        #expect(Self.decide(Self.gesture(x: 301), Self.observation(), state) == .good)
        // The iPhone Duo held wide: 951 points, the fold in the middle.
        state.visibleX = 0...951
        #expect(Self.decide(Self.gesture(x: 474), Self.observation(), state) == .again)
        #expect(Self.decide(Self.gesture(x: 477), Self.observation(), state) == .good)
    }

    // MARK: Not a tap (R404)

    @Test func scrollsLongPressesSecondFingersAndCancelledTouchesDoNothing() {
        #expect(Self.decide(Self.gesture(movement: 10)) == .none(.notATap))
        #expect(Self.decide(Self.gesture(movement: 9.9)) == .good)
        #expect(Self.decide(Self.gesture(duration: 0.5)) == .none(.notATap))
        #expect(Self.decide(Self.gesture(duration: 0.499)) == .good)
        #expect(Self.decide(Self.gesture(fingers: 2)) == .none(.notATap))
        #expect(Self.decide(Self.gesture(interrupted: true)) == .none(.notATap))
        #expect(Self.decide(Self.gesture(duration: -0.01)) == .none(.notATap))
    }

    // MARK: The card's own content wins (R405, R406, R412)

    @Test func cardControlsCancelledTapsAndSelectionsDoNothing() {
        #expect(Self.decide(Self.gesture(), Self.observation(exempt: true)) == .none(.exempt))
        #expect(Self.decide(Self.gesture(), Self.observation(prevented: true)) == .none(.cancelledByCard))
        #expect(Self.decide(Self.gesture(), Self.observation(selection: true)) == .none(.selection))
        let q = Self.state(.question, update: Self.question)
        #expect(Self.decide(Self.gesture(update: Self.question), Self.observation(Self.question, exempt: true), q) == .none(.exempt))
        #expect(Self.decide(Self.gesture(update: Self.question), Self.observation(Self.question, selection: true), q) == .none(.selection))
    }

    @Test func aTapWhileTypingOnlyPutsTheKeyboardAway() {
        let q = Self.state(.question, update: Self.question)
        #expect(Self.decide(Self.gesture(update: Self.question), Self.observation(Self.question, editable: true), q) == .dismissKeyboard)
        #expect(Self.decide(Self.gesture(update: Self.question), Self.observation(Self.question, composing: true), q) == .dismissKeyboard)
        #expect(Self.decide(Self.gesture(), Self.observation(editable: true)) == .dismissKeyboard)
        // An exempt target (the field itself) keeps its tap.
        #expect(Self.decide(Self.gesture(), Self.observation(exempt: true, editable: true)) == .none(.exempt))
    }

    // MARK: Which card (R415) and when (R416)

    @Test func aTouchThatBeganOnAnotherSideOrCardDoesNothing() {
        // Down on the question, up on the answer.
        #expect(Self.decide(Self.gesture(update: Self.question)) == .none(.otherCard))
        // The page saw it on another card.
        #expect(Self.decide(Self.gesture(), Self.observation(CardUpdate(card: 8, generation: 4, side: .answer))) == .none(.otherCard))
        #expect(Self.decide(Self.gesture(update: nil)) == .none(.otherCard))
        #expect(Self.decide(Self.gesture(), Self.observation(), Self.state(update: nil)) == .none(.otherCard))
    }

    @Test func nothingWhileTheReviewTakesNoInput() {
        #expect(Self.decide(Self.gesture(), Self.observation(), Self.state(nil)) == .none(.inactive))
        // The review's side and the page's must agree.
        #expect(Self.decide(Self.gesture(), Self.observation(), Self.state(.question)) == .none(.inactive))
    }

    @Test func doubleTapPreventionIgnoresARevealTapRightAfterAnAnswer() {
        let q = Self.state(.question, update: Self.question, lastAnswer: 10.0)
        func tap(at down: TimeInterval) -> ReviewTapAction {
            Self.decide(Self.gesture(down: down, update: Self.question), Self.observation(Self.question), q)
        }
        // AE12: a second tap 120 ms after the first does nothing; one 400 ms after reveals.
        #expect(tap(at: 10.12) == .none(.doubleTapPrevention))
        #expect(tap(at: 10.249) == .none(.doubleTapPrevention))
        #expect(tap(at: 10.25) == .reveal)
        #expect(tap(at: 10.4) == .reveal)
        var off = q
        off.doubleTapPrevention = 0
        #expect(Self.decide(Self.gesture(down: 10.05, update: Self.question), Self.observation(Self.question), off) == .reveal)
    }

    @Test func anAnswerTapRightAfterTheAnswerAppearsDoesNothing() {
        let s = Self.state(answerShown: 20)
        #expect(Self.decide(Self.gesture(down: 20.1), Self.observation(), s) == .none(.answerJustShown))
        #expect(Self.decide(Self.gesture(down: 20.3), Self.observation(), s) == .good)
        var off = s
        off.doubleTapPrevention = 0
        #expect(Self.decide(Self.gesture(down: 20.01), Self.observation(), off) == .good)
    }

    // MARK: Accessibility (R184, R1981)

    @Test func withVoiceOverOrSwitchControlTheAnswerZonesAreOff() {
        #expect(Self.decide(Self.gesture(), Self.observation(), Self.state(assistive: true)) == .none(.assistiveTechnology))
        // The question side still reveals: the Show Answer button is there too.
        #expect(Self.decide(Self.gesture(update: Self.question), Self.observation(Self.question),
                            Self.state(.question, update: Self.question, assistive: true)) == .reveal)
    }

    // MARK: Pairing

    @Test func aGestureAndItsObservationPairInEitherOrder() {
        var pairing = ReviewTapPairing()
        #expect(pairing.add(Self.gesture(x: 300), at: 1) == nil)
        let first = pairing.add(Self.observation(), point: CGPoint(x: 305, y: 196), at: 1.02)
        #expect(first?.0 == Self.gesture(x: 300))
        #expect(pairing.add(Self.observation(), point: CGPoint(x: 100, y: 200), at: 2) == nil)
        #expect(pairing.add(Self.gesture(x: 102), at: 2.01)?.1 == Self.observation())
    }

    @Test func aPairNeedsTheSamePlaceAndAShortWait() {
        var pairing = ReviewTapPairing()
        _ = pairing.add(Self.gesture(x: 300), at: 1)
        // The page saw a tap somewhere else: neither acts.
        #expect(pairing.add(Self.observation(), point: CGPoint(x: 330, y: 200), at: 1.01) == nil)
        #expect(pairing.add(Self.observation(), point: CGPoint(x: 300, y: 200), at: 1.02) == nil)
        // Too late.
        var late = ReviewTapPairing()
        _ = late.add(Self.gesture(x: 300), at: 1)
        #expect(late.add(Self.observation(), point: CGPoint(x: 300, y: 200), at: 2.5) == nil)
        // Each part is used once.
        var once = ReviewTapPairing()
        _ = once.add(Self.gesture(x: 300), at: 1)
        #expect(once.add(Self.observation(), point: CGPoint(x: 300, y: 200), at: 1.01) != nil)
        #expect(once.add(Self.observation(), point: CGPoint(x: 300, y: 200), at: 1.02) == nil)
    }
}
