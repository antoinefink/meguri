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

/// A small effect boundary lets offscreen component tests inspect a real
/// WebKit target without posting to the workstation's accessibility service.
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
            let rect = null;
            const walk = document.createTreeWalker(reading, NodeFilter.SHOW_TEXT);
            while (walk.nextNode()) {
                const node = walk.currentNode, offset = node.textContent.search(/\\S/);
                if (offset < 0 || !node.parentElement || node.parentElement.closest('[aria-hidden="true"],script,style')) continue;
                const range = document.createRange();
                range.setStart(node, offset); range.setEnd(node, offset + 1);
                rect = Array.from(range.getClientRects()).find(r => r.width > 0 && r.height > 0);
                if (rect) break;
            }
            if (!rect) {
                const image = reading.matches('img') ? reading : reading.querySelector('img[alt]');
                if (image) rect = image.getBoundingClientRect();
            }
            if (!rect && reading.matches('input,textarea,button,select,[aria-label]')) rect = reading.getBoundingClientRect();
            const viewport = window.visualViewport;
            return JSON.stringify({id:anchor.id, text:text.slice(0,4000),
                x:rect ? rect.left + rect.width / 2 : null, y:rect ? rect.top + rect.height / 2 : null,
                left:viewport?.offsetLeft || 0, top:viewport?.offsetTop || 0,
                width:viewport?.width || innerWidth, height:viewport?.height || innerHeight});
            """, world: Self.world)
        guard let data = json.data(using: .utf8), let value = try? JSONDecoder().decode(AccessibilityDOMTarget.self, from: data),
              update == binding, behaviorAvailable, hostReports.viewRevision == geometry else { return nil }
        let metadata = CardAccessibilityTarget(update: binding, elementID: value.id, text: value.text)
        guard let x = value.x, let y = value.y, value.width > 0, value.height > 0,
              x >= value.left, y >= value.top, x < value.left + value.width, y < value.top + value.height,
              let window = webView.window else { return (metadata, nil) }
        // Visual viewport coordinates include both saved page zoom and pinch;
        // DOM rects already include scrolling. No device-pixel-ratio scaling.
        let horizontal = CGFloat((x - value.left) / value.width) * webView.bounds.width
        let vertical = CGFloat((y - value.top) / value.height) * webView.bounds.height
        #if os(macOS)
        let local = CGPoint(x: webView.bounds.minX + horizontal,
                            y: webView.isFlipped ? webView.bounds.minY + vertical : webView.bounds.maxY - vertical)
        let screenPoint = window.convertPoint(toScreen: webView.convert(local, to: nil))
        guard let element = webView.accessibilityHitTest(screenPoint) as? NSObject,
              element !== webView,
              let role = element.accessibilityAttributeValue(.role) as? String,
              role != "AXWebArea", role != "AXScrollArea", role != "AXGroup" else { return (metadata, nil) }
        #else
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
