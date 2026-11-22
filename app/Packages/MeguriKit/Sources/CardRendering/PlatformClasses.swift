import WebKit
#if os(macOS)
import AppKit
public typealias CardPlatformView = NSView
#else
import UIKit
public typealias CardPlatformView = UIView
#endif

public enum PlatformClasses {
    public static var current: CardRuntime.Platform {
        #if os(macOS)
        .mac
        #else
        UIDevice.current.userInterfaceIdiom == .pad ? .iPad : .iPhone
        #endif
    }
}

/// Native focus exclusion applies to the whole idle WebKit subtree.
open class CardNativeWebView: WKWebView {
    public internal(set) var idle = true
    var reviewOwnsFocus = false
    var onHostGeometryChange: (() -> Void)?
    /// Native owners can supply an inset content rectangle. nil uses the
    /// current WebKit viewport. Both changes invalidate the host epoch.
    public var contentRectangle: CGRect? {
        didSet { if oldValue != contentRectangle { onHostGeometryChange?() } }
    }
    var hostContentRect: CGRect {
        if let contentRectangle { return contentRectangle }
        #if os(macOS)
        var rect = bounds
        // Flipped or not, the covered part is on the window's bottom side.
        if isFlipped { rect.size.height -= min(rect.height, obscuredBottom) }
        else { rect.origin.y += min(rect.height, obscuredBottom); rect.size.height -= min(rect.height, obscuredBottom) }
        return rect
        #else
        // The scroll view's bottom inset is the covered part (below).
        return bounds.inset(by: scrollView.adjustedContentInset)
        #endif
    }
    /// Covered by the review's answer area at the bottom (N16.9): WebKit's
    /// layout viewport ends above it. A change is a geometry change (H5).
    var obscuredBottom: CGFloat = 0 {
        didSet {
            guard oldValue != obscuredBottom else { return }
            var insets = obscuredContentInsets
            insets.bottom = obscuredBottom
            obscuredContentInsets = insets
            #if !os(macOS)
            // WebKit's own insets keep fixed elements clear; the scroll
            // range comes from the scroll view's own inset, as on iOS bars.
            scrollView.contentInset.bottom = obscuredBottom
            scrollView.verticalScrollIndicatorInsets.bottom = obscuredBottom
            #endif
            onHostGeometryChange?()
        }
    }
    open override var pageZoom: CGFloat {
        didSet { if oldValue != pageZoom { onHostGeometryChange?() } }
    }
    #if os(macOS)
    /// The review's card actions (N20.8), shown first in the card's context
    /// menu, above the system's text items; nil leaves WebKit's menu alone.
    public var contextActions: (() -> [NSMenuItem])?
    open override func willOpenMenu(_ menu: NSMenu, with event: NSEvent) {
        super.willOpenMenu(menu, with: event)
        guard let items = contextActions?(), !items.isEmpty else { return }
        // Reloading the card's page is not a review action; the text items stay.
        for item in menu.items where item.identifier?.rawValue == "WKMenuItemIdentifierReload" { menu.removeItem(item) }
        while menu.items.first?.isSeparatorItem == true { menu.removeItem(at: 0) }
        while menu.items.last?.isSeparatorItem == true { menu.removeItem(at: menu.items.count - 1) }
        if !menu.items.isEmpty { menu.insertItem(.separator(), at: 0) }
        for item in items.reversed() { menu.insertItem(item, at: 0) }
    }
    open override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        onHostGeometryChange?()
    }
    open override func setFrameSize(_ newSize: NSSize) {
        let changed = frame.size != newSize
        super.setFrameSize(newSize)
        if changed { onHostGeometryChange?() }
    }
    open override func setBoundsSize(_ newSize: NSSize) {
        let changed = bounds.size != newSize
        super.setBoundsSize(newSize)
        if changed { onHostGeometryChange?() }
    }
    open override func scrollWheel(with event: NSEvent) {
        onHostGeometryChange?()
        super.scrollWheel(with: event)
    }
    #else
    private var lastHostGeometry: [CGFloat] = []
    open override func didMoveToWindow() {
        super.didMoveToWindow()
        onHostGeometryChange?()
    }
    open override var frame: CGRect {
        didSet { if oldValue != frame { onHostGeometryChange?() } }
    }
    open override var bounds: CGRect {
        didSet { if oldValue != bounds { onHostGeometryChange?() } }
    }
    open override func layoutSubviews() {
        super.layoutSubviews()
        checkHostScrollGeometry()
    }
    func checkHostScrollGeometry() {
        let inset = scrollView.adjustedContentInset
        let geometry = [scrollView.contentOffset.x, scrollView.contentOffset.y,
                        scrollView.zoomScale, inset.top, inset.left, inset.bottom, inset.right]
        if geometry != lastHostGeometry {
            lastHostGeometry = geometry
            onHostGeometryChange?()
        }
    }
    #endif
    #if os(macOS)
    open override var isOpaque: Bool { true }
    open override var acceptsFirstResponder: Bool { !idle && reviewOwnsFocus && super.acceptsFirstResponder }
    open override var canBecomeKeyView: Bool { !idle && reviewOwnsFocus && super.canBecomeKeyView }
    #else
    open override var canBecomeFirstResponder: Bool { !idle && reviewOwnsFocus && super.canBecomeFirstResponder }
    #endif

    func setIdle(_ value: Bool) {
        idle = value
        #if os(macOS)
        setAccessibilityHidden(value)
        #else
        accessibilityElementsHidden = value
        #endif
    }

    func applyAppearance(_ value: CardRuntime.Appearance) {
        #if os(macOS)
        let desired: NSAppearance.Name = value == .dark ? .darkAqua : .aqua
        if appearance?.name != desired {
            appearance = NSAppearance(named: desired)
        }
        #else
        let desired: UIUserInterfaceStyle = value == .dark ? .dark : .light
        if overrideUserInterfaceStyle != desired {
            overrideUserInterfaceStyle = desired
        }
        #endif
    }
}
