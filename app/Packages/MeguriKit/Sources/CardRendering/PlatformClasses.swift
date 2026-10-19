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
        appearance = NSAppearance(named: value == .dark ? .darkAqua : .aqua)
        #else
        overrideUserInterfaceStyle = value == .dark ? .dark : .light
        #endif
    }
}
