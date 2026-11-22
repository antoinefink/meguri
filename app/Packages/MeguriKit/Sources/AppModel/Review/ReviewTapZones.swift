#if os(iOS)
import CardRendering
import UIKit
import UIKit.UIGestureRecognizerSubclass

/// Review tap zones on iPhone and iPad (U180). A touch observer on the card
/// view measures each touch without taking it from WebKit; the page's host
/// script reports the same gesture after the card's own handlers; the two
/// meet here and `ReviewTapRules` decides. Nothing here is an accessibility
/// element: the buttons stay the accessible way to answer (R1981).
final class ReviewTapZones {
    /// The review now, without the card view's facts this fills in.
    var state: (() -> ReviewTapState?)?
    var onAction: ((ReviewTapAction) -> Void)?
    private weak var pair: CardPagePair?
    private let observer = ReviewTouchObserver()
    private var pairing = ReviewTapPairing()

    init(pair: CardPagePair) {
        self.pair = pair
        observer.began = { [weak self] in self?.touchBegan() ?? (nil, true) }
        observer.finished = { [weak self] gesture in self?.receive(gesture) }
        pair.container.addGestureRecognizer(observer)
        pair.onHostEvent = { [weak self] page, event in self?.receive(page: page, event: event) }
    }

    func close() {
        observer.view?.removeGestureRecognizer(observer)
        pairing.clear()
        pair?.onHostEvent = nil
    }

    /// The side on screen when the finger goes down, and whether the card is
    /// moving under it: a tap that stops a scroll is not a tap on the card.
    private func touchBegan() -> (CardUpdate?, Bool) {
        guard let pair else { return (nil, true) }
        let scroll = pair.current.webView.scrollView
        return (pair.current.update, scroll.isDragging || scroll.isDecelerating || scroll.isZooming)
    }

    private func receive(_ gesture: ReviewTapGesture) {
        if let (gesture, observation) = pairing.add(gesture, at: ProcessInfo.processInfo.systemUptime) {
            decide(gesture, observation)
        }
    }

    private func receive(page: CardPage, event: CardHostEvent) {
        guard let pair, page === pair.current, case .input(let context, let input) = event,
              case .click(let geometry, let point, let button, _) = input.detail, button == 0,
              context.update == pair.current.update else { return }
        // The page's point, back in the card view's points (H5's mapping, inverted).
        let rect = context.contentRect
        let local = CGPoint(x: rect.minX + point.x * rect.width / geometry.width,
                            y: rect.minY + point.y * rect.height / geometry.height)
        let inCard = page.webView.convert(local, to: pair.container)
        let observation = ReviewTapObservation(update: context.update, exempt: input.state.exempt,
            defaultPrevented: input.state.defaultPrevented, selection: input.state.selection,
            editable: input.state.editable || input.state.typeans, composing: input.state.composing)
        if let (gesture, observation) = pairing.add(observation, point: inCard, at: ProcessInfo.processInfo.systemUptime) {
            decide(gesture, observation)
        }
    }

    private func decide(_ gesture: ReviewTapGesture, _ observation: ReviewTapObservation) {
        guard let pair, var state = state?() else { return }
        state.update = pair.current.update
        state.visibleX = Self.visibleX(of: pair.container)
        state.assistiveTechnology = UIAccessibility.isVoiceOverRunning || UIAccessibility.isSwitchControlRunning
        let action = ReviewTapRules.decide(gesture, observation, state)
        if action == .dismissKeyboard { pair.dismissKeyboard() }
        onAction?(action)
    }

    /// R402: the card view's visible width, inside the safe area.
    static func visibleX(of view: UIView) -> ClosedRange<CGFloat> {
        let visible = view.bounds.intersection(view.safeAreaLayoutGuide.layoutFrame)
        let frame = visible.isNull || visible.width <= 0 ? view.bounds : visible
        return frame.minX...max(frame.minX, frame.maxX)
    }
}

/// Measures touches on the card and never recognizes, delays or cancels
/// them, so WebKit's scrolling, selection, links and pinch zoom are unchanged
/// (R409, R410).
final class ReviewTouchObserver: UIGestureRecognizer, UIGestureRecognizerDelegate {
    var began: (() -> (CardUpdate?, Bool))?
    var finished: ((ReviewTapGesture) -> Void)?
    private var start: (uptime: TimeInterval, point: CGPoint, update: CardUpdate?, moving: Bool)?
    private var down: Set<ObjectIdentifier> = []
    private var fingers = 0
    private var movement: CGFloat = 0

    init() {
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
        // R411: a trackpad or mouse click on iPad is a tap.
        allowedTouchTypes = [UITouch.TouchType.direct, .indirectPointer, .pencil].map { NSNumber(value: $0.rawValue) }
        delegate = self
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        for touch in touches {
            down.insert(ObjectIdentifier(touch))
            if start == nil {
                let (update, moving) = began?() ?? (nil, true)
                start = (touch.timestamp, touch.location(in: view), update, moving)
            }
        }
        fingers = max(fingers, down.count)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let start else { return }
        for touch in touches {
            let point = touch.location(in: view)
            movement = max(movement, hypot(point.x - start.point.x, point.y - start.point.y))
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        end(touches, interrupted: false)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        end(touches, interrupted: true)
    }

    private func end(_ touches: Set<UITouch>, interrupted: Bool) {
        guard let start, let last = touches.first else { return }
        for touch in touches {
            let point = touch.location(in: view)
            movement = max(movement, hypot(point.x - start.point.x, point.y - start.point.y))
            down.remove(ObjectIdentifier(touch))
        }
        if interrupted { self.start?.moving = true }
        guard down.isEmpty else { return }
        let gesture = ReviewTapGesture(downUptime: start.uptime, upUptime: last.timestamp, fingers: fingers,
            movement: movement, interrupted: interrupted || start.moving, location: last.location(in: view),
            downUpdate: start.update)
        state = .failed
        finished?(gesture)
    }

    override func reset() {
        super.reset()
        start = nil; down = []; fingers = 0; movement = 0
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
                           shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
}
#endif
