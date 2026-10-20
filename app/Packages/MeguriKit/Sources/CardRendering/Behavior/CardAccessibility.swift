import Foundation
#if os(macOS)
import AppKit
#else
import UIKit
#endif

private nonisolated struct AccessibilityDOMTarget: Decodable {
    let id: String
    let text: String
    let x: Double?
    let y: Double?
    let left: Double
    let top: Double
    let width: Double
    let height: Double
}

public nonisolated struct CardAccessibilityTarget: Equatable, Sendable {
    public let update: CardUpdate
    public let elementID: String
    public let text: String
}

/// Offscreen component tests suppress native effects. Actual OS targets and
/// VoiceOver cursor movement require a supported, visible surface in the VM.
struct CardAccessibilityEffects {
    var isActive: (CardNativeWebView) -> Bool = { view in
        #if os(macOS)
        guard NSApp?.isActive == true, NSApp?.isHidden == false, let window = view.window,
              window.isKeyWindow, window.isVisible, !window.isMiniaturized, window.attachedSheet == nil,
              let screen = window.screen, screen.frame.intersects(window.frame) else { return false }
        return !view.isHiddenOrHasHiddenAncestor && !view.visibleRect.isEmpty
        #else
        guard UIApplication.shared.applicationState == .active, let window = view.window,
              window.isKeyWindow, !window.isHidden, window.alpha > 0,
              window.windowScene?.activationState == .foregroundActive,
              window.rootViewController?.presentedViewController == nil else { return false }
        return !view.isHidden && view.alpha > 0 && window.bounds.intersects(view.convert(view.bounds, to: window))
        #endif
    }
    var post: (Any) -> Void = { element in
        #if os(macOS)
        NSAccessibility.post(element: element, notification: .layoutChanged, userInfo: [.uiElements: [element]])
        #else
        UIAccessibility.post(notification: .layoutChanged, argument: element)
        #endif
    }
}

extension CardPage {
    /// Resolve through public native hit testing at the answer's rendered
    /// text, never by replacing WebKit's accessibility tree or keyboard focus.
    /// A nil result is unavailable, not evidence that the web area is #answer.
    func resolveAnswerAccessibilityTarget() async throws -> (CardAccessibilityTarget, Any?)? {
        guard behaviorAvailable, let binding = update, binding.side == .answer else { return nil }
        let geometry = hostReports.viewRevision
        let json = try await string("""
            const anchor = document.getElementById('answer') || document.getElementById('qa');
            if (!anchor) return '';
            let text = (anchor.innerText || anchor.textContent || '').trim();
            let reading = anchor;
            if (!text && anchor.id === 'answer') {
                for (let sibling = anchor.nextElementSibling; sibling; sibling = sibling.nextElementSibling) {
                    const content = (sibling.innerText || sibling.textContent || sibling.getAttribute('alt') || sibling.getAttribute('aria-label') || '').trim();
                    if (content) { reading = sibling; text = content; break; }
                }
            }
            const viewport = window.visualViewport;
            const left = viewport?.offsetLeft ?? 0, top = viewport?.offsetTop ?? 0;
            const width = viewport?.width ?? innerWidth, height = viewport?.height ?? innerHeight;
            const visiblePoint = rect => {
                const x1 = Math.max(rect.left, left), y1 = Math.max(rect.top, top);
                const x2 = Math.min(rect.right, left + width), y2 = Math.min(rect.bottom, top + height);
                return x2 > x1 && y2 > y1 ? {x:(x1 + x2) / 2, y:(y1 + y2) / 2} : null;
            };
            const excluded = element => element.closest('[aria-hidden="true"],script,style') ||
                ['hidden', 'collapse'].includes(getComputedStyle(element).visibility);
            let point = null;
            const walk = document.createTreeWalker(reading, NodeFilter.SHOW_TEXT);
            while (walk.nextNode()) {
                const node = walk.currentNode, offset = node.textContent.search(/\\S/);
                if (offset < 0 || !node.parentElement || excluded(node.parentElement)) continue;
                const readableRange = document.createRange();
                readableRange.setStart(node, offset); readableRange.setEnd(node, node.textContent.trimEnd().length);
                for (const rect of readableRange.getClientRects()) {
                    point = visiblePoint(rect);
                    if (point) break;
                }
                if (point) break;
            }
            if (!point) {
                const images = reading.matches('img') ? [reading] : reading.querySelectorAll('img[alt]');
                for (const image of images) {
                    if (!excluded(image)) point = visiblePoint(image.getBoundingClientRect());
                    if (point) break;
                }
            }
            if (!point && reading.matches('input,textarea,button,select') && !excluded(reading)) {
                point = visiblePoint(reading.getBoundingClientRect());
            }
            return JSON.stringify({id:anchor.id, text:text.slice(0,4000),
                x:point?.x ?? null, y:point?.y ?? null, left, top, width, height});
            """, world: Self.world)
        guard let data = json.data(using: .utf8), let value = try? JSONDecoder().decode(AccessibilityDOMTarget.self, from: data),
              update == binding, behaviorAvailable, hostReports.viewRevision == geometry else { return nil }
        let metadata = CardAccessibilityTarget(update: binding, elementID: value.id, text: value.text)
        guard let x = value.x, let y = value.y, value.width > 0, value.height > 0,
              x >= value.left, y >= value.top, x < value.left + value.width, y < value.top + value.height else { return (metadata, nil) }
        // Visual viewport coordinates include both saved page zoom and pinch;
        // DOM rects already include scrolling. No device-pixel-ratio scaling.
        let horizontal = CGFloat((x - value.left) / value.width) * webView.bounds.width
        let vertical = CGFloat((y - value.top) / value.height) * webView.bounds.height
        #if os(macOS)
        guard let window = webView.window else { return (metadata, nil) }
        let local = CGPoint(x: webView.bounds.minX + horizontal,
                            y: webView.isFlipped ? webView.bounds.minY + vertical : webView.bounds.maxY - vertical)
        let screenPoint = window.convertPoint(toScreen: webView.convert(local, to: nil))
        guard let element = webView.accessibilityHitTest(screenPoint) as? NSObject,
              element !== webView,
              let accessible = element as? any NSAccessibilityProtocol,
              let role = accessible.accessibilityRole(),
              role.rawValue != "AXWebArea", role.rawValue != "AXScrollArea", role.rawValue != "AXGroup" else { return (metadata, nil) }
        #else
        guard webView.window != nil else { return (metadata, nil) }
        let local = CGPoint(x: webView.bounds.minX + horizontal, y: webView.bounds.minY + vertical)
        let screenPoint = UIAccessibility.convertToScreenCoordinates(CGRect(origin: local, size: .zero), in: webView).origin
        guard let element = webView.accessibilityHitTest(screenPoint, event: nil) as? NSObject,
              element !== webView, element.isAccessibilityElement else { return (metadata, nil) }
        #endif
        return (metadata, element)
    }

    func announceAnswer(allowFocus: Bool = true) async {
        guard behaviorAvailable, let binding = update, binding.side == .answer, accessibilityTarget?.update != binding else { return }
        do {
            guard let (metadata, element) = try await resolveAnswerAccessibilityTarget(), update == binding, behaviorAvailable else { return }
            accessibilityTarget = metadata
            onAccessibilityTarget?(metadata)
            guard allowFocus, update == binding, behaviorAvailable, let element,
                  accessibilityEffects.isActive(webView) else { return }
            accessibilityEffects.post(element)
        } catch { diagnostics.record("answerAccessibility", update: binding) }
    }
}
