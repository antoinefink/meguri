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
        return bounds
        #else
        return bounds.inset(by: scrollView.adjustedContentInset)
        #endif
    }
    open override var pageZoom: CGFloat {
        didSet { if oldValue != pageZoom { onHostGeometryChange?() } }
    }
    #if os(macOS)
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
